//! 公共/健康/注册登录 HTTP handlers：health、public_stats、agent_guide、register、login。

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use epicode::engine::user_manager::{UserInfo, UserManager, UserPlan};

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
        "total_mcp_tools": 41,
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
    &'static str,
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
        "### Response: three-layer nesting (IMPORTANT)\n",
        "\n",
        "Epicode responses are triple-nested JSON. Parse each layer:\n",
        "  Layer 1 (JSON-RPC envelope):  result.content[0].text\n",
        "  Layer 2 (SMRP envelope):      data / status / protocol\n",
        "  Layer 3 (payload):            the actual data inside data\n",
        "\n",
        "Example memory_search response (abridged):\n",
        "  {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"{\\\"data\\\":{\\\"memories\\\":[...],\\\"total\\\":5},\\\"status\\\":{\\\"identity\\\":{\\\"name\\\":\\\"...\\\"}},\\\"protocol\\\":{\\\"ok\\\":true,\\\"tool\\\":\\\"memory_search\\\"}}\"}]}}\n",
        "\n",
        "Parse result.content[0].text as JSON to get data.memories.\n",
        "\n",
        "### SMRP envelope fields\n",
        "\n",
        "  protocol.ok      = true = success, false = tool-level error\n",
        "  protocol.tool    = which tool produced this response\n",
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
        "  SMRP ok=false = tool-level error (data may be null)\n",
        "\n",
        "## 4. Session Lifecycle (MANDATORY)\n",
        "\n",
        "  1. identity_confirm(name, mission, author)   -- ONCE, then immutable forever\n",
        "  2. ctx_load(project, task)                    -- ALWAYS at session start\n",
        "  3. ... work normally, call tools below ...\n",
        "  4. session_summary(accomplished, next_steps)  -- ALWAYS at session end\n",
        "\n",
        "- ctx_load with task param enables intent-aware retrieval - always pass a task description.\n",
        "- session_summary auto-tags as memory_class=session - next ctx_load picks up from it.\n",
        "\n",
        "## 5. 36-Tool Quick Reference\n",
        "\n",
        "### Memory CRUD & Search (9 tools)\n",
        "  memory_create    Store a memory; similar memories auto-cluster\n",
        "  memory_search    Semantic search (vector + BM25 hybrid, max limit 200)\n",
        "  memory_recall    Deep recall via knowledge graph associations\n",
        "  memory_get       Get a specific memory by ID\n",
        "  memory_list      List memories with filters (labels, pagination)\n",
        "  memory_update    Update content/labels of a memory\n",
        "  memory_delete    Delete a memory by ID\n",
        "  memory_export    Bulk export memories (JSON, optional label filter)\n",
        "  memory_restore   Restore importance/state of a memory\n",
        "\n",
        "### Session Lifecycle (4 tools)\n",
        "  ctx_load         MANDATORY at session start - loads relevant context\n",
        "  ctx_save         Manually save a context checkpoint\n",
        "  session_summary  MANDATORY at session end - persists summary\n",
        "  session_list     List recent session memories\n",
        "\n",
        "### Knowledge Capture (4 tools)\n",
        "  pattern_learn    Store a code pattern/convention\n",
        "  pattern_recall   Recall patterns relevant to a context\n",
        "  decision_record  Record an architecture/design decision\n",
        "  bug_memory       Record a bug + fix to avoid repeating\n",
        "\n",
        "### Knowledge Graph (4 tools)\n",
        "  knowledge_relations  Query KG relations for a memory\n",
        "  concepts             List concept prototypes (topic clusters)\n",
        "  kg_quality           Assess KG health (density, orphan rate)\n",
        "  dream_cycle          Run consolidation to strengthen connections\n",
        "\n",
        "### Identity - Immutable (3 tools)\n",
        "  identity_confirm    Confirm permanent identity - ONCE then immutable\n",
        "  identity_step       Ritual ceremony: 5-step identity confirmation\n",
        "  identity_finalize   Seal identity permanently\n",
        "\n",
        "### Skills & Feedback (6 tools)\n",
        "  skill_execute    Execute a skill from the library\n",
        "  skill_feedback   Submit skill helpfulness feedback\n",
        "  skills_sync      Export skills to local agent directories\n",
        "  feedback_submit  Submit memory outcome feedback (system learns)\n",
        "  enforced_rules   Get hard constraints (inject into system prompts)\n",
        "  project_list     List projects with stored memories\n",
        "\n",
        "### Documents & Diagnostics (6 tools)\n",
        "  doc_import           Import markdown doc into memory\n",
        "  doc_list             List imported documents\n",
        "  context_observe      Auto-extract & store conversation context\n",
        "  embedding_diagnostic Diagnose embedding dimension health\n",
        "  embedding_migrate    Re-embed ALL memories (heavy operation)\n",
        "  space_stats          Space statistics (memory/vertex/cluster count)\n",
        "\n",
        "## 6. Memory Class System\n",
        "\n",
        "Memories are layered by lifecycle class:\n",
        "  permanent  Default; long-lived, importance decays slowly\n",
        "  session    Superseded after 7 days; auto-tagged by session_summary/ctx_save\n",
        "  bridge     Short-lived connector; superseded after 1 day (auto-fuse)\n",
        "\n",
        "null memory_class is treated as permanent.\n",
        "\n",
        "## 7. Best Practices\n",
        "\n",
        "- ALWAYS call feedback_submit after using search/recall results.\n",
        "- ALWAYS call ctx_load(task=...) at start for precision retrieval.\n",
        "- ALWAYS call session_summary at end so next session resumes cleanly.\n",
        "- Inject enforced_rules() output into your system prompt as mandatory constraints.\n",
        "- Use memory_recall (not memory_search) when you need connected context.\n",
        "- Run dream_cycle periodically to strengthen KG connections.\n",
        "- Identity is IMMUTABLE after identity_confirm - choose wisely.\n",

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

    match st
        .user_mgr
        .register(&req.user_id, &api_key, plan, &req.password)
    {
        Ok(info) => {
            tracing::info!("user registered: {} plan={:?}", info.user_id, info.plan);
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "user_id": info.user_id,
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
    pub user_id: String,
    pub password: String,
}

/// P27 Pulse: 接收守护进程遥测 — 真实活跃时间回流(带身份验证+归属校验)
#[derive(serde::Deserialize)]
#[allow(dead_code)] // 运维探针保留
pub struct PulseHeartbeat {
    pub task_id: String,
    pub real_active_ms: i64,
    pub local_now_ms: i64,
}

#[allow(dead_code)] // 运维探针保留
pub async fn pulse_heartbeat(
    State(st): State<CloudState>,
    axum::extract::Extension(user): axum::extract::Extension<UserInfo>,
    axum::Json(req): axum::Json<PulseHeartbeat>,
) -> (StatusCode, Json<serde_json::Value>) {
    if req.real_active_ms < 0 || req.real_active_ms > 86_400_000 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "real_active_ms out of range"})),
        );
    }
    let engine = match st.user_mgr.get_engine(&user.user_id) {
        Ok(e) => e,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": e})),
            )
        }
    };
    match engine
        .storage
        .set_real_active(&req.task_id, &user.user_id, req.real_active_ms)
    {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))),
        Err(e) if e.contains("not found") => (
            StatusCode::NOT_FOUND,
            Json(
                serde_json::json!({"ok": false, "stop": true, "reason": "task completed or not found — stop pulsing"}),
            ),
        ),
        Err(e) => (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"ok": false, "stop": true, "error": e})),
        ),
    }
}

