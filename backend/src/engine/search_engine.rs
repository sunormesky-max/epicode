use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use crate::domain::space::Space;
use crate::domain::tetra::{MemoryPayload, TetraId};
use crate::engine::vector::VectorLayer;

use super::cognitive::CognitiveEngine;
use super::embedding::EmbeddingService;
use super::hnsw::HnswIndex;
use super::knowledge::KnowledgeGraph;

pub struct SearchEngineState {
    pub hnsw: Mutex<HnswIndex>,
    pub search_total: AtomicU64,
    pub search_hits: AtomicU64,
    pub search_miss_queries: Mutex<Vec<String>>,
    pub search_top_labels: Mutex<HashMap<String, u32>>,
    pub access_counts: Mutex<HashMap<TetraId, u32>>,
    df_cache: Mutex<Option<DfCache>>,
}

struct DfCache {
    df_map: HashMap<String, usize>,
    doc_count: usize,
    avg_dl: f64,
    tetra_count: usize,
}

impl SearchEngineState {
    pub fn new(hnsw: HnswIndex) -> Self {
        Self {
            hnsw: Mutex::new(hnsw),
            search_total: AtomicU64::new(0),
            search_hits: AtomicU64::new(0),
            search_miss_queries: Mutex::new(Vec::new()),
            search_top_labels: Mutex::new(HashMap::new()),
            access_counts: Mutex::new(HashMap::new()),
            df_cache: Mutex::new(None),
        }
    }

    pub fn invalidate_df_cache(&self) {
        *self.df_cache.lock() = None;
    }

    fn get_or_build_df(
        &self,
        tetras: &[crate::domain::tetra::Tetrahedron],
    ) -> (HashMap<String, usize>, usize, f64) {
        let mut cache = self.df_cache.lock();
        if let Some(ref c) = *cache {
            if c.tetra_count == tetras.len() {
                return (c.df_map.clone(), c.doc_count, c.avg_dl);
            }
        }
        let (df_map, doc_count, avg_dl) = build_df_map(tetras);
        *cache = Some(DfCache {
            df_map: df_map.clone(),
            doc_count,
            avg_dl,
            tetra_count: tetras.len(),
        });
        (df_map, doc_count, avg_dl)
    }
}

pub struct SearchCtx<'a> {
    pub state: &'a SearchEngineState,
    pub space: &'a Space,
    pub knowledge: &'a KnowledgeGraph,
    pub cognitive: &'a CognitiveEngine,
    pub embedding: &'a EmbeddingService,
    pub label_index: &'a Mutex<HashMap<String, Vec<TetraId>>>,
}

#[derive(Debug, Default)]
pub struct SearchFilters {
    pub labels: Option<Vec<String>>,
    pub min_importance: Option<f64>,
    pub max_importance: Option<f64>,
    pub since_ts: Option<i64>,
    pub until_ts: Option<i64>,
    pub project: Option<String>,
}

fn is_cjk(ch: char) -> bool {
    let cp = ch as u32;
    (cp >= 0x4E00 && cp <= 0x9FFF)
        || (cp >= 0x3400 && cp <= 0x4DBF)
        || (cp >= 0x3000 && cp <= 0x303F)
        || (cp >= 0x3040 && cp <= 0x309F)
        || (cp >= 0x30A0 && cp <= 0x30FF)
        || (cp >= 0xAC00 && cp <= 0xD7AF)
}

pub fn tokenize(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut tokens = Vec::new();
    let mut word_buf = String::new();
    let mut cjk_buf = String::new();

    for ch in lower.chars() {
        if ch.is_whitespace() {
            if !word_buf.is_empty() {
                tokens.push(word_buf.clone());
                word_buf.clear();
            }
            if !cjk_buf.is_empty() {
                flush_cjk(&cjk_buf, &mut tokens);
                cjk_buf.clear();
            }
        } else if is_cjk(ch) {
            if !word_buf.is_empty() {
                tokens.push(word_buf.clone());
                word_buf.clear();
            }
            cjk_buf.push(ch);
        } else if ch.is_alphanumeric()
            || ch == '-'
            || ch == '_'
            || ch == '.'
            || ch == '/'
            || ch == ':'
        {
            if !cjk_buf.is_empty() {
                flush_cjk(&cjk_buf, &mut tokens);
                cjk_buf.clear();
            }
            word_buf.push(ch);
        } else {
            if !word_buf.is_empty() {
                tokens.push(word_buf.clone());
                word_buf.clear();
            }
            if !cjk_buf.is_empty() {
                flush_cjk(&cjk_buf, &mut tokens);
                cjk_buf.clear();
            }
        }
    }
    if !word_buf.is_empty() {
        tokens.push(word_buf);
    }
    if !cjk_buf.is_empty() {
        flush_cjk(&cjk_buf, &mut tokens);
    }

    tokens
}

