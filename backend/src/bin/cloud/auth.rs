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

fn has_conflicting_user_ids(user_ids: &[&str]) -> bool {
    user_ids
        .first()
        .is_some_and(|first| user_ids.iter().any(|user_id| user_id != first))
}

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
        // 受信代理模式(四轮审计): 反代后所有登录共享 TCP peer(网关IP)额度 —
        // 显式设置 EPICODE_TRUSTED_PROXY=1 时改用网关注入的 X-Real-IP
        // (默认关闭: 客户端伪造 X-Real-For/X-Forwarded-For 可绕过限流,
        // 仅当后端仅接受网关流量时开启)
        let trusted = std::env::var("TETRAMEM_TRUSTED_PROXY")
            .map(|v| v == "1")
            .unwrap_or(false);
        let ip = if trusted {
            headers
                .get("X-Real-IP")
                .and_then(|v| v.to_str().ok())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .unwrap_or_else(|| addr.ip().to_string())
        } else {
            addr.ip().to_string()
        };
        format!("ip:{ip}")
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
    // IP 限流对登录/注册路径无条件执行(审计三轮高优): 凭据头"存在"不等于
    // "有效" — 带任意 junk X-API-Key 的 /v1/login 此前会跳过 IP 桶绕过限流
    let is_auth_endpoint = path == "/v1/login" || path == "/register";
    if is_auth_endpoint || !has_credential {
        if let Some(resp) = check_rate_limit(&st, &client_id, RATE_LIMIT_MAX) {
            return resp;
        }
    }

    // Public/static endpoints + auth endpoints (already rate-limited above) bypass API-key auth.
    // API call stats are recorded after authentication, keyed by api_key.
    // Counting here used client_id=anonymous, which user_stats never reads
    // and the flush cannot authenticate.
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
    let mut authenticated_users = Vec::new();
    for credential in [
        header_key.as_deref(),
        cookie_key.as_deref(),
        ticket_key.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(user) = st.user_mgr.authenticate(credential) {
            authenticated_users.push(user);
        }
    }

    let user_ids: Vec<&str> = authenticated_users
        .iter()
        .map(|user| user.user_id.as_str())
        .collect();
    // Never let header precedence route a request as a different account than its cookie or SSE ticket.
    if has_conflicting_user_ids(&user_ids) {
        if let Some(response) = check_rate_limit(
            &st,
            &format!("conflicting-auth-ip:{}", addr.ip()),
            RATE_LIMIT_MAX,
        ) {
            return response;
        }
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "success": false, "error": "conflicting authentication credentials"
            })),
        )
            .into_response();
    }

    let user_info = match authenticated_users.into_iter().next() {
        Some(user) => user,
        None => {
            let key_len = header_key
                .as_ref()
                .or(cookie_key.as_ref())
                .or(ticket_key.as_ref())
                .map_or(0, String::len);
            tracing::warn!("auth failed: path={} key_len={}", path, key_len); // 不打印 key 前缀（kimi2.7 #20）
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
    record_api_call(&st, &user_info.api_key);
    // 认证后限流: 统一按真实用户身份键(套餐分级 Free=60/Pro=300/Ent=1000 每分钟)
    let plan_limit = match user_info.plan {
        UserPlan::Free => 60,
        UserPlan::Pro => 300,
        UserPlan::Enterprise => 1000,
    };
    if let Some(resp) = check_rate_limit(&st, &format!("user:{}", user_info.user_id), plan_limit) {
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

#[cfg(test)]
mod tests {
    use super::has_conflicting_user_ids;

    #[test]
    fn conflicting_user_credentials_are_rejected_without_rejecting_same_user_migration() {
        assert!(has_conflicting_user_ids(&["user-a", "user-b"]));
        assert!(!has_conflicting_user_ids(&["user-a", "user-a"]));
        assert!(!has_conflicting_user_ids(&["user-a"]));
        assert!(!has_conflicting_user_ids(&[]));
    }
}

fn record_api_call(st: &CloudState, api_key: &str) {
    if api_key.is_empty() {
        return;
    }
    let mut counts = st.api_call_counts.lock();
    *counts.entry(api_key.to_string()).or_insert(0) += 1;
    drop(counts);
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let mut daily = st.api_calls_daily.lock();
    let user_daily = daily
        .entry(api_key.to_string())
        .or_insert_with(HashMap::new);
    *user_daily.entry(today).or_insert(0) += 1;
}
