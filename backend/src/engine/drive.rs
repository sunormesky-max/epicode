use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::domain::tetra::MemoryPayload;

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
#[allow(dead_code)] // 字段为下游分析保留
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
#[allow(dead_code)] // 观察快照字段, 部分暂未被读
struct ObserveState {
    tetra_count: usize,
    cluster_count: usize,
    avg_entropy: f64,
    energy_ratio: f64,
    unexplored_ratio: f64,
    redundancy_ratio: f64,
}

impl Default for DriveEngine {
    fn default() -> Self {
        Self::new()
    }
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
    /// Optional, versioned pointers to the persistent memories that justify this signal.
    /// This intentionally contains no memory text and no confidence score.
    #[serde(default)]
    pub grounding: Option<DriveGrounding>,
    #[serde(default)]
    pub terminal_reason: Option<DriveTerminalReason>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct DriveGrounding {
    pub reason: String,
    pub evidence: Vec<DriveMemoryEvidence>,
    #[serde(default)]
    pub uncertainty: Vec<DriveGroundingUncertainty>,
    #[serde(default)]
    pub fresh_until: Option<i64>,
    /// False when one or more evidence IDs are not known persistent-memory IDs.
    #[serde(default)]
    pub complete: bool,
}

impl DriveGrounding {
    pub fn is_fresh_at(&self, now: i64) -> bool {
        self.fresh_until.is_none_or(|deadline| now < deadline)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct DriveMemoryEvidence {
    pub id: u64,
    /// Stable fingerprint of content and eligibility metadata, not a confidence value.
    pub revision: String,
    pub recorded_at: i64,
    #[serde(default)]
    pub last_reviewed_at: Option<i64>,
    pub importance: f64,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DriveGroundingUncertainty {
    SingleMemorySource,
    NotReviewed,
    KnownConflict,
    UnresolvedEvidence,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DriveTerminalReason {
    TtlExpired,
    EvidenceStale,
    RetriesExhausted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveOutcomeSentiment {
    Positive,
    Negative,
    Neutral,
}

impl DriveOutcomeSentiment {
    pub fn reward(self) -> f64 {
        match self {
            Self::Positive => 5.0,
            Self::Negative => -3.0,
            Self::Neutral => 1.0,
        }
    }
}

pub fn memory_is_current(memory: &MemoryPayload, now: i64) -> bool {
    memory.timestamp <= now
        && memory.importance.is_finite()
        && memory.valid_to.is_none()
        && memory.valid_from <= now
        && memory.expired_at.is_none_or(|expired_at| expired_at > now)
        && memory
            .invalidated_at
            .is_none_or(|invalidated_at| invalidated_at > now)
}

pub fn memory_is_recent(memory: &MemoryPayload, now: i64, window_secs: i64) -> bool {
    memory.timestamp <= now
        && memory.timestamp > now.saturating_sub(window_secs)
        && memory_is_current(memory, now)
}

pub fn memory_revision(memory: &MemoryPayload) -> u64 {
    fn update(hash: &mut u64, bytes: &[u8]) {
        for byte in bytes {
            *hash ^= u64::from(*byte);
            *hash = hash.wrapping_mul(0x100000001b3);
        }
    }

    fn update_optional_i64(hash: &mut u64, value: Option<i64>) {
        match value {
            Some(value) => {
                update(hash, &[1]);
                update(hash, &value.to_le_bytes());
            }
            None => update(hash, &[0]),
        }
    }

    fn update_optional_string(hash: &mut u64, value: Option<&str>) {
        match value {
            Some(value) => {
                update(hash, &[1]);
                update(hash, &(value.len() as u64).to_le_bytes());
                update(hash, value.as_bytes());
            }
            None => update(hash, &[0]),
        }
    }

    // FNV-1a keeps the persisted fingerprint stable across process restarts.
    let mut hash = 0xcbf29ce484222325;
    update(&mut hash, &memory.content_hash.to_le_bytes());
    update(&mut hash, &memory.timestamp.to_le_bytes());
    update(&mut hash, &memory.valid_from.to_le_bytes());
    update_optional_i64(&mut hash, memory.valid_to);
    update_optional_i64(&mut hash, memory.expired_at);
    update_optional_i64(&mut hash, memory.invalidated_at);
    update(&mut hash, &memory.importance.to_bits().to_le_bytes());
    let mut labels = memory.labels.clone();
    labels.sort_unstable();
    for label in labels {
        update(&mut hash, &(label.len() as u64).to_le_bytes());
        update(&mut hash, label.as_bytes());
    }
    update_optional_string(&mut hash, memory.memory_type.as_deref());
    update_optional_string(&mut hash, memory.memory_class.as_deref());
    hash
}

impl DriveSignal {
    pub fn is_retryable(&self) -> bool {
        matches!(self.status, DriveStatus::Pending | DriveStatus::Delivered)
    }

    fn description_transport_fields(
        &self,
        e2e_public_key: Option<&str>,
    ) -> (serde_json::Value, Option<String>) {
        match e2e_public_key {
            Some(public_key) => {
                match super::e2e::encrypt_for(self.description.as_bytes(), public_key) {
                    Ok(ciphertext) => (serde_json::Value::Null, Some(ciphertext)),
                    Err(error) => {
                        tracing::warn!("[γ2] drive description encryption failed; falling back to plaintext: {}", error);
                        (serde_json::json!(self.description), None)
                    }
                }
            }
            None => (serde_json::json!(self.description), None),
        }
    }

    pub fn inbox_value(&self) -> serde_json::Value {
        self.inbox_value_with_e2e(None)
    }

    pub fn inbox_value_with_e2e(&self, e2e_public_key: Option<&str>) -> serde_json::Value {
        let mut value = serde_json::to_value(self)
            .expect("DriveSignal serialization is infallible for this struct");
        let (description, encrypted_description) =
            self.description_transport_fields(e2e_public_key);
        let (grounding, encrypted_grounding) = self.grounding_transport_fields(e2e_public_key);
        if let Some(fields) = value.as_object_mut() {
            fields.insert("description".to_string(), description);
            if let Some(ciphertext) = encrypted_description {
                fields.insert("description_e2e".to_string(), serde_json::json!(ciphertext));
            }
            fields.insert("grounding".to_string(), grounding);
            if let Some(ciphertext) = encrypted_grounding {
                fields.insert("grounding_e2e".to_string(), serde_json::json!(ciphertext));
            }
            fields.insert(
                "retryable".to_string(),
                serde_json::json!(self.is_retryable()),
            );
        }
        value
    }

    fn grounding_transport_fields(
        &self,
        e2e_public_key: Option<&str>,
    ) -> (serde_json::Value, Option<String>) {
        let Some(grounding) = &self.grounding else {
            return (serde_json::Value::Null, None);
        };
        let Some(public_key) = e2e_public_key else {
            return (
                serde_json::to_value(grounding)
                    .expect("DriveGrounding serialization is infallible for this struct"),
                None,
            );
        };
        let plaintext = match serde_json::to_vec(grounding) {
            Ok(plaintext) => plaintext,
            Err(error) => {
                tracing::error!("[γ2] drive grounding serialization failed: {}", error);
                return (serde_json::Value::Null, None);
            }
        };
        match super::e2e::encrypt_for(&plaintext, public_key) {
            Ok(ciphertext) => (serde_json::Value::Null, Some(ciphertext)),
            Err(error) => {
                tracing::error!("[γ2] drive grounding encryption failed: {}", error);
                (serde_json::Value::Null, None)
            }
        }
    }

    pub fn sse_value(&self, e2e_public_key: Option<&str>) -> serde_json::Value {
        let (description, encrypted_description) =
            self.description_transport_fields(e2e_public_key);
        let (grounding, encrypted_grounding) = self.grounding_transport_fields(e2e_public_key);
        serde_json::json!({
            "id": self.id,
            "intent_type": self.intent_type,
            "status": self.status,
            "urgency": self.urgency,
            "description": description,
            "description_e2e": encrypted_description,
            "evidence": self.evidence,
            "grounding": grounding,
            "grounding_e2e": encrypted_grounding,
            "enqueued_at_ms": self.enqueued_at_ms,
        })
    }
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

impl DriveIntent {
    /// Keep outcome learning tied to the drive that produced the acknowledged signal.
    pub fn feedback_target(&self) -> Option<(Drive, f64)> {
        match self {
            Self::Warn => Some((Drive::Vitality, 1.0)),
            Self::Suggest => Some((Drive::Coherence, 0.7)),
            Self::Explore => Some((Drive::Curiosity, 0.5)),
            Self::Constrain => Some((Drive::Efficiency, 0.3)),
            Self::Request | Self::Share => None,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum DriveUrgency {
    Low,
    #[default]
    Medium,
    High,
    Critical,
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

/// The queue totals cannot reveal who executed a signal. Report only facts
/// visible from the current inbox and retained queue entries.
pub fn inbox_empty_reason(signal_count: usize, stats: &serde_json::Value) -> &'static str {
    if signal_count > 0 {
        "has_signals"
    } else if stats.get("total").and_then(|value| value.as_u64()) == Some(0) {
        "no_signals"
    } else {
        "no_pending"
    }
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

impl DriveFeedback {
    pub fn sentiment(&self) -> DriveOutcomeSentiment {
        Self::sentiment_for_outcome(&self.outcome)
    }

    pub fn sentiment_for_outcome(outcome: &str) -> DriveOutcomeSentiment {
        let outcome = outcome.to_lowercase();
        let positive = [
            "success",
            "done",
            "completed",
            "effective",
            "helpful",
            "good",
            "actioned",
            "resolved",
            "处理",
            "完成",
            "有效",
            "采纳",
        ];
        let negative = [
            "ignored",
            "rejected",
            "failed",
            "error",
            "useless",
            "not helpful",
            "unhelpful",
            "not effective",
            "not useful",
            "not done",
            "拒绝",
            "忽略",
            "无效",
        ];
        if negative.iter().any(|word| outcome.contains(word)) {
            DriveOutcomeSentiment::Negative
        } else if positive.iter().any(|word| outcome.contains(word)) {
            DriveOutcomeSentiment::Positive
        } else {
            DriveOutcomeSentiment::Neutral
        }
    }

    pub fn learning_success(&self) -> bool {
        match self.sentiment() {
            DriveOutcomeSentiment::Positive => true,
            DriveOutcomeSentiment::Negative => false,
            DriveOutcomeSentiment::Neutral => self.executed,
        }
    }
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

pub const MAX_QUEUE_SIGNALS: usize = 2_000;

fn urgency_rank(urgency: &DriveUrgency) -> u8 {
    match urgency {
        DriveUrgency::Critical => 3,
        DriveUrgency::High => 2,
        DriveUrgency::Medium => 1,
        DriveUrgency::Low => 0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveEnqueueError {
    QueueFull,
}

pub struct DriveQueue {
    signals: Mutex<Vec<DriveSignal>>,
    next_id: AtomicU64,
    /// Explicit positive/negative outcome bins keyed by intent and evidence.
    /// These feed back into future signal admission.
    stats_executed: AtomicU64,
    stats_duplicate_ack: AtomicU64,
    stats_rejected: AtomicU64,
    sweep_total: AtomicU64,
    capacity_rejected: AtomicU64,
    // D1: push notification for drive signal enqueue
    notify: Arc<tokio::sync::Notify>,
    ingested: Mutex<HashSet<u64>>,
    policy: Mutex<HashMap<String, (u32, u32)>>,
    policy_version: AtomicU64,
}

impl Default for DriveQueue {
    fn default() -> Self {
        Self::new()
    }
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
            capacity_rejected: AtomicU64::new(0),
            ingested: Mutex::new(HashSet::new()),
            policy: Mutex::new(HashMap::new()),
            policy_version: AtomicU64::new(1),
        }
    }

    /// D1: Get notify handle for SSE push subscription
    pub fn notify_handle(&self) -> Arc<tokio::sync::Notify> {
        self.notify.clone()
    }
    /// Compatibility helper for older callers; returns 0 when the queue is full.
    /// New code should use `try_enqueue` so it can observe backpressure directly.
    pub fn enqueue(&self, signal: DriveSignal) -> u64 {
        match self.try_enqueue(signal) {
            Ok(id) => id,
            Err(error) => {
                tracing::warn!("[Drive] signal admission rejected: {:?}", error);
                0
            }
        }
    }

    pub fn try_enqueue(&self, mut signal: DriveSignal) -> Result<u64, DriveEnqueueError> {
        let mut signals = self.signals.lock();
        if signals.len() >= MAX_QUEUE_SIGNALS {
            self.capacity_rejected.fetch_add(1, Ordering::Relaxed);
            return Err(DriveEnqueueError::QueueFull);
        }
        loop {
            let candidate = self.next_id.fetch_add(1, Ordering::SeqCst);
            if !signals.iter().any(|s| s.id == candidate) {
                signal.id = candidate;
                break;
            }
        }
        signal.status = DriveStatus::Pending;
        signal.terminal_reason = None;
        signal.enqueued_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let id = signal.id;
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
        Ok(id)
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
                        s.terminal_reason = Some(DriveTerminalReason::TtlExpired);
                        expired_count += 1;
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
        self.peek_pending_matching(limit, |_| true)
    }

    /// Apply the consumer filter before the limit so unrelated pending work cannot starve it.
    pub(crate) fn peek_pending_matching(
        &self,
        limit: usize,
        predicate: impl Fn(&DriveSignal) -> bool,
    ) -> Vec<DriveSignal> {
        self.sweep_expired();
        let signals = self.signals.lock();
        signals
            .iter()
            .filter(|s| matches!(s.status, DriveStatus::Pending) && predicate(s))
            .take(limit)
            .cloned()
            .collect()
    }

    /// Peek at unacked signals (Pending + Delivered, but not yet Executed/Rejected).
    /// Used by throttle to prevent re-emitting signals that haven't been acked yet.
    pub fn peek_unacked(&self, limit: usize) -> Vec<DriveSignal> {
        self.sweep_expired();
        let signals = self.signals.lock();
        // Keep pending ahead of delivered. Within each status, cycle through urgency
        // classes from highest to lowest and use FIFO within each class.
        let unacked: Vec<&DriveSignal> = signals
            .iter()
            .filter(|s| matches!(s.status, DriveStatus::Pending | DriveStatus::Delivered))
            .collect();
        let mut ordered = Vec::with_capacity(unacked.len());
        for pending_status in [true, false] {
            let mut urgency_buckets: [Vec<&DriveSignal>; 4] = std::array::from_fn(|_| Vec::new());
            for signal in &unacked {
                if matches!(signal.status, DriveStatus::Pending) == pending_status {
                    urgency_buckets[urgency_rank(&signal.urgency) as usize].push(signal);
                }
            }
            for bucket in &mut urgency_buckets {
                bucket.sort_by(|a, b| {
                    a.enqueued_at_ms
                        .cmp(&b.enqueued_at_ms)
                        .then_with(|| a.id.cmp(&b.id))
                });
            }
            let rounds = urgency_buckets
                .iter()
                .map(Vec::len)
                .max()
                .unwrap_or_default();
            for index in 0..rounds {
                for urgency in (0..urgency_buckets.len()).rev() {
                    if let Some(signal) = urgency_buckets[urgency].get(index) {
                        ordered.push(*signal);
                    }
                }
            }
        }
        ordered.into_iter().take(limit).cloned().collect()
    }

    pub fn get_signal(&self, drive_id: u64) -> Option<DriveSignal> {
        self.sweep_expired();
        self.signals
            .lock()
            .iter()
            .find(|signal| signal.id == drive_id)
            .cloned()
    }

    pub fn expire_stale_evidence(&self, drive_ids: &[u64]) -> usize {
        if drive_ids.is_empty() {
            return 0;
        }
        let stale_ids: HashSet<u64> = drive_ids.iter().copied().collect();
        let mut expired = 0;
        let mut signals = self.signals.lock();
        for signal in signals.iter_mut() {
            if stale_ids.contains(&signal.id) && signal.is_retryable() {
                signal.status = DriveStatus::Expired;
                signal.terminal_reason = Some(DriveTerminalReason::EvidenceStale);
                expired += 1;
            }
        }
        expired
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
                    signal.terminal_reason = None;
                    self.stats_executed.fetch_add(1, Ordering::SeqCst);
                } else {
                    signal.retry_count += 1;
                    if signal.retry_count <= Self::MAX_RETRIES {
                        signal.status = DriveStatus::Pending;
                    } else {
                        // P1闸接线: 拒绝耗尽 → Rejected(死信专态)。曾进Expired与TTL过期混同,
                        // Rejected枚举从未被产生("死状态") — evidence_done 无法识别"被执行端否决"
                        signal.status = DriveStatus::Rejected;
                        signal.terminal_reason = Some(DriveTerminalReason::RetriesExhausted);
                        self.stats_rejected.fetch_add(1, Ordering::SeqCst);
                    }
                }
                signal.feedback = Some(feedback.clone());
                self.learn_outcome(
                    &signal.intent_type,
                    &signal.evidence,
                    feedback.learning_success(),
                );
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

    /// Persist old terminal signals before removing them from memory. Hold the queue lock
    /// through the archive write so no concurrent queue change can invalidate the snapshot.
    pub fn archive_archivable(
        &self,
        cutoff_ms: i64,
        archive: impl FnOnce(&[DriveSignal]) -> Result<usize, String>,
    ) -> Result<usize, String> {
        let now = Self::now_ts() * 1000;
        let mut signals = self.signals.lock();
        let archivable: Vec<_> = signals
            .iter()
            .filter(|s| {
                let terminal = matches!(
                    s.status,
                    DriveStatus::Executed | DriveStatus::Rejected | DriveStatus::Expired
                );
                let old = now.saturating_sub(s.enqueued_at_ms) > cutoff_ms;
                terminal && old
            })
            .cloned()
            .collect();
        if archivable.is_empty() {
            return Ok(0);
        }
        let moved = archive(&archivable)?;
        if moved != archivable.len() {
            return Err(format!(
                "archived {moved} of {} drive signals",
                archivable.len()
            ));
        }
        let ids: HashSet<u64> = archivable.iter().map(|s| s.id).collect();
        signals.retain(|s| !ids.contains(&s.id));
        Ok(moved)
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
                        && s.retry_count > Self::MAX_RETRIES)
            })
            .count();
        let ttl_expired_count = signals
            .iter()
            .filter(|s| {
                matches!(s.status, DriveStatus::Expired)
                    && (matches!(s.terminal_reason, Some(DriveTerminalReason::TtlExpired))
                        || (s.terminal_reason.is_none() && s.retry_count == 0))
            })
            .count();
        let stale_evidence_expired = signals
            .iter()
            .filter(|s| matches!(s.terminal_reason, Some(DriveTerminalReason::EvidenceStale)))
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
            "stale_evidence_expired": stale_evidence_expired,
            "unique_executed": unique_executed,
            "duplicate_ack_suppressed": duplicate_ack_suppressed,
            "retry_attempts": signals.iter().map(|s| s.retry_count as u64).sum::<u64>(),
            "sweep_applied": self.sweep_total.load(Ordering::Relaxed),
            "capacity": MAX_QUEUE_SIGNALS,
            "capacity_rejected": self.capacity_rejected.load(Ordering::Relaxed),
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

    pub fn learn_outcome(&self, intent: &DriveIntent, evidence: &[u64], successful: bool) {
        let fp = Self::fingerprint(intent, evidence);
        let mut pol = self.policy.lock();
        let entry = pol.entry(fp).or_insert((0, 0));
        if successful {
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
        self.reserve_next_id(max_id.saturating_add(1));
        drop(queue);
        // Phase 2: restore 后立即 sweep, 重启后过期信号直接转 Expired
        self.sweep_expired();
        tracing::info!(
            "[Drive] restore: {} signals loaded, sweep_expired applied",
            count
        );
    }

    pub fn reserve_next_id(&self, next_id: u64) {
        self.next_id.fetch_max(next_id, Ordering::SeqCst);
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
            grounding: None,
            terminal_reason: None,
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
    fn inbox_empty_reason_does_not_claim_external_feedback_was_self_consumed() {
        let queue = DriveQueue::new();
        assert_eq!(inbox_empty_reason(0, &queue.stats()), "no_signals");

        let executed = queue.enqueue(pending_signal(DriveIntent::Suggest, "External action"));
        let rejected = queue.enqueue(pending_signal(DriveIntent::Warn, "External rejection"));
        let feedback = |executed| DriveFeedback {
            responded_at: 0,
            executed,
            outcome: "reported by the primary executor".into(),
            reflection: None,
        };
        assert_eq!(queue.acknowledge(executed, feedback(true)), (true, true));
        for _ in 0..=DriveQueue::max_retries() {
            assert_eq!(queue.acknowledge(rejected, feedback(false)), (true, true));
        }

        assert!(queue.peek_unacked(10).is_empty());
        let stats = queue.stats();
        assert_eq!(stats["executed"], 1);
        assert_eq!(stats["rejected"], 1);
        assert_eq!(stats["total"], 2);
        assert_eq!(inbox_empty_reason(0, &stats), "no_pending");
        assert_eq!(inbox_empty_reason(1, &stats), "has_signals");
    }

    fn pending_signal(intent_type: DriveIntent, description: &str) -> DriveSignal {
        DriveSignal {
            id: 0,
            timestamp: 0,
            intent_type,
            description: description.to_string(),
            evidence: Vec::new(),
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
            grounding: None,
            terminal_reason: None,
        }
    }

    #[test]
    fn filtered_pending_peek_does_not_starve_later_matching_signals() {
        let queue = DriveQueue::new();
        for index in 0..12 {
            queue.enqueue(pending_signal(
                DriveIntent::Warn,
                &format!("external warning {index}"),
            ));
        }
        let explore_ids: Vec<_> = (0..4)
            .map(|index| {
                queue.enqueue(pending_signal(
                    DriveIntent::Explore,
                    &format!("self-driving exploration {index}"),
                ))
            })
            .collect();

        let selected = queue.peek_pending_matching(3, |signal| {
            matches!(signal.intent_type, DriveIntent::Explore)
        });
        assert_eq!(
            selected.iter().map(|signal| signal.id).collect::<Vec<_>>(),
            explore_ids[..3]
        );

        let external_pending = queue.peek_pending(10);
        assert_eq!(external_pending.len(), 10);
        assert!(external_pending
            .iter()
            .all(|signal| matches!(signal.intent_type, DriveIntent::Warn)));
    }

    #[test]
    fn drive_signal_inbox_json_includes_retryability_and_snake_case_enums() {
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
            status: DriveStatus::Delivered,
            feedback: None,
            retry_count: 0,
            expires_at: Some(1_780_000_100),
            enqueued_at_ms: 1_780_000_000_000,
            time_budget_ms: None,
            grounding: Some(DriveGrounding {
                reason: "recent search-gap memory".into(),
                evidence: vec![DriveMemoryEvidence {
                    id: 10,
                    revision: "0000000000000014".into(),
                    recorded_at: 1_780_000_000,
                    last_reviewed_at: None,
                    importance: 1.0,
                }],
                uncertainty: vec![
                    DriveGroundingUncertainty::SingleMemorySource,
                    DriveGroundingUncertainty::NotReviewed,
                ],
                fresh_until: Some(1_780_003_600),
                complete: true,
            }),
            terminal_reason: None,
        };

        let value = signal.inbox_value();
        assert_eq!(value["intent_type"], "explore");
        assert_eq!(value["status"], "delivered");
        assert_eq!(value["evidence"], serde_json::json!([10, 20]));
        assert_eq!(
            value["grounding"]["uncertainty"],
            serde_json::json!(["single_memory_source", "not_reviewed"])
        );
        assert_eq!(value["retryable"], true);

        let mut terminal = signal;
        terminal.status = DriveStatus::Executed;
        assert_eq!(terminal.inbox_value()["retryable"], false);
    }

    #[test]
    fn drive_inbox_and_sse_encrypt_descriptions_and_grounding() {
        let (public_pem, private_pem) = crate::engine::e2e::test_keypair_pem();
        let signal = DriveSignal {
            id: 8,
            timestamp: 1_780_000_000,
            intent_type: DriveIntent::Warn,
            description: "Private drive details".into(),
            evidence: vec![22],
            urgency: DriveUrgency::High,
            target_capability: None,
            emotion: None,
            origin_tick: 4,
            status: DriveStatus::Pending,
            feedback: None,
            retry_count: 0,
            expires_at: None,
            enqueued_at_ms: 1_780_000_000_100,
            time_budget_ms: None,
            grounding: Some(DriveGrounding {
                reason: "private memory-derived suggestion".into(),
                evidence: vec![DriveMemoryEvidence {
                    id: 22,
                    revision: "0000000000000017".into(),
                    recorded_at: 1_780_000_000,
                    last_reviewed_at: Some(1_780_000_010),
                    importance: 3.0,
                }],
                uncertainty: vec![DriveGroundingUncertainty::SingleMemorySource],
                fresh_until: Some(1_780_000_600),
                complete: true,
            }),
            terminal_reason: None,
        };

        let inbox = signal.inbox_value_with_e2e(Some(&public_pem));
        let sse = signal.sse_value(Some(&public_pem));
        for encrypted in [
            inbox["description_e2e"].as_str().expect("inbox ciphertext"),
            sse["description_e2e"].as_str().expect("SSE ciphertext"),
        ] {
            assert_eq!(
                crate::engine::e2e::decrypt_with(encrypted, &private_pem).unwrap(),
                signal.description.as_bytes()
            );
        }
        let grounding_plaintext = serde_json::to_vec(signal.grounding.as_ref().unwrap()).unwrap();
        for encrypted in [
            inbox["grounding_e2e"]
                .as_str()
                .expect("inbox grounding ciphertext"),
            sse["grounding_e2e"]
                .as_str()
                .expect("SSE grounding ciphertext"),
        ] {
            assert_eq!(
                crate::engine::e2e::decrypt_with(encrypted, &private_pem).unwrap(),
                grounding_plaintext
            );
        }
        assert_eq!(inbox["description"], serde_json::Value::Null);
        assert_eq!(sse["description"], serde_json::Value::Null);
        assert_eq!(inbox["grounding"], serde_json::Value::Null);
        assert_eq!(sse["grounding"], serde_json::Value::Null);
        assert_eq!(inbox["retryable"], true);
    }

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
            grounding: None,
            terminal_reason: None,
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

    #[test]
    fn existing_recency_fixture_excludes_temporally_invalid_memory() {
        let now = 1_800_000_000;
        let memory = |timestamp, importance| MemoryPayload {
            timestamp,
            valid_from: timestamp,
            importance,
            ..MemoryPayload::default()
        };
        let mut fresh = memory(now - 60, 3.0);
        let stale = memory(now - 3_600, 3.0);
        let mut superseded = memory(now - 60, 3.0);
        superseded.valid_to = Some(now - 1);
        let mut expired = memory(now - 60, 3.0);
        expired.expired_at = Some(now - 1);
        let mut invalidated = memory(now - 60, 3.0);
        invalidated.invalidated_at = Some(now - 1);
        let future = memory(now + 60, 3.0);
        let low_importance = memory(now - 60, 2.0);
        fresh.content_hash = 1;

        let fixture = [
            (1, fresh),
            (2, stale),
            (3, superseded),
            (4, expired),
            (5, invalidated),
            (6, future),
            (7, low_importance),
        ];
        let before: Vec<_> = fixture
            .iter()
            .filter(|(_, memory)| {
                memory.timestamp > now - 600
                    && memory.importance >= 2.5
                    && memory.valid_to.is_none()
            })
            .map(|(id, _)| *id)
            .collect();
        let after: Vec<_> = fixture
            .iter()
            .filter(|(_, memory)| memory.importance >= 2.5 && memory_is_recent(memory, now, 600))
            .map(|(id, _)| *id)
            .collect();

        assert_eq!(before, vec![1, 4, 5, 6]);
        assert_eq!(after, vec![1]);
    }

    #[test]
    fn memory_revision_tracks_content_and_relevance_metadata_but_not_review_access() {
        let mut memory = MemoryPayload::default();
        memory.timestamp = 1_800_000_000;
        memory.valid_from = memory.timestamp;
        memory.content_hash = 10;
        memory.labels = vec!["decision".into()];
        memory.importance = 2.5;
        let revision = memory_revision(&memory);

        memory.last_reviewed_ts = Some(memory.timestamp + 10);
        assert_eq!(memory_revision(&memory), revision);
        memory.labels.push("architecture".into());
        assert_ne!(memory_revision(&memory), revision);
    }

    #[test]
    fn grounding_freshness_closes_at_its_source_deadline() {
        let grounding = DriveGrounding {
            reason: "recent decision memory".into(),
            evidence: Vec::new(),
            uncertainty: Vec::new(),
            fresh_until: Some(100),
            complete: true,
        };
        assert!(grounding.is_fresh_at(99));
        assert!(!grounding.is_fresh_at(100));
    }

    #[test]
    fn explicit_outcome_sentiment_overrides_the_executed_flag_for_learning() {
        let not_helpful = DriveFeedback {
            responded_at: 1,
            executed: true,
            outcome: "useless despite being executed".into(),
            reflection: None,
        };
        let helpful = DriveFeedback {
            responded_at: 1,
            executed: false,
            outcome: "helpful but not actioned".into(),
            reflection: None,
        };
        let neutral = DriveFeedback {
            responded_at: 1,
            executed: false,
            outcome: "deferred".into(),
            reflection: None,
        };

        assert!(!not_helpful.learning_success());
        assert!(helpful.learning_success());
        assert!(!neutral.learning_success());
        assert_eq!(
            DriveFeedback::sentiment_for_outcome("later"),
            DriveOutcomeSentiment::Neutral
        );
        assert_eq!(
            DriveFeedback::sentiment_for_outcome("not helpful"),
            DriveOutcomeSentiment::Negative
        );
    }

    #[test]
    fn queue_policy_learns_usefulness_separately_from_execution_status() {
        let queue = DriveQueue::new();
        let mut suggestion = pending_signal(DriveIntent::Suggest, "suggestion");
        suggestion.evidence = vec![10];
        let id = queue.enqueue(suggestion);
        let signal = queue.get_signal(id).unwrap();
        queue.acknowledge(
            id,
            DriveFeedback {
                responded_at: 1,
                executed: true,
                outcome: "useless after execution".into(),
                reflection: None,
            },
        );

        assert_eq!(queue.stats()["policy_success"], 0);
        assert_eq!(queue.stats()["policy_fail"], 1);
        assert!(queue.should_emit(&signal.intent_type, &signal.evidence));
    }

    #[test]
    fn outcome_policy_survives_queue_state_serialization() {
        let queue = DriveQueue::new();
        queue.learn_outcome(&DriveIntent::Suggest, &[10, 20], true);
        let encoded = serde_json::to_string(&queue.policy_snapshot()).unwrap();
        let restored_policy = serde_json::from_str(&encoded).unwrap();
        let restored = DriveQueue::new();
        restored.restore_policy(restored_policy);

        assert!(!restored.should_emit(&DriveIntent::Suggest, &[10, 20]));
        assert!(restored.should_emit(&DriveIntent::Suggest, &[10, 21]));
    }

    #[test]
    fn feedback_rewards_only_the_drive_for_the_signal_intent() {
        assert_eq!(
            DriveIntent::Warn.feedback_target(),
            Some((Drive::Vitality, 1.0))
        );
        assert_eq!(
            DriveIntent::Suggest.feedback_target(),
            Some((Drive::Coherence, 0.7))
        );
        assert_eq!(
            DriveIntent::Explore.feedback_target(),
            Some((Drive::Curiosity, 0.5))
        );
        assert_eq!(
            DriveIntent::Constrain.feedback_target(),
            Some((Drive::Efficiency, 0.3))
        );
        assert_eq!(DriveIntent::Share.feedback_target(), None);
    }

    #[test]
    fn queue_round_robins_urgency_classes_with_fifo_within_each_class() {
        let queue = DriveQueue::new();
        let critical = queue.enqueue(pending_signal(DriveIntent::Warn, "critical"));
        let high_first = queue.enqueue(pending_signal(DriveIntent::Warn, "high old"));
        let high_second = queue.enqueue(pending_signal(DriveIntent::Warn, "high new"));
        let medium = queue.enqueue(pending_signal(DriveIntent::Suggest, "medium"));
        let low = queue.enqueue(pending_signal(DriveIntent::Explore, "low"));
        {
            let mut signals = queue.signals.lock();
            signals
                .iter_mut()
                .find(|s| s.id == critical)
                .unwrap()
                .urgency = DriveUrgency::Critical;
            signals
                .iter_mut()
                .find(|s| s.id == high_first)
                .unwrap()
                .urgency = DriveUrgency::High;
            signals
                .iter_mut()
                .find(|s| s.id == high_second)
                .unwrap()
                .urgency = DriveUrgency::High;
            signals.iter_mut().find(|s| s.id == medium).unwrap().urgency = DriveUrgency::Medium;
            signals.iter_mut().find(|s| s.id == low).unwrap().urgency = DriveUrgency::Low;
            signals
                .iter_mut()
                .find(|s| s.id == high_first)
                .unwrap()
                .enqueued_at_ms = 10;
            signals
                .iter_mut()
                .find(|s| s.id == high_second)
                .unwrap()
                .enqueued_at_ms = 20;
        }

        let ids: Vec<_> = queue
            .peek_unacked(5)
            .into_iter()
            .map(|signal| signal.id)
            .collect();
        assert_eq!(ids, vec![critical, high_first, medium, low, high_second]);
    }

    #[test]
    fn queue_backpressure_enforces_the_existing_hard_capacity_and_preserves_live_work() {
        let queue = DriveQueue::new();
        {
            let mut signals = queue.signals.lock();
            for id in 1..=MAX_QUEUE_SIGNALS as u64 {
                let mut signal = pending_signal(DriveIntent::Warn, "live");
                signal.id = id;
                signals.push(signal);
            }
        }

        assert_eq!(
            queue.try_enqueue(pending_signal(DriveIntent::Suggest, "overflow")),
            Err(DriveEnqueueError::QueueFull)
        );
        assert_eq!(queue.snapshot().len(), MAX_QUEUE_SIGNALS);
        assert_eq!(queue.stats()["capacity"], MAX_QUEUE_SIGNALS);
        assert_eq!(queue.stats()["capacity_rejected"], 1);
    }

    #[test]
    fn queue_rejects_new_work_without_discarding_unarchived_terminal_records() {
        let queue = DriveQueue::new();
        {
            let mut signals = queue.signals.lock();
            for id in 1..=MAX_QUEUE_SIGNALS as u64 {
                let mut signal = pending_signal(DriveIntent::Warn, "signal");
                signal.id = id;
                if id <= 100 {
                    signal.status = DriveStatus::Executed;
                }
                signals.push(signal);
            }
        }
        queue
            .next_id
            .store(MAX_QUEUE_SIGNALS as u64 + 1, Ordering::SeqCst);

        assert_eq!(
            queue.try_enqueue(pending_signal(DriveIntent::Suggest, "new")),
            Err(DriveEnqueueError::QueueFull)
        );
        let remaining = queue.snapshot();
        assert_eq!(remaining.len(), MAX_QUEUE_SIGNALS);
        assert_eq!(remaining[0].id, 1);
        assert!(matches!(remaining[0].status, DriveStatus::Executed));
    }

    #[test]
    fn poll_keeps_old_terminal_records_until_archive_succeeds() {
        let queue = DriveQueue::new();
        let mut old = pending_signal(DriveIntent::Suggest, "old terminal");
        old.id = 1;
        old.timestamp = DriveQueue::now_ts() - 2 * 86400;
        old.enqueued_at_ms = (DriveQueue::now_ts() - 2 * 86400) * 1000;
        old.status = DriveStatus::Executed;
        queue.restore(vec![old]);
        assert!(queue.poll(10).is_empty());
        assert_eq!(queue.snapshot().len(), 1);
    }

    #[test]
    fn archive_failure_keeps_queue_and_active_rows_for_retry() {
        use super::super::storage::StorageManager;

        let dir =
            std::env::temp_dir().join(format!("epicode_drive_archive_{}", uuid::Uuid::new_v4()));
        let storage = StorageManager::new(&dir).unwrap();
        let queue = DriveQueue::new();
        let old_ms = (DriveQueue::now_ts() - 8 * 86400) * 1000;
        let signals: Vec<_> = (1..=2)
            .map(|id| {
                let mut signal = pending_signal(DriveIntent::Suggest, "terminal");
                signal.id = id;
                signal.status = DriveStatus::Executed;
                signal.enqueued_at_ms = old_ms;
                signal
            })
            .collect();
        queue.restore(signals.clone());
        storage.save_drive_signals(&signals).unwrap();

        let conn = rusqlite::Connection::open(storage.db_path()).unwrap();
        conn.execute_batch(
            "CREATE TABLE drive_signals_archive AS SELECT * FROM drive_signals WHERE 1=0;
             CREATE TRIGGER fail_second_archive BEFORE INSERT ON drive_signals_archive
             WHEN NEW.id = 2 BEGIN SELECT RAISE(FAIL, 'injected archive failure'); END;",
        )
        .unwrap();
        let archive = |signals: &[DriveSignal]| storage.save_archived_signals(signals);
        assert!(queue.archive_archivable(7 * 86400 * 1000, archive).is_err());
        assert_eq!(queue.snapshot().len(), 2);
        assert_eq!(storage.load_drive_signals().unwrap().len(), 2);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM drive_signals_archive", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);

        conn.execute("DROP TRIGGER fail_second_archive", [])
            .unwrap();
        assert_eq!(
            queue
                .archive_archivable(7 * 86400 * 1000, |signals| {
                    storage.save_archived_signals(signals)
                })
                .unwrap(),
            2
        );
        assert!(queue.snapshot().is_empty());
        assert!(storage.load_drive_signals().unwrap().is_empty());
        storage.save_archived_signals(&signals).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM drive_signals_archive", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 2);
        let restarted = DriveQueue::new();
        restarted.reserve_next_id(storage.max_drive_signal_id().unwrap() + 1);
        assert_eq!(
            restarted.enqueue(pending_signal(DriveIntent::Suggest, "after restart")),
            3
        );
        let mut reused_id = signals[0].clone();
        reused_id.description = "distinct historical signal".into();
        storage.save_archived_signals(&[reused_id]).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM drive_signals_archive WHERE id=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
        drop(conn);
        drop(storage);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn expired_signal_sweep_counts_each_signal_once() {
        let queue = DriveQueue::new();
        let now = DriveQueue::now_ts();
        for index in 0..3 {
            let mut signal = pending_signal(DriveIntent::Warn, "expires");
            signal.expires_at = Some(now - 1);
            signal.origin_tick = index;
            queue.enqueue(signal);
        }

        queue.sweep_expired();
        assert_eq!(queue.sweep_applied(), 3);
        queue.sweep_expired();
        assert_eq!(queue.sweep_applied(), 3);
    }

    #[test]
    fn grounding_round_trips_across_queue_restore_and_legacy_rows_default_cleanly() {
        let queue = DriveQueue::new();
        let mut signal = pending_signal(DriveIntent::Suggest, "grounded");
        signal.evidence = vec![42];
        signal.grounding = Some(DriveGrounding {
            reason: "recent decision memory".into(),
            evidence: vec![DriveMemoryEvidence {
                id: 42,
                revision: "0000000000000064".into(),
                recorded_at: 1_800_000_000,
                last_reviewed_at: None,
                importance: 2.5,
            }],
            uncertainty: vec![
                DriveGroundingUncertainty::SingleMemorySource,
                DriveGroundingUncertainty::NotReviewed,
            ],
            fresh_until: Some(1_800_001_800),
            complete: true,
        });
        let id = queue.enqueue(signal.clone());
        let persisted = serde_json::to_string(&queue.snapshot()).unwrap();
        let restored_signals: Vec<DriveSignal> = serde_json::from_str(&persisted).unwrap();
        let restored = DriveQueue::new();
        restored.restore(restored_signals);
        assert_eq!(restored.get_signal(id).unwrap().grounding, signal.grounding);

        let mut legacy = serde_json::to_value(signal).unwrap();
        legacy.as_object_mut().unwrap().remove("grounding");
        legacy.as_object_mut().unwrap().remove("terminal_reason");
        let legacy: DriveSignal = serde_json::from_value(legacy).unwrap();
        assert!(legacy.grounding.is_none());
        assert!(legacy.terminal_reason.is_none());
    }
}