fn flush_cjk(buf: &str, tokens: &mut Vec<String>) {
    let chars: Vec<char> = buf.chars().collect();
    if chars.is_empty() {
        return;
    }
    if chars.len() == 1 {
        tokens.push(chars[0].to_string());
        return;
    }
    for w in chars.windows(2) {
        tokens.push(format!("{}{}", w[0], w[1]));
    }
    if chars.len() <= 3 {
        tokens.push(buf.to_string());
    }
}

pub fn search(
    ctx: &SearchCtx,
    query: &str,
    k: usize,
    vector: Option<&crate::engine::vector::VectorLayer>,
    filters: Option<&SearchFilters>,
) -> Result<Vec<(TetraId, f64, f64, MemoryPayload)>, String> {
    let search_query = if ctx.cognitive.enabled() {
        match ctx.cognitive.translate_and_expand(query) {
            Ok((en, _translated)) => en,
            Err(e) => {
                tracing::debug!(
                    "[Gateway] translate_and_expand failed: {}, using raw query",
                    e
                );
                query.to_string()
            }
        }
    } else {
        query.to_string()
    };
    let query_embedding = compute_query_embedding(&search_query, vector, ctx.embedding);

    let hnsw_candidates = if let Some(ref qe) = query_embedding {
        let hnsw = ctx.state.hnsw.lock();
        let candidate_k = (k * 3).max(20);
        hnsw.search_knn(qe, candidate_k, 50)
    } else {
        vec![]
    };

    let all_tetras = ctx.space.all_tetrahedrons();
    let (df_map, doc_count, avg_dl) = ctx.state.get_or_build_df(&all_tetras);
    let df_map_ref = &df_map;

    let access_counts = ctx.state.access_counts.lock();
    let mut scored: Vec<(TetraId, f64, f64, MemoryPayload)> = if !hnsw_candidates.is_empty() {
        let candidate_set: HashSet<u64> = hnsw_candidates.iter().map(|(id, _)| *id).collect();
        all_tetras
            .into_iter()
            .filter(|t| candidate_set.contains(&t.id))
            .filter(|t| passes_filters(t, filters))
            .map(|t| {
                score_tetra(
                    &t,
                    &query_embedding,
                    &search_query,
                    avg_dl,
                    doc_count,
                    df_map_ref,
                    false,
                    &access_counts,
                )
            })
            .collect()
    } else {
        let label_idx = ctx.label_index.lock();
        let query_tokens = tokenize(&search_query);
        let mut candidate_ids: HashSet<u64> = HashSet::new();
        for tok in &query_tokens {
            if let Some(ids) = label_idx.get(tok) {
                for &id in ids {
                    candidate_ids.insert(id);
                }
            }
        }
        for (label, ids) in label_idx.iter() {
            let label_lower = label.to_lowercase();
            if query_tokens
                .iter()
                .any(|w| label_lower.contains(w.as_str()))
            {
                for &id in ids {
                    candidate_ids.insert(id);
                }
            }
        }
        drop(label_idx);

        if candidate_ids.is_empty() {
            let cap = 2000;
            candidate_ids = all_tetras.iter().take(cap).map(|t| t.id).collect();
            tracing::warn!(
                "[Search] no label candidates, using first {} of {} tetras as fallback",
                cap,
                all_tetras.len()
            );
        }
        all_tetras
            .into_iter()
            .filter(|t| candidate_ids.contains(&t.id))
            .filter(|t| passes_filters(t, filters))
            .map(|t| {
                score_tetra(
                    &t,
                    &query_embedding,
                    &search_query,
                    avg_dl,
                    doc_count,
                    df_map_ref,
                    true,
                    &access_counts,
                )
            })
            .collect()
    };
    drop(access_counts);

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let best_sim = scored.first().map(|s| s.1).unwrap_or(0.0);
    let need_rerank = best_sim < 0.35 && ctx.cognitive.enabled() && scored.len() > 1;
    if need_rerank {
        let rerank_n = scored.len().min(10);
        let cand_text: Vec<String> = scored
            .iter()
            .take(rerank_n)
            .enumerate()
            .map(|(i, (_id, sim, _mass, payload))| {
                let preview: String = payload.content.chars().take(120).collect();
                let labels = payload.labels.join(",");
                let alias_str = if payload.aliases.is_empty() {
                    String::new()
                } else {
                    let a: Vec<String> = payload.aliases.iter().take(3).cloned().collect();
                    format!(" [{}]", a.join("; "))
                };
                format!(
                    "[{}] [{}] sim={:.3}{} {}",
                    i, labels, sim, alias_str, preview
                )
            })
            .collect();
        let cand_joined = cand_text.join("\n");
        match ctx.cognitive.rerank(query, &cand_joined) {
            Ok(ranked_indices) => {
                let mut reranked: Vec<(TetraId, f64, f64, MemoryPayload)> = Vec::new();
                for idx in ranked_indices {
                    if (idx as usize) < rerank_n {
                        reranked.push(scored[idx as usize].clone());
                    }
                }
                for item in scored.into_iter().skip(rerank_n) {
                    reranked.push(item);
                }
                scored = reranked;
            }
            Err(e) => {
                tracing::debug!("[Gateway] LLM rerank failed: {}", e);
            }
        }
    }

    scored.truncate(k);

    ctx.state.search_total.fetch_add(1, AtomicOrdering::Relaxed);
    if let Some((id, sim, _, payload)) = scored.first() {
        if *sim > 0.25 {
            ctx.state.search_hits.fetch_add(1, AtomicOrdering::Relaxed);
            for label in &payload.labels {
                *ctx.state
                    .search_top_labels
                    .lock()
                    .entry(label.clone())
                    .or_insert(0) += 1;
            }
            *ctx.state.access_counts.lock().entry(*id).or_insert(0) += 1;
        }
    }
    if scored.is_empty() || scored.first().map(|s| s.1).unwrap_or(0.0) < 0.15 {
        let mut miss = ctx.state.search_miss_queries.lock();
        miss.push(query.to_string());
        if miss.len() > 50 {
            miss.remove(0);
        }
    }

    Ok(scored)
}

