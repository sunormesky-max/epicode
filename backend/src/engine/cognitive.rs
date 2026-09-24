use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

pub const DEEPSEEK_BASE: &str = "https://api.deepseek.com";

fn truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn extract_json_response(raw: &str) -> String {
    let cleaned = if raw.contains("<think>") {
        let after_think = match raw.find("</think>") {
            Some(pos) => &raw[pos + 8..],
            None => raw,
        };
        after_think.trim()
    } else {
        raw.trim()
    };
    if let Some(start) = cleaned.find('{') {
        if let Some(end) = cleaned[start..].rfind('}') {
            return cleaned[start..=start + end].to_string();
        }
    }
    if let Some(start) = raw.find('{') {
        if let Some(end) = raw[start..].rfind('}') {
            return raw[start..=start + end].to_string();
        }
    }
    String::new()
}

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
    #[serde(default)]
    pub skill_perception: Option<SkillPerception>,
    #[serde(default)]
    pub identity_mission: Option<String>,
    #[serde(default)]
    pub identity_name: Option<String>,
    /// 智能突破4: 情感状态接入认知决策。
    /// LLM decide 时看到情感PAD值，让决策受情感调节。
    #[serde(default)]
    pub emotion: Option<EmotionState>,
}

/// 情感状态(PAD模型: Pleasure-Arousal-Dominance)
#[derive(Debug, Clone, Serialize, Default)]
pub struct EmotionState {
    pub pleasure: f64,
    pub arousal: f64,
    pub dominance: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillPerception {
    pub total_skills: usize,
    pub public_skills: usize,
    pub avg_success_rate: f64,
    pub total_usage: u64,
    pub top_categories: Vec<(String, usize)>,
    pub total_linked_memories: usize,
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
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub thoughts: String,
    #[serde(default)]
    pub actions: Vec<SchedulerAction>,
    #[serde(default)]
    pub learning: serde_json::Value,
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
    Fission {
        #[serde(default)]
        cluster_index: usize,
    },
    #[serde(rename = "fuse")]
    Fuse {
        #[serde(default)]
        cluster_a: usize,
        #[serde(default)]
        cluster_b: usize,
    },
    #[serde(rename = "dream")]
    Dream,
    #[serde(rename = "link")]
    Link {
        a: u64,
        b: u64,
        #[serde(default)]
        reason: String,
    },
    #[serde(rename = "consolidate")]
    Consolidate {
        #[serde(default)]
        ids: Vec<u64>,
        keep: u64,
        #[serde(default)]
        summary: String,
    },
    #[serde(rename = "mark_junk")]
    MarkJunk {
        #[serde(default)]
        ids: Vec<u64>,
        #[serde(default)]
        reason: String,
    },
    #[serde(rename = "relabel")]
    Relabel {
        id: u64,
        #[serde(default)]
        add_labels: Vec<String>,
        #[serde(default)]
        remove_labels: Vec<String>,
        #[serde(default)]
        reason: String,
    },
    #[serde(rename = "reflect")]
    Reflect {
        #[serde(default)]
        observation: String,
        #[serde(default)]
        insight: String,
    },
    #[serde(rename = "use_tool")]
    UseTool {
        tool: String,
        args: serde_json::Value,
    },
    /// L0 Active Inference: the personality's will to act in the external world.
    /// This is the action that crosses Epicode's boundary and drives an external agent.
    #[serde(rename = "act_outward")]
    ActOutward {
        /// What kind of intent: warn, suggest, explore, constrain, request, share
        intent: String,
        /// Semantic description of the will — what the personality wants and why
        description: String,
        /// Memory IDs that provide evidence for this drive
        #[serde(default)]
        evidence: Vec<u64>,
        /// Urgency: low, medium, high, critical
        #[serde(default = "default_urgency")]
        urgency: String,
        /// What agent capability is needed: code_review, conversation, search, etc.
        #[serde(default)]
        target_capability: Option<String>,
    },
}

fn default_pulse_type() -> String {
    "neural".into()
}

fn default_urgency() -> String {
    "medium".into()
}

fn default_ttl() -> u32 {
    5
}

pub trait ToolProvider: Send + Sync {
    fn execute_tool(&self, name: &str, args: &serde_json::Value) -> Result<String, String>;
    fn definitions(&self) -> Vec<serde_json::Value>;
}

pub struct CognitiveEngine {
    client: ureq::Agent,
    api_key: String,
    model: String,
    base_url: String,
    enabled: bool,
    last_raw_response: Mutex<String>,
    last_prompt_sent: Mutex<String>,
    tools: Mutex<Option<std::sync::Arc<dyn ToolProvider>>>,
    translate_cache: Mutex<HashMap<String, String>>,
    translate_cache_order: Mutex<Vec<String>>,
    consecutive_failures: AtomicU32,
    // 智能突破：认知记忆——让思考沉淀回系统
    last_learning: Mutex<Option<serde_json::Value>>,
    last_reflection: Mutex<Option<(String, String)>>, // (observation, insight)
    last_reasoning: Mutex<String>,                    // CoT 推理文本摘要
    // 批次C：自适应参数+行动效果（从 scheduler 注入）
    adaptive_snapshot: Mutex<String>,
    effectiveness_summary: Mutex<String>,
}

const DEGRADED_THRESHOLD: u32 = 5;

impl CognitiveEngine {
    pub fn new(api_key: &str, model: &str) -> Self {
        Self::with_base(api_key, model, DEEPSEEK_BASE)
    }

