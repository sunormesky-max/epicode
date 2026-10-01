//! Local-only audit reproductions. Uses disposable accounts/data and loopback TCP.
//! Run with: cargo run --locked --offline --features ort/load-dynamic --example audit_probe
// A11/A09 复现需要真实 tcp.rs。tcp.rs 自 #133 起引用 super::mcp_endpoint /
// super::state(共享 guard/persona 函数) — example 上下文无这些兄弟模块,
// 故在此提供最小桩(模块解析需要; 复现路径不走 persona 分支)。
mod state {
    use std::sync::Arc;
    #[allow(dead_code)]
    #[derive(Clone)]
    pub struct CloudState {
        pub user_mgr: Arc<epicode::engine::user_manager::UserManager>,
        pub pub_skills: Arc<epicode::engine::skills::SkillEngine>,
    }
}
mod mcp_endpoint {
    use super::state::CloudState;
    use epicode::engine::Engine;
    use std::sync::Arc;

    /// 桩镜像(与 cloud::mcp_endpoint 的 pub(super) 结构签名一致, tcp.rs 编译需要)
    pub struct McpGateRejection {
        pub response: serde_json::Value,
    }
    pub fn persona_readiness_response(
        id: Option<serde_json::Value>,
        code: &str,
    ) -> serde_json::Value {
        serde_json::json!({
            "jsonrpc": "2.0", "id": id,
            "result": { "protocol": { "ok": false, "error": { "code": code, "message": "persona stub (audit_probe)" } } }
        })
    }
    pub fn guard_mcp_request(
        _state: &CloudState,
        _user_id: &str,
        _engine: &Engine,
        _request: &serde_json::Value,
    ) -> Option<McpGateRejection> {
        None
    }
    pub fn start_cognitive_loop_if_needed(
        _state: &CloudState,
        _user_id: &str,
        _engine: Arc<Engine>,
    ) {
    }
}

#[path = "../src/bin/cloud/tcp.rs"]
mod cloud_tcp;

use epicode::engine::user_manager::{Permission, UserManager, UserPlan, UserRole};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

fn send(reader: &mut BufReader<TcpStream>, request: Value) -> Value {
    writeln!(reader.get_mut(), "{}", request).unwrap();
    reader.get_mut().flush().unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    serde_json::from_str(&line).expect("JSON-RPC response")
}

fn tool_data(response: &Value) -> Value {
    serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .expect("tool text"),
    )
    .unwrap()
}

#[tokio::main]
async fn main() {
    // No production credentials and no external LLM calls.
    std::env::set_var("LLM_API_KEY", "");
    std::env::set_var("DEEPSEEK_API_KEY", "");
    let base = std::env::temp_dir().join(format!("epicode-audit-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&base).unwrap();
    let mgr = Arc::new(UserManager::new(&base));
    mgr.register(
        "audit-owner",
        "local-audit-owner-key",
        UserPlan::Pro,
        "local-audit-password",
    )
    .unwrap();
    let viewer = mgr
        .create_subaccount(
            "audit-owner",
            "audit-viewer",
            "local-audit-password",
            UserRole::Viewer,
        )
        .unwrap();
    assert!(!viewer.role.can(Permission::MemoryWrite));
    let engine = mgr.get_engine(&viewer.user_id).unwrap();
    engine
        .confirm_identity(
            "Audit fixture".into(),
            "Disposable audit".into(),
            "Local test".into(),
            HashMap::new(),
        )
        .unwrap();
    let (memory_id, _) = engine
        .scheduler
        .api_create_memory(
            "Local disposable audit record before update",
            vec!["audit-fixture".into()],
        )
        .unwrap();

    println!(
        "{}",
        json!({"probe":"engine_identity_without_shared_vector", "expected_user_id":viewer.user_id, "actual_user_id":engine.user_id})
    );
    let auth =
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"api_key":viewer.api_key}});
    let probe_state = state::CloudState {
        user_mgr: mgr.clone(),
        pub_skills: std::sync::Arc::new(epicode::engine::skills::SkillEngine::new(
            engine.storage.clone(),
        )),
    };
    let (_, handler) = cloud_tcp::tcp_try_authenticate(&auth.to_string(), &probe_state).unwrap();
    let update = json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"memory_update","arguments":{"id":memory_id,"content":"Local audit record modified by viewer"}}});
    let response: Value = serde_json::from_str(&handler.process_json(&update.to_string())).unwrap();
    let data = tool_data(&response);
    println!(
        "{}",
        json!({"probe":"viewer_mcp_update", "role_allows_write":false,"protocol_ok":data["protocol"]["ok"],"response":data})
    );

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let shutdown = Arc::new(AtomicBool::new(false));
    let thread_state = std::sync::Arc::new(state::CloudState {
        user_mgr: mgr.clone(),
        pub_skills: std::sync::Arc::new(epicode::engine::skills::SkillEngine::new(
            engine.storage.clone(),
        )),
    });
    let thread_shutdown = shutdown.clone();
    let runtime = tokio::runtime::Handle::current();
    let server = std::thread::spawn(move || {
        let _entered = runtime.enter();
        cloud_tcp::run_tcp_server(&address.to_string(), &thread_state, &thread_shutdown);
    });
    let stream = (0..50)
        .find_map(|_| {
            let stream = TcpStream::connect(address).ok();
            if stream.is_none() {
                std::thread::sleep(Duration::from_millis(20));
            }
            stream
        })
        .expect("loopback server ready");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut reader = BufReader::new(stream);
    assert_eq!(send(&mut reader, auth)["result"]["status"], "authenticated");
    mgr.reset_api_key(&viewer.user_id).unwrap();
    assert!(mgr.authenticate(&viewer.api_key).is_none());
    let response = send(
        &mut reader,
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"memory_get","arguments":{"id":memory_id}}}),
    );
    let data = tool_data(&response);
    println!(
        "{}",
        json!({"probe":"tcp_read_after_key_reset","old_key_valid":false,"protocol_ok":data["protocol"]["ok"],"response":data})
    );
    drop(reader);

    let mut oversized = TcpStream::connect(address).unwrap();
    oversized
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    oversized
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    oversized
        .write_all(&vec![b'x'; 1024 * 1024 + 1024])
        .unwrap();
    let mut one = [0u8; 1];
    let before_newline = match oversized.read(&mut one) {
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            ) =>
        {
            "connection_still_open"
        }
        Ok(0) => "connection_closed",
        _ => "other",
    };
    oversized.write_all(b"\n").unwrap();
    oversized
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let closed_after_newline = matches!(oversized.read(&mut one), Ok(0));
    println!(
        "{}",
        json!({"probe":"tcp_limit_before_newline","bytes_sent":1024*1024+1024,"before_newline":before_newline,"closed_after_newline":closed_after_newline})
    );
    drop(oversized);
    shutdown.store(true, Ordering::Relaxed);
    server.join().unwrap();
    println!(
        "{}",
        json!({"fixture_data_dir":base,"production_modified":false})
    );
}