fn passes_filters(t: &crate::domain::tetra::Tetrahedron, filters: Option<&SearchFilters>) -> bool {
    let Some(f) = filters else { return true };
    if let Some(ref labels) = f.labels {
        if !labels.is_empty() {
            let has_any = labels.iter().any(|l| t.data.labels.contains(l));
            if !has_any {
                return false;
            }
        }
    }
    if let Some(min) = f.min_importance {
        if t.data.importance < min {
            return false;
        }
    }
    if let Some(max) = f.max_importance {
        if t.data.importance > max {
            return false;
        }
    }
    if let Some(since) = f.since_ts {
        if t.data.timestamp < since {
            return false;
        }
    }
    if let Some(until) = f.until_ts {
        if t.data.timestamp > until {
            return false;
        }
    }
    if let Some(ref project) = f.project {
        let has_project = t
            .data
            .labels
            .iter()
            .any(|l| l == project || l.starts_with(&format!("project:{}", project)));
        if !has_project {
            return false;
        }
    }
    true
}

fn compute_query_embedding(
    text: &str,
    vector: Option<&crate::engine::vector::VectorLayer>,
    embedding: &EmbeddingService,
) -> Option<Vec<f64>> {
    if let Some(vl) = vector {
        match vl.embed(text) {
            Ok(emb) => return Some(emb),
            Err(e) => tracing::warn!("[Gateway] ONNX query embed failed: {}", e),
        }
    }
    if embedding.enabled() {
        match embedding.embed(text) {
            Ok(emb) => return Some(emb),
            Err(e) => tracing::warn!("[Gateway] HTTP query embed failed: {}", e),
        }
    }
    None
}

