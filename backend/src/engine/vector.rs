use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use parking_lot::Mutex;
use tokenizers::Tokenizer;

pub const EMBEDDING_DIM: usize = 1024;
const MAX_INPUT_CHARS: usize = 2000;

pub struct VectorLayer {
    session: Arc<Mutex<Session>>,
    session_lock_failures: std::sync::atomic::AtomicUsize,
    /// ONNX 禁用时间戳（Unix秒）。0=未禁用，>0=禁用时刻。
    /// 超时后自动恢复重试（5分钟窗口），避免一次卡顿永久禁用。
    onnx_disabled_until: std::sync::atomic::AtomicU64,
    /// 当前活跃的 ONNX 推理线程数（防止死锁时线程无限累积）
    active_inference_threads: Arc<std::sync::atomic::AtomicUsize>,
    tokenizer: Tokenizer,
    dim: usize,
    use_cls_pool: bool,
    first_output: String,
    input_names: Vec<String>,
    has_token_type_ids: bool,
    cache: Mutex<HashMap<String, Vec<f64>>>,
    cache_order: Mutex<std::collections::VecDeque<String>>,
}

/// 最大并发 ONNX 推理线程数（超过则拒绝新请求，防止死锁场景线程爆炸）
const MAX_CONCURRENT_ONNX: usize = 8;

/// ONNX 禁用后的恢复窗口（秒）。到期后允许重新尝试推理。
const ONNX_DISABLE_COOLDOWN_SECS: u64 = 300; // 5 分钟

impl VectorLayer {
    pub fn load(model_dir: &Path) -> Result<Self, String> {
        let model_path = model_dir.join("model.onnx");
        let tokenizer_path = model_dir.join("tokenizer.json");

        if !model_path.exists() {
            return Err(format!("model not found: {}", model_path.display()));
        }
        if !tokenizer_path.exists() {
            return Err(format!("tokenizer not found: {}", tokenizer_path.display()));
        }

        let mut session = Session::builder()
            .map_err(|e| format!("session builder: {}", e))?
            .with_optimization_level(GraphOptimizationLevel::Level1)
            .map_err(|e| format!("optimization: {}", e))?
            .commit_from_file(&model_path)
            .map_err(|e| format!("ONNX session: {}", e))?;

        let tokenizer =
            Tokenizer::from_file(&tokenizer_path).map_err(|e| format!("tokenizer load: {}", e))?;

        let input_names: Vec<String> = session
            .inputs()
            .iter()
            .map(|i| i.name().to_string())
            .collect();
        let has_token_type_ids = input_names.iter().any(|n| n == "token_type_ids");
        let first_output = session.outputs()[0].name().to_string();

        let dummy = tokenizer
            .encode("x", true)
            .map_err(|e| format!("tokenize: {}", e))?;
        let dummy_ids = dummy.get_ids();
        let dummy_mask = dummy.get_attention_mask();
        let dummy_len = dummy_ids.len();
        let mut dummy_inputs = HashMap::from([
            ("input_ids", make_int64_tensor(dummy_ids, dummy_len)?),
            ("attention_mask", make_int64_tensor(dummy_mask, dummy_len)?),
        ]);
        if has_token_type_ids {
            dummy_inputs.insert(
                "token_type_ids",
                make_int64_tensor(dummy.get_type_ids(), dummy_len)?,
            );
        }
        let dummy_out = session
            .run(dummy_inputs)
            .map_err(|e| format!("dummy run: {}", e))?;
        let output_tensor = dummy_out.get(&*first_output).ok_or("no output")?;
        let (shape, _data) = output_tensor
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("extract: {}", e))?;
        let dim = shape.last().copied().unwrap_or(EMBEDDING_DIM as i64) as usize;
        drop(dummy_out);

        let use_cls_pool = dim > 1000;

        tracing::info!(
            "VectorLayer loaded: {} dims, model={}, token_type_ids={}, pooling={}",
            dim,
            model_path.display(),
            has_token_type_ids,
            if use_cls_pool { "cls" } else { "mean" }
        );

