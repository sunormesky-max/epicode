//! D2: Runtime binding — primary_executor register/unregister/status/heartbeat
//! C Spec §14.1: 多端只读, 执行写单主 primary_executor

use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;
use serde::Deserialize;
use std::collections::HashMap;

use epicode::engine::user_manager::UserInfo;
// P0 隔离修复: runtime 控制面必须用【当前用户】的 engine 填 envelope,
// 之前误用 first_engine(任意用户) 导致 status.identity 显示别人的身份

/// ── α1fix: primary binding 持久化 (重启不丢绑定关系; 激活仍靠心跳) ──
const BINDINGS_FILE: &str = "/var/lib/tetramem/runtime_bindings.json";

fn save_bindings(executors: &HashMap<String, super::state::ExecutorBinding>) {
    let path = std::path::Path::new(BINDINGS_FILE);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match serde_json::to_string_pretty(executors) {
        Ok(s) => {
            if std::fs::write(path, s).is_err() {
                tracing::warn!("[D2] bindings persist write failed");
            }
        }
        Err(e) => tracing::warn!("[D2] bindings serialize failed: {}", e),
    }
}

pub fn load_bindings() -> HashMap<String, super::state::ExecutorBinding> {
    match std::fs::read_to_string(BINDINGS_FILE) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}

fn user_engine(st: &CloudState, user_id: &str) -> Option<std::sync::Arc<epicode::engine::Engine>> {
    let slots = st.user_mgr.slots_read();
    slots.get(user_id).map(|s| s.engine.clone())
}
use super::state::CloudState;

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub agent_id: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// D3: e2e flag — default false (§14.5: e2e=false 禁 high/critical 自动执行)
    #[serde(default)]
    pub e2e_enabled: bool,
    /// α0.4: 机器指纹 (agent 自愿申报: hostname+os hash), 落 binding 锚
    #[serde(default)]
    pub machine_fingerprint: Option<String>,
    /// α0.4: 装配时读取的 manifest 协议版本
    #[serde(default)]
    pub manifest_version: Option<String>,
    /// γ2: 端侧 E2E 公钥 (PEM) — 提供则意志通道传输层加密
    #[serde(default)]
    pub e2e_public_key: Option<String>,
}

/// POST /v1/runtime/register
pub async fn register(
    State(st): State<CloudState>,
    Extension(user): Extension<UserInfo>,
    Json(req): Json<RegisterRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let now = Utc::now().timestamp();
    let caps = if req.capabilities.is_empty() {
        vec!["write".into(), "ack".into(), "forget".into()]
    } else {
        req.capabilities
    };
    let binding = super::state::ExecutorBinding {
        agent_id: req.agent_id.clone(),
        user_id: user.user_id.clone(),
        capabilities: caps.clone(),
        registered_at: now,
        last_heartbeat: now,
        e2e_enabled: req.e2e_enabled,
        e2e_public_key: req.e2e_public_key.clone(),
    };

    {
        let mut executors = st.primary_executors.write();
        if let Some(existing) = executors.get(&user.user_id) {
            if !existing.is_expired() && existing.agent_id != req.agent_id {
                return (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({
                        "success": false,
                        "error": "primary_executor already bound by another agent",
                        "current_agent": existing.agent_id,
                    })),
                );
            }
        }
        executors.insert(user.user_id.clone(), binding);
        save_bindings(&executors);
        drop(executors);
        st.user_mgr.set_has_primary_executor(&user.user_id, true);
        if let Ok(e) = st.user_mgr.get_engine_strict(&user.user_id) {
            e.scheduler.set_runtime_primary(true);
        }
        // γ2: 注入端侧 E2E 公钥到 scheduler (inbox/SSE 传输层加密用)
        if let Ok(e) = st.user_mgr.get_engine_strict(&user.user_id) {
            e.scheduler.set_e2e_pubkey(req.e2e_public_key.as_deref());
        }
    }

    let engine = user_engine(&st, &user.user_id);
    if let Some(ref e) = engine {
        // α0.4: binding 锚记忆 — 机器指纹 + manifest 版本 + 时间, 时间线保留 (unregister 不删)
        let e2e_kfp = req
            .e2e_public_key
            .as_ref()
            .map(|p| epicode::engine::crypto::compute_integrity_hash(p.as_bytes(), b"e2e-kfp"))
            .unwrap_or_else(|| "none".into());
        let audit = format!(
            "[binding-anchor] primary={} machine={} manifest={} e2e={} e2e_key={} user={} at {}",
            req.agent_id,
            req.machine_fingerprint.as_deref().unwrap_or("unreported"),
            req.manifest_version.as_deref().unwrap_or("unreported"),
            req.e2e_enabled,
            &e2e_kfp[..16.min(e2e_kfp.len())],
            user.user_id,
            now
        );
        let _ = e.scheduler.api_remember_with_labels(
            &audit,
            vec![
                "op_audit".into(),
                "runtime".into(),
                "binding-anchor".into(),
                "l0-exempt".into(),
            ],
        );
    }

    tracing::info!(
        "[D2] primary_executor registered: agent={} user={} e2e={}",
        req.agent_id,
        user.user_id,
        req.e2e_enabled
    );

    let body = serde_json::json!({
        "success": true, "agent_id": req.agent_id,
        "e2e_enabled": req.e2e_enabled, "capabilities": caps,
    });
    match engine {
        Some(e) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &e,
                "runtime_register",
                body,
            )),
        ),
        None => (StatusCode::OK, Json(body)),
    }
}

