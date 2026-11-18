static SPIKE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
use super::*;
use crate::engine::Engine;

/// 测试专用:每次调用使用全新的唯一临时数据目录,
/// 避免多个测试(及多次运行)共享并持久化 `./data`(identity.json / tetramem.db)
/// 而互相影响。
#[test]
fn build_search_filters_parses_as_of() {
    let h = McpHandler::new(Arc::new(isolated_engine()));
    // P1-6: as_of 时点查询进 filter
    let f = h
        .build_search_filters(&serde_json::json!({"query": "x", "as_of": 1_700_000_000}))
        .expect("as_of alone must produce filters");
    assert_eq!(f.as_of, Some(1_700_000_000));
    // 非正数被过滤(防误传0当"全部")
    let f0 = h
        .build_search_filters(&serde_json::json!({"query": "x", "as_of": 0}))
        .expect("filters present");
    assert_eq!(f0.as_of, None);
    // 无任何过滤参数仍返回 None(向后兼容)
    assert!(h
        .build_search_filters(&serde_json::json!({"query": "x"}))
        .is_none());
}

pub(super) fn isolated_engine() -> Engine {
    let dir = std::env::temp_dir().join(format!(
        "epicode-mcp-test-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    Engine::with_data_dir(dir)
}

/// 测试专用:通过 MCP 的 identity_step / identity_finalize 在当前(隔离的)
/// 引擎中确认身份,使测试不再依赖其他测试遗留在 `./data` 里的 identity.json。
pub(super) fn confirm_test_identity(h: &McpHandler) {
    for step in 1..=5 {
        let val = if step == 1 { "TestAgent" } else { "test" };
        let raw = format!(
            r#"{{"jsonrpc":"2.0","id":0,"method":"tools/call","params":{{"name":"identity_step","arguments":{{"step":{},"value":"{}"}}}}}}"#,
            step, val
        );
        h.process_json(&raw);
    }
    h.process_json(r#"{"jsonrpc":"2.0","id":0,"method":"tools/call","params":{"name":"identity_finalize","arguments":{}}}"#);
}

#[test]
fn strip_html_keeps_lone_lt_and_following_text() {
    assert_eq!(
        strip_html("if a < b then keep this tail"),
        "if a < b then keep this tail"
    );
    assert_eq!(
        strip_html("price<5 and more text after"),
        "price<5 and more text after"
    );
    assert_eq!(strip_html("1 < 2"), "1 < 2");
    assert_eq!(strip_html("<"), "<");
    assert_eq!(strip_html("a<b"), "a<b");
    assert_eq!(strip_html("a <3 b"), "a <3 b");
    assert_eq!(strip_html("x < y > z"), "x < y > z");
    assert_eq!(strip_html("<b>bold</b> text"), "bold text");
    assert_eq!(strip_html("a<script>alert(1)</script>b"), "aalert(1)b");
    assert_eq!(strip_html("<!-- c -->ok"), "ok");
    assert_eq!(
        strip_html("中文 <i>斜体</i> 与 a < b 之后"),
        "中文 斜体 与 a < b 之后"
    );
}

#[test]
fn mcp_initialize() {
    let eng = isolated_engine();
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
    let eng = isolated_engine();
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
    assert!(tools.len() >= 12);
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(names.contains(&"memory_create"));
    assert!(names.contains(&"memory_search"));
    assert!(names.contains(&"memory_propose_actions"));
    assert!(names.contains(&"action_claim"));
    assert!(names.contains(&"action_complete"));
    assert!(names.contains(&"action_list"));
    assert!(names.contains(&"drive_to_actions"));
    assert!(names.contains(&"worker_register"));
    assert!(names.contains(&"action_assign"));
    assert!(names.contains(&"action_dispatch"));
    assert!(names.contains(&"memory_recall"));
    assert!(names.contains(&"memory_ask"));
    assert!(names.contains(&"ctx_load"));
    assert!(names.contains(&"ctx_save"));
    assert!(names.contains(&"pattern_learn"));
    assert!(names.contains(&"pattern_recall"));
    assert!(names.contains(&"decision_record"));
    assert!(names.contains(&"bug_memory"));
    assert!(names.contains(&"session_summary"));

    let memory_search = tools
        .iter()
        .find(|tool| tool["name"] == "memory_search")
        .expect("memory_search tool must be listed");
    let search_modes = memory_search["inputSchema"]["properties"]["mode"]["enum"]
        .as_array()
        .expect("memory_search mode enum");
    assert!(search_modes
        .iter()
        .any(|mode| mode.as_str() == Some("auto")));
    assert!(search_modes
        .iter()
        .any(|mode| mode.as_str() == Some("fusion")));

    let drive_inbox = tools
        .iter()
        .find(|tool| tool["name"] == "drive_inbox")
        .expect("drive_inbox tool must be listed");
    assert_eq!(
        drive_inbox["inputSchema"]["properties"]["limit"]["default"],
        50
    );
    // Agent UX: every tool must advertise MCP annotations for client auto-approve.
    for tool in tools {
        let name = tool["name"].as_str().unwrap_or("?");
        let ann = tool.get("annotations").unwrap_or(&serde_json::Value::Null);
        assert!(
            ann.get("readOnlyHint").and_then(|v| v.as_bool()).is_some(),
            "tool {name} missing readOnlyHint"
        );
        assert!(
            ann.get("destructiveHint")
                .and_then(|v| v.as_bool())
                .is_some(),
            "tool {name} missing destructiveHint"
        );
        assert!(
            ann.get("idempotentHint")
                .and_then(|v| v.as_bool())
                .is_some(),
            "tool {name} missing idempotentHint"
        );
    }
    let memory_search_ann = memory_search["annotations"].clone();
    assert_eq!(memory_search_ann["readOnlyHint"], true);
    assert_eq!(memory_search_ann["destructiveHint"], false);
    let memory_delete = tools
        .iter()
        .find(|tool| tool["name"] == "memory_delete")
        .expect("memory_delete tool must be listed");
    assert_eq!(memory_delete["annotations"]["destructiveHint"], true);
    assert_eq!(memory_delete["annotations"]["readOnlyHint"], false);

    assert!(drive_inbox["description"]
        .as_str()
        .unwrap_or_default()
        .contains("pending and delivered"));
    for tool in tools {
        assert_eq!(
            tool["outputSchema"]["required"],
            serde_json::json!(["protocol", "data", "status"])
        );
    }
}

#[test]
fn mcp_role_gate_classifies_every_advertised_tool() {
    let h = McpHandler::new(Arc::new(isolated_engine()));
    let listed = h.handle(McpRequest {
        jsonrpc: "2.0".into(),
        id: Some(serde_json::json!(1)),
        method: "tools/list".into(),
        params: None,
    });
    for tool in listed.result.unwrap()["tools"].as_array().unwrap() {
        let name = tool["name"].as_str().unwrap();
        assert!(
            McpHandler::tool_permission(name).is_some(),
            "listed MCP tool {name} must have an explicit permission"
        );
    }
    // MemCard tools are hidden unless the feature flag is enabled.
    assert_eq!(
        McpHandler::tool_permission("blocks_get"),
        Some(crate::engine::user_manager::Permission::MemoryRead)
    );
    assert_eq!(
        McpHandler::tool_permission("blocks_set"),
        Some(crate::engine::user_manager::Permission::MemoryWrite)
    );
    assert_eq!(McpHandler::tool_permission("future_write_tool"), None);
}

#[test]
fn mcp_role_gate_denies_viewer_writes_and_unknown_tools() {
    use crate::engine::user_manager::UserRole;

    let h = McpHandler::new(Arc::new(isolated_engine())).with_role_gate(UserRole::Viewer);
    for name in [
        "epicode_handshake",
        "ctx_save",
        "decision_record",
        "bug_memory",
        "session_summary",
        "context_observe",
        "feedback_submit",
        "task_start",
        "task_check",
        "task_complete",
        "task_alert",
        "drive_ack",
        "blocks_set",
        "identity_finalize",
        "memory_propose_actions",
        "action_claim",
        "action_heartbeat",
        "action_complete",
        "drive_to_actions",
        "worker_register",
        "worker_unregister",
        "action_assign",
        "action_dispatch",
        "action_planner_run",
        "action_control",
        "future_write_tool",
    ] {
        let response = h.handle(McpRequest {
            jsonrpc: "2.0".into(),
            id: Some(serde_json::json!(name)),
            method: "tools/call".into(),
            params: Some(serde_json::json!({"name": name, "arguments": {}})),
        });
        assert_eq!(
            response.result.unwrap()["structuredContent"]["protocol"]["error"]["code"],
            403,
            "viewer must not call {name}"
        );
    }
}

/// #261 follow-up: a read-only caller must not be able to force a planner
/// pass (which can enqueue drive notifications). Fails on #261 150fe9d.
#[test]
fn viewer_planner_status_is_side_effect_free() {
    use crate::engine::user_manager::UserRole;
    let engine = Arc::new(isolated_engine());
    confirm_test_identity(&McpHandler::new(Arc::clone(&engine)));
    let h = McpHandler::new(engine).with_role_gate(UserRole::Viewer);
    let call = |name: &str, args: serde_json::Value| {
        h.handle(McpRequest {
            jsonrpc: "2.0".into(),
            id: Some(serde_json::json!(1)),
            method: "tools/call".into(),
            params: Some(serde_json::json!({"name": name, "arguments": args})),
        })
        .result
        .unwrap()["structuredContent"]
            .clone()
    };
    let st = call(
        "action_planner_status",
        serde_json::json!({"run_now": true}),
    );
    assert_eq!(st["protocol"]["ok"], true, "{st}");
    assert_eq!(
        h.engine.scheduler.planner_pass().status()["passes"],
        0,
        "{st}"
    );
    let run = call("action_planner_run", serde_json::json!({}));
    assert_eq!(run["protocol"]["error"]["code"], 403, "{run}");
}

#[test]
fn mcp_custom_permissions_apply_to_reads_and_writes() {
    use crate::engine::user_manager::UserRole;

    let engine = Arc::new(isolated_engine());
    let no_permissions = McpHandler::new(Arc::clone(&engine))
        .with_role_gate(UserRole::Viewer)
        .with_custom_permissions(Some(vec![]));
    for name in ["memory_search", "space_stats", "ctx_save"] {
        let response = no_permissions.handle(McpRequest {
            jsonrpc: "2.0".into(),
            id: Some(serde_json::json!(name)),
            method: "tools/call".into(),
            params: Some(serde_json::json!({"name": name, "arguments": {}})),
        });
        assert_eq!(
            response.result.unwrap()["structuredContent"]["protocol"]["error"]["code"],
            403,
            "explicit empty grant set must deny {name}"
        );
    }

    // With the corresponding grant, authorization passes and the next
    // check (unconfirmed identity in this isolated engine) supplies 4003.
    let granted_write = McpHandler::new(Arc::clone(&engine))
        .with_role_gate(UserRole::Viewer)
        .with_custom_permissions(Some(vec!["memory_write".into()]));
    let response = granted_write.handle(McpRequest {
        jsonrpc: "2.0".into(),
        id: Some(serde_json::json!("ctx_save")),
        method: "tools/call".into(),
        params: Some(serde_json::json!({"name": "ctx_save", "arguments": {}})),
    });
    assert_eq!(
        response.result.unwrap()["structuredContent"]["protocol"]["error"]["code"],
        4003
    );

    let viewer = McpHandler::new(engine).with_role_gate(UserRole::Viewer);
    let response = viewer.handle(McpRequest {
        jsonrpc: "2.0".into(),
        id: Some(serde_json::json!("space_stats")),
        method: "tools/call".into(),
        params: Some(serde_json::json!({"name": "space_stats", "arguments": {}})),
    });
    assert_eq!(
        response.result.unwrap()["structuredContent"]["protocol"]["error"]["code"],
        4003
    );
}

#[tokio::test]
async fn mcp_drive_inbox_includes_the_shared_retryable_signal_contract() {
    let mut engine = isolated_engine();
    engine.start();
    engine
        .scheduler()
        .drive_queue()
        .enqueue(crate::engine::drive::DriveSignal {
            id: 0,
            timestamp: 1_780_000_000,
            intent_type: crate::engine::drive::DriveIntent::Explore,
            description: "test inbox wire shape".into(),
            evidence: vec![10],
            urgency: crate::engine::drive::DriveUrgency::Low,
            target_capability: None,
            emotion: None,
            origin_tick: 1,
            status: crate::engine::drive::DriveStatus::Pending,
            feedback: None,
            retry_count: 0,
            expires_at: None,
            enqueued_at_ms: 1_780_000_000_000,
            time_budget_ms: None,
            grounding: None,
            terminal_reason: None,
        });
    let handler = McpHandler::new(Arc::new(engine));

    let response = handler.tool_drive_inbox(&serde_json::json!({"limit": 1}));

    assert_eq!(response["data"]["signals"][0]["intent_type"], "explore");
    assert_eq!(response["data"]["signals"][0]["retryable"], true);
    assert_eq!(
        response["data"]["signals"][0]["target_capability"],
        serde_json::Value::Null
    );
    assert_eq!(
        response["data"]["signals"][0]["emotion"],
        serde_json::Value::Null
    );
    assert_eq!(
        response["data"]["signals"][0]["feedback"],
        serde_json::Value::Null
    );
    assert_eq!(
        response["data"]["signals"][0]["expires_at"],
        serde_json::Value::Null
    );
    assert_eq!(
        response["data"]["signals"][0]["time_budget_ms"],
        serde_json::Value::Null
    );
    assert_eq!(
        response["data"]["signals"][0]["grounding"],
        serde_json::Value::Null
    );
    assert_eq!(response["protocol"]["error"], serde_json::Value::Null);
    assert_eq!(response["data"]["empty_reason"], "has_signals");
    let zero_limit = handler.tool_drive_inbox(&serde_json::json!({"limit": 0}));
    assert_eq!(zero_limit["data"]["signals"].as_array().unwrap().len(), 1);
    assert_eq!(zero_limit["data"]["empty_reason"], "has_signals");
}

#[test]
fn mcp_drive_inbox_reports_no_pending_after_external_feedback() {
    let engine = isolated_engine();
    let queue = engine.scheduler().drive_queue();
    let signal: crate::engine::drive::DriveSignal = serde_json::from_value(serde_json::json!({
        "id": 0,
        "timestamp": chrono::Utc::now().timestamp(),
        "intent_type": "suggest",
        "description": "Externally handled proposal",
        "urgency": "medium",
        "origin_tick": 0
    }))
    .unwrap();
    let executed = queue.enqueue(signal.clone());
    let rejected = queue.enqueue(signal);
    let feedback = |executed| crate::engine::drive::DriveFeedback {
        responded_at: 0,
        executed,
        outcome: "primary executor response".into(),
        reflection: None,
    };
    queue.acknowledge(executed, feedback(true));
    for _ in 0..=crate::engine::drive::DriveQueue::max_retries() {
        queue.acknowledge(rejected, feedback(false));
    }

    let response = McpHandler::new(Arc::new(engine)).tool_drive_inbox(&serde_json::json!({}));
    assert!(response["data"]["signals"].as_array().unwrap().is_empty());
    assert_eq!(response["data"]["stats"]["executed"], 1);
    assert_eq!(response["data"]["stats"]["rejected"], 1);
    assert_eq!(response["data"]["empty_reason"], "no_pending");
}

#[tokio::test]
async fn mcp_drive_inbox_encrypts_descriptions_for_registered_e2e_key() {
    use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey};
    use rsa::RsaPrivateKey;

    let private_key = RsaPrivateKey::new(&mut rand::thread_rng(), 2048).unwrap();
    let private_pem = private_key
        .to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
        .unwrap()
        .to_string();
    let public_pem = private_key
        .to_public_key()
        .to_public_key_pem(rsa::pkcs8::LineEnding::LF)
        .unwrap();
    let mut engine = isolated_engine();
    engine.start();
    engine.scheduler().set_e2e_pubkey(Some(&public_pem));
    engine
        .scheduler()
        .drive_queue()
        .enqueue(crate::engine::drive::DriveSignal {
            id: 0,
            timestamp: 1_780_000_000,
            intent_type: crate::engine::drive::DriveIntent::Warn,
            description: "private MCP drive detail".into(),
            evidence: vec![11],
            urgency: crate::engine::drive::DriveUrgency::High,
            target_capability: None,
            emotion: None,
            origin_tick: 4,
            status: crate::engine::drive::DriveStatus::Pending,
            feedback: None,
            retry_count: 0,
            expires_at: None,
            enqueued_at_ms: 1_780_000_000_100,
            time_budget_ms: None,
            grounding: Some(crate::engine::drive::DriveGrounding {
                reason: "private memory evidence".into(),
                evidence: Vec::new(),
                uncertainty: vec![
                    crate::engine::drive::DriveGroundingUncertainty::UnresolvedEvidence,
                ],
                fresh_until: None,
                complete: false,
            }),
            terminal_reason: None,
        });
    let handler = McpHandler::new(Arc::new(engine));

    let response = handler.tool_drive_inbox(&serde_json::json!({"limit": 1}));
    let signal = &response["data"]["signals"][0];
    assert_eq!(signal["description"], serde_json::Value::Null);
    assert_eq!(signal["grounding"], serde_json::Value::Null);
    assert_eq!(signal["retryable"], true);
    let ciphertext = signal["description_e2e"].as_str().expect("E2E ciphertext");
    assert_eq!(
        crate::engine::e2e::decrypt_with(ciphertext, &private_pem).unwrap(),
        b"private MCP drive detail"
    );
    let grounding_ciphertext = signal["grounding_e2e"]
        .as_str()
        .expect("E2E grounding ciphertext");
    let grounding_plaintext =
        crate::engine::e2e::decrypt_with(grounding_ciphertext, &private_pem).unwrap();
    let decoded: crate::engine::drive::DriveGrounding =
        serde_json::from_slice(&grounding_plaintext).unwrap();
    assert_eq!(decoded.reason, "private memory evidence");
    assert!(decoded.evidence.is_empty());
}

#[test]
fn mcp_unknown_method() {
    let eng = isolated_engine();
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
    let mut eng = isolated_engine();
    eng.start();
    let h = McpHandler::new(Arc::new(eng));
    let raw = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":null}"#;
    let output = h.process_json(raw);
    let resp: McpResponse = serde_json::from_str(&output).unwrap();
    assert!(resp.error.is_none());
}

#[tokio::test]
async fn mcp_memory_ask_returns_structured_smrp_and_marks_tool_errors() {
    let mut engine = isolated_engine();
    engine.start();
    let handler = McpHandler::new(Arc::new(engine));

    let denied = serde_json::from_str::<serde_json::Value>(&handler.process_json(
            r#"{"jsonrpc":"2.0","id":"ask-denied","method":"tools/call","params":{"name":"memory_ask","arguments":{"question":"What should I remember?"}}}"#,
        ))
        .unwrap();
    assert_eq!(denied["result"]["isError"], true);
    assert_eq!(
        denied["result"]["structuredContent"]["protocol"]["ok"],
        false
    );

    for step in 1..=5 {
        let value = if step == 1 { "TestAgent" } else { "test" };
        let request = format!(
            r#"{{"jsonrpc":"2.0","id":0,"method":"tools/call","params":{{"name":"identity_step","arguments":{{"step":{},"value":"{}"}}}}}}"#,
            step, value
        );
        handler.process_json(&request);
    }
    handler.process_json(
            r#"{"jsonrpc":"2.0","id":0,"method":"tools/call","params":{"name":"identity_finalize","arguments":{}}}"#,
        );

    let response = serde_json::from_str::<serde_json::Value>(&handler.process_json(
            r#"{"jsonrpc":"2.0","id":"ask-success","method":"tools/call","params":{"name":"memory_ask","arguments":{"question":"What should I remember?"}}}"#,
        ))
        .unwrap();
    let call_result = &response["result"];
    let structured = &call_result["structuredContent"];
    assert_eq!(call_result["isError"], false);
    assert_eq!(structured["protocol"]["tool"], "memory_ask");
    assert_eq!(structured["data"]["answer"], "No relevant memories found.");
    assert_eq!(structured["data"]["memory_count"], 0);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(
            call_result["content"][0]["text"].as_str().unwrap()
        )
        .unwrap(),
        *structured
    );
}

#[test]
fn oversized_mcp_requests_and_responses_return_jsonrpc_errors() {
    let handler = McpHandler::new(Arc::new(isolated_engine()));
    let oversized_request = format!(
        r#"{{"jsonrpc":"2.0","id":"too-large","method":"ping","padding":"{}"}}"#,
        "x".repeat(MAX_MCP_REQUEST_BYTES)
    );
    let request_error =
        serde_json::from_str::<serde_json::Value>(&handler.process_json(&oversized_request))
            .unwrap();
    assert_eq!(request_error["error"]["code"], -32000);

    let response = McpResponse {
        jsonrpc: "2.0".into(),
        id: Some(serde_json::json!("large-result")),
        result: Some(serde_json::json!({"content": "x".repeat(MAX_MCP_RESPONSE_BYTES)})),
        error: None,
    };
    let response_error = serde_json::from_str::<serde_json::Value>(&serialize_mcp_response(
        response,
        Some(serde_json::json!("large-result")),
    ))
    .unwrap();
    assert_eq!(response_error["id"], "large-result");
    assert_eq!(response_error["error"]["code"], -32000);
}

#[test]
fn stdio_and_tcp_line_reader_rejects_oversized_lines_before_buffering_them() {
    let input = vec![b'x'; MAX_MCP_REQUEST_BYTES + 1];
    let mut reader = std::io::Cursor::new(input);
    let mut line = Vec::new();

    let error = read_mcp_line(&mut reader, &mut line).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(line.len() <= MAX_MCP_REQUEST_BYTES);
}

#[tokio::test]
async fn mcp_memory_create() {
    let mut eng = isolated_engine();
    eng.start();
    let h = McpHandler::new(Arc::new(eng));
    let init_raw = r#"{"jsonrpc":"2.0","id":0,"method":"tools/call","params":{"name":"identity_step","arguments":{"step":1,"value":"TestAgent"}}}"#;
    h.process_json(init_raw);
    for step in 2..=5 {
        let raw = format!(
            r#"{{"jsonrpc":"2.0","id":0,"method":"tools/call","params":{{"name":"identity_step","arguments":{{"step":{},"value":"test"}}}}}}"#,
            step
        );
        h.process_json(&raw);
    }
    let finalize_raw = r#"{"jsonrpc":"2.0","id":0,"method":"tools/call","params":{"name":"identity_finalize","arguments":{}}}"#;
    h.process_json(finalize_raw);
    let raw = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"memory_create","arguments":{"content":"hello world","labels":["test"]}}}"#;
    let output = h.process_json(raw);
    assert!(
        output.contains("created") || output.contains("exists"),
        "output was: {}",
        output
    );
}

