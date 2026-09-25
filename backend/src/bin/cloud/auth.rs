//! 鉴权 + 限流中间件（由 HTTP Router 通过 from_fn_with_state 挂载）。

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Instant;

use axum::extract::{ConnectInfo, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::IntoResponse;
use axum::Json;

use epicode::engine::user_manager::UserPlan;

use super::helpers::require_admin;
use super::state::{CloudState, RateBucket, RATE_LIMIT_MAX, RATE_LIMIT_WINDOW_SECS};

pub async fn auth_middleware(
    State(st): State<CloudState>,
    headers: axum::http::HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    mut request: axum::extract::Request,
    next: Next,
) -> axum::response::Response {
    let path = request.uri().path().to_string();

    // Identity for rate-limiting: per-source-IP for auth endpoints so credential
    // brute-force on /v1/login is throttled per attacker, not hidden behind a shared anon bucket.
    // 安全修复：不信任客户端发送的 X-Forwarded-For（可被伪造绕过限流）。
    // 使用 TCP 连接的真实远程地址。Nginx 反代场景下为 127.0.0.1（此时 Nginx 已做 limit_req），
    // 直连场景下为真实客户端 IP。
    let client_id = if path == "/v1/login" || path == "/register" {
        format!("ip:{}", addr.ip())
    } else {
        "anonymous".to_string()
    };

    // 预认证粗限流(审计二轮): 仅无凭据流量与登录/注册走这里;
    // 带凭据(cookie/header/ticket)的请求在认证成功后按 user_id 精细分桶 —
    // 此前 cookie 登录的网页请求共用 anonymous 桶, 单键被限会误伤所有会话
    let has_credential = headers.contains_key("X-API-Key")
        || headers.contains_key("X-Admin-Key")
        || headers.get_all("cookie").iter().any(|v| {
            v.to_str()
                .map(|s| s.contains("epicode_session="))
                .unwrap_or(false)
        });
    if !has_credential {
        if let Some(resp) = check_rate_limit(st, &client_id, RATE_LIMIT_MAX) {
            return resp;
        }
    }

    // 累计 API 调用次数（非公开端点才计）+ 按日按用户统计
    let is_public = path.starts_with("/health")
        || path == "/ready"
        || path == "/"
        || path == "/docs"
        || path == "/openapi.yaml"
        || path == "/v1/login"
        || path == "/v1/skills/explore"
        || path == "/stats/public"
        || path == "/v1/agent-guide"
        || path == "/v1/smrp";
    if !is_public {
        let mut counts = st.api_call_counts.lock();
        *counts.entry(client_id.clone()).or_insert(0) += 1;
        drop(counts);
        // 按日按用户统计（api_key → date → count），用于前端曲线图 + 异步 flush 到用户 db
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let mut daily = st.api_calls_daily.lock();
        let user_daily = daily.entry(client_id.clone()).or_insert_with(HashMap::new);
        *user_daily.entry(today).or_insert(0) += 1;
    }

    // Public/static endpoints + auth endpoints (already rate-limited above) bypass API-key auth.
    if path.starts_with("/health")
        || path == "/v1/health"
        || path == "/ready"
        || path == "/"
        || path == "/docs"
        || path == "/openapi.yaml"
        || path == "/v1/login"
        || path == "/v1/skills/explore"
        || path == "/stats/public"
        || path == "/v1/agent-guide"
        || path == "/v1/smrp"
    {
        return next.run(request).await;
    }

    if path.starts_with("/admin") {
        if let Err(resp) = require_admin(&st.admin_key, &headers) {
            return resp.into_response();
        }
        return next.run(request).await;
    }

    if path == "/register" {
        return next.run(request).await;
    }

    if path == "/v1/logout" {
        return next.run(request).await;
    }

    let header_key = headers
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let cookie_key = headers
        .get_all("cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|s| s.split(';'))
        .map(|s| s.trim())
        .find_map(|s| s.strip_prefix("epicode_session="))
        .map(|s| s.to_string());
    // SSE旧路径退役: ?key=裸密钥出URL(审计安全债) — 现支持header→cookie→ticket(短票)三级
    // 半截工程收口(2026-08-30 租户断连根因): 签发端已有, 消费端缺失 — EventSource无法带header,
    // 无cookie租户(无密码账号)此前只能回退已退役的?key= → 401死循环
    // 2026-09 审计修复: ticket 限定 /v1/stream 路由 + 一次性原子消费(重放窗口=0)
    let ticket_key: Option<String> = if path == "/v1/stream" {
        request.uri().query().and_then(|q| {
            q.split('&')
                .find_map(|kv| kv.strip_prefix("ticket="))
                .and_then(|t| {
                    let mut m = st.stream_tickets.lock();
                    match m.get(t).cloned() {
                        Some((api_key, exp)) if exp > chrono::Utc::now().timestamp() => {
                            // 消费即毁: 验证成功的票当场移除, 杜绝 120s 内跨路由重放
                            m.remove(t);
                            Some(api_key)
                        }
                        Some(_) => {
                            m.remove(t);
                            None
                        }
                        None => None,
                    }
                })
        })
    } else {
        None
    };
    let api_key_owned = header_key.or(cookie_key).or(ticket_key).unwrap_or_default();
    let api_key = api_key_owned.as_str();

    let user_info = match st.user_mgr.authenticate(api_key) {
        Some(u) => u,
        None => {
            tracing::warn!("auth failed: path={} key_len={}", path, api_key.len()); // 不打印 key 前缀（kimi2.7 #20）
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "success": false, "error": "invalid API key"
                })),
            )
                .into_response();
        }
    };

    st.user_mgr.touch(&user_info.user_id);
    // 认证后限流: 统一按真实用户身份键(套餐分级 Free=60/Pro=300/Ent=1000 每分钟)
    let plan_limit = match user_info.plan {
        UserPlan::Free => 60,
        UserPlan::Pro => 300,
        UserPlan::Enterprise => 1000,
    };
    if let Some(resp) = check_rate_limit(st, &format!("user:{}", user_info.user_id), plan_limit) {
        return resp;
    }
    request.extensions_mut().insert(user_info);
    next.run(request).await
}

/// 限流检查, 返回 Some(429响应) 表示超限拒绝
fn check_rate_limit(st: &CloudState, key: &str, limit: usize) -> Option<axum::response::Response> {
    use std::time::Duration;
    const RATE_BUCKET_SOFT_CAP: usize = 100_000;
    let mut limits = st.rate_limits.lock();
    let now = Instant::now();
    // 插入路径突发防护: 接近容量先就地淘汰过期窗口, 不再单靠 5 分钟周期清理
    // (审计二轮: 周期间隙伪造键仍可短时堆积)
    if limits.len() > RATE_BUCKET_SOFT_CAP {
        let cutoff = now - Duration::from_secs(RATE_LIMIT_WINDOW_SECS * 2);
        limits.retain(|_, b| b.window_start > cutoff);
    }
    let bucket = limits.entry(key.to_string()).or_insert_with(|| RateBucket {
        count: 0,
        window_start: now,
    });
    if now.duration_since(bucket.window_start).as_secs() > RATE_LIMIT_WINDOW_SECS {
        bucket.count = 0;
        bucket.window_start = now;
    }
    bucket.count += 1;
    if bucket.count > limit {
        return Some(
            (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({
                    "success": false,
                    "error": format!("rate limit exceeded ({} requests/min)", limit)
                })),
            )
                .into_response(),
        );
    }
    None
}