/// POST /v1/runtime/unregister
pub async fn unregister(
    State(st): State<CloudState>,
    Extension(user): Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    let removed = {
        let mut executors = st.primary_executors.write();
        let r = executors.remove(&user.user_id).is_some();
        save_bindings(&executors);
        drop(executors);
        r
    };
    st.user_mgr.set_has_primary_executor(&user.user_id, false);
    if let Ok(e) = st.user_mgr.get_engine_strict(&user.user_id) {
        e.scheduler.set_runtime_primary(false);
    }
    tracing::info!(
        "[D2] primary_executor unregistered: user={} removed={}",
        user.user_id,
        removed
    );
    let body = serde_json::json!({"success": true, "removed": removed});
    let engine = user_engine(&st, &user.user_id);
    match engine {
        Some(e) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &e,
                "runtime_unregister",
                body,
            )),
        ),
        None => (StatusCode::OK, Json(body)),
    }
}

/// GET /v1/runtime/status
pub async fn status(
    State(st): State<CloudState>,
    Extension(user): Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    let binding = {
        let executors = st.primary_executors.write();
        executors.get(&user.user_id).cloned()
    };
    let body = match binding {
        Some(b) => {
            let expired = b.is_expired();
            serde_json::json!({
                "bound": !expired, "agent_id": b.agent_id, "e2e_enabled": b.e2e_enabled,
                "capabilities": b.capabilities, "registered_at": b.registered_at,
                "last_heartbeat": b.last_heartbeat,
                "expired": expired,
            })
        }
        None => serde_json::json!({"bound": false}),
    };
    let engine = user_engine(&st, &user.user_id);
    match engine {
        Some(e) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &e,
                "runtime_status",
                body,
            )),
        ),
        None => (StatusCode::OK, Json(body)),
    }
}

/// POST /v1/runtime/heartbeat
pub async fn heartbeat(
    State(st): State<CloudState>,
    Extension(user): Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    let now = Utc::now().timestamp();
    let updated = {
        let mut executors = st.primary_executors.write();
        let u = if let Some(b) = executors.get_mut(&user.user_id) {
            b.last_heartbeat = now; // 复活语义: expired binding 收到心跳即重新激活
            true
        } else {
            false
        };
        if u {
            save_bindings(&executors);
        }
        u
    };
    // 审计修复: heartbeat 补注入 — engine 重载后 flag/pubkey 丢失, 心跳复活时同步
    if updated {
        if let Ok(e) = st.user_mgr.get_engine_strict(&user.user_id) {
            e.scheduler.set_runtime_primary(true);
            let pk = st
                .primary_executors
                .read()
                .get(&user.user_id)
                .and_then(|b| b.e2e_public_key.clone());
            e.scheduler.set_e2e_pubkey(pk.as_deref());
        }
    }
    let body = serde_json::json!({"success": updated, "timestamp": now});
    let engine = user_engine(&st, &user.user_id);
    match engine {
        Some(e) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &e,
                "runtime_heartbeat",
                body,
            )),
        ),
        None => (StatusCode::OK, Json(body)),
    }
}