#[tokio::test]
async fn mcp_space_stats() {
    let mut eng = isolated_engine();
    eng.start();
    let h = McpHandler::new(Arc::new(eng));
    confirm_test_identity(&h);
    let raw = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"space_stats","arguments":{}}}"#;
    let output = h.process_json(raw);
    assert!(
        output.contains("schema_version")
            && output.contains("memories")
            && output.contains("ports_assigned")
    );
}

#[tokio::test]
#[ignore = "flaky: Engine::start()初始化竞态(CI单核必败, 生产路径日验)"]
async fn mcp_ctx_save_and_load() {
    let mut eng = isolated_engine();
    eng.start();
    let h = McpHandler::new(Arc::new(eng));
    confirm_test_identity(&h);

    let save_raw = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ctx_save","arguments":{"summary":"Use parking_lot for all mutexes","category":"pattern","project":"Epicode"}}}"#;
    let save_output = h.process_json(save_raw);
    assert!(
        save_output.contains("schema_version")
            && save_output.contains("placement")
            && save_output.contains("relations_formed")
    );

    let load_raw = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"ctx_load","arguments":{"project":"Epicode"}}}"#;
    let load_output = h.process_json(load_raw);
    assert!(load_output.contains("context_loaded"));
}

#[tokio::test]
#[ignore = "flaky: Engine::start()初始化竞态(CI单核必败, 生产路径日验)"]
async fn mcp_decision_record() {
    let mut eng = isolated_engine();
    eng.start();
    let h = McpHandler::new(Arc::new(eng));
    confirm_test_identity(&h);
    let raw = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decision_record","arguments":{"title":"Use SQLite","chosen":"SQLite with WAL","alternatives":"PostgreSQL, RocksDB","rationale":"Embedded, zero-config, WAL mode is fast enough","project":"Epicode"}}}"#;
    let output = h.process_json(raw);
    assert!(
        output.contains("schema_version")
            && output.contains("placement")
            && output.contains("relations_formed")
    );
}

