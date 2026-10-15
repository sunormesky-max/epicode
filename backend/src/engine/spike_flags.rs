//! Feature flags for Memory-OS ABI spikes.
//! Most default OFF → legacy behavior. Exception: `recall_outcome_log` defaults ON
//! because it is append-only and never enters retrieval scoring (design-draft §b).

/// True when env is set to a truthy value (`1`/`true`/`yes`/`on`, case-insensitive).
pub fn env_flag(name: &str) -> bool {
    match std::env::var(name) {
        Ok(v) => {
            let v = v.trim();
            v == "1"
                || v.eq_ignore_ascii_case("true")
                || v.eq_ignore_ascii_case("yes")
                || v.eq_ignore_ascii_case("on")
        }
        Err(_) => false,
    }
}

pub fn slim_envelope_env() -> bool {
    env_flag("EPICODE_SLIM_ENVELOPE")
}

/// Per-call slim: env flag OR MCP arg `verbosity=slim`.
pub fn slim_envelope_requested(args: &serde_json::Value) -> bool {
    if args
        .get("verbosity")
        .and_then(|v| v.as_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("slim"))
    {
        return true;
    }
    slim_envelope_env()
}

/// Append-only recall/outcome logging (design-draft §b).
/// Default **ON** — logging does not change retrieval scoring.
/// Set `EPICODE_RECALL_OUTCOME_LOG=0|false|off|no` to disable.
pub fn recall_outcome_log() -> bool {
    match std::env::var("EPICODE_RECALL_OUTCOME_LOG") {
        Ok(v) => {
            let v = v.trim();
            !(v == "0"
                || v.eq_ignore_ascii_case("false")
                || v.eq_ignore_ascii_case("no")
                || v.eq_ignore_ascii_case("off"))
        }
        Err(_) => true,
    }
}

/// Retention window for recall_log / outcome_event rows (days).
/// Prune is opt-in via `StorageManager::prune_recall_outcome_logs`; not auto-run.
pub const RECALL_OUTCOME_RETENTION_DAYS: i64 = 90;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbosity_slim_without_env() {
        let args = serde_json::json!({"verbosity": "slim"});
        assert!(slim_envelope_requested(&args));
        let args2 = serde_json::json!({"verbosity": "full"});
        // Without env, full stays off.
        assert!(!slim_envelope_requested(&args2));
    }

    #[test]
    fn recall_outcome_log_defaults_on() {
        std::env::remove_var("EPICODE_RECALL_OUTCOME_LOG");
        assert!(recall_outcome_log());
        std::env::set_var("EPICODE_RECALL_OUTCOME_LOG", "0");
        assert!(!recall_outcome_log());
        std::env::set_var("EPICODE_RECALL_OUTCOME_LOG", "true");
        assert!(recall_outcome_log());
        std::env::remove_var("EPICODE_RECALL_OUTCOME_LOG");
    }
}
