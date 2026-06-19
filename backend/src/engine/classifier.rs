use std::collections::HashMap;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryInfo {
    pub name: String,
    pub parent: Option<String>,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifyResult {
    pub category: String,
    pub parent: Option<String>,
}

pub struct CategoryClassifier {
    categories: RwLock<HashMap<String, CategoryInfo>>,
    client: ureq::Agent,
    api_key: String,
    model: String,
    enabled: bool,
    pub(crate) thread_count: std::sync::atomic::AtomicUsize,
}

impl CategoryClassifier {
    pub fn new(api_key: &str, model: &str) -> Self {
        Self {
            categories: RwLock::new(HashMap::new()),
            client: ureq::AgentBuilder::new()
                .timeout_read(std::time::Duration::from_secs(30))
                .timeout_write(std::time::Duration::from_secs(5))
                .build(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            enabled: !api_key.is_empty(),
            thread_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn classify(&self, content: &str, labels: &[String]) -> ClassifyResult {
        if !self.enabled {
            return ClassifyResult {
                category: "general".to_string(),
                parent: None,
            };
        }

        let cats = self.categories.read();
        let existing_names: Vec<String> = cats.keys().cloned().collect();
        drop(cats);

        let llm_result = self.llm_classify(content, labels, &existing_names);

        let mut cats = self.categories.write();
        if let Some(existing) = cats.get(&llm_result.category) {
            let info = CategoryInfo {
                name: existing.name.clone(),
                parent: existing.parent.clone(),
                count: existing.count + 1,
            };
            let result = ClassifyResult {
                category: info.name.clone(),
                parent: info.parent.clone(),
            };
            cats.insert(info.name.clone(), info);
            return result;
        }

        let info = CategoryInfo {
            name: llm_result.category.clone(),
            parent: llm_result.parent.clone(),
            count: 1,
        };
        let result = ClassifyResult {
            category: info.name.clone(),
            parent: info.parent.clone(),
        };
        cats.insert(info.name.clone(), info);
        result
    }

    fn llm_classify(
        &self,
        content: &str,
        labels: &[String],
        existing: &[String],
    ) -> ClassifyResult {
        let existing_str = if existing.is_empty() {
            "none".to_string()
        } else {
            existing.join(", ")
        };
        let labels_str = labels.join(", ");

        let url = format!("{}/v1/chat/completions", super::cognitive::DEEPSEEK_BASE);
        let resp = self.client
            .post(&url)
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set("Content-Type", "application/json")
            .send_json(ureq::json!({
                "model": self.model,
                "messages": [
                    {"role": "system", "content": CLASSIFY_PROMPT},
                    {"role": "user", "content": format!("Content: {}\nLabels: {}\nExisting categories: {}", content.chars().take(200).collect::<String>(), labels_str, existing_str)}
                ],
                "temperature": 0.0,
                "max_tokens": 128,
                "response_format": {"type": "json_object"}
            }));

        match resp {
            Ok(resp) => {
                let body: serde_json::Value = resp.into_json().unwrap_or_default();
                if let Some(content_str) = body["choices"][0]["message"]["content"].as_str() {
                    if let Ok(parsed) = serde_json::from_str::<ClassifyResult>(content_str) {
                        return parsed;
                    }
                }
                fallback_classify(labels)
            }
            Err(_) => fallback_classify(labels),
        }
    }
}

fn fallback_classify(labels: &[String]) -> ClassifyResult {
    let label_set: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
    let category = if label_set.iter().any(|l| {
        [
            "physics",
            "quantum",
            "mechanics",
            "relativity",
            "thermodynamics",
        ]
        .contains(l)
    }) {
        "science.physics".to_string()
    } else if label_set
        .iter()
        .any(|l| ["biology", "genetics", "dna", "evolution"].contains(l))
    {
        "science.biology".to_string()
    } else if label_set
        .iter()
        .any(|l| ["astronomy", "planet", "solar", "space"].contains(l))
    {
        "science.astronomy".to_string()
    } else if label_set
        .iter()
        .any(|l| ["git", "programming", "python", "rust", "coding", "version"].contains(l))
    {
        "tech.programming".to_string()
    } else if label_set
        .iter()
        .any(|l| ["network", "tcp", "protocol", "http"].contains(l))
    {
        "tech.networking".to_string()
    } else if label_set
        .iter()
        .any(|l| ["crypto", "bitcoin", "blockchain"].contains(l))
    {
        "tech.cryptocurrency".to_string()
    } else if label_set
        .iter()
        .any(|l| ["coffee", "food", "cooking", "nutrition", "diet"].contains(l))
    {
        "life.food".to_string()
    } else if label_set
        .iter()
        .any(|l| ["music", "mozart", "art", "composition"].contains(l))
    {
        "culture.music".to_string()
    } else if label_set
        .iter()
        .any(|l| ["geography", "mountain", "volcano", "trench", "ocean"].contains(l))
    {
        "geo.geography".to_string()
    } else if label_set
        .iter()
        .any(|l| ["battery", "energy", "electronics", "device"].contains(l))
    {
        "tech.electronics".to_string()
    } else if label_set
        .iter()
        .any(|l| ["bee", "animal", "insect", "nature"].contains(l))
    {
        "nature.animal".to_string()
    } else if label_set
        .iter()
        .any(|l| ["weather", "rain", "climate"].contains(l))
    {
        "geo.weather".to_string()
    } else if label_set
        .iter()
        .any(|l| ["memory", "learning", "ai", "embedding"].contains(l))
    {
        "ai.systems".to_string()
    } else if label_set
        .iter()
        .any(|l| ["identity", "constitution", "core"].contains(l))
    {
        "core.identity".to_string()
    } else if label_set
        .iter()
        .any(|l| ["security", "safety", "auth"].contains(l))
    {
        "core.security".to_string()
    } else {
        "general.misc".to_string()
    };

    let parent = category.split('.').next().unwrap_or("general").to_string();

    ClassifyResult {
        category,
        parent: Some(parent),
    }
}

const CLASSIFY_PROMPT: &str = r#"Classify this memory into a fine-grained category hierarchy.

Rules:
1. Use dot-notation: "domain.subdomain" (e.g. "physics.quantum", "tech.programming", "life.food")
2. Be SPECIFIC: "physics.quantum" not just "science"
3. Prefer matching an existing category if semantically close
4. Set parent to the top-level domain
5. CRITICAL domain mapping:
   - Content about "David", "I am", "identity", "my memories" → "system.identity"
   - Content about Epicode internals (cylinder, port, pulse, scheduler, vertex, tetra, cluster, embedding, HNSW, fission, dream engine, knowledge graph) → "architecture.internal"
   - Content about Rust, programming, tokio, axum, SQLite → "tech.programming"
   - Content about AI/ML concepts (embeddings, vectors, cosine similarity, neural networks) → "ai.theory"
   - Content about system design, optimization, performance → "engineering.performance"

Return JSON:
{"category": "system.identity", "parent": "system"}"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_classify_physics() {
        let r = fallback_classify(&["quantum".to_string(), "physics".to_string()]);
        assert_eq!(r.category, "science.physics");
    }

    #[test]
    fn fallback_classify_core() {
        let r = fallback_classify(&["identity".to_string(), "system".to_string()]);
        assert_eq!(r.category, "core.identity");
    }

    #[test]
    fn fallback_classify_programming() {
        let r = fallback_classify(&["git".to_string(), "system".to_string()]);
        assert_eq!(r.category, "tech.programming");
    }

    #[test]
    fn classify_result_has_no_layer() {
        let r = fallback_classify(&["test".to_string()]);
        assert!(r.category.contains('.'));
    }
}