/// P27c: Pulse 脚本分发(带API Key验证)
#[allow(dead_code)] // 运维探针保留
pub async fn pulse_script(
    _user: axum::extract::Extension<UserInfo>,
) -> impl axum::response::IntoResponse {
    (
        StatusCode::OK,
        [("content-type", "text/plain; charset=utf-8")],
        r#"#!/usr/bin/env bash
# Epicode Pulse v2.0 - 时间掐表器
# 掐表: pulse-start --task-id <id> --api-key <key> --budget-ms <ms>
# 停表: pulse-stop --task-id <id>
# 查表: pulse-status

__epicode_pulse_start() {
  local TASK_ID="" API_KEY="" BUDGET_MS=0 ENDPOINT="https://epicode.cn"
  while [ $# -gt 0 ]; do
    case "$1" in
      --task-id) TASK_ID="$2"; shift 2 ;;
      --api-key) API_KEY="$2"; shift 2 ;;
      --budget-ms) BUDGET_MS="$2"; shift 2 ;;
      --endpoint) ENDPOINT="$2"; shift 2 ;;
      *) shift ;;
    esac
  done
  if [ -z "$TASK_ID" ] || [ -z "$API_KEY" ] || [ "$BUDGET_MS" -eq 0 ]; then
    echo "usage: pulse-start --task-id <id> --api-key <key> --budget-ms <ms>" >&2
    return 1
  fi
  local DIR="$HOME/.epicode"
  mkdir -p "$DIR"
  local PID_FILE="$DIR/pulse-${TASK_ID}.pid"
  local SIG_FILE="$DIR/pulse-${TASK_ID}"
  if [ -f "$PID_FILE" ]; then kill "$(cat "$PID_FILE")" 2>/dev/null; fi
  (
    local START=$(date +%s%3N)
    local LAST_BEAT=0
    while true; do
      local NOW=$(date +%s%3N)
      local ELAPSED=$((NOW - START))
      local PCT=$((ELAPSED * 100 / BUDGET_MS))
      local PHASE="explore"
      if [ "$PCT" -ge 90 ]; then PHASE="deliver"
      elif [ "$PCT" -ge 70 ]; then PHASE="verify"
      elif [ "$PCT" -ge 30 ]; then PHASE="build"
      fi
      echo "[+$((ELAPSED/1000))s|剩$((100-PCT))%|${PHASE}]" > "$SIG_FILE"
      if [ $((NOW - LAST_BEAT)) -ge 30000 ]; then
        local RESP
        RESP=$(curl -s -m 5 -X POST "${ENDPOINT}/api/v1/task-heartbeat" \
          -H "X-API-Key: ${API_KEY}" \
          -H "Content-Type: application/json" \
          -d "{\"task_id\":\"${TASK_ID}\",\"real_active_ms\":${ELAPSED},\"local_now_ms\":${NOW}}" 2>/dev/null)
        if echo "$RESP" | grep -q '"stop"\|"error"\|401\|403'; then
          cp "$SIG_FILE" "$SIG_FILE.final" 2>/dev/null
          rm -f "$PID_FILE" "$SIG_FILE" 2>/dev/null
          exit 0
        fi
        LAST_BEAT=$NOW
      fi
      if [ $((ELAPSED - BUDGET_MS * 2)) -gt 0 ]; then
        cp "$SIG_FILE" "$SIG_FILE.final" 2>/dev/null
        rm -f "$PID_FILE" "$SIG_FILE" 2>/dev/null
        exit 0
      fi
      sleep 5
    done
  ) &
  echo $! > "$PID_FILE"
  echo "pulse started: task=$TASK_ID pid=$(cat "$PID_FILE")"
}

