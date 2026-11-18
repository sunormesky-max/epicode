use super::*;

impl McpHandler {
    /// 设置配额上下文(cloud.rs 的 mcp_endpoint / TCP 认证后调用)。
    /// Every callable tool needs an explicit permission. An unknown tool stays
    /// unmapped and is rejected for subaccounts before it reaches the dispatcher.
    pub(in crate::engine::mcp) fn tool_permission(
        name: &str,
    ) -> Option<crate::engine::user_manager::Permission> {
        use crate::engine::user_manager::Permission as P;
        match name {
            // Read access includes contextual, diagnostic, task and drive views.
            "skill_get"
            | "task_status"
            | "memory_search"
            | "library_search"
            | "memory_recall"
            | "memory_wander"
            | "memory_ask"
            | "memory_get"
            | "memory_list"
            | "ctx_load"
            | "pattern_recall"
            | "space_stats"
            | "knowledge_relations"
            | "concepts"
            | "skill_execute"
            | "enforced_rules"
            | "project_list"
            | "embedding_diagnostic"
            | "kg_quality"
            | "doc_list"
            | "memory_export"
            | "session_list"
            | "drive_inbox"
            | "action_list"
            | "action_planner_status"
            | "action_goal_list"
            | "worker_list"
            | "blocks_get" => Some(P::MemoryRead),
            // These tools change memories or persistent session/drive state.
            "epicode_handshake"
            | "task_start"
            | "task_check"
            | "task_complete"
            | "task_alert"
            | "memory_create"
            | "memory_update"
            | "ctx_save"
            | "pattern_learn"
            | "decision_record"
            | "bug_memory"
            | "session_summary"
            | "context_observe"
            | "feedback_submit"
            | "doc_import"
            | "memory_improve"
            | "drive_ack"
            | "blocks_set"
            | "memory_propose_actions"
            | "action_claim"
            | "action_heartbeat"
            | "action_complete"
            | "drive_to_actions"
            | "worker_register"
            | "worker_unregister"
            | "action_assign"
            | "action_dispatch"
            | "action_planner_run"
            | "action_goal_create"
            | "action_goal_prove"
            | "action_control" => Some(P::MemoryWrite),
            // Deletion, quarantine and restoration change protected memory state.
            "memory_delete" | "memory_forget" | "memory_restore" => Some(P::MemoryDelete),
            // Skill and library management.
            "skill_auto_extract" | "skills_sync" | "skill_feedback" => Some(P::SkillManage),
            "dream_cycle" => Some(P::MemoryDelete),
            // Identity setup is immutable; migration rewrites stored embeddings.
            "identity_confirm" | "identity_step" | "identity_finalize" | "embedding_migrate" => {
                Some(P::PersonaImport)
            }
            _ => None,
        }
    }

    /// MCP ToolAnnotations (spec): hints so clients can auto-approve safe reads
    /// and warn on destructive calls. Pure metadata — no runtime behavior change.
    pub(in crate::engine::mcp) fn tool_annotations(name: &str) -> serde_json::Value {
        let (read_only, destructive, idempotent, open_world) =
            Self::tool_annotation_class(name).unwrap_or((false, false, false, false));
        serde_json::json!({
            "readOnlyHint": read_only,
            "destructiveHint": destructive,
            "idempotentHint": idempotent,
            "openWorldHint": open_world,
        })
    }

    /// Explicit (readOnly, destructive, idempotent, openWorld) class per tool.
    /// `None` = unclassified; the catalog consistency test rejects that.
    pub(in crate::engine::mcp) fn tool_annotation_class(
        name: &str,
    ) -> Option<(bool, bool, bool, bool)> {
        let class = match name {
            // Pure reads
            "memory_search"
            | "memory_recall"
            | "memory_wander"
            | "memory_ask"
            | "memory_get"
            | "memory_list"
            | "memory_export"
            | "library_search"
            | "space_stats"
            | "concepts"
            | "knowledge_relations"
            | "pattern_recall"
            | "enforced_rules"
            | "project_list"
            | "embedding_diagnostic"
            | "kg_quality"
            | "doc_list"
            | "session_list"
            | "skills_sync"
            | "skill_get"
            | "skill_execute"
            | "task_status"
            | "drive_inbox"
            | "action_list"
            | "action_planner_status"
            | "action_goal_list"
            | "worker_list"
            | "blocks_get"
            | "ctx_load" => (true, false, true, false),
            // Destructive / hard-to-reverse
            "memory_delete" | "memory_forget" | "dream_cycle" | "embedding_migrate"
            | "identity_finalize" => (false, true, false, false),
            // Re-runnable writes (idempotent-ish)
            "memory_update"
            | "identity_confirm"
            | "drive_ack"
            | "memory_propose_actions"
            | "action_claim"
            | "action_heartbeat"
            | "action_complete"
            | "drive_to_actions"
            | "worker_register"
            | "worker_unregister"
            | "action_assign"
            | "action_dispatch"
            | "action_planner_run"
            | "action_goal_create"
            | "action_goal_prove"
            | "action_control"
            | "skill_feedback"
            | "feedback_submit"
            | "blocks_set"
            | "task_check" => (false, false, true, false),
            // Other writes / side-effects
            "memory_create" | "memory_restore" | "memory_improve" | "ctx_save"
            | "pattern_learn" | "decision_record" | "bug_memory" | "session_summary"
            | "context_observe" | "identity_step" | "task_start" | "task_complete"
            | "task_alert" | "skill_auto_extract" | "doc_import" | "epicode_handshake" => {
                (false, false, false, false)
            }
            _ => return None,
        };
        Some(class)
    }
}