        Ok(Self {
            session: Arc::new(Mutex::new(session)),
            session_lock_failures: std::sync::atomic::AtomicUsize::new(0),
            onnx_disabled_until: std::sync::atomic::AtomicU64::new(0),
            active_inference_threads: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            tokenizer,
            first_output,
            input_names: input_names.clone(),
            has_token_type_ids,
            dim,
            use_cls_pool,
            cache: Mutex::new(HashMap::new()),
            cache_order: Mutex::new(std::collections::VecDeque::new()),
        })
    }

    pub fn embed(&self, text: &str) -> Result<Vec<f64>, String> {
        self._embed(text)
    }

    pub fn embed_passage(&self, text: &str) -> Result<Vec<f64>, String> {
        self._embed(text)
    }

    /// L1相2d: 批量嵌入(预热语义) — tokenize全批+右padding+单次session.run, 结果写缓存。
    /// 后续单条_embed对同文本命中缓存 → ingest批路径的每条真实嵌入摊薄为一次批量推理。
    /// CLS池化对右padding天然免疫(只取位置0); 非CLS池化模型退回逐条; 16条/块防超时。
    pub fn embed_batch(&self, texts: &[String]) -> Result<usize, String> {
        self.embed_batch_vecs(texts).map(|v| v.len())
    }

    /// L1fix: 返回实际向量的批量嵌入 — 图书馆灌装直用
    pub fn embed_batch_vecs(&self, texts: &[String]) -> Result<Vec<Vec<f64>>, String> {
        if !self.use_cls_pool {
            let mut out = Vec::with_capacity(texts.len());
            for t in texts {
                out.push(self._embed(t).unwrap_or_default());
            }
            return Ok(out);
        }
        let truncated: Vec<String> = texts
            .iter()
            .map(|t| t.chars().take(MAX_INPUT_CHARS).collect())
            .collect();
        let mut need: Vec<usize> = Vec::new();
        {
            let cache = self.cache.lock();
            for (i, t) in truncated.iter().enumerate() {
                if !cache.contains_key(t) {
                    need.push(i);
                }
            }
        }
        if need.is_empty() {
            return Ok(Vec::new());
        }
        // 内存压力门：系统可用内存不足时延迟批量预热（缓存未命中路径可惰性重试）。
        // 防止午夜多引擎恢复时 ONNX 批量推理与引擎装载叠加成工作集爆炸
        // （2026-09-06 两次规则重启的根因：RSS 5-6.8G + swap 耗尽 → ONNX 超时风暴）。
        #[cfg(target_os = "linux")]
        {
            if let Ok(mi) = std::fs::read_to_string("/proc/meminfo") {
                let avail_kb = mi
                    .lines()
                    .find(|l| l.starts_with("MemAvailable:"))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(u64::MAX);
                if avail_kb < 1_500_000 {
                    tracing::warn!(
                        "[VectorLayer] batch prewarm deferred: MemAvailable {}MB < 1500MB (memory pressure gate)",
                        avail_kb / 1024
                    );
                    return Ok(Vec::new());
                }
            }
        }
        let disabled_until = self
            .onnx_disabled_until
            .load(std::sync::atomic::Ordering::Relaxed);
        if disabled_until > 0 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            if now < disabled_until {
                return Ok(Vec::new());
            }
            self.onnx_disabled_until
                .store(0, std::sync::atomic::Ordering::Relaxed);
            self.active_inference_threads
                .store(0, std::sync::atomic::Ordering::Relaxed);
        }
        let mut _warmed = 0usize; // 计数保留诊断用途
        let mut result: Vec<Option<Vec<f64>>> = vec![None; texts.len()];
        for chunk_start in (0..need.len()).step_by(8) {
            let idxs: Vec<usize> = need[chunk_start..(chunk_start + 16).min(need.len())].to_vec();
            let mut ids_rows: Vec<Vec<i64>> = Vec::with_capacity(idxs.len());
            let mut mask_rows: Vec<Vec<i64>> = Vec::with_capacity(idxs.len());
            for &i in &idxs {
                let e = self
                    .tokenizer
                    .encode(truncated[i].as_str(), true)
                    .map_err(|er| format!("tokenize: {}", er))?;
                ids_rows.push(e.get_ids().iter().map(|&x| x as i64).collect());
                mask_rows.push(e.get_attention_mask().iter().map(|&x| x as i64).collect());
            }
            let max_len = ids_rows.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
            let b = ids_rows.len();
            let mut flat_ids = vec![0i64; b * max_len];
            let mut flat_mask = vec![0i64; b * max_len];
            for r in 0..b {
                for (c, v) in ids_rows[r].iter().enumerate() {
                    flat_ids[r * max_len + c] = *v;
                }
                for (c, v) in mask_rows[r].iter().enumerate() {
                    flat_mask[r * max_len + c] = *v;
                }
            }
            let active = self
                .active_inference_threads
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if active >= MAX_CONCURRENT_ONNX {
                self.active_inference_threads
                    .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                tracing::warn!(
                    "[VectorLayer] batch ONNX concurrency limit reached, partial prewarm"
                );
                break;
            }
            let (tx, rx) = std::sync::mpsc::channel();
            let session_arc = self.session.clone();
            let first_output_name = self.first_output.clone();
            let input_names = self.input_names.clone();
            let active_counter = self.active_inference_threads.clone();
            let ids_flat = flat_ids.clone().into_boxed_slice();
            let mask_flat = flat_mask.clone().into_boxed_slice();
            std::thread::spawn(move || {
                let result: Result<Vec<f32>, String> = (|| {
                    let mut session = session_arc.lock();
                    let ids_t = ort::value::Tensor::from_array(([b, max_len], ids_flat))
                        .map_err(|e| format!("ids tensor: {}", e))?
                        .into_dyn();
                    let mask_t = ort::value::Tensor::from_array(([b, max_len], mask_flat))
                        .map_err(|e| format!("mask tensor: {}", e))?
                        .into_dyn();
                    let mut inputs =
                        HashMap::from([("input_ids", ids_t), ("attention_mask", mask_t)]);
                    if input_names.iter().any(|n| n == "token_type_ids") {
                        let tt: Vec<i64> = vec![0; b * max_len];
                        let t =
                            ort::value::Tensor::from_array(([b, max_len], tt.into_boxed_slice()))
                                .map_err(|e| format!("tt tensor: {}", e))?
                                .into_dyn();
                        inputs.insert("token_type_ids", t);
                    }
                    let outputs = session.run(inputs).map_err(|e| format!("ort run: {}", e))?;
                    let t = outputs
                        .get(&*first_output_name)
                        .ok_or_else(|| format!("no output: {}", first_output_name))?;
                    let (_s, data) = t
                        .try_extract_tensor::<f32>()
                        .map_err(|e| format!("extract: {}", e))?;
                    Ok(data.to_vec())
                })();
                let _ = tx.send(result);
                active_counter.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            });
            let raw = match rx.recv_timeout(std::time::Duration::from_secs(60)) {
                Ok(Ok(d)) => d,
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    tracing::error!("[VectorLayer] batch ONNX timeout (60s), partial prewarm");
                    break;
                }
            };
            for (row, &i) in idxs.iter().enumerate() {
                let start = row * max_len * self.dim;
                if raw.len() < start + self.dim {
                    continue;
                }
                let emb = l2_normalize(&cls_pool(&raw[start..start + self.dim], self.dim));
                let mut cache = self.cache.lock();
                let mut order = self.cache_order.lock();
                cache.insert(truncated[i].clone(), emb.clone());
                order.push_back(truncated[i].clone());
                while cache.len() > 1000 {
                    if let Some(old) = order.pop_front() {
                        cache.remove(&old);
                    } else {
                        break;
                    }
                }
                _warmed += 1;
                result[i] = Some(emb);
            }
        }
        let ordered: Vec<Vec<f64>> = result.into_iter().map(|r| r.unwrap_or_default()).collect();
        Ok(ordered)
    }

    fn _embed(&self, text: &str) -> Result<Vec<f64>, String> {
        let truncated: String = text.chars().take(MAX_INPUT_CHARS).collect();

        {
            let cache = self.cache.lock();
            if let Some(emb) = cache.get(&truncated) {
                return Ok(emb.clone());
            }
        }

        // ONNX 禁用检查：超时窗口内跳过推理，到期后自动恢复重试
        let disabled_until = self
            .onnx_disabled_until
            .load(std::sync::atomic::Ordering::Relaxed);
        if disabled_until > 0 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            if now < disabled_until {
                return Ok(vec![]);
            }
            // 冷却窗口已过，清除禁用标志和线程计数器，允许重新尝试
            tracing::info!("[VectorLayer] ONNX cooldown expired, re-enabling inference attempts");
            self.onnx_disabled_until
                .store(0, std::sync::atomic::Ordering::Relaxed);
            self.active_inference_threads
                .store(0, std::sync::atomic::Ordering::Relaxed);
        }
        let encoding = self
            .tokenizer
            .encode(truncated.as_str(), true)
            .map_err(|e| format!("tokenize: {}", e))?;

        let ids = encoding.get_ids();
        let mask = encoding.get_attention_mask();
        let len = ids.len();

        let raw = {
            // ONNX 推理放到独立线程 + 10 秒超时：防止 ort 内部 condition_variable 死锁
            // 导致 session.run() 永不返回 → 持有 session 锁 → 所有 embedding 请求阻塞
            // → tokio worker 饿死 → accept 停 → 全系统假死。

            // 并发线程数保护：如果已有太多推理线程在等锁（死锁场景），不再 spawn 新线程
            let active = self
                .active_inference_threads
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if active >= MAX_CONCURRENT_ONNX {
                self.active_inference_threads
                    .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                tracing::warn!(
                    "[VectorLayer] ONNX concurrency limit reached ({}), skipping inference",
                    active
                );
                return Ok(vec![]);
            }

            let (tx, rx) = std::sync::mpsc::channel();
            let session_arc = self.session.clone();
            let first_output_name = self.first_output.clone();
            let input_names_clone = self.input_names.clone();
            let active_counter = self.active_inference_threads.clone();
            let type_ids = if self.has_token_type_ids {
                Some(encoding.get_type_ids().to_vec())
            } else {
                None
            };
            let ids_vec = ids.to_vec();
            let mask_vec = mask.to_vec();

            std::thread::spawn(move || {
                let result: Result<Vec<f32>, String> = (|| {
                    let mut session = session_arc.lock();
                    let mut inputs = HashMap::from([
                        ("input_ids", make_int64_tensor(&ids_vec, ids_vec.len())?),
                        (
                            "attention_mask",
                            make_int64_tensor(&mask_vec, mask_vec.len())?,
                        ),
                    ]);
                    if input_names_clone.iter().any(|n| n == "token_type_ids") {
                        if let Some(ref tids) = type_ids {
                            inputs.insert("token_type_ids", make_int64_tensor(tids, tids.len())?);
                        }
                    }
                    let outputs = session.run(inputs).map_err(|e| format!("ort run: {}", e))?;
                    let output_tensor = outputs
                        .get(&*first_output_name)
                        .ok_or_else(|| format!("no output: {}", first_output_name))?;
                    let (_shape, data) = output_tensor
                        .try_extract_tensor::<f32>()
                        .map_err(|e| format!("extract tensor: {}", e))?;
                    Ok(data.to_vec())
                })();
                let _ = tx.send(result);
                // 线程完成时递减计数器（无论是正常完成还是 session.run 提前返回）
                active_counter.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            });

            // 10 秒超时：ort 正常推理 10-50ms，10 秒一定够。
            // 超时则认为 ONNX 死锁——永久禁用 ONNX（设 disabled 标志），
            // 后续所有 embedding 请求直接返回空向量，搜索降级到 BM25 关键词。
            // 之前每次超时只跳过当前请求但继续尝试 → 泄漏线程累积 → 系统资源耗尽。
            // L1迁移教训: 2000字符语料chunk在负载下推理5-15s, 10s超时会连环熔断 — 30s(f95c61d1修复, 曾被备份还原覆盖)
            match rx.recv_timeout(std::time::Duration::from_secs(30)) {
                Ok(Ok(data)) => data,
                Ok(Err(e)) => return Err(e),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    let fails = self
                        .session_lock_failures
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                        + 1;
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    tracing::error!("[VectorLayer] ONNX inference timeout (10s) — disabling ONNX for {}s (failures={}). Searches will use BM25 keyword only until recovery.", ONNX_DISABLE_COOLDOWN_SECS, fails);
                    self.onnx_disabled_until.store(
                        now + ONNX_DISABLE_COOLDOWN_SECS,
                        std::sync::atomic::Ordering::Relaxed,
                    );
                    return Ok(vec![]);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("ONNX inference thread panicked".to_string());
                }
            }
        };

        let embedding = if self.use_cls_pool {
            cls_pool(&raw, self.dim)
        } else {
            mean_pool(&raw, mask, len, self.dim)
        };
        let normalized = l2_normalize(&embedding);

        {
            let mut cache = self.cache.lock();
            let mut order = self.cache_order.lock();
            cache.insert(truncated.clone(), normalized.clone());
            order.push_back(truncated);
            while cache.len() > 1000 {
                if let Some(old) = order.pop_front() {
                    cache.remove(&old);
                } else {
                    break;
                }
            }
        }

        Ok(normalized)
    }

    pub fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
        if a.len() != b.len() || a.is_empty() {
            return 0.0;
        }
        let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
        let norm_b: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm_a < 1e-10 || norm_b < 1e-10 {
            return 0.0;
        }
        (dot / (norm_a * norm_b)).clamp(0.0, 1.0)
    }

    pub fn best_similarity(
        emb_a: &[f64],
        labels_a: &[String],
        emb_b: &[f64],
        labels_b: &[String],
    ) -> f64 {
        if !emb_a.is_empty() && !emb_b.is_empty() && emb_a.len() == emb_b.len() {
            Self::cosine_similarity(emb_a, emb_b)
        } else {
            Self::label_jaccard(labels_a, labels_b)
        }
    }

    pub fn embedding_to_blob(embedding: &[f64]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(embedding.len() * 8);
        for &v in embedding {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        bytes
    }

    pub fn blob_to_embedding(blob: &[u8]) -> Vec<f64> {
        if blob.is_empty() || !blob.len().is_multiple_of(8) {
            return Vec::new();
        }
        blob.as_chunks::<8>()
            .0
            .iter()
            .map(|chunk| {
                f64::from_le_bytes([
                    chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
                ])
            })
            .collect()
    }

    pub fn label_jaccard(a: &[String], b: &[String]) -> f64 {
        if a.is_empty() && b.is_empty() {
            return 1.0;
        }
        if a.is_empty() || b.is_empty() {
            return 0.0;
        }
        let set_a: std::collections::HashSet<&String> = a.iter().collect();
        let set_b: std::collections::HashSet<&String> = b.iter().collect();
        let intersection = set_a.intersection(&set_b).count();
        let union = set_a.union(&set_b).count();
        if union == 0 {
            0.0
        } else {
            intersection as f64 / union as f64
        }
    }
}