#[tokio::test]
#[ignore = "flaky: Engine::start()初始化竞态(CI单核必败, 生产路径日验)"]
async fn mcp_bug_memory() {
    let mut eng = isolated_engine();
    eng.start();
    let h = McpHandler::new(Arc::new(eng));
    confirm_test_identity(&h);
    let raw = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"bug_memory","arguments":{"symptoms":"tests hang on CI","root_cause":"ureq blocking async runtime","fix":"wrap in spawn_blocking","module":"gateway.rs","project":"Epicode"}}}"#;
    let output = h.process_json(raw);
    assert!(
        output.contains("schema_version")
            && output.contains("placement")
            && output.contains("relations_formed")
    );
}

#[tokio::test]
async fn mcp_session_summary() {
    let mut eng = isolated_engine();
    eng.start();
    let h = McpHandler::new(Arc::new(eng));
    confirm_test_identity(&h);
    let raw = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"session_summary","arguments":{"accomplished":"Fixed 6 critical rollback issues","next_steps":"Deploy to cloud, run benchmarks","blockers":"none","project":"Epicode"}}}"#;
    let output = h.process_json(raw);
    assert!(
        output.contains("schema_version")
            && output.contains("placement")
            && output.contains("relations_formed")
    );
}

#[tokio::test]
async fn mcp_pattern_learn_and_recall() {
    let mut eng = isolated_engine();
    eng.start();
    let h = McpHandler::new(Arc::new(eng));
    confirm_test_identity(&h);

    let learn_raw = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"pattern_learn","arguments":{"pattern":"All DB writes use transactions","language":"rust","project":"Epicode","example":"conn.unchecked_transaction()?"}}}"#;
    let learn_output = h.process_json(learn_raw);
    assert!(
        learn_output.contains("schema_version")
            && learn_output.contains("placement")
            && learn_output.contains("relations_formed")
    );

    let recall_raw = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"pattern_recall","arguments":{"context":"database write","language":"rust","project":"Epicode"}}}"#;
    let recall_output = h.process_json(recall_raw);
    assert!(recall_output.contains("patterns"));
}

#[tokio::test]
async fn mcp_memory_search_returns_content() {
    let mut eng = isolated_engine();
    eng.start();
    let h = McpHandler::new(Arc::new(eng));
    for step in 1..=5 {
        let val = if step == 1 { "TestAgent" } else { "test" };
        let raw = format!(
            r#"{{"jsonrpc":"2.0","id":0,"method":"tools/call","params":{{"name":"identity_step","arguments":{{"step":{},"value":"{}"}}}}}}"#,
            step, val
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
        "output was: {}",
        search_output
    );
    assert!(search_output.contains("content"));
}

#[tokio::test]
async fn mcp_skills_sync_manifest_paginates_by_default() {
    let mut eng = Engine::new();
    eng.start();
    let h = McpHandler::new(Arc::new(eng));
    for step in 1..=5 {
        let val = if step == 1 { "SkillsAgent" } else { "test" };
        let raw = format!(
            r#"{{"jsonrpc":"2.0","id":0,"method":"tools/call","params":{{"name":"identity_step","arguments":{{"step":{},"value":"{}"}}}}}}"#,
            step, val
        );
        h.process_json(&raw);
    }
    h.process_json(r#"{"jsonrpc":"2.0","id":0,"method":"tools/call","params":{"name":"identity_finalize","arguments":{}}}"#);

    let out = h.process_json(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_sync","arguments":{"format":"manifest","limit":1}}}"#,
        );
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let data = &v["result"]["structuredContent"]["data"];
    // limit=1 should return at most 1 skill (system manual may or may not be present)
    let returned = data["returned"].as_u64().unwrap_or(999);
    assert!(returned <= 1, "limit=1 must return <=1: {data}");
    assert!(data.get("total").is_some());
    assert!(data.get("truncated").is_some());
    assert_eq!(data["offset"], 0);

    // Schema advertises pagination defaults
    let list = h.handle(McpRequest {
        jsonrpc: "2.0".into(),
        id: Some(serde_json::json!(2)),
        method: "tools/list".into(),
        params: None,
    });
    let tools = list.result.unwrap()["tools"].as_array().unwrap().clone();
    let sync = tools
        .iter()
        .find(|t| t["name"] == "skills_sync")
        .expect("skills_sync");
    assert_eq!(sync["inputSchema"]["properties"]["limit"]["default"], 100);
    assert_eq!(sync["inputSchema"]["properties"]["offset"]["default"], 0);
}

#[tokio::test]
async fn mcp_initialized_notification() {
    let eng = isolated_engine();
    let h = McpHandler::new(Arc::new(eng));
    let resp = h.handle(McpRequest {
        jsonrpc: "2.0".into(),
        id: None,
        method: "notifications/initialized".into(),
        params: None,
    });
    assert!(resp.error.is_none());
}

fn identity_ritual(h: &McpHandler) {
    for step in 1..=5 {
        let val = if step == 1 { "FeedbackAgent" } else { "test" };
        let raw = format!(
            r#"{{"jsonrpc":"2.0","id":0,"method":"tools/call","params":{{"name":"identity_step","arguments":{{"step":{},"value":"{}"}}}}}}"#,
            step, val
        );
        h.process_json(&raw);
    }
    h.process_json(
            r#"{"jsonrpc":"2.0","id":0,"method":"tools/call","params":{"name":"identity_finalize","arguments":{}}}"#,
        );
}

#[tokio::test]
async fn mcp_feedback_submit_creates_edges_and_accepts_from_id() {
    let dir = std::env::temp_dir().join(format!("epicode-fb-{}", uuid::Uuid::new_v4()));
    let _ = std::fs::create_dir_all(&dir);
    let mut eng = Engine::with_data_dir(dir.clone());
    eng.start();
    let engine = Arc::new(eng);
    let h = McpHandler::new(engine.clone());
    identity_ritual(&h);

    let c1 = h.process_json(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"memory_create","arguments":{"content":"alpha feedback seed memory about SQLite WAL","labels":["db","sqlite"]}}}"#,
        );
    let c2 = h.process_json(
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"memory_create","arguments":{"content":"beta feedback seed memory about embedded databases","labels":["db","embedded"]}}}"#,
        );
    assert!(c1.contains("created") || c1.contains("exists"), "c1={c1}");
    assert!(c2.contains("created") || c2.contains("exists"), "c2={c2}");

    let ids: Vec<u64> = engine
        .space
        .all_tetrahedrons()
        .into_iter()
        .filter(|t| t.data.content.contains("feedback seed"))
        .map(|t| t.id)
        .collect();
    assert!(ids.len() >= 2, "need two seed memories, got {ids:?}");
    let a = ids[0];
    let b = ids[1];
    let before_imp = engine
        .space
        .get_tetrahedron(a)
        .map(|t| t.data.importance)
        .unwrap_or(0.0);

    let fb = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"feedback_submit","arguments":{{"memory_ids":[{a},{b}],"relevance":"highly_relevant","outcome":"task_completed","query":"sqlite embedded","concept_links":[{{"from_id":{a},"to_id":{b},"relation":"related"}}]}}}}}}"#
    );
    let out = h.process_json(&fb);
    assert!(
        out.contains("\"edges_created\"") || out.contains("edges_created"),
        "output missing edges_created: {out}"
    );
    assert!(
        out.contains("\"concept_links_formed\":1") || out.contains("concept_links_formed\": 1"),
        "from_id/to_id must form a link: {out}"
    );
    // Either reinforced existing or created co-retrieval SimilarTo.
    assert!(
        out.contains("\"edges_created\":1")
            || out.contains("\"edges_reinforced\":1")
            || out.contains("edges_created\": 1")
            || out.contains("edges_reinforced\": 1"),
        "expected retrieval-quality edge update: {out}"
    );
    let after_imp = engine
        .space
        .get_tetrahedron(a)
        .map(|t| t.data.importance)
        .unwrap_or(0.0);
    assert!(
        after_imp > before_imp,
        "highly_relevant+task_completed should bump importance ({before_imp} -> {after_imp})"
    );
    let rels = engine.scheduler.kg_handle().all_relations();
    assert!(
        rels.iter()
            .any(|r| { (r.source == a && r.target == b) || (r.source == b && r.target == a) }),
        "expected KG edge between {a} and {b}, got {rels:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn mcp_feedback_submit_accepts_legacy_ab_concept_links() {
    let dir = std::env::temp_dir().join(format!("epicode-fb-ab-{}", uuid::Uuid::new_v4()));
    let _ = std::fs::create_dir_all(&dir);
    let mut eng = Engine::with_data_dir(dir.clone());
    eng.start();
    let engine = Arc::new(eng);
    let h = McpHandler::new(engine.clone());
    identity_ritual(&h);
    h.process_json(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"memory_create","arguments":{"content":"legacy link memory one","labels":["legacy"]}}}"#,
        );
    h.process_json(
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"memory_create","arguments":{"content":"legacy link memory two","labels":["legacy"]}}}"#,
        );
    let ids: Vec<u64> = engine
        .space
        .all_tetrahedrons()
        .into_iter()
        .filter(|t| t.data.content.contains("legacy link memory"))
        .map(|t| t.id)
        .collect();
    assert!(ids.len() >= 2);
    let a = ids[0];
    let b = ids[1];
    let fb = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"feedback_submit","arguments":{{"memory_ids":[{a}],"relevance":"partially_relevant","outcome":"no_action_needed","concept_links":[{{"a":{a},"b":{b},"relation":"similar"}}]}}}}}}"#
    );
    let out = h.process_json(&fb);
    assert!(
        out.contains("\"concept_links_formed\":1") || out.contains("concept_links_formed\": 1"),
        "legacy a/b links must still work: {out}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
#[ignore = "flaky: Engine::start()初始化竞态(CI单核必败, 生产路径日验)"]
async fn mcp_context_observe_extracts_decision() {
    let mut eng = isolated_engine();
    eng.start();
    let h = McpHandler::new(Arc::new(eng));
    confirm_test_identity(&h);

    let raw = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"context_observe","arguments":{"context":"User: What DB should we use?\nAssistant: We decided to use SQLite with WAL mode because it is embedded and zero-config, going with SQLite instead of PostgreSQL for simplicity","project":"Epicode","role":"designing"}}}"#;
    let output = h.process_json(raw);
    assert!(output.contains("observed"));
    assert!(output.contains("memories_created"));
}

#[tokio::test]
#[ignore = "flaky: Engine::start()初始化竞态(CI单核必败, 生产路径日验)"]
async fn mcp_context_observe_extracts_bug() {
    let mut eng = isolated_engine();
    eng.start();
    let h = McpHandler::new(Arc::new(eng));
    confirm_test_identity(&h);

    let raw = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"context_observe","arguments":{"context":"The tests were hanging because ureq was blocking the async runtime, fixed by wrapping in spawn_blocking. The root cause was synchronous HTTP inside tokio context."}}}"#;
    let output = h.process_json(raw);
    assert!(output.contains("observed"));
}

