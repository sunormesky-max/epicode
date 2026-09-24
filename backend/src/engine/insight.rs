//! 洞察事件类型——调度器产出的认知成果，经 SSE 推送给在线客户端。
//!
//! 定义在 engine 层（而非 cloud.rs），因为 SchedulerCenter（engine）需要构造它，
//! 而 cloud（bin）依赖 engine，不能反向依赖。
//!
//! per-user 路由：每个事件携带 user_id，SSE 端按鉴权用户过滤，隐私不串流。

use serde::Serialize;

/// 调度器主动推送的洞察事件。
#[derive(Clone, Serialize)]
pub struct InsightEvent {
    /// 洞察来源：dream / cognitive_reflect / cognitive_thought
    pub kind: &'static str,
    /// 产生洞察的用户 ID（per-user 路由，SSE 端按此过滤）。
    pub user_id: String,
    pub title: String,
    pub detail: String,
    pub timestamp: String,
}

impl InsightEvent {
    /// 构造并发送到广播通道。
    /// 无订阅者时 send 返回 Err，静默忽略（洞察推送是旁路，不能拖垮调度器）。
    pub fn emit(
        tx: &tokio::sync::broadcast::Sender<InsightEvent>,
        kind: &'static str,
        user_id: &str,
        title: &str,
        detail: &str,
    ) {
        let _ = tx.send(InsightEvent {
            kind,
            user_id: user_id.into(),
            title: title.into(),
            detail: detail.into(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emit_without_subscribers_does_not_panic() {
        let (tx, _) = tokio::sync::broadcast::channel::<InsightEvent>(16);
        // 无接收端，send 应返回 Err 但不 panic
        InsightEvent::emit(&tx, "dream", "user_a", "title", "detail");
    }

    #[test]
    fn emit_delivers_to_subscriber() {
        let (tx, mut rx) = tokio::sync::broadcast::channel::<InsightEvent>(16);
        InsightEvent::emit(&tx, "cognitive_reflect", "user_b", "obs", "insight");
        let event = rx.try_recv().unwrap();
        assert_eq!(event.kind, "cognitive_reflect");
        assert_eq!(event.user_id, "user_b");
        assert_eq!(event.title, "obs");
        assert_eq!(event.detail, "insight");
        assert!(!event.timestamp.is_empty());
    }
}