fn hash_string(s: &str) -> u64 {
    let mut h: u64 = 14695981039346656037;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

pub fn hash_content(s: &str) -> u64 {
    hash_string(s)
}

fn keyword_score(query: &str, payload: &MemoryPayload) -> f64 {
    let query_tokens = tokenize(query);
    if query_tokens.is_empty() {
        return 0.0;
    }
    let content_lower = payload.content.to_lowercase();
    let alias_text = payload.aliases.join(" ").to_lowercase();
    let searchable = format!("{} {}", content_lower, alias_text);
    let matched = query_tokens
        .iter()
        .filter(|w| searchable.contains(w.as_str()))
        .count();
    matched as f64 / query_tokens.len() as f64
}

fn compute_label_boost(query: &str, labels: &[String]) -> f64 {
    let query_tokens = tokenize(query);
    if query_tokens.is_empty() || labels.is_empty() {
        return 0.0;
    }
    let label_text = labels.join(" ").to_lowercase();
    let label_match = query_tokens
        .iter()
        .filter(|w| label_text.contains(w.as_str()))
        .count();
    label_match as f64 / query_tokens.len() as f64
}

fn compute_entity_boost(query: &str, labels: &[String]) -> f64 {
    let query_tokens = tokenize(query);
    if query_tokens.is_empty() {
        return 0.0;
    }
    let entity_labels: Vec<&String> = labels.iter().filter(|l| l.starts_with("entity:")).collect();
    if entity_labels.is_empty() {
        return 0.0;
    }
    let entity_text = entity_labels
        .iter()
        .map(|l| l.trim_start_matches("entity:").to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    let matched = query_tokens
        .iter()
        .filter(|w| entity_text.contains(w.as_str()))
        .count();
    matched as f64 / query_tokens.len() as f64
}

fn bm25_score(
    query: &str,
    payload: &MemoryPayload,
    avg_dl: f64,
    doc_count: usize,
    df_map: &std::collections::HashMap<String, usize>,
) -> f64 {
    let query_tokens = tokenize(query);
    if query_tokens.is_empty() {
        return 0.0;
    }

    let content_lower = payload.content.to_lowercase();
    let alias_text = payload.aliases.join(" ").to_lowercase();
    let labels_text = payload.labels.join(" ").to_lowercase();
    let doc_text = format!("{} {} {}", content_lower, alias_text, labels_text);
    let doc_tokens = tokenize(&doc_text);
    let dl = doc_tokens.len() as f64;

    let k1 = 1.2;
    let b = 0.75;
    let n = doc_count.max(1) as f64;

    let mut score = 0.0;
    for term in &query_tokens {
        let tf = doc_tokens.iter().filter(|t| **t == *term).count() as f64;
        if tf == 0.0 {
            continue;
        }
        let df = *df_map.get(term).unwrap_or(&1) as f64;
        let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
        let tf_norm = (tf * (k1 + 1.0)) / (tf + k1 * (1.0 - b + b * dl / avg_dl));
        score += idf * tf_norm;
    }
    score
}

fn build_df_map(
    tetras: &[crate::domain::tetra::Tetrahedron],
) -> (HashMap<String, usize>, usize, f64) {
    let mut df: HashMap<String, usize> = HashMap::new();
    let mut total_dl: f64 = 0.0;
    let doc_count = tetras.len();
    for t in tetras {
        let content_lower = t.data.content.to_lowercase();
        let alias_text = t.data.aliases.join(" ").to_lowercase();
        let labels_text = t.data.labels.join(" ").to_lowercase();
        let doc_text = format!("{} {} {}", content_lower, alias_text, labels_text);
        let doc_tokens = tokenize(&doc_text);
        total_dl += doc_tokens.len() as f64;
        let mut seen = HashSet::new();
        for term in doc_tokens {
            if seen.insert(term.clone()) {
                *df.entry(term).or_insert(0) += 1;
            }
        }
    }
    let avg_dl = if doc_count > 0 {
        total_dl / doc_count as f64
    } else {
        1.0
    };
    (df, doc_count, avg_dl.max(1.0))
}

fn is_noise_label(labels: &[String]) -> bool {
    labels.iter().any(|l| {
        let lower = l.to_lowercase();
        lower == "test"
            || lower == "testing"
            || lower == "junk"
            || lower == "scratch"
            || lower == "tmp"
            || lower == "temp"
    })
}

fn is_noise_content(content: &str) -> bool {
    let lower = content.to_lowercase();
    let trimmed = content.trim();
    if trimmed.len() < 10 {
        return true;
    }
    let noise_phrases = [
        "test",
        "testing 123",
        "hello world",
        "测试中文",
        "测试内容",
        "test content",
    ];
    for phrase in &noise_phrases {
        if lower == *phrase || trimmed.eq_ignore_ascii_case(phrase) {
            return true;
        }
    }
    if lower.starts_with("[session]") && lower.contains("| accomplished: test") {
        return true;
    }
    if lower.starts_with("[finding]") && lower.contains("| test") {
        return true;
    }
    false
}

fn score_tetra(
    t: &crate::domain::tetra::Tetrahedron,
    query_embedding: &Option<Vec<f64>>,
    search_query: &str,
    avg_dl: f64,
    doc_count: usize,
    df_map: &HashMap<String, usize>,
    keyword_fallback: bool,
    access_counts: &parking_lot::MutexGuard<HashMap<TetraId, u32>>,
) -> (TetraId, f64, f64, MemoryPayload) {
    let vec_sim = if let Some(ref qe) = query_embedding {
        if !t.data.embedding.is_empty() && t.data.embedding.len() == qe.len() {
            VectorLayer::cosine_similarity(qe, &t.data.embedding)
        } else if keyword_fallback {
            keyword_score(search_query, &t.data)
        } else {
            0.0
        }
    } else if keyword_fallback {
        keyword_score(search_query, &t.data)
    } else {
        0.0
    };
    let bm25 = bm25_score(search_query, &t.data, avg_dl, doc_count, df_map);
    let has_exact_match = has_exact_keyword_match(search_query, &t.data);
    let bm25_norm = (bm25 / (bm25 + 1.0)) * if has_exact_match { 0.35 } else { 0.20 };
    let hybrid = vec_sim * 0.50 + bm25_norm;
    let label_boost = compute_label_boost(search_query, &t.data.labels);
    let entity_boost = compute_entity_boost(search_query, &t.data.labels);
    let now_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as f64;
    let age_days = ((now_ts - t.data.timestamp as f64) / 86400.0).max(0.0);
    let recency = (-age_days * 0.08).exp();
    let importance = t.data.importance.max(0.1);
    let access_count = *access_counts.get(&t.id).unwrap_or(&0) as f64;
    let access_boost = 1.0 + (access_count / (access_count + 5.0)) * 0.3;
    let raw =
        (hybrid + label_boost * 0.15 + entity_boost * 0.20) * recency * importance * access_boost;
    let penalty = if t.data.labels.iter().any(|l| l.starts_with("meta-")) {
        0.5
    } else {
        1.0
    };
    let noise_penalty = if is_noise_label(&t.data.labels) || is_noise_content(&t.data.content) {
        0.3
    } else {
        1.0
    };
    let mass_boost = if raw > 0.3 {
        1.0 + (t.mass - 1.0).max(0.0) * 0.1
    } else {
        1.0
    };
    let final_sim = (raw * penalty * noise_penalty * mass_boost).min(1.0);
    (t.id, final_sim, t.mass, t.data.clone())
}

fn has_exact_keyword_match(query: &str, payload: &MemoryPayload) -> bool {
    let query_tokens = tokenize(query);
    if query_tokens.is_empty() {
        return false;
    }
    let content_lower = payload.content.to_lowercase();
    let all_text = format!(
        "{} {} {}",
        content_lower,
        payload.aliases.join(" ").to_lowercase(),
        payload.labels.join(" ").to_lowercase()
    );
    let match_count = query_tokens
        .iter()
        .filter(|t| all_text.contains(t.as_str()))
        .count();
    match_count as f64 / query_tokens.len() as f64 > 0.5
}
