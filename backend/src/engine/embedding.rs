use parking_lot::Mutex;

const DEFAULT_EMBEDDING_URL: &str = "http://localhost:11434/api/embed";
const DEFAULT_EMBEDDING_MODEL: &str = "nomic-embed-text";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingProvider {
    Ollama,
    OpenAI,
    Generic,
}

impl EmbeddingProvider {
    pub fn detect(api_url: &str, model: &str) -> Self {
        let provider_env = std::env::var("EMBEDDING_PROVIDER").unwrap_or_default().to_lowercase();
        match provider_env.as_str() {
            "openai" => return EmbeddingProvider::OpenAI,
            "ollama" => return EmbeddingProvider::Ollama,
            "generic" => return EmbeddingProvider::Generic,
            _ => {}
        }

        if api_url.contains("openai.com") || model.starts_with("text-embedding") {
            return EmbeddingProvider::OpenAI;
        }
        if api_url.contains("11434")
            || api_url.contains("localhost")
            || api_url.contains("127.0.0.1")
        {
            return EmbeddingProvider::Ollama;
        }
        EmbeddingProvider::Generic
    }

    pub fn api_url(&self, configured_url: &str) -> String {
        match self {
            EmbeddingProvider::OpenAI => "https://api.openai.com/v1/embeddings".to_string(),
            EmbeddingProvider::Ollama => configured_url.to_string(),
            EmbeddingProvider::Generic => configured_url.to_string(),
        }
    }

    pub fn request_body(&self, model: &str, input: &str) -> serde_json::Value {
        match self {
            EmbeddingProvider::Ollama => serde_json::json!({
                "model": model,
                "input": input
            }),
            EmbeddingProvider::OpenAI | EmbeddingProvider::Generic => serde_json::json!({
                "model": model,
                "input": input,
                "encoding_format": "float"
            }),
        }
    }

    pub fn extract_embedding(&self, resp: &serde_json::Value) -> Result<Vec<f64>, String> {
        let vec = match self {
            EmbeddingProvider::Ollama => resp["embeddings"][0]
                .as_array()
                .ok_or_else(|| "no embeddings array in ollama response".to_string())?
                .iter()
                .filter_map(|v| v.as_f64())
                .collect::<Vec<f64>>(),
            EmbeddingProvider::OpenAI | EmbeddingProvider::Generic => resp["data"][0]["embedding"]
                .as_array()
                .ok_or_else(|| "no embedding array in response".to_string())?
                .iter()
                .filter_map(|v| v.as_f64())
                .collect::<Vec<f64>>(),
        };
        Ok(vec)
    }
}

pub struct EmbeddingService {
    client: attohttpc::Session,
    api_key: String,
    api_url: String,
    model: String,
    enabled: bool,
    provider: EmbeddingProvider,
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
        let provider = EmbeddingProvider::detect(&api_url, &model);
        let is_ollama = matches!(provider, EmbeddingProvider::Ollama);

        let client = attohttpc::Session::new();

