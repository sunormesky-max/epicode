use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

// ═══════════════════════════════════════════════════════════
// Legacy Drive Engine (original drive system — kept for backward compat)
// ═══════════════════════════════════════════════════════════

/// The personality's active drive — what it "wants" to do right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Drive {
    Curiosity,
    Coherence,
    Efficiency,
    Vitality,
}

impl std::fmt::Display for Drive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Drive::Curiosity => write!(f, "Curiosity"),
            Drive::Coherence => write!(f, "Coherence"),
            Drive::Efficiency => write!(f, "Efficiency"),
            Drive::Vitality => write!(f, "Vitality"),
        }
    }
}

impl std::str::FromStr for Drive {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Curiosity" => Ok(Drive::Curiosity),
            "Coherence" => Ok(Drive::Coherence),
            "Efficiency" => Ok(Drive::Efficiency),
            "Vitality" => Ok(Drive::Vitality),
            _ => Err(format!("unknown drive: {}", s)),
        }
    }
}

/// Drive Engine — tracks the dominant drive and adapts based on outcomes.
pub struct DriveEngine {
    dominant: Drive,
    curiosity_score: f64,
    coherence_score: f64,
    efficiency_score: f64,
    vitality_score: f64,
    tick_count: u64,
    last_observe: ObserveState,
    /// δ1: 回执→权重演化历史 (最近50条: tick, drive, delta) — 环4可观测性
    evolution_history: std::collections::VecDeque<(u64, String, f64)>,
}

#[derive(Clone, Default)]
struct ObserveState {
    tetra_count: usize,
    cluster_count: usize,
    avg_entropy: f64,
    energy_ratio: f64,
    unexplored_ratio: f64,
    redundancy_ratio: f64,
}

impl DriveEngine {
    pub fn new() -> Self {
        Self {
            dominant: Drive::Coherence,
            curiosity_score: 1.0,
            coherence_score: 1.0,
            efficiency_score: 1.0,
            vitality_score: 1.0,
            tick_count: 0,
            last_observe: ObserveState::default(),
            evolution_history: std::collections::VecDeque::new(),
        }
    }

    pub fn dominant(&self) -> Drive {
        self.dominant
    }

    pub fn set_dominant(&mut self, d: Drive) {
        self.dominant = d;
    }

    pub fn snapshot(&self) -> serde_json::Value {
        serde_json::json!({
            "dominant": self.dominant.to_string(),
        })
    }

    /// 稳态持久化: 演化状态跨重启存活 (曾每次重启 weights 重置 1.0, 意志失忆)
    pub fn persist_snapshot(&self) -> serde_json::Value {
        let hist: Vec<serde_json::Value> = self.evolution_history.iter()
            .map(|(tick, drive, delta)| serde_json::json!({"tick": tick, "drive": drive, "delta": delta}))
            .collect();
        serde_json::json!({
            "dominant": self.dominant.to_string(),
            "curiosity": self.curiosity_score, "coherence": self.coherence_score,
            "efficiency": self.efficiency_score, "vitality": self.vitality_score,
            "tick_count": self.tick_count, "evolution_history": hist,
        })
    }
    pub fn restore_from(&mut self, v: &serde_json::Value) {
        if let Some(s) = v.get("dominant").and_then(|x| x.as_str()) {
            if let Ok(d) = s.parse::<Drive>() {
                self.dominant = d;
            }
        }
        for (k, field) in [
            ("curiosity", 0),
            ("coherence", 1),
            ("efficiency", 2),
            ("vitality", 3),
        ] {
            if let Some(x) = v.get(k).and_then(|y| y.as_f64()) {
                match field {
                    0 => self.curiosity_score = x,
                    1 => self.coherence_score = x,
                    2 => self.efficiency_score = x,
                    _ => self.vitality_score = x,
                }
            }
        }
        if let Some(t) = v.get("tick_count").and_then(|x| x.as_u64()) {
            self.tick_count = t;
        }
        if let Some(h) = v.get("evolution_history").and_then(|x| x.as_array()) {
            self.evolution_history = h
                .iter()
                .filter_map(|e| {
                    let tick = e.get("tick")?.as_u64()?;
                    let drive = e.get("drive")?.as_str()?.to_string();
                    let delta = e.get("delta")?.as_f64()?;
                    Some((tick, drive, delta))
                })
                .collect();
        }
    }

