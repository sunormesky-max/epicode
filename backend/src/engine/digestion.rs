use std::sync::Arc;

use super::cognitive::CognitiveEngine;
use super::scheduler::SchedulerCenter;
use crate::domain::tetra::TetraId;

#[derive(Debug, Clone)]
pub struct DigestChunk {
    pub content: String,
    pub index: usize,
}

#[derive(Debug)]
pub struct DigestResult {
    pub total_chunks: usize,
    pub memories_created: usize,
    pub ids: Vec<TetraId>,
    pub labels_map: Vec<(TetraId, Vec<String>)>,
    pub skipped: usize,
}

pub struct DigestionEngine {
    scheduler: Arc<SchedulerCenter>,
    cognitive: Arc<CognitiveEngine>,
}

impl DigestionEngine {
    pub fn new(scheduler: Arc<SchedulerCenter>, cognitive: Arc<CognitiveEngine>) -> Self {
        Self {
            scheduler,
            cognitive,
        }
    }

    pub fn is_allowed_file(filename: &str) -> bool {
        let lower = filename.to_lowercase();
        lower.ends_with(".txt")
            || lower.ends_with(".md")
            || lower.ends_with(".json")
            || lower.ends_with(".csv")
    }

    pub fn extract_text(&self, raw: &str, filename: &str) -> Result<String, String> {
        let lower = filename.to_lowercase();
        if lower.ends_with(".json") {
            self.extract_json(raw)
        } else if lower.ends_with(".csv") {
            self.extract_csv(raw)
        } else {
            Ok(raw.to_string())
        }
    }

    fn extract_json(&self, raw: &str) -> Result<String, String> {
        let val: serde_json::Value =
            serde_json::from_str(raw).map_err(|e| format!("invalid JSON: {e}"))?;
        Ok(self.flatten_json_value(&val, 0))
    }

