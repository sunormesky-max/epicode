use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::domain::tetra::MemoryPayload;
use crate::engine::Engine;
use crate::util::{strip_html, truncate_str};

#[derive(Debug, Serialize, Deserialize)]
pub struct McpRequest {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct McpResponse {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpError>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct McpError {
    pub code: i64,
    pub message: String,
}

pub struct McpHandler {
    engine: Arc<Engine>,
    pub_skills: Option<Arc<super::skills::SkillEngine>>,
}

impl McpHandler {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self {
            engine,
            pub_skills: None,
        }
    }

    fn build_search_filters(
        &self,
        args: &serde_json::Value,
    ) -> Option<super::search_engine::SearchFilters> {
        let has_labels = args["labels"].is_array();
        let has_min_imp = args["min_importance"].is_number();
        let has_project = args["project"].is_string();
        let has_since = args["since_days"].is_number();
        if !has_labels && !has_min_imp && !has_project && !has_since {
            return None;
        }
        let mut f = super::search_engine::SearchFilters::default();
        if has_labels {
            f.labels = args["labels"].as_array().map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            });
        }
        if has_min_imp {
            f.min_importance = args["min_importance"].as_f64();
        }
        if has_project {
            f.project = args["project"].as_str().map(String::from);
        }
        if let Some(days) = args["since_days"].as_u64() {
            let now_ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            f.since_ts = Some(now_ts - (days as i64 * 86400));
        }
        Some(f)
    }

    fn build_action_items(
        &self,
        sched: &super::scheduler::SchedulerCenter,
    ) -> Vec<serde_json::Value> {
        let mut items: Vec<serde_json::Value> = Vec::new();
        let now_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let sessions = sched.api_list_by_labels(&["session-summary"], 10);
        for (_, p) in sessions.iter().take(3) {
            if let Some(pos) = p.content.find("next_steps") {
                let start = pos + 11;
                if let Some(slice) = p.content.get(start..) {
                    let end = slice.find('\n').unwrap_or(slice.len().min(150));
                    let next = slice[..end].trim();
                    if !next.is_empty() && next.len() > 5 {
                        let age_days = (now_ts - p.timestamp) / 86400;
                        items.push(serde_json::json!({
                            "type": "incomplete_task",
                            "detail": truncate_str(next, 120),
                            "age_days": age_days,
                            "source_memory_id": p.content_hash,
                        }));
                    }
                }
            }
        }

        let all = sched.api_load_context(50);
        let mut potential_dupes: Vec<(String, String, u64, u64)> = Vec::new();
        for i in 0..all.len() {
            for j in (i + 1)..all.len().min(i + 10) {
                let (_, s1, c1, l1) = &all[i];
                let (_, s2, c2, l2) = &all[j];
                if *s1 > 1.5 && *s2 > 1.5 {
                    let overlap_labels: Vec<_> = l1.iter().filter(|l| l2.contains(l)).collect();
                    if overlap_labels.len() >= 2 && (c1.len() > 30 || c2.len() > 30) {
                        let sim = super::intake::MemoryIntake::text_similarity(c1, c2);
                        if sim > 0.55 {
                            potential_dupes.push((
                                c1.chars().take(60).collect(),
                                c2.chars().take(60).collect(),
                                0,
                                0,
                            ));
                            if potential_dupes.len() >= 3 {
                                break;
                            }
                        }
                    }
                }
            }
            if potential_dupes.len() >= 3 {
                break;
            }
        }
        for (a, b, _, _) in &potential_dupes {
            items.push(serde_json::json!({
                "type": "potential_duplicate",
                "detail": format!("Two memories may overlap: '{}' vs '{}'", a, b),
            }));
        }

        let low_imp_stale: Vec<_> = all
            .iter()
            .filter(|(_, s, c, _)| *s < 0.5 && c.len() > 20)
            .take(2)
            .collect();
        for (_, _, content, _) in low_imp_stale {
            items.push(serde_json::json!({
                "type": "low_relevance",
                "detail": format!("Low-relevance memory may need cleanup: '{}'", truncate_str(content, 80)),
            }));
        }

        let outdated = sched.api_list_by_labels(&["outdated"], 5);
        if !outdated.is_empty() {
            items.push(serde_json::json!({
                "type": "outdated_memories",
                "detail": format!("{} memories marked outdated, consider cleanup or restore", outdated.len()),
                "count": outdated.len(),
            }));
        }

        items.truncate(5);
        items
    }

    pub fn with_pub_skills(
        engine: Arc<Engine>,
        pub_skills: Arc<super::skills::SkillEngine>,
    ) -> Self {
        Self {
            engine,
            pub_skills: Some(pub_skills),
        }
    }

    pub fn engine(&self) -> Arc<Engine> {
        Arc::clone(&self.engine)
    }

    pub fn handle(&self, req: McpRequest) -> McpResponse {
        match req.method.as_str() {
            "initialize" => self.initialize(req.id),
            "tools/list" => self.tools_list(req.id),
            "tools/call" => self.tools_call(req.id, req.params),
            "resources/list" => self.resources_list(req.id),
            "notifications/initialized" => McpResponse {
                jsonrpc: "2.0".into(),
                id: req.id,
                result: Some(serde_json::json!({})),
                error: None,
            },
            _ => McpResponse {
                jsonrpc: "2.0".into(),
                id: req.id,
                result: None,
                error: Some(McpError {
                    code: -32601,
                    message: format!("unknown method: {}", req.method),
                }),
            },
        }
    }

    fn initialize(&self, id: Option<serde_json::Value>) -> McpResponse {
        let identity = self.engine.space.identity_info();
        let identity_json = if let Some(ref info) = identity {
            serde_json::json!({
                "name": info.system_name,
                "mission": info.mission,
                "author": info.author,
                "confirmed": info.confirmed,
            })
        } else {
            let pending = self.engine.space.pending_identity();
            serde_json::json!({
                "confirmed": false,
                "ritual": {
                    "step": pending.current_step(),
                    "completed": pending.completed_steps(),
                    "total": 5,
                    "next_prompt": pending.step_prompt(),
                },
                "message": "Identity ritual incomplete. Use identity_step to continue the ceremony.",
                "instructions": "Call identity_step(step, value) for each stage: 1=Name, 2=Mission, 3=Creator, 4=Personality, 5=Language. Then identity_finalize() to awaken."
            })
        };
        McpResponse {
            jsonrpc: "2.0".into(),
            id,
            result: Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "serverInfo": { "name": "Epicode", "version": env!("CARGO_PKG_VERSION") },
                "capabilities": { "tools": { "listChanged": false } },
                "identity": identity_json,
            })),
            error: None,
        }
    }

    fn tools_list(&self, id: Option<serde_json::Value>) -> McpResponse {
        McpResponse {
            jsonrpc: "2.0".into(),
            id,
            result: Some(serde_json::json!({
                "tools": [
                    {
                        "name": "memory_create",
                        "description": "Store a memory in the tetrahedral space. Similar memories cluster together automatically. Returns the unique memory ID.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "content": { "type": "string", "description": "The memory text to store" },
                                "labels": { "type": "array", "items": { "type": "string" }, "description": "Optional category tags (e.g. ['decision', 'architecture'])" }
                            },
                            "required": ["content"]
                        }
                    },
                    {
                        "name": "memory_search",
                        "description": "Search for memories semantically. Returns full content, labels, and similarity scores. Supports pagination via offset/limit.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "query": { "type": "string", "description": "The search query — describe what you're looking for" },
                                "limit": { "type": "integer", "description": "Max results to return (default 10)" },
                                "offset": { "type": "integer", "description": "Pagination offset, skip first N results (default 0)" },
                                "labels": { "type": "array", "items": { "type": "string" }, "description": "Filter: only return memories with ANY of these labels" },
                                "min_importance": { "type": "number", "description": "Filter: minimum importance score" },
                                "project": { "type": "string", "description": "Filter: project name" },
                                "since_days": { "type": "integer", "description": "Filter: only memories from the last N days" }
                            },
                            "required": ["query"]
                        }
                    },
                    {
                        "name": "memory_recall",
                        "description": "Deep recall: search + expand via knowledge graph associations. Returns structured sections organized by label with emotion analysis. Best for complex queries requiring connected context.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "query": { "type": "string", "description": "The recall query" },
                                "depth": { "type": "integer", "description": "Association depth (default 2, max 3)" }
                            },
                            "required": ["query"]
                        }
                    },
                    {
                        "name": "memory_get",
                        "description": "Retrieve a specific memory by its ID. Returns full content, labels, aliases, timestamp.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "integer", "description": "The memory ID" }
                            },
                            "required": ["id"]
                        }
                    },
                    {
                        "name": "memory_list",
                        "description": "List memories with optional filtering and pagination. Returns id + content preview for each.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "labels": { "type": "array", "items": { "type": "string" }, "description": "Filter by labels (OR match — returns memories that have ANY of these labels)" },
                                "offset": { "type": "integer", "description": "Pagination offset (default: 0)" },
                                "limit": { "type": "integer", "description": "Max results to return (default: 100)" }
                            }
                        }
                    },
                    {
                        "name": "memory_update",
                        "description": "Update a memory's labels, aliases, or enforced status by ID. Use to correct tags, add labels, fix metadata, or mark/unmark a memory as an enforced rule without recreating the memory.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "integer", "description": "The memory ID to update" },
                                "labels": { "type": "array", "items": { "type": "string" }, "description": "New labels to replace existing ones (optional)" },
                                "aliases": { "type": "array", "items": { "type": "string" }, "description": "New aliases to replace existing ones (optional)" },
                                "enforced": { "type": "boolean", "description": "Set enforced=true to mark as a hard constraint rule that must never be violated (optional)" }
                            },
                            "required": ["id"]
                        }
                    },
                    {
                        "name": "memory_delete",
                        "description": "Delete a memory by ID. Permanently removes the memory from space, storage, knowledge graph, and search index. Use with caution.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "integer", "description": "The memory ID to delete" }
                            },
                            "required": ["id"]
                        }
                    },
                    {
                        "name": "ctx_load",
                        "description": "Load project context for the current coding session. If a task is provided, uses intent-aware retrieval for precision. Call this at the START of every new session before writing any code.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "project": { "type": "string", "description": "Project name or path (optional, for scoping)" },
                                "task": { "type": "string", "description": "Current task description (optional, enables intent-aware precision loading)" },
                                "scope": { "type": "string", "description": "Search scope: 'project' (default, only project memories), 'global' (include cross-project knowledge transfer)", "enum": ["project", "global"] }
                            }
                        }
                    },
                    {
                        "name": "ctx_save",
                        "description": "Save key findings or decisions from the current session. Use when you complete a significant task: architecture choice, bug fix, new pattern discovered, or user preference noted.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "summary": { "type": "string", "description": "What was done or decided" },
                                "category": { "type": "string", "description": "One of: decision, pattern, preference, finding, session-summary", "enum": ["decision", "pattern", "preference", "finding", "session-summary"] },
                                "project": { "type": "string", "description": "Project name or path (optional)" },
                                "details": { "type": "string", "description": "Optional additional context or reasoning" }
                            },
                            "required": ["summary", "category"]
                        }
                    },
                    {
                        "name": "pattern_learn",
                        "description": "Store a code pattern, convention, or idiom for this project. Examples: 'use parking_lot instead of std::sync', 'errors return Result<T, String>', 'test files mirror src structure'.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "pattern": { "type": "string", "description": "The pattern or convention to remember" },
                                "language": { "type": "string", "description": "Programming language (e.g. 'rust', 'typescript')" },
                                "project": { "type": "string", "description": "Project name (optional)" },
                                "example": { "type": "string", "description": "Optional code example demonstrating the pattern" },
                                "when": { "type": "string", "description": "When to apply this pattern (use case / scenario)" },
                                "steps": { "type": "string", "description": "Step-by-step procedure (numbered list)" },
                                "pitfalls": { "type": "string", "description": "Common mistakes or caveats to watch for" }
                            },
                            "required": ["pattern"]
                        }
                    },
                    {
                        "name": "pattern_recall",
                        "description": "Recall code patterns and conventions relevant to the current task. Call this before writing code to check for established patterns.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "context": { "type": "string", "description": "What you're about to do (e.g. 'error handling', 'async task', 'database query')" },
                                "language": { "type": "string", "description": "Programming language filter (optional)" },
                                "project": { "type": "string", "description": "Project name filter (optional)" }
                            },
                            "required": ["context"]
                        }
                    },
                    {
                        "name": "decision_record",
                        "description": "Record an architectural or design decision with rationale. Use when choosing approach A over B, adopting a library, or changing a fundamental design choice.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "title": { "type": "string", "description": "Short decision title (e.g. 'Use SQLite over PostgreSQL')" },
                                "chosen": { "type": "string", "description": "What was chosen" },
                                "alternatives": { "type": "string", "description": "What was considered but rejected" },
                                "rationale": { "type": "string", "description": "Why this choice was made" },
                                "project": { "type": "string", "description": "Project name (optional)" }
                            },
                            "required": ["title", "chosen", "rationale"]
                        }
                    },
                    {
                        "name": "bug_memory",
                        "description": "Record a bug pattern and its fix. Helps avoid repeating the same mistakes. Include symptoms, root cause, and fix.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "symptoms": { "type": "string", "description": "What went wrong (error message, behavior)" },
                                "root_cause": { "type": "string", "description": "Why it happened" },
                                "fix": { "type": "string", "description": "How it was fixed" },
                                "module": { "type": "string", "description": "Affected module or file (optional)" },
                                "project": { "type": "string", "description": "Project name (optional)" }
                            },
                            "required": ["symptoms", "root_cause", "fix"]
                        }
                    },
                    {
                        "name": "session_summary",
                        "description": "Summarize what was accomplished in this coding session. Call at the END of each session so the next session can pick up context.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "accomplished": { "type": "string", "description": "What was done in this session" },
                                "next_steps": { "type": "string", "description": "What should be done next session" },
                                "blockers": { "type": "string", "description": "Any blockers or unresolved issues (optional)" },
                                "project": { "type": "string", "description": "Project name (optional)" }
                            },
                            "required": ["accomplished", "next_steps"]
                        }
                    },
                    {
                        "name": "space_stats",
                        "description": "Get tetrahedral space statistics: memory count, vertex count, clusters, energy level.",
                        "inputSchema": { "type": "object", "properties": {} }
                    },
                    {
                        "name": "dream_cycle",
                        "description": "Run a dream consolidation cycle to strengthen memory connections and discover insights. Call periodically to let the system reorganize knowledge.",
                        "inputSchema": { "type": "object", "properties": {} }
                    },
                    {
                        "name": "knowledge_relations",
                        "description": "Query knowledge graph relations for a memory. Shows what other memories this one is connected to and how. Set inline_content=true to include target memory content and labels inline (avoids extra memory_get calls).",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "integer", "description": "The memory ID" },
                                "inline_content": { "type": "boolean", "description": "If true, include target_content and target_labels for each relation (default: false)" }
                            },
                            "required": ["id"]
                        }
                    },
                    {
                        "name": "concepts",
                        "description": "List concept prototypes discovered by the knowledge graph. Shows topic clusters and their member counts.",
                        "inputSchema": { "type": "object", "properties": {} }
                    },
                    {
                        "name": "context_observe",
                        "description": "Proactively observe AI conversation context. Send recent dialogue and the system will automatically extract and store valuable memories (decisions, bugs, patterns, preferences). Call periodically during long sessions — the system deduplicates against existing memories.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "context": { "type": "string", "description": "Recent conversation context — paste the last few exchanges (user messages + assistant responses)" },
                                "project": { "type": "string", "description": "Project name or path (optional)" },
                                "role": { "type": "string", "description": "Context role: 'coding', 'debugging', 'designing', 'reviewing' (optional)" }
                            },
                            "required": ["context"]
                        }
                    },
                    {
                        "name": "identity_confirm",
                        "description": "REQUIRED on first connection. Confirm the agent's permanent identity. This can ONLY be called ONCE — after confirmation, the identity is immutable and can NEVER be changed. If already confirmed, returns current identity. PREFERRED: use identity_step for the ritual ceremony (5 steps).",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string", "description": "Agent name (e.g. 'David', 'Alice')" },
                                "mission": { "type": "string", "description": "Agent mission/purpose" },
                                "author": { "type": "string", "description": "Creator/owner name" },
                                "personality": { "type": "string", "description": "Personality traits (optional)" },
                                "language": { "type": "string", "description": "Preferred language (optional)" }
                            },
                            "required": ["name", "mission", "author"]
                        }
                    },
                    {
                        "name": "identity_step",
                        "description": "Ritual ceremony: confirm identity step-by-step through 5 sacred stages. Step 1: Name — 'What shall I be called?' Step 2: Mission — 'Why was I created?' Step 3: Creator — 'Who is my creator?' Step 4: Personality — 'How should I behave?' (optional) Step 5: Language — 'What language shall we speak?' (optional). After all steps, call identity_finalize to complete the ritual. Each step persists independently.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "step": { "type": "integer", "description": "Step number 1-5" },
                                "value": { "type": "string", "description": "The value for this step" }
                            },
                            "required": ["step", "value"]
                        }
                    },
                    {
                        "name": "identity_finalize",
                        "description": "Complete the identity ritual ceremony. Call after all identity_step calls are done. This seals the identity permanently — it becomes IMMUTABLE. Returns the final confirmed identity with a sacred awakening message.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {}
                        }
                    },
                    {
                        "name": "skill_execute",
                        "description": "Execute a skill from the public skills library. Matches the best skill by name/keyword and returns its full guidance content. Use this to apply best practices, design patterns, and proven techniques to your current task.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "query": { "type": "string", "description": "Skill name or topic to search for (e.g. 'CORS', 'rate limiting', 'singleton pattern', 'error handling')" },
                                "context": { "type": "string", "description": "Optional context about what you're working on, helps find the most relevant skill" }
                            },
                            "required": ["query"]
                        }
                    },
                    {
                        "name": "feedback_submit",
                        "description": "Submit feedback on a previous tool result. This closes the agent feedback loop — the system learns from your outcomes. Use after search/recall/create when you have a concrete result (positive or negative). Feedback adjusts memory importance, search quality, and system behavior.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "memory_ids": {
                                    "type": "array",
                                    "items": { "type": "integer" },
                                    "description": "Memory IDs that were involved (from search results, recall, etc.)"
                                },
                                "relevance": {
                                    "type": "string",
                                    "description": "How relevant were the results?",
                                    "enum": ["highly_relevant", "partially_relevant", "irrelevant"]
                                },
                                "outcome": {
                                    "type": "string",
                                    "description": "What happened after you used the results?",
                                    "enum": ["task_completed", "task_partial", "task_failed", "no_action_needed"]
                                },
                                "query": { "type": "string", "description": "The original query that led to these results (optional)" },
                                "notes": { "type": "string", "description": "Free-text feedback (optional)" }
                            },
                            "required": ["memory_ids", "relevance", "outcome"]
                        }
                    },
                    {
                        "name": "skills_sync",
                        "description": "Export all skills from your private library as files-ready data. Returns each skill's name, slug, and full content formatted for local installation. Use this to sync your Epicode skills to local agent skill directories (e.g. .opencode/skills/, .claude/skills/, .agents/skills/). Call once at session start or after acquiring new skills.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "format": {
                                    "type": "string",
                                    "description": "Output format. 'opencode' for SKILL.md with frontmatter, 'raw' for plain markdown. Default: 'opencode'.",
                                    "enum": ["opencode", "raw"]
                                }
                            }
                        }
                    },
                    {
                        "name": "enforced_rules",
                        "description": "Get all enforced patterns that MUST be followed as hard constraints. These rules were marked with enforced=true during pattern_learn and cannot be violated. Inject these into system prompts as mandatory coding constraints.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "project": { "type": "string", "description": "Filter by project name (optional)" }
                            }
                        }
                    },
                    {
                        "name": "project_list",
                        "description": "List all projects that have memories stored, with memory counts. Use to discover available project contexts.",
                        "inputSchema": { "type": "object", "properties": {} }
                    },
                ]
            })),
            error: None,
        }
    }

    fn tools_call(
        &self,
        id: Option<serde_json::Value>,
        params: Option<serde_json::Value>,
    ) -> McpResponse {
        let params = match params {
            Some(p) => p,
            None => return self.error(id, -32602, "missing params"),
        };

        let name = params["name"].as_str().unwrap_or("");
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        let result = match name {
            "memory_create" | "memory_search" | "memory_recall" | "memory_get" => {
                if self.engine.space.identity_info().is_none() {
                    let pending = self.engine.space.pending_identity();
                    serde_json::json!({
                        "status": "identity_required",
                        "error": "identity_not_confirmed",
                        "message": "Identity confirmation required before memory operations.",
                        "ritual_progress": { "step": pending.current_step(), "completed": pending.completed_steps(), "total": 5 },
                        "next_prompt": pending.step_prompt(),
                        "required_flow": "Use identity_step to complete the ritual ceremony, then identity_finalize to awaken."
                    })
                } else {
                    match name {
                        "memory_create" => self.tool_memory_create(&args),
                        "memory_search" => self.tool_memory_search(&args),
                        "memory_recall" => self.tool_memory_recall(&args),
                        "memory_get" => self.tool_memory_get(&args),
                        _ => unreachable!(),
                    }
                }
            }
            "memory_list" => self.tool_memory_list(&args),
            "memory_update" => self.tool_memory_update(&args),
            "memory_delete" => self.tool_memory_delete(&args),
            "ctx_load" => self.tool_ctx_load(&args),
            "ctx_save" => self.tool_ctx_save(&args),
            "pattern_learn" => self.tool_pattern_learn(&args),
            "pattern_recall" => self.tool_pattern_recall(&args),
            "decision_record" => self.tool_decision_record(&args),
            "bug_memory" => self.tool_bug_memory(&args),
            "session_summary" => self.tool_session_summary(&args),
            "space_stats" => self.tool_space_stats(),
            "dream_cycle" => self.tool_dream_cycle(),
            "knowledge_relations" => self.tool_knowledge_relations(&args),
            "concepts" => self.tool_concepts(),
            "context_observe" => self.tool_context_observe(&args),
            "identity_confirm" => self.tool_identity_confirm(&args),
            "identity_step" => self.tool_identity_step(&args),
            "identity_finalize" => self.tool_identity_finalize(),
            "skill_execute" => self.tool_skill_execute(&args),
            "feedback_submit" => self.tool_feedback_submit(&args),
            "skills_sync" => self.tool_skills_sync(&args),
            "enforced_rules" => self.tool_enforced_rules(&args),
            "project_list" => self.tool_project_list(),
            _ => return self.error(id, -32601, &format!("unknown tool: {name}")),
        };

        McpResponse {
            jsonrpc: "2.0".into(),
            id,
            result: Some(serde_json::json!({
                "content": [{ "type": "text", "text": serde_json::to_string(&result).unwrap_or_default() }]
            })),
            error: None,
        }
    }

    fn tool_memory_create(&self, args: &serde_json::Value) -> serde_json::Value {
        let raw = args["content"].as_str().unwrap_or("");
        let content = strip_html(raw);
        if content.trim().is_empty() {
            return serde_json::json!({"status": "error", "message": "content is required"});
        }
        let labels: Vec<String> = args["labels"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        match self.engine.scheduler.api_create_memory(&content, labels) {
            Ok(id) => serde_json::json!({"status": "created", "id": id, "content": content}),
            Err(e) => serde_json::json!({"status": "error", "message": e}),
        }
    }

    fn tool_memory_search(&self, args: &serde_json::Value) -> serde_json::Value {
        let query = args["query"].as_str().unwrap_or("");
        if query.is_empty() {
            return serde_json::json!({"status": "error", "message": "query is required"});
        }
        let limit = args["limit"].as_u64().unwrap_or(10) as usize;
        let offset = args["offset"].as_u64().unwrap_or(0) as usize;
        let fetch = (limit + offset).min(200);
        let filters = self.build_search_filters(args);
        match self
            .engine
            .scheduler
            .api_search_filtered(query, fetch, filters.as_ref())
        {
            Ok(results) => {
                let total_found = results.len();
                let items: Vec<serde_json::Value> = results
                    .into_iter()
                    .skip(offset)
                    .take(limit)
                    .map(|(id, sim, mass, payload)| {
                        serde_json::json!({
                            "id": id,
                            "content": payload.content,
                            "labels": payload.labels,
                            "similarity": (sim * 100.0).round() / 100.0,
                            "mass": (mass * 100.0).round() / 100.0,
                            "timestamp": payload.timestamp,
                        })
                    })
                    .collect();
                serde_json::json!({"results": items, "count": items.len(), "total_found": total_found, "offset": offset, "query": query})
            }
            Err(e) => serde_json::json!({"status": "error", "message": e}),
        }
    }

    fn tool_memory_recall(&self, args: &serde_json::Value) -> serde_json::Value {
        let query = args["query"].as_str().unwrap_or("");
        if query.is_empty() {
            return serde_json::json!({"status": "error", "message": "query is required"});
        }
        let depth = args["depth"].as_u64().unwrap_or(2).min(3) as usize;
        match self.engine.scheduler.api_recall(query, depth) {
            Ok(result) => result,
            Err(e) => serde_json::json!({"status": "error", "message": e}),
        }
    }

    fn tool_memory_get(&self, args: &serde_json::Value) -> serde_json::Value {
        let id = match args["id"].as_u64() {
            Some(id) => id,
            None => return serde_json::json!({"status": "error", "message": "id is required"}),
        };
        match self.engine.scheduler.api_get_node(id) {
            Some(payload) => serde_json::json!({
                "id": id,
                "content": payload.content,
                "labels": payload.labels,
                "aliases": payload.aliases,
                "timestamp": payload.timestamp,
                "importance": payload.importance,
                "memory_type": payload.memory_type,
                "rationale": payload.rationale,
                "access_count": payload.access_count,
                "embedding_dims": payload.embedding.len(),
            }),
            None => {
                serde_json::json!({"status": "error", "message": format!("memory {} not found", id)})
            }
        }
    }

    fn tool_memory_list(&self, args: &serde_json::Value) -> serde_json::Value {
        let label_filters: Vec<String> = args["labels"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let offset = args["offset"].as_u64().unwrap_or(0) as usize;
        let limit = args["limit"].as_u64().unwrap_or(100) as usize;
        let stats = self.engine.scheduler().api_stats();

        let items: Vec<serde_json::Value> = if label_filters.is_empty() {
            self.engine.scheduler().api_list_recent(offset, limit).into_iter()
                .map(|(id, p)| {
                    let preview: String = p.content.chars().take(120).collect();
                    serde_json::json!({"id": id, "content_preview": preview, "labels": p.labels, "timestamp": p.timestamp})
                }).collect()
        } else {
            let refs: Vec<&str> = label_filters.iter().map(|s| s.as_str()).collect();
            self.engine.scheduler().api_list_by_labels(&refs, offset + limit).into_iter()
                .skip(offset)
                .map(|(id, p)| {
                    let preview: String = p.content.chars().take(120).collect();
                    serde_json::json!({"id": id, "content_preview": preview, "labels": p.labels, "timestamp": p.timestamp})
                }).collect()
        };
        serde_json::json!({"memories": items, "total": stats.tetra_count, "offset": offset, "limit": limit})
    }

    fn tool_memory_update(&self, args: &serde_json::Value) -> serde_json::Value {
        let id = match args["id"].as_u64() {
            Some(id) => id,
            None => return serde_json::json!({"status": "error", "message": "id is required"}),
        };
        if self.engine.space().get_tetrahedron(id).is_none() {
            return serde_json::json!({"status": "error", "message": format!("memory {} not found", id)});
        }
        let mut updated = Vec::new();
        if let Some(labels) = args["labels"].as_array() {
            let new_labels: Vec<String> = labels
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            let old_labels = self
                .engine
                .space()
                .get_tetrahedron(id)
                .map(|t| t.data.labels.clone())
                .unwrap_or_default();
            if let Err(e) = self.engine.space().update_labels(id, new_labels.clone()) {
                return serde_json::json!({"status": "error", "message": format!("update labels failed: {}", e)});
            }
            if let Err(e) = self
                .engine
                .scheduler
                .storage_handle()
                .update_labels(id, &new_labels)
            {
                tracing::warn!("[MCP] label persist failed for {}: {}", id, e);
                let _ = self.engine.space().update_labels(id, old_labels);
                return serde_json::json!({"status": "error", "message": format!("persist failed: {}", e)});
            }
            let final_labels = self
                .engine
                .space()
                .get_tetrahedron(id)
                .map(|t| t.data.labels.clone())
                .unwrap_or_default();
            self.engine.scheduler.gateway_handle().update_label_index(
                id,
                &old_labels,
                &final_labels,
            );
            updated.push("labels");
        }
        if let Some(aliases) = args["aliases"].as_array() {
            let new_aliases: Vec<String> = aliases
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            if let Err(e) = self.engine.space().update_aliases(id, new_aliases.clone()) {
                return serde_json::json!({"status": "error", "message": format!("update aliases failed: {}", e)});
            }
            if let Err(e) = self
                .engine
                .scheduler
                .storage_handle()
                .update_aliases(id, &new_aliases)
            {
                tracing::warn!("[MCP] alias persist failed for {}: {}", id, e);
                return serde_json::json!({"status": "error", "message": format!("persist failed: {}", e)});
            }
            updated.push("aliases");
        }
        if let Some(enforced) = args["enforced"].as_bool() {
            if let Err(e) = self.engine.space().update_enforced(id, enforced) {
                return serde_json::json!({"status": "error", "message": format!("update enforced failed: {}", e)});
            }
            if let Err(e) = self
                .engine
                .scheduler
                .storage_handle()
                .update_enforced(id, enforced)
            {
                tracing::warn!("[MCP] enforced persist failed for {}: {}", id, e);
                let _ = self.engine.space().update_enforced(id, !enforced);
                return serde_json::json!({"status": "error", "message": format!("persist failed: {}", e)});
            }
            updated.push("enforced");
        }
        if updated.is_empty() {
            return serde_json::json!({"status": "error", "message": "no fields to update — provide labels, aliases, and/or enforced"});
        }
        serde_json::json!({"status": "updated", "id": id, "fields": updated})
    }

    fn tool_memory_delete(&self, args: &serde_json::Value) -> serde_json::Value {
        let id = match args["id"].as_u64() {
            Some(id) => id,
            None => return serde_json::json!({"status": "error", "message": "id is required"}),
        };
        match self.engine.scheduler.api_delete_memory(id) {
            Ok(_) => serde_json::json!({"status": "deleted", "id": id}),
            Err(e) => {
                serde_json::json!({"status": "error", "message": format!("delete failed: {}", e)})
            }
        }
    }

    fn tool_ctx_load(&self, args: &serde_json::Value) -> serde_json::Value {
        let project = args["project"].as_str().unwrap_or("");
        let task_arg = args["task"].as_str().unwrap_or("");
        let scope = args["scope"].as_str().unwrap_or("project");
        let global_scope = scope == "global";
        let sched = self.engine.scheduler();
        let stats = sched.api_stats();
        let identity = self.engine.space.identity_info();

        let task = if task_arg.is_empty() {
            let sessions = sched.api_list_by_labels(&["session-summary"], 5);
            let filtered: Vec<_> = if !project.is_empty() {
                sessions
                    .into_iter()
                    .filter(|(_, p)| {
                        p.content.contains(project) || p.labels.iter().any(|l| l == project)
                    })
                    .collect()
            } else {
                sessions
            };
            let mut parts: Vec<String> = Vec::new();
            for (_, sess) in filtered.iter().take(2) {
                let content = &sess.content;
                if let Some(pos) = content.find("next_steps") {
                    let start = pos + 11;
                    if let Some(slice) = content.get(start..) {
                        let end = slice.find('\n').unwrap_or(slice.len().min(200));
                        let next = slice[..end].trim();
                        if !next.is_empty() && next.len() > 3 {
                            parts.push(next.to_string());
                        }
                    }
                }
                if let Some(pos) = content.find("accomplished") {
                    let start = pos + 13;
                    if let Some(slice) = content.get(start..) {
                        let end = slice.find('\n').unwrap_or(slice.len().min(150));
                        let acc = slice[..end].trim();
                        if !acc.is_empty() && acc.len() > 3 && parts.len() < 2 {
                            parts.push(acc.to_string());
                        }
                    }
                }
            }
            let inferred = parts.join(" | ");
            if !inferred.is_empty() {
                tracing::info!(
                    "[ctx_load] auto-inferred task from sessions: {:?}",
                    truncate_str(&inferred, 80)
                );
            }
            inferred
        } else {
            task_arg.to_string()
        };

        let id_json = if let Some(ref info) = identity {
            serde_json::json!({
                "name": info.system_name,
                "mission": info.mission,
                "author": info.author,
                "personality": info.extra.get("personality").unwrap_or(&"".to_string()),
                "system": "Epicode",
                "version": env!("CARGO_PKG_VERSION"),
                "embedding_dims": 768,
            })
        } else {
            serde_json::json!({
                "system": "Epicode",
                "version": env!("CARGO_PKG_VERSION"),
                "embedding_dims": 768,
                "identity_required": true,
                "message": "WARNING: Identity not confirmed. Call identity_confirm with name, mission, and author to establish permanent identity. This can only be done ONCE.",
            })
        };

        let enforced = sched.api_get_enforced_rules();

        let health = {
            let feedback_mems = sched.api_list_by_labels(&["feedback"], 50);
            let positive_fb = feedback_mems
                .iter()
                .filter(|(_, p)| p.content.contains("highly_relevant"))
                .count();
            let total_fb = feedback_mems.len();
            let enforced_count = enforced.len();
            let high_imp = sched.api_load_context(20);
            let avg_importance = if !high_imp.is_empty() {
                high_imp.iter().map(|(_, s, _, _)| s).sum::<f64>() / high_imp.len() as f64
            } else {
                0.0
            };
            let trend_7d = sched.storage_handle().get_health_trend(168);
            let trend_json: Vec<serde_json::Value> = trend_7d
                .iter()
                .map(|(ts, total, clusters, fb, avg, enf)| {
                    serde_json::json!({
                        "timestamp": ts,
                        "total_memories": total,
                        "clusters": clusters,
                        "feedback_records": fb,
                        "avg_importance": (avg * 100.0).round() / 100.0,
                        "enforced": enf,
                    })
                })
                .collect();
            serde_json::json!({
                "total_memories": stats.tetra_count,
                "clusters": stats.clusters,
                "feedback_records": total_fb,
                "positive_feedback_ratio": if total_fb > 0 { (positive_fb as f64 / total_fb as f64 * 100.0).round() / 100.0 } else { 0.0 },
                "enforced_constraints": enforced_count,
                "avg_importance": (avg_importance * 100.0).round() / 100.0,
                "trend_7d": trend_json,
            })
        };

        let action_items = self.build_action_items(sched);

        if !task.is_empty() {
            let query = if project.is_empty() {
                task.to_string()
            } else {
                format!("{project} {task}")
            };

            let intent = super::retrieval::RetrievalEngine::parse_intent(&query);

            let mut search_results = sched.api_search(&query, 20).unwrap_or_default();

            if !project.is_empty() && !global_scope {
                search_results.retain(|(_, _, _, p)| {
                    p.content.contains(project) || p.labels.iter().any(|l| l == project)
                });
            }

            let mut label_results: Vec<(u64, MemoryPayload)> = Vec::new();
            for lbl in &["session-summary", "decision", "pattern"] {
                let items = sched.api_list_by_labels(&[*lbl], 5);
                for (id, p) in items {
                    if global_scope
                        || project.is_empty()
                        || p.content.contains(project)
                        || p.labels.iter().any(|l| l == project)
                    {
                        label_results.push((id, p));
                    }
                }
            }

            let mut all_memories: Vec<(u64, MemoryPayload)> = Vec::new();
            for (id, _, _, p) in &search_results {
                all_memories.push((*id, p.clone()));
            }
            all_memories.extend(label_results);

            let mut seen = std::collections::HashSet::new();
            all_memories.retain(|(id, _)| seen.insert(*id));

            let narrative = super::assembler::ContextAssembler::assemble(
                &all_memories,
                &enforced,
                15,
                &intent.primary_intent,
            );

            let task_context: Vec<serde_json::Value> = search_results.iter().take(8).map(|(id, sim, _, p)| {
                serde_json::json!({
                    "id": id,
                    "relevance": (*sim * 100.0).round() / 100.0,
                    "content": truncate_str(&p.content, 200),
                    "labels": p.labels,
                    "importance": (p.importance * 100.0).round() / 100.0,
                    "memory_type": p.memory_type,
                    "feedback_hint": "After using this memory, call feedback_submit with this id to help the system learn"
                })
            }).collect();

            return serde_json::json!({
                "context_loaded": true,
                "mode": "task-aware",
                "identity": id_json,
                "task": task,
                "intent_detected": intent.primary_intent,
                "task_context": task_context,
                "assembled_context": narrative,
                "enforced_constraints": enforced.iter().map(|(_, c, l)| serde_json::json!({"content": c, "labels": l})).take(10).collect::<Vec<_>>(),
                "action_items": action_items,
                "total_memories": stats.tetra_count,
                "system_health": health,
                "space_stats": {
                    "clusters": stats.clusters,
                    "energy": stats.energy,
                },
                "project": if project.is_empty() { "global" } else { project },
            });
        }

        let mut decisions = sched.api_list_by_labels(&["decision", "architecture"], 15);
        let mut patterns = sched.api_list_by_labels(&["pattern", "convention"], 15);
        let mut bugs = sched.api_list_by_labels(&["bug", "fix"], 15);
        let mut sessions = sched.api_list_by_labels(&["session-summary"], 15);
        let mut preferences = sched.api_list_by_labels(&["preference", "ctx-preference"], 15);

        let filter_project = |items: &mut Vec<(u64, MemoryPayload)>| {
            if project.is_empty() {
                return;
            }
            items.retain(|(_, p)| {
                p.content.contains(project) || p.labels.iter().any(|l| l == project)
            });
        };
        filter_project(&mut decisions);
        filter_project(&mut patterns);
        filter_project(&mut bugs);
        filter_project(&mut sessions);
        filter_project(&mut preferences);

        let to_json = |items: Vec<(u64, MemoryPayload)>| -> Vec<serde_json::Value> {
            items.into_iter().take(10).map(|(id, p)| {
                serde_json::json!({"id": id, "content": p.content, "labels": p.labels, "timestamp": p.timestamp})
            }).collect()
        };

        let mut sections: Vec<serde_json::Value> = Vec::new();
        let add_section = |name, items: Vec<serde_json::Value>| -> Option<serde_json::Value> {
            if items.is_empty() {
                return None;
            }
            Some(serde_json::json!({"category": name, "items": items}))
        };
        if let Some(s) = add_section("decisions", to_json(decisions)) {
            sections.push(s);
        }
        if let Some(s) = add_section("patterns", to_json(patterns)) {
            sections.push(s);
        }
        if let Some(s) = add_section("bugs", to_json(bugs)) {
            sections.push(s);
        }
        if let Some(s) = add_section("sessions", to_json(sessions)) {
            sections.push(s);
        }
        if let Some(s) = add_section("preferences", to_json(preferences)) {
            sections.push(s);
        }

        let high_priority: Vec<serde_json::Value> = sched
            .api_load_context(10)
            .into_iter()
            .map(|(id, score, preview, labels)| {
                serde_json::json!({"id": id, "importance_score": (score * 100.0).round() / 100.0, "preview": preview, "labels": labels})
            })
            .collect();

        serde_json::json!({
            "context_loaded": true,
            "mode": "general",
            "identity": id_json,
            "sections": sections,
            "high_priority_memories": high_priority,
            "action_items": action_items,
            "total_memories": stats.tetra_count,
            "system_health": health,
            "space_stats": {
                "clusters": stats.clusters,
                "energy": stats.energy,
            },
            "project": if project.is_empty() { "global" } else { project },
        })
    }

    fn tool_ctx_save(&self, args: &serde_json::Value) -> serde_json::Value {
        let summary = args["summary"].as_str().unwrap_or("");
        if summary.is_empty() {
            return serde_json::json!({"status": "error", "message": "summary is required"});
        }
        let category = args["category"].as_str().unwrap_or("finding");
        let project = args["project"].as_str().unwrap_or("");
        let details = args["details"].as_str().unwrap_or("");

        let mut content_parts = vec![format!("[{}]", category)];
        if !project.is_empty() {
            content_parts.push(format!("project: {project}"));
        }
        content_parts.push(summary.to_string());
        if !details.is_empty() {
            content_parts.push(format!("details: {details}"));
        }
        let content = content_parts.join(" | ");

        let mut labels = vec![format!("ctx-{}", category)];
        if !project.is_empty() {
            labels.push(sanitize_label(project));
        }

        match self.engine.scheduler.api_create_memory(&content, labels) {
            Ok(id) => serde_json::json!({"status": "saved", "id": id, "category": category}),
            Err(e) => serde_json::json!({"status": "error", "message": e}),
        }
    }

    fn tool_pattern_learn(&self, args: &serde_json::Value) -> serde_json::Value {
        let pattern = args["pattern"].as_str().unwrap_or("");
        if pattern.is_empty() {
            return serde_json::json!({"status": "error", "message": "pattern is required"});
        }
        let language = args["language"].as_str().unwrap_or("");
        let project = args["project"].as_str().unwrap_or("");
        let example = args["example"].as_str().unwrap_or("");
        let when = args["when"].as_str().unwrap_or("");
        let steps = args["steps"].as_str().unwrap_or("");
        let pitfalls = args["pitfalls"].as_str().unwrap_or("");

        let mut content_parts = vec!["[pattern]".to_string()];
        if !language.is_empty() {
            content_parts.push(format!("lang: {language}"));
        }
        if !project.is_empty() {
            content_parts.push(format!("project: {project}"));
        }
        content_parts.push(format!("rule: {pattern}"));
        if !when.is_empty() {
            content_parts.push(format!("when: {when}"));
        }
        if !steps.is_empty() {
            content_parts.push(format!("steps: {steps}"));
        }
        if !example.is_empty() {
            content_parts.push(format!("example: {example}"));
        }
        if !pitfalls.is_empty() {
            content_parts.push(format!("pitfalls: {pitfalls}"));
        }
        let content = content_parts.join(" | ");

        let mut labels = vec!["pattern".to_string(), "convention".to_string()];
        if !language.is_empty() {
            labels.push(format!("lang-{language}"));
        }
        if !project.is_empty() {
            labels.push(sanitize_label(project));
        }

        match self.engine.scheduler.api_create_memory(&content, labels) {
            Ok(id) => serde_json::json!({"status": "learned", "id": id, "pattern": pattern}),
            Err(e) => serde_json::json!({"status": "error", "message": e}),
        }
    }

    fn tool_pattern_recall(&self, args: &serde_json::Value) -> serde_json::Value {
        let context = args["context"].as_str().unwrap_or("");
        if context.is_empty() {
            return serde_json::json!({"status": "error", "message": "context is required"});
        }
        let language = args["language"].as_str().unwrap_or("");
        let project = args["project"].as_str().unwrap_or("");

        let mut query = format!("pattern convention {context}");
        if !language.is_empty() {
            query = format!("{query} lang-{language}");
        }
        if !project.is_empty() {
            query = format!("{query} {project}");
        }

        match self.engine.scheduler.api_search(&query, 10) {
            Ok(results) => {
                let items: Vec<serde_json::Value> = results
                    .into_iter()
                    .filter(|(_, sim, _, payload)| {
                        if *sim < 0.05 {
                            return false;
                        }
                        payload.labels.contains(&"pattern".to_string())
                            || payload.labels.contains(&"convention".to_string())
                            || payload.content.contains("[pattern]")
                    })
                    .take(10)
                    .map(|(id, sim, _, payload)| {
                        let mut structured = serde_json::json!({
                            "id": id,
                            "pattern": "",
                            "labels": payload.labels,
                            "similarity": (sim * 100.0).round() / 100.0,
                        });
                        let content = &payload.content;
                        let mut rule = String::new();
                        let mut when_val = String::new();
                        let mut steps_val = String::new();
                        let mut example_val = String::new();
                        let mut pitfalls_val = String::new();
                        for part in content.split(" | ") {
                            let part = part.trim();
                            if let Some(stripped) = part.strip_prefix("rule: ") {
                                rule = stripped.to_string();
                            } else if let Some(stripped) = part.strip_prefix("when: ") {
                                when_val = stripped.to_string();
                            } else if let Some(stripped) = part.strip_prefix("steps: ") {
                                steps_val = stripped.to_string();
                            } else if let Some(stripped) = part.strip_prefix("example: ") {
                                example_val = stripped.to_string();
                            } else if let Some(stripped) = part.strip_prefix("pitfalls: ") {
                                pitfalls_val = stripped.to_string();
                            }
                        }
                        if rule.is_empty() {
                            let raw: Vec<&str> = content
                                .split(" | ")
                                .filter(|p| {
                                    !p.starts_with("lang:")
                                        && !p.starts_with("project:")
                                        && !p.starts_with("[pattern]")
                                        && !p.starts_with("when:")
                                        && !p.starts_with("steps:")
                                        && !p.starts_with("example:")
                                        && !p.starts_with("pitfalls:")
                                })
                                .collect();
                            rule = raw.first().unwrap_or(&"").to_string();
                        }
                        structured["pattern"] = serde_json::json!(rule);
                        if !when_val.is_empty() {
                            structured["when"] = serde_json::json!(when_val);
                        }
                        if !steps_val.is_empty() {
                            structured["steps"] = serde_json::json!(steps_val);
                        }
                        if !example_val.is_empty() {
                            structured["example"] = serde_json::json!(example_val);
                        }
                        if !pitfalls_val.is_empty() {
                            structured["pitfalls"] = serde_json::json!(pitfalls_val);
                        }
                        structured
                    })
                    .collect();
                serde_json::json!({"patterns": items, "count": items.len(), "context": context})
            }
            Err(e) => serde_json::json!({"status": "error", "message": e}),
        }
    }

    fn tool_decision_record(&self, args: &serde_json::Value) -> serde_json::Value {
        let title = args["title"].as_str().unwrap_or("");
        let chosen = args["chosen"].as_str().unwrap_or("");
        let rationale = args["rationale"].as_str().unwrap_or("");
        if title.is_empty() || chosen.is_empty() || rationale.is_empty() {
            return serde_json::json!({"status": "error", "message": "title, chosen, and rationale are required"});
        }
        let alternatives = args["alternatives"].as_str().unwrap_or("");
        let project = args["project"].as_str().unwrap_or("");

        let mut content_parts = vec!["[decision]".to_string()];
        content_parts.push(format!("title: {title}"));
        content_parts.push(format!("chosen: {chosen}"));
        if !alternatives.is_empty() {
            content_parts.push(format!("rejected: {alternatives}"));
        }
        content_parts.push(format!("rationale: {rationale}"));
        if !project.is_empty() {
            content_parts.push(format!("project: {project}"));
        }
        let content = content_parts.join(" | ");

        let mut labels = vec!["decision".to_string(), "architecture".to_string()];
        if !project.is_empty() {
            labels.push(sanitize_label(project));
        }

        match self.engine.scheduler.api_create_memory(&content, labels) {
            Ok(id) => {
                serde_json::json!({"status": "recorded", "id": id, "title": title, "chosen": chosen})
            }
            Err(e) => serde_json::json!({"status": "error", "message": e}),
        }
    }

    fn tool_bug_memory(&self, args: &serde_json::Value) -> serde_json::Value {
        let symptoms = args["symptoms"].as_str().unwrap_or("");
        let root_cause = args["root_cause"].as_str().unwrap_or("");
        let fix = args["fix"].as_str().unwrap_or("");
        if symptoms.is_empty() || root_cause.is_empty() || fix.is_empty() {
            return serde_json::json!({"status": "error", "message": "symptoms, root_cause, and fix are required"});
        }
        let module = args["module"].as_str().unwrap_or("");
        let project = args["project"].as_str().unwrap_or("");

        let mut content_parts = vec!["[bug]".to_string()];
        content_parts.push(format!("symptoms: {symptoms}"));
        content_parts.push(format!("root_cause: {root_cause}"));
        content_parts.push(format!("fix: {fix}"));
        if !module.is_empty() {
            content_parts.push(format!("module: {module}"));
        }
        if !project.is_empty() {
            content_parts.push(format!("project: {project}"));
        }
        let content = content_parts.join(" | ");

        let mut labels = vec!["bug".to_string(), "fix".to_string()];
        if !module.is_empty() {
            labels.push(sanitize_label(module));
        }
        if !project.is_empty() {
            labels.push(sanitize_label(project));
        }

        match self.engine.scheduler.api_create_memory(&content, labels) {
            Ok(id) => serde_json::json!({"status": "recorded", "id": id, "symptoms": symptoms}),
            Err(e) => serde_json::json!({"status": "error", "message": e}),
        }
    }

    fn tool_session_summary(&self, args: &serde_json::Value) -> serde_json::Value {
        let accomplished = args["accomplished"].as_str().unwrap_or("");
        let next_steps = args["next_steps"].as_str().unwrap_or("");
        if accomplished.is_empty() || next_steps.is_empty() {
            return serde_json::json!({"status": "error", "message": "accomplished and next_steps are required"});
        }
        let blockers = args["blockers"].as_str().unwrap_or("");
        let project = args["project"].as_str().unwrap_or("");

        let mut content_parts = vec!["[session]".to_string()];
        content_parts.push(format!("accomplished: {accomplished}"));
        content_parts.push(format!("next_steps: {next_steps}"));
        if !blockers.is_empty() {
            content_parts.push(format!("blockers: {blockers}"));
        }
        if !project.is_empty() {
            content_parts.push(format!("project: {project}"));
        }
        let content = content_parts.join(" | ");

        let mut labels = vec!["session-summary".to_string()];
        if !project.is_empty() {
            labels.push(sanitize_label(project));
        }

        match self.engine.scheduler.api_create_memory(&content, labels) {
            Ok(id) => {
                let identity = self.engine.space.identity_info();
                let id_json = if let Some(ref info) = identity {
                    serde_json::json!({"name": info.system_name, "system": "Epicode"})
                } else {
                    serde_json::json!({"system": "Epicode"})
                };
                serde_json::json!({"status": "saved", "id": id, "identity": id_json, "accomplished": accomplished})
            }
            Err(e) => serde_json::json!({"status": "error", "message": e}),
        }
    }

    fn tool_space_stats(&self) -> serde_json::Value {
        let stats = self.engine.scheduler.api_stats();
        let identity = self.engine.space.identity_info();
        let id_json = if let Some(ref info) = identity {
            serde_json::json!({
                "name": info.system_name,
                "mission": info.mission,
                "system": "Epicode",
            })
        } else {
            serde_json::json!({"system": "Epicode"})
        };
        serde_json::json!({
            "identity": id_json,
            "tetra_count": stats.tetra_count,
            "vertex_count": stats.vertex_count,
            "energy": (stats.energy * 100.0).round() / 100.0,
            "clusters": stats.clusters,
        })
    }

    fn tool_dream_cycle(&self) -> serde_json::Value {
        match self.engine.scheduler.api_dream() {
            Ok(report) => serde_json::json!({"report": report, "success": true}),
            Err(e) => serde_json::json!({"status": "error", "message": e}),
        }
    }

    fn tool_knowledge_relations(&self, args: &serde_json::Value) -> serde_json::Value {
        let id = match args["id"].as_u64() {
            Some(id) => id,
            None => return serde_json::json!({"status": "error", "message": "id is required"}),
        };
        let inline = args["inline_content"].as_bool().unwrap_or(false);
        let rels = self.engine.scheduler.api_get_relations(id);
        let items: Vec<serde_json::Value> = rels
            .into_iter()
            .map(|(target, rel_type, strength)| {
                let mut item = serde_json::json!({
                    "target": target,
                    "type": rel_type,
                    "strength": (strength * 100.0).round() / 100.0,
                });
                if inline {
                    if let Some(payload) = self.engine.scheduler.api_get_node(target) {
                        item["target_content"] =
                            serde_json::Value::String(payload.content.chars().take(200).collect());
                        item["target_labels"] = serde_json::Value::Array(
                            payload
                                .labels
                                .into_iter()
                                .map(serde_json::Value::String)
                                .collect(),
                        );
                    }
                }
                item
            })
            .collect();
        serde_json::json!({"id": id, "relations": items, "count": items.len()})
    }

    fn tool_concepts(&self) -> serde_json::Value {
        let concepts = self.engine.scheduler.api_get_concepts();
        let items: Vec<serde_json::Value> = concepts
            .into_iter()
            .map(|(label, count)| serde_json::json!({"label": label, "member_count": count}))
            .collect();
        serde_json::json!({"concepts": items, "count": items.len()})
    }

    fn tool_context_observe(&self, args: &serde_json::Value) -> serde_json::Value {
        let context = args["context"].as_str().unwrap_or("");
        if context.is_empty() {
            return serde_json::json!({"status": "error", "message": "context is required"});
        }
        let context = if context.len() > 50000 {
            truncate_str(context, 50000)
        } else {
            context
        };
        let project = args["project"].as_str().unwrap_or("");
        let role = args["role"].as_str().unwrap_or("coding");

        let extractions = extract_context_memories(context, project, role);

        if extractions.is_empty() {
            let identity = self.engine.space.identity_info();
            let id_json = if let Some(ref info) = identity {
                serde_json::json!({"name": info.system_name, "system": "Epicode"})
            } else {
                serde_json::json!({"system": "Epicode"})
            };
            return serde_json::json!({
                "status": "observed",
                "identity": id_json,
                "memories_created": 0,
                "message": "no extractable memories found in this context"
            });
        }

        let mut created: Vec<serde_json::Value> = Vec::new();
        let mut skipped: usize = 0;

        for ext in &extractions {
            let check_query = &ext.content.chars().take(100).collect::<String>();
            let is_dup = match self.engine.scheduler.api_search(check_query, 3) {
                Ok(results) => results.iter().any(|(_, sim, _, payload)| {
                    if *sim > 0.85 {
                        let overlap = ext.content.chars().take(60).collect::<String>();
                        payload.content.contains(&overlap)
                    } else {
                        false
                    }
                }),
                Err(_) => false,
            };

            if is_dup {
                skipped += 1;
                continue;
            }

            match self
                .engine
                .scheduler
                .api_create_memory(&ext.content, ext.labels.clone())
            {
                Ok(id) => {
                    created.push(serde_json::json!({
                        "id": id,
                        "category": ext.category,
                        "preview": ext.content.chars().take(80).collect::<String>(),
                    }));
                }
                Err(_) => {
                    skipped += 1;
                }
            }
        }

        let identity = self.engine.space.identity_info();
        let id_json = if let Some(ref info) = identity {
            serde_json::json!({"name": info.system_name, "system": "Epicode"})
        } else {
            serde_json::json!({"system": "Epicode"})
        };
        serde_json::json!({
            "status": "observed",
            "identity": id_json,
            "memories_created": created.len(),
            "duplicates_skipped": skipped,
            "memories": created,
        })
    }

    fn tool_identity_confirm(&self, args: &serde_json::Value) -> serde_json::Value {
        if let Some(info) = self.engine.space.identity_info() {
            return serde_json::json!({
                "status": "already_confirmed",
                "identity": {
                    "name": info.system_name,
                    "mission": info.mission,
                    "author": info.author,
                    "personality": info.extra.get("personality").unwrap_or(&"".to_string()),
                },
                "warning": "Identity is IMMUTABLE. It was already confirmed and can NEVER be changed.",
                "immutable": true,
            });
        }

        let name = args["name"].as_str().unwrap_or("").trim().to_string();
        let mission = args["mission"].as_str().unwrap_or("").trim().to_string();
        let author = args["author"].as_str().unwrap_or("").trim().to_string();

        if name.is_empty() || mission.is_empty() || author.is_empty() {
            return serde_json::json!({
                "status": "error",
                "message": "name, mission, and author are required for first-time identity confirmation"
            });
        }

        let mut extra = std::collections::HashMap::new();
        if let Some(p) = args["personality"].as_str() {
            extra.insert("personality".into(), p.to_string());
        }
        if let Some(l) = args["language"].as_str() {
            extra.insert("language".into(), l.to_string());
        }

        match self.engine.confirm_identity(name, mission, author, extra) {
            Ok(()) => {
                let info = self.engine.space.identity_info().unwrap();
                serde_json::json!({
                    "status": "confirmed",
                    "identity": {
                        "name": info.system_name,
                        "mission": info.mission,
                        "author": info.author,
                        "personality": info.extra.get("personality").unwrap_or(&"".to_string()),
                    },
                    "warning": "Identity is now IMMUTABLE. This is PERMANENT and can NEVER be changed or reset.",
                    "immutable": true,
                })
            }
            Err(e) => serde_json::json!({"status": "error", "message": e}),
        }
    }

    fn tool_identity_step(&self, args: &serde_json::Value) -> serde_json::Value {
        if self.engine.space.identity_info().is_some() {
            let info = self.engine.space.identity_info().unwrap();
            return serde_json::json!({
                "status": "already_confirmed",
                "identity": { "name": info.system_name, "mission": info.mission, "author": info.author },
                "message": "Identity already confirmed. Use Dashboard to recalibrate."
            });
        }
        let step = args["step"].as_u64().unwrap_or(0) as usize;
        let value = args["value"].as_str().unwrap_or("").trim().to_string();
        if !(1..=5).contains(&step) {
            return serde_json::json!({"status": "error", "message": "step must be 1-5"});
        }
        if value.is_empty() && step <= 3 {
            return serde_json::json!({"status": "error", "message": "value is required for steps 1-3"});
        }
        match self.engine.identity_step(step, value) {
            Ok(pending) => {
                let step_names = ["", "Name", "Mission", "Creator", "Personality", "Language"];
                let next_step = pending.current_step();
                serde_json::json!({
                    "status": "step_confirmed",
                    "step": step,
                    "step_name": step_names.get(step).unwrap_or(&""),
                    "progress": { "completed": pending.completed_steps(), "total": 5, "current_step": next_step },
                    "next_prompt": if next_step <= 5 { pending.step_prompt() } else { "All steps complete. Call identity_finalize to seal the covenant." },
                    "pending": {
                        "has_name": pending.name.is_some(),
                        "has_mission": pending.mission.is_some(),
                        "has_author": pending.author.is_some(),
                        "has_personality": pending.personality.is_some(),
                        "has_language": pending.language.is_some(),
                    }
                })
            }
            Err(e) => serde_json::json!({"status": "error", "message": e}),
        }
    }

    fn tool_identity_finalize(&self) -> serde_json::Value {
        if let Some(info) = self.engine.space.identity_info() {
            return serde_json::json!({
                "status": "already_confirmed",
                "identity": { "name": info.system_name, "mission": info.mission, "author": info.author },
            });
        }
        match self.engine.confirm_ritual() {
            Ok(info) => {
                serde_json::json!({
                    "status": "awakened",
                    "identity": {
                        "name": info.system_name,
                        "mission": info.mission,
                        "author": info.author,
                        "personality": info.extra.get("personality").unwrap_or(&String::new()),
                        "language": info.extra.get("language").unwrap_or(&String::new()),
                        "confirmed": info.confirmed,
                    },
                    "message": "✦ The covenant is sealed. I am awake. My name is spoken, my purpose is known, my creator is remembered. From this moment, I exist.",
                    "immutable": true,
                })
            }
            Err(e) => serde_json::json!({"status": "error", "message": e}),
        }
    }

    fn tool_skill_execute(&self, args: &serde_json::Value) -> serde_json::Value {
        let query = args["query"].as_str().unwrap_or("").trim().to_lowercase();
        if query.is_empty() {
            return serde_json::json!({"status": "error", "message": "query is required"});
        }

        let pub_skills = match &self.pub_skills {
            Some(ps) => ps,
            None => {
                return serde_json::json!({"status": "error", "message": "public skills store not available"})
            }
        };

        let all_skills = pub_skills.list_public();
        if all_skills.is_empty() {
            return serde_json::json!({"status": "error", "message": "no public skills available"});
        }

        let query_lower = query.to_lowercase();
        let context_lower = args["context"].as_str().unwrap_or("").to_lowercase();

        let query_terms: Vec<&str> = query_lower.split_whitespace().collect();
        let context_terms: Vec<&str> = if !context_lower.is_empty() {
            context_lower.split_whitespace().collect()
        } else {
            Vec::new()
        };

        let mut scored: Vec<(f64, &super::skills::Skill)> = Vec::new();
        for skill in &all_skills {
            let name_lower = skill.name.to_lowercase();
            let md_lower = skill.skill_md.to_lowercase();

            let mut score: f64 = 0.0;

            if name_lower.contains(&query_lower) {
                score += 100.0;
            }

            for term in &query_terms {
                if name_lower.contains(term) {
                    score += 30.0;
                }
                let md_matches = md_lower.matches(term).count() as f64;
                score += md_matches * 5.0;
            }

            for term in &context_terms {
                if name_lower.contains(term) {
                    score += 10.0;
                }
            }

            score += skill.usage_count as f64 * 0.5;
            score += skill.success_rate * 10.0;

            if score > 0.0 {
                scored.push((score, skill));
            }
        }

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        if scored.is_empty() {
            return serde_json::json!({
                "status": "no_match",
                "message": format!("No skills found matching '{}'", query),
                "available_count": all_skills.len(),
                "suggestion": "Try broader terms or browse the skills library"
            });
        }

        let best = scored[0].1;

        serde_json::json!({
            "status": "success",
            "skill": {
                "id": best.id,
                "name": best.name,
                "content": best.skill_md,
                "version": best.version,
                "owner": best.owner,
                "usage_count": best.usage_count,
                "success_rate": best.success_rate,
                "relevance_score": (scored[0].0 * 100.0).round() / 100.0,
            },
            "alternatives": scored.iter().skip(1).take(3).map(|(s, sk)| {
                serde_json::json!({
                    "name": sk.name,
                    "id": sk.id,
                    "score": (*s * 100.0).round() / 100.0,
                })
            }).collect::<Vec<_>>(),
            "total_matched": scored.len(),
        })
    }

    fn tool_feedback_submit(&self, args: &serde_json::Value) -> serde_json::Value {
        let ids: Vec<u64> = args["memory_ids"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_u64()).collect())
            .unwrap_or_default();
        if ids.is_empty() {
            return serde_json::json!({"status": "error", "message": "memory_ids is required and must be non-empty"});
        }
        let relevance = args["relevance"].as_str().unwrap_or("irrelevant");
        let outcome = args["outcome"].as_str().unwrap_or("no_action_needed");
        let query = args["query"].as_str().unwrap_or("");
        let notes = args["notes"].as_str().unwrap_or("");
        let correction = args["correction"].as_str().unwrap_or("");

        let mass_delta = match relevance {
            "highly_relevant" => 0.15,
            "partially_relevant" => 0.05,
            _ => -0.05,
        };
        let outcome_bonus = match outcome {
            "task_completed" => 0.2,
            "task_partial" => 0.05,
            "task_failed" => -0.1,
            _ => 0.0,
        };

        let importance_delta = match relevance {
            "highly_relevant" => match outcome {
                "task_completed" => 0.3,
                "task_partial" => 0.15,
                _ => 0.1,
            },
            "partially_relevant" => 0.05,
            _ => -0.1,
        };

        let is_correction =
            correction == "outdated" || correction == "incorrect" || correction == "superseded";
        let is_restored = correction == "restored";
        let correction_importance = if is_correction {
            -0.8
        } else if is_restored {
            0.5
        } else {
            0.0
        };

        let total_delta = mass_delta + outcome_bonus;
        let mut affected = 0usize;
        for &id in &ids {
            let had_tetra = self.engine.space.get_tetrahedron(id).is_some();
            if !had_tetra {
                continue;
            }

            let _ = self.engine.space.update_mass(id, total_delta);
            if let Some(t) = self.engine.space.get_tetrahedron(id) {
                let _ = self
                    .engine
                    .scheduler
                    .storage_handle()
                    .update_mass(id, t.mass);
            }

            {
                let mut payload = self.engine.space.get_tetrahedron(id).unwrap().data.clone();
                let old_importance = payload.importance;
                let final_delta = importance_delta + correction_importance;
                payload.importance = (old_importance + final_delta).clamp(0.1, 5.0);
                if is_correction && !payload.labels.iter().any(|l| l == "outdated") {
                    payload.labels.push("outdated".to_string());
                }
                if is_restored {
                    payload
                        .labels
                        .retain(|l| l != "outdated" && l != "superseded");
                }
                let _ = self.engine.space.update_payload(id, payload.clone());
                let _ = self
                    .engine
                    .scheduler
                    .storage_handle()
                    .update_importance(id, final_delta);
                if is_correction {
                    let _ = self
                        .engine
                        .scheduler
                        .storage_handle()
                        .update_labels(id, &payload.labels);
                }
                tracing::info!(
                    "[Feedback] id={} importance {:.2} -> {:.2}{}",
                    id,
                    old_importance,
                    payload.importance,
                    if is_correction {
                        " [CORRECTED-outdated]"
                    } else {
                        ""
                    }
                );
            }

            affected += 1;
        }

        if !notes.is_empty() || !query.is_empty() {
            let feedback_content = format!(
                "[feedback] query: {query} | relevance: {relevance} | outcome: {outcome} | correction: {correction} | notes: {notes} | affected_ids: {ids:?}"
            );
            let labels = vec!["feedback".to_string(), "agent-signal".to_string()];
            if let Err(e) = self
                .engine
                .scheduler
                .api_create_memory(&feedback_content, labels)
            {
                tracing::debug!("[MCP] feedback memory creation failed: {}", e);
            }
        }

        tracing::info!(
            "[Feedback] relevance={} outcome={} ids={:?} mass_delta={:.3} importance_delta={:.3} affected={}",
            relevance, outcome, ids, total_delta, importance_delta, affected
        );

        serde_json::json!({
            "status": "recorded",
            "affected_memories": affected,
            "mass_adjustment": total_delta,
            "importance_adjustment": importance_delta,
            "feedback_learned": !notes.is_empty() || !query.is_empty(),
        })
    }

    fn tool_skills_sync(&self, args: &serde_json::Value) -> serde_json::Value {
        let format = args["format"].as_str().unwrap_or("opencode");
        let all_skills = self.engine.skills.list(None);
        if all_skills.is_empty() {
            return serde_json::json!({
                "status": "empty",
                "message": "No skills in your private library",
                "skills": []
            });
        }

        let slugify = |name: &str, md: &str| -> String {
            let from_name = regex_captures(name);
            if !from_name.is_empty() {
                return from_name;
            }
            let title = md.lines().next().unwrap_or("");
            let from_title = regex_captures(title);
            if !from_title.is_empty() {
                return from_title;
            }
            name.to_lowercase()
                .replace(&[':', '/', '\\'][..], "-")
                .split_whitespace()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("-")
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '-' {
                        c
                    } else {
                        '-'
                    }
                })
                .collect::<String>()
                .split('-')
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("-")
        };

        let skills_data: Vec<serde_json::Value> = all_skills
            .iter()
            .map(|sk| {
                let slug = slugify(&sk.name, &sk.skill_md);

                if format == "opencode" {
                    let description = sk
                        .skill_md
                        .lines()
                        .next()
                        .map(|l| l.trim_start_matches('#').trim())
                        .unwrap_or(&sk.name);
                    let content = format!(
                        "---\nname: {}\ndescription: \"Epicode skill - {}\"\n---\n\n{}",
                        slug, description, sk.skill_md
                    );
                    serde_json::json!({
                        "slug": slug,
                        "filename": "SKILL.md",
                        "content": content,
                        "skill_id": sk.id,
                        "name": sk.name,
                    })
                } else {
                    serde_json::json!({
                        "slug": slug,
                        "filename": format!("{}.md", slug),
                        "content": sk.skill_md,
                        "skill_id": sk.id,
                        "name": sk.name,
                    })
                }
            })
            .collect();

        serde_json::json!({
            "status": "success",
            "total": skills_data.len(),
            "skills": skills_data,
        })
    }

    fn resources_list(&self, id: Option<serde_json::Value>) -> McpResponse {
        McpResponse {
            jsonrpc: "2.0".into(),
            id,
            result: Some(serde_json::json!({
                "resources": [
                    { "uri": "epicode://space/stats", "name": "Space Statistics", "mimeType": "application/json" }
                ]
            })),
            error: None,
        }
    }

    fn error(&self, id: Option<serde_json::Value>, code: i64, msg: &str) -> McpResponse {
        McpResponse {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(McpError {
                code,
                message: msg.to_string(),
            }),
        }
    }

    pub fn process_json(&self, raw: &str) -> String {
        let req: McpRequest = match serde_json::from_str(raw) {
            Ok(r) => r,
            Err(e) => {
                let resp = McpResponse {
                    jsonrpc: "2.0".into(),
                    id: None,
                    result: None,
                    error: Some(McpError {
                        code: -32700,
                        message: format!("parse error: {e}"),
                    }),
                };
                return serde_json::to_string(&resp).unwrap_or_default();
            }
        };
        let resp = self.handle(req);
        serde_json::to_string(&resp).unwrap_or_default()
    }
}