    /// δ1: 四驱权重 + 演化历史 — GET /v1/drive/evolution 的数据源
    pub fn evolution_snapshot(&self) -> serde_json::Value {
        let hist: Vec<serde_json::Value> = self
            .evolution_history
            .iter()
            .map(|(tick, drive, delta)| {
                serde_json::json!({
                    "tick": tick, "drive": drive, "delta": (delta * 1000.0).round() / 1000.0,
                })
            })
            .collect();
        serde_json::json!({
            "dominant": self.dominant.to_string(),
            "weights": {
                "curiosity": (self.curiosity_score * 1000.0).round() / 1000.0,
                "coherence": (self.coherence_score * 1000.0).round() / 1000.0,
                "efficiency": (self.efficiency_score * 1000.0).round() / 1000.0,
                "vitality": (self.vitality_score * 1000.0).round() / 1000.0,
            },
            "observe_ticks": self.tick_count,
            "evolution_history": hist,
        })
    }

    /// Observe the current system state and adjust dominant drive.
    pub fn observe(
        &mut self,
        tetra_count: usize,
        cluster_count: usize,
        avg_entropy: f64,
        energy_ratio: f64,
        unexplored_ratio: f64,
        redundancy_ratio: f64,
    ) {
        self.tick_count += 1;
        self.last_observe = ObserveState {
            tetra_count,
            cluster_count,
            avg_entropy,
            energy_ratio,
            unexplored_ratio,
            redundancy_ratio,
        };

        // Simple heuristic: pick the dominant drive based on system state
        if unexplored_ratio > 0.3 {
            self.dominant = Drive::Curiosity;
        } else if avg_entropy > 0.4 {
            self.dominant = Drive::Coherence;
        } else if redundancy_ratio > 0.2 {
            self.dominant = Drive::Efficiency;
        } else if energy_ratio > 0.8 {
            self.dominant = Drive::Vitality;
        }
    }

    /// Reward a drive type based on action effectiveness.
    pub fn reward(&mut self, drive_type: Drive, effectiveness: f64) {
        let delta = effectiveness * 0.01;
        match drive_type {
            Drive::Curiosity => self.curiosity_score += delta,
            Drive::Coherence => self.coherence_score += delta,
            Drive::Efficiency => self.efficiency_score += delta,
            Drive::Vitality => self.vitality_score += delta,
        }
        // δ1: 记录演化历史 (环4: 回执改策略的可观测证据)
        self.evolution_history
            .push_back((self.tick_count, drive_type.to_string(), delta));
        while self.evolution_history.len() > 50 {
            self.evolution_history.pop_front();
        }
    }

    pub fn should_pulse(&self) -> bool {
        self.dominant == Drive::Curiosity || self.dominant == Drive::Vitality
    }

    pub fn should_fission(&self) -> bool {
        self.dominant == Drive::Coherence && self.last_observe.avg_entropy > 0.3
    }

    pub fn should_dream(&self) -> bool {
        self.dominant == Drive::Coherence
    }

