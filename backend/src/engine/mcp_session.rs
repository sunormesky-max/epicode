//! MCP session 管理（HTTP 边缘层专用，不进入 process_json）。
//!
//! 软 session 策略：initialize 颁发 session，但后续请求无头也放行（向后兼容）。
//! session 仅用于 GET SSE 推送通道关联 + 未来调度器洞察推送。
//! 严格留在 HTTP 边缘层——TCP MCP 通道（裸 JSON 行）完全不使用此模块。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
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

/// session 注册表。线程安全（Mutex<HashMap>），可跨 handler 共享。
/// 低争用场景（每连接校验一次，非每请求）：Mutex 足够且消除 dashmap 依赖。
#[derive(Clone)]
pub struct SessionRegistry {
    sessions: Arc<Mutex<HashMap<String, SessionInfo>>>,
}

impl SessionRegistry {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 创建新 session，返回 session ID（globally unique + opaque）。
    pub fn create(&self, user_id: &str) -> String {
        let id = generate_session_id();
        let now = Instant::now();
        let mut map = self.sessions.lock().expect("session lock");
        map.insert(
            id.clone(),
            SessionInfo {
                user_id: user_id.into(),
                created_at: now,
                last_active: now,
            },
        );
        let over_threshold = map.len() > GC_THRESHOLD;
        drop(map);
        if over_threshold {
            self.gc();
        }
        id
    }

    /// 校验 session 有效性（存在且未过期）。不存在或过期返回 false。
    /// 有效则刷新 last_active。
    pub fn validate(&self, session_id: &str) -> bool {
        let mut map = self.sessions.lock().expect("session lock");
        match map.get_mut(session_id) {
            Some(info) if info.last_active.elapsed() < SESSION_TTL => {
                info.last_active = Instant::now();
                true
            }
            Some(_) => {
                map.remove(session_id);
                false
            }
            None => false,
        }
    }

    /// 终止 session。存在并移除返回 true，不存在返回 false。
    pub fn terminate(&self, session_id: &str) -> bool {
        self.sessions
            .lock()
            .expect("session lock")
            .remove(session_id)
            .is_some()
    }

    /// 当前活跃 session 数（诊断/指标用）。
    pub fn active_count(&self) -> usize {
        self.sessions.lock().expect("session lock").len()
    }

    /// 清理过期 session（GC_THRESHOLD 超限时调用）。
    fn gc(&self) {
        let now = Instant::now();
        self.sessions
            .lock()
            .expect("session lock")
            .retain(|_, info| now.duration_since(info.last_active) < SESSION_TTL);
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
    let hex: String = buf.iter().map(|b| format!("{b:02x}")).collect();
    format!("sess-{hex}")
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
        assert!(reg.validate(&id), "刚创建的 session 应有效");
        assert!(reg.validate(&id), "二次校验仍应有效（刷新 last_active）");
    }

    #[test]
    fn validate_unknown_session() {
        let reg = SessionRegistry::new();
        assert!(!reg.validate("sess-nonexistent"));
    }

    #[test]
    fn terminate_removes_session() {
        let reg = SessionRegistry::new();
        let id = reg.create("user_a");
        assert!(reg.terminate(&id));
        assert!(!reg.validate(&id), "终止后不应再有效");
        assert!(!reg.terminate(&id), "二次终止返回 false");
    }

    #[test]
    fn active_count_tracks_lifecycle() {
        let reg = SessionRegistry::new();
        assert_eq!(reg.active_count(), 0);
        let _a = reg.create("a");
        let _b = reg.create("b");
        assert_eq!(reg.active_count(), 2);
    }

    #[test]
    fn concurrent_create_and_validate() {
        let reg = SessionRegistry::new();
        let reg2 = reg.clone();
        let handle = thread::spawn(move || {
            let mut ids = Vec::new();
            for k in 0..50 {
                ids.push(reg2.create(&format!("user_{k}")));
            }
            ids
        });
        let local: Vec<String> = (0..50).map(|k| reg.create(&format!("local_{k}"))).collect();
        let remote = handle.join().expect("no panic");
        assert_eq!(reg.active_count(), 100);
        for id in local.iter().chain(remote.iter()) {
            assert!(reg.validate(id), "并发创建的 session 应全部有效");
        }
    }
}