/// GET /v1/runtime/manifest — α0.1 签名合同端点
/// 返回机器可读合同: 协议版本 + 权限边界 + 风险卡 + 撤销承诺。
/// 签名: HMAC-SHA256(domain-separated key from TETRAMEM_MASTER_KEY)
pub async fn manifest(
    State(st): State<CloudState>,
    Extension(user): Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    let issued_at = Utc::now().timestamp();

    let risk_card = serde_json::json!([
        {"id":"R1","title":"通道加密可选","severity":"medium","detail":"register带端侧公钥则意志摘要传输层加密(γ2);无私钥端则仍为明文","mitigation":"epicode-run genkey + --e2e-key; 私钥永不离开端侧"},
        {"id":"R2","title":"你不在时也会动","severity":"medium","detail":"无人时仍耗注意力/token/机器","mitigation":"quiet_hours显式设置"},
        {"id":"R3","title":"权限等于宿主已有的手","severity":"high","detail":"文件/消息/脚本;改不了身份但能外泄已见内容","mitigation":"工具白名单"},
        {"id":"R4","title":"单主写权","severity":"high","detail":"机器被盗等于手被抢走","mitigation":"120s超时解绑+一键unregister"},
        {"id":"R5","title":"供应链","severity":"high","detail":"skill/脚本被换包","mitigation":"只装签名制品;来路不明一律拒"},
        {"id":"R6","title":"假闭环","severity":"info","detail":"装上不等于生命闭环","mitigation":"delta期验收前不宣称闭环"},
        {"id":"R7","title":"必须可撤销","severity":"high","detail":"常驻驻留可能被遗忘","mitigation":"unregister/关sidecar/收high;记忆仍保留"},
        {"id":"R8","title":"空铃烧资源","severity":"medium","detail":"will质量差时手会空抓","mitigation":"DISCARD+降权+每小时轮次上限"}
    ]);

    let body = serde_json::json!({
        "protocol": "agent-runtime/0.1",
        "issued_at": issued_at,
        "issued_to": user.user_id,
        "artifact": {
            "url": "https://epicode.cn/runtime/epicode-run.mjs",
            "sha256": "4b248fe9798d75207dba12583430c182e5d7e24d234a2c4f788c3866b8fd2fcb",
            "type": "headless-runner (gamma-1, Node>=18 单文件)",
            "adapter_url": "https://epicode.cn/runtime/adapter.mjs",
            "adapter_note": "adapter 为 alpha2 级 (无 genkey/E2E); epicode-run 为 gamma 级 (含 E2E)"
        },
        "permissions": {
            "max_urgency_auto": "medium",
            "e2e_default": false,
            "write_per_cycle": 1,
            "identity_touch": false,
            "quiet_hours": "respect"
        },
        "risk_card": risk_card,
        "confirmations_required": ["user_install","user_e2e","user_persist"],
        "revocation": {"unregister":true,"kill_sidecar":true,"revoke_high":true}
    });

    // 签名: 域分离派生 key = HMAC(master, "manifest-signing-v1")
    let signature = match std::env::var("TETRAMEM_MASTER_KEY") {
        Ok(mk) => {
            let domain_key = epicode::engine::crypto::compute_integrity_hash(
                b"manifest-signing-v1",
                mk.as_bytes(),
            );
            let body_str = serde_json::to_string(&body).unwrap_or_default();
            epicode::engine::crypto::compute_integrity_hash(
                body_str.as_bytes(),
                domain_key.as_bytes(),
            )
        }
        Err(_) => "unsigned:master-key-unset".to_string(),
    };

    let mut payload = body;
    payload["signature"] = serde_json::json!(signature);

    let engine = user_engine(&st, &user.user_id);
    match engine {
        Some(e) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &e,
                "runtime_manifest",
                payload,
            )),
        ),
        None => (StatusCode::OK, Json(payload)),
    }
}
