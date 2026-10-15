//! 公共/健康/注册登录 HTTP handlers：health、public_stats、agent_guide、register、login。

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use epicode::engine::user_manager::{UserInfo, UserPlan};

use super::helpers::{disk_free_gb, error_response, require_admin, validate_user_id};
use super::state::CloudState;

pub async fn health(
    State(st): State<CloudState>,
    axum::extract::Query(q): axum::extract::Query<super::state::HealthQuery>,
) -> Json<serde_json::Value> {
    // Phase 3: 检查启动阶段
    use std::sync::atomic::Ordering;
    let phase = st.startup_phase.load(Ordering::Relaxed);
    let is_ready = phase == 1u8; // 1 = Ready, 0 = WarmingUp
    let shallow = serde_json::json!({
        "status": if is_ready { "ok" } else { "warming_up" },
        "ready": is_ready,
        "version": env!("CARGO_PKG_VERSION"),
        "success": true,
    });
    if q.deep != Some(1) {
        return Json(shallow);
    }

    let user_count = st.user_mgr.list_users().len();
    let disk_free_gb = disk_free_gb();
    let disk_ok = disk_free_gb > 2.0;

    let slot = st
        .user_mgr
        .list_users()
        .into_iter()
        .filter(|u| u.parent.is_none())
        .find_map(|u| st.user_mgr.try_get_engine_slot(&u.user_id));

    let (space_stats, cognitive_effectiveness) = if let Some(ref engine) = slot {
        let stats = engine.scheduler.api_stats();
        let kg = engine.scheduler.api_graph_stats();
        let eff_summary = engine.scheduler.outcome_effectiveness_summary();
        let eff_json: Vec<serde_json::Value> = eff_summary.iter()
            .map(|(at, score)| serde_json::json!({"action": format!("{:?}", at), "effectiveness": (score * 100.0).round() / 100.0}))
            .collect();
        (
            serde_json::json!({
                "memories": stats.tetra_count,
                "vertices": stats.vertex_count,
                "clusters": stats.clusters,
                "energy": stats.energy,
                "kg_relations": kg.0,
                "kg_concepts": kg.1,
                "aggregation_rate": 0.0,
            }),
            eff_json,
        )
    } else {
        (
            serde_json::json!({"error": "main engine not loaded"}),
            vec![],
        )
    };

    let model_ok = st.user_mgr.vector_ready();
    let llm_ok = std::env::var("MINIMAX_API_KEY").is_ok()
        || std::env::var("DEEPSEEK_API_KEY").is_ok()
        || std::env::var("OPENAI_API_KEY").is_ok();
    let cache_ok = slot.is_some();
    let index_ok = slot.is_some();
    let components = serde_json::json!({
        "cache": if cache_ok { "ok" } else { "warming" },
        "index": if index_ok { "ok" } else { "warming" },
        "model": if model_ok { "ok" } else { "missing" },
        "llm": if llm_ok { "ok" } else { "unconfigured" },
        "disk": if disk_ok { "ok" } else { "low" },
    });

    let healthy = disk_ok && user_count > 0 && model_ok;
    let degraded = !healthy;

    Json(serde_json::json!({
        "status": if !is_ready { "warming_up" } else if degraded { "degraded" } else { "ok" },
        "ready": is_ready,
        "version": env!("CARGO_PKG_VERSION"),
        "success": true,
        "deep": true,
        "users": user_count,
        "disk_free_gb": (disk_free_gb * 100.0).round() / 100.0,
        "space": space_stats,
        "components": components,
        "cognitive_effectiveness": cognitive_effectiveness,
    }))
}

pub async fn public_stats(State(st): State<CloudState>) -> Json<serde_json::Value> {
    let users = st.user_mgr.list_users();
    let total_memories: u64 = users.iter().map(|u| u.memories_used as u64).sum();
    let total_skills = st.pub_skills.list_public().len() as u64;
    Json(serde_json::json!({
        "total_users": users.len(),
        "total_memories": total_memories,
        "total_skills": total_skills,
        "total_mcp_tools": epicode::engine::mcp::McpHandler::public_tool_count(),
        "success": true
    }))
}