    pub fn should_evict(&self) -> bool {
        self.dominant == Drive::Efficiency && self.last_observe.redundancy_ratio > 0.15
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DriveSignal {
    pub id: u64,
    pub timestamp: i64,
    #[serde(rename = "intent_type")]
    pub intent_type: DriveIntent,
    pub description: String,
    #[serde(default)]
    pub evidence: Vec<u64>,
    pub urgency: DriveUrgency,
    #[serde(default)]
    pub target_capability: Option<String>,
    #[serde(default)]
    pub emotion: Option<EmotionSnapshot>,
    pub origin_tick: u64,
    #[serde(default = "default_status")]
    pub status: DriveStatus,
    #[serde(default)]
    pub feedback: Option<DriveFeedback>,
    /// Phase 2: 重试计数(dead-letter 判定用)
    #[serde(default)]
    pub retry_count: u32,
    /// Phase 2: 过期时间戳(unix 秒), None = 不过期
    #[serde(default)]
    pub expires_at: Option<i64>,
    #[serde(default)]
    pub enqueued_at_ms: i64,
    /// 时间效性集成(L0汇合): 信号携带的时间预算(unix ms), None = 无预算语义
    #[serde(default)]
    pub time_budget_ms: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriveIntent {
    Warn,
    Suggest,
    Explore,
    Constrain,
    Request,
    Share,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriveUrgency {
    Low,
    Medium,
    High,
    Critical,
}

impl Default for DriveUrgency {
    fn default() -> Self {
        DriveUrgency::Medium
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriveStatus {
    Pending,
    Delivered,
    Executed,
    Rejected,
    Expired,
}

pub fn default_status() -> DriveStatus {
    DriveStatus::Pending
}

/// Phase 2: 根据 urgency 计算默认 TTL(unix 秒), 信号创建时自动赋期
/// Critical: 24h, High: 3天, Medium: 7天, Low: 14天
pub fn default_expires_at(urgency: &DriveUrgency) -> Option<i64> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let ttl_secs: i64 = match urgency {
        DriveUrgency::Critical => 86400,   // 1 天
        DriveUrgency::High => 3 * 86400,   // 3 天
        DriveUrgency::Medium => 7 * 86400, // 7 天
        DriveUrgency::Low => 14 * 86400,   // 14 天
    };
    Some(now + ttl_secs)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EmotionSnapshot {
    pub pleasure: f64,
    pub arousal: f64,
    pub dominance: f64,
    pub label: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DriveFeedback {
    pub responded_at: i64,
    pub executed: bool,
    pub outcome: String,
    #[serde(default)]
    pub reflection: Option<String>,
}

pub fn will_content_closed(text: &str) -> bool {
    let t = text.to_lowercase();
    t.contains("superseded")
        || t.contains("already closed")
        || t.contains("already done")
        || t.contains("terminal-locked")
        || t.contains("anti-redundancy")
        || t.contains("no-op")
        || text.contains("已闭环")
        || text.contains("意图已闭环")
        || text.contains("闭环完成")
        || text.contains("闭环由")
        || text.contains("去重闭环")
        || text.contains("不再单独维护")
        || text.contains("已测签")
        || text.contains("不应再")
}

pub fn strip_canned_will_prefix(desc: &str) -> Option<&str> {
    const PHRASES: &[&str] = &[
        "may need follow-through:",
        "has actionable potential:",
        "suggests an action item:",
        "needs implementation:",
        "may need attention:",
        "in warn category:",
    ];
    let lower = desc.to_lowercase();
    let mut cut = 0usize;
    for p in PHRASES {
        if let Some(i) = lower.find(p) {
            let end = i + p.len();
            if end > cut {
                cut = end;
            }
        }
    }
    if cut == 0 {
        None
    } else {
        Some(desc[cut..].trim())
    }
}

pub fn will_has_action(text: &str) -> bool {
    let t = text.to_lowercase();
    const EN: &[&str] = &[
        "implement",
        "fix",
        "add ",
        "deploy",
        "remove",
        "change",
        "migrate",
        "should",
        "must",
        "todo",
        "next:",
        "next ",
        "need to",
        "needs to",
        "repair",
        "patch",
    ];
    const ZH: &[&str] = &[
        "需要",
        "应当",
        "必须",
        "落地",
        "实现",
        "修复",
        "修改",
        "部署",
        "删除",
        "增加",
        "下一刀",
        "待办",
        "未做",
        "要做",
        "补上",
        "补测",
    ];
    EN.iter().any(|k| t.contains(k)) || ZH.iter().any(|k| text.contains(k))
}

pub fn will_text_reject(description: &str) -> Option<&'static str> {
    let desc = description.trim();
    if desc.is_empty() {
        return Some("empty");
    }
    if will_content_closed(desc) {
        return Some("closed");
    }
    if let Some(rest) = strip_canned_will_prefix(desc) {
        if will_content_closed(rest) {
            return Some("closed");
        }
        if !will_has_action(rest) {
            return Some("canned");
        }
    }
    None
}

pub struct DriveQueue {
    signals: Mutex<Vec<DriveSignal>>,
    next_id: AtomicU64,
    /// Counters for learning: how many signals of each intent were executed vs rejected.
    /// This feeds back into the DriveEngine to adjust future signal generation.
    stats_executed: AtomicU64,
    stats_duplicate_ack: AtomicU64,
    stats_rejected: AtomicU64,
    sweep_total: AtomicU64,
    // D1: push notification for drive signal enqueue
    notify: Arc<tokio::sync::Notify>,
    ingested: Mutex<HashSet<u64>>,
    policy: Mutex<HashMap<String, (u32, u32)>>,
    policy_version: AtomicU64,
}

impl DriveQueue {
    pub fn new() -> Self {
        Self {
            signals: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
            sweep_total: AtomicU64::new(0),
            notify: Arc::new(tokio::sync::Notify::new()),
            stats_executed: AtomicU64::new(0),
            stats_duplicate_ack: AtomicU64::new(0),
            stats_rejected: AtomicU64::new(0),
            ingested: Mutex::new(HashSet::new()),
            policy: Mutex::new(HashMap::new()),
            policy_version: AtomicU64::new(1),
        }
    }

    /// D1: Get notify handle for SSE push subscription
    pub fn notify_handle(&self) -> Arc<tokio::sync::Notify> {
        self.notify.clone()
    }
    pub fn enqueue(&self, mut signal: DriveSignal) -> u64 {
        // 审计修复: id 碰撞防御 — 若 Vec 已有该 id(异常竞态), 自旋跳到空闲 id
        loop {
            let candidate = self.next_id.fetch_add(1, Ordering::SeqCst);
            let exists = self.signals.lock().iter().any(|s| s.id == candidate);
            if !exists {
                signal.id = candidate;
                break;
            }
        }
        signal.status = DriveStatus::Pending;
        signal.enqueued_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let id = signal.id;
        let mut signals = self.signals.lock();
        if signals.len() >= 2000 {
            // Cap修复: 只清终态(Executed/Rejected/Expired)中最旧的, 不清活的(Pending/Delivered)
            // 之前清Pending导致新信号被吃 → inbox幽灵空
            let mut removed = 0;
            signals.retain(|s| {
                let is_terminal = matches!(
                    s.status,
                    DriveStatus::Executed | DriveStatus::Rejected | DriveStatus::Expired
                );
                if removed < 100 && is_terminal {
                    removed += 1;
                    false
                } else {
                    true
                }
            });
        }
        let desc: String = signal.description.chars().take(80).collect();
        let itype = signal.intent_type.clone();
        let urg = signal.urgency.clone();
        signals.push(signal);
        self.notify.notify_waiters();
        tracing::info!(
            "[Drive] signal #{} enqueued: {:?} urgency={:?} desc={}",
            id,
            itype,
            urg,
            desc
        );
        id
    }

    pub fn poll(&self, limit: usize) -> Vec<DriveSignal> {
        let mut signals = self.signals.lock();
        let mut to_deliver = Vec::new();
        for signal in signals.iter_mut() {
            if matches!(signal.status, DriveStatus::Pending) {
                signal.status = DriveStatus::Delivered;
                to_deliver.push(signal.clone());
                if to_deliver.len() >= limit {
                    break;
                }
            }
        }
        if !to_deliver.is_empty() {
            let mut ing = self.ingested.lock();
            for s in &to_deliver {
                ing.insert(s.id);
            }
        }
        let now = chrono::Utc::now().timestamp();
        signals.retain(|s| {
            let age = now - s.timestamp;
            age < 86400 || matches!(s.status, DriveStatus::Pending | DriveStatus::Delivered)
        });
        to_deliver
    }

    /// Peek at pending signals WITHOUT changing their status.
    /// Used by self-driving loop to decide which signals to consume.
    /// Signals that are NOT consumed by self-driving remain Pending
    /// and will be visible to external agents via poll().
    /// Phase 2 TTL 统一 sweep: 扫描所有 pending/delivered 信号, 过期的标记为 Expired。
    /// 在 inbox/stats/restore 等所有读操作前执行, 保证过期信号不遗漏。
    fn sweep_expired(&self) {
        let mut signals = self.signals.lock();
        let now = Self::now_ts();
        let mut expired_count = 0u32;
        for s in signals.iter_mut() {
            if matches!(s.status, DriveStatus::Pending | DriveStatus::Delivered) {
                if let Some(exp) = s.expires_at {
                    if now > exp {
                        s.status = DriveStatus::Expired;
                        expired_count += 1;
                        self.sweep_total
                            .fetch_add(expired_count as u64, std::sync::atomic::Ordering::Relaxed);
                        tracing::info!(
                            "[Drive] signal #{} expired via sweep (TTL {}s ago)",
                            s.id,
                            now - exp
                        );
                    }
                }
            }
        }
        if expired_count > 0 {
            self.sweep_total
                .fetch_add(expired_count as u64, std::sync::atomic::Ordering::Relaxed);
            tracing::info!(
                "[Drive] sweep_expired: {} signals -> Expired (total: {})",
                expired_count,
                self.sweep_total.load(std::sync::atomic::Ordering::Relaxed)
            );
        }
    }

    pub fn peek_pending(&self, limit: usize) -> Vec<DriveSignal> {
        self.sweep_expired();
        let signals = self.signals.lock();
        signals
            .iter()
            .filter(|s| matches!(s.status, DriveStatus::Pending))
            .take(limit)
            .cloned()
            .collect()
    }

    /// Peek at unacked signals (Pending + Delivered, but not yet Executed/Rejected).
    /// Used by throttle to prevent re-emitting signals that haven't been acked yet.
    pub fn peek_unacked(&self, limit: usize) -> Vec<DriveSignal> {
        self.sweep_expired();
        let signals = self.signals.lock();
        // P1-6: 新优先 — Pending 排前面, 然后按 ID 降序 (最新的 delivered 先看到)
        let mut unacked: Vec<&DriveSignal> = signals
            .iter()
            .filter(|s| matches!(s.status, DriveStatus::Pending | DriveStatus::Delivered))
            .collect();
        // Pending 优先, 同状态内按 ID 降序
        unacked.sort_by(|a, b| {
            let a_pending = matches!(a.status, DriveStatus::Pending);
            let b_pending = matches!(b.status, DriveStatus::Pending);
            b_pending.cmp(&a_pending).then_with(|| b.id.cmp(&a.id))
        });
        unacked.into_iter().take(limit).cloned().collect()
    }

    /// Mark a signal as consumed (skip Delivered state — go straight to Executed/Rejected).
    /// Used by self-driving after processing a signal.
    /// Phase 2: 最大重试次数, 超过则进 dead-letter(Expired)
    const MAX_RETRIES: u32 = 3;
    pub fn max_retries() -> u32 {
        Self::MAX_RETRIES
    }

    pub fn mark_consumed(&self, drive_id: u64, feedback: DriveFeedback) -> (bool, bool) {
        let mut signals = self.signals.lock();
        for signal in signals.iter_mut() {
            if signal.id == drive_id {
                let is_terminal = matches!(
                    signal.status,
                    DriveStatus::Executed | DriveStatus::Rejected | DriveStatus::Expired
                );
                if is_terminal {
                    self.stats_duplicate_ack.fetch_add(1, Ordering::SeqCst);
                    tracing::debug!(
                        "[Drive] signal #{} idempotent skip (already {:?})",
                        drive_id,
                        signal.status
                    );
                    return (true, false);
                }
                if feedback.executed {
                    signal.status = DriveStatus::Executed;
                    self.stats_executed.fetch_add(1, Ordering::SeqCst);
                } else {
                    signal.retry_count += 1;
                    if signal.retry_count <= Self::MAX_RETRIES {
                        signal.status = DriveStatus::Pending;
                    } else {
                        // P1闸接线: 拒绝耗尽 → Rejected(死信专态)。曾进Expired与TTL过期混同,
                        // Rejected枚举从未被产生("死状态") — evidence_done 无法识别"被执行端否决"
                        signal.status = DriveStatus::Rejected;
                        self.stats_rejected.fetch_add(1, Ordering::SeqCst);
                    }
                }
                signal.feedback = Some(feedback.clone());
                self.learn_outcome(&signal.intent_type, &signal.evidence, feedback.executed);
                return (true, true);
            }
        }
        (false, false)
    }

    pub fn acknowledge(&self, drive_id: u64, feedback: DriveFeedback) -> (bool, bool) {
        let (result, first_ack) = self.mark_consumed(drive_id, feedback.clone());
        if result && first_ack {
            tracing::info!(
                "[Drive] signal #{} acknowledged: executed={}",
                drive_id,
                feedback.executed
            );
        }
        (result, first_ack)
    }

    pub fn stats(&self) -> serde_json::Value {
        self.sweep_expired();
        let signals = self.signals.lock();
        let pending = signals
            .iter()
            .filter(|s| matches!(s.status, DriveStatus::Pending))
            .count();
        let delivered = signals
            .iter()
            .filter(|s| matches!(s.status, DriveStatus::Delivered))
            .count();
        let executed = signals
            .iter()
            .filter(|s| matches!(s.status, DriveStatus::Executed))
            .count();
        let rejected = signals
            .iter()
            .filter(|s| matches!(s.status, DriveStatus::Rejected))
            .count();
        let expired = signals
            .iter()
            .filter(|s| matches!(s.status, DriveStatus::Expired))
            .count();
        let retrying = signals
            .iter()
            .filter(|s| s.retry_count > 0 && matches!(s.status, DriveStatus::Pending))
            .count();
        // P4-5: 区分 dead-letter (retry exhausted) vs TTL-expired
        let dead_letter_count = signals
            .iter()
            .filter(|s| {
                matches!(s.status, DriveStatus::Rejected)
                    || (matches!(s.status, DriveStatus::Expired)
                        && s.retry_count > Self::MAX_RETRIES as u32)
            })
            .count();
        let ttl_expired_count = signals
            .iter()
            .filter(|s| matches!(s.status, DriveStatus::Expired) && s.retry_count == 0)
            .count();
        let unique_executed = executed; // 当前无去重计数器，executed 本身就是唯一
        let duplicate_ack_suppressed = self.stats_duplicate_ack.load(Ordering::SeqCst);
        serde_json::json!({
            "pending": pending, "delivered": delivered,
            "executed": executed, "rejected": rejected,
            "ingested": self.ingested.lock().len(),
            "policy_version": self.policy_version.load(Ordering::SeqCst),
            "policy_bins": self.policy.lock().len(),
            "policy_suppressed": self.policy_stats().1,
            "policy_success": self.policy.lock().values().map(|b| b.0).sum::<u32>(),
            "policy_fail": self.policy.lock().values().map(|b| b.1).sum::<u32>(),
            "expired": expired, "retrying": retrying,
            "total": signals.len(),
            "max_retries": Self::MAX_RETRIES,
            "dead_letter_count": dead_letter_count,
            "ttl_expired_count": ttl_expired_count,
            "unique_executed": unique_executed,
            "duplicate_ack_suppressed": duplicate_ack_suppressed,
            "retry_attempts": signals.iter().map(|s| s.retry_count as u64).sum::<u64>(),
            "sweep_applied": self.sweep_total.load(Ordering::Relaxed),
        })
    }

    /// Phase 2: 当前 unix 时间戳(秒)
    fn now_ts() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
    }

    /// Phase 3 P0-3: 获取累计 sweep 过期数（契约 #1658）
    pub fn sweep_applied(&self) -> u64 {
        self.sweep_total.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn snapshot(&self) -> Vec<DriveSignal> {
        self.signals.lock().clone()
    }

    pub fn record_ingested(&self, ids: &[u64]) {
        let mut ing = self.ingested.lock();
        for id in ids {
            if *id > 0 {
                ing.insert(*id);
            }
        }
    }

    pub fn ingested_ids(&self) -> Vec<u64> {
        let mut v: Vec<u64> = self.ingested.lock().iter().copied().collect();
        v.sort_unstable();
        v
    }

    pub fn restore_ingested(&self, ids: Vec<u64>) {
        let mut ing = self.ingested.lock();
        *ing = ids.into_iter().collect();
    }

    pub fn fingerprint(intent: &DriveIntent, evidence: &[u64]) -> String {
        let mut ev: Vec<u64> = evidence.to_vec();
        ev.sort_unstable();
        format!(
            "{:?}:{}",
            intent,
            ev.iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(",")
        )
    }

    pub fn has_live_fingerprint(&self, intent: &DriveIntent, evidence: &[u64]) -> bool {
        let fp = Self::fingerprint(intent, evidence);
        self.signals.lock().iter().any(|s| {
            matches!(s.status, DriveStatus::Pending | DriveStatus::Delivered)
                && Self::fingerprint(&s.intent_type, &s.evidence) == fp
        })
    }

    pub fn evidence_done(&self, evidence: &[u64]) -> bool {
        if evidence.is_empty() {
            return false;
        }
        let set: HashSet<u64> = evidence.iter().copied().collect();
        // P1闸接线: Rejected 也是已裁决 — 被执行端明确拒绝的意志不得凭同一证据立即重生
        // (曾只认 Executed, 拒绝后同证据可反复重生直到 policy fail>=3 才消停)
        self.signals.lock().iter().any(|s| {
            matches!(s.status, DriveStatus::Executed | DriveStatus::Rejected)
                && s.evidence.iter().any(|e| set.contains(e))
        })
    }

    pub fn should_birth(
        &self,
        intent: &DriveIntent,
        evidence: &[u64],
        description: &str,
    ) -> Result<(), &'static str> {
        if let Some(why) = will_text_reject(description) {
            return Err(why);
        }
        if self.has_live_fingerprint(intent, evidence) {
            return Err("pending");
        }
        if self.evidence_done(evidence) {
            return Err("executed");
        }
        if !self.should_emit(intent, evidence) {
            return Err("policy");
        }
        Ok(())
    }

    pub fn should_emit(&self, intent: &DriveIntent, evidence: &[u64]) -> bool {
        let fp = Self::fingerprint(intent, evidence);
        let pol = self.policy.lock();
        match pol.get(&fp) {
            Some(&(success, _)) if success >= 1 && !evidence.is_empty() => false,
            Some(&(success, fail)) if fail >= 3 && fail > success.saturating_mul(2) => false,
            _ => true,
        }
    }

    pub fn learn_outcome(&self, intent: &DriveIntent, evidence: &[u64], executed: bool) {
        let fp = Self::fingerprint(intent, evidence);
        let mut pol = self.policy.lock();
        let entry = pol.entry(fp).or_insert((0, 0));
        if executed {
            entry.0 = entry.0.saturating_add(1);
        } else {
            entry.1 = entry.1.saturating_add(1);
        }
        self.policy_version.fetch_add(1, Ordering::SeqCst);
    }

    pub fn policy_version(&self) -> u64 {
        self.policy_version.load(Ordering::SeqCst)
    }

    pub fn set_policy_version(&self, v: u64) {
        self.policy_version.store(v.max(1), Ordering::SeqCst);
    }

    pub fn policy_snapshot(&self) -> HashMap<String, (u32, u32)> {
        self.policy.lock().clone()
    }

    pub fn restore_policy(&self, p: HashMap<String, (u32, u32)>) {
        *self.policy.lock() = p;
    }

    pub fn policy_stats(&self) -> (usize, usize) {
        let pol = self.policy.lock();
        let suppressed = pol
            .values()
            .filter(|(s, f)| *f >= 3 && *f > s.saturating_mul(2))
            .count();
        (pol.len(), suppressed)
    }

    pub fn restore(&self, signals: Vec<DriveSignal>) {
        let count = signals.len();
        let mut queue = self.signals.lock();
        *queue = signals;
        let max_id = queue.iter().map(|s| s.id).max().unwrap_or(0);
        // 审计修复: next_id 只升不降 — 防止 reload 竞态下 id 回卷复用
        // (同 id 双 signal 互相覆盖: Suggest 被 Explore 顶掉的幽灵丢失)
        let cur = self.next_id.load(Ordering::SeqCst);
        self.next_id.store(cur.max(max_id + 1), Ordering::SeqCst);
        drop(queue);
        // Phase 2: restore 后立即 sweep, 重启后过期信号直接转 Expired
        self.sweep_expired();
        tracing::info!(
            "[Drive] restore: {} signals loaded, sweep_expired applied",
            count
        );
    }
}

#[cfg(test)]
mod will_valve_tests {
    use super::*;

    fn sig(desc: &str, ev: Vec<u64>) -> DriveSignal {
        DriveSignal {
            id: 0,
            timestamp: 0,
            intent_type: DriveIntent::Suggest,
            description: desc.to_string(),
            evidence: ev,
            urgency: DriveUrgency::Medium,
            target_capability: None,
            emotion: None,
            origin_tick: 0,
            status: DriveStatus::Pending,
            feedback: None,
            retry_count: 0,
            expires_at: None,
            enqueued_at_ms: 0,
            time_budget_ms: None,
        }
    }

    #[test]
    fn reject_closed_superseded() {
        assert_eq!(
            will_text_reject("Architecture memory #6213 (importance 2.0) has actionable potential: already superseded, 不再单独维护"),
            Some("closed")
        );
    }

    #[test]
    fn reject_canned_decision_title() {
        assert_eq!(
            will_text_reject("Decision memory #6218 may need follow-through: [decision] | title: Will-filter valve sits at birth"),
            Some("canned")
        );
    }

    #[test]
    fn allow_canned_with_real_action() {
        assert_eq!(
            will_text_reject("Decision memory #1 may need follow-through: 下一刀落地出生口阀"),
            None
        );
    }

    #[test]
    fn reject_empty() {
        assert_eq!(will_text_reject("   "), Some("empty"));
    }

    #[test]
    fn queue_blocks_live_and_executed() {
        let q = DriveQueue::new();
        let id = q.enqueue(sig("下一刀实现过滤阀", vec![77]));
        assert_eq!(
            q.should_birth(&DriveIntent::Suggest, &[77], "下一刀实现过滤阀"),
            Err("pending")
        );
        q.acknowledge(
            id,
            DriveFeedback {
                responded_at: 1,
                executed: true,
                outcome: "done".into(),
                reflection: None,
            },
        );
        assert_eq!(
            q.should_birth(&DriveIntent::Suggest, &[77], "下一刀实现过滤阀"),
            Err("executed")
        );
        assert!(q
            .should_birth(&DriveIntent::Suggest, &[88], "下一刀实现另一件事")
            .is_ok());
        assert!(!q.should_emit(&DriveIntent::Suggest, &[77]));
    }

    #[test]
    fn success_closes_nonempty_evidence() {
        let q = DriveQueue::new();
        q.learn_outcome(&DriveIntent::Explore, &[9], true);
        assert!(!q.should_emit(&DriveIntent::Explore, &[9]));
        assert!(q.should_emit(&DriveIntent::Suggest, &[]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejected_evidence_blocks_rebirth() {
        // P1闸接线: 执行端明确拒绝后, 同证据不得立即重生(evidence_done 纳入 Rejected)
        let q = DriveQueue::new();
        let sig = DriveSignal {
            id: 0,
            timestamp: 0,
            intent_type: DriveIntent::Suggest,
            description: "test rejected rebirth".into(),
            evidence: vec![42],
            urgency: DriveUrgency::Medium,
            target_capability: None,
            emotion: None,
            origin_tick: 0,
            status: default_status(),
            feedback: None,
            retry_count: 0,
            expires_at: None,
            enqueued_at_ms: 0,
            time_budget_ms: None,
        };
        let id = q.enqueue(sig);
        // 拒绝 MAX_RETRIES+1 次 → 前3次回Pending重试, 第4次进Rejected(死信)
        for _ in 0..=DriveQueue::MAX_RETRIES {
            q.mark_consumed(
                id,
                DriveFeedback {
                    responded_at: 0,
                    executed: false,
                    outcome: "dismissed".into(),
                    reflection: None,
                },
            );
        }
        assert!(
            q.evidence_done(&[42]),
            "rejected evidence should count as adjudicated"
        );
        let verdict = q.should_birth(&DriveIntent::Suggest, &[42], "another desc");
        assert!(
            verdict.is_err(),
            "rebirth after rejection must be blocked: {:?}",
            verdict
        );
    }
}