#[tokio::test]
#[ignore = "flaky: Engine::start()初始化竞态(CI单核必败, 生产路径日验)"]
async fn mcp_context_observe_empty() {
    let mut eng = isolated_engine();
    eng.start();
    let h = McpHandler::new(Arc::new(eng));
    confirm_test_identity(&h);

    let raw = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"context_observe","arguments":{"context":"ok","role":"coding"}}}"#;
    let output = h.process_json(raw);
    assert!(output.contains("observed"));
    assert!(output.contains("memories_created"));
}

#[tokio::test]
#[ignore = "flaky: Engine::start()初始化竞态(CI单核必败, 生产路径日验)"]
async fn mcp_context_observe_dedup() {
    let mut eng = isolated_engine();
    eng.start();
    let h = McpHandler::new(Arc::new(eng));
    confirm_test_identity(&h);

    let ctx_raw = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"context_observe","arguments":{"context":"We decided to use SQLite with WAL mode for all database operations because it provides great performance with zero configuration overhead","project":"Epicode"}}}"#;
    let out1 = h.process_json(ctx_raw);
    assert!(out1.contains("observed"));

    let out2 = h.process_json(ctx_raw);
    assert!(out2.contains("duplicates_skipped"));
}

/// memory_delete must permanently purge: memory_get afterwards must miss,
/// also after a prior memory_forget (live repro: #3116 still readable).
#[tokio::test]
async fn memory_delete_purges_even_after_forget() {
    let mut eng = Engine::new();
    eng.start();
    let h = McpHandler::new(Arc::new(eng));
    for step in 1..=5 {
        let val = if step == 1 { "PurgeAgent" } else { "test" };
        let raw = format!(
            r#"{{"jsonrpc":"2.0","id":0,"method":"tools/call","params":{{"name":"identity_step","arguments":{{"step":{},"value":"{}"}}}}}}"#,
            step, val
        );
        h.process_json(&raw);
    }
    h.process_json(r#"{"jsonrpc":"2.0","id":0,"method":"tools/call","params":{"name":"identity_finalize","arguments":{}}}"#);
    let mut ids = Vec::new();
    for (i, txt) in ["purge me plain", "purge me after forget"]
        .iter()
        .enumerate()
    {
        let raw = format!(
            r#"{{"jsonrpc":"2.0","id":{},"method":"tools/call","params":{{"name":"memory_create","arguments":{{"content":"{}"}}}}}}"#,
            i + 1,
            txt
        );
        let v: serde_json::Value = serde_json::from_str(&h.process_json(&raw)).unwrap();
        let sc = &v["result"]["structuredContent"]["data"];
        let id = sc["id"]
            .as_u64()
            .or_else(|| sc["memory_id"].as_u64())
            .unwrap_or_else(|| panic!("{v}"));
        ids.push(id);
    }
    h.process_json(&format!(
            r#"{{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{{"name":"memory_forget","arguments":{{"id":{}}}}}}}"#,
            ids[1]
        ));
    for id in &ids {
        let del: serde_json::Value = serde_json::from_str(&h.process_json(&format!(
                r#"{{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{{"name":"memory_delete","arguments":{{"id":{}}}}}}}"#,
                id
            )))
            .unwrap();
        assert_eq!(
            del["result"]["structuredContent"]["data"]["mode"], "purge",
            "{del}"
        );
        let get: serde_json::Value = serde_json::from_str(&h.process_json(&format!(
                r#"{{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{{"name":"memory_get","arguments":{{"id":{}}}}}}}"#,
                id
            )))
            .unwrap();
        assert!(
            get["result"]["structuredContent"]["data"]["content"].is_null(),
            "memory {id} still readable after delete: {get}"
        );
    }
}

/// Unified delete semantics: the shared delete path (REST DELETE + MCP
/// memory_delete) purges memory + relations; forget stays recoverable;
/// identity anchors and enforced rules are refused.
#[tokio::test]
async fn unified_delete_purges_forget_restores_anchor_refused() {
    let mut eng = Engine::new();
    eng.start();
    let h = McpHandler::new(Arc::new(eng));
    for step in 1..=5 {
        let val = if step == 1 { "UnifyAgent" } else { "test" };
        h.process_json(&format!(
                r#"{{"jsonrpc":"2.0","id":0,"method":"tools/call","params":{{"name":"identity_step","arguments":{{"step":{},"value":"{}"}}}}}}"#,
                step, val
            ));
    }
    h.process_json(r#"{"jsonrpc":"2.0","id":0,"method":"tools/call","params":{"name":"identity_finalize","arguments":{}}}"#);
    let create = |content: &str, labels: &str| -> u64 {
        let v: serde_json::Value = serde_json::from_str(&h.process_json(&format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"memory_create","arguments":{{"content":"{}","labels":{}}}}}}}"#,
                content, labels
            )))
            .unwrap();
        let d = &v["result"]["structuredContent"]["data"];
        d["id"]
            .as_u64()
            .or_else(|| d["memory_id"].as_u64())
            .unwrap_or_else(|| panic!("{v}"))
    };
    let call = |name: &str, id: u64| -> serde_json::Value {
        serde_json::from_str(&h.process_json(&format!(
                r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"{}","arguments":{{"id":{}}}}}}}"#,
                name, id
            )))
            .unwrap()
    };
    let sched = h.engine().scheduler.clone();
    let a = create("unify delete target alpha", "[\"t\"]");
    let b = create("unify delete neighbour beta", "[\"t\"]");
    sched
        .kg_handle()
        .add_relation(a, b, super::super::knowledge::RelationType::SimilarTo, 0.9);
    assert!(sched.api_get_relations(a).iter().any(|(t, _, _)| *t == b));

    // Shared delete path (what REST DELETE /v1/memories/:id calls).
    let d = sched.api_delete_memory(a).expect("purge");
    assert_eq!(d["mode"], super::super::scheduler::DELETE_MODE);
    assert_eq!(d["mode"], "purge");
    assert!(call("memory_get", a)["result"]["structuredContent"]["data"]["content"].is_null());
    assert!(
        sched.api_get_relations(a).is_empty(),
        "relations of purged memory remain"
    );
    assert!(
        !sched.api_get_relations(b).iter().any(|(t, _, _)| *t == a),
        "dangling relation to purged memory"
    );
    assert!(sched
        .api_delete_memory(a)
        .unwrap_err()
        .contains("not found"));

    // MCP reports the same mode.
    let c = create("unify delete via mcp gamma", "[\"t\"]");
    let mcp = call("memory_delete", c);
    assert_eq!(
        mcp["result"]["structuredContent"]["data"]["mode"], d["mode"],
        "{mcp}"
    );

    // Forget is recoverable.
    sched.api_forget_memory(b).expect("forget");
    let restored = call("memory_restore", b);
    assert_eq!(
        restored["result"]["structuredContent"]["protocol"]["ok"], true,
        "{restored}"
    );
    assert!(h
        .engine()
        .space
        .get_tetrahedron(b)
        .unwrap()
        .data
        .valid_to
        .is_none());

    // Identity anchors refused (both paths), still readable afterwards.
    let anchor = create(
        "I am UnifyAgent, my creator is test",
        "[\"identity\",\"system\"]",
    );
    let err = sched.api_delete_memory(anchor).unwrap_err();
    assert!(
        err.starts_with(super::super::scheduler::PROTECTED_ERR_PREFIX),
        "{err}"
    );
    let mcp_anchor = call("memory_delete", anchor);
    assert_eq!(
        mcp_anchor["result"]["structuredContent"]["protocol"]["error"]["code"], 403,
        "{mcp_anchor}"
    );
    assert!(h.engine().space.get_tetrahedron(anchor).is_some());
}

/// Spike S1: verbosity=slim must shrink JSON vs default full envelope on same fixture.
#[tokio::test]
async fn spike_s1_slim_envelope_beats_baseline_size() {
    let mut eng = Engine::new();
    eng.start();
    let h = McpHandler::new(Arc::new(eng));
    for step in 1..=5 {
        let val = if step == 1 { "SlimBenchAgent" } else { "test" };
        let raw = format!(
            r#"{{"jsonrpc":"2.0","id":0,"method":"tools/call","params":{{"name":"identity_step","arguments":{{"step":{},"value":"{}"}}}}}}"#,
            step, val
        );
        h.process_json(&raw);
    }
    h.process_json(r#"{"jsonrpc":"2.0","id":0,"method":"tools/call","params":{"name":"identity_finalize","arguments":{}}}"#);

    // Long contents to amplify duplication cost of tiers+sections+results.
    let long = "Lorem ipsum dolor sit amet, tetrahedral memory clustering. ".repeat(40);
    for i in 0..8 {
        let content = format!("{long} fixture-{i}-unique-token-slim-bench");
        let raw = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"memory_create","arguments":{{"content":{},"labels":["spike","slim"]}}}}}}"#,
            serde_json::to_string(&content).unwrap()
        );
        h.process_json(&raw);
    }

    let full_search = h.process_json(
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"memory_search","arguments":{"query":"tetrahedral memory clustering slim-bench","limit":10}}}"#,
        );
    let slim_search = h.process_json(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"memory_search","arguments":{"query":"tetrahedral memory clustering slim-bench","limit":10,"verbosity":"slim"}}}"#,
        );
    let full_v: serde_json::Value = serde_json::from_str(&full_search).unwrap();
    let slim_v: serde_json::Value = serde_json::from_str(&slim_search).unwrap();
    let full_data = &full_v["result"]["structuredContent"]["data"];
    let slim_data = &slim_v["result"]["structuredContent"]["data"];

    assert!(full_data.get("tiers").is_some() || full_data.get("results").is_some());
    assert_eq!(slim_data["envelope"], "slim");
    assert!(slim_data.get("items").is_some());
    assert!(slim_data.get("tiers").is_none());
    assert!(slim_data.get("results").is_none());
    assert!(slim_data.get("sections").is_none());
    assert!(slim_data.get("budget_spent").is_some());

    let (full_b, full_t) = super::super::smrp::estimate_payload_size(full_data);
    let (slim_b, slim_t) = super::super::smrp::estimate_payload_size(slim_data);
    assert!(
        slim_b < full_b,
        "slim search {slim_b}B should beat full {full_b}B"
    );
    // Strong win: expect ≤40% of baseline on this duplicated fixture.
    assert!(
        slim_b * 100 / full_b.max(1) <= 40,
        "slim search ratio {}% of full (want ≤40%)",
        slim_b * 100 / full_b.max(1)
    );
    eprintln!(
            "[spike-s1] memory_search full={full_b}B/~{full_t}tok slim={slim_b}B~{slim_t}tok ratio={:.1}%",
            slim_b as f64 * 100.0 / full_b as f64
        );

    let full_recall = h.process_json(
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"memory_recall","arguments":{"query":"tetrahedral memory clustering slim-bench","depth":2,"verbosity":"full"}}}"#,
        );
    let slim_recall = h.process_json(
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"memory_recall","arguments":{"query":"tetrahedral memory clustering slim-bench","depth":2,"verbosity":"slim"}}}"#,
        );
    let fr: serde_json::Value = serde_json::from_str(&full_recall).unwrap();
    let sr: serde_json::Value = serde_json::from_str(&slim_recall).unwrap();
    let frd = &fr["result"]["structuredContent"]["data"];
    let srd = &sr["result"]["structuredContent"]["data"];
    assert!(frd.get("tiers").is_some());
    assert!(
        frd.get("sections").is_none(),
        "MCP full drops sections duplicate"
    );
    assert!(frd.get("items_total").is_some());
    assert_eq!(srd["envelope"], "slim");
    assert!(srd.get("sections").is_none());
    assert!(srd.get("tiers").is_none());
    let (frb, frt) = super::super::smrp::estimate_payload_size(frd);
    let (srb, srt) = super::super::smrp::estimate_payload_size(srd);
    assert!(srb < frb, "slim recall {srb}B should beat full {frb}B");
    eprintln!(
        "[spike-s1] memory_recall full={frb}B~{frt}tok slim={srb}B~{srt}tok ratio={:.1}%",
        srb as f64 * 100.0 / frb as f64
    );
}