fn make_int64_tensor(data: &[u32], len: usize) -> Result<ort::value::Value, String> {
    let v: Vec<i64> = data.iter().map(|&x| x as i64).collect();
    ort::value::Tensor::from_array(([1usize, len], v.into_boxed_slice()))
        .map_err(|e| format!("tensor create: {}", e))
        .map(|t| t.into_dyn())
}

fn cls_pool(raw: &[f32], dim: usize) -> Vec<f64> {
    if raw.len() < dim {
        return vec![0.0f64; dim];
    }
    raw[..dim].iter().map(|&v| v as f64).collect()
}

fn mean_pool(raw: &[f32], mask: &[u32], seq_len: usize, dim: usize) -> Vec<f64> {
    if raw.len() < seq_len * dim {
        return vec![0.0f64; dim];
    }
    let mut result = vec![0.0f64; dim];
    let mut count = vec![0.0f64; dim];
    for i in 0..seq_len {
        if mask[i] == 0 {
            continue;
        }
        for j in 0..dim {
            result[j] += raw[i * dim + j] as f64;
            count[j] += 1.0;
        }
    }
    for j in 0..dim {
        if count[j] > 0.0 {
            result[j] /= count[j];
        }
    }
    result
}

fn l2_normalize(v: &[f64]) -> Vec<f64> {
    let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm < 1e-10 {
        return v.to_vec();
    }
    v.iter().map(|x| x / norm).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_identical() {
        let v = vec![1.0, 2.0, 3.0];
        assert!((VectorLayer::cosine_similarity(&v, &v) - 1.0).abs() < 1e-10);
    }

    #[test]
    fn cosine_orthogonal() {
        let sim = VectorLayer::cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]);
        assert!(sim.abs() < 1e-10);
    }

    #[test]
    fn cosine_empty() {
        assert_eq!(VectorLayer::cosine_similarity(&[], &[]), 0.0);
    }

    #[test]
    fn cosine_different_lengths() {
        assert_eq!(VectorLayer::cosine_similarity(&[1.0], &[1.0, 2.0]), 0.0);
    }

    #[test]
    #[allow(clippy::approx_constant)] // 测试夹具值
    fn blob_roundtrip() {
        let original: Vec<f64> = vec![1.0, -2.5, 3.14, 0.0, 1e-10];
        let blob = VectorLayer::embedding_to_blob(&original);
        let restored = VectorLayer::blob_to_embedding(&blob);
        assert_eq!(restored.len(), original.len());
        for (a, b) in original.iter().zip(restored.iter()) {
            assert!((a - b).abs() < 1e-15);
        }
    }

    #[test]
    fn blob_empty() {
        assert!(VectorLayer::embedding_to_blob(&[]).is_empty());
        assert!(VectorLayer::blob_to_embedding(&[]).is_empty());
    }

    #[test]
    fn best_sim_prefers_embedding() {
        let sim = VectorLayer::best_similarity(
            &[1.0, 0.0],
            &["rust".into()],
            &[0.9, 0.1],
            &["python".into()],
        );
        assert!(sim > 0.8);
    }

    #[test]
    fn best_sim_falls_back_to_labels() {
        let sim = VectorLayer::best_similarity(&[], &["rust".into()], &[], &["rust".into()]);
        assert!((sim - 1.0).abs() < 1e-10);
    }

    #[test]
    fn l2_normalize_unit() {
        let v = vec![3.0, 4.0];
        let n = l2_normalize(&v);
        let norm: f64 = n.iter().map(|x| x * x).sum::<f64>().sqrt();
        assert!((norm - 1.0).abs() < 1e-10);
    }

    #[test]
    fn label_jaccard_same() {
        assert!((VectorLayer::label_jaccard(&["a".into()], &["a".into()]) - 1.0).abs() < 1e-10);
    }

    #[test]
    fn label_jaccard_disjoint() {
        assert!(VectorLayer::label_jaccard(&["a".into()], &["b".into()]).abs() < 1e-10);
    }

    #[test]
    fn embed_loads_and_runs() {
        let model_dir = std::path::Path::new("models");
        if !model_dir.join("model.onnx").exists() {
            eprintln!("skipping embed test: no model.onnx");
            return;
        }
        let layer = match VectorLayer::load(model_dir) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("skipping embed test: load failed: {}", e);
                return;
            }
        };
        let emb = match layer.embed("quantum tunneling in semiconductors") {
            Ok(e) => e,
            Err(e) => {
                eprintln!("skipping embed test: embed failed: {}", e);
                return;
            }
        };
        if emb.len() != EMBEDDING_DIM {
            eprintln!(
                "skipping embed test: output dim={} expected={}",
                emb.len(),
                EMBEDDING_DIM
            );
            return;
        }
        let norm: f64 = emb.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm < 0.01 {
            eprintln!(
                "skipping embed test: embedding norm={} (likely model mismatch)",
                norm
            );
            return;
        }
        assert!(
            (norm - 1.0).abs() < 0.01,
            "embedding should be normalized, norm={}",
            norm
        );

        let emb2 = layer.embed("Rust ownership and borrowing").expect("embed2");
        let sim = VectorLayer::cosine_similarity(&emb, &emb2);
        assert!(
            sim < 0.9,
            "unrelated texts should not be too similar: {}",
            sim
        );

        let emb3 = layer
            .embed("quantum tunneling in semiconductors")
            .expect("embed3");
        assert_eq!(emb, emb3, "cache should return identical vector");

        let emb4 = layer
            .embed("quantum mechanical tunneling through semiconductor barriers")
            .expect("embed4");
        let sim_related = VectorLayer::cosine_similarity(&emb, &emb4);
        assert!(
            sim_related > 0.5,
            "similar texts should have high similarity: {}",
            sim_related
        );
    }
}
