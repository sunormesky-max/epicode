//! Local-only audit reproductions. Uses disposable accounts/data and loopback TCP.
//! Run with: cargo run --locked --offline --features ort/load-dynamic --example audit_probe
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
    let (_, handler) = cloud_tcp::tcp_try_authenticate(&auth.to_string(), &mgr).unwrap();
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
    let thread_mgr = mgr.clone();
    let thread_shutdown = shutdown.clone();
    let runtime = tokio::runtime::Handle::current();
    let server = std::thread::spawn(move || {
        let _entered = runtime.enter();
        cloud_tcp::run_tcp_server(&address.to_string(), &thread_mgr, &thread_shutdown);
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