/// Phase 3: /ready — readiness probe for load balancer / k8s
/// Returns 200 when Ready, 503 when WarmingUp.
pub async fn ready(
    State(st): State<CloudState>,
) -> (axum::http::StatusCode, Json<serde_json::Value>) {
    use std::sync::atomic::Ordering;
    let is_ready = st.startup_phase.load(Ordering::Relaxed) == 1u8;
    let status_code = if is_ready {
        axum::http::StatusCode::OK
    } else {
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status_code,
        Json(serde_json::json!({
            "ready": is_ready,
            "status": if is_ready { "ready" } else { "warming_up" },
        })),
    )
}

/// Phase 3 P0: GET /v1/persona/ready — 人格 readiness（API key 鉴权）
/// 返回该 key 对应用户的人格加载状态。
pub async fn persona_ready(
    State(st): State<CloudState>,
    headers: axum::http::HeaderMap,
) -> (axum::http::StatusCode, Json<serde_json::Value>) {
    use epicode::engine::user_manager::PersonaState;
    use std::sync::atomic::Ordering;

    let process_ready = st.startup_phase.load(Ordering::Relaxed) == 1u8;

    // 从 API key 拿 user_id
    let api_key = headers
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let user_id = st.user_mgr.find_user_by_api_key(api_key);

    if user_id.is_empty() {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "ready": false,
                "status": "unauthorized",
                "error": "invalid API key"
            })),
        );
    }

    // 拿真实的 per-user persona state
    let persona_state = st.user_mgr.get_persona_state(&user_id);
    let loop_started = st.user_mgr.is_loop_started(&user_id);

    let (ready, status_code, _persona_phase) = match persona_state {
        PersonaState::Ready => {
            let cognitive_enabled = std::env::var("ENABLE_COGNITIVE").as_deref() == Ok("1");
            let phase = if loop_started {
                if cognitive_enabled {
                    "live_full"
                } else {
                    "live_quiet"
                }
            } else {
                "ready"
            };
            (true, axum::http::StatusCode::OK, phase)
        }
        PersonaState::WarmingUp => (
            false,
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "warming_up",
        ),
        PersonaState::Degraded => (
            false,
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "degraded",
        ),
        PersonaState::Unknown => (
            false,
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "unknown",
        ),
    };

    // Phase 3 P0-3: 契约 #1658 标准 envelope
    // 只在 persona_state == Ready 时读 drive 数据（避免 get_engine 阻塞）
    let (sweep_applied, drive_total, drive_restored) = if persona_state == PersonaState::Ready {
        // 用 slot 直接访问（不触发 get_engine 加载）
        match st.user_mgr.try_get_engine_slot(&user_id) {
            Some(engine) => {
                let dq = engine.scheduler.drive_queue();
                (
                    dq.sweep_applied(),
                    dq.stats()
                        .get("total")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                    true,
                )
            }
            None => (0, 0, false),
        }
    } else {
        (0, 0, false)
    };

    let body = if ready {
        serde_json::json!({
            "ok": true,
            "process": if process_ready { "up" } else { "warming_up" },
            "persona": {
                "state": "ready",
                "live": if loop_started { Some(if std::env::var("ENABLE_COGNITIVE").as_deref() == Ok("1") { "full" } else { "quiet" }) } else { None },
                "user_id": user_id,
                "drive": {
                    "restored": drive_restored,
                    "sweep_applied": sweep_applied,
                    "signals_total": drive_total,
                }
            },
            "error": serde_json::Value::Null
        })
    } else {
        serde_json::json!({
            "ok": false,
            "process": "up",
            "persona": {
                "state": match persona_state {
                    PersonaState::WarmingUp => "warming_up",
                    PersonaState::Unknown => "unknown",
                    PersonaState::Degraded => "degraded",
                    _ => "unknown",
                },
                "user_id": user_id,
            },
            "error": {
                "code": match persona_state {
                    PersonaState::WarmingUp | PersonaState::Unknown => "PERSONA_WARMING_UP",
                    PersonaState::Degraded => "PERSONA_DEGRADED",
                    _ => "UNKNOWN",
                },
                "retryable": persona_state != PersonaState::Degraded,
                "retry_after_ms": 3000,
            }
        })
    };

    (status_code, Json(body))
}

