//! MCP session 管理（HTTP 边缘层专用，不进入 process_json）。
//!
//! 软 session 策略：initialize 颁发 session，但后续请求无头也放行（向后兼容）。
//! session 仅用于 GET SSE 推送通道关联 + 未来调度器洞察推送。
//! 严格留在 HTTP 边缘层——TCP MCP 通道（裸 JSON 行）完全不使用此模块。

use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// session 过期时间。
const SESSION_TTL: Duration = Duration::from_secs(3600);
/// 触发 GC 的 session 数量阈值。
const GC_THRESHOLD: usize = 1000;

/// 单个 session 的元信息。
#[derive(Clone)]
pub struct SessionInfo {
    pub user_id: String,
    pub created_at: Instant,
    pub last_active: Instant,
}

/// session 注册表。线程安全（DashMap），可跨 handler 共享。
#[derive(Clone)]
pub struct SessionRegistry {
    sessions: Arc<DashMap<String, SessionInfo>>,
}

impl SessionRegistry {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
        }
    }

    /// 创建新 session，返回 session ID（globally unique + opaque）。
    pub fn create(&self, user_id: &str) -> String {
        let id = generate_session_id();
        let now = Instant::now();
        self.sessions.insert(
            id.clone(),
            SessionInfo {
                user_id: user_id.into(),
                created_at: now,
                last_active: now,
            },
        );
        self.gc_if_needed();
        id
    }

    /// 校验 session 有效性（存在且未过期）。不存在或过期返回 false。
    /// 有效则刷新 last_active。
    pub fn validate(&self, session_id: &str) -> bool {
        if let Some(mut entry) = self.sessions.get_mut(session_id) {
            if entry.last_active.elapsed() < SESSION_TTL {
                entry.last_active = Instant::now();
                return true;
            }
            drop(entry);
            self.sessions.remove(session_id);
        }
        false
    }

    /// 终止 session。存在并移除返回 true，不存在返回 false。
    pub fn terminate(&self, session_id: &str) -> bool {
        self.sessions.remove(session_id).is_some()
    }

    /// 当前活跃 session 数（诊断/指标用）。
    pub fn active_count(&self) -> usize {
        self.sessions.len()
    }

    /// 定期清理过期 session。超过阈值时触发。
    fn gc_if_needed(&self) {
        if self.sessions.len() > GC_THRESHOLD {
            let now = Instant::now();
            self.sessions
                .retain(|_, info| now.duration_since(info.last_active) < SESSION_TTL);
        }
    }
}

impl Default for SessionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 生成 session ID。使用 OsRng 密码学安全随机数（之前用 SystemTime+LCG 可预测）。
/// 格式 `sess-<32 hex>`，满足规范"globally unique + opaque + 可见 ASCII"。
fn generate_session_id() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    let hex: String = buf.iter().map(|b| format!("{:02x}", b)).collect();
    format!("sess-{}", hex)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn create_returns_nonempty_unique_id() {
        let reg = SessionRegistry::new();
        let id1 = reg.create("user_a");
        let id2 = reg.create("user_a");
        assert!(id1.starts_with("sess-"));
        assert_ne!(id1, id2, "并发创建应产生不同 ID");
        assert!(id1.len() > 20);
    }

    #[test]
    fn validate_fresh_session() {
        let reg = SessionRegistry::new();
        let id = reg.create("user_a");
        assert!(reg.validate(&id));
    }

    #[test]
    fn validate_nonexistent_returns_false() {
        let reg = SessionRegistry::new();
        assert!(!reg.validate("sess-doesnotexist"));
    }

    #[test]
    fn terminate_removes_session() {
        let reg = SessionRegistry::new();
        let id = reg.create("user_a");
        assert!(reg.terminate(&id));
        assert!(!reg.validate(&id), "终止后应无效");
        assert!(!reg.terminate(&id), "二次终止返回 false");
    }

    #[test]
    fn session_records_user_id() {
        let reg = SessionRegistry::new();
        let id = reg.create("alice");
        let info = reg.sessions.get(&id).unwrap();
        assert_eq!(info.user_id, "alice");
    }

    #[test]
    fn active_count_tracks_sessions() {
        let reg = SessionRegistry::new();
        assert_eq!(reg.active_count(), 0);
        reg.create("a");
        reg.create("b");
        assert_eq!(reg.active_count(), 2);
    }

    #[test]
    fn session_ids_are_globally_unique_under_concurrency() {
        let reg = Arc::new(SessionRegistry::new());
        let mut handles = vec![];
        for _ in 0..8 {
            let r = Arc::clone(&reg);
            handles.push(thread::spawn(move || r.create("concurrent_user")));
        }
        let ids: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "并发 session ID 必须全部唯一");
    }
}
