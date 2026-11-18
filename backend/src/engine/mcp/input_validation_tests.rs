use super::tests::{confirm_test_identity, isolated_engine};
use super::*;
use std::sync::Arc;

fn setup() -> McpHandler {
    let h = McpHandler::new(Arc::new(isolated_engine()));
    confirm_test_identity(&h);
    let m = h.tool_memory_create(&serde_json::json!({
        "content": "Rust release pipeline needs changelog and smoke tests",
        "labels": ["release"]
    }));
    assert_eq!(m["protocol"]["ok"], true, "{m}");
    h
}

fn propose_one(h: &McpHandler) -> (u64, Vec<serde_json::Value>) {
    let p = h.tool_memory_propose_actions(&serde_json::json!({
            "query": "release pipeline", "limit": 1,
            "actions": [{"title": "Ship release", "done_when": ["changelog updated"], "required_capabilities": ["rust"]}]
        }));
    assert_eq!(p["protocol"]["ok"], true, "{p}");
    let a = p["data"]["actions"][0].clone();
    (
        a["id"].as_u64().unwrap(),
        a["done_when"].as_array().cloned().unwrap_or_default(),
    )
}

#[test]
fn propose_actions_honours_explicit_plan_and_dispatch_matches() {
    let h = setup();
    let p = h.tool_memory_propose_actions(&serde_json::json!({
            "query": "release pipeline",
            "actions": [
                {"title": "Build", "summary": "cargo build --release", "done_when": "binary built", "required_capabilities": ["rust"]},
                {"title": "Smoke", "done_when": ["smoke green"], "required_capabilities": "ops"}
            ]
        }));
    assert_eq!(p["protocol"]["ok"], true, "{p}");
    let acts = p["data"]["actions"].as_array().unwrap();
    assert_eq!(acts.len(), 2);
    assert_eq!(acts[0]["summary"], "Build: cargo build --release");
    assert_eq!(acts[0]["done_when"], serde_json::json!(["binary built"]));
    assert_eq!(
        acts[0]["required_capabilities"],
        serde_json::json!(["rust"])
    );
    assert_eq!(acts[1]["required_capabilities"], serde_json::json!(["ops"]));
    for (w, c) in [("w-rust", "rust"), ("w-ops", "ops")] {
        let r = h.tool_worker_register(&serde_json::json!({"worker_id": w, "capabilities": [c]}));
        assert_eq!(r["protocol"]["ok"], true, "{r}");
    }
    let d = h.tool_action_dispatch(&serde_json::json!({"limit": 5}));
    assert_eq!(d["data"]["assigned"], 2, "{d}");
    // Top-level caps override auto-derived skill-name caps in fallback mode.
    let f = h.tool_memory_propose_actions(&serde_json::json!({
            "query": "release pipeline", "limit": 1, "title": "Custom title", "required_capabilities": ["rust"]
        }));
    assert_eq!(f["data"]["actions"][0]["summary"], "Custom title", "{f}");
    assert_eq!(
        f["data"]["actions"][0]["required_capabilities"],
        serde_json::json!(["rust"])
    );
    let bad = h.tool_memory_propose_actions(
        &serde_json::json!({"query": "release pipeline", "actions": [{"bogus": 1}]}),
    );
    assert_eq!(bad["protocol"]["error"]["code"], 400, "{bad}");
}

#[test]
fn complete_schema_errors_are_400_and_keep_budget() {
    let h = setup();
    let (id, _) = propose_one(&h);
    let c = h.tool_action_claim(&serde_json::json!({"action_id": id, "claimed_by": "w1"}));
    assert_eq!(c["protocol"]["ok"], true, "{c}");
    for bad in [
        serde_json::json!({"action_id": id, "outcome": "done it", "claimed_by": "w1", "evidence": ["#1"]}),
        serde_json::json!({"action_id": id, "outcome": "done it", "claimed_by": "w1", "done_when_evidence": "changelog updated in repo"}),
        serde_json::json!({"action_id": id, "outcome": "done it", "claimed_by": "w1", "done_when_evidence": [1]}),
        serde_json::json!({"action_id": id, "outcome": "done it", "claimed_by": "w1", "success": "yes"}),
    ] {
        let r = h.tool_action_complete(&bad);
        assert_eq!(r["protocol"]["error"]["code"], 400, "{r}");
    }
    let a = h.engine.scheduler.action_frontier().get(id).unwrap();
    assert_eq!(a.state.as_str(), "claimed");
    assert_eq!(a.requeue_count, 0);
}