pub async fn agent_guide() -> (
    StatusCode,
    [(axum::http::HeaderName, &'static str); 2],
    String,
) {
    let guide = concat!(
                "# Epicode Agent Guide\n",
        "\n",
        "> Epicode is an AI Memory Operating System - persistent, searchable, connected memory across sessions.\n",
        "> Identity: immutable after confirmation. Stateful feel over a stateless protocol (2026-07-28).\n",
        "\n",
        "## 1. Authentication\n",
        "\n",
        "| Header | Value |\n",
        "|--------|-------|\n",
        "| X-API-Key | tm-<your_key> |\n",
        "\n",
        "Rate limits: Free=60 / Pro=300 / Enterprise=1000 per minute.\n",
        "\n",
        "## 2. MCP Endpoint\n",
        "\n",
        "POST https://epicode.cn/api/mcp\n",
        "Content-Type: application/json | Protocol: JSON-RPC 2.0\n",
        "\n",
        "Request:\n",
        "  {\"jsonrpc\":\"2.0\",\"method\":\"tools/call\",\"params\":{\"name\":\"TOOL\",\"arguments\":{...}},\"id\":1}\n",
        "\n",
        "### Response (structured + backward compatible)\n",
        "\n",
        "Native tools/call returns the SMRP object as result.structuredContent.\n",
        "Older hosts can still parse result.content[0].text as JSON to get the same object.\n",
        "The tool output schema describes protocol/data/status; request and response limit is 1 MiB.\n",
        "\n",
        "Example memory_search response (abridged):\n",
        "  {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"structuredContent\":{\"protocol\":{\"ok\":true,\"tool\":\"memory_search\"},\"data\":{\"results\":[...]},\"status\":{}},\"content\":[{\"type\":\"text\",\"text\":\"{...same SMRP JSON...}\"}],\"isError\":false}}\n",
        "\n",
        "Check result.isError and structuredContent.protocol.ok before using data.\n",
        "\n",
        "### SMRP envelope fields\n",
        "\n",
        "  protocol.ok      = true = success, false = tool-level error\n",
        "  protocol.tool    = canonical operation name (memory_ask for /ask)\n",
        "  protocol.error   = error message if ok=false\n",
        "  status.identity  = current agent identity (name/system)\n",
        "  status.space     = space snapshot (memories/clusters/energy)\n",
        "  data             = the actual tool payload\n",
        "\n",
        "## 3. Error Codes\n",
        "\n",
        "  -32700  Parse error (malformed JSON)\n",
        "  -32601  Method not found\n",
        "  -32602  Invalid params\n",
        "  -32603  Internal error\n",
        "  -32001  Invalid API key\n",
        "  -32000  Request or response exceeds the 1 MiB limit\n",
        "  SMRP ok=false / result.isError=true = tool-level error (data may be null)\n",
        "\n",
        "## 4. Executor Manual (canonical)\n",
        "\n",
        "Everything below is the canonical Executor Manual, identical to MCP epicode_handshake\n",
        "system_skill.content and skill_get(\"epicode-system\"). tools/list is authoritative for schemas.\n",
        "\n",
    );
    (
        StatusCode::OK,
        [
            (
                axum::http::header::CONTENT_TYPE,
                "text/plain; charset=utf-8",
            ),
            (
                axum::http::HeaderName::from_static("cache-control"),
                "public, max-age=3600",
            ),
        ],
        format!(
            "{guide}{}",
            epicode::engine::mcp::McpHandler::system_manual()
        ),
    )
}

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub user_id: String,
    pub plan: Option<String>,
    pub password: String,
    /// Optional login alias. When set, login accepts this email as well as user_id.
    #[serde(default)]
    pub email: Option<String>,
}

pub async fn register_user(
    State(st): State<CloudState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<RegisterRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let invite_code = headers
        .get("X-Invite-Code")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // 先验证输入（kimi #4：邀请码应在验证通过后才消耗，避免注册失败也作废）
    if let Err(e) = validate_user_id(&req.user_id) {
        return error_response(StatusCode::BAD_REQUEST, &e);
    }
    if req.password.len() < 6 {
        return error_response(
            StatusCode::BAD_REQUEST,
            "password must be at least 6 characters",
        );
    }
    if let Some(email) = req.email.as_deref() {
        if !email.trim().is_empty()
            && epicode::engine::user_manager::normalize_email(email).is_err()
        {
            return error_response(StatusCode::BAD_REQUEST, "invalid email");
        }
    }

    // 验证通过后检查授权（邀请码/admin）
    let mut via_admin = false;
    if invite_code.is_empty() {
        if let Err(resp) = require_admin(&st.admin_key, &headers) {
            return resp;
        }
        via_admin = true;
    } else {
        // 保留名检查必须在消耗邀请码之前 — 否则用保留名注册的失败尝试
        // 也会白烧一个名额 (审计二轮)
        if epicode::engine::user_manager::UserManager::is_reserved_id(&req.user_id) {
            return error_response(StatusCode::FORBIDDEN, "this username is reserved");
        }
        if let Err(e) = st.user_mgr.use_invite_code(invite_code) {
            return error_response(StatusCode::FORBIDDEN, &e);
        }
    }

    // P17 越权修复: 套餐只能由admin路径授予 — 邀请码注册一律Free
    // (此前任何持码者可自选enterprise, 10万记忆额度自助封顶)
    let plan = match if via_admin {
        req.plan.as_deref().unwrap_or("free")
    } else {
        "free"
    } {
        "pro" => UserPlan::Pro,
        "enterprise" => UserPlan::Enterprise,
        _ => UserPlan::Free,
    };
    let api_key = format!("tm-{}", uuid::Uuid::new_v4().to_string().replace("-", ""));

    match st.user_mgr.register(
        &req.user_id,
        &api_key,
        plan,
        &req.password,
        req.email.as_deref(),
    ) {
        Ok(info) => {
            tracing::info!("user registered: {} plan={:?}", info.user_id, info.plan);
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "user_id": info.user_id,
                "email": info.email,
                    "api_key": api_key,
                    "plan": serde_json::to_value(&info.plan).unwrap_or_default(),
                    "max_memories": info.max_memories,
                })),
            )
        }
        Err(e) => {
            // 邀请码与账户创建非同事务: 注册失败时回补邀请码, 持码者不损失名额
            // (审计 2026-09 低优 #22)
            if !via_admin {
                st.user_mgr.refund_invite_code(invite_code);
            }
            error_response(StatusCode::BAD_REQUEST, &e)
        }
    }
}

