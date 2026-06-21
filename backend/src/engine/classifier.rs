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
    client: attohttpc::Session,
    api_key: String,
    model: String,
    enabled: bool,
    pub(crate) thread_count: std::sync::atomic::AtomicUsize,
}

impl CategoryClassifier {
    pub fn new(api_key: &str, model: &str) -> Self {
        Self {
            categories: RwLock::new(HashMap::new()),
            client: attohttpc::Session::new(),
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
            return fallback_classify(labels);
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
        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": CLASSIFY_PROMPT},
                {"role": "user", "content": format!("Content: {}\nLabels: {}\nExisting categories: {}", content.chars().take(200).collect::<String>(), labels_str, existing_str)}
            ],
            "temperature": 0.0,
            "max_tokens": 128,
            "response_format": {"type": "json_object"}
        });

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .and_then(|r| r.send());

        match resp {
            Ok(resp) => {
                let body: serde_json::Value = resp.json().unwrap_or_default();
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
        .any(|l| ["history", "war", "ancient", "empire"].contains(l))
    {
        "history.general".to_string()
    } else if label_set
        .iter()
        .any(|l| ["language", "grammar", "linguistics"].contains(l))
    {
        "language.general".to_string()
    } else {
        "general".to_string()
    };

    ClassifyResult {
        category,
        parent: None,
    }
}

const CLASSIFY_PROMPT: &str = r#"You are a classifier. Given content, labels, and existing categories, return a JSON object with exactly two fields: "category" (a dot-notation category like "science.physics") and "parent" (the parent category or null). Use existing categories when appropriate."#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_result_has_no_layer() {
        let cc = CategoryClassifier::new("", "");
        let result = cc.classify("quantum entanglement", &["unknown".to_string()]);
        assert_eq!(result.category, "general");
    }

    #[test]
    fn fallback_classify_physics() {
        let result = CategoryClassifier::new("", "")
            .classify("anything", &["physics".to_string(), "quantum".to_string()]);
        assert_eq!(result.category, "science.physics");
    }

    #[test]
    fn fallback_classify_programming() {
        let result = CategoryClassifier::new("", "")
            .classify("anything", &["rust".to_string(), "programming".to_string()]);
        assert_eq!(result.category, "tech.programming");
    }

    #[test]
    fn fallback_classify_core() {
        let result = CategoryClassifier::new("", "").classify("anything", &[]);
        assert_eq!(result.category, "general");
    }
}
