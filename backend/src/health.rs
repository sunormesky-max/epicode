//! 公共/健康/注册登录 HTTP handlers：health、public_stats、agent_guide、register、login。

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use epicode::engine::user_manager::{UserPlan, UserInfo};

use super::helpers::{disk_free_gb, error_response, require_admin, validate_user_id};
use super::state::CloudState;

pub async fn health(
    State(st): State<CloudState>,
    axum::extract::Query(q): axum::extract::Query<super::state::HealthQuery>,
) -> Json<serde_json::Value> {
    let shallow = serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "success": true,
    });
    if q.deep != Some(1) {
        return Json(shallow);
    }

    let user_count = st.user_mgr.list_users().len();
    let disk_free_gb = disk_free_gb();
    let disk_ok = disk_free_gb > 2.0;

    let main_engine = st.user_mgr.list_users().into_iter()
        .filter(|u| u.parent.is_none())
        .filter_map(|u| st.user_mgr.get_engine(&u.user_id).ok())
        .next();

    let (space_stats, cognitive_effectiveness) = if let Some(ref engine) = main_engine {
        let stats = engine.scheduler.api_stats();
        let kg = engine.scheduler.api_graph_stats();
        let eff_summary = engine.scheduler.outcome_effectiveness_summary();
        let eff_json: Vec<serde_json::Value> = eff_summary.iter()
            .map(|(at, score)| serde_json::json!({"action": format!("{:?}", at), "effectiveness": (score * 100.0).round() / 100.0}))
            .collect();
        (serde_json::json!({
            "memories": stats.tetra_count,
            "vertices": stats.vertex_count,
            "clusters": stats.clusters,
            "energy": stats.energy,
            "kg_relations": kg.0,
            "kg_concepts": kg.1,
            "aggregation_rate": 0.0,
        }), eff_json)
    } else {
        (serde_json::json!({"error": "main engine not available"}), vec![])
    };

    let healthy = disk_ok && user_count > 0;
    let degraded = !healthy;

    Json(serde_json::json!({
        "status": if degraded { "degraded" } else { "ok" },
        "version": env!("CARGO_PKG_VERSION"),
        "success": true,
        "deep": true,
        "users": user_count,
        "disk_free_gb": (disk_free_gb * 100.0).round() / 100.0,
        "space": space_stats,
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
        "success": true
    }))
}

