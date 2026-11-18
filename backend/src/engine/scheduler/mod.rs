use parking_lot::Mutex as ParkMutex;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

use crate::domain::space::Space;
use crate::domain::tetra::{MemoryPayload, TetraId, TetraMeta, Tetrahedron};
use crate::domain::vertex::Point3;

use super::adaptive::AdaptiveParams;
use super::bus::{EngineEvent, EventSender};
use super::cognitive::{CognitiveEngine, SchedulerAction, SystemState};
use super::dream::DreamEngine;
use super::drive::DriveEngine;
use super::dynamics;
use super::energy::EnergyCenter;
use super::knowledge::KnowledgeGraph;
use super::outcome::{ActionOutcome, ActionType, OutcomeTracker};
use super::run_layers::{LayerMetrics, RunLayer};
use super::security::SecurityGuard;

/// Mode reported by every permanent-delete entry point (REST + MCP).
pub const DELETE_MODE: &str = "purge";
/// Error prefix for refusals on protected memories (enforced rules, identity anchors).
pub const PROTECTED_ERR_PREFIX: &str = "protected: ";

/// SMRP §6 — 分数可解释性：记录各 boost 调整作用于哪些记忆。
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct SearchScoreNotes {
    pub cluster_boosted: Vec<TetraId>,
    pub importance_boosted: Vec<TetraId>,
    pub access_boosted: Vec<TetraId>,
    pub penalized: Vec<TetraId>,
    pub kg_expanded: Vec<TetraId>, // 突破1: multi_hop KG扩展的记忆
    /// Phase 1: exact 模式的命中来源(每条结果通过哪些信号命中)
    /// 非 exact 模式为空 HashMap
    #[serde(default)]
    pub matched_by_map: std::collections::HashMap<TetraId, Vec<String>>,
}

struct CognitiveThought {
    tick: u64,
    state: SystemState,
}