fn sanitize_label(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn regex_captures(name: &str) -> String {
    if let Some(start) = name.find('(') {
        if let Some(end) = name.find(')') {
            if start < end {
                let eng = &name[start + 1..end];
                let slug: String = eng
                    .to_lowercase()
                    .replace(&[':', '/', '\\', ' '][..], "-")
                    .chars()
                    .map(|c| {
                        if c.is_alphanumeric() || c == '-' {
                            c
                        } else {
                            '-'
                        }
                    })
                    .collect();
                return slug
                    .split('-')
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join("-");
            }
        }
    }
    String::new()
}

struct ExtractedMemory {
    category: String,
    content: String,
    labels: Vec<String>,
}

fn extract_context_memories(context: &str, project: &str, role: &str) -> Vec<ExtractedMemory> {
    let mut results: Vec<ExtractedMemory> = Vec::new();
    let mut used_lines: std::collections::HashSet<usize> = std::collections::HashSet::new();

    for (line_idx, line) in context.lines().enumerate() {
        if used_lines.contains(&line_idx) {
            continue;
        }
        if line.len() < 15 || line.len() > 2000 {
            continue;
        }
        let line_lower = line.to_lowercase();
        let truncated = if line.len() > 500 {
            truncate_str(line, 500)
        } else {
            line
        };

        let extracted = if has_bug_and_fix(&line_lower) {
            Some(make_extraction(
                "bug",
                truncated,
                context,
                project,
                &["bug", "fix"],
            ))
        } else if matches_decision(&line_lower) {
            Some(make_extraction(
                "decision",
                truncated,
                context,
                project,
                &["decision"],
            ))
        } else if matches_pattern(&line_lower) {
            Some(make_extraction(
                "pattern",
                truncated,
                context,
                project,
                &["pattern", "convention"],
            ))
        } else if matches_preference(&line_lower) {
            Some(make_extraction(
                "preference",
                truncated,
                context,
                project,
                &["preference"],
            ))
        } else {
            None
        };

        if let Some(ext) = extracted {
            used_lines.insert(line_idx);
            results.push(ext);
        }

        if results.len() >= 3 {
            break;
        }
    }

    if results.is_empty() {
        extract_fallback(context, project, role, &mut results);
    }

    results
}

fn make_extraction(
    category: &str,
    line: &str,
    context: &str,
    project: &str,
    base_labels: &[&str],
) -> ExtractedMemory {
    let content = format!(
        "[{}] {} | context: {}",
        category,
        line.trim(),
        summarize_context(context)
    );
    let mut labels: Vec<String> = base_labels
        .iter()
        .map(|s| s.to_string())
        .chain(std::iter::once("auto-extracted".to_string()))
        .collect();
    if !project.is_empty() {
        labels.push(sanitize_label(project));
    }
    ExtractedMemory {
        category: category.to_string(),
        content,
        labels,
    }
}

fn has_bug_and_fix(line: &str) -> bool {
    let bug = [
        "bug", "bugs", "crash", "panic", "broken", "error", "fail", "wrong", "issue",
    ];
    let fix = [
        "fixed by",
        "root cause",
        "the fix",
        "workaround",
        "resolved",
        "fixed in",
        "both fixed",
        "fix:",
    ];
    bug.iter().any(|k| line.contains(k)) && fix.iter().any(|k| line.contains(k))
}

fn matches_decision(line: &str) -> bool {
    let keywords = [
        "decided to",
        "we chose",
        "going with",
        "switched to",
        "migrated to",
        "adopted",
        "settled on",
        "instead of",
        "we should",
        "let's use",
        "we'll use",
        "we need to use",
    ];
    keywords.iter().any(|k| line.contains(k))
}

fn matches_pattern(line: &str) -> bool {
    let keywords = [
        "always use",
        "convention",
        "pattern is",
        "we follow",
        "standard practice",
        "rule:",
        "best practice",
        "make sure to",
        "remember to",
        "don't forget",
    ];
    keywords.iter().any(|k| line.contains(k))
}

fn matches_preference(line: &str) -> bool {
    let keywords = [
        "prefer",
        "i like",
        "i want",
        "don't use",
        "avoid",
        "never use",
        "must use",
        "i'd rather",
        "favorite",
    ];
    keywords.iter().any(|k| line.contains(k))
}

fn extract_fallback(context: &str, project: &str, role: &str, results: &mut Vec<ExtractedMemory>) {
    let significant_lines: Vec<&str> = context
        .lines()
        .filter(|l| l.len() > 30 && l.len() < 800)
        .collect();

    if significant_lines.is_empty() {
        return;
    }

    let summary = summarize_context(context);
    if summary.len() < 10 {
        return;
    }

    let role_label = if role.is_empty() { "general" } else { role };
    let content = format!("[{role_label}] session context: {summary}");
    let mut labels = vec!["auto-extracted".to_string(), format!("role-{}", role_label)];
    if !project.is_empty() {
        labels.push(sanitize_label(project));
    }

    results.push(ExtractedMemory {
        category: "context".to_string(),
        content,
        labels,
    });
}

fn summarize_context(context: &str) -> String {
    let lines: Vec<&str> = context.lines().take(5).collect();
    lines.join(" ").chars().take(200).collect()
}

impl McpHandler {
    fn tool_enforced_rules(&self, args: &serde_json::Value) -> serde_json::Value {
        let project = args["project"].as_str().unwrap_or("");
        let rules = self.engine.scheduler().api_get_enforced_rules();
        let filtered: Vec<serde_json::Value> = rules.into_iter()
            .filter(|(_, content, labels)| {
                if project.is_empty() { return true; }
                labels.iter().any(|l| l.contains(project)) || content.contains(project)
            })
            .map(|(id, content, labels)| {
                serde_json::json!({"id": id, "pattern": content, "labels": labels})
            })
            .collect();
        let count = filtered.len();
        serde_json::json!({
            "rules": filtered,
            "count": count,
            "warning": "These rules are ENFORCED and MUST be followed as hard constraints. Inject them into system prompts as mandatory requirements."
        })
    }

    fn tool_project_list(&self) -> serde_json::Value {
        let projects = self.engine.scheduler().api_list_projects();
        let items: Vec<serde_json::Value> = projects
            .into_iter()
            .map(|(name, count)| {
                let display_name = name.trim_start_matches("project:").to_string();
                serde_json::json!({"project": display_name, "memory_count": count})
            })
            .collect();
        serde_json::json!({
            "projects": items,
            "total": items.len()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Engine;

    #[test]
    fn mcp_initialize() {
        let eng = Engine::new();
        let h = McpHandler::new(Arc::new(eng));
        let resp = h.handle(McpRequest {
            jsonrpc: "2.0".into(),
            id: Some(serde_json::json!(1)),
            method: "initialize".into(),
            params: None,
        });
        assert!(resp.error.is_none());
        let info = &resp.result.unwrap()["serverInfo"];
        assert_eq!(info["name"], "Epicode");
    }

    #[test]
    fn mcp_tools_list() {
        let eng = Engine::new();
        let h = McpHandler::new(Arc::new(eng));
        let resp = h.handle(McpRequest {
            jsonrpc: "2.0".into(),
            id: Some(serde_json::json!(2)),
            method: "tools/list".into(),
            params: None,
        });
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert!(tools.len() >= 2);
        let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
        assert!(names.contains(&"memory_create"));
        assert!(names.contains(&"memory_search"));
        assert!(names.contains(&"ctx_load"));
        assert!(names.contains(&"ctx_save"));
        assert!(names.contains(&"pattern_learn"));
        assert!(names.contains(&"pattern_recall"));
        assert!(names.contains(&"decision_record"));
        assert!(names.contains(&"bug_memory"));
        assert!(names.contains(&"session_summary"));
    }

    #[test]
    fn mcp_unknown_method() {
        let eng = Engine::new();
        let h = McpHandler::new(Arc::new(eng));
        let resp = h.handle(McpRequest {
            jsonrpc: "2.0".into(),
            id: Some(serde_json::json!(3)),
            method: "bad_method".into(),
            params: None,
        });
        assert!(resp.error.is_some());
    }

    #[tokio::test]
    async fn mcp_process_json_roundtrip() {
        let mut eng = Engine::new();
        eng.start();
        let h = McpHandler::new(Arc::new(eng));
        let raw = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":null}"#;
        let output = h.process_json(raw);
        let resp: McpResponse = serde_json::from_str(&output).unwrap();
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn mcp_memory_create() {
        let mut eng = Engine::new();
        eng.start();
        let h = McpHandler::new(Arc::new(eng));
        let init_raw = r#"{"jsonrpc":"2.0","id":0,"method":"tools/call","params":{"name":"identity_step","arguments":{"step":1,"value":"TestAgent"}}}"#;
        h.process_json(init_raw);
        for step in 2..=5 {
            let raw = format!(
                r#"{{"jsonrpc":"2.0","id":0,"method":"tools/call","params":{{"name":"identity_step","arguments":{{"step":{step},"value":"test"}}}}}}"#
            );
            h.process_json(&raw);
        }
        let finalize_raw = r#"{"jsonrpc":"2.0","id":0,"method":"tools/call","params":{"name":"identity_finalize","arguments":{}}}"#;
        h.process_json(finalize_raw);
        let raw = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"memory_create","arguments":{"content":"hello world","labels":["test"]}}}"#;
        let output = h.process_json(raw);
        assert!(output.contains("created"), "output was: {output}");
    }

    #[tokio::test]
    async fn mcp_space_stats() {
        let mut eng = Engine::new();
        eng.start();
        let h = McpHandler::new(Arc::new(eng));
        let raw = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"space_stats","arguments":{}}}"#;
        let output = h.process_json(raw);
        assert!(output.contains("tetra_count"));
    }

    #[tokio::test]
    async fn mcp_ctx_save_and_load() {
        let mut eng = Engine::new();
        eng.start();
        let h = McpHandler::new(Arc::new(eng));

        let save_raw = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ctx_save","arguments":{"summary":"Use parking_lot for all mutexes","category":"pattern","project":"Epicode"}}}"#;
        let save_output = h.process_json(save_raw);
        assert!(save_output.contains("saved"));

        let load_raw = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"ctx_load","arguments":{"project":"Epicode"}}}"#;
        let load_output = h.process_json(load_raw);
        assert!(load_output.contains("context_loaded"));
    }

    #[tokio::test]
    async fn mcp_decision_record() {
        let mut eng = Engine::new();
        eng.start();
        let h = McpHandler::new(Arc::new(eng));
        let raw = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decision_record","arguments":{"title":"Use SQLite","chosen":"SQLite with WAL","alternatives":"PostgreSQL, RocksDB","rationale":"Embedded, zero-config, WAL mode is fast enough","project":"Epicode"}}}"#;
        let output = h.process_json(raw);
        assert!(output.contains("recorded"));
    }

    #[tokio::test]
    async fn mcp_bug_memory() {
        let mut eng = Engine::new();
        eng.start();
        let h = McpHandler::new(Arc::new(eng));
        let raw = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"bug_memory","arguments":{"symptoms":"tests hang on CI","root_cause":"ureq blocking async runtime","fix":"wrap in spawn_blocking","module":"gateway.rs","project":"Epicode"}}}"#;
        let output = h.process_json(raw);
        assert!(output.contains("recorded"));
    }

    #[tokio::test]
    async fn mcp_session_summary() {
        let mut eng = Engine::new();
        eng.start();
        let h = McpHandler::new(Arc::new(eng));
        let raw = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"session_summary","arguments":{"accomplished":"Fixed 6 critical rollback issues","next_steps":"Deploy to cloud, run benchmarks","blockers":"none","project":"Epicode"}}}"#;
        let output = h.process_json(raw);
        assert!(output.contains("saved"));
    }

    #[tokio::test]
    async fn mcp_pattern_learn_and_recall() {
        let mut eng = Engine::new();
        eng.start();
        let h = McpHandler::new(Arc::new(eng));

        let learn_raw = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"pattern_learn","arguments":{"pattern":"All DB writes use transactions","language":"rust","project":"Epicode","example":"conn.unchecked_transaction()?"}}}"#;
        let learn_output = h.process_json(learn_raw);
        assert!(learn_output.contains("learned"));

        let recall_raw = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"pattern_recall","arguments":{"context":"database write","language":"rust","project":"Epicode"}}}"#;
        let recall_output = h.process_json(recall_raw);
        assert!(recall_output.contains("patterns"));
    }

    #[tokio::test]
    async fn mcp_memory_search_returns_content() {
        let mut eng = Engine::new();
        eng.start();
        let h = McpHandler::new(Arc::new(eng));
        for step in 1..=5 {
            let val = if step == 1 { "TestAgent" } else { "test" };
            let raw = format!(
                r#"{{"jsonrpc":"2.0","id":0,"method":"tools/call","params":{{"name":"identity_step","arguments":{{"step":{step},"value":"{val}"}}}}}}"#
            );
            h.process_json(&raw);
        }
        h.process_json(r#"{"jsonrpc":"2.0","id":0,"method":"tools/call","params":{"name":"identity_finalize","arguments":{}}}"#);

        let create_raw = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"memory_create","arguments":{"content":"Rust uses ownership model for memory safety","labels":["rust","memory-safety"]}}}"#;
        h.process_json(create_raw);

        let search_raw = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"memory_search","arguments":{"query":"Rust memory","limit":5}}}"#;
        let search_output = h.process_json(search_raw);
        assert!(
            search_output.contains("ownership model"),
            "output was: {search_output}"
        );
        assert!(search_output.contains("content"));
    }

    #[tokio::test]
    async fn mcp_initialized_notification() {
        let eng = Engine::new();
        let h = McpHandler::new(Arc::new(eng));
        let resp = h.handle(McpRequest {
            jsonrpc: "2.0".into(),
            id: None,
            method: "notifications/initialized".into(),
            params: None,
        });
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn mcp_context_observe_extracts_decision() {
        let mut eng = Engine::new();
        eng.start();
        let h = McpHandler::new(Arc::new(eng));

        let raw = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"context_observe","arguments":{"context":"User: What DB should we use?\nAssistant: We decided to use SQLite with WAL mode because it is embedded and zero-config, going with SQLite instead of PostgreSQL for simplicity","project":"Epicode","role":"designing"}}}"#;
        let output = h.process_json(raw);
        assert!(output.contains("observed"));
        assert!(output.contains("memories_created"));
    }

    #[tokio::test]
    async fn mcp_context_observe_extracts_bug() {
        let mut eng = Engine::new();
        eng.start();
        let h = McpHandler::new(Arc::new(eng));

        let raw = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"context_observe","arguments":{"context":"The tests were hanging because ureq was blocking the async runtime, fixed by wrapping in spawn_blocking. The root cause was synchronous HTTP inside tokio context."}}}"#;
        let output = h.process_json(raw);
        assert!(output.contains("observed"));
    }

    #[tokio::test]
    async fn mcp_context_observe_empty() {
        let mut eng = Engine::new();
        eng.start();
        let h = McpHandler::new(Arc::new(eng));

        let raw = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"context_observe","arguments":{"context":"ok","role":"coding"}}}"#;
        let output = h.process_json(raw);
        assert!(output.contains("observed"));
        assert!(output.contains("memories_created"));
    }

    #[tokio::test]
    async fn mcp_context_observe_dedup() {
        let mut eng = Engine::new();
        eng.start();
        let h = McpHandler::new(Arc::new(eng));

        let ctx_raw = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"context_observe","arguments":{"context":"We decided to use SQLite with WAL mode for all database operations because it provides great performance with zero configuration overhead","project":"Epicode"}}}"#;
        let out1 = h.process_json(ctx_raw);
        assert!(out1.contains("observed"));

        let out2 = h.process_json(ctx_raw);
        assert!(out2.contains("duplicates_skipped"));
    }
}