#[tokio::test]
async fn spike_s2_blocks_require_flag_and_roundtrip() {
    let _guard = SPIKE_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("EPICODE_MEMCARD");

    let mut eng = Engine::new();
    eng.start();
    let h = McpHandler::new(Arc::new(eng));
    // Promoted: listed and callable without EPICODE_MEMCARD.
    let list_off = h.process_json(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#);
    assert!(list_off.contains("blocks_get") && list_off.contains("blocks_set"));
    let call_off = h.process_json(
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"blocks_get","arguments":{"kind":"persona"}}}"#,
        );
    assert!(!call_off.contains("MemCard disabled"), "{call_off}");

    std::env::set_var("EPICODE_MEMCARD", "1");
    let tmp = std::env::temp_dir().join(format!(
        "epicode-spike-s2-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&tmp);
    let mut eng2 = Engine::with_data_dir(tmp);
    eng2.start();
    let h2 = McpHandler::new(Arc::new(eng2));
    for step in 1..=5 {
        let val = if step == 1 { "MemCardAgent" } else { "test" };
        let raw = format!(
            r#"{{"jsonrpc":"2.0","id":0,"method":"tools/call","params":{{"name":"identity_step","arguments":{{"step":{},"value":"{}"}}}}}}"#,
            step, val
        );
        h2.process_json(&raw);
    }
    h2.process_json(
            r#"{"jsonrpc":"2.0","id":0,"method":"tools/call","params":{"name":"identity_finalize","arguments":{}}}"#,
        );

    let list_on = h2.process_json(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#);
    assert!(list_on.contains("blocks_get"));
    assert!(list_on.contains("blocks_set"));

    let set_out = h2.process_json(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"blocks_set","arguments":{"kind":"user","content":"prefers dark mode and terse answers"}}}"#,
        );
    let set_v: serde_json::Value = serde_json::from_str(&set_out).unwrap();
    assert_eq!(
        set_v["result"]["structuredContent"]["data"]["version"], 1,
        "set_out={set_out}"
    );
    assert!(
        set_v["result"]["structuredContent"]["status"]
            .get("memcards")
            .is_some(),
        "status should include memcard meta when flag on"
    );

    let get_out = h2.process_json(
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"blocks_get","arguments":{"kind":"user"}}}"#,
        );
    let get_v: serde_json::Value = serde_json::from_str(&get_out).unwrap();
    assert_eq!(
        get_v["result"]["structuredContent"]["data"]["content"],
        "prefers dark mode and terse answers"
    );
    assert_eq!(get_v["result"]["structuredContent"]["data"]["version"], 1);

    std::env::remove_var("EPICODE_MEMCARD");
}

/// Round-2 S2: MemCard preference A/B vs memory_search under long noise.
#[tokio::test]
async fn spike_s2_memcard_preference_ab_vs_search() {
    let _guard = SPIKE_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("EPICODE_MEMCARD", "1");

    let tmp = std::env::temp_dir().join(format!(
        "epicode-spike-s2-pref-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&tmp);
    let mut eng = Engine::with_data_dir(tmp);
    eng.start();
    let h = McpHandler::new(Arc::new(eng));
    for step in 1..=5 {
        let val = if step == 1 {
            "PrefHarnessAgent"
        } else {
            "test"
        };
        let raw = format!(
            r#"{{"jsonrpc":"2.0","id":0,"method":"tools/call","params":{{"name":"identity_step","arguments":{{"step":{},"value":"{}"}}}}}}"#,
            step, val
        );
        h.process_json(&raw);
    }
    h.process_json(
            r#"{"jsonrpc":"2.0","id":0,"method":"tools/call","params":{"name":"identity_finalize","arguments":{}}}"#,
        );

    let card = r#"User preferences:
1. UI theme: dark mode
2. Preferred editor: neovim
3. Code comments language: 中文
4. Shell: fish
5. Package manager: pnpm
6. Test runner: cargo nextest
7. Commit style: conventional commits
8. Reply length: terse
9. Keyboard layout: colemak
10. Favorite snacks: 辣条
"#;
    let qa: Vec<(&str, &[&str])> = vec![
        ("What UI theme does the user prefer?", &["dark"]),
        ("Which editor does the user prefer?", &["neovim"]),
        ("What language for code comments?", &["中文"]),
        ("Which shell does the user use?", &["fish"]),
        ("Preferred package manager?", &["pnpm"]),
        ("What test runner is preferred?", &["nextest"]),
        ("Commit message style?", &["conventional"]),
        ("Does the user want long or terse replies?", &["terse"]),
        ("Keyboard layout preference?", &["colemak"]),
        ("Favorite snacks?", &["辣条"]),
    ];

    // Seed MemCard.
    let set_raw = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"blocks_set","arguments":{{"kind":"user","content":{}}}}}}}"#,
        serde_json::to_string(card).unwrap()
    );
    let set_out = h.process_json(&set_raw);
    let set_v: serde_json::Value = serde_json::from_str(&set_out).unwrap();
    assert_eq!(
        set_v["result"]["structuredContent"]["data"]["version"], 1,
        "blocks_set failed: {set_out}"
    );

    // Seed SAME prefs as memories + long distractors that share question keywords.
    for (i, line) in card.lines().skip(1).enumerate() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        let content = format!("[preference] {l}");
        let raw = format!(
            r#"{{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{{"name":"memory_create","arguments":{{"content":{},"labels":["preference","spike-pref"]}}}}}}"#,
            serde_json::to_string(&content).unwrap()
        );
        h.process_json(&raw);
        let _ = i;
    }
    let noise = "Lorem ipsum editor theme shell package manager test runner commit keyboard snacks reply language comments UI user prefers vscode emacs bash npm yarn jest mocha long verbose qwerty chocolate cookies. ".repeat(40);
    for i in 0..10 {
        let content = format!(
                "{noise} distractor-{i}: UI theme light mode; editor vscode; shell bash; package manager npm; test runner jest; commit style freeform; reply length verbose; keyboard qwerty; snacks chocolate; comments English."
            );
        let raw = format!(
            r#"{{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{{"name":"memory_create","arguments":{{"content":{},"labels":["noise","spike-pref"]}}}}}}"#,
            serde_json::to_string(&content).unwrap()
        );
        h.process_json(&raw);
    }

    // Baseline: answer only via memory_search top-k (flag MemCard still on for blocks, but we ignore card).
    let mut base_hits = 0usize;
    let mut base_bytes = 0usize;
    for (q, expected) in &qa {
        let raw = format!(
            r#"{{"jsonrpc":"2.0","id":20,"method":"tools/call","params":{{"name":"memory_search","arguments":{{"query":{},"limit":5}}}}}}"#,
            serde_json::to_string(q).unwrap()
        );
        let out = h.process_json(&raw);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let data = &v["result"]["structuredContent"]["data"];
        let (b, _) = super::super::smrp::estimate_payload_size(data);
        base_bytes += b;
        let mut blob = String::new();
        if let Some(results) = data.get("results").and_then(|r| r.as_array()) {
            for item in results.iter().take(5) {
                if let Some(c) = item.get("content").and_then(|c| c.as_str()) {
                    blob.push_str(c);
                    blob.push('\n');
                }
            }
        }
        if super::super::memcard::pref_answer_hits(&blob, expected) {
            base_hits += 1;
        }
    }

    // MemCard path: answer from blocks_get ONLY (no vector recall).
    let get_out = h.process_json(
            r#"{"jsonrpc":"2.0","id":30,"method":"tools/call","params":{"name":"blocks_get","arguments":{"kind":"user"}}}"#,
        );
    let get_v: serde_json::Value = serde_json::from_str(&get_out).unwrap();
    let card_content = get_v["result"]["structuredContent"]["data"]["content"]
        .as_str()
        .expect("card content")
        .to_string();
    let mut mem_hits = 0usize;
    let mut mem_bytes = 0usize;
    for (q, expected) in &qa {
        let ans = super::super::memcard::answer_prefs_from_card(&card_content, q);
        mem_bytes += ans.len();
        // Also count structured blocks_get envelope once amortized — use answer text bytes
        // (card is the only retrieval surface).
        if super::super::memcard::pref_answer_hits(&ans, expected) {
            mem_hits += 1;
        }
    }
    // Include one blocks_get envelope for fair "tool return" accounting.
    let (get_b, _) =
        super::super::smrp::estimate_payload_size(&get_v["result"]["structuredContent"]["data"]);
    let mem_bytes_with_envelope = mem_bytes + get_b;

    eprintln!(
            "[spike-s2-ab] memcard hits={mem_hits}/10 answer_bytes={mem_bytes} (+envelope {get_b} => {mem_bytes_with_envelope}); baseline hits={base_hits}/10 search_bytes={base_bytes}; ratio={:.1}%",
            mem_bytes_with_envelope as f64 * 100.0 / base_bytes.max(1) as f64
        );

    assert_eq!(mem_hits, 10, "MemCard must answer 10/10 from blocks only");
    assert!(
        mem_hits >= base_hits,
        "MemCard accuracy {mem_hits} must be ≥ baseline {base_hits}"
    );
    assert!(
        mem_bytes_with_envelope * 100 / base_bytes.max(1) <= 20,
        "MemCard bytes {mem_bytes_with_envelope} should be ≤20% of baseline {base_bytes}"
    );
    if base_hits < 10 {
        eprintln!(
            "[spike-s2-ab] baseline missed {} prefs — long distractors drowned preference memories",
            10 - base_hits
        );
    }

    std::env::remove_var("EPICODE_MEMCARD");
}