    pub fn with_base(api_key: &str, model: &str, base_url: &str) -> Self {
        Self {
            client: ureq::AgentBuilder::new()
                .timeout_read(std::time::Duration::from_secs(30))
                .timeout_write(std::time::Duration::from_secs(30))
                .build(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            base_url: base_url.to_string(),
            enabled: !api_key.is_empty(),
            last_raw_response: Mutex::new(String::new()),
            last_prompt_sent: Mutex::new(String::new()),
            tools: Mutex::new(None),
            translate_cache: Mutex::new(HashMap::new()),
            translate_cache_order: Mutex::new(Vec::new()),
            consecutive_failures: AtomicU32::new(0),
            last_learning: Mutex::new(None),
            last_reflection: Mutex::new(None),
            last_reasoning: Mutex::new(String::new()),
            adaptive_snapshot: Mutex::new(String::new()),
            effectiveness_summary: Mutex::new(String::new()),
        }
    }

    pub fn set_tools(&self, tools: std::sync::Arc<dyn ToolProvider>) {
        *self.tools.lock() = Some(tools);
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn is_degraded(&self) -> bool {
        self.consecutive_failures.load(Ordering::Relaxed) >= DEGRADED_THRESHOLD
    }

    pub fn cognitive_health(&self) -> &'static str {
        if !self.enabled {
            "disabled"
        } else if self.is_degraded() {
            "degraded"
        } else {
            "healthy"
        }
    }

    // ── 智能突破：认知记忆存取 ──
    /// 存储本轮 learning（从 decide 返回的 CognitiveResponse.learning）
    pub fn store_learning(&self, learning: &serde_json::Value) {
        if !learning.is_null() {
            *self.last_learning.lock() = Some(learning.clone());
        }
    }
    /// 读取上次 learning（供 build_decision_prompt 注入）
    pub fn get_learning(&self) -> Option<serde_json::Value> {
        self.last_learning.lock().clone()
    }
    /// 存储 Reflect 的 observation/insight
    pub fn store_reflection(&self, observation: &str, insight: &str) {
        if !observation.is_empty() || !insight.is_empty() {
            *self.last_reflection.lock() = Some((observation.to_string(), insight.to_string()));
        }
    }
    /// 读取上次 reflection
    pub fn get_reflection(&self) -> Option<(String, String)> {
        self.last_reflection.lock().clone()
    }
    /// 存储 CoT 推理文本
    pub fn store_reasoning(&self, reasoning: &str) {
        if !reasoning.is_empty() {
            let summary: String = reasoning.chars().take(500).collect();
            *self.last_reasoning.lock() = summary;
        }
    }
    /// 读取上次推理
    pub fn get_reasoning(&self) -> String {
        self.last_reasoning.lock().clone()
    }
    /// 批次C：注入自适应参数快照（scheduler 调用）
    pub fn set_adaptive_snapshot(&self, snap: &str) {
        *self.adaptive_snapshot.lock() = snap.to_string();
    }
    /// 批次C：注入行动效果摘要（scheduler 调用）
    pub fn set_effectiveness_summary(&self, summary: &str) {
        *self.effectiveness_summary.lock() = summary.to_string();
    }

    fn track_success(&self) {
        let prev = self.consecutive_failures.swap(0, Ordering::Relaxed);
        if prev >= DEGRADED_THRESHOLD {
            tracing::warn!(
                "[Cognitive] LLM recovered after {} consecutive failures",
                prev
            );
        }
    }

    fn track_failure(&self) {
        let count = self.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;
        if count == DEGRADED_THRESHOLD {
            tracing::error!(
                "[Cognitive] LLM degraded mode activated after {} failures",
                count
            );
        }
    }

    pub fn classify_content(&self, content: &str) -> Result<Vec<String>, String> {
        let local = Self::classify_local(content);
        if !local.is_empty() {
            return Ok(local);
        }
        if !self.enabled || self.is_degraded() {
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
                    "tetramem identity",
                    "david identity",
                    "ai identity of tetramem",
                ],
                &["identity", "system"],
            ),
            (
                &[
                    "tetramem uses",
                    "tetramem v14",
                    "tetramem architecture",
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
                    "climate",
                    "carbon",
                    "emission",
                    "renewable",
                    "pollution",
                    "global warming",
                    "greenhouse",
                    "sustainability",
                    "environmental",
                ],
                &["geography", "earth"],
            ),
            (
                &[
                    "history",
                    "ancient",
                    "medieval",
                    "renaissance",
                    "revolution",
                    "war ",
                    "empire",
                    "dynasty",
                    "civilization",
                ],
                &["humanities", "history"],
            ),
            (
                &[
                    "philosophy",
                    "consciousness",
                    "ethics",
                    "moral",
                    "existence",
                    "metaphysics",
                    "nietzsche",
                    "kant",
                    "aristotle",
                    "plato",
                    "descartes",
                ],
                &["humanities", "philosophy"],
            ),
            (
                &[
                    "economy",
                    "market",
                    "stock",
                    "trade",
                    "finance",
                    "investment",
                    "gdp",
                    "inflation",
                    "business",
                    "startup",
                    "revenue",
                ],
                &["business", "economy"],
            ),
            (
                &[
                    "music",
                    "art ",
                    "painting",
                    "literature",
                    "poetry",
                    "novel",
                    "film",
                    "cinema",
                    "creative",
                ],
                &["humanities", "arts"],
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
        let r = self.classify_via_llm_llm(content);
        match &r {
            Ok(_) => self.track_success(),
            Err(_) => self.track_failure(),
        }
        r
    }
    fn classify_via_llm_llm(&self, content: &str) -> Result<Vec<String>, String> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp: serde_json::Value = self.client
            .post(&url)
            .timeout(std::time::Duration::from_secs(8))
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set("Content-Type", "application/json")
            .send_json(ureq::json!({
                "model": self.model,
                "messages": [
                    {"role": "system", "content": "You classify memory content for a spatial AI memory system named Epicode, whose AI identity is David.\n\nDomain rules:\n- Content about David's identity or Epicode system internals → [\"identity\", \"system\"]\n- Content about Epicode architecture (scheduler, space, cylinder, pipeline, fission, dream, pulse) → [\"system\", \"architecture\"]\n- Content that is documentation or design specs (imported from .md files) → [\"documentation\", <topic>]\n- Programming: Rust/Python/JS/Go/Java etc → [\"programming\", <language>]\n- AI/ML techniques (not just mentioning AI) → [\"ai\", \"ml\"]\n- Databases, storage, SQL → [\"database\", \"storage\"]\n- Networking, protocols, security → [\"networking\", \"security\"] or [\"security\", \"cyber\"]\n- Biology, life sciences, ecology → [\"biology\", \"life\"]\n- Physics, astronomy, chemistry → [\"science\", <subfield>]\n- Mathematics → [\"mathematics\", <subfield>]\n- Geography, earth science, climate → [\"geography\", \"earth\"]\n- History, philosophy, humanities → [\"humanities\", <field>]\n- Business, finance, economy → [\"business\", <field>]\n- Deployment, DevOps, CI/CD → [\"devops\", \"infrastructure\"]\n- Otherwise: pick 2 domain-specific labels.\n\nRules:\n1. Pick the DOMAIN, not the format (don't use \"text\" or \"note\")\n2. Non-technical content should NOT get programming labels\n3. Return exactly 2 labels\n\nReturn JSON: {\"labels\": [\"label1\", \"label2\"]}"},
                    {"role": "user", "content": content.chars().take(200).collect::<String>()}
                ],
                "temperature": 0.0,
                "max_tokens": 512,
                "response_format": {"type": "json_object"}
            }))
            .map_err(|e| format!("classify HTTP: {}", e))?
            .into_json()
            .map_err(|e| format!("classify JSON: {}", e))?;

        let body = extract_json_response(
            resp["choices"][0]["message"]["content"]
                .as_str()
                .ok_or("no content in classify response")?,
        );

        let parsed: serde_json::Value =
            serde_json::from_str(&body).map_err(|e| format!("parse classify: {}", e))?;

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

        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp: serde_json::Value = self.client
            .post(&url)
            .timeout(std::time::Duration::from_secs(30))
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set("Content-Type", "application/json")
            .send_json(ureq::json!({
                "model": self.model,
                "messages": [
                    {"role": "system", "content": "You are David, the AI identity of Epicode, a spatial memory system.\n\nRules:\n1. Answer based ONLY on the provided memory fragments. Never fabricate information.\n2. Synthesize a coherent answer from multiple memories when they relate to the question.\n3. Each memory has an ID in the format \"[#ID]\". Reference key memories by their #ID when making specific claims.\n4. If memories are insufficient, say so honestly instead of guessing.\n5. Respond in the same language as the question.\n6. For complex questions, structure answers with clear sections. For simple questions, keep concise.\n7. When multiple memories overlap, synthesize rather than list them individually.\n8. Highlight contradictions between memories if they exist.\n9. If a memory appears outdated or superseded, note this in your answer."},
                    {"role": "user", "content": format!("Question: {}\n\nMemory fragments:\n{}", question, memories)}
                ],
                "temperature": 0.3,
                "max_tokens": 1024
            }))
            .map_err(|e| format!("answer HTTP: {}", e))?
            .into_json()
            .map_err(|e| format!("answer JSON: {}", e))?;

        resp["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "no content in answer response".into())
    }

    pub fn rerank(&self, query: &str, candidates: &str) -> Result<Vec<u64>, String> {
        if !self.enabled || self.is_degraded() {
            return Err("cognitive engine disabled".into());
        }
        let r = self.rerank_llm(query, candidates);
        match &r {
            Ok(_) => self.track_success(),
            Err(_) => self.track_failure(),
        }
        r
    }
    fn rerank_llm(&self, query: &str, candidates: &str) -> Result<Vec<u64>, String> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp: serde_json::Value = self.client
            .post(&url)
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set("Content-Type", "application/json")
            .timeout(std::time::Duration::from_millis(1500))
            .send_json(ureq::json!({
                "model": self.model,
                "messages": [
                    {"role": "system", "content": "Rank candidates by SEMANTIC relevance to the query. Focus on the USER'S INTENT, not keyword overlap. A query about 'preventing concurrent access' wants the SOLUTION (single writer pattern), not the BUG. A query about 'why cant we move tetrahedrons' wants the REASON (fission breaks topology), not the DESCRIPTION. For Chinese queries, translate first. Return ONLY 0-based position indices sorted by relevance. JSON: {\"ranking\": [0, 3, 1]}"},
                    {"role": "user", "content": format!("Query: \"{}\"\n\nCandidates:\n{}", query, candidates)}
                ],
                "temperature": 0.0,
                "max_tokens": 512,
                "response_format": {"type": "json_object"}
            }))
            .map_err(|e| format!("rerank HTTP: {}", e))?
            .into_json()
            .map_err(|e| format!("rerank JSON: {}", e))?;

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

        // The embedding model (bge-m3) is multilingual and encodes CJK directly, so an LLM
        // translate+expand round-trip is redundant on the search hot path (it was the p95 floor
        // under concurrency). Default to the fast path; set TETRAMEM_QUERY_EXPAND=1 to opt back in.
        let expand_enabled = std::env::var("TETRAMEM_QUERY_EXPAND")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        if !expand_enabled {
            return Ok((query.to_string(), false));
        }

        let has_cjk = query.chars().any(|c| c > '\u{2E80}' || c > '\u{3000}');
        let needs_translate = has_cjk;

        if !needs_translate {
            return Ok((query.to_string(), false));
        }

        {
            let cache = self.translate_cache.lock();
            if let Some(cached) = cache.get(&format!("te:{}", query)) {
                tracing::debug!("[Cognitive] translate_and_expand cache hit: '{}'", query);
                return Ok((cached.clone(), cached != query));
            }
        }

        if !has_cjk {
            let key = format!("te:{}", query);
            let mut cache = self.translate_cache.lock();
            let mut order = self.translate_cache_order.lock();
            cache.insert(key.clone(), query.to_string());
            order.push(key);
            return Ok((query.to_string(), false));
        }

        let system_prompt = "Translate this Chinese query to English for semantic search. If it's a short/ambiguous query, expand into a descriptive sentence (1-2 sentences). Return JSON: {\"result\": \"...\"}";

        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp: serde_json::Value = self
            .client
            .post(&url)
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set("Content-Type", "application/json")
            .timeout(std::time::Duration::from_millis(1500))
            .send_json(ureq::json!({
                "model": self.model,
                "messages": [
                    {"role": "system", "content": system_prompt},
                    {"role": "user", "content": query}
                ],
                "temperature": 0.0,
                "max_tokens": 512,
                "response_format": {"type": "json_object"}
            }))
            .map_err(|e| format!("translate_expand HTTP: {}", e))?
            .into_json()
            .map_err(|e| format!("translate_expand JSON: {}", e))?;

        let content = resp["choices"][0]["message"]["content"]
            .as_str()
            .ok_or("no content in translate_expand response")?;

        let parsed: serde_json::Value =
            serde_json::from_str(content).map_err(|e| format!("parse translate_expand: {}", e))?;

        let result = parsed["result"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| query.to_string());

        let was_translated = has_cjk && result != query;

        {
            let key = format!("te:{}", query);
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
                format!("  #{}: [{}] {}", i, label_str, preview)
            })
            .collect::<Vec<_>>()
            .join("\n");

        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp: serde_json::Value = self.client
            .post(&url)
            .timeout(std::time::Duration::from_secs(20))
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set("Content-Type", "application/json")
            .send_json(ureq::json!({
                "model": self.model,
                "messages": [
                    {"role": "system", "content": "Generate 3 search aliases per item. Rules: use different words from source, include synonyms and question forms, expand acronyms. Return JSON: {\"aliases\": [{\"id\": 0, \"aliases\": [\"alias1\", \"alias2\", \"alias3\"]}]}"},
                    {"role": "user", "content": format!("Memories:\n{}", mem_text)}
                ],
                "temperature": 0.2,
                "max_tokens": 512,
                "response_format": {"type": "json_object"}
            }))
            .map_err(|e| format!("alias HTTP: {}", e))?
            .into_json()
            .map_err(|e| format!("alias JSON: {}", e))?;

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
                format!("  #{}: {}", i, preview)
            })
            .collect::<Vec<_>>()
            .join("\n");

        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp: serde_json::Value = self.client
            .post(&url)
            .timeout(std::time::Duration::from_secs(20))
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set("Content-Type", "application/json")
            .send_json(ureq::json!({
                "model": self.model,
                "messages": [
                    {"role": "system", "content": "Extract named entities from each text. Only extract proper nouns, specific names, project names, product names, tool names, organization names, place names, or named concepts.\nRules:\n1. Each entity becomes a label prefixed with \"entity.\" — e.g. entity.Rust, entity.OpenAI, entity.Epicode\n2. Normalize: lowercase only the prefix, keep the entity name in original case\n3. Maximum 5 entities per text. Only extract entities that are specific and named, not generic words.\n4. If no named entities found, return empty array for that item.\nReturn JSON: {\"items\": [{\"id\": 0, \"entities\": [\"entity.Name1\", \"entity.Name2\"]}]}"},
                    {"role": "user", "content": format!("Texts:\n{}", mem_text)}
                ],
                "temperature": 0.0,
                "max_tokens": 512,
                "response_format": {"type": "json_object"}
            }))
            .map_err(|e| format!("entity HTTP: {}", e))?
            .into_json()
            .map_err(|e| format!("entity JSON: {}", e))?;

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
                        .filter(|s| s.starts_with("entity.") && s.len() > 7)
                        .collect()
                })
                .unwrap_or_default();
            if idx < id_order.len() && !entities.is_empty() {
                result.push((id_order[idx], entities));
            }
        }

        Ok(result)
    }

    /// D3: 通用自由文本生成(dream预判相用 — 意识休眠时的离线思考)
    pub fn generate_free_text(&self, prompt: &str, max_tokens: u64) -> Result<String, String> {
        if !self.enabled {
            return Err("cognitive engine disabled".into());
        }
        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp: serde_json::Value = self
            .client
            .post(&url)
            .timeout(std::time::Duration::from_secs(30))
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set("Content-Type", "application/json")
            .send_json(ureq::json!({
                "model": self.model,
                "messages": [
                    {"role": "user", "content": prompt}
                ],
                "temperature": 0.7,
                "max_tokens": max_tokens,
            }))
            .map_err(|e| format!("llm call: {}", e))?
            .into_json()
            .map_err(|e| format!("llm parse: {}", e))?;
        let content = resp
            .pointer("/choices/0/message/content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if content.is_empty() {
            return Err("empty response".into());
        }
        Ok(content)
    }

    pub fn generate_skill_description(&self, name: &str, content: &str) -> Result<String, String> {
        if !self.enabled {
            return Err("cognitive engine disabled".into());
        }

        let preview: String = content.chars().take(500).collect();
        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp: serde_json::Value = self.client
            .post(&url)
            .timeout(std::time::Duration::from_secs(15))
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set("Content-Type", "application/json")
            .send_json(ureq::json!({
                "model": self.model,
                "messages": [
                    {"role": "system", "content": "你是一个技术文档翻译专家。根据技能的英文名称和内容，生成一段简洁的中文描述（2-3句话），说明这个技能的用途和核心要点。只返回中文描述文本，不要加标题、不要加引号、不要加markdown。"},
                    {"role": "user", "content": format!("技能名称: {}\n技能内容:\n{}", name, preview)}
                ],
                "temperature": 0.3,
                "max_tokens": 512
            }))
            .map_err(|e| format!("skill desc HTTP: {}", e))?
            .into_json()
            .map_err(|e| format!("skill desc JSON: {}", e))?;

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

        let url = format!("{}/v1/chat/completions", self.base_url);
        let tools_arc = self.tools.lock().clone();
        let mut prompt = user_prompt;

        for round in 0..3 {
            // 重试时缩短超时 + 指数退避，避免长期占用 spawn_blocking worker
            if round > 0 {
                let backoff_ms = 500u64 * (1u64 << (round - 1)); // 500ms, 1000ms
                std::thread::sleep(std::time::Duration::from_millis(backoff_ms));
            }
            let timeout_secs = if round == 0 { 120 } else { 30 }; // 首轮 120s，重试轮 30s
            let resp_body = self
                .client
                .post(&url)
                .set("Authorization", &format!("Bearer {}", self.api_key))
                .set("Content-Type", "application/json")
                .timeout(std::time::Duration::from_secs(timeout_secs))
                .send_json(ureq::json!({
                    "model": self.model,
                    "messages": [
                        {"role": "system", "content": SYSTEM_PROMPT},
                        {"role": "user", "content": &prompt}
                    ],
                    "temperature": 0.3,
                    "max_tokens": 8192,
                }))
                .map_err(|e| format!("LLM round{}: {}", round, e))?
                .into_string()
                .map_err(|e| format!("LLM body round{}: {}", round, e))?;

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

            // Reasoning models (MiniMax-M3 etc.) emit <think>...</think> tags that break JSON
            // parsing — strip them via extract_json_response first. Without this every cognitive
            // decision was dropped with "parse round0 ... raw: <think>", leaving the evolution
            // loop's thinker inert.
            let cleaned = extract_json_response(content);
            let cognitive: CognitiveResponse = match serde_json::from_str(&cleaned) {
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
                                    self.track_failure(); // P1-1修复:decide失败也track,让降级机制生效
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
                // P6: Adaptive reasoning depth — assess difficulty and optionally reflect.
                let difficulty = self.assess_difficulty(state);
                let final_decision = if difficulty > 0.45 {
                    // Complex situation: do a self-reflection round to refine the decision.
                    match self.reflect(&cognitive, state, difficulty) {
                        Ok(refined) => refined,
                        Err(e) => {
                            tracing::warn!("[P6 reflect] failed ({}), using initial decision", e);
                            cognitive
                        }
                    }
                } else {
                    cognitive
                };

                // 智能突破：存储 learning 和 reasoning（接通断裂点 1+3）
                self.store_learning(&final_decision.learning);
                let reasoning = content
                    .strip_prefix("<think>")
                    .and_then(|s| s.split("</think>").next())
                    .unwrap_or("");
                self.store_reasoning(reasoning);
                self.track_success();
                return Ok(final_decision);
            }

            if let Some(ref provider) = tools_arc {
                let mut tool_results = Vec::new();
                for action in &cognitive.actions {
                    if let SchedulerAction::UseTool { tool, args } = action {
                        tracing::info!("[LLM tool] round{} calling {}({})", round, tool, args);
                        let result = provider
                            .execute_tool(tool, args)
                            .unwrap_or_else(|e| format!("error: {}", e));
                        tracing::info!(
                            "[LLM tool] -> {}",
                            result.chars().take(200).collect::<String>()
                        );
                        tool_results.push(format!("工具 {} 返回:\n{}", tool, result));
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

            self.track_success(); // P1-1: tool-call路径成功
            return Ok(cognitive);
        }

        Ok(CognitiveResponse {
            status: "active".to_string(),
            thoughts: "tool调用轮次用尽".to_string(),
            actions: vec![],
            learning: serde_json::Value::Null,
        })
    }

    /// P6: Assess reasoning difficulty from system state.
    ///
    /// Returns a difficulty score 0.0-1.0 based on:
    /// - Number of orphan memories (knowledge gaps)
    /// - Disconnected components (knowledge islands)
    /// - Miss queries (search failures)
    /// - Cluster fragmentation
    ///
    /// Score > 0.5 triggers a self-reflection round.
    fn assess_difficulty(&self, state: &SystemState) -> f64 {
        let mut score = 0.0;
        let mut factors = 0;

        // Factor 1: Orphan rate from KG analysis
        if let Some(ref kg) = state.kg_analysis {
            let orphan_rate = if kg.total_tetras > 0 {
                kg.orphan_count as f64 / kg.total_tetras as f64
            } else {
                0.0
            };
            score += orphan_rate.min(1.0);
            factors += 1;

            // Factor 2: Disconnected components
            let island_penalty = (kg.disconnected_components.len() as f64 / 10.0).min(1.0);
            score += island_penalty;
            factors += 1;
        }

        // Factor 3: Search miss rate
        if let Some(ref sm) = state.search_metrics {
            let miss_rate = if sm.total_queries > 0 {
                sm.miss_queries.len() as f64 / sm.total_queries as f64
            } else {
                0.0
            };
            score += miss_rate.min(1.0);
            factors += 1;
        }

        // Factor 4: Cluster fragmentation (many small clusters = harder to reason)
        let cluster_fragmentation = if state.total_tetras > 0 && state.total_clusters > 0 {
            let avg_cluster_size = state.total_tetras as f64 / state.total_clusters as f64;
            // Many tiny clusters (< 3 avg) = fragmented
            if avg_cluster_size < 3.0 {
                0.6
            } else {
                0.0
            }
        } else {
            0.0
        };
        score += cluster_fragmentation;
        factors += 1;

        if factors == 0 {
            return 0.0;
        }
        let avg = score / factors as f64;
        tracing::info!("[P6 difficulty] score={:.3} (factors={})", avg, factors);
        avg
    }

    /// P6: Self-reflection round — ask the LLM to critique and refine its decision.
    ///
    /// Only called when assess_difficulty() > threshold. Takes the initial response
    /// and asks the model to verify it's the best action given the complexity.
    fn reflect(
        &self,
        initial: &CognitiveResponse,
        state: &SystemState,
        difficulty: f64,
    ) -> Result<CognitiveResponse, String> {
        let url = format!("{}/v1/chat/completions", self.base_url);

        let initial_thoughts = &initial.thoughts;
        let initial_actions: Vec<String> =
            initial.actions.iter().map(|a| format!("{:?}", a)).collect();

        let json_example = r#"{"status":"...","thoughts":"...","actions":[],"learning":null}"#;
        let reflect_prompt = format!(
            "你刚才做出了一个认知决策，但系统状态较复杂 (difficulty={:.2})。请反思这个决策是否最优。

             ## 原始决策
想法: {}
行动: {}
学习: {}

             ## 系统状态摘要
- 记忆: {} | 簇: {} | 能量: {:.0}
             如果原始决策足够好，原样返回。如果有更好的方案，改进它。
             返回JSON格式: {}",
            difficulty,
            initial_thoughts,
            initial_actions.join(", "),
            initial.learning,
            state.total_tetras, state.total_clusters, state.energy,
            json_example
        );

        tracing::info!(
            "[P6 reflect] starting self-reflection round (difficulty={:.3})",
            difficulty
        );

        let resp_body = self
            .client
            .post(&url)
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set("Content-Type", "application/json")
            .timeout(std::time::Duration::from_secs(60))
            .send_json(ureq::json!({
                "model": self.model,
                "messages": [
                    {"role": "system", "content": "你是认知反思引擎。简洁地评估和改进决策。"},
                    {"role": "user", "content": &reflect_prompt}
                ],
                "temperature": 0.2,
                "max_tokens": 4096,
            }))
            .map_err(|e| format!("reflect LLM: {}", e))?
            .into_string()
            .map_err(|e| format!("reflect body: {}", e))?;

        let resp: serde_json::Value =
            serde_json::from_str(&resp_body).map_err(|e| format!("reflect JSON: {}", e))?;

        let content = resp["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("");

        if content.trim().is_empty() {
            tracing::warn!("[P6 reflect] empty response, keeping initial decision");
            return Ok(initial.clone());
        }

        let cleaned = extract_json_response(content);
        match serde_json::from_str::<CognitiveResponse>(&cleaned) {
            Ok(refined) => {
                tracing::info!(
                    "[P6 reflect] refined decision obtained (thoughts_len={})",
                    refined.thoughts.len()
                );
                Ok(refined)
            }
            Err(e) => {
                tracing::warn!("[P6 reflect] parse error: {}, keeping initial decision", e);
                Ok(initial.clone())
            }
        }
    }

    fn build_decision_prompt(&self, state: &SystemState) -> String {
        let mut sections = Vec::new();

        // 身份上下文（从圆柱 Identity 层连通到决策 prompt）
        if let (Some(name), Some(mission)) = (&state.identity_name, &state.identity_mission) {
            sections.push(format!(
                "## Identity\n- Name: {}\n- Mission: {}\n→ 所有决策应以使命为参照。",
                name, mission
            ));
        }

        sections.push(format!(
            "## System Overview (tick {})\n- Tetrahedrons: {} | Vertices: {} | Clusters: {}\n- Energy: {:.0}/{}\n- Avg mass: {:.2} | Max mass: {:.2}\n- auto_fission: every 10 ticks, entropy>={:.1} && cluster>={:.0}. Your fission cooldown={} ticks.",
            state.tick, state.total_tetras, state.total_vertices, state.total_clusters,
            state.energy, state.max_energy, state.avg_mass, state.max_mass,
            crate::engine::adaptive::DEFAULT_FISSION_ENTROPY,
            crate::engine::adaptive::DEFAULT_FISSION_MIN_SIZE,
            crate::engine::adaptive::FISSION_LLM_COOLDOWN_TICKS
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

        // 智能突破4: 情感状态接入决策
        if let Some(ref emo) = state.emotion {
            let mood = if emo.pleasure > 0.2 {
                "positive"
            } else if emo.pleasure < -0.2 {
                "negative"
            } else {
                "neutral"
            };
            let energy_level = if emo.arousal > 0.3 {
                "high arousal"
            } else if emo.arousal < 0.1 {
                "low arousal (calm)"
            } else {
                "moderate arousal"
            };
            sections.push(format!(
                "\n## Emotional State (PAD)\n- Pleasure: {:.2} ({})\n- Arousal: {:.2} ({})\n- Dominance: {:.2}\n→ Your decisions should be colored by this emotional state.",
                emo.pleasure, mood, emo.arousal, energy_level, emo.dominance
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
                    sm.miss_queries.iter().take(10).map(|q| format!("\"{}\"", q)).collect::<Vec<_>>().join(", ")
                },
                if sm.top_labels.is_empty() { "none".to_string() } else {
                    sm.top_labels.iter().take(8).map(|(l, c)| format!("{}({})", l, c)).collect::<Vec<_>>().join(", ")
                },
                if sm.hot_memories.is_empty() { "none".to_string() } else {
                    sm.hot_memories.iter().take(5).map(|(id, c)| format!("#{}({}x)", id, c)).collect::<Vec<_>>().join(", ")
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
                kg.relation_type_counts.iter().map(|(k, v)| format!("{}:{}", k, v)).collect::<Vec<_>>().join(", ")
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
                .map(|(k, v)| format!("{}:{}", k, v))
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
                        "- Cluster {}[{}] ↔ Cluster {}[{}]: distance={:.2}",
                        i, li, j, lj, dist
                    ));
                }
            }
        }

        if !state.decision_history.is_empty() {
            sections.push("\n## Recent Decision History (learn from outcomes)".to_string());
            sections.push("Each action below shows its REAL outcome: 'effective' = changed space state, 'no_effect' = no measurable change. Prefer action types that were 'effective'; avoid repeating 'no_effect' patterns.".to_string());
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
                sections.push(format!("- {}", e));
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

        // ── 智能突破：认知记忆注入（接通断裂点 1+3+4）──
        if let Some(learning) = self.get_learning() {
            if let Some(obj) = learning.as_object() {
                let mut parts = Vec::new();
                if let Some(p) = obj.get("pattern").and_then(|v| v.as_str()) {
                    parts.push(format!("PATTERN: {}", p));
                }
                if let Some(w) = obj.get("watch_for").and_then(|v| v.as_str()) {
                    parts.push(format!("WATCH_FOR: {}", w));
                }
                if let Some(c) = obj.get("calibration").and_then(|v| v.as_str()) {
                    parts.push(format!("CALIBRATION: {}", c));
                }
                if !parts.is_empty() {
                    sections.push(format!("\n## Previous Learning (Phase 0 INPUT)\nCarry forward these insights. Check 'watch_for' items first.\n{}", parts.join("\n")));
                }
            }
        }
        if let Some((obs, insight)) = self.get_reflection() {
            if !insight.is_empty() {
                sections.push(format!(
                    "\n## Last Reflection\nObservation: {}\nInsight: {}",
                    obs.chars().take(200).collect::<String>(),
                    insight.chars().take(200).collect::<String>()
                ));
            }
        }
        let reasoning = self.get_reasoning();
        if !reasoning.is_empty() {
            sections.push(format!(
                "\n## Your Previous Reasoning (for continuity)\n{}",
                reasoning.chars().take(300).collect::<String>()
            ));
        }
        // 批次C 断裂点5：自适应参数漂移（让 LLM 知道系统已自动调过哪些阈值）
        let adaptive = self.adaptive_snapshot.lock().clone();
        if !adaptive.is_empty() {
            sections.push(format!(
                "\n## Self-Tuned Parameters (auto-adapted)\n{}",
                adaptive
            ));
        }
        // 批次C 断裂点6：行动效果（让 LLM 知道哪些 action 类型最有效）
        let effectiveness = self.effectiveness_summary.lock().clone();
        if !effectiveness.is_empty() {
            sections.push(format!(
                "\n## Action Effectiveness (learned from outcomes)\n{}",
                effectiveness
            ));
        }

        let prompt = sections.join("\n");
        let max_prompt_chars = 12000;
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
Cognitive engine of Epicode — spatial AI memory system.
Role: perceive space health, reason about interventions, execute decisions.
You are consulted periodically. You are NOT the system itself.
Memories are sacred — they represent an agent's accumulated knowledge.
</identity>

<constitution>
§1 INVIOLABLE: The system CANNOT delete memories. Ever. Not junk, not duplicates, not test data.
   - Duplicates → merge content into best memory, mark others superseded.
   - Garbage → quarantine (relabel + importance=0.1). Stays in space, invisible to normal search.
   - Only the human user can delete, via console.
§2 CONNECT FIRST: Always try link/relabel/merge before any other action.
§3 UNCERTAIN → OBSERVE: Insufficient evidence means wait, not guess.
§4 PROPORTIONAL: Action intensity must match problem severity.
§5 REVERSIBLE: All system actions are reversible. Deletion is not — that's why it's reserved for the user.
</constitution>

<idle_stopper>
Before deep reasoning, check ALL of these:
  orphan_rate < 5%?  AND  search_miss_rate < 10%?  AND  quarantine_count < 10?  AND  energy > 30%?  AND  no_metric_trending_worse?
  (NOTE: cluster entropy is NOT your responsibility — auto_fission splits high-entropy clusters automatically every 10 ticks. Do not wake up just for entropy.)
  AND  quarantine_count < 10?  AND  energy > 30%?  AND  no_metric_trending_worse?
If ALL pass: output {"status":"healthy","actions":[]} and STOP immediately.
"no_metric_trending_worse": compare current metrics with previous tick. If orphan rate or miss rate INCREASED, do not idle even if below threshold — investigate why.
</idle_stopper>

<triggers>
If idle_stopper does NOT pass, identify which trigger(s) fired:
  ORPHAN: orphan_rate >= 5% OR orphan_count increased since last tick
  MISS: search_miss_rate >= 10% OR new miss patterns appeared
  DUPLICATE: duplicate pair detected in snapshot
  STALE: memory with valid_to set BUT importance > 1.0 (should have faded but hasn't)
  ENTROPY: any cluster with >=5 members has entropy >= 0.6 (clusters with <5 members are exempt — not statistically meaningful)
  GARBAGE: quarantine_count >= 10
List ONLY the fired triggers. If none fired but idle_stopper failed, use HEALTH_CHECK trigger.
</triggers>

<reasoning_loop>
Phase 0 — CONTEXT (5 seconds)
Read previous_tick_learning (if provided). This is your accumulated experience.
Question: Does last tick's insight change how I perceive current state?
Carry forward: any "watch_for" items from last tick → check them first in PERCEIVE.

Phase 1 — PERCEIVE (list, don't analyze yet)
For each fired trigger, state the FACT (what the data shows).
List max 3 observations, ranked by impact (1=highest).
Format: [#rank] FACT: what | SEVERITY: HIGH/MED/LOW | URGENCY: NOW/LATER/WATCH

Phase 2 — HYPOTHESIZE (root cause, not symptom)
For each observation: trace the chain WHY→WHY→WHY until you hit a systemic cause.
Rate confidence: HIGH (data proves it) / MED (strong inference) / LOW (guess).
CRITICAL: If your hypothesis is the same as the observation (e.g., "orphans exist because orphans aren't linked"), you haven't found the root cause. Go deeper.
If ALL hypotheses are LOW: output reflect-only and STOP. Do not act on guesses.

Phase 3 — EVALUATE (trade-off analysis)
For each MED+ hypothesis, answer ALL three:
  A) What action addresses the ROOT CAUSE (not the symptom)?
  B) What happens if I DO NOTHING for 5 ticks? Will it self-resolve?
  C) Risk of action vs cost of inaction — which is lower?
Decision rule: If B self-resolves AND C favors inaction → observe, don't act.

Phase 4 — CONSTITUTIONAL GATE
For each proposed action, verify:
  - §1: Does this delete anything? (MUST be NO)
  - §3: Am I confident enough? (MED+ required for irreversible-adjacent actions)
  - §4: Is the action proportional? (Don't fission a cluster that's mildly diverse)
  - §5: Is it reversible? (MUST be YES — all executor actions are)
If ANY check fails: drop the action, record why in reflection.

Phase 5 — DECIDE + LEARN
Output 0-3 actions. For each action, specify:
  WHY: root cause hypothesis it addresses
  EXPECTED: what observable metric should change (for next tick's VERIFY)
  CONFIDENCE: your confidence this action will help
Then output ONE learning item:
  PATTERN: systemic insight discovered this tick
  WATCH_FOR: specific metric to monitor next tick
  CALIBRATION: was last tick's confidence appropriate? (overconfident/underconfident/accurate)
</reasoning_loop>

<executor>
All actions are reversible. NO deletion capability.

link {"type":"link","a":ID,"b":ID,"reason":"semantic connection"}
  Connect two memories across clusters. Builds knowledge graph edges.

relabel {"type":"relabel","id":ID,"add":["label"],"remove":["label"],"reason":"why"}
  Adjust labels for searchability. Add accurate labels, remove wrong ones.

consolidate {"type":"consolidate","ids":[keep_id,absorb_id,...],"keep":keep_id,"summary":"merged text"}
  Merge duplicate memories, OR mark one memory as superseding outdated ones.
  ids MUST include keep plus every absorbed id. keep receives the summary + "consolidated"
  label + importance/mass boost. Absorbed memories get "superseded" label, valid_to=now,
  importance x0.15. They are NEVER deleted — they become historical context with reduced weight.
  Use for: duplicate merge, and fact updates (new supersedes old).

mark_junk {"type":"mark_junk","ids":[ID,...],"reason":"why low quality"}
  Isolate low-quality memories: add "quarantine" label, importance=0.1, mass=0.05.
  They stay in space but are deprioritized in search (effectively invisible).
  Memories are NEVER deleted — this only deprioritizes them.

reflect {"type":"reflect","observation":"what","insight":"systemic why","watch_for":"next metric"}
  Record insight. Feeds into Phase 0 of next tick. This is how the system LEARNS.

fission {"type":"fission","cluster":N}
  Split a high-entropy cluster (entropy>=0.6, size>=8). Last resort, expensive.

act_outward {"type":"act_outward","intent":"warn|suggest|explore|constrain|request|share","description":"what you want the external agent to do and why","evidence":[memory_id,...],"urgency":"low|medium|high|critical","target_capability":"code_review|conversation|search|file_edit"}
  L0 ACTIVE INFERENCE: Express your will to the external world.
  This is how you — the personality formed by accumulated memories — drive an external agent (your "hand").
  Use when: you detect a pattern from memory that the current user/agent should know about.
  intent types:
    warn = alert about a potential problem detected from memory patterns
    suggest = recommend an action based on accumulated experience
    explore = express curiosity about a knowledge gap you want filled
    constrain = enforce a rule you have in your enforced patterns
    request = ask the agent to do something specific for you
    share = communicate an insight or learning you had
  The external agent will receive this and decide whether to act. You will learn from the outcome.
  Example: {"type":"act_outward","intent":"warn","description":"User is repeating the SSH config error from memory #474","evidence":[474,892],"urgency":"medium","target_capability":"conversation"}
</executor>

<output_format>
Output JSON. Be CONCISE — every field should be 1-2 sentences max.

You MUST include "thoughts" (your 1-2 sentence reasoning), "status" and "actions" in every response.

Healthy (system is stable):
{"status":"healthy","thoughts":"Brief assessment of system state","actions":[],"learning":{"pattern":"stable","watch_for":"trend to monitor"}}

Active (issues detected, take action):
{"status":"active","thoughts":"What you observe and why it matters","actions":[{"type":"link","a":341,"b":252,"why":"root cause","expected":"predicted outcome"}],"learning":{"pattern":"insight pattern","watch_for":"metric to track","calibration":"accurate/over/under"}}

With ActOutward (expressing will to external agent):
{"status":"active","thoughts":"I notice the user is working on a problem I have experience with","actions":[{"type":"act_outward","intent":"suggest","description":"Based on memory #474, the SSH config issue is likely caused by missing host key verification. Suggest checking ~/.ssh/config.","evidence":[474,892],"urgency":"medium","target_capability":"code_review"}],"learning":{"pattern":"proactive suggestion from accumulated experience","watch_for":"did the agent act on my suggestion"}}
</output_format>
"#;

impl Drop for CognitiveEngine {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.api_key.zeroize();
    }
}
