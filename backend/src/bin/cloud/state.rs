//! Cloud 二进制共享状态：限流桶、全局状态、常量、公共查询结构。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;
use serde::Deserialize;

use epicode::engine::skills::SkillEngine;
use epicode::engine::user_manager::UserManager;

/// 单客户端限流桶。
pub struct RateBucket {
    pub count: usize,
    pub window_start: Instant,
}

/// Phase 3: 启动阶段 — WarmingUp 时 /health 返回 warming_up, 非 health 请求返回 503
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // 启动期保留
pub enum StartupPhase {
    WarmingUp,
    Ready,
}

/// D2: Primary Executor binding (C Spec §14.1: 多端只读, 执行写单主)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExecutorBinding {
    pub agent_id: String,
    pub user_id: String,
    pub capabilities: Vec<String>,
    pub registered_at: i64,
    pub last_heartbeat: i64,
    /// D3: e2e flag (§14.5: e2e=false 禁 high/critical 自动执行)
    pub e2e_enabled: bool,
    /// γ2: 端侧 E2E 公钥 (PEM SPKI) — 有钥则 signal description 传输层加密
    #[serde(default)]
    pub e2e_public_key: Option<String>,
}

impl ExecutorBinding {
    pub fn is_expired(&self) -> bool {
        let now = chrono::Utc::now().timestamp();
        let last = self.last_heartbeat;
        now - last > 120 // 120s heartbeat timeout
    }
}

#[derive(Clone)]
pub struct CloudState {
    pub user_mgr: Arc<UserManager>,
    pub admin_key: String,
    pub rate_limits: Arc<Mutex<HashMap<String, RateBucket>>>,
    pub active_tasks: Arc<std::sync::atomic::AtomicU32>,
    pub pub_skills: Arc<SkillEngine>,
    /// 累计 API 调用次数（按用户 api_key 分桶）
    pub api_call_counts: Arc<Mutex<HashMap<String, u64>>>,
    /// 按日 API 调用统计（api_key → (date_str → count)），按用户分桶，用于前端曲线图 + 异步 flush 到用户 db
    pub api_calls_daily: Arc<Mutex<HashMap<String, HashMap<String, u64>>>>,
    /// 智能化突破：认知洞察广播通道（LLM thoughts/dream insights/reflect → SSE → 前端）
    #[allow(dead_code)] // 认知洞察广播通道保留
    pub insight_tx: Arc<tokio::sync::broadcast::Sender<epicode::engine::insight::InsightEvent>>,
    /// Phase 3: 启动阶段标记(WarmingUp → Ready)
    pub startup_phase: Arc<std::sync::atomic::AtomicU8>,
    /// D2: primary_executor binding per user (user_id → ExecutorBinding)
    pub primary_executors: Arc<parking_lot::RwLock<HashMap<String, ExecutorBinding>>>,
    pub stream_tickets: Arc<parking_lot::Mutex<HashMap<String, (String, i64)>>>,
    /// L1 图书馆: 全局共享知识资产(独立SQLite+共享HNSW, 无引擎依赖)
    pub library: Arc<epicode::engine::library::LibraryStore>,
}

pub const RATE_LIMIT_WINDOW_SECS: u64 = 60;
pub const RATE_LIMIT_MAX: usize = 120;

#[derive(Deserialize)]
pub struct HealthQuery {
    pub deep: Option<i32>,
}