#[derive(Debug, Clone, PartialEq)]
struct RecalledDriveEvidence {
    id: u64,
    content: String,
    timestamp: i64,
    relevance: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExploreBlockReason {
    NoCurrentEvidence,
    ConflictingEvidence,
}

fn recalled_drive_evidence(result: &serde_json::Value) -> Vec<RecalledDriveEvidence> {
    let Some(sections) = result.get("results").and_then(|value| value.as_object()) else {
        return Vec::new();
    };
    let mut by_id = HashMap::new();
    for items in sections.values().filter_map(|value| value.as_array()) {
        for item in items {
            let (Some(id), Some(content), Some(timestamp)) = (
                item.get("id").and_then(|value| value.as_u64()),
                item.get("content").and_then(|value| value.as_str()),
                item.get("timestamp").and_then(|value| value.as_i64()),
            ) else {
                continue;
            };
            let relevance = item
                .get("relevance")
                .and_then(|value| value.as_array())
                .map(|values| {
                    values
                        .iter()
                        .filter_map(serde_json::Value::as_f64)
                        .fold(0.0_f64, f64::max)
                })
                .unwrap_or_default();
            if !content.trim().is_empty() {
                let candidate = RecalledDriveEvidence {
                    id,
                    content: content.to_string(),
                    timestamp,
                    relevance,
                };
                if by_id
                    .get(&id)
                    .is_none_or(|existing: &RecalledDriveEvidence| existing.relevance < relevance)
                {
                    by_id.insert(id, candidate);
                }
            }
        }
    }
    let mut evidence: Vec<_> = by_id.into_values().collect();
    evidence.sort_by(|a, b| {
        b.relevance
            .partial_cmp(&a.relevance)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
    evidence.truncate(3);
    evidence
}

fn local_exploration_summary(
    question: &str,
    evidence: &[RecalledDriveEvidence],
    conflicts: &[u64],
) -> Result<String, ExploreBlockReason> {
    if evidence.is_empty() {
        return Err(ExploreBlockReason::NoCurrentEvidence);
    }
    if !conflicts.is_empty() {
        return Err(ExploreBlockReason::ConflictingEvidence);
    }
    let evidence_text = evidence
        .iter()
        .map(|item| {
            format!(
                "- [#{}] recorded_at={}: {}",
                item.id,
                item.timestamp,
                item.content.chars().take(160).collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let caveat = if evidence.len() == 1 {
        "One related memory only; it is not independently corroborated."
    } else {
        "These are retrieved memory fragments, not independent verification."
    };
    Ok(format!(
        "Question: {}\nEvidence from current local memories:\n{}\nCaveat: {}\nThis is a source summary, not proof that the knowledge gap is resolved.",
        question, evidence_text, caveat
    ))
}

/// 模型返回后等待 cycle 门以应用决策的上限;routine tick 通常为毫秒级。
const COMMIT_APPLY_WAIT: Duration = Duration::from_secs(30);

/// 人格包核心记忆上限(导出与导入一致;导入按配额逐条预占)。
const PERSONALITY_CORE_MEMORY_LIMIT: usize = 50;
/// 人格包单条记忆内容上限(字符)。
const PERSONALITY_CONTENT_MAX_CHARS: usize = 4000;
/// 导入时视为合法的未来时间容差(秒),吸收两端时钟偏差。
const IMPORT_CLOCK_SKEW_SECS: i64 = 300;

/// 人格包里的一条核心记忆,已做校验与清洗。
#[derive(Debug, Clone, PartialEq)]
struct ImportedMemory {
    content: String,
    labels: Vec<String>,
    timestamp: i64,
    valid_from: i64,
    valid_to: Option<i64>,
    expired_at: Option<i64>,
    invalidated_at: Option<i64>,
    importance: Option<f64>,
    memory_class: Option<String>,
    source_agent: Option<String>,
}

impl ImportedMemory {
    /// 由导出条目构建。缺失/非法的时间回落到 `now`;未来时间(超出时钟容差)视为非法。
    fn from_export(mem: &serde_json::Value, now: i64) -> Option<Self> {
        let raw = mem
            .get("content")
            .and_then(|c| c.as_str())
            .filter(|c| !c.trim().is_empty())
            .or_else(|| mem.get("preview").and_then(|p| p.as_str()))?;
        let mut body = raw.trim();
        while let Some(rest) = body.strip_prefix("[imported]") {
            body = rest.trim_start();
        }
        if body.is_empty() {
            return None;
        }
        let body: String = body.chars().take(PERSONALITY_CONTENT_MAX_CHARS).collect();
        let sane = |v: i64| v > 0 && v <= now + IMPORT_CLOCK_SKEW_SECS;
        let time = |key: &str| mem.get(key).and_then(|v| v.as_i64()).filter(|v| sane(*v));
        let timestamp = time("timestamp")
            .or_else(|| time("valid_from"))
            .unwrap_or(now);
        let valid_from = time("valid_from").unwrap_or(timestamp);
        let after_start = |v: i64| v >= valid_from;
        let mut labels: Vec<String> = mem
            .get("labels")
            .and_then(|l| l.as_array())
            .map(|ls| {
                ls.iter()
                    .filter_map(|l| l.as_str())
                    .map(str::trim)
                    .filter(|l| !l.is_empty() && !IMPORT_RESERVED_LABELS.contains(l))
                    .take(10)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        labels.push("imported".into());
        labels.push("l0-exempt".into());
        Some(Self {
            content: format!("[imported] {}", body),
            labels,
            timestamp,
            valid_from,
            valid_to: time("valid_to").filter(|v| after_start(*v)),
            expired_at: time("expired_at"),
            invalidated_at: time("invalidated_at"),
            importance: mem
                .get("importance")
                .and_then(|v| v.as_f64())
                .filter(|v| v.is_finite())
                .map(|v| v.clamp(0.05, 3.0)),
            memory_class: mem
                .get("memory_class")
                .and_then(|v| v.as_str())
                .filter(|c| matches!(*c, "permanent" | "session" | "bridge"))
                .map(String::from),
            source_agent: mem
                .get("source_agent")
                .and_then(|v| v.as_str())
                .map(|s| s.chars().take(128).collect()),
        })
    }

    /// 把原时间线与来源写回新建记忆;enforced 永不经导入授予。
    fn apply_to(&self, data: &mut MemoryPayload) {
        data.timestamp = self.timestamp;
        data.valid_from = self.valid_from;
        data.valid_to = self.valid_to;
        data.expired_at = self.expired_at;
        data.invalidated_at = self.invalidated_at;
        if let Some(imp) = self.importance {
            data.importance = imp;
        }
        if self.memory_class.is_some() {
            data.memory_class = self.memory_class.clone();
        }
        if self.source_agent.is_some() {
            data.source_agent = self.source_agent.clone();
        }
        data.enforced = false;
    }
}

/// 导入时不从包里继承的系统标签(由系统状态决定,不随包迁移)。
const IMPORT_RESERVED_LABELS: [&str; 4] = ["imported", "l0-exempt", "quarantine", "enforced"];

#[derive(Default)]
struct SingleFlightGate {
    in_flight: AtomicBool,
}

impl SingleFlightGate {
    fn try_acquire(self: &Arc<Self>) -> Option<SingleFlightPermit> {
        self.in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| SingleFlightPermit(Arc::clone(self)))
    }
}

struct SingleFlightPermit(Arc<SingleFlightGate>);

impl Drop for SingleFlightPermit {
    fn drop(&mut self) {
        self.0.in_flight.store(false, Ordering::Release);
    }
}

/// SMRP §7.2 — scheduler 层完整创建报告（gateway 安置 + scheduler 录入副产物）。
#[derive(Debug, Clone)]
pub struct CreateReport {
    pub id: TetraId,
    pub is_new: bool,
    pub placement: Option<super::gateway::PlacementOutcome>,
    pub relations_formed: usize,
    pub auto_labels: Vec<String>,
    pub importance: f64,
    pub memory_type: Option<String>,
    pub rationale: Option<String>,
    pub dedup_matched: Option<(TetraId, f64)>,
    pub conflicts_marked: Vec<TetraId>,
}

#[derive(Debug, Clone)]
pub enum ScheduledTask {
    CreateTetra {
        core: Point3,
        data: MemoryPayload,
        mass: f64,
    },
}

struct TickSnapshot {
    tick: u64,
    energy: f64,
    tetras: Vec<TetraMeta>,
    clusters: Vec<crate::domain::space::Cluster>,
    labels_map: HashMap<u64, Vec<String>>,
    core_map: HashMap<u64, Point3>,
}

pub struct SchedulerCenter {
    space: Arc<Space>,
    energy: Arc<EnergyCenter>,
    knowledge: Arc<KnowledgeGraph>,
    cognitive: Arc<CognitiveEngine>,
    gateway: Arc<super::gateway::GatewayCenter>,
    queue: ParkMutex<VecDeque<ScheduledTask>>,
    loop_gate: Arc<SingleFlightGate>,
    cycle_gate: Arc<SingleFlightGate>,
    /// 所有 dream 入口(手动 api_dream / LLM Dream 动作 / auto_dream)共享:同一时刻只允许一个 dream 周期,
    /// 否则两个周期基于各自快照写回 payload,互相覆盖(lost update)并重复扣能量。
    dream_gate: Arc<SingleFlightGate>,
    /// 调度器发起的模型调用(decide 与认知钩子)单飞:慢层只占自己的门,不占 cycle_gate。
    deliberative_gate: Arc<SingleFlightGate>,
    layers: LayerMetrics,
    tx: EventSender,
    tick_interval: parking_lot::RwLock<Duration>,
    horizon: parking_lot::Mutex<super::horizon::Horizon>,
    tick_count: AtomicU64,
    recent_events: ParkMutex<Vec<String>>,
    decision_history: ParkMutex<Vec<super::cognitive::DecisionRecord>>,
    prev_snapshot: ParkMutex<Option<super::cognitive::StateSnapshot>>,
    last_dream_tick: AtomicU64,
    max_energy: f64,
    emotion: ParkMutex<super::emotion::EmotionState>,
    security: Arc<SecurityGuard>,
    storage: Arc<super::storage::StorageManager>,
    last_fission_tick: AtomicU64,
    // α0.2: runtime primary binding flag (cloud layer sets via set_runtime_primary)
    runtime_has_primary: ParkMutex<bool>,
    /// γ2: 端侧 E2E 公钥 (由 cloud register 注入; 传输层加密用)
    runtime_e2e_pubkey: ParkMutex<Option<String>>,
    /// 审计修复: 归属用户 (enqueue/ack 日志标记, 消除跨用户观测混淆)
    owner_user: ParkMutex<String>,
    last_body_missing_tick: AtomicU64,
    last_reclassify_tick: AtomicU64,
    skills: ParkMutex<Option<Arc<super::skills::SkillEngine>>>,
    pub_skills: ParkMutex<Option<Arc<super::skills::SkillEngine>>>,
    drive: ParkMutex<DriveEngine>,
    outcome: ParkMutex<OutcomeTracker>,
    adaptive: ParkMutex<AdaptiveParams>,
    last_merge_pairs: ParkMutex<HashSet<(usize, usize)>>,
    /// 冷记忆复习冷却:tetra → 最近一次送审时间(秒)。过期条目在每次复习时清理,规模有界。
    cold_reviewed_at: ParkMutex<HashMap<TetraId, i64>>,
    feedback_agg_cache: ParkMutex<Option<(usize, std::time::Instant, HashSet<u64>)>>,
    skill_feedback_agg_cache: ParkMutex<Option<(usize, std::time::Instant, HashSet<u64>)>>,
    drive_queue: Arc<super::drive::DriveQueue>,
    drive_save_gate: ParkMutex<()>,
    /// Agent-facing memory action frontier (propose → claim → complete → outcome memory).
    action_frontier: Arc<super::agent_action::ActionFrontier>,
    /// Dispatch: registered workers for capability-aware assignment.
    worker_registry: Arc<super::dispatch::WorkerRegistry>,
    /// PR-L: deterministic planner pass (coalesced wake + heartbeat stall detection).
    planner_pass: Arc<super::planner_pass::PlannerPass>,
}

mod api;
mod archive;
mod cognitive;
mod drive;
mod import_export;
mod tasks;
mod tick;

#[cfg(test)]
mod tests;

impl SchedulerCenter {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        space: Arc<Space>,
        energy: Arc<EnergyCenter>,
        knowledge: Arc<KnowledgeGraph>,
        cognitive: Arc<CognitiveEngine>,
        gateway: Arc<super::gateway::GatewayCenter>,
        tx: EventSender,
        _rx: broadcast::Receiver<EngineEvent>,
        tick_interval_ms: u64,
        max_energy: f64,
    ) -> Self {
        Self::with_security(
            space,
            energy,
            knowledge,
            cognitive,
            gateway,
            tx,
            _rx,
            tick_interval_ms,
            max_energy,
            Arc::new(SecurityGuard::from_env()),
            Arc::new(
                super::storage::StorageManager::new(std::path::Path::new("data"))
                    .expect("storage init failed"),
            ),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_security(
        space: Arc<Space>,
        energy: Arc<EnergyCenter>,
        knowledge: Arc<KnowledgeGraph>,
        cognitive: Arc<CognitiveEngine>,
        gateway: Arc<super::gateway::GatewayCenter>,
        tx: EventSender,
        _rx: broadcast::Receiver<EngineEvent>,
        tick_interval_ms: u64,
        max_energy: f64,
        security: Arc<SecurityGuard>,
        storage: Arc<super::storage::StorageManager>,
    ) -> Self {
        Self {
            space,
            energy,
            knowledge,
            cognitive,
            gateway,
            queue: ParkMutex::new(VecDeque::new()),
            loop_gate: Arc::new(SingleFlightGate::default()),
            cycle_gate: Arc::new(SingleFlightGate::default()),
            dream_gate: Arc::new(SingleFlightGate::default()),
            deliberative_gate: Arc::new(SingleFlightGate::default()),
            layers: LayerMetrics::default(),
            tx,
            tick_interval: parking_lot::RwLock::new(Duration::from_millis(tick_interval_ms)),
            horizon: parking_lot::Mutex::new(super::horizon::Horizon::new(tick_interval_ms)),
            tick_count: AtomicU64::new(0),
            recent_events: ParkMutex::new(Vec::new()),
            decision_history: ParkMutex::new(Vec::new()),
            prev_snapshot: ParkMutex::new(None),
            last_dream_tick: AtomicU64::new(0),
            max_energy,
            emotion: ParkMutex::new(super::emotion::EmotionState::default()),
            security,
            storage,
            last_fission_tick: AtomicU64::new(0),
            runtime_has_primary: ParkMutex::new(false),
            runtime_e2e_pubkey: ParkMutex::new(None),
            owner_user: ParkMutex::new(String::new()),
            last_body_missing_tick: AtomicU64::new(0),
            last_reclassify_tick: AtomicU64::new(0),
            skills: ParkMutex::new(None),
            pub_skills: ParkMutex::new(None),
            drive: ParkMutex::new(DriveEngine::new()),
            outcome: ParkMutex::new(OutcomeTracker::new()),
            adaptive: ParkMutex::new(AdaptiveParams::new()),
            last_merge_pairs: ParkMutex::new(HashSet::new()),
            cold_reviewed_at: ParkMutex::new(HashMap::new()),
            feedback_agg_cache: ParkMutex::new(None),
            skill_feedback_agg_cache: ParkMutex::new(None),
            drive_queue: Arc::new(super::drive::DriveQueue::new()),
            drive_save_gate: ParkMutex::new(()),
            action_frontier: Arc::new(super::agent_action::ActionFrontier::new()),
            worker_registry: Arc::new(super::dispatch::WorkerRegistry::new()),
            planner_pass: Arc::new(super::planner_pass::PlannerPass::with_defaults()),
        }
    }

    pub fn set_skills(&self, skills: Arc<super::skills::SkillEngine>) {
        *self.skills.lock() = Some(skills);
    }

    pub fn set_pub_skills(&self, pub_skills: Arc<super::skills::SkillEngine>) {
        *self.pub_skills.lock() = Some(pub_skills);
    }

    fn build_snapshot(&self) -> TickSnapshot {
        let tick = self.tick_count.load(Ordering::SeqCst);
        let energy = self.energy.available();
        let tetras = self.space.all_tetras_meta();
        let clusters = self.find_clusters_cached();
        let labels_map: HashMap<u64, Vec<String>> =
            tetras.iter().map(|t| (t.id, t.labels.clone())).collect();
        let core_map: HashMap<u64, Point3> = tetras.iter().map(|t| (t.id, t.core)).collect();
        TickSnapshot {
            tick,
            energy,
            tetras,
            clusters,
            labels_map,
            core_map,
        }
    }

    /// L0: Lock the drive engine for reward adjustment
    pub fn drive_engine_lock(&self) -> parking_lot::MutexGuard<'_, super::drive::DriveEngine> {
        self.drive.lock()
    }

    pub fn drive_queue(&self) -> &Arc<super::drive::DriveQueue> {
        &self.drive_queue
    }

    pub fn action_frontier(&self) -> &Arc<super::agent_action::ActionFrontier> {
        &self.action_frontier
    }

    pub fn worker_registry(&self) -> &Arc<super::dispatch::WorkerRegistry> {
        &self.worker_registry
    }

    pub fn planner_pass(&self) -> &Arc<super::planner_pass::PlannerPass> {
        &self.planner_pass
    }

    pub fn kg_handle(&self) -> Arc<KnowledgeGraph> {
        self.knowledge.clone()
    }

    pub fn gateway_handle(&self) -> Arc<super::gateway::GatewayCenter> {
        self.gateway.clone()
    }

    pub fn knowledge_handle(&self) -> Arc<KnowledgeGraph> {
        self.knowledge.clone()
    }

    pub fn set_tick_interval(&self, ms: u64) {
        let mut interval = self.tick_interval.write();
        *interval = Duration::from_millis(ms);
        self.horizon.lock().set_base_ms(ms);
    }

    pub fn horizon_status(&self) -> (String, f64, u32) {
        let horizon = self.horizon.lock();
        (
            horizon.phase().as_str().to_string(),
            horizon.pressure(),
            horizon.debt(),
        )
    }

    /// 分层运行观测:每层运行/推迟/耗时 + horizon 状态。
    pub fn run_layers_snapshot(&self) -> serde_json::Value {
        let (phase, pressure, debt, missed) = self.horizon_observability();
        serde_json::json!({
            "layers": self.layers.snapshot(),
            "horizon": {
                "phase": phase,
                "pressure": pressure,
                "debt": debt,
                "missed_beats": missed,
            },
            "in_flight": {
                "cycle": self.cycle_gate.in_flight.load(Ordering::Acquire),
                "deliberative": self.deliberative_gate.in_flight.load(Ordering::Acquire),
                "dream": self.dream_gate.in_flight.load(Ordering::Acquire),
            },
        })
    }

    /// Tick observability: phase / pressure / debt / cumulative missed commits.
    pub fn horizon_observability(&self) -> (String, f64, u32, u64) {
        let horizon = self.horizon.lock();
        (
            horizon.phase().as_str().to_string(),
            horizon.pressure(),
            horizon.debt(),
            horizon.missed_commits(),
        )
    }

    /// α0.2: cloud runtime register/unregister 时更新; detect_prediction_errors 读取产生 body_missing
    /// δ2: 空铃降权 — dead-letter 的 evidence 记忆 importance 衰减
    /// 语义: 同类 evidence 反复空铃 → 记忆降权 → 不再产出 drive (7日空铃下降机制)
    pub fn decay_evidence(&self, evidence: &[TetraId], amount: f64) {
        for &eid in evidence {
            if let Some(t) = self.space.get_tetrahedron(eid) {
                let cur = t.data.importance;
                let next = (cur - amount).max(0.05);
                if next < cur {
                    let _ = self.space.update_importance(eid, next);
                    if let Some(t2) = self.space.get_tetrahedron(eid) {
                        if let Err(e) = self.storage.upsert_tetra(&t2) {
                            tracing::warn!("[d2] evidence decay persist failed #{}: {}", eid, e);
                        }
                    }
                    tracing::info!(
                        "[d2] ring decay: evidence #{} importance {:.2} -> {:.2}",
                        eid,
                        cur,
                        next
                    );
                }
            }
        }
    }

    pub fn set_runtime_primary(&self, val: bool) {
        *self.runtime_has_primary.lock() = val;
    }

    /// 审计修复: 设置归属用户 (Engine::build 时注入; 日志标记用)
    pub fn set_owner_user(&self, uid: &str) {
        *self.owner_user.lock() = uid.to_string();
    }

    /// γ2: 注入/清除端侧 E2E 公钥
    pub fn set_e2e_pubkey(&self, pem: Option<&str>) {
        *self.runtime_e2e_pubkey.lock() = pem.map(|s| s.to_string());
    }

    /// γ2: 取当前 E2E 公钥
    pub fn e2e_pubkey(&self) -> Option<String> {
        self.runtime_e2e_pubkey.lock().clone()
    }
    pub fn storage_handle(&self) -> Arc<super::storage::StorageManager> {
        self.storage.clone()
    }
    pub fn space_handle(&self) -> Arc<Space> {
        self.space.clone()
    }
}