#[test]
fn trivial_evidence_does_not_prove_done() {
    let h = setup();
    let (id, _) = propose_one(&h);
    h.tool_action_claim(&serde_json::json!({"action_id": id, "claimed_by": "w1"}));
    let r = h.tool_action_complete(&serde_json::json!({
        "action_id": id, "outcome": "shipped", "claimed_by": "w1", "done_when_evidence": ["x"]
    }));
    let a = h.engine.scheduler.action_frontier().get(id).unwrap();
    assert_ne!(a.state.as_str(), "done", "{r}");
}

#[test]
fn worker_id_is_alias_and_unknown_params_rejected() {
    let h = setup();
    let (id, _) = propose_one(&h);
    let c = h.tool_action_claim(&serde_json::json!({"action_id": id, "worker_id": "w-alias"}));
    assert_eq!(c["protocol"]["ok"], true, "{c}");
    assert_eq!(
        h.engine
            .scheduler
            .action_frontier()
            .get(id)
            .unwrap()
            .claimed_by
            .as_deref(),
        Some("w-alias")
    );
    let hb = h.tool_action_heartbeat(&serde_json::json!({"action_id": id, "worker_id": "w-alias"}));
    assert_eq!(hb["protocol"]["ok"], true, "{hb}");
    let mismatch = h.tool_action_heartbeat(
        &serde_json::json!({"action_id": id, "worker_id": "a", "claimed_by": "b"}),
    );
    assert_eq!(mismatch["protocol"]["error"]["code"], 400);
    let unk = h.tool_action_claim(&serde_json::json!({"action_id": id, "worker": "w"}));
    assert_eq!(unk["protocol"]["error"]["code"], 400, "{unk}");
}

#[test]
fn goal_done_when_string_is_single_item() {
    let h = setup();
    let g = h.tool_action_goal_create(
        &serde_json::json!({"title": "ship", "done_when": "release tagged"}),
    );
    assert_eq!(
        g["data"]["goal"]["done_when"],
        serde_json::json!(["release tagged"]),
        "{g}"
    );
    let bad = h.tool_action_goal_create(&serde_json::json!({"title": "ship", "done_when": 3}));
    assert_eq!(bad["protocol"]["error"]["code"], 400);
}

#[test]
fn worker_unregister_owner_admin_only() {
    let h = setup();
    h.tool_worker_register(&serde_json::json!({"worker_id": "smoke-w", "capabilities": ["ops"]}));
    let r = h.tool_worker_unregister(&serde_json::json!({"worker_id": "smoke-w"}));
    assert_eq!(r["protocol"]["ok"], true, "{r}");
    assert!(h
        .engine
        .scheduler
        .worker_registry()
        .get("smoke-w")
        .is_none());
    let again = h.tool_worker_unregister(&serde_json::json!({"worker_id": "smoke-w"}));
    assert_eq!(again["protocol"]["error"]["code"], 404);
    let dev = McpHandler::new(h.engine.clone())
        .with_role_gate(crate::engine::user_manager::UserRole::Developer);
    h.tool_worker_register(&serde_json::json!({"worker_id": "w2"}));
    let denied = dev.tool_worker_unregister(&serde_json::json!({"worker_id": "w2"}));
    assert_eq!(denied["protocol"]["error"]["code"], 403);
    // Live claim blocks unless force.
    let (id, _) = propose_one(&h);
    h.tool_action_claim(&serde_json::json!({"action_id": id, "claimed_by": "w2"}));
    let busy = h.tool_worker_unregister(&serde_json::json!({"worker_id": "w2"}));
    assert_eq!(busy["protocol"]["error"]["code"], 409, "{busy}");
    let forced = h.tool_worker_unregister(&serde_json::json!({"worker_id": "w2", "force": true}));
    assert_eq!(forced["protocol"]["ok"], true);
}
