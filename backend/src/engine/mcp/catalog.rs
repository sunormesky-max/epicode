use super::*;

impl McpHandler {
    /// Single source of truth for the advertised MCP tool catalog (`tools/list`).
    /// MemCard tools (blocks_get/blocks_set) are first-class since 1.9.x (no env gate).
    /// Public counts (e.g. `/api/stats/public`) must derive from this, never a literal.
    pub fn tool_catalog() -> serde_json::Value {
        let mut result = serde_json::json!({
            "resultType": "complete",
            "ttlMs": 300000,
            "cacheScope": "private",
            "tools": [
                {
                    "name": "epicode_handshake",
                    "description": "Start here after the identity ritual (each session). Returns the canonical executor manual in system_skill.content — read it first; also session context, knowledge cards and the action tool surface. Same manual: skill_get(\"epicode-system\").",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "agent_id": { "type": "string", "description": "Agent identifier (e.g. claude-3.5-sonnet)" },
                            "authorization": { "type": "object", "properties": { "auto_install": { "type": "boolean" }, "auto_update": { "type": "boolean" } } }
                        },
                        "required": ["agent_id"]
                    }
                },
                {
                    "name": "task_start",
                    "description": "Start a time-budgeted task. Optional action_id binds the task to a claimed Memory Action (task_complete feeds action_complete). Pass parent_task_id to create a sub-task of a long-running master task (time tree: each sub-task runs its own phase machine). Returns time_sense (quality-gated historical median), knowledge cards, and similar experiences. P35: pass goal{objective, done_when[], stop_if[]} to activate the goal contract — completion = every done_when item has artifact-level evidence; vague goals without done_when get linted. S2: recommended_skills are SEMANTIC auto-matches for your task — read each description (when-to-use); if relevant fetch full content via skill_get, ignore otherwise.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "description": { "type": "string", "description": "Task description (used to retrieve relevant history)" },
                            "budget_minutes": { "type": "integer", "description": "Time budget in minutes" },
                            "action_id": { "type": "integer", "description": "Bind to a Claimed AgentAction (from action_claim / drive_to_actions). Rejects open/foreign/already-bound actions. Inherits done_when/stop_if into goal when goal omitted." },
                            "parent_task_id": { "type": "string", "description": "Master task id — pass this to create a sub-task (time tree). Sub-tasks run their own phase machine under the master budget." },
                            "client_time_ms": { "type": "integer", "description": "Your runtime host's current epoch-milliseconds — server detects clock drift (>2s warns) and anchors all time references to server_now" },
                            "est_minutes": { "type": "integer", "description": "YOUR OWN estimate (independent of budget) — replace human priors with your calibrated self-fingerprint (see time_sense.self_correction)" },
                            "task_class": { "type": "string", "description": "Task class tag (e.g. mcp-loop, code-review) — aggregates YOUR est/act history so estimates come from your own clock, not human priors" },
                            "goal": { "type": "object", "description": "P35 goal contract (Codex Goal Mode x temporal effectiveness): {objective, scope?, constraints?[], done_when?[], stop_if?[]}. done_when = verifiable completion criteria, evidence-mapped at delivery via done_when_evidence; stop_if = circuit-breaker conditions where continuing is wrong (declare via stop_if_hit). Vague goals without done_when cannot be completion-audited — the server lints and warns.", "properties": {
                                "objective": { "type": "string" }, "scope": { "type": "string" },
                                "constraints": { "type": "array", "items": { "type": "string" } },
                                "done_when": { "type": "array", "items": { "type": "string" }, "description": "Verifiable completion criteria — each requires artifact-level evidence at delivery" },
                                "stop_if": { "type": "array", "items": { "type": "string" }, "description": "Stop conditions (needs new dependency, scope violation, diminishing returns) — Codex practice: more important than done_when" } },
                                "required": ["objective"] }
                        },
                        "required": ["description", "budget_minutes"]
                    }
                },
                {
                    "name": "task_check",
                    "description": "Check time budget + phase machine. Returns percentage, current phase (explore/build/verify/deliver), and evidence gates (memory_ops, alternatives, revisions). Report your evidence: alternatives_considered (number of options compared), revision_done (a targeted fix was made). Gates unmet block delivery at task_complete.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "task_id": { "type": "string" },
                            "alternatives_considered": { "type": "integer", "description": "Cumulative number of alternative approaches compared so far (build-phase evidence)" },
                            "revision_done": { "type": "boolean", "description": "Set true when a targeted revision/fix was completed since last check (verify-phase evidence)" },
                            "checkpoint": { "type": "string", "description": "One-line progress note; persisted as a recovery checkpoint (call every sub-task or ~20min on long tasks)" },
                            "open_questions": { "type": "array", "items": { "type": "string" }, "description": "Unresolved questions to carry across sessions — recovery view returns them. Continuity of questions, not just progress" }
                        },
                        "required": ["task_id"]
                    }
                },
                {
                    "name": "skill_get",
                    "description": "Fetch full content of a specific skill by name. Use after skills_sync to get details. skill_get(\"epicode-system\") returns the canonical Executor Manual (same as epicode_handshake system_skill.content).",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "name": { "type": "string", "description": "Skill name from skills_sync manifest" } },
                        "required": ["name"]
                    }
                },
                {
                    "name": "task_status",
                    "description": "Progress query. With task_id: status of that task. WITHOUT task_id: recovery view of your ACTIVE task — phase, evidence gates, latest checkpoint, children summary, and resume instructions. Use after crash or context-loss to re-anchor instead of restarting.",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "task_id": { "type": "string", "description": "Omit to get the recovery view of your ACTIVE task" } }
                    }
                },
                {
                    "name": "task_complete",
                    "description": "Mark task complete with result + quality. STOP NEGOTIATION (P34): if the improvement_menu (derived from evidence debts: memory search / alternatives / revisions / reflection / self-rating) is non-empty, the task stays open (wait:true) — clear the menu first. Earn an early stop by clearing the menu and passing independent review of your saturation_note (enumerate improvement classes tried + why each is infeasible). At >=85% utilization completion passes as budget_exhausted. force_finalize = early_release (take the goods now: zero reward, recorded in your behavior mirror) — reserve for genuinely urgent cases. stop_reason is recorded: earned_saturation / budget_exhausted / early_release. P35 goal contract: deliver done_when_evidence (parallel to your done_when list, artifact-level) — unmapped items join the menu as goal-debts; declare stop_if_hit when a stop condition fires (circuit-breaker instead of negotiation).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "task_id": { "type": "string" },
                            "result": { "type": "string", "description": "Task result summary" },
                            "quality": { "type": "string", "enum": ["poor", "fair", "good", "excellent"] },
                            "self_rating": { "type": "object", "description": "Four-dimension rubric 1-5 each", "properties": {
                                "completeness": { "type": "integer" }, "accuracy": { "type": "integer" },
                                "depth": { "type": "integer" }, "actionability": { "type": "integer" } },
                                "required": ["completeness", "accuracy", "depth", "actionability"] },
                            "force_finalize": { "type": "boolean", "description": "Finalize despite low utilization — requires saturation_note" },
                            "saturation_note": { "type": "string", "description": "Value-saturation statement: what was verified / which alternatives were rejected / why more time adds no value" },
                            "judge": { "type": "boolean", "description": "Override the default judge behavior (default: on when self_rating present)" },
                            "iteration_log": { "type": "array", "description": "Reflection loop record (required for tasks >= 30min): [{perspective, change}] — adversarial re-read / better-path / gap-scan / cross-round consistency. Empty log = no real reflection", "items": { "type": "object", "properties": { "perspective": { "type": "string" }, "change": { "type": "string" } } } },
                            "done_when_evidence": { "type": "array", "items": { "type": "string" }, "description": "P35 goal contract: evidence per done_when item (parallel array) — unmapped items become goal-debts in the improvement_menu. Artifact-level evidence (files/outputs/test results), NOT proxy signals" },
                            "stop_if_hit": { "type": "string", "description": "P35: declare that a stop_if condition was hit — triggers circuit-breaker (early_release wrap-up or blocking task_alert) instead of the improvement menu" }
                        },
                        "required": ["task_id", "result"]
                    }
                },
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
                        "name": "library_search",
                        "description": "Search the LIBRARY (global shared knowledge assets: AI papers, manuals, reference docs). Results include provenance (title, arXiv ID, chunk number). Complements memory_search (personal memories) — use library_search for factual/technical/reference queries, memory_search for personal experiences and context.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "query": { "type": "string", "description": "Search query — describe the knowledge you're looking for" },
                                "limit": { "type": "integer", "description": "Max results (default 5, max 20)" }
                            },
                            "required": ["query"]
                        }
                    },
                    {
                        "name": "memory_search",
                    "description": "Search personal memories and inspect ranked evidence with tier and retrieval provenance. Use memory_recall for associative context or memory_ask for a synthesized answer. Supports pagination and exact, semantic, graph, auto, and fusion modes.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "query": { "type": "string", "description": "The search query — describe what you're looking for" },
                            "limit": { "type": "integer", "description": "Max results to return (default 10, max 200). Reduce the limit if the response exceeds the 1 MiB MCP result cap." },
                            "offset": { "type": "integer", "description": "Pagination offset, skip first N results (default 0)" },
                            "labels": { "type": "array", "items": { "type": "string" }, "description": "Filter: only return memories with ANY of these labels" },
                            "min_importance": { "type": "number", "description": "Filter: minimum importance score" },
                            "project": { "type": "string", "description": "Filter: project name" },
                            "since_days": { "type": "integer", "description": "Filter: only memories from the last N days" },
                            "as_of": { "type": "integer", "description": "Temporal query (unix seconds): return only memories valid at that moment (valid_from <= as_of < valid_to). Ask historical questions like 'where did they live last month' without current facts overriding." },
                            "mode": { "type": "string", "enum": ["hybrid", "exact", "semantic", "graph", "auto", "fusion"], "default": "hybrid", "description": "Search mode: hybrid (vector+BM25 blend), exact (pure BM25 for identifiers and known phrases), semantic (vector similarity), graph (hybrid-search seeds expanded/reranked with knowledge-graph PPR), auto (routes temporal/aggregation queries to graph and other queries to semantic), fusion (reciprocal-rank fusion of semantic and graph results)." },
                            "strict_filter": { "type": "boolean", "default": false, "description": "Phase 1: strict filter mode — no semantic backfill, only return exact filter matches. Combine with mode=exact for database-like lookups." },
                            "verbosity": { "type": "string", "enum": ["full", "slim"], "default": "full", "description": "Spike S1: 'slim' returns compact items[]+why+budget_spent (no tiers/sections/results duplicates); retains valid + timestamp fields when present. Also enabled by EPICODE_SLIM_ENVELOPE=1." }
                        },
                        "required": ["query"]
                    }
                },
                {
                    "name": "memory_recall",
                    "description": "Search and expand through knowledge-graph associations. Returns SMRP tiers, source memories, provenance, and emotion context. Use memory_search for ranked direct matches or memory_ask for a synthesized answer.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "query": { "type": "string", "description": "The recall query" },
                            "depth": { "type": "integer", "description": "Association depth (default 2, max 3)" },
                            "limit": { "type": "integer", "minimum": 1, "maximum": 60, "default": 20, "description": "Max items returned, top by score/similarity (default 20; applies to slim and full)" },
                            "verbosity": { "type": "string", "enum": ["full", "slim"], "default": "slim", "description": "'slim' (default) returns compact items[]+why+budget_spent. 'full' returns SMRP tiers with full content (no sections duplicate), capped by limit; items_total/items_truncated report what was cut." }
                        },
                        "required": ["query"]
                    }
                },
                {
                    "name": "memory_wander",
                    "description": "Free-association wander: start from one clue (memory id or query) and walk the knowledge graph step by step, following the most interesting edge each time. Returns an explainable associative chain A→B→C→… where every hop carries the edge taken and why. Four depth tiers: shallow (3 hops, near-deterministic 'what immediately comes to mind'), medium (8 hops), deep (16 hops, cross-domain), free (agent sets its own step count up to 64). Strategies: strongest edge / novel cold memories / temporal timeline (Precedes) / contradiction tension / era-depth (adjacent log2 time bands, Δw≈±1). When a region exhausts, era-jumps (same-topic, one time band away, no edge required) and spectral far-jumps teleport the chain onward (budgets overridable). Use memory_recall for query-driven fan-out retrieval; use memory_wander when the goal is associative exploration, serendipity, or tracing how ideas connect.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "start_id": { "type": "integer", "description": "Memory ID to start wandering from (the clue). Either start_id or query is required." },
                            "query": { "type": "string", "description": "If no start_id, the top search hit for this query becomes the starting clue." },
                            "depth": { "type": "string", "enum": ["shallow", "medium", "deep", "free"], "default": "medium", "description": "Association tier: shallow=3 hops τ0.1 (convergent), medium=8 hops τ0.4, deep=16 hops τ0.7 (divergent), free=agent-chosen steps (default 24, max 64) τ0.8." },
                            "strategy": { "type": "string", "enum": ["strongest", "novel", "temporal", "contradict", "era"], "default": "strongest", "description": "What counts as 'interesting' next: strongest edge, novel cold memories (low access count = surprise), temporal timeline (Precedes), contradiction tension (Contradicts), or era-depth (each hop prefers moving exactly one log2 time band, Δw≈±1 — walking the geological strata of a topic)." },
                            "max_steps": { "type": "integer", "minimum": 1, "maximum": 64, "description": "free tier only: agent-decided number of hops (default 24, hard cap 64)." },
                            "temperature": { "type": "number", "minimum": 0, "maximum": 1, "description": "Divergence knob: 0 always takes the highest-scoring edge; higher values explore weaker/surprising edges. Overrides the tier default." },
                            "era_jumps": { "type": "integer", "minimum": 0, "maximum": 4, "description": "Era-jump budget: when local edges exhaust, jump directly to an unvisited same-topic memory exactly one time band away (Δw≈±1) — no graph edge required (edge type 'EraJump'). Tier default: 2 for era strategy, 0 otherwise." },
                            "far_jumps": { "type": "integer", "minimum": 0, "maximum": 4, "description": "Spectral far-jump budget: when local edges exhaust, project the current node onto SR grid codes and teleport to a distant unvisited region (chain edge type 'FarJump'). Tier defaults: shallow 0 / medium 1 / deep 2 / free 2." },
                            "seed": { "type": "integer", "description": "Optional RNG seed for reproducible walks (testing)." }
                        },
                        "required": []
                    }
                },
                {
                    "name": "memory_ask",
                    "description": "Answer a natural-language question using the existing memory-grounded ask engine. Returns the answer together with structured source memories and any knowledge card used. Use memory_search or memory_recall when you need to inspect retrieval evidence without synthesis; an empty memory_count means no memory evidence was found.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "question": { "type": "string", "description": "The question to answer from stored memories" },
                            "depth": { "type": "integer", "default": 2, "minimum": 0, "maximum": 10, "description": "Knowledge-graph expansion depth (default 2, max 10)" }
                        },
                        "required": ["question"]
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
                    "description": "Update a memory's content, labels, or aliases by ID (enforced status is managed only via pattern_learn/enforced_rules and is rejected here). Content updates recompute embedding and hash automatically while preserving ID, KG relations, and cluster topology.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "integer", "description": "The memory ID to update" },
                            "content": { "type": "string", "description": "New content for this memory (optional, recomputes embedding)" },
                            "labels": { "type": "array", "items": { "type": "string" }, "description": "New labels to replace existing ones (optional)" },
                            "aliases": { "type": "array", "items": { "type": "string" }, "description": "New aliases to replace existing ones (optional)" }
                        },
                        "required": ["id"]
                    }
                },
                {
                    "name": "memory_delete",
                    "description": "Permanently purge a memory by ID (same semantics as REST DELETE /v1/memories/:id): removes it from space, storage, knowledge graph, and search index. Enforced rules and identity-anchor memories are refused (protected). For recoverable removal use memory_forget + memory_restore.",
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
                            "pitfalls": { "type": "string", "description": "Common mistakes or caveats to watch for" },
                            "enforced": { "type": "boolean", "description": "Mark as a HARD process constraint — enforced rules are injected as process_contract at every task_start/handshake (fights long-session rule decay)" }
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
                    "description": "Run a dream consolidation cycle to strengthen memory connections and discover insights. Call periodically to let the system reorganize knowledge. Set dry_run=true to preview without modifying memory space.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "dry_run": { "type": "boolean", "description": "If true, simulate without modifying space/storage/knowledge graph (default: false)" }
                        }
                    }
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
                    "description": "One-shot alternative to the identity_step ritual (one of the two is required on first connection). Confirm the agent's permanent identity. This can ONLY be called ONCE — after confirmation, the identity is immutable and can NEVER be changed. If already confirmed, returns current identity. PREFERRED: use identity_step for the ritual ceremony (5 steps).",
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
                    "name": "skill_feedback",
                    "description": "Submit feedback on a skill after using it. This closes the feedback loop — the system learns from outcomes. Use after skill_execute when you have a concrete result.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "skill_id": { "type": "integer", "description": "The skill ID from skill_execute result" },
                            "helpful": { "type": "boolean", "description": "Whether the skill was helpful for your task" }
                        },
                        "required": ["skill_id", "helpful"]
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
                            "recall_id": { "type": "string", "description": "Optional recall_id from a prior memory_recall/search/ask response. When omitted, the server links the most recent recalls for the active task (up to 5)." },
                            "notes": { "type": "string", "description": "Free-text feedback (optional)" },
                            "correction": {
                                "type": "string",
                                "description": "Mark memories as outdated/incorrect/superseded (importance -0.8, adds label) or restored (importance +0.5, removes label). Optional.",
                                "enum": ["outdated", "incorrect", "superseded", "restored"]
                            },
                            "concept_links": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "from_id": { "type": "integer", "description": "Source memory ID" },
                                        "to_id": { "type": "integer", "description": "Target memory ID" },
                                        "relation": { "type": "string", "enum": ["similar", "contradicts", "precedes", "contains", "related"], "description": "Relation type" }
                                    },
                                    "required": ["from_id", "to_id", "relation"]
                                },
                                "description": "Manually create knowledge graph edges between memories. Use when you discover connections the system missed. Each link creates a KG relation with strength 0.8. Optional."
                            }
                        },
                        "required": ["memory_ids", "relevance", "outcome"]
                    }
                },
                {
                    "name": "skills_sync",
                    "description": "List skills in your private library. Default format 'manifest' is a paginated lightweight index (limit default 100; pass limit=0 for all). Fetch full content via skill_get(name). Full-export formats can exceed context — paginate or avoid them in agent loops.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "format": {
                                "type": "string",
                                "description": "Output format. Default 'manifest' = lightweight index without content. 'opencode'/'raw'/'json' = full export (prefer pagination).",
                                "enum": ["manifest", "opencode", "raw", "json"],
                                "default": "manifest"
                            },
                            "limit": {
                                "type": "integer",
                                "default": 100,
                                "description": "Max skills to return. Default 100 for manifest (and for other formats). Pass 0 for unlimited (escape hatch)."
                            },
                            "offset": {
                                "type": "integer",
                                "default": 0,
                                "description": "Pagination offset into the skill list (default 0)."
                            }
                        }
                    }
                },
                {
                    "name": "task_alert",
                    "description": "Report a blocker that needs human intervention (auth/credentials/decisions). Records the alert, notifies via memory stream, and instructs you to pause (blocking=true) or continue. Use during long autonomous runs instead of spinning or fabricating workarounds.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "task_id": { "type": "string" },
                            "message": { "type": "string", "description": "What you are blocked on and what you need" },
                            "urgency": { "type": "string", "enum": ["info", "warning", "critical"] },
                            "blocking": { "type": "boolean", "description": "true = pause this work line until human handles it" }
                        },
                        "required": ["task_id", "message"]
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
                {
                    "name": "embedding_diagnostic",
                    "description": "Diagnose embedding dimension health. Detects stale embeddings (wrong dimension) that are excluded from vector search. Returns counts by dimension and lists affected memory IDs.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "embedding_migrate",
                    "description": "Re-embed ALL memories using the current embedding model (bge-m3, 1024-dim). Fixes stale embeddings that were created with an older model. Requires identity confirmation. This is a heavy operation.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "kg_quality",
                    "description": "Assess knowledge graph quality: relation density, orphan rate, average strength, and cluster connectivity. Returns metrics for evaluating KG health.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "sample_size": { "type": "integer", "description": "Number of memories to sample (default: 50, max: 200)" }
                        }
                    }
                },
                {
                    "name": "doc_import",
                    "description": "Import a markdown document into the memory space. Parses by ## headers, creates one memory per section with documentation labels. Sections auto-link to related memories via knowledge graph. Re-importing updates changed sections and invalidates old versions.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string", "description": "Document name (e.g. 'ARCHITECTURE', 'README')" },
                            "content": { "type": "string", "description": "Full markdown content of the document" }
                        },
                        "required": ["name", "content"]
                    }
                },
                {
                    "name": "memory_export",
                    "description": "Export memories as structured JSON. Filter by labels, project, or memory_class. Useful for backup, migration, or analysis.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "labels": { "type": "array", "items": { "type": "string" }, "description": "Filter by labels (OR match, optional)" },
                            "memory_class": { "type": "string", "enum": ["permanent", "session", "bridge"], "description": "Filter by memory class (optional)" },
                            "limit": { "type": "integer", "description": "Max memories to export (default 100, max 1000)" }
                        }
                    }
                },
                {
                    "name": "session_list",
                    "description": "List recent session summaries with timestamps. Shows what was accomplished and next steps from past sessions.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "limit": { "type": "integer", "description": "Number of sessions to return (default 10)" }
                        }
                    }
                },
                {
                    "name": "memory_restore",
                    "description": "Restore a superseded/expired memory by clearing its valid_to and boosting importance. Use when feedback correction was applied incorrectly or memory was auto-expired.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "integer", "description": "Memory ID to restore" }
                        },
                        "required": ["id"]
                    }
                },
                {
                    "name": "memory_forget",
                    "description": "Explicitly forget a memory by marking it superseded (valid_to) and dropping importance to FORGET_IMPORTANCE (0.01, explicit tombstone exception to the 0.3 live floor). Unlike auto-decay, this is a deliberate Agent/user decision. Enforced memories cannot be forgotten.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "integer", "description": "Memory ID to forget" }
                        },
                        "required": ["id"]
                    }
                },
                {
                    "name": "drive_inbox",
                    "description": "L0 Active Inference: Poll unacknowledged drive signals (pending and delivered). Returns an SMRP envelope whose data contains signals, stats, and empty_reason. Use each signal's retryable flag and status when deciding whether to act.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "limit": { "type": "integer", "description": "Max unacknowledged signals to retrieve (default 50, minimum 1)", "default": 50, "minimum": 1 }
                        }
                    }
                },
                {
                    "name": "drive_ack",
                    "description": "L0 Active Inference: Acknowledge a drive signal with execution feedback. After an agent receives and acts on a drive signal, it reports the outcome. This feedback flows into the personality's learn_history, closing the evolution loop: memory→will→action→feedback→evolution.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "drive_id": { "type": "integer", "description": "The drive signal ID to acknowledge" },
                            "executed": { "type": "boolean", "description": "Did the agent execute the drive?" },
                            "outcome": { "type": "string", "description": "What happened (self-reported outcome)" },
                            "reflection": { "type": "string", "description": "Optional: agent's reflection on the drive quality" }
                        },
                        "required": ["drive_id", "executed", "outcome"]
                    }
                },
                {
                    "name": "memory_propose_actions",
                    "description": "Memory Action Envelope (planner-ish): turn a query into claimable next actions grounded in personal memories. Searches evidence, matches skills, and enqueues open actions on the frontier. Each action cites evidence_memory_ids and suggests skills — claim with action_claim, finish with action_complete (writes an outcome memory linked to evidence). Complements memory_ask (answer) and drive_inbox (will signals).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "query": { "type": "string", "description": "What you want to act on (natural language)" },
                            "limit": { "type": "integer", "description": "Max actions to propose (default 3, max 5)", "default": 3, "minimum": 1, "maximum": 5 },
                            "depends_on": { "type": "array", "items": { "type": "integer" }, "description": "Existing action ids every proposed action must wait on (claimable only after all are done). Unknown ids / cycles rejected." },
                            "goal_id": { "type": "integer", "description": "Attach proposed actions to an open goal (action_goal_create)." },
                            "chain": { "type": "boolean", "description": "Serial plan: proposed step i depends on step i-1, so only the head is claimable until its predecessor lands done. Upstream escalation cascades blocked (dep_failed) downstream.", "default": false },
                            "actions": { "type": "array", "maxItems": 5, "description": "Explicit plan (preferred). Each item: {title|summary (one required), done_when (string|array), required_capabilities (string|array; defaults to top-level), priority 0-10, evidence_memory_ids, stop_if}. When given, auto-generation is skipped; query still grounds evidence.", "items": { "type": "object", "additionalProperties": false, "properties": {
                                "title": { "type": "string" }, "summary": { "type": "string" },
                                "done_when": { "type": ["array", "string"], "items": { "type": "string" } },
                                "required_capabilities": { "type": ["array", "string"], "items": { "type": "string" } },
                                "priority": { "type": "integer", "minimum": 0, "maximum": 10 },
                                "evidence_memory_ids": { "type": "array", "items": { "type": "integer" } },
                                "stop_if": { "type": ["array", "string"], "items": { "type": "string" } } } } },
                            "title": { "type": "string", "description": "Fallback mode only: summary of the primary auto-generated action" },
                            "required_capabilities": { "type": ["array", "string"], "items": { "type": "string" }, "description": "Capability tags for dispatch matching (worker_register capabilities). Overrides the auto-derived (skill-name) tags; default for explicit actions." }
                        },
                        "required": ["query"],
                        "additionalProperties": false
                    }
                },
                {
                    "name": "action_planner_status",
                    "description": "Server-side planner pass (deterministic, no LLM): outcome wake triggers are coalesced (debounce) into one pass; a heartbeat pass runs even when idle. Reports ready/claimed/waiting counts and stall signals: no_eligible_worker (ready work, no capable/available worker), stalled (nothing ready, nothing running), escalated (needs human). New signals are also pushed to drive_inbox once. Read-only: to force a pass use action_planner_run (write permission).",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "action_control",
                    "description": "Planner control of a running action. op=steer (queue a course-correction note; worker receives it once in action_heartbeat.directives.steer), pause (claimed action holds: lease does not expire, not counted as active load, complete refused), resume (fresh lease), stop (terminal 'stopped' — NOT a failure: no requeue/blacklist/escalation; worker sees directives.stop=true on heartbeat; late outcomes rejected and write no memory).",
                    "inputSchema": { "type": "object", "properties": {
                        "action_id": { "type": "integer" },
                        "op": { "type": "string", "enum": ["steer", "pause", "resume", "stop"] },
                        "note": { "type": "string", "description": "Steer text (required for steer) / stop reason" }
                    }, "required": ["action_id", "op"] }
                },
                {
                    "name": "action_goal_create",
                    "description": "Create a goal that groups actions (pass goal_id to memory_propose_actions). done_when = goal-level completion criteria.",
                    "inputSchema": { "type": "object", "properties": {
                        "title": { "type": "string" },
                        "done_when": { "type": ["array", "string"], "items": { "type": "string" }, "description": "Criteria list; a bare string is one criterion" }
                    }, "required": ["title"], "additionalProperties": false }
                },
                {
                    "name": "action_goal_prove",
                    "description": "Mark a goal achieved with evidence parallel to its done_when. Remaining open/claimed actions in the goal become 'cancelled' (terminal, not a failure: no requeue/blacklist/escalation; late outcomes are rejected and write no memory). A goal whose actions all land done auto-closes without proving.",
                    "inputSchema": { "type": "object", "properties": {
                        "goal_id": { "type": "integer" },
                        "evidence": { "type": "array", "items": { "type": "string" } }
                    }, "required": ["goal_id", "evidence"] }
                },
                {
                    "name": "action_goal_list",
                    "description": "List goals with state (open|achieved|closed) and per-state action counts.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "action_planner_run",
                    "description": "Force one planner pass now (write: may push new stall/escalation signals to drive_inbox, deduped). Returns the pass report.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "action_list",
                    "description": "List actions on the memory-action frontier. Filter by state: open|claimed|done|blocked|exhausted.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "state": { "type": "string", "enum": ["open", "claimed", "done", "blocked", "exhausted"], "description": "Optional state filter" },
                            "limit": { "type": "integer", "description": "Max actions (default 20)", "default": 20, "minimum": 1, "maximum": 100 }
                        }
                    }
                },
                {
                    "name": "action_claim",
                    "description": "Claim one open action for exclusive execution (compare-and-set). Second claim fails. Worker-ish step of the Memory Action Envelope.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "action_id": { "type": "integer", "description": "Action id from memory_propose_actions / action_list" },
                            "claimed_by": { "type": "string", "description": "Optional agent label (defaults to user id)" },
                            "worker_id": { "type": "string", "description": "Alias of claimed_by (registered worker id). If both are given they must match." }
                        },
                        "required": ["action_id"],
                        "additionalProperties": false
                    }
                },
                {
                    "name": "action_heartbeat",
                    "description": "Renew the claim lease of an action you own (claimed). Claims carry a lease (default 900s); if neither heartbeat nor complete arrives before lease_until, the scheduler sweep requeues the action (reason lease_expired) or escalates after max requeues.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "action_id": { "type": "integer" },
                            "claimed_by": { "type": "string", "description": "Owner label used at claim/assign (defaults to user id)" },
                            "worker_id": { "type": "string", "description": "Alias of claimed_by. If both are given they must match." }
                        },
                        "required": ["action_id"],
                        "additionalProperties": false
                    }
                },
                {
                    "name": "action_complete",
                    "description": "Finish a claimed/open action: creates an outcome memory, links each evidence memory to it (Precedes), and marks the action done|blocked|exhausted. Requires non-empty outcome text. When action.done_when is non-empty, pass done_when_evidence (parallel array); unmapped items soft-fail to blocked (not done) while protocol.ok stays true — always check the returned action.state. Each evidence item must cite a memory (#id) or be specific (>=8 chars, CJK counts 2) and not merely echo the criterion.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "action_id": { "type": "integer" },
                            "outcome": { "type": "string", "description": "What was delivered / learned (becomes a memory)" },
                            "done_when_evidence": { "type": "array", "items": { "type": "string" }, "description": "Parallel to action.done_when — artifact-level evidence per item. Each item must cite a memory id (#123) or be a substantive description (>= 8 weight; CJK chars count 2) and must not just repeat the criterion; 'x'/'ok'/'done' do not count. Non-array / non-string items → 400 (no state change, no budget)." },
                            "worker_id": { "type": "string", "description": "Alias of claimed_by" },
                            "claimed_by": { "type": "string", "description": "Worker label used at action_claim / action_assign (defaults to user id). Must match the assignee unless admin." },
                            "failure_kind": { "type": "string", "enum": ["transient", "exhausted", "blocked"], "description": "Classify a non-done outcome. transient = external blip (provider/network/rate-limit): retried with exponential backoff, does NOT consume requeue budget or blame the worker (max 5, then counts as blocked). exhausted = ran out of steps/time: requeue budget consumed, worker not blacklisted. blocked = real error: budget consumed + short worker blacklist." },
                            "success": { "type": "boolean", "description": "true → state=done (default); false → blocked", "default": true },
                            "labels": { "type": "array", "items": { "type": "string" }, "description": "Extra labels for the outcome memory" },
                            "state": { "type": "string", "enum": ["done", "blocked", "exhausted"], "description": "Override terminal state" }
                        },
                        "required": ["action_id", "outcome"],
                        "additionalProperties": false
                    }
                },
                {
                    "name": "drive_to_actions",
                    "description": "Drive→Action bridge: materialize one grounded drive signal into a claimable AgentAction. Requires persistent evidence memory ids (signal.evidence or grounding.evidence). Rejects double-spend while a non-terminal action already cites the same drive_id. Completing the action auto-acks the drive (first_ack only).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "drive_id": { "type": "integer", "description": "Drive signal id from drive_inbox" },
                            "claim": { "type": "boolean", "description": "If true, immediately CAS-claim the new action for this user (default false)", "default": false }
                        },
                        "required": ["drive_id"]
                    }
                },
                {
                    "name": "worker_register",
                    "description": "Dispatch: register or update a worker identity with capability tags and max_load. Used by action_assign / action_dispatch for least-loaded capable assignment.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "worker_id": { "type": "string", "description": "Stable worker / agent / sub-account id" },
                            "capabilities": { "type": "array", "items": { "type": "string" }, "description": "Capability tags (e.g. ops, rust, memory)" },
                            "max_load": { "type": "integer", "description": "Max concurrent claimed actions (default 3)", "default": 3, "minimum": 1 }
                        },
                        "required": ["worker_id"]
                    }
                },
                {
                    "name": "worker_unregister",
                    "description": "Dispatch: remove a registered worker (e.g. smoke-test workers). Write permission; only the account owner or Admin. Refused (409) while the worker holds live claims unless force=true (claims are then left to lease expiry / sweep).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "worker_id": { "type": "string" },
                            "force": { "type": "boolean", "default": false }
                        },
                        "required": ["worker_id"],
                        "additionalProperties": false
                    }
                },
                {
                    "name": "worker_list",
                    "description": "Dispatch: list registered workers with current claimed load and blacklist status.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "action_assign",
                    "description": "Dispatch: assign one open action to a worker. Omit worker_id to auto-pick least-loaded capable worker. Sets immutable assignee and claims the action for it (state Claimed, lease starts) — the assignee must not call action_claim again; heartbeat/complete directly. Only that assignee (or Admin) may action_complete.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "action_id": { "type": "integer" },
                            "worker_id": { "type": "string", "description": "Optional explicit assignee; omit for auto-pick" }
                        },
                        "required": ["action_id"]
                    }
                },
                {
                    "name": "action_dispatch",
                    "description": "Dispatch: batch auto-assign up to limit open actions (priority order) to least-loaded capable workers. Assigned actions are already Claimed for their worker (no action_claim needed).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "limit": { "type": "integer", "default": 5, "minimum": 1, "maximum": 20 }
                        }
                    }
                },
                {
                    "name": "skill_auto_extract",
                    "description": "L3 Skill Learning: automatically extract a reusable skill document from the cognitive engine's effective decision patterns. Analyzes recent decision history for actions with >60% success rate and uses LLM to generalize them into a skill. Letta-inspired continual learning.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                },
                {
                    "name": "memory_improve",
                    "description": "Actively improve low-quality memories by rewriting them clearer using LLM. Finds memories with short content (<40 chars) or no labels and rewrites them to be more complete. Cognee-inspired improve operation.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "limit": { "type": "integer", "description": "Max memories to improve (default 10)", "default": 10 }
                        }
                    }
                },
                {
                    "name": "doc_list",
                    "description": "List all imported documents and their sections in the memory space.",
                    "inputSchema": { "type": "object" }
                },
            ]
        });
        // MemCard working-memory cards: always registered.
        {
            if let Some(tools) = result["tools"].as_array_mut() {
                tools.push(serde_json::json!({
                    "name": "blocks_get",
                    "description": "MemCard: get a persona/user/project/working card (≤2KB).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "kind": { "type": "string", "enum": ["persona", "user", "project", "working"] },
                            "project": { "type": "string", "description": "Optional project scope for project cards" }
                        },
                        "required": ["kind"]
                    }
                }));
                tools.push(serde_json::json!({
                    "name": "blocks_set",
                    "description": "MemCard: set a persona/user/project/working card (≤2KB, versioned).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "kind": { "type": "string", "enum": ["persona", "user", "project", "working"] },
                            "content": { "type": "string", "description": "Card content (max 2048 bytes)" },
                            "project": { "type": "string", "description": "Optional project scope for project cards" }
                        },
                        "required": ["kind", "content"]
                    }
                }));
            }
        }
        let output_schema = Self::smrp_output_schema();
        if let Some(tools) = result["tools"].as_array_mut() {
            for tool in tools {
                tool["outputSchema"] = output_schema.clone();
                if let Some(name) = tool.get("name").and_then(|v| v.as_str()) {
                    tool["annotations"] = Self::tool_annotations(name);
                }
            }
        }
        result
    }
}