        Self {
            client,
            enabled: !api_key.is_empty() || is_ollama,
            api_key,
            api_url: provider.api_url(&api_url),
            model,
            provider,
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

        let body = self.provider.request_body(&self.model, &truncated);

        let mut req = self.client.post(&self.api_url);
        if !self.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.api_key));
        }

        let resp: serde_json::Value = req
            .json(&body)
            .map_err(|e| format!("embedding request build: {e}"))?
            .send()
            .map_err(|e| format!("embedding HTTP: {e}"))?
            .json()
            .map_err(|e| format!("embedding JSON: {e}"))?;

        let embedding = self.provider.extract_embedding(&resp)?;

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
    fn service_disabled_without_key() {
        let svc = EmbeddingService {
            client: attohttpc::Session::new(),
            api_key: String::new(),
            api_url: "http://example.com".to_string(),
            model: "test".to_string(),
            enabled: false,
            provider: EmbeddingProvider::Generic,
            cache: Mutex::new(std::collections::HashMap::new()),
            cache_order: Mutex::new(Vec::new()),
        };
        assert!(svc.embed("test").is_err());
    }

    #[test]
    fn provider_detect_openai_by_url() {
        assert_eq!(
            EmbeddingProvider::detect("https://api.openai.com/v1/embeddings", "model"),
            EmbeddingProvider::OpenAI
        );
    }

    #[test]
    fn provider_detect_openai_by_model() {
        assert_eq!(
            EmbeddingProvider::detect("http://example.com", "text-embedding-3-small"),
            EmbeddingProvider::OpenAI
        );
    }

    #[test]
    fn provider_detect_ollama() {
        assert_eq!(
            EmbeddingProvider::detect("http://localhost:11434/api/embed", "nomic-embed-text"),
            EmbeddingProvider::Ollama
        );
        assert_eq!(
            EmbeddingProvider::detect("http://127.0.0.1:11434/api/embed", "model"),
            EmbeddingProvider::Ollama
        );
    }

    #[test]
    fn provider_detect_generic() {
        assert_eq!(
            EmbeddingProvider::detect("https://api.siliconflow.cn/v1/embeddings", "model"),
            EmbeddingProvider::Generic
        );
    }

    #[test]
    fn provider_env_override() {
        std::env::set_var("EMBEDDING_PROVIDER", "openai");
        assert_eq!(
            EmbeddingProvider::detect("http://localhost:11434", "nomic-embed-text"),
            EmbeddingProvider::OpenAI
        );
        std::env::set_var("EMBEDDING_PROVIDER", "ollama");
        assert_eq!(
            EmbeddingProvider::detect("https://api.openai.com", "text-embedding-3-small"),
            EmbeddingProvider::Ollama
        );
        std::env::remove_var("EMBEDDING_PROVIDER");
    }

    #[test]
    fn ollama_request_body_format() {
        let body = EmbeddingProvider::Ollama.request_body("nomic-embed-text", "hello");
        assert_eq!(body["model"], "nomic-embed-text");
        assert_eq!(body["input"], "hello");
        assert!(body.get("encoding_format").is_none());
    }

    #[test]
    fn openai_request_body_format() {
        let body = EmbeddingProvider::OpenAI.request_body("text-embedding-3-small", "hello");
        assert_eq!(body["model"], "text-embedding-3-small");
        assert_eq!(body["input"], "hello");
        assert_eq!(body["encoding_format"], "float");
    }

    #[test]
    fn generic_request_body_format() {
        let body = EmbeddingProvider::Generic.request_body("model", "hello");
        assert_eq!(body["model"], "model");
        assert_eq!(body["input"], "hello");
        assert_eq!(body["encoding_format"], "float");
    }

    #[test]
    fn ollama_extract_embedding() {
        let resp = serde_json::json!({"embeddings": [[0.1, 0.2, 0.3]]});
        let emb = EmbeddingProvider::Ollama.extract_embedding(&resp).unwrap();
        assert_eq!(emb, vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn openai_extract_embedding() {
        let resp = serde_json::json!({"data": [{"embedding": [0.1, 0.2, 0.3]}]});
        let emb = EmbeddingProvider::OpenAI.extract_embedding(&resp).unwrap();
        assert_eq!(emb, vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn generic_extract_embedding() {
        let resp = serde_json::json!({"data": [{"embedding": [0.4, 0.5, 0.6]}]});
        let emb = EmbeddingProvider::Generic.extract_embedding(&resp).unwrap();
        assert_eq!(emb, vec![0.4, 0.5, 0.6]);
    }

    #[test]
    fn openai_api_url() {
        assert_eq!(
            EmbeddingProvider::OpenAI.api_url("http://ignored"),
            "https://api.openai.com/v1/embeddings"
        );
    }

    #[test]
    fn ollama_api_url_uses_configured() {
        assert_eq!(
            EmbeddingProvider::Ollama.api_url("http://localhost:11434/api/embed"),
            "http://localhost:11434/api/embed"
        );
    }

    #[test]
    fn generic_api_url_uses_configured() {
        assert_eq!(
            EmbeddingProvider::Generic.api_url("https://api.siliconflow.cn/v1/embeddings"),
            "https://api.siliconflow.cn/v1/embeddings"
        );
    }
}