    fn flatten_json_value(&self, val: &serde_json::Value, depth: usize) -> String {
        match val {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Null => String::new(),
            serde_json::Value::Array(arr) => arr
                .iter()
                .map(|v| self.flatten_json_value(v, depth + 1))
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("\n"),
            serde_json::Value::Object(obj) => {
                let indent = "  ".repeat(depth);
                obj.iter()
                    .map(|(k, v)| {
                        let child = self.flatten_json_value(v, depth + 1);
                        if child.contains('\n') {
                            format!("{indent}{k}:\n{child}")
                        } else {
                            format!("{indent}{k}: {child}")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }
    }

    fn extract_csv(&self, raw: &str) -> Result<String, String> {
        let mut lines = raw.lines();
        let header_line = lines.next().ok_or("empty CSV")?;
        let headers: Vec<&str> = header_line.split(',').map(|s| s.trim()).collect();
        let mut rows = Vec::new();
        for line in lines {
            if line.trim().is_empty() {
                continue;
            }
            let fields: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
            let mut parts = Vec::new();
            for (i, field) in fields.iter().enumerate() {
                if let Some(header) = headers.get(i) {
                    if !field.is_empty() {
                        parts.push(format!("{header}: {field}"));
                    }
                }
            }
            if !parts.is_empty() {
                rows.push(parts.join(", "));
            }
        }
        Ok(rows.join("\n\n"))
    }

    pub fn digest(
        &self,
        content: &str,
        source: &str,
        max_chunk_size: usize,
    ) -> Result<DigestResult, String> {
        if content.trim().is_empty() {
            return Err("content is empty".into());
        }
        if content.len() > 1_000_000 {
            return Err("content exceeds 1MB limit".into());
        }
        if max_chunk_size < 50 {
            return Err("chunk size must be at least 50".into());
        }

        let chunks = self.chunk_content(content, max_chunk_size);
        let total_chunks = chunks.len();

        tracing::info!(
            "[Digestion] starting: source='{}', {} chars → {} chunks (max_chunk={})",
            source,
            content.len(),
            total_chunks,
            max_chunk_size
        );

        let mut ids = Vec::new();
        let mut labels_map = Vec::new();
        let mut skipped = 0usize;

        for chunk in &chunks {
            let text = chunk.content.trim().to_string();
            if text.is_empty() {
                skipped += 1;
                continue;
            }

            let labels = self.classify_chunk(&text);

            let enriched = if !source.is_empty() {
                format!("{text}\n[source: {source}]")
            } else {
                text.clone()
            };

            match self.scheduler.api_remember(&enriched) {
                Ok((id, auto_labels)) => {
                    let mut final_labels = labels;
                    for l in &auto_labels {
                        if !final_labels.contains(l) {
                            final_labels.push(l.clone());
                        }
                    }
                    if !final_labels.iter().any(|l| l == "digested") {
                        final_labels.push("digested".to_string());
                    }
                    if let Err(e) = self
                        .scheduler
                        .storage_handle()
                        .update_labels(id, &final_labels)
                    {
                        tracing::warn!("[Digestion] label update failed for #{}: {}", id, e);
                    }
                    labels_map.push((id, final_labels));
                    ids.push(id);
                }
                Err(e) => {
                    tracing::warn!("[Digestion] chunk {} failed: {}", chunk.index, e);
                    skipped += 1;
                }
            }
        }

        let created = ids.len();
        tracing::info!(
            "[Digestion] complete: source='{}', {}/{} created, {} skipped",
            source,
            created,
            total_chunks,
            skipped
        );

        Ok(DigestResult {
            total_chunks,
            memories_created: created,
            ids,
            labels_map,
            skipped,
        })
    }

    fn chunk_content(&self, content: &str, max_size: usize) -> Vec<DigestChunk> {
        let paragraphs: Vec<&str> = content
            .split("\n\n")
            .flat_map(|p| {
                if p.len() > max_size {
                    self.split_long_text(p, max_size)
                } else {
                    vec![p]
                }
            })
            .filter(|p| !p.trim().is_empty())
            .collect();

        let mut chunks = Vec::new();
        let mut buffer = String::new();

        for para in &paragraphs {
            if buffer.len() + para.len() + 2 > max_size && !buffer.is_empty() {
                chunks.push(buffer.trim().to_string());
                buffer.clear();
            }
            if !buffer.is_empty() {
                buffer.push_str("\n\n");
            }
            buffer.push_str(para);
        }
        if !buffer.trim().is_empty() {
            chunks.push(buffer.trim().to_string());
        }

        chunks
            .into_iter()
            .enumerate()
            .map(|(i, content)| DigestChunk { content, index: i })
            .collect()
    }

    fn split_long_text<'a>(&self, text: &'a str, max_size: usize) -> Vec<&'a str> {
        let mut result = Vec::new();
        let mut start = 0;

        while start < text.len() {
            let mut end = (start + max_size).min(text.len());
            while end < text.len() && !text.is_char_boundary(end) {
                end += 1;
            }
            if end >= text.len() {
                result.push(&text[start..]);
                break;
            }

            let search_range = &text[start..end];
            let cut = if let Some(pos) = search_range.rfind("。") {
                start + pos + "。".len()
            } else if let Some(pos) = search_range.rfind("．") {
                start + pos + "．".len()
            } else if let Some(pos) = search_range.rfind(". ") {
                start + pos + ". ".len()
            } else if let Some(pos) = search_range.rfind("\n") {
                start + pos + 1
            } else if let Some(pos) = search_range.rfind(" ") {
                start + pos + 1
            } else {
                end
            };
            let mut safe_cut = cut;
            while safe_cut < text.len() && !text.is_char_boundary(safe_cut) {
                safe_cut += 1;
            }
            result.push(&text[start..safe_cut]);
            start = safe_cut;
        }

        result
    }

    fn classify_chunk(&self, text: &str) -> Vec<String> {
        if self.cognitive.enabled() {
            match self.cognitive.classify_content(text) {
                Ok(labels) => return labels,
                Err(e) => {
                    tracing::debug!(
                        "[Digestion] cognitive classify failed: {}, using heuristic",
                        e
                    );
                }
            }
        }
        self.heuristic_classify(text)
    }

    fn heuristic_classify(&self, text: &str) -> Vec<String> {
        let lower = text.to_lowercase();
        let mut labels = Vec::new();

        let rules: &[(&str, &str)] = &[
            ("函数", "code"),
            ("function", "code"),
            ("class ", "code"),
            ("import ", "code"),
            ("fn ", "code"),
            ("pub ", "code"),
            ("架构", "architecture"),
            ("设计", "architecture"),
            ("模块", "architecture"),
            ("系统", "architecture"),
            ("component", "architecture"),
            ("安全", "security"),
            ("加密", "security"),
            ("认证", "security"),
            ("密码", "security"),
            ("authentication", "security"),
            ("测试", "testing"),
            ("test", "testing"),
            ("bug", "testing"),
            ("部署", "deployment"),
            ("服务器", "deployment"),
            ("docker", "deployment"),
            ("nginx", "deployment"),
            ("systemd", "deployment"),
            ("数据库", "database"),
            ("sql", "database"),
            ("sqlite", "database"),
            ("查询", "database"),
            ("query", "database"),
            ("api", "api"),
            ("接口", "api"),
            ("endpoint", "api"),
            ("性能", "performance"),
            ("优化", "performance"),
            ("延迟", "performance"),
            ("文档", "docs"),
            ("README", "docs"),
            ("说明", "docs"),
            ("配置", "config"),
            ("环境变量", "config"),
            ("config", "config"),
            ("用户", "user"),
            ("权限", "user"),
            ("角色", "user"),
            ("记忆", "memory"),
            ("搜索", "search"),
            ("向量", "vector"),
            ("算法", "algorithm"),
            ("模型", "model"),
            ("训练", "ml"),
        ];

        for (keyword, label) in rules {
            if lower.contains(keyword) && !labels.contains(&label.to_string()) {
                labels.push(label.to_string());
            }
        }

        if labels.is_empty() {
            labels.push("general".to_string());
        }

        labels.truncate(3);
        labels
    }
}
