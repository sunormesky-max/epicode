use std::collections::HashMap;
use std::path::Path;

use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use parking_lot::Mutex;
use tokenizers::Tokenizer;

pub const EMBEDDING_DIM: usize = 768;
const EMBED_PREFIX: &str = "query: ";
const MAX_INPUT_CHARS: usize = 500;

pub struct VectorLayer {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
    cache: Mutex<HashMap<String, Vec<f64>>>,
    cache_order: Mutex<Vec<String>>,
}

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

        let session = Session::builder()
            .map_err(|e| format!("session builder: {}", e))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
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
        if input_names.iter().any(|n| n == "token_type_ids") {
            tracing::info!("VectorLayer: model supports token_type_ids");
        }

        tracing::info!(
            "VectorLayer loaded: {} dims, model={}, prefix=true",
            EMBEDDING_DIM,
            model_path.display()
        );

        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
            cache: Mutex::new(HashMap::new()),
            cache_order: Mutex::new(Vec::new()),
        })
    }

    pub fn embed(&self, text: &str) -> Result<Vec<f64>, String> {
        let prefixed = if text.starts_with("query:") || text.starts_with("passage:") {
            text.to_string()
        } else {
            format!("{}{}", EMBED_PREFIX, text)
        };
        let truncated: String = prefixed.chars().take(MAX_INPUT_CHARS).collect();

        {
            let cache = self.cache.lock();
            if let Some(emb) = cache.get(&truncated) {
                return Ok(emb.clone());
            }
        }

        let encoding = self
            .tokenizer
            .encode(truncated.as_str(), true)
            .map_err(|e| format!("tokenize: {}", e))?;

        let ids = encoding.get_ids();
        let mask = encoding.get_attention_mask();
        let len = ids.len();

        let raw = {
            let mut session = self.session.lock();
            if session.outputs().is_empty() {
                return Err("ONNX model has no outputs".to_string());
            }
            let first_output = session.outputs()[0].name().to_owned();

            let mut inputs = HashMap::from([
                ("input_ids", make_int64_tensor(ids, len)?),
                ("attention_mask", make_int64_tensor(mask, len)?),
            ]);

            let input_names: Vec<String> = session
                .inputs()
                .iter()
                .map(|i| i.name().to_string())
                .collect();
            if input_names.iter().any(|n| n == "token_type_ids") {
                let type_ids = encoding.get_type_ids();
                inputs.insert("token_type_ids", make_int64_tensor(type_ids, len)?);
            }

            let outputs = session.run(inputs).map_err(|e| format!("ort run: {}", e))?;

            let output_tensor = outputs
                .get(&*first_output)
                .ok_or_else(|| format!("no output: {}", first_output))?;

            let (_shape, data) = output_tensor
                .try_extract_tensor::<f32>()
                .map_err(|e| format!("extract tensor: {}", e))?;

            data.to_vec()
        };

        let embedding = mean_pool(&raw, mask, len, EMBEDDING_DIM);
        let normalized = l2_normalize(&embedding);

        {
            let mut cache = self.cache.lock();
            let mut order = self.cache_order.lock();
            cache.insert(truncated.clone(), normalized.clone());
            order.push(truncated);
            while cache.len() > 1000 {
                if order.is_empty() {
                    break;
                }
                let old = order.remove(0);
                cache.remove(&old);
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
        if blob.is_empty() || blob.len() % 8 != 0 {
            return Vec::new();
        }
        blob.chunks_exact(8)
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
            tracing::debug!("skipping embed test: no model.onnx");
            return;
        }
        let layer = VectorLayer::load(model_dir).expect("VectorLayer load");
        let emb = layer
            .embed("quantum tunneling in semiconductors")
            .expect("embed");
        assert_eq!(emb.len(), EMBEDDING_DIM);
        let norm: f64 = emb.iter().map(|x| x * x).sum::<f64>().sqrt();
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