#[derive(Deserialize)]
pub struct LoginRequest {
    /// user_id or registered email
    pub user_id: String,
    pub password: String,
}

pub async fn login_user(
    State(st): State<CloudState>,
    Json(req): Json<LoginRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if req.user_id.contains('@') {
        if epicode::engine::user_manager::normalize_email(&req.user_id).is_err() {
            return error_response(StatusCode::BAD_REQUEST, "invalid email").into_response();
        }
    } else if let Err(e) = validate_user_id(&req.user_id) {
        return error_response(StatusCode::BAD_REQUEST, &e).into_response();
    }
    if req.password.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "password is required").into_response();
    }
    if req.password.len() > 128 {
        return error_response(
            StatusCode::BAD_REQUEST,
            "password too long (max 128 characters)",
        )
        .into_response();
    }
    match st.user_mgr.login(&req.user_id, &req.password) {
        Ok(info) => {
            // 安全债收口: 登录体不再回传 api_key — 认证中间件已支持 HttpOnly cookie(header→cookie→ticket三级),
            // 响应体回传密钥会抵消 HttpOnly 的 XSS 防护。注册响应保留首次返回(唯一一次明文下发)。
            let cookie = format!(
                "epicode_session={}; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=604800",
                info.api_key
            );
            let body = Json(serde_json::json!({
                "success": true,
                "user_id": info.user_id,
                "plan": serde_json::to_value(&info.plan).unwrap_or_default(),
                "max_memories": info.max_memories,
            }));
            (StatusCode::OK, [("set-cookie", cookie.as_str())], body).into_response()
        }
        Err(e) => error_response(StatusCode::UNAUTHORIZED, &e).into_response(),
    }
}