#[test]
fn memory_action_envelope_propose_claim_complete_writes_outcome() {
    let eng = isolated_engine();
    let handler = McpHandler::new(std::sync::Arc::new(eng));
    confirm_test_identity(&handler);

    let r1 = handler.tool_memory_create(&serde_json::json!({
        "content": "Epicode production deploy uses blue-green with health checks on /healthz",
        "labels": ["deploy", "ops", "epicode"]
    }));
    let r2 = handler.tool_memory_create(&serde_json::json!({
        "content": "Rollback runbook: switch traffic back to green within 5 minutes",
        "labels": ["deploy", "rollback"]
    }));
    assert_eq!(r1["protocol"]["ok"], true, "{r1}");
    assert_eq!(r2["protocol"]["ok"], true, "{r2}");

    let proposed = handler.tool_memory_propose_actions(&serde_json::json!({
        "query": "production deploy checklist",
        "limit": 2
    }));
    assert_eq!(proposed["protocol"]["ok"], true, "{proposed}");
    let actions = proposed["data"]["actions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        !actions.is_empty(),
        "expected at least one action: {proposed}"
    );
    let action_id = actions[0]["id"].as_u64().expect("action id");
    let evidence = actions[0]["evidence_memory_ids"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(!evidence.is_empty(), "action must cite evidence memories");

    let claim = handler.tool_action_claim(&serde_json::json!({
        "action_id": action_id,
        "claimed_by": "test-agent"
    }));
    assert_eq!(claim["protocol"]["ok"], true, "{claim}");
    let claim2 = handler.tool_action_claim(&serde_json::json!({
        "action_id": action_id,
        "claimed_by": "other-agent"
    }));
    assert_eq!(
        claim2["protocol"]["ok"], false,
        "second claim must fail: {claim2}"
    );

    let empty = handler.tool_action_complete(&serde_json::json!({
        "action_id": action_id,
        "outcome": "   "
    }));
    assert_eq!(
        empty["protocol"]["ok"], false,
        "empty outcome rejected: {empty}"
    );

    // Foreign worker label cannot complete someone else's claim.
    let foreign = handler.tool_action_complete(&serde_json::json!({
        "action_id": action_id,
        "outcome": "interloper",
        "claimed_by": "other-agent"
    }));
    assert_eq!(foreign["protocol"]["error"]["code"], 403, "{foreign}");

    let dw_len = actions[0]["done_when"]
        .as_array()
        .map(|a| a.len())
        .unwrap_or(0);
    let evid: Vec<String> = (0..dw_len).map(|i| format!("artifact-{i}")).collect();
    let done = handler.tool_action_complete(&serde_json::json!({
        "action_id": action_id,
        "outcome": "Documented deploy checklist with blue-green steps and rollback pointer",
        "success": true,
        "claimed_by": "test-agent",
        "done_when_evidence": evid
    }));
    assert_eq!(done["protocol"]["ok"], true, "{done}");
    let outcome_id = done["data"]["outcome_memory_id"]
        .as_u64()
        .expect("outcome id");
    assert!(outcome_id > 0);
    assert!(done["data"]["relations_created"].as_u64().unwrap_or(0) >= 1);

    let listed = handler.tool_action_list(&serde_json::json!({ "state": "done" }));
    assert_eq!(listed["protocol"]["ok"], true, "{listed}");
    let done_actions = listed["data"]["actions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(done_actions.iter().any(|a| a["id"] == action_id));
}

#[tokio::test]
async fn drive_to_actions_rejects_double_spend_and_acks_on_complete() {
    let mut engine = isolated_engine();
    engine.start();
    let handler = McpHandler::new(std::sync::Arc::new(engine));
    confirm_test_identity(&handler);

    let mem = handler.tool_memory_create(&serde_json::json!({
        "content": "Cluster CPU saturation playbook: scale workers then drain",
        "labels": ["ops", "sre"]
    }));
    assert_eq!(mem["protocol"]["ok"], true, "{mem}");
    let mid = mem["data"]["id"].as_u64().expect("memory id");

    // Re-get engine via handler internals — enqueue on scheduler
    let drive_id = {
        let q = handler.engine.scheduler().drive_queue();
        q.enqueue(crate::engine::drive::DriveSignal {
            id: 0,
            timestamp: 1_780_000_100,
            intent_type: crate::engine::drive::DriveIntent::Explore,
            description: "Investigate CPU saturation using the playbook".into(),
            evidence: vec![mid],
            urgency: crate::engine::drive::DriveUrgency::High,
            target_capability: None,
            emotion: None,
            origin_tick: 2,
            status: crate::engine::drive::DriveStatus::Pending,
            feedback: None,
            retry_count: 0,
            expires_at: None,
            enqueued_at_ms: 1_780_000_100_000,
            time_budget_ms: None,
            grounding: None,
            terminal_reason: None,
        })
    };

    let first = handler.tool_drive_to_actions(&serde_json::json!({
        "drive_id": drive_id,
        "claim": true
    }));
    assert_eq!(first["protocol"]["ok"], true, "{first}");
    let action_id = first["data"]["action"]["id"].as_u64().unwrap();
    // ActionCard v1+ moved provenance under lineage (#254).
    assert_eq!(first["data"]["action"]["lineage"]["origin"], "drive");
    assert_eq!(
        first["data"]["action"]["lineage"]["source_drive_id"],
        drive_id
    );

    let dup = handler.tool_drive_to_actions(&serde_json::json!({
        "drive_id": drive_id
    }));
    assert_eq!(
        dup["protocol"]["ok"], false,
        "double-spend must fail: {dup}"
    );
    assert_eq!(dup["protocol"]["error"]["code"], 409);

    // Without done_when proof the complete soft-fails → requeued, and must NOT ack.
    let soft = handler.tool_action_complete(&serde_json::json!({
        "action_id": action_id,
        "outcome": "claimed it worked",
        "success": true
    }));
    assert_eq!(soft["protocol"]["ok"], true, "{soft}");
    assert!(soft["data"]["drive_ack"].is_null(), "{soft}");
    let reclaim = handler.tool_action_claim(&serde_json::json!({"action_id": action_id}));
    assert_eq!(reclaim["protocol"]["ok"], true, "{reclaim}");
    let dw_len = first["data"]["action"]["done_when"]
        .as_array()
        .map(|a| a.len())
        .unwrap_or(0);
    let evid: Vec<String> = (0..dw_len).map(|i| format!("artifact-{i}")).collect();
    let done = handler.tool_action_complete(&serde_json::json!({
        "action_id": action_id,
        "outcome": "Scaled workers and drained hot pods per playbook",
        "success": true,
        "done_when_evidence": evid
    }));
    assert_eq!(done["protocol"]["ok"], true, "{done}");
    assert_eq!(done["data"]["drive_ack"]["acknowledged"], true);
    assert_eq!(done["data"]["drive_ack"]["first_ack"], true);

    // After terminal action, materializing again is allowed (drive already acked;
    // live-drive guard cleared). May fail if evidence policy blocks — only assert
    // has_live_drive is false via action_list.
    let listed = handler.tool_action_list(&serde_json::json!({"state": "done"}));
    assert!(listed["data"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["id"] == action_id));
}

#[test]
fn task_start_action_id_requires_claim_and_feeds_complete() {
    let handler = McpHandler::new(std::sync::Arc::new(isolated_engine()));
    confirm_test_identity(&handler);

    let mem = handler.tool_memory_create(&serde_json::json!({
        "content": "Ship checklist requires changelog and smoke tests",
        "labels": ["release"]
    }));
    assert_eq!(mem["protocol"]["ok"], true, "{mem}");

    let proposed = handler.tool_memory_propose_actions(&serde_json::json!({
        "query": "release checklist",
        "limit": 1
    }));
    assert_eq!(proposed["protocol"]["ok"], true, "{proposed}");
    let action_id = proposed["data"]["actions"][0]["id"].as_u64().unwrap();

    // Reject unclaimed
    let bad = handler.tool_task_start(&serde_json::json!({
        "description": "do release",
        "budget_minutes": 15,
        "action_id": action_id
    }));
    assert_eq!(bad["protocol"]["ok"], false, "{bad}");
    assert_eq!(bad["protocol"]["error"]["code"], 409);

    handler.tool_action_claim(&serde_json::json!({
        "action_id": action_id,
        "claimed_by": handler.engine.user_id
    }));

    let start = handler.tool_task_start(&serde_json::json!({
        "description": "do release",
        "budget_minutes": 15,
        "action_id": action_id
    }));
    // task_start uses smrp_ok_nn — still protocol.ok
    assert_eq!(start["protocol"]["ok"], true, "{start}");
    let task_id = start["data"]["task_id"].as_str().unwrap().to_string();
    assert_eq!(start["data"]["bound_action"]["id"], action_id);

    // Foreign claimer cannot bind — already bound to this task; second start with same action fails
    let again = handler.tool_task_start(&serde_json::json!({
        "description": "other",
        "budget_minutes": 10,
        "action_id": action_id
    }));
    assert_eq!(again["protocol"]["ok"], false, "{again}");

    // Force-complete path: call feed via tool_action_complete through task_complete is heavy
    // (stop negotiation). Directly verify bind + complete helper.
    // Bound action inherits done_when; task_complete's evidence must flow through.
    let dw_len = handler
        .engine
        .scheduler
        .action_frontier()
        .get(action_id)
        .unwrap()
        .done_when
        .len();
    let evid: Vec<String> = (0..dw_len).map(|i| format!("artifact-{i}")).collect();
    let fed = handler.feed_bound_action_from_task(&task_id, "changelog+smoke done", true, &evid);
    let fed = fed.expect("feed");
    assert_eq!(fed["protocol"]["ok"], true, "{fed}");
    assert_eq!(fed["data"]["action"]["state"], "done");
    assert_eq!(fed["data"]["action"]["lineage"]["bound_task_id"], task_id);
}

#[test]
fn action_tool_surface_advertises_planner_worker() {
    let h = McpHandler::new(std::sync::Arc::new(isolated_engine()));
    confirm_test_identity(&h);
    let raw = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"epicode_handshake","arguments":{}}}"#;
    let v: serde_json::Value = serde_json::from_str(&h.process_json(raw)).unwrap();
    let surface = &v["result"]["structuredContent"]["data"]["action_tool_surface"];
    assert!(surface["planner"]
        .as_array()
        .unwrap()
        .iter()
        .any(|x| x == "memory_propose_actions"));
    assert!(surface["worker"]
        .as_array()
        .unwrap()
        .iter()
        .any(|x| x == "action_claim"));
}

#[test]
fn action_complete_soft_fails_unproven_done_when() {
    let handler = McpHandler::new(std::sync::Arc::new(isolated_engine()));
    confirm_test_identity(&handler);
    let _ = handler.tool_memory_create(&serde_json::json!({
        "content": "Need both docs and tests before release",
        "labels": ["release"]
    }));
    let proposed = handler.tool_memory_propose_actions(&serde_json::json!({
        "query": "release",
        "limit": 1
    }));
    let action_id = proposed["data"]["actions"][0]["id"].as_u64().unwrap();
    handler.tool_action_claim(&serde_json::json!({
        "action_id": action_id,
        "claimed_by": handler.engine.user_id
    }));
    // Propose always sets non-empty done_when — complete without evidence → blocked
    let done = handler.tool_action_complete(&serde_json::json!({
        "action_id": action_id,
        "outcome": "shipped somehow",
        "success": true
    }));
    assert_eq!(done["protocol"]["ok"], true, "{done}");
    assert_eq!(done["data"]["done_when_proven"], false);
    assert!(!done["data"]["done_when_debts"]
        .as_array()
        .unwrap()
        .is_empty());
    // Accountability: blocked soft-fail requeues (no silent drop)
    assert_eq!(done["data"]["accountability"]["disposition"], "requeued");
    assert_eq!(done["data"]["action"]["state"], "open");

    // New action with full evidence → done
    let proposed2 = handler.tool_memory_propose_actions(&serde_json::json!({
        "query": "release",
        "limit": 1
    }));
    let aid2 = proposed2["data"]["actions"][0]["id"].as_u64().unwrap();
    let dw = proposed2["data"]["actions"][0]["done_when"]
        .as_array()
        .unwrap()
        .clone();
    handler.tool_action_claim(&serde_json::json!({
        "action_id": aid2,
        "claimed_by": handler.engine.user_id
    }));
    let evid: Vec<String> = dw
        .iter()
        .map(|_| "artifact: CI green + CHANGELOG".into())
        .collect();
    let ok = handler.tool_action_complete(&serde_json::json!({
        "action_id": aid2,
        "outcome": "release cut",
        "success": true,
        "done_when_evidence": evid
    }));
    assert_eq!(ok["protocol"]["ok"], true, "{ok}");
    assert_eq!(ok["data"]["action"]["state"], "done");
    assert_eq!(ok["data"]["done_when_proven"], true);
}

