//! Feature flags for Memory-OS ABI spikes. All default OFF → legacy behavior.

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
}