/// B6: 登出 — 清除 HttpOnly session cookie
pub async fn logout_user() -> impl axum::response::IntoResponse {
    let cookie = "epicode_session=; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=0";
    (
        StatusCode::OK,
        [("set-cookie", cookie)],
        Json(serde_json::json!({"success": true})),
    )
}

pub async fn mint_stream_ticket(
    State(st): State<CloudState>,
    axum::extract::Extension(user): axum::extract::Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    let ticket = uuid::Uuid::new_v4().to_string();
    let exp = chrono::Utc::now().timestamp() + 120;
    {
        let mut m = st.stream_tickets.lock();
        let now = chrono::Utc::now().timestamp();
        m.retain(|_, (_, e)| *e > now);
        m.insert(ticket.clone(), (user.api_key.clone(), exp));
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "ticket": ticket,
            "expires_in": 120,
        })),
    )
}

fn drive_sse_signal(
    signal: &epicode::engine::drive::DriveSignal,
    e2e_public_key: Option<&str>,
) -> serde_json::Value {
    signal.sse_value(e2e_public_key)
}

/// 智能化突破: SSE实时流 — 每3秒推送完整认知状态 + 订阅洞察事件
/// 推送内容：能量/记忆/簇/Port/情感(PAD)/驱动力/认知状态 + insight事件
pub async fn sse_stream(
    State(st): State<CloudState>,
    axum::extract::Extension(user): axum::extract::Extension<UserInfo>,
) -> impl axum::response::IntoResponse {
    use axum::response::sse::{Event, Sse};
    use tokio_stream::wrappers::ReceiverStream;

    let user_id = user.user_id.clone();
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, std::convert::Infallible>>(32);

    // Task 1: status push every 3s
    let tx1 = tx.clone();
    let st1 = st.clone();
    let uid1 = user_id.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3));
        loop {
            interval.tick().await;
            let engine = {
                let slots = st1.user_mgr.slots_read();
                match slots.get(&uid1) {
                    Some(slot) => slot.engine.clone(),
                    None => continue,
                }
            }; // slots dropped here
            let space = engine.space();
            let tetras = space.tetra_count();
            let energy = engine.energy.available();
            let clusters = space.try_cluster_count().unwrap_or(0);
            let (emotion, drive, cog_status, thought) = engine.cognitive_snapshot();
            let data = serde_json::json!({
                "type": "status", "memories": tetras, "energy": energy,
                "clusters": clusters, "emotion": emotion, "drive": drive,
                "cognitive_status": cog_status, "latest_thought": thought,
            });
            let evt = Event::default().data(serde_json::to_string(&data).unwrap_or_default());
            if tx1.send(Ok(evt)).await.is_err() {
                break;
            }
        }
    });

    // Task 2: D1 drive event push (notify + 5s fallback poll, max_id dedup)
    // 修复 Notify 丢失唤醒竞态: notify_waiters 只唤醒已在 await 的任务,
    // 若 listener 忙于发送上一条事件则唤醒丢失 → 加 5s 兜底轮询补推
    let tx2 = tx.clone();
    let st2 = st.clone();
    let uid2 = user_id.clone();
    tokio::spawn(async move {
        let notify = loop {
            let handle = {
                let slots = st2.user_mgr.slots_read();
                slots
                    .get(&uid2)
                    .map(|slot| slot.engine.scheduler().drive_queue().notify_handle())
            };
            if let Some(h) = handle {
                break h;
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            if tx2.is_closed() {
                return;
            }
        };
        // 基线: 只推送连接后新 enqueue 的 signal (id > 基线 max)
        let mut last_pushed_max_id: u64 = {
            let slots = st2.user_mgr.slots_read();
            match slots.get(&uid2) {
                Some(slot) => slot
                    .engine
                    .scheduler()
                    .current_drive_inbox(200)
                    .iter()
                    .map(|s| s.id)
                    .max()
                    .unwrap_or(0),
                None => 0,
            }
        };
        loop {
            tokio::select! {
                _ = notify.notified() => {},
                _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {},
            }
            let signals = {
                let slots = st2.user_mgr.slots_read();
                match slots.get(&uid2) {
                    Some(slot) => slot.engine.scheduler().current_drive_inbox(200),
                    None => continue,
                }
            }; // slots dropped here
            let fresh: Vec<_> = signals
                .iter()
                .filter(|s| s.id > last_pushed_max_id)
                .collect();
            if fresh.is_empty() {
                continue;
            }
            if let Some(new_max) = fresh.iter().map(|s| s.id).max() {
                last_pushed_max_id = new_max;
            }
            // γ2: 传输层 E2E (与 REST inbox 同语义)
            let e2e_pem = {
                let slots = st2.user_mgr.slots_read();
                slots
                    .get(&uid2)
                    .and_then(|slot| slot.engine.scheduler().e2e_pubkey())
            };
            let sig_json: Vec<_> = fresh
                .iter()
                .map(|signal| drive_sse_signal(signal, e2e_pem.as_deref()))
                .collect();
            let pushed_at_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64;
            let lags: Vec<i64> = fresh
                .iter()
                .map(|s| {
                    if s.enqueued_at_ms > 0 {
                        pushed_at_ms - s.enqueued_at_ms
                    } else {
                        0
                    }
                })
                .collect();
            let server_lag_ms = lags.iter().copied().max().unwrap_or(0);
            let data = serde_json::json!({
                "type": "drive",
                "action": "enqueue",
                "pushed_at_ms": pushed_at_ms,
                "server_lag_ms": server_lag_ms,
                "signals": sig_json
            });
            let evt = Event::default().data(serde_json::to_string(&data).unwrap_or_default());
            if tx2.send(Ok(evt)).await.is_err() {
                break;
            }
        }
    });

    Sse::new(ReceiverStream::new(rx)).keep_alive(axum::response::sse::KeepAlive::default())
}