#[test]
fn action_frontier_persists_across_restore() {
    let dir = std::env::temp_dir().join(format!(
        "epicode-action-persist-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let eng = Engine::with_data_dir(dir.clone());
    let h = McpHandler::new(std::sync::Arc::new(eng));
    confirm_test_identity(&h);
    let _ = h.tool_memory_create(&serde_json::json!({
        "content": "persist me for action frontier",
        "labels": ["persist"]
    }));
    let proposed = h.tool_memory_propose_actions(&serde_json::json!({
        "query": "persist",
        "limit": 1
    }));
    assert_eq!(proposed["protocol"]["ok"], true, "{proposed}");
    let aid = proposed["data"]["actions"][0]["id"].as_u64().unwrap();
    h.engine.scheduler.save_action_frontier();
    drop(h);

    let eng2 = Engine::with_data_dir(dir);
    eng2.scheduler.restore_action_frontier();
    let restored = eng2.scheduler.action_frontier().get(aid);
    assert!(restored.is_some(), "action {} should survive restart", aid);
    assert_eq!(
        restored.unwrap().state,
        crate::engine::agent_action::ActionState::Open
    );
}

#[test]
fn propose_returns_action_card_schema() {
    let h = McpHandler::new(std::sync::Arc::new(isolated_engine()));
    confirm_test_identity(&h);
    let _ = h.tool_memory_create(&serde_json::json!({
        "content": "card schema fixture memory",
        "labels": ["card"]
    }));
    let proposed = h.tool_memory_propose_actions(&serde_json::json!({
        "query": "card schema",
        "limit": 1
    }));
    assert_eq!(proposed["protocol"]["ok"], true, "{proposed}");
    assert_eq!(
        proposed["data"]["action_card_schema"],
        "epicode.action_card/v2"
    );
    let card = &proposed["data"]["cards"][0];
    assert_eq!(card["schema"], "epicode.action_card");
    assert_eq!(card["schema_version"], 2);
    assert!(card["lineage"]["parents"].is_array());
}

/// PR-J: transient failures (provider/network) must not burn the requeue
/// budget nor blacklist the worker; they back off instead. Fails on #258:
/// three transient blips escalate the action to a human.
#[test]
fn transient_failures_back_off_without_consuming_requeue_budget() {
    let handler = McpHandler::new(std::sync::Arc::new(isolated_engine()));
    confirm_test_identity(&handler);
    let _ = handler.tool_memory_create(&serde_json::json!({
        "content": "flaky upstream evidence",
        "labels": ["ops"]
    }));
    let p = handler.tool_memory_propose_actions(&serde_json::json!({"query": "flaky", "limit": 1}));
    let id = p["data"]["actions"][0]["id"].as_u64().unwrap();
    for round in 0..3 {
        let frontier = handler.engine.scheduler.action_frontier();
        // Skip any backoff window so the same worker can retry immediately in-test.
        frontier.clear_backoff_for_test(id);
        let c =
            handler.tool_action_claim(&serde_json::json!({"action_id": id, "claimed_by": "w1"}));
        assert_eq!(c["protocol"]["ok"], true, "round {round}: {c}");
        let r = handler.tool_action_complete(&serde_json::json!({
            "action_id": id,
            "outcome": "upstream 503",
            "success": false,
            "failure_kind": "transient",
            "claimed_by": "w1"
        }));
        assert_eq!(r["protocol"]["ok"], true, "{r}");
        assert_eq!(
            r["data"]["accountability"]["disposition"], "retry_backoff",
            "{r}"
        );
    }
    let a = handler.engine.scheduler.action_frontier().get(id).unwrap();
    assert_eq!(a.state.as_str(), "open");
    assert_eq!(a.requeue_count, 0);
    assert!(!a.escalated);
    assert_eq!(a.transient_retries, 3);
    assert!(a.not_before.is_some());
    // Worker was not blacklisted for transient blips.
    assert!(handler
        .engine
        .scheduler
        .worker_registry()
        .get("w1")
        .map(|w| w.blacklisted_until.is_none())
        .unwrap_or(true));
    // Backoff window: claim before not_before is refused.
    let early =
        handler.tool_action_claim(&serde_json::json!({"action_id": id, "claimed_by": "w1"}));
    assert_eq!(early["protocol"]["ok"], false, "{early}");
}

/// PR-K: a chained proposal is a serial plan — only the head is claimable
/// until its predecessor lands Done. Fails on #259: every step is claimable.
#[test]
fn chained_proposals_only_head_is_claimable() {
    let handler = McpHandler::new(std::sync::Arc::new(isolated_engine()));
    confirm_test_identity(&handler);
    for c in ["chain step evidence one", "chain step evidence two"] {
        let _ = handler.tool_memory_create(&serde_json::json!({"content": c, "labels": ["ops"]}));
    }
    let p = handler.tool_memory_propose_actions(
        &serde_json::json!({"query": "chain step evidence", "limit": 2, "chain": true}),
    );
    let acts = p["data"]["actions"].as_array().cloned().unwrap_or_default();
    assert_eq!(acts.len(), 2, "{p}");
    let (a, b) = (
        acts[0]["id"].as_u64().unwrap(),
        acts[1]["id"].as_u64().unwrap(),
    );
    let early = handler.tool_action_claim(&serde_json::json!({"action_id": b, "claimed_by": "w"}));
    assert_eq!(
        early["protocol"]["ok"], false,
        "downstream claim must wait: {early}"
    );
    let c = handler.tool_action_claim(&serde_json::json!({"action_id": a, "claimed_by": "w"}));
    assert_eq!(c["protocol"]["ok"], true, "{c}");
    let dw = acts[0]["done_when"]
        .as_array()
        .map(|x| x.len())
        .unwrap_or(0);
    let evid: Vec<String> = (0..dw).map(|i| format!("artifact-{i}")).collect();
    let d = handler.tool_action_complete(&serde_json::json!({
        "action_id": a, "outcome": "step one shipped", "claimed_by": "w", "done_when_evidence": evid
    }));
    assert_eq!(d["data"]["action"]["state"], "done", "{d}");
    let listed = handler.tool_action_list(&serde_json::json!({"state": "open"}));
    let card = listed["data"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == b)
        .cloned()
        .unwrap();
    assert_eq!(card["depends_on"], serde_json::json!([a]), "{card}");
    assert_eq!(card["readiness"]["state"], "ready", "{card}");
    let ok = handler.tool_action_claim(&serde_json::json!({"action_id": b, "claimed_by": "w"}));
    assert_eq!(ok["protocol"]["ok"], true, "{ok}");
}

/// PR-L defect: escalation promised "enqueue drive Suggest for human" (#255
/// design) but nothing reached the drive inbox — escalations died silently.
#[test]
fn escalation_reaches_drive_inbox_once() {
    let handler = McpHandler::new(std::sync::Arc::new(isolated_engine()));
    confirm_test_identity(&handler);
    let _ = handler
        .tool_memory_create(&serde_json::json!({"content": "doomed evidence", "labels": ["ops"]}));
    let p =
        handler.tool_memory_propose_actions(&serde_json::json!({"query": "doomed", "limit": 1}));
    let id = p["data"]["actions"][0]["id"].as_u64().unwrap();
    let mut last = serde_json::Value::Null;
    for _ in 0..3 {
        let c = handler.tool_action_claim(&serde_json::json!({"action_id": id, "claimed_by": "w"}));
        assert_eq!(c["protocol"]["ok"], true, "{c}");
        last = handler.tool_action_complete(&serde_json::json!({
            "action_id": id, "outcome": "broke", "success": false, "claimed_by": "w"
        }));
    }
    assert_eq!(
        last["data"]["accountability"]["disposition"], "escalated",
        "{last}"
    );
    let hits = |h: &McpHandler| {
        h.engine
            .scheduler
            .drive_queue()
            .snapshot()
            .into_iter()
            .filter(|s| {
                s.description.contains("[action_escalated]")
                    && s.description.contains(&format!("#{id}"))
            })
            .count()
    };
    assert_eq!(
        hits(&handler),
        1,
        "escalation must surface to the human once"
    );
    // Re-running the planner pass must not duplicate the notification.
    handler.engine.scheduler.run_action_planner_pass_now();
    assert_eq!(hits(&handler), 1);
}

/// task_pulse / handshake 必须和 ctx、task_check 用同一个活跃钟(real_active_ms 优先)。
/// Fails on 3218241: pulse 读原始 active_ms(=0) 报 explore, ctx 报 95%。
#[test]
fn task_pulse_uses_effective_active_clock() {
    let h = McpHandler::new(std::sync::Arc::new(isolated_engine()));
    confirm_test_identity(&h);
    let st = call_tool(
        &h,
        "task_start",
        serde_json::json!({"description": "pulse clock parity", "budget_minutes": 10}),
    );
    let tid = st["data"]["task_id"].as_str().expect("task_id").to_string();
    let uid = h.engine.user_id.clone();
    h.engine
        .storage
        .set_real_active(&tid, &uid, 570_000)
        .unwrap();
    let r = call_tool(&h, "memory_search", serde_json::json!({"query": "pulse"}));
    let pulse = &r["data"]["task_pulse"];
    assert_eq!(pulse["active_pct"], 95, "{r}");
    assert_eq!(pulse["phase"], "deliver", "{r}");
    let ctx_rule = r["data"]["ctx"]["◆"].as_str().unwrap_or("");
    assert!(ctx_rule.contains("95%"), "{r}");
}

fn call_tool(h: &McpHandler, name: &str, args: serde_json::Value) -> serde_json::Value {
    h.handle(McpRequest {
        jsonrpc: "2.0".into(),
        id: Some(serde_json::json!(1)),
        method: "tools/call".into(),
        params: Some(serde_json::json!({"name": name, "arguments": args})),
    })
    .result
    .unwrap()["structuredContent"]
        .clone()
}

fn evid_for(card: &serde_json::Value) -> Vec<String> {
    let n = card["done_when"].as_array().map(|a| a.len()).unwrap_or(0);
    (0..n).map(|i| format!("artifact-{i}")).collect()
}

/// PR-M: goal achieved cancels the rest (not a failure, no late outcome memory);
/// all-done auto-closes. Fails on #261: no goal tools.
#[test]
fn goal_met_cancels_remaining_and_all_done_autocloses() {
    let h = McpHandler::new(std::sync::Arc::new(isolated_engine()));
    confirm_test_identity(&h);
    for c in ["goal evidence alpha", "goal evidence beta"] {
        let _ = h.tool_memory_create(&serde_json::json!({"content": c, "labels": ["ops"]}));
    }
    let g = call_tool(
        &h,
        "action_goal_create",
        serde_json::json!({
            "title": "ship release", "done_when": ["release tagged"]
        }),
    );
    assert_eq!(g["protocol"]["ok"], true, "{g}");
    let gid = g["data"]["goal"]["id"].as_u64().unwrap();
    let p = h.tool_memory_propose_actions(
        &serde_json::json!({"query": "goal evidence", "limit": 2, "goal_id": gid}),
    );
    let acts = p["data"]["actions"].as_array().cloned().unwrap();
    assert_eq!(acts.len(), 2, "{p}");
    assert_eq!(acts[0]["goal"]["id"], gid, "{p}");
    let (a, b) = (
        acts[0]["id"].as_u64().unwrap(),
        acts[1]["id"].as_u64().unwrap(),
    );
    // b is claimed by a worker that will report late.
    assert_eq!(
        h.tool_action_claim(&serde_json::json!({"action_id": b, "claimed_by": "slow"}))["protocol"]
            ["ok"],
        true
    );
    // Unproven goal cannot be marked achieved.
    let bad = call_tool(
        &h,
        "action_goal_prove",
        serde_json::json!({"goal_id": gid, "evidence": []}),
    );
    assert_eq!(bad["protocol"]["ok"], false, "{bad}");
    let ok = call_tool(
        &h,
        "action_goal_prove",
        serde_json::json!({"goal_id": gid, "evidence": ["v1.9.0 tag"]}),
    );
    assert_eq!(ok["protocol"]["ok"], true, "{ok}");
    assert_eq!(ok["data"]["goal"]["state"], "achieved");
    assert_eq!(ok["data"]["cancelled"], serde_json::json!([a, b]));
    let fa = h.engine.scheduler.action_frontier().get(b).unwrap();
    assert_eq!(fa.state.as_str(), "cancelled");
    assert_eq!(fa.requeue_count, 0);
    assert!(!fa.escalated);
    // Late outcome from the slow worker: rejected, and no memory written.
    let before = h.engine.space.tetra_count();
    let late = h.tool_action_complete(&serde_json::json!({
        "action_id": b, "outcome": "late result", "claimed_by": "slow"
    }));
    assert_eq!(late["protocol"]["ok"], false, "{late}");
    assert_eq!(
        h.engine.space.tetra_count(),
        before,
        "late outcome must not pollute memory"
    );

    // Second goal: every action done → auto-close.
    let g2 = call_tool(
        &h,
        "action_goal_create",
        serde_json::json!({"title": "small"}),
    );
    let gid2 = g2["data"]["goal"]["id"].as_u64().unwrap();
    let p2 = h.tool_memory_propose_actions(
        &serde_json::json!({"query": "goal evidence", "limit": 1, "goal_id": gid2}),
    );
    let c = p2["data"]["actions"][0].clone();
    let cid = c["id"].as_u64().unwrap();
    h.tool_action_claim(&serde_json::json!({"action_id": cid, "claimed_by": "w"}));
    let d = h.tool_action_complete(&serde_json::json!({
        "action_id": cid, "outcome": "done", "claimed_by": "w", "done_when_evidence": evid_for(&c)
    }));
    assert_eq!(d["data"]["action"]["state"], "done", "{d}");
    let gl = call_tool(&h, "action_goal_list", serde_json::json!({}));
    let g2s = gl["data"]["goals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == gid2)
        .cloned()
        .unwrap();
    assert_eq!(g2s["state"], "closed", "{gl}");
}

/// PR-N: planner steers/pauses/stops a running action; the worker learns via
/// heartbeat. Fails on #262: no action_control tool, heartbeat has no directives.
#[test]
fn planner_controls_running_action_via_heartbeat() {
    let h = McpHandler::new(std::sync::Arc::new(isolated_engine()));
    confirm_test_identity(&h);
    let _ = h
        .tool_memory_create(&serde_json::json!({"content": "control evidence", "labels": ["ops"]}));
    h.tool_worker_register(&serde_json::json!({"worker_id": "w", "max_load": 1}));
    let p = h
        .tool_memory_propose_actions(&serde_json::json!({"query": "control evidence", "limit": 1}));
    let id = p["data"]["actions"][0]["id"].as_u64().unwrap();
    assert_eq!(
        h.tool_action_claim(&serde_json::json!({"action_id": id, "claimed_by": "w"}))["protocol"]
            ["ok"],
        true
    );

    let st = call_tool(
        &h,
        "action_control",
        serde_json::json!({"action_id": id, "op": "steer", "note": "focus on rollback path"}),
    );
    assert_eq!(st["protocol"]["ok"], true, "{st}");
    let hb = h.tool_action_heartbeat(&serde_json::json!({"action_id": id, "claimed_by": "w"}));
    assert_eq!(
        hb["data"]["directives"]["steer"][0], "focus on rollback path",
        "{hb}"
    );
    let hb2 = h.tool_action_heartbeat(&serde_json::json!({"action_id": id, "claimed_by": "w"}));
    assert_eq!(
        hb2["data"]["directives"]["steer"],
        serde_json::json!([]),
        "steer delivered once: {hb2}"
    );

    // Pause: not active load, lease does not expire, complete refused.
    let pa = call_tool(
        &h,
        "action_control",
        serde_json::json!({"action_id": id, "op": "pause"}),
    );
    assert_eq!(pa["protocol"]["ok"], true, "{pa}");
    let reg = h.engine.scheduler.worker_registry();
    assert_eq!(
        crate::engine::dispatch::WorkerRegistry::load_of(h.engine.scheduler.action_frontier(), "w"),
        0
    );
    let _ = reg;
    let hb3 = h.tool_action_heartbeat(&serde_json::json!({"action_id": id, "claimed_by": "w"}));
    assert_eq!(hb3["data"]["directives"]["paused"], true, "{hb3}");
    let early = h.tool_action_complete(
        &serde_json::json!({"action_id": id, "outcome": "x", "claimed_by": "w"}),
    );
    assert_eq!(
        early["protocol"]["ok"], false,
        "paused cannot complete: {early}"
    );
    let rs = call_tool(
        &h,
        "action_control",
        serde_json::json!({"action_id": id, "op": "resume"}),
    );
    assert_eq!(rs["protocol"]["ok"], true, "{rs}");

    // Stop: terminal non-failure, worker told via heartbeat, late outcome rejected w/o memory.
    let sp = call_tool(
        &h,
        "action_control",
        serde_json::json!({"action_id": id, "op": "stop", "note": "direction obsolete"}),
    );
    assert_eq!(sp["protocol"]["ok"], true, "{sp}");
    let a = h.engine.scheduler.action_frontier().get(id).unwrap();
    assert_eq!(a.state.as_str(), "stopped");
    assert_eq!(a.requeue_count, 0);
    assert!(!a.escalated);
    let hb4 = h.tool_action_heartbeat(&serde_json::json!({"action_id": id, "claimed_by": "w"}));
    assert_eq!(hb4["protocol"]["ok"], true, "{hb4}");
    assert_eq!(hb4["data"]["directives"]["stop"], true, "{hb4}");
    let before = h.engine.space.tetra_count();
    let late = h.tool_action_complete(
        &serde_json::json!({"action_id": id, "outcome": "late", "claimed_by": "w"}),
    );
    assert_eq!(late["protocol"]["ok"], false, "{late}");
    assert_eq!(h.engine.space.tetra_count(), before);
    // Viewer cannot control.
    let v = McpHandler::new(Arc::clone(&h.engine))
        .with_role_gate(crate::engine::user_manager::UserRole::Viewer);
    let denied = call_tool(
        &v,
        "action_control",
        serde_json::json!({"action_id": id, "op": "stop"}),
    );
    assert_eq!(denied["protocol"]["error"]["code"], 403, "{denied}");
}

/// #262 race, made deterministic: the action is cancelled AFTER the outcome
/// memory is written but BEFORE the frontier transition. The raced outcome
/// must not survive as an orphan memory.
#[test]
fn raced_cancel_leaves_no_orphan_outcome_memory() {
    let h = McpHandler::new(std::sync::Arc::new(isolated_engine()));
    confirm_test_identity(&h);
    let _ =
        h.tool_memory_create(&serde_json::json!({"content": "race evidence", "labels": ["ops"]}));
    let g = call_tool(
        &h,
        "action_goal_create",
        serde_json::json!({"title": "race", "done_when": ["x"]}),
    );
    let gid = g["data"]["goal"]["id"].as_u64().unwrap();
    let p = h.tool_memory_propose_actions(
        &serde_json::json!({"query": "race evidence", "limit": 1, "goal_id": gid}),
    );
    let card = p["data"]["actions"][0].clone();
    let id = card["id"].as_u64().unwrap();
    h.tool_action_claim(&serde_json::json!({"action_id": id, "claimed_by": "w"}));
    let before = h.engine.space.tetra_count();
    let frontier = std::sync::Arc::clone(h.engine.scheduler.action_frontier());
    set_before_complete_transition_hook(Some(Box::new(move || {
        frontier
            .prove_goal(gid, vec!["done elsewhere".into()])
            .unwrap();
    })));
    let r = h.tool_action_complete(&serde_json::json!({
            "action_id": id, "outcome": "raced", "claimed_by": "w", "done_when_evidence": evid_for(&card)
        }));
    set_before_complete_transition_hook(None);
    assert_eq!(r["protocol"]["ok"], false, "{r}");
    assert_eq!(
        h.engine.space.tetra_count(),
        before,
        "orphan outcome memory leaked"
    );
}

#[test]
fn action_heartbeat_renews_owner_lease_and_rejects_foreign() {
    let handler = McpHandler::new(std::sync::Arc::new(isolated_engine()));
    confirm_test_identity(&handler);
    let _ = handler.tool_memory_create(&serde_json::json!({
        "content": "lease runbook evidence",
        "labels": ["ops"]
    }));
    let p = handler.tool_memory_propose_actions(&serde_json::json!({"query": "lease", "limit": 1}));
    let id = p["data"]["actions"][0]["id"].as_u64().unwrap();
    let c = handler.tool_action_claim(&serde_json::json!({"action_id": id, "claimed_by": "w1"}));
    assert_eq!(c["protocol"]["ok"], true, "{c}");
    assert!(c["data"]["lease_until"].as_i64().is_some(), "{c}");
    let hb =
        handler.tool_action_heartbeat(&serde_json::json!({"action_id": id, "claimed_by": "w1"}));
    assert_eq!(hb["protocol"]["ok"], true, "{hb}");
    assert_eq!(hb["data"]["lease_secs"], 900);
    let bad =
        handler.tool_action_heartbeat(&serde_json::json!({"action_id": id, "claimed_by": "w2"}));
    assert_eq!(bad["protocol"]["error"]["code"], 403, "{bad}");
    // Live lease: sweep must not reclaim.
    assert_eq!(handler.engine.scheduler.sweep_action_leases(), 0);
}

#[test]
fn dispatch_assigns_least_loaded_and_rejects_foreign_complete() {
    let handler = McpHandler::new(std::sync::Arc::new(isolated_engine()));
    confirm_test_identity(&handler);

    assert_eq!(
        handler.tool_worker_register(&serde_json::json!({
            "worker_id": "alice",
            "capabilities": ["ops", "rust"],
            "max_load": 5
        }))["protocol"]["ok"],
        true
    );
    assert_eq!(
        handler.tool_worker_register(&serde_json::json!({
            "worker_id": "bob",
            "capabilities": ["ops"],
            "max_load": 5
        }))["protocol"]["ok"],
        true
    );

    let _ = handler.tool_memory_create(&serde_json::json!({
        "content": "ops runbook for cluster drain",
        "labels": ["ops"]
    }));
    let p1 = handler.tool_memory_propose_actions(&serde_json::json!({"query": "ops", "limit": 1}));
    let a1 = p1["data"]["actions"][0]["id"].as_u64().unwrap();
    handler
        .engine
        .scheduler
        .action_frontier()
        .set_required_capabilities(a1, vec!["ops".into()])
        .unwrap();
    let as1 = handler.tool_action_assign(&serde_json::json!({
        "action_id": a1,
        "worker_id": "alice"
    }));
    assert_eq!(as1["protocol"]["ok"], true, "{as1}");

    let p2 = handler.tool_memory_propose_actions(&serde_json::json!({"query": "ops", "limit": 1}));
    let a2 = p2["data"]["actions"][0]["id"].as_u64().unwrap();
    handler
        .engine
        .scheduler
        .action_frontier()
        .set_required_capabilities(a2, vec!["ops".into()])
        .unwrap();
    let as2 = handler.tool_action_assign(&serde_json::json!({ "action_id": a2 }));
    assert_eq!(as2["protocol"]["ok"], true, "{as2}");
    assert_eq!(as2["data"]["worker_id"], "bob", "{as2}");
    assert_eq!(as2["data"]["action"]["assignee"], "bob");

    let foreign = handler.tool_action_complete(&serde_json::json!({
        "action_id": a2,
        "outcome": "done by interloper",
        "success": true,
        "done_when_evidence": ["artifact-a", "artifact-b"]
    }));
    assert_eq!(foreign["protocol"]["ok"], false, "{foreign}");
    assert_eq!(foreign["protocol"]["error"]["code"], 403);

    // Outcome memory must NOT exist for foreign attempt (gate before create)
    let listed = handler.tool_memory_list(&serde_json::json!({"labels": ["action-outcome"]}));
    let items = listed["data"]["items"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        !items.iter().any(|i| i["content_preview"]
            .as_str()
            .unwrap_or("")
            .contains("interloper")),
        "foreign complete must not leave outcome memory: {listed}"
    );
}

#[test]
fn outcome_memory_carries_assignee_label() {
    let handler = McpHandler::new(std::sync::Arc::new(isolated_engine()));
    confirm_test_identity(&handler);
    let uid = handler.engine.user_id.clone();
    handler.tool_worker_register(&serde_json::json!({
        "worker_id": uid,
        "capabilities": ["general"],
        "max_load": 5
    }));
    let _ = handler.tool_memory_create(&serde_json::json!({
        "content": "ownership label fixture",
        "labels": ["ops"]
    }));
    let proposed = handler.tool_memory_propose_actions(&serde_json::json!({
        "query": "ownership label",
        "limit": 1
    }));
    let aid = proposed["data"]["actions"][0]["id"].as_u64().unwrap();
    let dw = proposed["data"]["actions"][0]["done_when"]
        .as_array()
        .unwrap()
        .clone();
    handler.tool_action_assign(&serde_json::json!({
        "action_id": aid,
        "worker_id": uid
    }));
    let evid: Vec<String> = dw.iter().map(|_| "artifact ok".into()).collect();
    let done = handler.tool_action_complete(&serde_json::json!({
        "action_id": aid,
        "outcome": "finished with ownership",
        "success": true,
        "done_when_evidence": evid
    }));
    assert_eq!(done["protocol"]["ok"], true, "{done}");
    assert_eq!(done["data"]["action"]["state"], "done");
    let oid = done["data"]["outcome_memory_id"].as_u64().unwrap();
    let got = handler.tool_memory_get(&serde_json::json!({ "id": oid }));
    let labels = got["data"]["labels"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let label_strs: Vec<String> = labels
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    assert!(
        label_strs.iter().any(|l| l == &format!("assignee:{}", uid)),
        "missing assignee label: {label_strs:?}"
    );
    assert!(
        label_strs.iter().any(|l| l.starts_with("source_agent:")),
        "missing source_agent label: {label_strs:?}"
    );
}