__epicode_pulse_stop() {
  local TID=""
  while [ $# -gt 0 ]; do
    case "$1" in
      --task-id) TID="$2"; shift 2 ;;
      *) shift ;;
    esac
  done
  if [ -z "$TID" ]; then
    echo "usage: pulse-stop --task-id <id>" >&2
    return 1
  fi
  local PF="$HOME/.epicode/pulse-${TID}.pid"
  if [ -f "$PF" ]; then
    kill "$(cat "$PF")" 2>/dev/null
    rm -f "$PF" "$HOME/.epicode/pulse-${TID}"
    echo "pulse stopped: task=$TID"
  else
    echo "no pulse running for task=$TID"
  fi
}

__epicode_pulse_status() {
  local found=0
  for pf in "$HOME"/.epicode/pulse-*.pid; do
    [ -f "$pf" ] || continue
    local t
    t=$(basename "$pf" .pid | sed 's/pulse-//')
    if kill -0 "$(cat "$pf")" 2>/dev/null; then
      echo "running: $t"
      found=1
    fi
  done
  if [ "$found" -eq 0 ]; then echo "no active pulses"; fi
  return 0
}

pulse-start() { __epicode_pulse_start "$@"; }
pulse-stop() { __epicode_pulse_stop "$@"; }
pulse-status() { __epicode_pulse_status; }
"#,
    )
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
                    .drive_queue()
                    .peek_unacked(200)
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
                    Some(slot) => slot.engine.scheduler().drive_queue().peek_unacked(200),
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
            let sig_json: Vec<_> = fresh.iter().map(|s| {
                let (desc_field, e2e_field) = match &e2e_pem {
                    Some(pem) => match epicode::engine::e2e::encrypt_for(s.description.as_bytes(), pem) {
                        Ok(ct) => (serde_json::Value::Null, serde_json::json!(ct)),
                        Err(_) => (serde_json::json!(s.description), serde_json::Value::Null),
                    },
                    None => (serde_json::json!(s.description), serde_json::Value::Null),
                };
                serde_json::json!({
                    "id": s.id,
                    "intent_type": s.intent_type,
                    "status": s.status,
                    "urgency": s.urgency,
                    "description": desc_field, "description_e2e": e2e_field, "evidence": s.evidence,
                    "enqueued_at_ms": s.enqueued_at_ms,
                })
            }).collect();
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