pub async fn agent_guide() -> (StatusCode, [(axum::http::HeaderName, &'static str); 2], &'static str) {
    let guide = concat!(
        "# Epicode Agent Guide\n",
        "\n",
        "Epicode is an AI Memory Operating System. It gives AI agents persistent, searchable, connected memory across sessions.\n",
        "\n",
        "## Authentication\n",
        "\n",
        "Header: `X-API-Key: YOUR_API_KEY`\n",
        "Rate limit: Free=60, Pro=300, Enterprise=1000 requests/minute\n",
        "\n",
        "## MCP Endpoint\n",
        "\n",
        "`POST https://epicode.cn/api/mcp`\n",
        "Content-Type: application/json | Protocol: JSON-RPC 2.0\n",
        "\n",
        "Request: `{\"jsonrpc\":\"2.0\",\"method\":\"tools/call\",\"params\":{\"name\":\"TOOL_NAME\",\"arguments\":{...}},\"id\":1}`\n",
        "Response: `{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"{...}\"}]}}`\n",
        "Note: the `text` field is a JSON string that needs to be parsed again.\n",
        "\n",
        "## Error Codes\n",
        "\n",
        "- `-32700` Parse error (malformed JSON)\n",
        "- `-32601` Method not found\n",
        "- `-32602` Invalid params\n",
        "- `-32603` Internal error\n",
        "- `-32001` Invalid API key\n",
        "\n",
        "## Quick Start\n",
        "\n",
        "```\n",
        "1. identity_confirm(name, mission, author)  — ONCE, then immutable\n",
        "2. ctx_load(project, task)                  — ALWAYS call at session start\n",
        "3. ... work normally, use tools below ...\n",
        "4. session_summary(accomplished, next_steps) — ALWAYS call at session end\n",
        "```\n",
        "\n",
        "## MCP Tools (call `tools/list` for full schemas)\n",
        "\n",
        "Memory CRUD:       memory_create, memory_get, memory_list, memory_update, memory_delete, memory_search\n",
        "Deep Recall:       memory_recall\n",
        "Session Lifecycle: ctx_load (MANDATORY at start), ctx_save, session_summary\n",
        "Knowledge Capture: pattern_learn, pattern_recall, decision_record, bug_memory\n",
        "Knowledge Graph:   knowledge_relations, concepts, kg_quality, dream_cycle\n",
        "Identity:          identity_confirm, identity_step, identity_finalize\n",
        "Skills:            skill_execute, skills_sync, skill_feedback\n",
        "Feedback & Rules:  feedback_submit, enforced_rules, project_list\n",
        "Documents:         doc_import, doc_list\n",
        "System:            space_stats, context_observe, embedding_diagnostic, embedding_migrate\n",
        "\n",
        "## Key Rules\n",
        "\n",
        "- ctx_load MUST be called at every session start. Pass `task` for precision loading.\n",
        "- feedback_submit after using search results — the system learns from outcomes.\n",
        "- session_summary at session end — next ctx_load picks up from it.\n",
        "- enforced_rules returns hard constraints — inject into system prompts.\n",
        "- Identity is immutable after confirmation.\n",
    );
    (
        StatusCode::OK,
        [
            (axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (axum::http::HeaderName::from_static("cache-control"), "public, max-age=3600"),
        ],
        guide,
    )
}

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub user_id: String,
    pub plan: Option<String>,
    pub password: String,
}

pub async fn register_user(
    State(st): State<CloudState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<RegisterRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let invite_code = headers.get("X-Invite-Code")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // 先验证输入（kimi #4：邀请码应在验证通过后才消耗，避免注册失败也作废）
    if let Err(e) = validate_user_id(&req.user_id) {
        return error_response(StatusCode::BAD_REQUEST, &e);
    }
    if req.password.len() < 6 {
        return error_response(StatusCode::BAD_REQUEST, "password must be at least 6 characters");
    }

    // 验证通过后检查授权（邀请码/admin）
    let via_admin = invite_code.is_empty();
    if via_admin {
        if let Err(resp) = require_admin(&st.admin_key, &headers) {
            return resp;
        }
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

    let plan = match req.plan.as_deref().unwrap_or("free") {
        "pro" => UserPlan::Pro,
        "enterprise" => UserPlan::Enterprise,
        _ => UserPlan::Free,
    };
    let api_key = format!("tm-{}", uuid::Uuid::new_v4().to_string().replace("-", ""));

    match st.user_mgr.register(&req.user_id, &api_key, plan, &req.password) {
        Ok(info) => {
            tracing::info!("user registered: {} plan={:?}", info.user_id, info.plan);
            (StatusCode::OK, Json(serde_json::json!({
                "success": true,
                "user_id": info.user_id,
                "api_key": api_key,
                "plan": serde_json::to_value(&info.plan).unwrap_or_default(),
                "max_memories": info.max_memories,
            })))
        }
        Err(e) => {
            // 注册失败回补邀请码 (审计 2026-09 低优 #22)
            if !via_admin {
                st.user_mgr.refund_invite_code(invite_code);
            }
            error_response(StatusCode::BAD_REQUEST, &e)
        }
    }
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub user_id: String,
    pub password: String,
}

pub async fn login_user(
    State(st): State<CloudState>,
    Json(req): Json<LoginRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if let Err(e) = validate_user_id(&req.user_id) {
        return error_response(StatusCode::BAD_REQUEST, &e).into_response();
    }
    if req.password.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "password is required").into_response();
    }
    if req.password.len() > 128 {
        return error_response(StatusCode::BAD_REQUEST, "password too long (max 128 characters)").into_response();
    }
    match st.user_mgr.login(&req.user_id, &req.password) {
        Ok(info) => {
            // B6修复:Set-Cookie HttpOnly(防 XSS 窃取),同时响应体保留 api_key 向后兼容
            let cookie = format!(
                "epicode_session={}; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=604800",
                info.api_key
            );
            let body = Json(serde_json::json!({
                "success": true,
                "user_id": info.user_id,
                "api_key": info.api_key,
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
    (StatusCode::OK, [("set-cookie", cookie)], Json(serde_json::json!({"success": true})))
}

/// 智能化突破: SSE实时流 — 每3秒推送完整认知状态 + 订阅洞察事件
/// 推送内容：能量/记忆/簇/Port/情感(PAD)/驱动力/认知状态 + insight事件
pub async fn sse_stream(
    State(st): State<CloudState>,
    axum::extract::Extension(user): axum::extract::Extension<UserInfo>,
) -> impl axum::response::IntoResponse {
    use axum::response::sse::{Event, Sse};
    use tokio_stream::wrappers::IntervalStream;
    use tokio_stream::StreamExt;

    let user_id = user.user_id.clone();

    // 状态流：每3秒推送认知状态快照
    let status_interval = IntervalStream::new(tokio::time::interval(std::time::Duration::from_secs(3)))
        .map(move |_| {
            // 用 slots_read 避免在 SSE 闭包里触发同步引擎恢复（会导致死锁）
            let slots = st.user_mgr.slots_read();
            let engine = match slots.get(&user_id) {
                Some(slot) => slot.engine.clone(),
                None => return Ok::<_, std::convert::Infallible>(Event::default().data(r#"{"status":"loading"}"#)),
            };
            drop(slots);
            let space = engine.space();
            let tetras = space.tetra_count();
            let vertices = space.vertex_count();
            let (ports_assigned, ports_free) = space.port_stats();
            let identity = space.identity_info();
            let energy = engine.energy.available();
            let clusters = space.find_clusters().len();

            // 智能化突破：获取认知引擎的完整状态
            let (emotion, drive, cognitive_status, latest_thought) = engine.cognitive_snapshot();
            let decision_count = engine.decision_history_count();
            // 批次C：认知深度数据（learning/reflection）
            let learning = engine.cognitive_learning();
            let reflection = engine.cognitive_reflection();

            let data = serde_json::json!({
                "type": "status",
                "memories": tetras,
                "vertices": vertices,
                "energy": energy,
                "clusters": clusters,
                "ports_assigned": ports_assigned,
                "ports_free": ports_free,
                "identity_name": identity.as_ref().map(|i| i.system_name.clone()).unwrap_or_default(),
                "cognitive_status": cognitive_status,
                "emotion": emotion,
                "drive": drive,
                "decision_count": decision_count,
                "latest_thought": latest_thought,
                "learning": learning,
                "last_reflection": reflection.as_ref().map(|(o, i)| serde_json::json!({"observation": o, "insight": i})),
            });
            Ok(Event::default().data(serde_json::to_string(&data).unwrap_or_default()))
        });

    // 洞察事件通过 insight_tx 推送，但 broadcast 订阅需要异步 stream
    // 批次1 先只做状态推送，insight 订阅在批次1.5 接入（需要 async stream merge）
    // SSE 状态里已包含 latest_thought，前端能实时看到 LLM 最新思考

    Sse::new(status_interval).keep_alive(axum::response::sse::KeepAlive::default())
}
