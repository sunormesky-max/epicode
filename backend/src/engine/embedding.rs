use parking_lot::Mutex;

const DEFAULT_EMBEDDING_URL: &str = "http://localhost:11434/api/embed";
const DEFAULT_EMBEDDING_MODEL: &str = "nomic-embed-text";

pub struct EmbeddingService {
    client: ureq::Agent,
    api_key: String,
    api_url: String,
    model: String,
    enabled: bool,
    cache: Mutex<std::collections::HashMap<String, Vec<f64>>>,
    cache_order: Mutex<Vec<String>>,
}

impl EmbeddingService {
    pub fn from_env() -> Self {
        let api_url = std::env::var("EMBEDDING_API_URL")
            .unwrap_or_else(|_| DEFAULT_EMBEDDING_URL.to_string());
        let model = std::env::var("EMBEDDING_MODEL")
            .unwrap_or_else(|_| DEFAULT_EMBEDDING_MODEL.to_string());
        let api_key = std::env::var("EMBEDDING_API_KEY")
            .or_else(|_| std::env::var("SILICONFLOW_API_KEY"))
            .unwrap_or_default();
        let is_ollama = api_url.contains("11434")
            || api_url.contains("localhost")
            || api_url.contains("127.0.0.1");

        Self {
            client: ureq::AgentBuilder::new().build(),
            enabled: !api_key.is_empty() || is_ollama,
            api_key,
            api_url,
            model,
            cache: Mutex::new(std::collections::HashMap::new()),
            cache_order: Mutex::new(Vec::new()),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn embed(&self, text: &str) -> Result<Vec<f64>, String> {
        if !self.enabled {
            return Err("embedding service disabled".into());
        }

        let truncated: String = text.chars().take(500).collect();
        {
            let cache = self.cache.lock();
            if let Some(emb) = cache.get(&truncated) {
                return Ok(emb.clone());
            }
        }

        let is_ollama = self.api_url.contains("11434")
            || self.api_url.contains("localhost")
            || self.api_url.contains("127.0.0.1");

        let mut req = self
            .client
            .post(&self.api_url)
            .timeout(std::time::Duration::from_secs(3))
            .set("Content-Type", "application/json");
        if !self.api_key.is_empty() {
            req = req.set("Authorization", &format!("Bearer {}", self.api_key));
        }

        let body: serde_json::Value = if is_ollama {
            ureq::json!({
                "model": self.model,
                "input": truncated
            })
        } else {
            ureq::json!({
                "model": self.model,
                "input": truncated,
                "encoding_format": "float"
            })
        };

        let resp: serde_json::Value = req
            .send_json(body)
            .map_err(|e| format!("embedding HTTP: {}", e))?
            .into_json()
            .map_err(|e| format!("embedding JSON: {}", e))?;

        let embedding: Vec<f64> = if is_ollama {
            resp["embeddings"][0]
                .as_array()
                .ok_or("no embeddings array in ollama response")?
                .iter()
                .filter_map(|v| v.as_f64())
                .collect()
        } else {
            resp["data"][0]["embedding"]
                .as_array()
                .ok_or("no embedding array in response")?
                .iter()
                .filter_map(|v| v.as_f64())
                .collect()
        };

        if embedding.is_empty() {
            return Err("empty embedding vector".into());
        }

        {
            let mut cache = self.cache.lock();
            let mut order = self.cache_order.lock();
            cache.insert(truncated.clone(), embedding.clone());
            order.push(truncated);
            while cache.len() > 100 {
                if order.is_empty() {
                    break;
                }
                let old = order.remove(0);
                cache.remove(&old);
            }
        }

        Ok(embedding)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_identical_vectors() {
        let v = vec![1.0, 2.0, 3.0];
        let sim = super::super::vector::VectorLayer::cosine_similarity(&v, &v);
        assert!((sim - 1.0).abs() < 1e-10);
    }

    #[test]
    fn cosine_orthogonal_vectors() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let sim = super::super::vector::VectorLayer::cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-10);
    }

    #[test]
    fn cosine_opposite_vectors() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        let sim = super::super::vector::VectorLayer::cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-10);
    }

    #[test]
    fn cosine_empty_vectors() {
        assert_eq!(
            super::super::vector::VectorLayer::cosine_similarity(&[], &[]),
            0.0
        );
    }

    #[test]
    fn cosine_different_lengths() {
        assert_eq!(
            super::super::vector::VectorLayer::cosine_similarity(&[1.0], &[1.0, 2.0]),
            0.0
        );
    }

    #[test]
    fn blob_roundtrip() {
        let original: Vec<f64> = vec![1.0, -2.5, 3.14, 0.0, 1e-10];
        let blob = super::super::vector::VectorLayer::embedding_to_blob(&original);
        let restored = super::super::vector::VectorLayer::blob_to_embedding(&blob);
        assert_eq!(restored.len(), original.len());
        for (a, b) in original.iter().zip(restored.iter()) {
            assert!((a - b).abs() < 1e-15);
        }
    }

    #[test]
    fn blob_empty() {
        let blob = super::super::vector::VectorLayer::embedding_to_blob(&[]);
        assert!(blob.is_empty());
        assert!(super::super::vector::VectorLayer::blob_to_embedding(&blob).is_empty());
    }

    #[test]
    fn best_sim_prefers_embedding() {
        let emb_a = vec![1.0, 0.0, 0.0];
        let emb_b = vec![0.9, 0.1, 0.0];
        let labels_a = vec!["rust".to_string()];
        let labels_b = vec!["python".to_string()];
        let sim = super::super::vector::VectorLayer::best_similarity(
            &emb_a, &labels_a, &emb_b, &labels_b,
        );
        assert!(sim > 0.8);
    }

    #[test]
    fn best_sim_falls_back_to_labels() {
        let labels_a = vec!["rust".to_string()];
        let labels_b = vec!["rust".to_string()];
        let sim =
            super::super::vector::VectorLayer::best_similarity(&[], &labels_a, &[], &labels_b);
        assert!((sim - 1.0).abs() < 1e-10);
    }

    #[test]
    fn service_disabled_without_key() {
        let svc = EmbeddingService {
            client: ureq::AgentBuilder::new().build(),
            api_key: String::new(),
            api_url: DEFAULT_EMBEDDING_URL.to_string(),
            model: DEFAULT_EMBEDDING_MODEL.to_string(),
            enabled: false,
            cache: Mutex::new(std::collections::HashMap::new()),
            cache_order: Mutex::new(Vec::new()),
        };
        assert!(!svc.enabled());
        assert!(svc.embed("test").is_err());
    }
}
