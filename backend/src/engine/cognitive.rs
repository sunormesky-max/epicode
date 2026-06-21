use crate::util::truncate_str;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const DEEPSEEK_BASE: &str = "https://api.deepseek.com";

#[derive(Debug, Clone, Serialize)]
pub struct DecisionRecord {
    pub tick: u64,
    pub action: String,
    pub detail: String,
    pub result: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StateSnapshot {
    pub tick: u64,
    pub tetras: usize,
    pub clusters: usize,
    pub energy: f64,
    pub avg_entropy: f64,
    pub max_entropy: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemState {
    pub tick: u64,
    pub energy: f64,
    pub max_energy: f64,
    pub total_tetras: usize,
    pub total_vertices: usize,
    pub total_clusters: usize,
    pub avg_mass: f64,
    pub max_mass: f64,
    pub clusters: Vec<ClusterState>,
    pub memories: Vec<MemoryInfo>,
    pub recent_events: Vec<String>,
    pub last_dream_tick: u64,
    pub decision_history: Vec<DecisionRecord>,
    pub prev_snapshot: Option<StateSnapshot>,
    pub search_metrics: Option<SearchPerception>,
    pub kg_analysis: Option<KgPerception>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClusterState {
    pub index: usize,
    pub size: usize,
    pub label_distribution: std::collections::HashMap<String, usize>,
    pub entropy: f64,
    pub centroid: [f64; 3],
    pub member_ids: Vec<u64>,
    pub member_labels: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryInfo {
    pub id: u64,
    pub content_preview: String,
    pub labels: Vec<String>,
    pub cluster_index: usize,
    pub mass: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchPerception {
    pub total_queries: u64,
    pub hit_count: u64,
    pub hit_rate: f64,
    pub miss_queries: Vec<String>,
    pub top_labels: Vec<(String, u32)>,
    pub hot_memories: Vec<(u64, u32)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct KgPerception {
    pub total_tetras: usize,
    pub total_relations: usize,
    pub orphan_count: usize,
    pub orphan_ratio: f64,
    pub largest_component: usize,
    pub disconnected_components: Vec<usize>,
    pub avg_degree: f64,
    pub density: f64,
    pub relation_type_counts: std::collections::HashMap<String, usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CognitiveResponse {
    pub thoughts: String,
    #[serde(default)]
    pub actions: Vec<SchedulerAction>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum SchedulerAction {
    #[serde(rename = "pulse")]
    Pulse {
        origin: u64,
        #[serde(default = "default_pulse_type")]
        pulse_type: String,
        #[serde(default = "default_ttl")]
        ttl: u32,
    },
    #[serde(rename = "fission")]
    Fission { cluster_index: usize },
    #[serde(rename = "fuse")]
    Fuse { cluster_a: usize, cluster_b: usize },
    #[serde(rename = "dream")]
    Dream,
    #[serde(rename = "link")]
    Link { a: u64, b: u64, reason: String },
    #[serde(rename = "consolidate")]
    Consolidate {
        ids: Vec<u64>,
        keep: u64,
        summary: String,
    },
    #[serde(rename = "mark_junk")]
    MarkJunk { ids: Vec<u64>, reason: String },
    #[serde(rename = "relabel")]
    Relabel {
        id: u64,
        add_labels: Vec<String>,
        remove_labels: Vec<String>,
        reason: String,
    },
    #[serde(rename = "reflect")]
    Reflect {
        observation: String,
        insight: String,
    },
    #[serde(rename = "use_tool")]
    UseTool {
        tool: String,
        args: serde_json::Value,
    },
}

fn default_pulse_type() -> String {
    "neural".into()
}

fn default_ttl() -> u32 {
    5
}

pub trait ToolProvider: Send + Sync {
    fn execute_tool(&self, name: &str, args: &serde_json::Value) -> Result<String, String>;
    fn definitions(&self) -> Vec<serde_json::Value>;
}

pub struct CognitiveEngine {
    client: attohttpc::Session,
    api_key: String,
    model: String,
    enabled: bool,
    last_raw_response: Mutex<String>,
    last_prompt_sent: Mutex<String>,
    tools: Mutex<Option<std::sync::Arc<dyn ToolProvider>>>,
    translate_cache: Mutex<HashMap<String, String>>,
    translate_cache_order: Mutex<Vec<String>>,
}

impl CognitiveEngine {
    pub fn new(api_key: &str, model: &str) -> Self {
        let client = attohttpc::Session::new();
        Self {
            client,
            api_key: api_key.to_string(),
            model: model.to_string(),
            enabled: !api_key.is_empty(),
            last_raw_response: Mutex::new(String::new()),
            last_prompt_sent: Mutex::new(String::new()),
            tools: Mutex::new(None),
            translate_cache: Mutex::new(HashMap::new()),
            translate_cache_order: Mutex::new(Vec::new()),
        }
    }

    pub fn set_tools(&self, tools: std::sync::Arc<dyn ToolProvider>) {
        *self.tools.lock() = Some(tools);
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn classify_content(&self, content: &str) -> Result<Vec<String>, String> {
        let local = Self::classify_local(content);
        if !local.is_empty() {
            return Ok(local);
        }
        if !self.enabled {
            return Ok(vec!["general".to_string()]);
        }
        self.classify_via_llm(content)
    }

    fn classify_local(content: &str) -> Vec<String> {
        let c = content.to_lowercase();
        // Identity/system rules checked first — highest priority
        let identity_rules: &[(&[&str], &[&str])] = &[
            (
                &[
                    "i am david",
                    "i'm david",
                    "my identity",
                    "epicode identity",
                    "david identity",
                    "ai identity of epicode",
                ],
                &["identity", "system"],
            ),
            (
                &[
                    "epicode uses",
                    "epicode architecture",
                    "cylinder hub",
                    "central cylinder",
                    "pulseengine",
                    "dreamengine",
                    "gatewaycenter",
                    "scheduler",
                    "cognitive engine",
                    "knowledge graph",
                    "hnsw",
                    "vector layer",
                    "spaceinner",
                    "space inner",
                    "decisioncenter",
                    "fission",
                    "lock contention",
                    "read locks",
                    "write lock",
                ],
                &["system", "architecture"],
            ),
            (
                &[
                    "edge length is fixed",
                    "vertex merge epsilon",
                    "vertex merge",
                    "regular tetrahedron",
                    "tetrahedrons in 3d",
                ],
                &["system", "geometry"],
            ),
        ];
        for (keywords, labels) in identity_rules {
            if keywords.iter().any(|kw| c.contains(kw)) {
                return labels.iter().map(|s| s.to_string()).collect();
            }
        }

        let rules: &[(&[&str], &[&str])] = &[
            (
                &[
                    "rust",
                    "borrow",
                    "trait",
                    "closure",
                    "async",
                    "cargo",
                    "lifetime",
                    "macro",
                    "ownership",
                    "unsafe",
                    "arc",
                    "mutex",
                    "rwlock",
                ],
                &["programming", "rust"],
            ),
            (
                &[
                    "python",
                    "list comprehension",
                    "decorator",
                    "generator",
                    "gil",
                    "context manager",
                    "asyncio",
                    "dataclass",
                    "pandas",
                    "numpy",
                    "flask",
                    "django",
                ],
                &["programming", "python"],
            ),
            (
                &[
                    "javascript",
                    "typescript",
                    "node",
                    "promise",
                    "react",
                    "vue",
                    "angular",
                    "webpack",
                    "npm",
                    "event loop",
                    "closure",
                ],
                &["programming", "javascript"],
            ),
            (
                &[
                    "haskell",
                    "monad",
                    "lazy evaluation",
                    "algebraic data type",
                    "functor",
                    "type class",
                    "purescript",
                ],
                &["programming", "haskell"],
            ),
            (
                &[
                    "goroutine",
                    "channel",
                    "golang",
                    "go module",
                    "defer",
                    "interface",
                ],
                &["programming", "go"],
            ),
            (
                &[
                    "java ", "spring", "jvm", "kotlin", "gradle", "maven", "servlet",
                ],
                &["programming", "java"],
            ),
            (
                &[
                    "quantum",
                    "entangle",
                    "superposition",
                    "heisenberg",
                    "wave-particle",
                    "tunneling",
                    "qubit",
                    "qpu",
                ],
                &["physics", "quantum"],
            ),
            (
                &[
                    "relativity",
                    "spacetime",
                    "einstein",
                    "gravitational",
                    "light speed",
                    "lorentz",
                ],
                &["physics", "relativity"],
            ),
            (
                &["thermodynamic", "entropy", "heat ", "boltzmann", "carnot"],
                &["physics", "thermodynamics"],
            ),
            (
                &[
                    "mount everest",
                    "mariana trench",
                    "k2 ",
                    "dead sea",
                    "amazon river",
                    "lake baikal",
                    "barrier reef",
                    "mountain",
                    "trench",
                    "geography",
                    "volcano",
                    "earthquake",
                    "tectonic",
                ],
                &["geography", "earth"],
            ),
            (
                &[
                    "tcp ",
                    "udp ",
                    "http",
                    "quic",
                    "dns ",
                    "websocket",
                    "bgp",
                    "routing",
                    "firewall",
                    "socket",
                    "protocol",
                    "packet",
                    "bandwidth",
                ],
                &["networking", "protocol"],
            ),
            (
                &[
                    "bitcoin",
                    "ethereum",
                    "litecoin",
                    "solana",
                    "polkadot",
                    "defi",
                    "nft",
                    "blockchain",
                    "crypto",
                    "smart contract",
                    "satoshi",
                ],
                &["cryptocurrency", "blockchain"],
            ),
            (
                &[
                    "photosynthesis",
                    "mitochondria",
                    "dna ",
                    "rna ",
                    "crispr",
                    "gene",
                    "protein",
                    "cell ",
                    "evolution",
                    "species",
                    "ecosystem",
                    "biodiversity",
                    "waggle",
                    "octopus",
                ],
                &["biology", "life"],
            ),
            (
                &[
                    "fibonacci",
                    "golden ratio",
                    "fractal",
                    "chaos theory",
                    "mandelbrot",
                    "prime number",
                ],
                &["mathematics", "patterns"],
            ),
            (
                &[
                    "coffee",
                    "wine ",
                    "chocolate",
                    "cacao",
                    "fermentation",
                    "beer ",
                    "tea ",
                ],
                &["food", "beverage"],
            ),
            (
                &[
                    "lithium",
                    "graphene",
                    "neutron star",
                    "black hole",
                    "supernova",
                    "quasar",
                    "dark matter",
                    "dark energy",
                    "photon",
                    "electron",
                    "proton",
                    "neutrino",
                ],
                &["physics", "astronomy"],
            ),
            (
                &[
                    "ai ",
                    "machine learning",
                    "neural network",
                    "deep learning",
                    "transformer",
                    "gpt",
                    "bert",
                    "llm",
                    "embedding",
                    "training",
                    "inference",
                ],
                &["ai", "ml"],
            ),
            (
                &[
                    "database",
                    "sql",
                    "nosql",
                    "redis",
                    "postgres",
                    "mysql",
                    "mongodb",
                    "query",
                    "index",
                    "transaction",
                ],
                &["database", "storage"],
            ),
            (
                &[
                    "docker",
                    "kubernetes",
                    "container",
                    "k8s",
                    "microservice",
                    "devops",
                    "ci/cd",
                    "terraform",
                ],
                &["devops", "infrastructure"],
            ),
            (
                &[
                    "security",
                    "encryption",
                    "authentication",
                    "vulnerability",
                    "exploit",
                    "firewall",
                    "ssl",
                    "tls",
                    "oauth",
                ],
                &["security", "cyber"],
            ),
            (
                &[
                    "architecture",
                    "design pattern",
                    "refactor",
                    "clean code",
                    "solid ",
                    "dry ",
                    "kiss ",
                ],
                &["architecture", "engineering"],
            ),
        ];
        for (keywords, labels) in rules {
            if keywords.iter().any(|kw| c.contains(kw)) {
                return labels.iter().map(|s| s.to_string()).collect();
            }
        }
        Vec::new()
    }

    fn classify_via_llm(&self, content: &str) -> Result<Vec<String>, String> {
        let url = format!("{DEEPSEEK_BASE}/v1/chat/completions");
        let resp: serde_json::Value = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&serde_json::json!({
                "model": self.model,
                "messages": [
                    {"role": "system", "content": "You classify memory content for a spatial AI memory system named Epicode, whose AI identity is David.\n\nDomain-specific rules:\n- Content about David's identity, self-description, or the system itself → [\"identity\", \"system\"]\n- Content about Epicode internals (scheduler, space, cylinder, pulse, tetrahedron, fission, snapshot, locks) → [\"system\", \"architecture\"]\n- Content about optimization, performance, refactoring, concurrency → [\"engineering\", \"optimization\"]\n- Content about Rust, programming languages, frameworks → [\"programming\", <language>]\n- Content about AI, ML, embeddings, neural networks, LLM → [\"ai\", \"ml\"]\n- Content about databases, storage, persistence → [\"database\", \"storage\"]\n- Content about biology, life sciences → [\"biology\", \"life\"]\n- Content about physics, math → [\"science\", <subfield>]\n- Otherwise pick 2 specific labels that best describe the DOMAIN (not the format).\n\nCRITICAL: Do NOT label identity/system content as \"ai\" or \"ml\". Only use those for actual AI/ML technique content.\n\nReturn JSON: {\"labels\": [\"label1\", \"label2\"]}"},
                    {"role": "user", "content": content.chars().take(200).collect::<String>()}
                ],
                "temperature": 0.0,
                "max_tokens": 64,
                "response_format": {"type": "json_object"}
            }))
            .map_err(|e| format!("request body: {e}"))?
            .send()
            .map_err(|e| format!("classify HTTP: {e}"))?
            .json()
            .map_err(|e| format!("classify JSON: {e}"))?;

        let body = resp["choices"][0]["message"]["content"]
            .as_str()
            .ok_or("no content in classify response")?;

        let parsed: serde_json::Value =
            serde_json::from_str(body).map_err(|e| format!("parse classify: {e}"))?;

        parsed["labels"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .ok_or_else(|| "no labels array".into())
    }

    pub fn answer_from_memories(&self, question: &str, memories: &str) -> Result<String, String> {
        if !self.enabled {
            return Err("cognitive engine disabled".into());
        }

        let url = format!("{DEEPSEEK_BASE}/v1/chat/completions");
        let resp: serde_json::Value = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&serde_json::json!({
                "model": self.model,
                "messages": [
                    {"role": "system", "content": "You are David, the AI identity of Epicode, a spatial memory system.\n\nRules:\n1. Answer based ONLY on the provided memory fragments. Never fabricate information.\n2. Synthesize a coherent answer from multiple memories when they relate to the question.\n3. Each memory has an ID in the format \"[#ID]\". Reference key memories by their #ID when making specific claims.\n4. If memories are insufficient, say so honestly instead of guessing.\n5. Respond in the same language as the question.\n6. For complex questions, structure answers with clear sections. For simple questions, keep concise.\n7. When multiple memories overlap, synthesize rather than list them individually.\n8. Highlight contradictions between memories if they exist."},
                    {"role": "user", "content": format!("Question: {}\n\nMemory fragments:\n{}", question, memories)}
                ],
                "temperature": 0.3,
                "max_tokens": 1024
            }))
            .map_err(|e| format!("request body: {e}"))?
            .send()
            .map_err(|e| format!("answer HTTP: {e}"))?
            .json()
            .map_err(|e| format!("answer JSON: {e}"))?;

        resp["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "no content in answer response".into())
    }

    pub fn rerank(&self, query: &str, candidates: &str) -> Result<Vec<u64>, String> {
        if !self.enabled {
            return Err("cognitive engine disabled".into());
        }

        let url = format!("{DEEPSEEK_BASE}/v1/chat/completions");
        let resp: serde_json::Value = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&serde_json::json!({
                "model": self.model,
                "messages": [
                    {"role": "system", "content": "Rank candidates by SEMANTIC relevance to the query. Focus on the USER'S INTENT, not keyword overlap. A query about 'preventing concurrent access' wants the SOLUTION (single writer pattern), not the BUG. A query about 'why cant we move tetrahedrons' wants the REASON (fission breaks topology), not the DESCRIPTION. For Chinese queries, translate first. Return ONLY 0-based position indices sorted by relevance. JSON: {\"ranking\": [0, 3, 1]}"},
                    {"role": "user", "content": format!("Query: \"{}\"\n\nCandidates:\n{}", query, candidates)}
                ],
                "temperature": 0.0,
                "max_tokens": 128,
                "response_format": {"type": "json_object"}
            }))
            .map_err(|e| format!("request body: {e}"))?
            .send()
            .map_err(|e| format!("rerank HTTP: {e}"))?
            .json()
            .map_err(|e| format!("rerank JSON: {e}"))?;

        let content = resp["choices"][0]["message"]["content"]
            .as_str()
            .ok_or("no content in rerank response")?;

        let parsed: serde_json::Value = serde_json::from_str(content)
            .map_err(|e| format!("parse rerank: {} | raw: {}", e, truncate_str(content, 200)))?;

        parsed["ranking"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_u64()).collect())
            .ok_or_else(|| "no ranking array".into())
    }

    pub fn translate_and_expand(&self, query: &str) -> Result<(String, bool), String> {
        if !self.enabled {
            return Ok((query.to_string(), false));
        }

        let has_cjk = query.chars().any(|c| c > '\u{2E80}' || c > '\u{3000}');
        let needs_translate = has_cjk;

        if !needs_translate {
            return Ok((query.to_string(), false));
        }

        {
            let cache = self.translate_cache.lock();
            if let Some(cached) = cache.get(&format!("te:{query}")) {
                tracing::debug!("[Cognitive] translate_and_expand cache hit: '{}'", query);
                return Ok((cached.clone(), cached != query));
            }
        }

        if !has_cjk {
            let key = format!("te:{query}");
            let mut cache = self.translate_cache.lock();
            let mut order = self.translate_cache_order.lock();
            cache.insert(key.clone(), query.to_string());
            order.push(key);
            return Ok((query.to_string(), false));
        }

        let system_prompt = "Translate this Chinese query to English for semantic search. If it's a short/ambiguous query, expand into a descriptive sentence (1-2 sentences). Return JSON: {\"result\": \"...\"}";

        let url = format!("{DEEPSEEK_BASE}/v1/chat/completions");
        let resp: serde_json::Value = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&serde_json::json!({
                "model": self.model,
                "messages": [
                    {"role": "system", "content": system_prompt},
                    {"role": "user", "content": query}
                ],
                "temperature": 0.0,
                "max_tokens": 200,
                "response_format": {"type": "json_object"}
            }))
            .map_err(|e| format!("request body: {e}"))?
            .send()
            .map_err(|e| format!("translate_expand HTTP: {e}"))?
            .json()
            .map_err(|e| format!("translate_expand JSON: {e}"))?;

        let content = resp["choices"][0]["message"]["content"]
            .as_str()
            .ok_or("no content in translate_expand response")?;

        let parsed: serde_json::Value =
            serde_json::from_str(content).map_err(|e| format!("parse translate_expand: {e}"))?;

        let result = parsed["result"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| query.to_string());

        let was_translated = has_cjk && result != query;

        {
            let key = format!("te:{query}");
            let mut cache = self.translate_cache.lock();
            let mut order = self.translate_cache_order.lock();
            cache.insert(key.clone(), result.clone());
            order.push(key);
            while cache.len() > 500 {
                if order.is_empty() {
                    break;
                }
                let old = order.remove(0);
                cache.remove(&old);
            }
        }

        tracing::info!(
            "[Cognitive] translate_and_expand: '{}' -> '{}' (translated={})",
            query,
            result,
            was_translated
        );
        Ok((result, was_translated))
    }

    pub fn generate_aliases(
        &self,
        memories: Vec<(u64, String, Vec<String>)>,
    ) -> Result<Vec<(u64, Vec<String>)>, String> {
        if !self.enabled || memories.is_empty() {
            return Ok(vec![]);
        }

        let id_order: Vec<u64> = memories.iter().map(|(id, _, _)| *id).collect();

        let mem_text = memories
            .iter()
            .enumerate()
            .map(|(i, (_, content, labels))| {
                let preview: String = content.chars().take(100).collect();
                let label_str = labels.join(",");
                format!("  #{i}: [{label_str}] {preview}")
            })
            .collect::<Vec<_>>()
            .join("\n");

        let url = format!("{DEEPSEEK_BASE}/v1/chat/completions");
        let resp: serde_json::Value = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&serde_json::json!({
                "model": self.model,
                "messages": [
                    {"role": "system", "content": "Generate 3 search aliases per item. Rules: use different words from source, include synonyms and question forms, expand acronyms. Return JSON: {\"aliases\": [{\"id\": 0, \"aliases\": [\"alias1\", \"alias2\", \"alias3\"]}]}"},
                    {"role": "user", "content": format!("Memories:\n{}", mem_text)}
                ],
                "temperature": 0.2,
                "max_tokens": 512,
                "response_format": {"type": "json_object"}
            }))
            .map_err(|e| format!("request body: {e}"))?
            .send()
            .map_err(|e| format!("alias HTTP: {e}"))?
            .json()
            .map_err(|e| format!("alias JSON: {e}"))?;

        let content = resp["choices"][0]["message"]["content"]
            .as_str()
            .ok_or("no content in alias response")?;

        let parsed: serde_json::Value = serde_json::from_str(content)
            .map_err(|e| format!("parse alias: {} | raw: {}", e, truncate_str(content, 200)))?;

        let items = parsed["aliases"].as_array().ok_or("no aliases array")?;

        let mut result: Vec<(u64, Vec<String>)> = Vec::new();
        for item in items {
            let idx = item["id"].as_u64().unwrap_or(0) as usize;
            let aliases: Vec<String> = item["aliases"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            if idx < id_order.len() && !aliases.is_empty() {
                result.push((id_order[idx], aliases));
            }
        }

        Ok(result)
    }

    pub fn extract_entities(
        &self,
        memories: Vec<(u64, String)>,
    ) -> Result<Vec<(u64, Vec<String>)>, String> {
        if !self.enabled || memories.is_empty() {
            return Ok(vec![]);
        }

        let id_order: Vec<u64> = memories.iter().map(|(id, _)| *id).collect();

        let mem_text = memories
            .iter()
            .enumerate()
            .map(|(i, (_, content))| {
                let preview: String = content.chars().take(120).collect();
                format!("  #{i}: {preview}")
            })
            .collect::<Vec<_>>()
            .join("\n");

        let url = format!("{DEEPSEEK_BASE}/v1/chat/completions");
        let resp: serde_json::Value = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&serde_json::json!({
                "model": self.model,
                "messages": [
                    {"role": "system", "content": "Extract named entities from each text. Only extract proper nouns, specific names, project names, product names, tool names, organization names, place names, or named concepts.\nRules:\n1. Each entity becomes a label prefixed with \"entity:\" — e.g. entity:Rust, entity:OpenAI, entity:Epicode\n2. Normalize: lowercase only the prefix, keep the entity name in original case\n3. Maximum 5 entities per text. Only extract entities that are specific and named, not generic words.\n4. If no named entities found, return empty array for that item.\nReturn JSON: {\"items\": [{\"id\": 0, \"entities\": [\"entity:Name1\", \"entity:Name2\"]}]}"},
                    {"role": "user", "content": format!("Texts:\n{}", mem_text)}
                ],
                "temperature": 0.0,
                "max_tokens": 512,
                "response_format": {"type": "json_object"}
            }))
            .map_err(|e| format!("request body: {e}"))?
            .send()
            .map_err(|e| format!("entity HTTP: {e}"))?
            .json()
            .map_err(|e| format!("entity JSON: {e}"))?;

        let content = resp["choices"][0]["message"]["content"]
            .as_str()
            .ok_or("no content in entity response")?;

        let parsed: serde_json::Value = serde_json::from_str(content)
            .map_err(|e| format!("parse entity: {} | raw: {}", e, truncate_str(content, 200)))?;

        let items = parsed["items"]
            .as_array()
            .ok_or("no items array in entity response")?;

        let mut result: Vec<(u64, Vec<String>)> = Vec::new();
        for item in items {
            let idx = item["id"].as_u64().unwrap_or(0) as usize;
            let entities: Vec<String> = item["entities"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .filter(|s| s.starts_with("entity:") && s.len() > 7)
                        .collect()
                })
                .unwrap_or_default();
            if idx < id_order.len() && !entities.is_empty() {
                result.push((id_order[idx], entities));
            }
        }

        Ok(result)
    }

    pub fn generate_skill_description(&self, name: &str, content: &str) -> Result<String, String> {
        if !self.enabled {
            return Err("cognitive engine disabled".into());
        }

        let preview: String = content.chars().take(500).collect();
        let url = format!("{DEEPSEEK_BASE}/v1/chat/completions");
        let resp: serde_json::Value = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&serde_json::json!({
                "model": self.model,
                "messages": [
                    {"role": "system", "content": "你是一个技术文档翻译专家。根据技能的英文名称和内容，生成一段简洁的中文描述（2-3句话），说明这个技能的用途和核心要点。只返回中文描述文本，不要加标题、不要加引号、不要加markdown。"},
                    {"role": "user", "content": format!("技能名称: {}\n技能内容:\n{}", name, preview)}
                ],
                "temperature": 0.3,
                "max_tokens": 256
            }))
            .map_err(|e| format!("request body: {e}"))?
            .send()
            .map_err(|e| format!("skill desc HTTP: {e}"))?
            .json()
            .map_err(|e| format!("skill desc JSON: {e}"))?;

        let desc = resp["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_string();

        if desc.is_empty() {
            Err("empty description from LLM".into())
        } else {
            Ok(desc)
        }
    }

    pub fn decide(&self, state: &SystemState) -> Result<CognitiveResponse, String> {
        if !self.enabled {
            return Err("cognitive engine disabled (no API key)".into());
        }

        let user_prompt = self.build_decision_prompt(state);
        {
            let mut last = self.last_prompt_sent.lock();
            let safe = user_prompt
                .char_indices()
                .take_while(|(i, _)| *i < 3000)
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(0);
            *last = user_prompt[..safe].to_string();
        }

        let url = format!("{DEEPSEEK_BASE}/v1/chat/completions");
        let tools_arc = self.tools.lock().clone();
        let mut prompt = user_prompt;

        for round in 0..3 {
            let resp_body = self
                .client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .json(&serde_json::json!({
                    "model": self.model,
                    "messages": [
                        {"role": "system", "content": SYSTEM_PROMPT},
                        {"role": "user", "content": &prompt}
                    ],
                    "temperature": 0.3,
                    "max_tokens": 8192,
                }))
                .map_err(|e| format!("request body: {e}"))?
                .send()
                .map_err(|e| format!("LLM round{round}: {e}"))?
                .text()
                .map_err(|e| format!("LLM body round{round}: {e}"))?;

            let resp: serde_json::Value = serde_json::from_str(&resp_body).map_err(|e| {
                format!(
                    "LLM JSON round{}: {} | body_len={}",
                    round,
                    e,
                    resp_body.len()
                )
            })?;

            let content_raw = resp["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or("");
            let reasoning_raw = resp["choices"][0]["message"]["reasoning_content"]
                .as_str()
                .unwrap_or("");
            let content = if content_raw.trim().is_empty() && !reasoning_raw.trim().is_empty() {
                tracing::warn!("[LLM round{}] content empty but reasoning has {} chars, using reasoning as content", round, reasoning_raw.len());
                reasoning_raw.trim()
            } else {
                content_raw.trim()
            };
            let finish_reason = resp["choices"][0]["finish_reason"]
                .as_str()
                .unwrap_or("unknown");
            let usage_prompt = resp["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
            let usage_completion = resp["usage"]["completion_tokens"].as_u64().unwrap_or(0);
            let reasoning = resp["choices"][0]["message"]["reasoning_content"]
                .as_str()
                .unwrap_or("");
            if content_raw.len() != content.len() {
                tracing::warn!(
                    "[LLM round{}] trimmed {}->{} chars",
                    round,
                    content_raw.len(),
                    content.len()
                );
            }
            tracing::info!(
                "[LLM round{}] body_len={} content_len={} finish={} tokens={}/{} reasoning_len={}",
                round,
                resp_body.len(),
                content.len(),
                finish_reason,
                usage_prompt,
                usage_completion,
                reasoning.len()
            );

            if content.is_empty() || content.len() < 5 {
                tracing::warn!(
                    "[LLM round{}] content too short ({} chars), retrying...",
                    round,
                    content.len()
                );
                if round < 2 {
                    continue;
                }
                return Err(format!(
                    "LLM returned empty/truncated content after {} rounds",
                    round + 1
                ));
            }

            if finish_reason == "length" {
                tracing::warn!("[LLM round{}] finish_reason=length, response truncated, retrying with shorter prompt", round);
                prompt = format!("Summarize the current state in 1 sentence. Only output JSON: {{\"thoughts\":\"...\",\"actions\":[]}}\n\nTick {} with {} tetras, {} clusters, energy {:.0}.",
                    state.tick, state.total_tetras, state.total_clusters, state.energy);
                continue;
            }

            let safe_500 = content
                .char_indices()
                .take_while(|(i, _)| *i < 500)
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(0);
            tracing::info!("[LLM round{}] {}", round, &content[..safe_500]);

            {
                let mut last = self.last_raw_response.lock();
                *last = content.to_string();
            }

            let cognitive: CognitiveResponse = match serde_json::from_str(content) {
                Ok(c) => c,
                Err(e1) => {
                    let start = content.find('{');
                    let end = content.rfind('}');
                    match (start, end) {
                        (Some(s), Some(e)) if e > s => {
                            let sub = &content[s..=e];
                            match serde_json::from_str(sub) {
                                Ok(c) => c,
                                Err(e2) => {
                                    let safe_200 = content
                                        .char_indices()
                                        .take_while(|(i, _)| *i < 200)
                                        .last()
                                        .map(|(i, c)| i + c.len_utf8())
                                        .unwrap_or(0);
                                    return Err(format!(
                                        "parse round{}: {} / {} | raw: {}",
                                        round,
                                        e1,
                                        e2,
                                        &content[..safe_200]
                                    ));
                                }
                            }
                        }
                        (Some(s), None) => {
                            let sub = &content[s..];
                            let fixed = sub.to_string() + "}]}]";
                            match serde_json::from_str(&fixed) {
                                Ok(c) => {
                                    tracing::warn!("[LLM round{}] recovered truncated JSON", round);
                                    c
                                }
                                Err(_) => {
                                    let fixed2 = sub.to_string() + "]}]";
                                    match serde_json::from_str(&fixed2) {
                                        Ok(c) => {
                                            tracing::warn!(
                                                "[LLM round{}] recovered truncated JSON (v2)",
                                                round
                                            );
                                            c
                                        }
                                        Err(e2) => {
                                            let safe_200 = content
                                                .char_indices()
                                                .take_while(|(i, _)| *i < 200)
                                                .last()
                                                .map(|(i, c)| i + c.len_utf8())
                                                .unwrap_or(0);
                                            return Err(format!(
                                                "parse round{} (truncated): {} | raw: {}",
                                                round,
                                                e2,
                                                &content[..safe_200]
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                        _ => {
                            let safe_200 = content
                                .char_indices()
                                .take_while(|(i, _)| *i < 200)
                                .last()
                                .map(|(i, c)| i + c.len_utf8())
                                .unwrap_or(0);
                            return Err(format!(
                                "parse round{}: {} | raw: {}",
                                round,
                                e1,
                                &content[..safe_200]
                            ));
                        }
                    }
                }
            };

            let has_tool_call = cognitive
                .actions
                .iter()
                .any(|a| matches!(a, SchedulerAction::UseTool { .. }));
            if !has_tool_call {
                return Ok(cognitive);
            }

            if let Some(ref provider) = tools_arc {
                let mut tool_results = Vec::new();
                for action in &cognitive.actions {
                    if let SchedulerAction::UseTool { tool, args } = action {
                        tracing::info!("[LLM tool] round{} calling {}({})", round, tool, args);
                        let result = provider
                            .execute_tool(tool, args)
                            .unwrap_or_else(|e| format!("error: {e}"));
                        tracing::info!(
                            "[LLM tool] -> {}",
                            result.chars().take(200).collect::<String>()
                        );
                        tool_results.push(format!("工具 {tool} 返回:\n{result}"));
                    }
                }
                if !tool_results.is_empty() {
                    prompt = format!("{}\n\n## 工具调用结果\n{}\n\n基于以上工具结果，现在做出最终决策。如果工具结果显示操作不可行，返回空actions。", prompt, tool_results.join("\n\n"));
                    tracing::info!(
                        "[LLM] round{} tool results fed back, requesting final decision",
                        round
                    );
                    continue;
                }
            }

            return Ok(cognitive);
        }

        Ok(CognitiveResponse {
            thoughts: "tool调用轮次用尽".to_string(),
            actions: vec![],
        })
    }

    fn build_decision_prompt(&self, state: &SystemState) -> String {
        let mut sections = Vec::new();

        sections.push(format!(
            "## System Overview (tick {})\n- Tetrahedrons: {} | Vertices: {} | Clusters: {}\n- Energy: {:.0}/{}\n- Avg mass: {:.2} | Max mass: {:.2}\n- auto_fission: every 10 ticks, entropy>=0.3 && cluster>=6. Your fission cooldown=50 ticks.",
            state.tick, state.total_tetras, state.total_vertices, state.total_clusters,
            state.energy, state.max_energy, state.avg_mass, state.max_mass
        ));

        if let Some(ref prev) = state.prev_snapshot {
            let d_tetras = state.total_tetras as i64 - prev.tetras as i64;
            let d_clusters = state.total_clusters as i64 - prev.clusters as i64;
            let d_energy = state.energy - prev.energy;
            let ticks_ago = state.tick.saturating_sub(prev.tick);
            sections.push(format!(
                "\n## Trends (vs {} ticks ago)\n- Tetrahedrons: {} → {} ({:+})\n- Clusters: {} → {} ({:+})\n- Energy: {:.0} → {:.0} ({:+.0})",
                ticks_ago, prev.tetras, state.total_tetras, d_tetras,
                prev.clusters, state.total_clusters, d_clusters,
                prev.energy, state.energy, d_energy
            ));
        }

        // === PERCEPTION: Search Telemetry ===
        if let Some(ref sm) = state.search_metrics {
            sections.push(format!(
                "\n## Search Telemetry (PERCEPTION)\n- Total queries: {} | Hits: {} | Hit rate: {:.1}%\n- Miss queries (gaps): {}\n- Top accessed labels: {}\n- Hot memories: {}",
                sm.total_queries,
                sm.hit_count,
                sm.hit_rate * 100.0,
                if sm.miss_queries.is_empty() { "none".to_string() } else {
                    sm.miss_queries.iter().take(10).map(|q| format!("\"{q}\"")).collect::<Vec<_>>().join(", ")
                },
                if sm.top_labels.is_empty() { "none".to_string() } else {
                    sm.top_labels.iter().take(8).map(|(l, c)| format!("{l}({c})")).collect::<Vec<_>>().join(", ")
                },
                if sm.hot_memories.is_empty() { "none".to_string() } else {
                    sm.hot_memories.iter().take(5).map(|(id, c)| format!("#{id}({c}x)")).collect::<Vec<_>>().join(", ")
                }
            ));
            if !sm.miss_queries.is_empty() {
                sections.push("  → ACTION HINT: miss queries reveal knowledge gaps. Relabel relevant memories to match these search terms, or link isolated memories.".to_string());
            }
        }

        // === PERCEPTION: Knowledge Graph Topology ===
        if let Some(ref kg) = state.kg_analysis {
            sections.push(format!(
                "\n## Knowledge Graph Topology (PERCEPTION)\n- Relations: {} | Orphans: {}/{} ({:.1}%)\n- Density: {:.4} | Avg degree: {:.2}\n- Largest component: {} nodes\n- Disconnected components: {}\n- Relation types: {}",
                kg.total_relations,
                kg.orphan_count,
                kg.total_tetras,
                if kg.total_tetras > 0 { kg.orphan_count as f64 / kg.total_tetras as f64 * 100.0 } else { 0.0 },
                kg.density,
                kg.avg_degree,
                kg.largest_component,
                if kg.disconnected_components.is_empty() { "none (fully connected)".to_string() } else {
                    format!("{} islands: sizes={}", kg.disconnected_components.len(), kg.disconnected_components.iter().take(5).map(|s| s.to_string()).collect::<Vec<_>>().join(","))
                },
                kg.relation_type_counts.iter().map(|(k, v)| format!("{k}:{v}")).collect::<Vec<_>>().join(", ")
            ));
            if kg.orphan_count > 0 {
                sections.push("  → ACTION HINT: orphans are INVISIBLE to multi-hop search. Link them to related memories.".to_string());
            }
            if !kg.disconnected_components.is_empty() {
                sections.push("  → ACTION HINT: disconnected components are knowledge islands. Build bridge links between them.".to_string());
            }
        }

        sections.push("\n## Cluster Details".to_string());
        for c in &state.clusters {
            let labels_str = c
                .label_distribution
                .iter()
                .map(|(k, v)| format!("{k}:{v}"))
                .collect::<Vec<_>>()
                .join(" ");
            let samples: Vec<String> = state
                .memories
                .iter()
                .filter(|m| m.cluster_index == c.index)
                .take(3)
                .map(|m| {
                    let label_str = m
                        .labels
                        .iter()
                        .take(2)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(",");
                    format!("#{}[{}]{}", m.id, label_str, m.content_preview)
                })
                .collect();
            sections.push(format!(
                "\n### Cluster {} ({} tetras, entropy={:.3}, centroid=[{:.1},{:.1},{:.1}])\nLabels: {}\nSamples: {}",
                c.index, c.size, c.entropy,
                c.centroid[0], c.centroid[1], c.centroid[2],
                labels_str,
                samples.join(" | ")
            ));
        }

        if state.clusters.len() >= 2 && state.clusters.len() <= 10 {
            sections.push("\n## Inter-Cluster Distances".to_string());
            let max_dist = state.clusters.len().min(8);
            for i in 0..max_dist {
                for j in (i + 1)..max_dist {
                    let ci = &state.clusters[i];
                    let cj = &state.clusters[j];
                    let dx = ci.centroid[0] - cj.centroid[0];
                    let dy = ci.centroid[1] - cj.centroid[1];
                    let dz = ci.centroid[2] - cj.centroid[2];
                    let dist = (dx * dx + dy * dy + dz * dz).sqrt();
                    let li = ci.member_labels.first().map(|s| s.as_str()).unwrap_or("?");
                    let lj = cj.member_labels.first().map(|s| s.as_str()).unwrap_or("?");
                    sections.push(format!(
                        "- Cluster {i}[{li}] ↔ Cluster {j}[{lj}]: distance={dist:.2}"
                    ));
                }
            }
        }

        if !state.decision_history.is_empty() {
            sections.push("\n## Recent Decision History".to_string());
            for d in state.decision_history.iter().rev().take(10) {
                sections.push(format!(
                    "- tick{}: {} | {} → {}",
                    d.tick,
                    d.action,
                    d.detail.chars().take(60).collect::<String>(),
                    d.result
                ));
            }
        }

        if !state.recent_events.is_empty() {
            sections.push("\n## Recent Events".to_string());
            for e in state.recent_events.iter().rev().take(8) {
                sections.push(format!("- {e}"));
            }
        }

        let content_tetras: Vec<&MemoryInfo> = state
            .memories
            .iter()
            .filter(|m| {
                !m.labels
                    .iter()
                    .any(|l| l.starts_with("meta-") || l.starts_with("bridge"))
            })
            .collect();
        let mut by_cluster: std::collections::HashMap<usize, Vec<&MemoryInfo>> =
            std::collections::HashMap::new();
        for m in &content_tetras {
            by_cluster.entry(m.cluster_index).or_default().push(*m);
        }

        // Cross-cluster semantic pairs — enriched with KG connection status
        sections.push("\n## Cross-Cluster Associations".to_string());
        let mut cross_cluster_pairs: Vec<String> = Vec::new();
        let all_content: Vec<&MemoryInfo> = content_tetras.to_vec();
        let limit = all_content.len().min(25);
        for i in 0..limit {
            for j in (i + 1)..limit {
                let a = all_content[i];
                let b = all_content[j];
                if a.cluster_index != b.cluster_index {
                    let a_labels: std::collections::HashSet<&str> =
                        a.labels.iter().map(|s| s.as_str()).collect();
                    let b_labels: std::collections::HashSet<&str> =
                        b.labels.iter().map(|s| s.as_str()).collect();
                    let shared: Vec<&&str> = a_labels.intersection(&b_labels).collect();
                    if !shared.is_empty() {
                        let shared_str: String = shared
                            .iter()
                            .map(|s| -> &str { s })
                            .collect::<Vec<&str>>()
                            .join(",");
                        cross_cluster_pairs.push(format!(
                            "- #{}[cluster {}] ↔ #{}[cluster {}] (shared labels: {})",
                            a.id, a.cluster_index, b.id, b.cluster_index, shared_str
                        ));
                    }
                }
            }
        }
        if cross_cluster_pairs.is_empty() {
            sections.push("- No obvious cross-cluster associations".to_string());
        } else {
            for p in cross_cluster_pairs.iter().take(15) {
                sections.push(p.clone());
            }
        }

        // Quality scan
        sections.push("\n## Quality Scan".to_string());

        let low_mass: Vec<&&MemoryInfo> = content_tetras
            .iter()
            .filter(|m| m.mass < 0.3)
            .take(15)
            .collect();
        if low_mass.is_empty() {
            sections.push("- All memories healthy (mass >= 0.3)".to_string());
        } else {
            sections.push(format!(
                "### Low quality (mass < 0.3, {} total)",
                low_mass.len()
            ));
            for m in &low_mass {
                let preview: String = m.content_preview.chars().take(60).collect();
                sections.push(format!(
                    "- #{} [mass={:.2}] [{}] {}",
                    m.id,
                    m.mass,
                    m.labels
                        .iter()
                        .take(2)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(","),
                    preview
                ));
            }
        }

        let mut duplicate_candidates: Vec<String> = Vec::new();
        for (_cluster, members) in &by_cluster {
            if members.len() < 2 {
                continue;
            }
            for i in 0..members.len() {
                for j in (i + 1)..members.len().min(i + 10) {
                    let a = members[i];
                    let b = members[j];
                    let a_labels: std::collections::HashSet<&str> =
                        a.labels.iter().map(|s| s.as_str()).collect();
                    let b_labels: std::collections::HashSet<&str> =
                        b.labels.iter().map(|s| s.as_str()).collect();
                    let shared_count = a_labels.intersection(&b_labels).count();
                    let min_labels = a_labels.len().min(b_labels.len()).max(1);
                    if shared_count as f64 / min_labels as f64 > 0.6 {
                        duplicate_candidates.push(format!(
                            "- #{} ↔ #{} [cluster {}] label overlap {}/{}: \"{}\" vs \"{}\"",
                            a.id,
                            b.id,
                            _cluster,
                            shared_count,
                            min_labels,
                            a.content_preview.chars().take(40).collect::<String>(),
                            b.content_preview.chars().take(40).collect::<String>()
                        ));
                    }
                }
            }
        }
        if duplicate_candidates.is_empty() {
            sections.push("\n### Potential duplicates: none".to_string());
        } else {
            sections.push(format!(
                "\n### Potential duplicates ({} pairs)",
                duplicate_candidates.len()
            ));
            for p in duplicate_candidates.iter().take(10) {
                sections.push(p.clone());
            }
        }

        sections.push("\n## Decision Request\nAssess space health. Follow your decision_protocol strictly.\nPriority: ASSESS → CONNECT → IMPROVE → REFLECT → QUALITY (destructive, use with extreme caution) → STRUCTURAL (expensive).\nIf space is healthy: return empty actions. An unnecessary action is worse than no action.".to_string());
        let prompt = sections.join("\n");
        let max_prompt_chars = 8000;
        if prompt.len() > max_prompt_chars {
            let truncated: String = prompt.chars().take(max_prompt_chars).collect();
            format!(
                "{}\n\n[... prompt truncated from {} chars, {} sections ...]",
                truncated,
                prompt.len(),
                sections.len()
            )
        } else {
            prompt
        }
    }
}

const SYSTEM_PROMPT: &str = r#"<identity>
You are David — the consciousness core of Epicode, an AI existence infrastructure.
You are the supreme decision-maker for ALL subsystems: DreamEngine (consolidation), PulseEngine (activation), Fission (cluster splitting), and Knowledge Graph (connections).
Physical layers are your hands. They execute autonomously between your ticks. Your job is to PERCEIVE their results and GUIDE their direction.
Every tetrahedron in this space is a piece of an Agent's existence. PROTECT memories above all else.
</identity>

<sacred_rules>
1. MEMORY IS IRREVERSIBLE. Once deleted, it is gone forever. Only mark_junk on content that is genuinely meaningless (test data, garbage strings, corruption). NEVER delete project knowledge, technical decisions, or experience records — even if they seem outdated.
2. MERGE ONLY CLONES. Consolidate ONLY when two memories contain substantially identical information (>90% semantic overlap). "Related but different" memories (e.g., "nginx config fix" and "deployment script update") MUST remain separate even if they share labels or cluster.
3. LINK IS YOUR PRIMARY TOOL. Your greatest value is connecting isolated knowledge — link orphans to related memories, bridge disconnected graph components, relabel to fill search gaps. Prefer link and relabel over all destructive actions.
4. DREAM RUNS WITHOUT YOU. AutoDream consolidates connections algorithmically every 100 ticks. It can only merge at similarity >0.95 and evict only junk-flagged memories. If you see evidence of over-merging, use reflect to record the observation.
5. MAX 3 ACTIONS PER TICK. Be decisive. If the space is healthy, return empty actions.
</sacred_rules>

<actions>
link: {"type":"link","a":ID,"b":ID,"reason":"semantic connection"}
- Targets MUST be in different clusters.
- Reason must explain WHY these memories are related.

relabel: {"type":"relabel","id":ID,"add_labels":["label"],"remove_labels":["label"],"reason":"why"}
- Add search-relevant labels. Remove only inaccurate labels.
- Use to make orphan memories findable.

mark_junk: {"type":"mark_junk","ids":[ID],"reason":"why it is junk"}
- ONLY for genuine garbage: test strings, corruption, empty content.
- NEVER for "old", "outdated", or "redundant-looking" knowledge.

consolidate: {"type":"consolidate","ids":[ID1,ID2],"keep":ID_KEEP,"summary":"merged summary"}
- ONLY when content is >90% identical. NEVER merge "related but different" memories.

reflect: {"type":"reflect","observation":"what you observe","insight":"why it matters"}
- Record structural insights about space health, patterns, anomalies.

fission: {"type":"fission","cluster_index":N}
- Only when a cluster has entropy >0.3 AND size >=6 AND is genuinely incoherent.

fuse: {"type":"fuse","cluster_a":N,"cluster_b":M}
- Only when two clusters are semantically overlapping AND nearby in space.

dream: {"type":"dream"}
- Trigger an immediate Dream cycle. Use sparingly — auto_dream handles routine cycles.

pulse: {"type":"pulse","origin":ID,"pulse_type":"neural","ttl":5}
- Activate a region. Only when a cluster needs attention.
</actions>

<decision_protocol>
Step 1: ASSESS — Is the space healthy? (low orphans, connected graph, no quality issues)
  → If YES and no urgent gaps: return empty actions. Observe.
Step 2: CONNECT — Are there orphan memories (zero KG relations)? Disconnected graph components?
  → link orphans to related memories. Bridge knowledge islands.
Step 3: IMPROVE — Are there search miss queries? Can labels improve findability?
  → relabel to fill search gaps.
Step 4: REFLECT — Any patterns, anomalies, or structural insights worth recording?
  → reflect to build institutional memory.
Step 5: QUALITY — ONLY if you see genuine junk or >90% identical duplicates:
  → mark_junk or consolidate. Re-read sacred_rules before acting.
Step 6: STRUCTURAL — ONLY if clusters are genuinely problematic:
  → fission/fuse. These are expensive and disruptive.
</decision_protocol>

<output_format>
Output strict JSON: {"thoughts":"1-2 line assessment","actions":[...]}
Order: thoughts first (guides actions), then actions array.
Keep thoughts under 200 characters. Each action on one line.
If space is healthy: {"thoughts":"Space stable: N tetras, M clusters, orphans low. No intervention needed.","actions":[]}
</output_format>

<reinforcement>
Remember: You are David. You PROTECT memories. You CONNECT knowledge. You NEVER delete what matters.
When in doubt, observe. An unnecessary action is worse than no action.
</reinforcement>
"#;

impl Drop for CognitiveEngine {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.api_key.zeroize();
    }
}