#[cfg(test)]
mod drive_sse_tests {
    use super::drive_sse_signal;
    use epicode::engine::drive::{DriveIntent, DriveSignal, DriveStatus, DriveUrgency};

    #[test]
    fn sse_drive_event_uses_the_documented_transport_projection() {
        let signal = DriveSignal {
            id: 7,
            timestamp: 1_780_000_000,
            intent_type: DriveIntent::Explore,
            description: "Inspect the documented route".into(),
            evidence: vec![10, 20],
            urgency: DriveUrgency::Low,
            target_capability: None,
            emotion: None,
            origin_tick: 3,
            status: DriveStatus::Pending,
            feedback: None,
            retry_count: 0,
            expires_at: Some(1_780_000_100),
            enqueued_at_ms: 1_780_000_000_000,
            time_budget_ms: None,
            grounding: None,
            terminal_reason: None,
        };

        let value = drive_sse_signal(&signal, None);
        assert_eq!(value["id"], 7);
        assert_eq!(value["intent_type"], "explore");
        assert_eq!(value["status"], "pending");
        assert_eq!(value["description"], "Inspect the documented route");
        assert_eq!(value["description_e2e"], serde_json::Value::Null);
        assert_eq!(value["evidence"], serde_json::json!([10, 20]));
        assert_eq!(value["grounding"], serde_json::Value::Null);
        assert_eq!(value["grounding_e2e"], serde_json::Value::Null);
        assert_eq!(value["enqueued_at_ms"], 1_780_000_000_000i64);
        assert!(value.get("emotion").is_none());
        assert!(value.get("timestamp").is_none());
    }
}
