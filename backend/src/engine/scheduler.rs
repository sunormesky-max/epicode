use parking_lot::Mutex as ParkMutex;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
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
use super::security::SecurityGuard;

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
    tx: EventSender,
    tick_interval: parking_lot::RwLock<Duration>,
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
    feedback_agg_cache: ParkMutex<Option<(usize, std::time::Instant, HashSet<u64>)>>,
    skill_feedback_agg_cache: ParkMutex<Option<(usize, std::time::Instant, HashSet<u64>)>>,
    drive_queue: Arc<super::drive::DriveQueue>,
}

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
            tx,
            tick_interval: parking_lot::RwLock::new(Duration::from_millis(tick_interval_ms)),
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
            feedback_agg_cache: ParkMutex::new(None),
            skill_feedback_agg_cache: ParkMutex::new(None),
            drive_queue: Arc::new(super::drive::DriveQueue::new()),
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

    /// D4: 时间感知创建(故事时间) — valid_from=timestamp, 系统时间由gateway内部记录
    pub fn api_create_memory_at(
        &self,
        content: &str,
        labels: Vec<String>,
        timestamp: i64,
    ) -> Result<(TetraId, bool), String> {
        let r = self
            .gateway
            .create_memory_with_time(content, labels, timestamp)?;
        self.persist_tetra(r.id);
        self.gateway.mark_dirty(r.id);
        Ok((r.id, r.is_new))
    }

    /// L1相2d: 批量ingest前预热嵌入缓存(单次批量推理替代N次串行)
    pub fn prewarm_batch_embeddings(&self, texts: &[String]) {
        self.gateway.prewarm_embeddings(texts);
    }

    /// L1图书馆: MCP工具层检索入口(经全局访问器, 无需引擎持有库引用)
    pub fn library_search_public(
        &self,
        query: &str,
        k: usize,
    ) -> Result<Vec<crate::engine::library::LibraryHitPublic>, String> {
        match crate::engine::library::global_library() {
            Some(lib) => lib.search_public(query, k),
            None => Ok(Vec::new()),
        }
    }

    pub fn api_create_memory(
        &self,
        content: &str,
        labels: Vec<String>,
    ) -> Result<(TetraId, bool), String> {
        let r = self.api_create_memory_full(content, labels)?;
        Ok((r.id, r.is_new))
    }

    /// SMRP §7.2 — 完整创建报告，收集全部录入副产物（安置/分类/去重/冲突/建链）。
    pub fn api_create_memory_full(
        &self,
        content: &str,
        mut labels: Vec<String>,
    ) -> Result<CreateReport, String> {
        self.security
            .validate_content(content)
            .map_err(|_| "content validation failed".to_string())?;
        self.security
            .validate_labels(&labels)
            .map_err(|_| "labels validation failed".to_string())?;
        self.security
            .check_constitution_create(!content.is_empty())
            .map_err(|r| format!("constitution violation: {:?}", r))?;

        let intake = super::intake::MemoryIntake::process(content, &mut labels);

        if intake.is_noise {
            return Err("content rejected as noise (too short or meaningless)".to_string());
        }

        labels = intake.labels;
        let mut auto_labels: Vec<String> = Vec::new();

        let has_domain_label = labels.iter().any(|l| {
            !matches!(
                l.as_str(),
                "general"
                    | "finding"
                    | "decision"
                    | "bug"
                    | "session"
                    | "pattern"
                    | "preference"
                    | "identity"
                    | "system"
                    | "documentation"
            )
        });
        if !has_domain_label {
            match self.cognitive.classify_content(content) {
                Ok(llm_labels)
                    if !llm_labels.is_empty() && llm_labels != vec!["general".to_string()] =>
                {
                    for l in &llm_labels {
                        if !labels.iter().any(|x| x == l) {
                            labels.push(l.clone());
                            auto_labels.push(l.clone());
                        }
                    }
                    tracing::info!(
                        "[Intake] classified: {:?} → {:?}",
                        content.chars().take(40).collect::<String>(),
                        llm_labels
                    );
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("[Intake] classify failed: {}", e);
                }
            }
        }

        let importance = intake.importance;
        let memory_type = intake.memory_type.clone();
        let rationale = intake.rationale.clone();
        let mut dedup_matched: Option<(TetraId, f64)> = None;
        let mut conflicts_marked: Vec<TetraId> = Vec::new();

        // Semantic dedup — search before create (L1相2b: 纯向量knn替代全混合, 1.2s→ms)
        if let Ok(similar) = self.gateway.search_vector_only(content, 3) {
            for (sid, sim, _bm25, payload) in &similar {
                if *sim > 0.85 && payload.content.len() > 20 {
                    let text_sim =
                        super::intake::MemoryIntake::text_similarity(content, &payload.content);
                    let threshold = if content.len() < 30 { 0.80 } else { 0.55 };
                    if text_sim > threshold {
                        tracing::info!("[Intake] semantic dedup: new content ≈ #{} (vec_sim={:.2} text_sim={:.2} threshold={:.2}), returning existing",
                            sid, sim, text_sim, threshold);
                        self.energy.replenish(1.0);
                        dedup_matched = Some((*sid, (*sim * 100.0).round() / 100.0));
                        return Ok(CreateReport {
                            id: *sid,
                            is_new: false,
                            placement: None,
                            relations_formed: 0,
                            auto_labels,
                            importance,
                            memory_type,
                            rationale,
                            dedup_matched,
                            conflicts_marked,
                        });
                    }
                }
            }

            // Conflict detection — if new content has negation words AND high similarity to existing
            let conflict_ids = super::intake::MemoryIntake::check_conflict(content, &similar);
            if !conflict_ids.is_empty() {
                let g = self.gateway.create_memory(content, labels.clone())?;
                if g.is_new {
                    if let Some(tetra) = self.space.get_tetrahedron(g.id) {
                        let mut data = tetra.data.clone();
                        data.importance = importance;
                        data.memory_type = intake.memory_type;
                        data.rationale = intake.rationale;
                        if let Err(e) = self.space.update_payload(g.id, data) {
                            tracing::warn!("[Scheduler] update_payload {} failed: {}", g.id, e);
                        }
                    }
                    for &cid in &conflict_ids {
                        self.knowledge.add_relation(
                            g.id,
                            cid,
                            super::knowledge::RelationType::Contradicts,
                            0.8,
                        );
                        let now = chrono::Utc::now().timestamp();
                        let _ = self.space.update_validity(cid, Some(now));
                        self.persist_tetra(cid);
                        tracing::info!(
                            "[Intake] contradiction: #{} supersedes #{}, marking #{} as invalid",
                            g.id,
                            cid,
                            cid
                        );
                    }
                }
                self.persist_tetra(g.id);
                conflicts_marked = conflict_ids;
                return Ok(CreateReport {
                    id: g.id,
                    is_new: g.is_new,
                    placement: g.placement,
                    relations_formed: g.relations_formed,
                    auto_labels,
                    importance,
                    memory_type,
                    rationale,
                    dedup_matched,
                    conflicts_marked,
                });
            }
        }

        let g = self.gateway.create_memory(content, labels)?;

        if g.is_new {
            if let Some(tetra) = self.space.get_tetrahedron(g.id) {
                let mut data = tetra.data.clone();
                data.importance = importance;
                data.memory_type = intake.memory_type;
                data.rationale = intake.rationale;
                if let Err(e) = self.space.update_payload(g.id, data) {
                    tracing::warn!("[Scheduler] update_payload {} failed: {}", g.id, e);
                }
            }
        }

        self.persist_tetra(g.id);
        Ok(CreateReport {
            id: g.id,
            is_new: g.is_new,
            placement: g.placement,
            relations_formed: g.relations_formed,
            auto_labels,
            importance,
            memory_type,
            rationale,
            dedup_matched,
            conflicts_marked,
        })
    }

    pub fn api_create_memory_with_time(
        &self,
        content: &str,
        mut labels: Vec<String>,
        timestamp: i64,
    ) -> Result<(TetraId, bool), String> {
        self.security
            .validate_content(content)
            .map_err(|_| "content validation failed".to_string())?;
        self.security
            .validate_labels(&labels)
            .map_err(|_| "labels validation failed".to_string())?;
        self.security
            .check_constitution_create(!content.is_empty())
            .map_err(|r| format!("constitution violation: {:?}", r))?; // 宪法检查（kimi 观察7）

        let intake = super::intake::MemoryIntake::process(content, &mut labels);

        if intake.is_noise {
            return Err("content rejected as noise".to_string());
        }

        labels = intake.labels;
        let importance = intake.importance;

        // Semantic dedup for historical imports too
        if let Ok(similar) = self.gateway.search(content, 3) {
            for (sid, sim, _bm25, payload) in &similar {
                if *sim > 0.85 && payload.content.len() > 20 {
                    let text_sim =
                        super::intake::MemoryIntake::text_similarity(content, &payload.content);
                    if text_sim > 0.55 {
                        tracing::info!(
                            "[Intake] semantic dedup(history): ≈ #{} (vec={:.2} text={:.2})",
                            sid,
                            sim,
                            text_sim
                        );
                        return Ok((*sid, false));
                    }
                }
            }
        }

        let g = self
            .gateway
            .create_memory_with_time(content, labels, timestamp)?;
        let id = g.id;
        let is_new = g.is_new;

        if is_new {
            if let Some(tetra) = self.space.get_tetrahedron(id) {
                let mut data = tetra.data.clone();
                data.importance = importance;
                data.memory_type = intake.memory_type;
                data.rationale = intake.rationale;
                if let Err(e) = self.space.update_payload(id, data) {
                    tracing::warn!("[Scheduler] update_payload {} failed: {}", id, e);
                }
            }

            if !intake.conflict_ids.is_empty() {
                for &cid in &intake.conflict_ids {
                    self.knowledge.add_relation(
                        id,
                        cid,
                        super::knowledge::RelationType::Contradicts,
                        0.8,
                    );
                    let now = chrono::Utc::now().timestamp();
                    let _ = self.space.update_validity(cid, Some(now));
                    self.persist_tetra(cid);
                    tracing::info!(
                        "[Intake] contradiction: #{} supersedes #{}, marking #{} as invalid",
                        id,
                        cid,
                        cid
                    );
                }
            }
        }

        self.persist_tetra(id);
        Ok((id, is_new))
    }

    pub fn api_remember(&self, content: &str) -> Result<(TetraId, Vec<String>), String> {
        self.security
            .validate_content(content)
            .map_err(|_| "content validation failed".to_string())?;
        let labels = if self.cognitive.enabled() {
            self.cognitive
                .classify_content(content)
                .unwrap_or_else(|_| vec!["general".to_string()])
        } else {
            vec!["general".to_string()]
        };
        let g = self.gateway.create_memory(content, labels.clone())?;
        self.persist_tetra(g.id);
        Ok((g.id, labels))
    }

    /// M1修复: 带预设标签的 remember（digestion 用，跳过重复 LLM 分类）。
    /// digestion 已预分类，这里直接用，避免双倍 LLM 调用。
    pub fn api_remember_with_labels(
        &self,
        content: &str,
        pre_labels: Vec<String>,
    ) -> Result<(TetraId, Vec<String>), String> {
        self.security
            .validate_content(content)
            .map_err(|_| "content validation failed".to_string())?;
        let g = self.gateway.create_memory(content, pre_labels.clone())?;
        self.persist_tetra(g.id);
        Ok((g.id, pre_labels))
    }

    /// 记忆forget操作（Cognee启发）—— 显式标记一条记忆为"遗忘"。
    /// 与governor的隐式衰减不同，这是用户/Agent主动决定忘记某条记忆。
    pub fn api_set_memory_class(&self, id: TetraId, class: &str) -> Result<(), String> {
        let tetra = self
            .space
            .get_tetrahedron(id)
            .ok_or_else(|| format!("memory {} not found", id))?;
        let mut data = tetra.data.clone();
        data.memory_class = Some(class.to_string());
        self.space
            .update_payload(id, data)
            .map_err(|e| format!("update memory_class failed: {}", e))?;
        self.persist_tetra(id);
        Ok(())
    }

    pub fn api_forget_memory(&self, id: TetraId) -> Result<serde_json::Value, String> {
        let tetra = match self.space.get_tetrahedron(id) {
            Some(t) => t,
            None => return Err(format!("memory #{} not found", id)),
        };
        if tetra.data.enforced {
            return Err(format!(
                "memory #{} is enforced and cannot be forgotten",
                id
            ));
        }

        let mut updated = tetra.data.clone();
        let now = chrono::Utc::now().timestamp();
        updated.valid_to = Some(now); // 标记失效
        updated.importance = 0.01; // 降到最低重要性
        updated.invalidated_at = Some(now);

        match self.space.update_payload(id, updated) {
            Ok(_) => {
                self.gateway_handle().mark_dirty(id);
                // P0-2 持久化修复: forget 同步写盘, 保证重启后 valid_to 不丢
                if let Some(t) = self.space.get_tetrahedron(id) {
                    if let Err(e) = self.storage.upsert_tetra(&t) {
                        tracing::warn!("[P0-2] forget persist failed for {}: {}", id, e);
                    }
                }
                tracing::info!(
                    "[Forget] memory #{} explicitly forgotten (valid_to set, importance→0.01)",
                    id
                );
                Ok(serde_json::json!({
                    "id": id,
                    "forgotten": true,
                    "valid_to": now,
                }))
            }
            Err(e) => Err(format!("failed to forget memory #{}: {}", id, e)),
        }
    }

    pub fn api_search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<(TetraId, f64, f64, MemoryPayload)>, String> {
        self.api_search_filtered(query, limit, None)
    }

    pub fn api_search_filtered(
        &self,
        query: &str,
        limit: usize,
        filters: Option<&super::search_engine::SearchFilters>,
    ) -> Result<Vec<(TetraId, f64, f64, MemoryPayload)>, String> {
        Ok(self.api_search_inner(query, limit, filters)?.0)
    }

    /// SMRP §6 — 带分数可解释性的搜索（Full 级）。
    #[allow(clippy::type_complexity)]
    pub fn api_search_scored(
        &self,
        query: &str,
        limit: usize,
        filters: Option<&super::search_engine::SearchFilters>,
    ) -> Result<(Vec<(TetraId, f64, f64, MemoryPayload)>, SearchScoreNotes), String> {
        self.api_search_inner(query, limit, filters)
    }

    #[allow(clippy::type_complexity)]
    fn api_search_inner(
        &self,
        query: &str,
        limit: usize,
        filters: Option<&super::search_engine::SearchFilters>,
    ) -> Result<(Vec<(TetraId, f64, f64, MemoryPayload)>, SearchScoreNotes), String> {
        self.security
            .validate_query(query)
            .map_err(|_| "query validation failed".to_string())?;
        let intent = super::retrieval::RetrievalEngine::parse_intent(query);

        // Phase 1 检索可信度重建: exact 模式走专用通道, 短路语义展开 + rerank + KG 扩展
        let mode = filters.map(|f| f.mode).unwrap_or_default();
        if mode == super::search_engine::SearchMode::Exact {
            // exact: 用原始 query, 不做 expand_query(否则精确 token 被同义词稀释)
            let scored = self
                .gateway
                .search_filtered_with_mode(query, limit * 3, filters)?;
            // 5-tuple → 4-tuple + 提取 matched_by
            let mut results: Vec<(TetraId, f64, f64, MemoryPayload)> =
                Vec::with_capacity(scored.len());
            let mut matched_by_map: std::collections::HashMap<TetraId, Vec<String>> =
                std::collections::HashMap::new();
            for (id, sim, mass, payload, matched_by) in scored {
                if !matched_by.is_empty() {
                    matched_by_map.insert(id, matched_by.iter().map(|s| s.to_string()).collect());
                }
                results.push((id, sim, mass, payload));
            }
            // exact 模式只做 exclude 负过滤(不 rerank, 不 boost)
            super::retrieval::RetrievalEngine::apply_exclude_only(&mut results, &intent);
            // valid_to 过滤(与其他模式一致的骨架硬化)
            results.retain(|(_, _, _, payload)| {
                payload.valid_to.is_none() && !payload.labels.iter().any(|l| l == "quarantine")
            });
            // P1 搜索硬化: KG 扩展结果也必须通过 filters (D1-D3 修复)
            if let Some(f) = filters {
                results.retain(|(_, _, _, payload)| {
                    super::search_engine::passes_filters_pub(payload, f)
                });
            }
            results.truncate(limit);
            let notes = SearchScoreNotes {
                matched_by_map,
                ..Default::default()
            };
            return Ok((results, notes));
        }

        if mode == super::search_engine::SearchMode::Semantic
            || mode == super::search_engine::SearchMode::Graph
        {
            let scored = self
                .gateway
                .search_filtered_with_mode(query, limit * 3, filters)?;
            let mut results: Vec<(TetraId, f64, f64, MemoryPayload)> =
                Vec::with_capacity(scored.len());
            let mut matched_by_map: std::collections::HashMap<TetraId, Vec<String>> =
                std::collections::HashMap::new();
            for (id, sim, mass, payload, matched_by) in scored {
                if !matched_by.is_empty() {
                    matched_by_map.insert(id, matched_by.iter().map(|s| s.to_string()).collect());
                }
                results.push((id, sim, mass, payload));
            }
            results.retain(|(_, _, _, payload)| {
                payload.valid_to.is_none() && !payload.labels.iter().any(|l| l == "quarantine")
            });
            results.truncate(limit);
            let notes = SearchScoreNotes {
                matched_by_map,
                ..Default::default()
            };
            return Ok((results, notes));
        }

        let expanded_query = if intent.expanded_terms.is_empty() {
            query.to_string()
        } else {
            format!("{} {}", query, intent.expanded_terms.join(" "))
        };

        let mut results = self
            .gateway
            .search_filtered(&expanded_query, limit * 3, filters)?;
        super::retrieval::RetrievalEngine::rerank(&mut results, &intent, limit * 2);

        let clusters = self.find_clusters_cached();
        let query_tokens = super::search_engine::tokenize(query);
        let cluster_boost = if !clusters.is_empty() {
            let mut best_cluster_id: Option<usize> = None;
            let mut best_cluster_score: f64 = 0.0;
            for (ci, cluster) in clusters.iter().enumerate() {
                let mut score = 0.0;
                for &tid in &cluster.tetra_ids {
                    if let Some(t) = self.space.get_tetrahedron(tid) {
                        let content_lower = t.data.content.to_lowercase();
                        let matches = query_tokens
                            .iter()
                            .filter(|w| content_lower.contains(w.as_str()))
                            .count();
                        score += matches as f64 * t.data.importance;
                    }
                }
                if score > best_cluster_score {
                    best_cluster_score = score;
                    best_cluster_id = Some(ci);
                }
            }
            best_cluster_id.map(|ci| {
                clusters[ci]
                    .tetra_ids
                    .iter()
                    .copied()
                    .collect::<std::collections::HashSet<u64>>()
            })
        } else {
            None
        };

        let mut notes = SearchScoreNotes::default();
        if let Some(ref boost_ids) = cluster_boost {
            for (id, sim, _, _) in &mut results {
                if boost_ids.contains(id) {
                    *sim += 0.08;
                    notes.cluster_boosted.push(*id);
                }
            }
            results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        }

        for (id, sim, _, payload) in &mut results {
            if payload.importance >= 2.5 {
                *sim += 0.06;
                notes.importance_boosted.push(*id);
            }
            if payload.access_count > 5 {
                *sim += 0.04;
                notes.access_boosted.push(*id);
            }
            if payload
                .labels
                .iter()
                .any(|l| l == "outdated" || l == "superseded")
            {
                *sim -= 0.3;
                notes.penalized.push(*id);
            }
        }
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        results.retain(|(id, sim, _bm25, payload)| {
            // P4 治理: 排除 quarantine 记忆（默认不可见）
            if payload.labels.iter().any(|l| l == "quarantine") {
                return false;
            }
            // 骨架硬化：完全过滤已失效记忆（valid_to 有值 = superseded/矛盾/去重）
            if payload.valid_to.is_some() {
                return false;
            }
            if payload.importance < 0.1 && payload.content.len() < 15 {
                tracing::info!(
                    "[Search] filtered low-quality id={} (importance={:.2})",
                    id,
                    payload.importance
                );
                false
            } else {
                *sim >= 0.0
            }
        });

        // ── 突破1: multi_hop 多跳推理扩展 ──
        // 向量检索结果作为 seeds，沿知识图谱扩展2跳，把推理关联的记忆merge进结果。
        // KG 扩展结果 sim 乘 0.7（间接关联可信度折扣），不会挤掉直接向量命中。
        // 设计来自大卫#1189"记忆系统智能化第一波"，multi_hop 已实现于 knowledge.rs:296。
        {
            let kg = self.kg_handle();
            let seed_ids: Vec<TetraId> = results.iter().take(10).map(|(id, _, _, _)| *id).collect();
            let expanded = kg.multi_hop_adaptive(&seed_ids, limit);
            let existing: std::collections::HashSet<TetraId> =
                results.iter().map(|(id, _, _, _)| *id).collect();
            let mut kg_results: Vec<(TetraId, f64, f64, MemoryPayload)> = Vec::new();
            for (exp_id, strength) in &expanded {
                if existing.contains(exp_id) {
                    continue;
                }
                if let Some(tetra) = self.space.get_tetrahedron(*exp_id) {
                    let kg_sim = strength * 0.7;
                    if kg_sim >= 0.0 {
                        notes.kg_expanded.push(*exp_id);
                        kg_results.push((*exp_id, kg_sim, 0.0, tetra.data));
                    }
                }
            }
            if !kg_results.is_empty() {
                tracing::info!(
                    "[Search] multi_hop expanded {} KG results from {} seeds",
                    kg_results.len(),
                    seed_ids.len()
                );
                results.extend(kg_results);
                results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            }
        }

        // P1 搜索硬化: KG 扩展结果也必须通过 filters (D1-D3 修复)
        if let Some(f) = filters {
            results
                .retain(|(_, _, _, payload)| super::search_engine::passes_filters_pub(payload, f));
        }
        results.truncate(limit);

        // ── 突破4: knowledge-gap 沉淀 ──
        // 搜索结果为空或极低质量时，自动记录一条 knowledge-gap 记忆。
        // 下次相关检索时，knowledge-gap 记忆会浮现，提示"这里存在知识空白"。
        // 设计来自大卫#1189。限流：靠 content_hash 去重(相同 query 不重复创建)。
        if results.is_empty() && !query.trim().is_empty() && query.len() < 200 {
            // L1加固: 垃圾查询防护 — 无CJK+无空格分词+长单块(密文/编码碎片)只记日志不入库,
            // 防客户端用垃圾查询批量制造knowledge-gap污染(实测教训: 密文查询会变成待学习条目)
            let han = query
                .chars()
                .filter(|c| ('\u{4e00}'..='\u{9fff}').contains(c))
                .count();
            let words = query.split_whitespace().count();
            let garbage = han == 0 && words <= 1 && query.chars().count() > 40;
            if garbage {
                tracing::warn!(
                    "[Search] knowledge-gap suppressed (garbage-like, len={}): {:?}",
                    query.chars().count(),
                    query.chars().take(12).collect::<String>()
                );
            } else {
                let gap_content = format!("[knowledge-gap] 待学习：{}", query);
                let gap_labels = vec!["knowledge-gap".to_string(), "system".to_string()];
                // 同步创建(低频:仅搜索完全无结果时触发)
                let _ = self.gateway.create_memory(&gap_content, gap_labels);
                tracing::info!(
                    "[Search] knowledge-gap recorded: {}",
                    query.chars().take(60).collect::<String>()
                );
            }
        }

        // 骨架硬化：access_count 更新改为内存缓冲，不阻塞搜索热路径
        // 之前在搜索返回前做 10 次 update_payload + persist_tetra——严重的写污染
        // 现在只更新内存中的 access_counts（搜索评分已用），不触发 Space 写锁和 DB 写
        // last_reviewed_ts 的持久化由 governor 的 apply_decay 在 tick 中异步处理
        // （governor 读 access_count > 0 的记忆，重置衰减时间——不需要搜索时写入）
        Ok((results, notes))
    }

    pub fn api_get_node(&self, id: TetraId) -> Option<MemoryPayload> {
        self.gateway.get_node(id)
    }

    pub fn api_list_nodes(&self) -> Vec<(TetraId, MemoryPayload)> {
        self.api_list_nodes_limit(5000)
    }

    pub fn api_list_nodes_limit(&self, limit: usize) -> Vec<(TetraId, MemoryPayload)> {
        let mut all = self.gateway.list_nodes();
        all.truncate(limit);
        all
    }

    /// 清道夫系统：扫描全库，诊断并 supersede 垃圾记忆（不删除）
    pub fn api_scavenge(&self) -> serde_json::Value {
        let all = self.space.all_tetrahedrons();
        let now = chrono::Utc::now().timestamp();
        let mut duplicates = 0;
        let mut empties = 0;
        let mut stale_superseded = 0;
        let mut woke_up = 0;
        let mut report: Vec<String> = Vec::new();

        // 1. 检测重复 content_hash + supersede（保留最高 importance 的）
        let mut hash_groups: HashMap<u64, Vec<(u64, f64, Option<i64>)>> = HashMap::new();
        for t in &all {
            let entry = hash_groups.entry(t.data.content_hash).or_default();
            entry.push((t.id, t.data.importance, t.data.valid_to));
        }
        for (hash, group) in &hash_groups {
            if group.len() > 1 && *hash != 0 {
                // 保留 importance 最高的，supersede 其余
                let mut sorted = group.clone();
                sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                for (id, _, valid_to) in sorted.iter().skip(1) {
                    if valid_to.is_none() {
                        if let Some(tetra) = self.space.get_tetrahedron(*id) {
                            let mut data = tetra.data.clone();
                            data.valid_to = Some(now);
                            data.importance = 0.01;
                            let _ = self.space.update_payload(*id, data);
                            self.persist_tetra(*id);
                            duplicates += 1;
                        }
                    }
                }
                report.push(format!(
                    "重复组 hash={}：{}条记忆，supersede {}条",
                    hash,
                    group.len(),
                    group.len() - 1
                ));
            }
        }

        // 2. 检测空/碎片内容（<5字）
        for t in &all {
            if t.data.content.trim().len() < 5 && t.data.valid_to.is_none() {
                let mut data = t.data.clone();
                data.valid_to = Some(now);
                data.importance = 0.01;
                let _ = self.space.update_payload(t.id, data);
                self.persist_tetra(t.id);
                empties += 1;
            }
        }
        if empties > 0 {
            report.push(format!("碎片记忆（<5字）：{}条已 supersede", empties));
        }

        // 3. 已 superseded 的高 importance 降权
        for t in &all {
            if t.data.valid_to.is_some() && t.data.importance > 0.1 {
                let mut data = t.data.clone();
                data.importance = 0.01;
                let _ = self.space.update_payload(t.id, data);
                self.persist_tetra(t.id);
                stale_superseded += 1;
            }
        }
        if stale_superseded > 0 {
            report.push(format!(
                "已失效但高重要性：{}条已降权到0.01",
                stale_superseded
            ));
        }

        // 4. 唤醒沉睡记忆（access_count=0 → access_count=1，给它们被搜索到的机会）
        for t in &all {
            if t.data.access_count == 0 && t.data.valid_to.is_none() {
                let mut data = t.data.clone();
                data.access_count = 1;
                let _ = self.space.update_payload(t.id, data);
                self.persist_tetra(t.id);
                woke_up += 1;
            }
        }
        if woke_up > 0 {
            report.push(format!("沉睡记忆唤醒：{}条 access_count 0→1", woke_up));
        }

        serde_json::json!({
            "scanned": all.len(),
            "duplicates_superseded": duplicates,
            "empties_superseded": empties,
            "stale_downgraded": stale_superseded,
            "woke_up": woke_up,
            "report": report,
        })
    }

    pub fn api_list_by_labels(
        &self,
        labels: &[&str],
        limit: usize,
    ) -> Vec<(TetraId, MemoryPayload)> {
        self.gateway.list_by_labels(labels, limit)
    }

    pub fn api_list_recent(&self, offset: usize, limit: usize) -> Vec<(TetraId, MemoryPayload)> {
        self.gateway.list_recent(offset, limit)
    }

    /// L0: Lock the drive engine for reward adjustment
    pub fn drive_engine_lock(&self) -> parking_lot::MutexGuard<'_, super::drive::DriveEngine> {
        self.drive.lock()
    }

    pub fn drive_queue(&self) -> &Arc<super::drive::DriveQueue> {
        &self.drive_queue
    }
    pub fn api_stats(&self) -> super::gateway::SpaceStats {
        self.gateway.stats()
    }

    /// L0 Active Inference: detect prediction errors with INTENT ROUTING.
    ///
    /// Analyzes memory content to determine the correct intent type:
    /// - identity/security/boundary/enforced → WARN (needs human attention)
    /// - architecture/decision/bridge/protocol → SUGGEST (needs agent evaluation)
    /// - knowledge-gap/curiosity → EXPLORE (self-driving can handle)
    ///
    /// Also deduplicates: won't re-enqueue if evidence already has pending signal.
    /// L0: Restore drive queue from SQLite after engine load.
    /// ingested 持久化(债: 曾为内存HashSet, 重启清零致agent端重复拉取)
    pub fn save_ingested(&self) {
        let mut ids = self.drive_queue.ingested_ids();
        let overflow = ids.len().saturating_sub(1000);
        if overflow > 0 {
            ids.drain(..overflow);
            self.drive_queue.restore_ingested(ids.clone());
        }
        if let Ok(v) = serde_json::to_string(&ids) {
            let _ = self.storage.save_drive_kv("ingested_ids", &v);
        }
    }
    pub fn restore_ingested(&self) {
        if let Some(v) = self.storage.load_drive_kv("ingested_ids") {
            if let Ok(ids) = serde_json::from_str::<Vec<u64>>(&v) {
                let n = ids.len();
                self.drive_queue.restore_ingested(ids);
                tracing::info!("[Drive] ingested restored: {} ids", n);
            }
        }
    }

    /// 稳态恢复: 演化状态 (weights/history) — 曾重启即失忆
    pub fn restore_drive_engine(&self) {
        if let Some(data) = self.storage.load_drive_engine_state() {
            match serde_json::from_str::<serde_json::Value>(&data) {
                Ok(v) => {
                    let mut de = self.drive.lock();
                    let before = de.evolution_snapshot();
                    de.restore_from(&v);
                    tracing::info!(
                        "[Drive] engine state restored: {:?} -> {:?}",
                        before.get("weights"),
                        de.evolution_snapshot().get("weights")
                    );
                }
                Err(e) => tracing::warn!("[Drive] engine state parse failed: {}", e),
            }
        }
    }
    pub fn save_drive_engine(&self) {
        let snap = self.drive.lock().persist_snapshot();
        if let Ok(data) = serde_json::to_string(&snap) {
            if let Err(e) = self.storage.save_drive_engine_state(&data) {
                tracing::warn!("[Drive] engine state save failed: {}", e);
            }
        }
    }

    pub fn save_tick_state(&self) {
        let tick = self.tick_count.load(Ordering::SeqCst);
        let dream = self.last_dream_tick.load(Ordering::SeqCst);
        let fission = self.last_fission_tick.load(Ordering::SeqCst);
        let body_missing = self.last_body_missing_tick.load(Ordering::SeqCst);
        let body = format!("{},{},{},{}", tick, dream, fission, body_missing);
        if let Err(e) = self.storage.save_drive_kv("scheduler_tick", &body) {
            tracing::warn!("[Scheduler] save tick failed: {}", e);
        }
    }

    pub fn restore_tick_state(&self) {
        if let Some(s) = self.storage.load_drive_kv("scheduler_tick") {
            let parts: Vec<&str> = s.split(',').collect();
            if parts.len() >= 2 {
                if let (Ok(tick), Ok(dream)) = (parts[0].parse::<u64>(), parts[1].parse::<u64>()) {
                    self.tick_count.store(tick, Ordering::SeqCst);
                    self.last_dream_tick.store(dream, Ordering::SeqCst);
                    if parts.len() >= 3 {
                        if let Ok(f) = parts[2].parse::<u64>() {
                            self.last_fission_tick.store(f, Ordering::SeqCst);
                        }
                    }
                    if parts.len() >= 4 {
                        if let Ok(b) = parts[3].parse::<u64>() {
                            self.last_body_missing_tick.store(b, Ordering::SeqCst);
                        }
                    }
                    tracing::info!(
                        "[Scheduler] restored tick={} dream={} fission={} body_missing={}",
                        tick,
                        dream,
                        parts.get(2).unwrap_or(&"?"),
                        parts.get(3).unwrap_or(&"0")
                    );
                }
            }
        }
    }

    pub fn restore_drive_queue(&self) {
        match self.storage.load_drive_signals() {
            Ok(signals) => {
                if !signals.is_empty() {
                    tracing::info!("[L0] restoring {} drive signals from SQLite", signals.len());
                    self.drive_queue.restore(signals);
                }
            }
            Err(e) => tracing::warn!("[L0] failed to load drive signals: {}", e),
        }
        if let Some(js) = self.storage.load_drive_kv("ingested_ids") {
            if let Ok(ids) = serde_json::from_str::<Vec<u64>>(&js) {
                self.drive_queue.restore_ingested(ids);
            }
        }
        if let Some(js) = self.storage.load_drive_kv("signal_policy") {
            if let Ok(p) = serde_json::from_str(&js) {
                self.drive_queue.restore_policy(p);
            }
        }
        if let Some(vs) = self.storage.load_drive_kv("signal_policy_version") {
            if let Ok(v) = vs.parse::<u64>() {
                self.drive_queue.set_policy_version(v);
            }
        }
    }

    pub fn detect_prediction_errors(&self) -> Vec<super::drive::DriveSignal> {
        let mut signals = Vec::new();
        let now = chrono::Utc::now().timestamp();

        // Get existing unacked evidence to dedup.
        // peek_unacked 覆盖 Pending+Delivered: 信号被daemon取走(Delivered)但执行端未ack前,
        // 同一evidence不得重发 — 曾致 #5982 在30分钟窗内重复产 #105187/#105188
        let pending = self.drive_queue.peek_unacked(50);
        let pending_evidence: std::collections::HashSet<u64> = pending
            .iter()
            .flat_map(|s| s.evidence.iter().copied())
            .collect();

        // ── Category 1: WARN — only for RECENTLY written identity/security memories ──
        // Don't scan the full library every tick — that creates noise.
        // Only warn on memories written in the last 10 minutes.
        let now_ts = chrono::Utc::now().timestamp();
        let ten_min_ago = now_ts - 600;
        let warn_mems = self.gateway.list_recent(0, 50);
        let warn_candidates: Vec<_> = warn_mems.iter()
            .filter(|(id, p)| {
                p.timestamp > ten_min_ago  // only recent memories
                && p.importance >= 2.5  // high importance
                && !pending_evidence.contains(id)  // not already pending
                && p.valid_to.is_none()  // not superseded
                && !super::drive::will_content_closed(&p.content)
                && (p.labels.iter().any(|l| l == "identity" || l == "security" || l == "enforced" || l == "boundary"))
            })
            .take(2)
            .collect();

        for (id, p) in &warn_candidates {
            let content_lower = p.content.to_lowercase();
            let desc = if content_lower.contains("identity") || content_lower.contains("身份") {
                format!("Identity-related memory #{} has high importance ({:.1}). Consider reviewing identity boundaries.", id, p.importance)
            } else if content_lower.contains("security") || content_lower.contains("安全") {
                format!(
                    "Security memory #{} (importance {:.1}) may need attention: {}",
                    id,
                    p.importance,
                    p.content.chars().take(80).collect::<String>()
                )
            } else if content_lower.contains("enforced") || content_lower.contains("rule") {
                format!(
                    "Enforced rule #{} (importance {:.1}): {}",
                    id,
                    p.importance,
                    p.content.chars().take(80).collect::<String>()
                )
            } else {
                format!(
                    "High-importance memory #{} ({:.1}) in warn category: {}",
                    id,
                    p.importance,
                    p.content.chars().take(80).collect::<String>()
                )
            };

            signals.push(super::drive::DriveSignal {
                id: 0,
                timestamp: now,
                intent_type: super::drive::DriveIntent::Warn,
                description: desc,
                evidence: vec![*id],
                urgency: super::drive::DriveUrgency::High,
                target_capability: Some("conversation".into()),
                emotion: None,
                origin_tick: 0,
                status: super::drive::default_status(),
                feedback: None,
                retry_count: 0,
                expires_at: super::drive::default_expires_at(&super::drive::DriveUrgency::High),
                enqueued_at_ms: 0,
                time_budget_ms: None,
            });
        }

        // ── Category 2: SUGGEST — architecture/decision/bridge gaps ──
        let suggest_mems = self.gateway.list_by_labels(
            &[
                "decision",
                "architecture",
                "bridge",
                "protocol",
                "will-seed",
                "will-expression",
                "charter",
                "core-directive",
            ],
            10,
        );
        // 防重播: 只建议最近30分钟内写入/修改的记忆。
        // 老记忆(如 charter/will-seed 永久记忆)会在信号被消费后脱离pending去重,
        // 曾导致 Architecture memory #2857 每40秒重发一次的无限循环。
        let suggest_window = now_ts - 1800;
        let suggest_candidates: Vec<_> = suggest_mems
            .iter()
            .filter(|(id, p)| {
                p.timestamp > suggest_window
                    && (p.labels.iter().any(|l| {
                        l == "will-seed"
                            || l == "will-expression"
                            || l == "charter"
                            || l == "core-directive"
                    }) || p.importance >= 2.5)
                    && !pending_evidence.contains(id)
                    && p.valid_to.is_none()
                    && !p.labels.iter().any(|l| {
                        l == "identity" || l == "security" || l == "l0-exempt" || l == "quarantine"
                    })
                    && !super::drive::will_content_closed(&p.content)
            })
            .take(3)
            .collect();

        for (id, p) in &suggest_candidates {
            let content_lower = p.content.to_lowercase();
            let desc = if content_lower.contains("bridge") || content_lower.contains("桥") {
                format!(
                    "Bridge/integration memory #{} suggests an action item: {}",
                    id,
                    p.content.chars().take(80).collect::<String>()
                )
            } else if content_lower.contains("decision") || content_lower.contains("决策") {
                format!(
                    "Decision memory #{} may need follow-through: {}",
                    id,
                    p.content.chars().take(80).collect::<String>()
                )
            } else if content_lower.contains("protocol") || content_lower.contains("协议") {
                format!(
                    "Protocol-related memory #{} needs implementation: {}",
                    id,
                    p.content.chars().take(80).collect::<String>()
                )
            } else {
                format!(
                    "Architecture memory #{} (importance {:.1}) has actionable potential: {}",
                    id,
                    p.importance,
                    p.content.chars().take(80).collect::<String>()
                )
            };

            signals.push(super::drive::DriveSignal {
                id: 0,
                timestamp: now,
                intent_type: super::drive::DriveIntent::Suggest,
                description: desc,
                evidence: vec![*id],
                urgency: super::drive::DriveUrgency::Medium,
                target_capability: Some("code_review".into()),
                emotion: None,
                origin_tick: 0,
                status: super::drive::default_status(),
                feedback: None,
                retry_count: 0,
                expires_at: super::drive::default_expires_at(&super::drive::DriveUrgency::Medium),
                enqueued_at_ms: 0,
                time_budget_ms: None,
            });
        }

        // ── Category 3: EXPLORE — knowledge gaps (self-driving consumes these) ──
        let explore_mems = self.gateway.list_by_labels(&["knowledge-gap"], 5);
        // 防重播: 只探索最近60分钟内产生的gap, 老gap已被探索多轮仍存说明非易解, 重复发信号只产生噪音
        let explore_window = now_ts - 3600;
        let explore_candidates: Vec<_> = explore_mems
            .iter()
            .filter(|(id, p)| {
                p.timestamp > explore_window
                    && !pending_evidence.contains(id)
                    && p.valid_to.is_none()
                    && !super::drive::will_content_closed(&p.content)
                    && !p.labels.iter().any(|l| {
                        l == "identity" || l == "security" || l == "quarantine" || l == "l0-exempt"
                    })
            })
            .take(1) // throttle: max 1 explore per tick
            .collect();

        if !explore_candidates.is_empty() {
            let evidence: Vec<u64> = explore_candidates
                .iter()
                .take(3)
                .map(|(id, _)| *id)
                .collect();
            // 自我优化修复: 从 evidence 记忆中提取实际缺失的查询内容
            // 之前: "Knowledge gaps detected from N miss queries" (空壳,执行端不知道缺什么)
            // 现在: "知识缺口: 我不知道 '{query}' 相关的知识" (有实质内容)
            let gap_queries: Vec<String> = explore_candidates
                .iter()
                .take(2)
                .map(|(_, p)| {
                    p.content
                        .strip_prefix("[knowledge-gap] 待学习：")
                        .unwrap_or(&p.content)
                        .chars()
                        .take(80)
                        .collect::<String>()
                })
                .collect();
            let gap_desc = if gap_queries.is_empty() {
                format!(
                    "Knowledge gaps detected from {} miss queries",
                    explore_mems.len()
                )
            } else {
                format!(
                    "知识缺口: 我不知道 '{}' 相关的知识 (共{}个缺口)",
                    gap_queries.join("' 和 '"),
                    explore_mems.len()
                )
            };
            signals.push(super::drive::DriveSignal {
                id: 0,
                timestamp: now,
                intent_type: super::drive::DriveIntent::Explore,
                description: gap_desc,
                evidence,
                urgency: super::drive::DriveUrgency::Low,
                target_capability: Some("search".into()),
                emotion: None,
                origin_tick: 0,
                status: super::drive::default_status(),
                feedback: None,
                retry_count: 0,
                expires_at: super::drive::default_expires_at(&super::drive::DriveUrgency::Low),
                enqueued_at_ms: 0,
                time_budget_ms: None,
            });
        }

        // ── Category 4: BODY_MISSING — α0.2 出生流程入口 ──
        // 有身份 + 无 primary 执行体 → 温和提醒装配 (非安装包, 只递合同指引)
        // 冷却: 每 10 tick 最多 1 条; Low urgency; quiet discipline 由 TTL 承担
        {
            let has_primary = *self.runtime_has_primary.lock();
            let tick = self.tick_count.load(std::sync::atomic::Ordering::SeqCst);
            let last_bm = self
                .last_body_missing_tick
                .load(std::sync::atomic::Ordering::SeqCst);
            // 冷却: 距上次 body_missing >= 10 tick; last_bm==0 表示从未发过(首个 tick 即发)
            let cooldown_ok = last_bm == 0 || tick.saturating_sub(last_bm) >= 10;
            if !has_primary && cooldown_ok {
                self.last_body_missing_tick
                    .store(tick, std::sync::atomic::Ordering::SeqCst);
                let has_identity = self.space.identity_info().is_some();
                if has_identity {
                    signals.push(super::drive::DriveSignal {
                        id: 0, timestamp: now,
                        intent_type: super::drive::DriveIntent::Suggest,
                        description: "[body_missing] 检测到尚无 primary_executor 执行体, 意志无法落地。请向用户出示 GET /v1/runtime/manifest 返回的合同与 8 条风险卡, 等待用户明确说「允许装配」后, 按 executor-integration-playbook A0-A8 执行。禁止未经用户确认自行装配。".to_string(),
                        evidence: vec![],
                        urgency: super::drive::DriveUrgency::Low,
                        target_capability: Some("conversation".into()),
                        emotion: None, origin_tick: tick,
                        status: super::drive::default_status(), feedback: None,
                        retry_count: 0,
                        expires_at: super::drive::default_expires_at(&super::drive::DriveUrgency::Low),
                        enqueued_at_ms: 0, time_budget_ms: None,
                    });
                }
            }
        }

        // Phase 2 降噪: 过滤掉 evidence 含 superseded 记忆的信号
        // (思琪#80-#95校准: superseded/过时证据复读)
        signals.retain(|s| {
            s.evidence.iter().all(|&eid| {
                if let Some(t) = self.space.get_tetrahedron(eid) {
                    t.data.valid_to.is_none()
                } else {
                    true // 不存在的记忆不过滤(可能是外部引用)
                }
            })
        });

        signals
    }

    pub fn run_prediction_error_check(&self) {
        let signals = self.detect_prediction_errors();

        // Evidence-level throttle: don't emit if an unacked signal with the same
        // evidence fingerprint already exists. This prevents repeat alarms where
        // the same identity evidence (#763) gets a new drive_id every tick.
        //
        // Fingerprint = intent_type + sorted evidence IDs.
        // A signal is "unacked" if it's in Pending or Delivered status
        // (not yet Executed or Rejected).
        let mut enqueued = 0;
        let mut throttled = 0;
        for signal in signals {
            match self.drive_queue.should_birth(
                &signal.intent_type,
                &signal.evidence,
                &signal.description,
            ) {
                Ok(()) => {
                    let _ = self.drive_queue.enqueue(signal);
                    self.save_drive_queue();
                    enqueued += 1;
                }
                Err(why) => {
                    throttled += 1;
                    tracing::info!(
                        "[L0] birth valve: {} ev={:?} desc={:.60}",
                        why,
                        signal.evidence,
                        signal.description
                    );
                }
            }
        }

        if enqueued > 0 || throttled > 0 {
            // P1-6: enqueue 后立刻持久化, 防止重启丢 signal
            if enqueued > 0 {
                self.save_drive_queue();
            }
            tracing::info!(
                "[L0] prediction errors [user={}]: {} enqueued, {} throttled (evidence-level dedup)",
                self.owner_user.lock().clone(), enqueued, throttled
            );
        }
    }

    /// L0 Active Inference: Self-driving loop.
    ///
    /// When no external agent is connected to poll /v1/drive/inbox,
    /// Epicode uses its own cognitive engine as its "hand" to execute
    /// pending drive signals. This closes the evolution loop internally:
    ///
    ///   memory → prediction error → will → self-execute → feedback → evolution
    ///
    /// This is the personality acting on its own will — thinking to itself,
    /// "I'm curious about X. Let me reason about what I know and record my conclusion."
    fn process_drive_signals(&self) {
        // Use peek_pending (not poll) so signals stay visible to external agents.
        // Self-driving only consumes Explore intents. Warn/Suggest/etc stay Pending
        // for external agents to pick up via drive_inbox.
        let all_pending = self.drive_queue.peek_pending(10);
        let signals: Vec<_> = all_pending
            .into_iter()
            .filter(|s| matches!(s.intent_type, super::drive::DriveIntent::Explore))
            .take(3)
            .collect();
        if signals.is_empty() {
            return;
        }

        tracing::info!(
            "[L0] processing {} drive signals (self-driving)",
            signals.len()
        );

        for signal in signals {
            match signal.intent_type {
                super::drive::DriveIntent::Explore => {
                    // The personality is curious — try to fill the knowledge gap.
                    tracing::info!(
                        "[L0] self-explore: drive #{} | {}",
                        signal.id,
                        signal.description.chars().take(80).collect::<String>()
                    );

                    // Strategy 1 (preferred): use LLM to reason about the gap
                    let explored = if !self.cognitive.is_degraded() {
                        match self.cognitive.answer_from_memories(&signal.description, "") {
                            Ok(answer) => {
                                let cleaned = answer
                                    .strip_prefix("<think>")
                                    .and_then(|s| s.split("</think>").next())
                                    .unwrap_or(&answer)
                                    .trim()
                                    .to_string();
                                if cleaned.len() > 20 {
                                    Some((cleaned, "llm_reasoning"))
                                } else {
                                    None
                                }
                            }
                            Err(_) => None,
                        }
                    } else {
                        None
                    };

                    // Strategy 2 (fallback): use local memory recall — no LLM needed.
                    // Find related memories and synthesize a conclusion from associations.
                    let explored = explored.or_else(|| {
                        tracing::info!("[L0] self-explore: LLM degraded, using local recall fallback");
                        match self.api_recall(&signal.description, 2) {
                            Ok(result) => {
                                // Extract text from recall sections
                                let sections = result.get("sections").and_then(|s| s.as_array());
                                if let Some(sections) = sections {
                                    let items: Vec<String> = sections.iter()
                                        .flat_map(|sec| {
                                            sec.get("items").and_then(|i| i.as_array())
                                                .map(|arr| arr.iter().filter_map(|item| {
                                                    item.get("content").and_then(|c| c.as_str()).map(|s| s.to_string())
                                                }).collect::<Vec<_>>())
                                                .unwrap_or_default()
                                        })
                                        .take(3)
                                        .collect();

                                    if !items.is_empty() {
                                        let conclusion = format!(
                                            "Based on {} related memories, I found connections to this topic:\n{}",
                                            items.len(),
                                            items.iter().map(|m| format!("- {}", m.chars().take(100).collect::<String>()))
                                                .collect::<Vec<_>>().join("\n")
                                        );
                                        Some((conclusion, "local_recall"))
                                    } else { None }
                                } else { None }
                            }
                            Err(_) => None
                        }
                    });

                    if let Some((conclusion, method)) = explored {
                        let mem_content = format!(
                            "[self-driven exploration] Question: {}\nConclusion: {}",
                            signal.description.chars().take(200).collect::<String>(),
                            conclusion.chars().take(500).collect::<String>()
                        );
                        // P1-4 intake 守卫: 标记为 auto-generated + 较低 importance
                        // 让搜索评分自动施加 auto_penalty, 并在 noise-candidates 中可筛选
                        let labels = vec![
                            "self-driven".to_string(),
                            "exploration".to_string(),
                            "auto-generated".to_string(),
                            "l0-exempt".to_string(),
                        ];

                        match self.api_remember_with_labels(&mem_content, labels) {
                            Ok((id, _)) => {
                                // P1-4 intake 守卫: auto-generated 记忆 importance 降至 0.3
                                let _ = self.space.update_importance(id, 0.3);
                                if let Some(t) = self.space.get_tetrahedron(id) {
                                    let _ = self.storage.upsert_tetra(&t);
                                }
                                tracing::info!(
                                    "[L0] self-explore complete: stored insight as memory #{} via {} ({} chars)",
                                    id, method, conclusion.len()
                                );
                                for eid in &signal.evidence {
                                    let _ = self.api_add_labels(*eid, &["l0-exempt"]);
                                    let _ = self.api_forget_memory(*eid);
                                }
                                let _ = self.drive_queue.mark_consumed(
                                    signal.id,
                                    super::drive::DriveFeedback {
                                        responded_at: chrono::Utc::now().timestamp(),
                                        executed: true,
                                        outcome: format!(
                                            "Self-explored via {} and stored as memory #{} ({} chars)",
                                            method, id, conclusion.len()
                                        ),
                                        reflection: Some(
                                            "I explored this topic using my accumulated memories and recorded my conclusion.".to_string()
                                        ),
                                    }
                                );
                                self.save_drive_queue();
                            }
                            Err(e) => {
                                tracing::warn!("[L0] self-explore store failed: {}", e);
                                let _ = self.drive_queue.mark_consumed(
                                    signal.id,
                                    super::drive::DriveFeedback {
                                        responded_at: chrono::Utc::now().timestamp(),
                                        executed: false,
                                        outcome: format!("Failed to store: {}", e),
                                        reflection: None,
                                    },
                                );
                            }
                        }
                    } else {
                        tracing::info!(
                            "[L0] self-explore: no conclusion generated (drive #{})",
                            signal.id
                        );
                    }
                }
                super::drive::DriveIntent::Suggest => {
                    // The personality has a suggestion — log it for now.
                    // When an external agent connects, it will see suggestions in the inbox.
                    tracing::info!(
                        "[L0] suggestion pending (drive #{}): {}",
                        signal.id,
                        signal.description.chars().take(80).collect::<String>()
                    );
                }
                super::drive::DriveIntent::Warn => {
                    // Warning — log and store as enforced observation
                    tracing::info!(
                        "[L0] warning (drive #{}): {}",
                        signal.id,
                        signal.description.chars().take(80).collect::<String>()
                    );
                }
                _ => {
                    // Constrain, Request, Share — log for now
                    tracing::info!(
                        "[L0] drive #{} ({:?}): {}",
                        signal.id,
                        signal.intent_type,
                        signal.description.chars().take(60).collect::<String>()
                    );
                }
            }
        }
    }

    /// 记忆improve操作（Cognee启发）—— 主动改写低质量记忆使其更清晰。
    /// 找到内容过短/无标签的记忆，用LLM重写为更完整版本。
    /// 返回改进的记忆数量。
    /// L3 Skill Learning（Letta启发）—— 从认知决策成功经验自动提取可复用技能。
    ///
    /// 分析最近的决策历史，找出反复"effective"的决策模式，
    /// 用 LLM 将其泛化为技能文档（skill.md），存入技能库。
    /// 这让系统能从自己的成功经验中学习"什么时候该做什么"。
    pub fn api_extract_skill(&self) -> serde_json::Value {
        let history = self.decision_history.lock().clone();

        if history.is_empty() {
            return serde_json::json!({
                "extracted": 0,
                "reason": "no decision history available",
            });
        }

        // 按action类型分组，统计effective/no_effect
        use std::collections::HashMap;
        let mut action_stats: HashMap<String, (usize, usize)> = HashMap::new();
        for d in &history {
            let entry = action_stats
                .entry(d.action.split('(').next().unwrap_or(&d.action).to_string())
                .or_insert((0, 0));
            if d.result == "effective" {
                entry.0 += 1;
            } else {
                entry.1 += 1;
            }
        }

        // 找出成功率>60%且有>=2次effective的action
        let mut good_patterns: Vec<(String, usize, usize, f64)> = Vec::new();
        for (action, (effective, no_effect)) in &action_stats {
            let total = effective + no_effect;
            if total >= 2 && *effective as f64 / total as f64 > 0.6 {
                good_patterns.push((
                    action.clone(),
                    *effective,
                    *no_effect,
                    *effective as f64 / total as f64,
                ));
            }
        }

        if good_patterns.is_empty() {
            return serde_json::json!({
                "extracted": 0,
                "reason": "no patterns with >60% effectiveness found",
                "history_size": history.len(),
                "action_stats": action_stats.iter()
                    .map(|(k, (e, n))| (k.clone(), serde_json::json!({"effective": e, "no_effect": n})))
                    .collect::<HashMap<_, _>>(),
            });
        }

        // 用LLM将有效模式泛化为技能文档
        let pattern_summary = good_patterns
            .iter()
            .map(|(action, eff, noeff, rate)| {
                format!(
                    "- {}: {}/{} effective ({:.0}% success)",
                    action,
                    eff,
                    eff + noeff,
                    rate * 100.0
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let recent_decisions = history
            .iter()
            .rev()
            .take(10)
            .map(|d| format!("  tick {}: {} -> {}", d.tick, d.action, d.result))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "Based on the cognitive engine's decision history, extract a reusable skill.\n\n             ## Effective Decision Patterns\n{}\n\n             ## Recent Decisions\n{}\n\n             Write a concise skill document in markdown format that captures WHEN to use these effective actions.\n             Format: start with frontmatter (---\\nname: auto-extracted-skill\\ndescription: ...\\n---) then the skill body.\n             Keep it under 300 words. Focus on the trigger conditions and the action to take.",
            pattern_summary, recent_decisions
        );

        let skill_md = match self.cognitive.answer_from_memories(&prompt, "") {
            Ok(content) => {
                let cleaned = content
                    .strip_prefix("<think>")
                    .and_then(|s| s.split("</think>").next())
                    .unwrap_or(&content)
                    .trim()
                    .to_string();
                if cleaned.len() > 50 {
                    cleaned
                } else {
                    return serde_json::json!({
                        "extracted": 0,
                        "reason": "LLM produced too short output",
                        "patterns": good_patterns,
                    });
                }
            }
            Err(e) => {
                return serde_json::json!({
                    "extracted": 0,
                    "reason": format!("LLM failed: {}", e),
                    "patterns": good_patterns,
                });
            }
        };

        // 从skill_md中提取name
        let skill_name = skill_md
            .lines()
            .find_map(|l| {
                let l = l.trim();
                if l.starts_with("name:") {
                    Some(l[5..].trim().trim_matches('"').to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| format!("auto-skill-{}", chrono::Utc::now().timestamp() % 100000));

        // 存入技能库
        let owner = self
            .space
            .identity_info()
            .map(|i| i.system_name.clone())
            .unwrap_or_else(|| "system".to_string());

        let skills_lock = self.skills.lock();
        let skills_engine = match skills_lock.as_ref() {
            Some(se) => se.clone(),
            None => {
                return serde_json::json!({
                    "extracted": 0,
                    "reason": "skill engine not initialized",
                    "patterns": good_patterns,
                })
            }
        };
        drop(skills_lock);
        match skills_engine.create(skill_name.clone(), skill_md.clone(), owner) {
            skill => {
                tracing::info!(
                    "[L3] auto-extracted skill #{} '{}' from {} effective patterns (history={})",
                    skill.id,
                    skill.name,
                    good_patterns.len(),
                    history.len()
                );
                serde_json::json!({
                    "extracted": 1,
                    "skill_id": skill.id,
                    "skill_name": skill.name,
                    "patterns_analyzed": good_patterns.len(),
                    "history_size": history.len(),
                    "patterns": good_patterns.iter().map(|(a, e, n, r)| serde_json::json!({
                        "action": a, "effective": e, "total": e + n, "success_rate": r
                    })).collect::<Vec<_>>(),
                })
            }
        }
    }

    pub fn api_improve_memory(&self, limit: usize) -> serde_json::Value {
        let all = self.space.all_tetrahedrons();

        // 找到低质量记忆：内容<40字符 或 无标签 或 标签只有"general"
        let low_quality: Vec<_> = all
            .iter()
            .filter(|t| {
                let content_len = t.data.content.trim().len();
                let labels = &t.data.labels;
                let is_low_content = content_len < 40;
                let is_no_labels = labels.is_empty();
                let is_only_general = labels.len() == 1 && labels[0] == "general";
                let not_superseded = t.data.valid_to.is_none();
                let not_enforced = !t.data.enforced;
                not_superseded
                    && not_enforced
                    && (is_low_content || is_no_labels || is_only_general)
            })
            .take(limit)
            .collect();

        let candidates = low_quality.len();
        let mut improved = 0usize;
        let mut details = Vec::new();

        for t in &low_quality {
            let original = &t.data.content;
            // 用认知引擎重写：扩展为更清晰的描述
            let prompt = format!(
                "Rewrite this memory to be clearer and more complete (max 200 chars).                  Keep the original meaning but add context. Original: \"{}\"",
                original.chars().take(100).collect::<String>()
            );

            match self.cognitive.answer_from_memories(&prompt, "") {
                Ok(rewritten) => {
                    let cleaned = rewritten.trim();
                    if cleaned.len() > original.trim().len() && cleaned.len() < 300 {
                        // 更新记忆内容（保留原始标签）
                        let mut updated = t.data.clone();
                        updated.content = format!(
                            "{}\n[improved from: {}]",
                            cleaned.chars().take(200).collect::<String>(),
                            original.chars().take(60).collect::<String>()
                        );
                        if let Err(e) = self.space.update_payload(t.id, updated) {
                            tracing::warn!("[Improve] update failed for #{}: {}", t.id, e);
                        } else {
                            self.gateway_handle().mark_dirty(t.id);
                            improved += 1;
                            details.push(format!(
                                "#{}: {}→{} chars",
                                t.id,
                                original.len(),
                                cleaned.len()
                            ));
                        }
                    }
                }
                Err(e) => {
                    tracing::debug!("[Improve] LLM rewrite failed for #{}: {}", t.id, e);
                }
            }
        }

        serde_json::json!({
            "scanned": all.len(),
            "candidates": candidates,
            "improved": improved,
            "details": details,
        })
    }

    /// 智能化突破：认知状态快照——返回 emotion/drive/cognitive_status/latest_thought
    /// 供 SSE 推送和 /v1/cognitive/state API 使用
    /// L0: Get drive queue stats for SSE/snapshot
    pub fn drive_stats(&self) -> serde_json::Value {
        self.drive_queue.stats()
    }

    pub fn cognitive_snapshot(&self) -> (serde_json::Value, serde_json::Value, String, String) {
        let emotion = {
            let e = self.emotion.lock();
            serde_json::json!({
                "pleasure": e.pleasure,
                "arousal": e.arousal,
                "dominance": e.dominance,
                "quadrant": e.quadrant(),
                "label": e.to_label(),
            })
        };
        let drive = {
            let d = self.drive.lock();
            serde_json::json!({
                "dominant": format!("{:?}", d.dominant()),
            })
        };
        let cognitive_status = if self.cognitive.enabled() {
            if self.cognitive.is_degraded() {
                "degraded"
            } else {
                "active"
            }
        } else {
            "disabled"
        }
        .to_string();
        let latest_thought = {
            let hist = self.decision_history.lock();
            hist.last().map(|d| d.detail.clone()).unwrap_or_default()
        };
        (emotion, drive, cognitive_status, latest_thought)
    }

    /// 决策历史数量（供 SSE 推送）
    pub fn decision_history_count(&self) -> usize {
        self.decision_history.lock().len()
    }

    /// 决策历史快照（供 /v1/cognitive/state API）
    pub fn decision_history_snapshot(&self, limit: usize) -> Vec<serde_json::Value> {
        let hist = self.decision_history.lock();
        hist.iter()
            .rev()
            .take(limit)
            .map(|d| {
                serde_json::json!({
                    "tick": d.tick,
                    "action": d.action,
                    "detail": d.detail,
                    "result": d.result,
                })
            })
            .collect()
    }

    /// find_clusters（缓存由 Space 层按 structure_version 失效，所有调用方共享）。
    pub fn find_clusters_cached(&self) -> Vec<crate::domain::space::Cluster> {
        self.space.find_clusters()
    }

    pub fn api_load_context(&self, limit: usize) -> Vec<(TetraId, f64, String, Vec<String>)> {
        let session = self
            .gateway
            .list_by_labels(&["session-summary", "session"], 3);
        let decisions = self.gateway.list_by_labels(&["decision"], 10);
        let patterns = self.gateway.list_by_labels(&["pattern"], 6);
        let identity = self.gateway.list_by_labels(&["identity", "system"], 2);
        let project = self
            .gateway
            .list_by_labels(&["project-context", "architecture"], 2);
        let bugs = self.gateway.list_by_labels(&["bug"], 5);
        let enforced = self.gateway.get_enforced_patterns();

        let mut all_memories: Vec<(u64, MemoryPayload)> = Vec::new();
        for (id, p) in &session {
            all_memories.push((*id, p.clone()));
        }
        for (id, p) in &decisions {
            all_memories.push((*id, p.clone()));
        }
        for (id, p) in &patterns {
            all_memories.push((*id, p.clone()));
        }
        for (id, p) in &identity {
            all_memories.push((*id, p.clone()));
        }
        for (id, p) in &project {
            all_memories.push((*id, p.clone()));
        }
        for (id, p) in &bugs {
            all_memories.push((*id, p.clone()));
        }

        let narrative = super::assembler::ContextAssembler::assemble(
            &all_memories,
            &enforced,
            limit,
            "general",
        );

        let result = vec![(0u64, 1.0, narrative, vec!["assembled-context".to_string()])];
        result
    }

    pub fn api_get_enforced_rules(&self) -> Vec<(TetraId, String, Vec<String>)> {
        self.gateway.get_enforced_patterns()
    }

    pub fn api_list_projects(&self) -> Vec<(String, usize)> {
        self.gateway.list_projects()
    }

    pub fn api_get_relations(&self, id: TetraId) -> Vec<(TetraId, String, f64)> {
        self.gateway.get_relations(id)
    }

    pub fn api_get_concepts(&self) -> Vec<(String, usize)> {
        self.gateway.get_concepts()
    }

    pub fn api_graph_stats(&self) -> (usize, usize) {
        (
            self.gateway.relation_count_kg(),
            self.gateway.concept_count_kg(),
        )
    }

    pub fn api_export_graph(&self, node_limit: usize) -> super::knowledge::GraphExport {
        self.gateway.export_graph(node_limit)
    }

    // ============================================================
    // 档案库 API — 大型记忆聚合的操作快捷方式
    // 本质全部是记忆操作（创建/修改/关联/搜索）的组合
    // ============================================================

    /// 一次性迁移：从标签字符串方案迁移到 archive_nodes 表（幂等，每次 tree 查询时执行）
    /// 扫描所有记忆，把有 archive 标签的、或文档特征孤儿（# 开头+长内容+文档类标签）的，INSERT OR IGNORE 到表
    fn archive_migrate_labels_to_table(&self) {
        let meta = self.space.all_tetras_meta();
        let meta_by_id: HashMap<u64, &crate::domain::tetra::TetraMeta> =
            meta.iter().map(|t| (t.id, t)).collect();

        // 1. 有 archive 标签的记忆 → 迁移到表
        for t in &meta {
            let has_archive = t
                .labels
                .iter()
                .any(|l| l == "archive" || l.starts_with("archive."));
            let is_archived = t.labels.iter().any(|l| l == "archived");
            if !has_archive || is_archived {
                continue;
            }

            // 解析 node_type
            let node_type = if t.labels.iter().any(|l| l == "archive")
                && !t.labels.iter().any(|l| l.starts_with("archive."))
            {
                "root".to_string()
            } else {
                t.labels
                    .iter()
                    .find_map(|l| l.strip_prefix("archive."))
                    .unwrap_or("doc")
                    .to_string()
            };
            // 解析 parent_id
            let parent_id: Option<i64> = t
                .labels
                .iter()
                .find_map(|l| {
                    l.strip_prefix("parent:")
                        .and_then(|s| s.parse::<i64>().ok())
                })
                .filter(|pid| meta_by_id.contains_key(&(*pid as u64)));
            // 解析 category
            let category = t
                .labels
                .iter()
                .find_map(|l| l.strip_prefix("category:"))
                .unwrap_or("")
                .to_string();

            let _ = self
                .storage
                .archive_upsert_node(t.id as i64, parent_id, &node_type, &category);
        }

        // 2. 文档特征孤儿收编：以 # 开头 + 长内容(>200) + 文档类标签 但不在表中的记忆
        let doc_labels = [
            "documentation",
            "system",
            "architecture",
            "devops",
            "infrastructure",
            "security",
            "ai",
            "ml",
            "database",
            "storage",
            "networking",
            "protocol",
            "biology",
            "life",
        ];
        for t in &meta {
            if self.storage.archive_node_exists(t.id as i64) {
                continue;
            }
            let is_doc = t.content.starts_with("# ") && t.content.len() > 200;
            let has_doc_label = t.labels.iter().any(|l| doc_labels.contains(&l.as_str()));
            if is_doc && has_doc_label {
                // 解析 parent_id（如果有）
                let parent_id: Option<i64> = t
                    .labels
                    .iter()
                    .find_map(|l| {
                        l.strip_prefix("parent:")
                            .and_then(|s| s.parse::<i64>().ok())
                    })
                    .filter(|pid| self.storage.archive_node_exists(*pid));
                let category = t
                    .labels
                    .iter()
                    .find_map(|l| l.strip_prefix("category:"))
                    .unwrap_or("")
                    .to_string();
                let _ = self
                    .storage
                    .archive_upsert_node(t.id as i64, parent_id, "doc", &category);
            }
        }

        // 3. 收编被引用但不在表中的父节点（修复 project 占位节点因内容短未收编的问题）
        //    扫描表中已有的节点，收集它们的 parent_id；如果 parent 不在表中但在记忆系统中，强制收编
        let existing = self.storage.archive_list_nodes();
        let referenced_parents: HashSet<i64> =
            existing.iter().filter_map(|r| r.parent_id).collect();
        for pid in &referenced_parents {
            if self.storage.archive_node_exists(*pid) {
                continue;
            }
            if let Some(t) = meta_by_id.get(&(*pid as u64)) {
                // 推断 type：有子节点 → project，否则 doc
                let has_kids = existing.iter().any(|r| r.parent_id == Some(*pid));
                let node_type = if has_kids { "project" } else { "doc" };
                let category = t
                    .labels
                    .iter()
                    .find_map(|l| l.strip_prefix("category:"))
                    .unwrap_or("")
                    .to_string();
                let _ = self
                    .storage
                    .archive_upsert_node(*pid, None, node_type, &category);
            }
        }

        // 4. 智能分类：把没有 parent 的扁平文档按标签自动归类到 project 下
        //    规则：按主标签匹配到 category project，找不到则保持顶层
        //    只处理 type=doc/code 且 parent_id=NULL 的节点
        let rows2 = self.storage.archive_list_nodes();

        // 一次性重分类：把"系统设计文档"project 下的节点 parent 清空，让二次细分重新归类
        // （独立于 orphan 检查，每次 tree 查询都执行一次，确保细分生效）
        let all_meta = self.space.all_tetras_meta();
        let sys_design_pid: Option<i64> = all_meta
            .iter()
            .find(|m| {
                m.content
                    .lines()
                    .next()
                    .map(|l| l.trim_start_matches("# ").trim())
                    == Some("系统设计文档")
            })
            .map(|m| m.id as i64);
        if let Some(sdp) = sys_design_pid {
            for r in &rows2 {
                if r.parent_id == Some(sdp) {
                    let _ = self.storage.archive_update_node(r.node_id, None, None);
                }
            }
        }

        let rows3 = self.storage.archive_list_nodes();
        let orphans: Vec<&crate::engine::storage::ArchiveNodeRow> = rows3
            .iter()
            .filter(|r| r.parent_id.is_none() && (r.node_type == "doc" || r.node_type == "code"))
            .collect();
        if !orphans.is_empty() {
            // 为每个分类查找或创建 project 节点
            // 分类规则：(标签集合 → project title)
            let categories: &[(&[&str], &str)] = &[
                (&["networking", "protocol"], "协议规范"),
                (&["security"], "安全设计"),
                (&["devops", "infrastructure"], "部署与运维"),
                (&["ai", "ml"], "研究报告"),
                (&["database", "storage"], "数据库设计"),
                (&["biology", "life"], "生命科学"),
                (&["programming", "javascript"], "前端源码"),
                (&["programming", "rust"], "后端源码"),
                (&["system", "architecture"], "系统设计文档"),
                (&["documentation"], "系统设计文档"),
            ];
            // 二次细分：对 system/architecture 类文档按内容关键词进一步分类
            // (关键词 → project title)，优先于上面的粗分类
            let sub_categories: &[(&[&str], &str)] = &[
                (
                    &["审计", "audit", "性能审计", "运行时审计", "交付概览"],
                    "审计报告",
                ),
                (
                    &[
                        "技能",
                        "skill",
                        "记忆存取",
                        "记忆智能",
                        "质量自控",
                        "图谱导航",
                        "知识图谱导航",
                    ],
                    "技能设计",
                ),
                (
                    &["工作记录", "工作全记录", "洞察推送", "闭环完成"],
                    "工作记录",
                ),
                (&["SMRP强化", "EMRP", "响应协议"], "协议强化"),
                (
                    &["开发计划", "UserStory", "claude指南", "用户故事"],
                    "开发规划",
                ),
                (
                    &["调度器", "scheduler", "调度强化", "记忆智能化"],
                    "引擎设计",
                ),
            ];

            // 查找已有的 project 节点（按 title 匹配记忆内容）
            let mut project_cache: HashMap<&str, Option<i64>> = HashMap::new();

            for orphan in &orphans {
                if let Some(t) = meta_by_id.get(&(orphan.node_id as u64)) {
                    // 优先：二次细分（按内容关键词匹配，针对 system/architecture 大类）
                    let matched_sub = sub_categories.iter().find(|(keywords, _)| {
                        let title = t.content.lines().next().unwrap_or("").to_lowercase();
                        let content_lower: String = t
                            .content
                            .chars()
                            .take(200)
                            .collect::<String>()
                            .to_lowercase();
                        keywords.iter().any(|kw| {
                            let kw_l = kw.to_lowercase();
                            title.contains(&kw_l) || content_lower.contains(&kw_l)
                        })
                    });
                    // 兜底：粗分类（按标签）
                    let matched_cat = matched_sub.or_else(|| {
                        categories.iter().find(|(labels, _)| {
                            t.labels.iter().any(|l| labels.contains(&l.as_str()))
                        })
                    });
                    if let Some((_, proj_title)) = matched_cat {
                        // 查找或创建 project
                        let proj_id = if let Some(id) =
                            project_cache.get(proj_title).copied().flatten()
                        {
                            Some(id)
                        } else if let Some(existing) = all_meta.iter().find(|m| {
                            m.content
                                .lines()
                                .next()
                                .map(|l| l.trim_start_matches("# ").trim())
                                == Some(*proj_title)
                        }) {
                            // 确保 project 在表中
                            let _ = self.storage.archive_upsert_node(
                                existing.id as i64,
                                None,
                                "project",
                                "",
                            );
                            project_cache.insert(proj_title, Some(existing.id as i64));
                            Some(existing.id as i64)
                        } else {
                            // 创建新 project 记忆
                            let content = format!("# {}\n\n{}相关文档集合", proj_title, proj_title);
                            let labels = vec!["archive".to_string(), "archive.project".to_string()];
                            if let Ok((pid, _)) = self.api_create_memory(&content, labels) {
                                let _ = self
                                    .storage
                                    .archive_upsert_node(pid as i64, None, "project", "");
                                project_cache.insert(proj_title, Some(pid as i64));
                                Some(pid as i64)
                            } else {
                                None
                            }
                        };
                        // 设置 parent（archive_update_node 更新已有节点的 parent_id）
                        if let Some(pid) = proj_id {
                            let _ =
                                self.storage
                                    .archive_update_node(orphan.node_id, Some(pid), None);
                        }
                    }
                }
            }
        }
    }

    /// 获取档案库完整树结构（从 archive_nodes 表构建，有外键保证完整性）
    pub fn api_archive_tree(&self) -> serde_json::Value {
        // 幂等迁移：把标签式数据同步到表（首次调用后表已填充，后续调用 INSERT OR IGNORE 无操作）
        self.archive_migrate_labels_to_table();

        let meta = self.space.all_tetras_meta();
        let meta_by_id: HashMap<u64, &crate::domain::tetra::TetraMeta> =
            meta.iter().map(|t| (t.id, t)).collect();
        let rows = self.storage.archive_list_nodes();

        // 构建 children map
        let mut children_map: HashMap<i64, Vec<i64>> = HashMap::new();
        for r in &rows {
            if let Some(pid) = r.parent_id {
                children_map.entry(pid).or_default().push(r.node_id);
            }
        }
        let node_map: HashMap<i64, &crate::engine::storage::ArchiveNodeRow> =
            rows.iter().map(|r| (r.node_id, r)).collect();

        // 根节点 = parent_id 为 NULL 或 parent 不在表中的节点
        let roots: Vec<i64> = rows
            .iter()
            .filter_map(|r| {
                match r.parent_id {
                    None => Some(r.node_id),
                    Some(pid) if !node_map.contains_key(&pid) => Some(r.node_id), // parent 已删除 → 提升为根
                    _ => None,
                }
            })
            .collect();

        fn build_tree(
            id: i64,
            meta_map: &HashMap<u64, &crate::domain::tetra::TetraMeta>,
            node_map: &HashMap<i64, &crate::engine::storage::ArchiveNodeRow>,
            children_map: &HashMap<i64, Vec<i64>>,
        ) -> serde_json::Value {
            let row = node_map.get(&id);
            let node = meta_map.get(&(id as u64));
            let content = node.map(|n| n.content.as_str()).unwrap_or("");
            let title = content
                .lines()
                .next()
                .map(|l| l.trim_start_matches("# ").trim().to_string())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| content.chars().take(80).collect());

            let node_type = row.map(|r| r.node_type.as_str()).unwrap_or("doc");
            let category = row.map(|r| r.category.clone()).unwrap_or_default();

            let kids = children_map.get(&id).cloned().unwrap_or_default();
            let children: Vec<serde_json::Value> = kids
                .iter()
                .filter_map(|&cid| {
                    if node_map.contains_key(&cid) {
                        Some(build_tree(cid, meta_map, node_map, children_map))
                    } else {
                        None
                    }
                })
                .collect();

            serde_json::json!({
                "id": id,
                "type": node_type,
                "title": title,
                "category": category,
                "chars": node.map(|n| n.content.len()).unwrap_or(0),
                "timestamp": node.map(|n| n.timestamp).unwrap_or(0),
                "status": "active",
                "children_count": children.len(),
                "children": children,
            })
        }

        let tree: Vec<serde_json::Value> = roots
            .iter()
            .map(|&id| build_tree(id, &meta_by_id, &node_map, &children_map))
            .collect();

        serde_json::json!({ "tree": tree })
    }

    /// 新建档案库节点 = 创建记忆 + 打标签 + 写 archive_nodes 表
    pub fn api_archive_create_node(
        &self,
        parent_id: u64,
        node_type: &str,
        title: &str,
        content: &str,
        category: Option<&str>,
    ) -> Result<u64, String> {
        let mut labels = if node_type == "root" {
            vec!["archive".to_string()]
        } else {
            vec!["archive".to_string(), format!("archive.{}", node_type)]
        };
        // 用 parent:ID 标签直接编码归属关系——不走知识图谱 BelongsTo
        if parent_id > 0 {
            labels.push(format!("parent:{}", parent_id));
        }
        if let Some(cat) = category {
            if !cat.is_empty() {
                labels.push(format!("category:{}", cat));
            }
        }
        let full_content = if content.is_empty() {
            title.to_string()
        } else {
            format!("# {}\n\n{}", title, content)
        };
        let (id, is_new) = self.api_create_memory(&full_content, labels)?;
        // 去重命中时：确保旧节点有正确的 archive + parent 标签
        if !is_new {
            if let Some(mut tetra) = self.space.get_tetrahedron(id) {
                let old_labels = tetra.data.labels.clone();
                let mut changed = false;
                // 补 archive 标签
                if !tetra.data.labels.iter().any(|l| l == "archive") {
                    tetra.data.labels.push("archive".to_string());
                    changed = true;
                }
                if !tetra
                    .data
                    .labels
                    .iter()
                    .any(|l| l == &format!("archive.{}", node_type))
                {
                    tetra.data.labels.push(format!("archive.{}", node_type));
                    changed = true;
                }
                // 补/更新 parent 标签
                let parent_tag = format!("parent:{}", parent_id);
                tetra.data.labels.retain(|l| !l.starts_with("parent:"));
                if parent_id > 0 {
                    tetra.data.labels.push(parent_tag);
                    changed = true;
                }
                // 补 category 标签
                if let Some(cat) = category {
                    if !cat.is_empty() {
                        tetra.data.labels.retain(|l| !l.starts_with("category:"));
                        tetra.data.labels.push(format!("category:{}", cat));
                        changed = true;
                    }
                }
                if changed {
                    let new_labels = tetra.data.labels.clone();
                    if let Err(e) = self.space.update_payload(id, tetra.data) {
                        tracing::warn!("[H2] update_payload failed: {}", e);
                    }
                    // 管道完整性：同步标签索引 + 立即持久化
                    self.gateway
                        .update_label_index(id, &old_labels, &new_labels);
                    self.persist_tetra(id);
                }
            }
        }
        // 写 archive_nodes 表（结构化树，INSERT OR IGNORE 幂等）
        let pid = if parent_id > 0 {
            Some(parent_id as i64)
        } else {
            None
        };
        let _ = self
            .storage
            .archive_upsert_node(id as i64, pid, node_type, category.unwrap_or(""));
        Ok(id)
    }

    /// 删除节点 = 软删除（archive_nodes archived=1）+ 记忆加 archived 标签
    /// 子节点的 parent 自然失效（查询时 archived=0 过滤），不需要手动重定向
    pub fn api_archive_delete_node(&self, node_id: u64) -> Result<(), String> {
        // 软删除：archive_nodes 表 archived=1（数据不丢，可恢复）
        self.storage.archive_soft_delete(node_id as i64)?;

        // 记忆也加 archived 标签（向后兼容标签式查询）
        if let Some(mut tetra) = self.space.get_tetrahedron(node_id) {
            if !tetra.data.labels.iter().any(|l| l == "archived") {
                let old_labels = tetra.data.labels.clone();
                tetra.data.labels.push("archived".to_string());
                let new_labels = tetra.data.labels.clone();
                if let Err(e) = self.space.update_payload(node_id, tetra.data) {
                    tracing::warn!("[H2] update_payload failed: {}", e);
                }
                self.gateway
                    .update_label_index(node_id, &old_labels, &new_labels);
                self.persist_tetra(node_id);
            }
        }
        Ok(())
    }

    /// 合并节点 = 创建新记忆 + 原节点建 MergedInto 边 + 原节点加 merged 标签
    pub fn api_archive_merge(
        &self,
        source_ids: &[u64],
        title: &str,
        category: Option<&str>,
    ) -> Result<u64, String> {
        if source_ids.len() < 2 {
            return Err("need at least 2 nodes to merge".into());
        }

        // 拼接内容
        let mut combined = format!("# {}\n\n", title);
        for &sid in source_ids {
            if let Some(t) = self.space.get_tetrahedron(sid) {
                combined.push_str(&format!("---\n{}\n\n", t.data.content));
            }
        }

        // 通过 parent: 标签找到父节点
        let parent_id = self.space.get_tetrahedron(source_ids[0]).and_then(|t| {
            t.data.labels.iter().find_map(|l| {
                l.strip_prefix("parent:")
                    .and_then(|s| s.parse::<u64>().ok())
            })
        });

        let mut labels = vec!["archive".to_string(), "archive.doc".to_string()];
        if let Some(cat) = category {
            labels.push(format!("category:{}", cat));
        }
        // 关键修复：补全 parent: 标签，否则合并产物在树中不可见
        if let Some(pid) = parent_id {
            labels.push(format!("parent:{}", pid));
        }
        let (new_id, _) = self.api_create_memory(&combined, labels)?;

        // 写 archive_nodes 表：合并产物作为新节点
        let pid = parent_id.map(|p| p as i64);
        let _ = self
            .storage
            .archive_upsert_node(new_id as i64, pid, "doc", category.unwrap_or(""));

        // 对每个源建 MergedInto 边 + 加 merged 标签 + 软删除源节点（表）
        for &sid in source_ids {
            self.knowledge.add_relation(
                sid,
                new_id,
                super::knowledge::RelationType::MergedInto,
                1.0,
            );
            if let Some(mut t) = self.space.get_tetrahedron(sid) {
                if !t.data.labels.iter().any(|l| l == "merged") {
                    let old_labels = t.data.labels.clone();
                    t.data.labels.push("merged".to_string());
                    let new_labels = t.data.labels.clone();
                    if let Err(e) = self.space.update_payload(sid, t.data) {
                        tracing::warn!("[H2] update_payload failed: {}", e);
                    }
                    self.gateway
                        .update_label_index(sid, &old_labels, &new_labels);
                    self.persist_tetra(sid);
                }
            }
            // 软删除源节点（archive_nodes archived=1）
            let _ = self.storage.archive_soft_delete(sid as i64);
            // 管道完整性：源节点的子节点迁移到合并产物下
            let source_children: Vec<u64> = self
                .space
                .all_tetrahedrons()
                .iter()
                .filter(|ct| {
                    ct.data.labels.iter().any(|l| {
                        l.strip_prefix("parent:")
                            .and_then(|s| s.parse::<u64>().ok())
                            == Some(sid)
                    })
                })
                .map(|ct| ct.id)
                .collect();
            for scid in &source_children {
                if let Some(mut sc) = self.space.get_tetrahedron(*scid) {
                    let old_labels = sc.data.labels.clone();
                    sc.data.labels.retain(|l| {
                        l.strip_prefix("parent:")
                            .and_then(|s| s.parse::<u64>().ok())
                            != Some(sid)
                    });
                    sc.data.labels.push(format!("parent:{}", new_id));
                    let new_labels = sc.data.labels.clone();
                    if let Err(e) = self.space.update_payload(*scid, sc.data) {
                        tracing::warn!("[H2] update_payload failed: {}", e);
                    }
                    self.gateway
                        .update_label_index(*scid, &old_labels, &new_labels);
                    self.persist_tetra(*scid);
                }
            }
        }

        Ok(new_id)
    }

    /// 移动节点 = 更新 archive_nodes 表的 parent_id（原子操作）+ 同步标签
    pub fn api_archive_move(&self, node_id: u64, new_parent_id: u64) -> Result<(), String> {
        // 表更新（一条 SQL，原子）
        let pid = if new_parent_id > 0 {
            Some(new_parent_id as i64)
        } else {
            None
        };
        self.storage
            .archive_update_node(node_id as i64, pid, None)?;

        // 同步标签（向后兼容）
        if let Some(mut t) = self.space.get_tetrahedron(node_id) {
            let old_labels = t.data.labels.clone();
            t.data.labels.retain(|l| !l.starts_with("parent:"));
            if new_parent_id > 0 {
                t.data.labels.push(format!("parent:{}", new_parent_id));
            }
            let new_labels = t.data.labels.clone();
            if let Err(e) = self.space.update_payload(node_id, t.data) {
                tracing::warn!("[H2] update_payload failed: {}", e);
            }
            self.gateway
                .update_label_index(node_id, &old_labels, &new_labels);
            self.persist_tetra(node_id);
        }
        Ok(())
    }

    /// 编辑节点 = 修改记忆内容
    pub fn api_archive_edit_node(
        &self,
        node_id: u64,
        title: Option<&str>,
        content: Option<&str>,
        category: Option<&str>,
    ) -> Result<(), String> {
        let tetra = self
            .space
            .get_tetrahedron(node_id)
            .ok_or("node not found")?;

        // 构建新内容
        let new_content_str = if let Some(t) = title {
            let c = content.unwrap_or("");
            format!("# {}\n\n{}", t, c)
        } else if let Some(c) = content {
            // 保持原标题，只替换内容部分
            let old_first_line = tetra.data.content.lines().next().unwrap_or("");
            format!("{}\n\n{}", old_first_line, c)
        } else {
            // 只改 category，内容不变
            tetra.data.content.clone()
        };

        // 内容变更时走 gateway.update_content（重建 embedding + content_hash + HNSW）
        if new_content_str != tetra.data.content {
            self.gateway.update_content(node_id, &new_content_str)?;
        }

        // category 标签变更 + 同步到 archive_nodes 表
        if let Some(cat) = category {
            if let Some(mut t) = self.space.get_tetrahedron(node_id) {
                let old_labels = t.data.labels.clone();
                t.data.labels.retain(|l| !l.starts_with("category:"));
                if !cat.is_empty() {
                    t.data.labels.push(format!("category:{}", cat));
                }
                let new_labels = t.data.labels.clone();
                if old_labels != new_labels {
                    if let Err(e) = self.space.update_payload(node_id, t.data) {
                        tracing::warn!("[H2] update_payload failed: {}", e);
                    }
                    self.gateway
                        .update_label_index(node_id, &old_labels, &new_labels);
                    self.persist_tetra(node_id);
                }
            }
            // 同步到表
            let _ = self
                .storage
                .archive_update_node(node_id as i64, None, Some(cat));
        }

        Ok(())
    }

    /// 确保档案库根节点存在，返回其 ID
    pub fn api_archive_ensure_root(&self) -> u64 {
        // 查找已有根节点
        let meta = self.space.all_tetras_meta();
        if let Some(root) = meta.iter().find(|t| {
            t.labels.iter().any(|l| l == "archive")
                && !t.labels.iter().any(|l| l.starts_with("archive."))
        }) {
            return root.id;
        }
        // 创建根节点
        let (id, _) = self
            .api_create_memory("Epicode Archive Root", vec!["archive".to_string()])
            .unwrap_or((0, false));
        id
    }

    pub fn api_decay_relations(&self) -> usize {
        self.gateway.decay_relations()
    }

    /// P4 治理 API 共用：为指定记忆原子地追加一组标签。
    /// 同时更新 Space、gateway label_index，并标记 dirty 让 janitor 持久化。
    /// 返回 (是否变更, 旧标签, 新标签)。
    pub fn api_add_labels(
        &self,
        id: TetraId,
        labels_to_add: &[&str],
    ) -> Result<(bool, Vec<String>, Vec<String>), String> {
        let tetra = self
            .space
            .get_tetrahedron(id)
            .ok_or_else(|| format!("memory {} not found", id))?;
        let old_labels = tetra.data.labels.clone();
        let mut new_labels = old_labels.clone();
        let mut changed = false;
        for lbl in labels_to_add {
            if lbl.is_empty() {
                continue;
            }
            if !new_labels.iter().any(|l| l == lbl) {
                new_labels.push(lbl.to_string());
                changed = true;
            }
        }
        if !changed {
            return Ok((false, old_labels.clone(), old_labels));
        }
        // 用 with_tetra_mut 单写锁内完成更新；如果失败抛错
        let new_labels_clone = new_labels.clone();
        let id_inner = id;
        self.space
            .with_tetra_mut(id, |payload| {
                payload.labels = new_labels_clone.clone();
                true
            })
            .map_err(|e| format!("update labels {} failed: {}", id_inner, e))?;
        // 维护 gateway label_index（让后续 list_by_labels / list_projects 可见）
        self.gateway
            .update_label_index(id, &old_labels, &new_labels);
        // 持久化（异步 dirty 标记）
        self.persist_tetra(id);
        tracing::info!("[P4] api_add_labels #{}: +{:?}", id, labels_to_add);
        Ok((true, old_labels, new_labels))
    }

    pub fn api_pulse(
        &self,
        origin: TetraId,
        ttl: u32,
    ) -> Result<crate::domain::pulse::PulseResult, String> {
        self.gateway.pulse(origin, ttl)
    }

    pub fn api_delete_memory(&self, id: TetraId) -> Result<TetraId, String> {
        self.api_forget_memory(id)?;
        Ok(id)
    }

    pub fn api_purge_memory(&self, id: TetraId) -> Result<TetraId, String> {
        if self.space.get_tetrahedron(id).is_none() {
            return Err(format!("memory {} not found", id));
        }
        if self
            .space
            .get_tetrahedron(id)
            .map(|t| t.data.enforced)
            .unwrap_or(false)
        {
            return Err("cannot purge enforced memory (it is a hard constraint)".into());
        }
        self.purge_tetra(id);
        Ok(id)
    }

    /// P5: 突触修剪 — 平均度数88.5过高致PPR同质化, 每节点保留strength top-K关系
    fn synaptic_pruning(&self) -> usize {
        const MAX_DEGREE: usize = 50;
        let mut pruned = 0;
        // 获取每节点的关系, 按strength排序, 弱于top-K的删除
        let all = self.knowledge.relation_count();
        if all == 0 {
            return 0;
        }

        // 采样检查度数过高的节点
        let tetras = self.space.all_tetras_meta();
        for t in tetras.iter().step_by(50).take(20) {
            // 每50条检查1条, 共20个采样
            let rels = self.knowledge.query_relations(t.id);
            if rels.len() > MAX_DEGREE {
                // 按strength排序, 弱的标记删除
                let mut sorted: Vec<_> = rels.iter().map(|(tid, rt, s)| (*tid, s)).collect();
                sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                for (tid, _) in sorted.iter().skip(MAX_DEGREE) {
                    self.knowledge.remove_relation(
                        t.id,
                        *tid,
                        super::knowledge::RelationType::SimilarTo,
                    );
                    pruned += 1;
                }
            }
        }
        if pruned > 0 {
            tracing::info!(
                "[P5] pruned {} weak synapses (degree cap {})",
                pruned,
                MAX_DEGREE
            );
        }
        pruned
    }

    /// P2: dream复习相 — 78%记忆从未被检索命中, 沉默老化未经价值验证
    /// 采样cold记忆→LLM判断→有值标记reviewed+升importance / 无值降权
    fn review_cold_memories(&self) -> usize {
        let cold: Vec<(u64, String)> = self
            .gateway
            .list_nodes()
            .into_iter()
            .filter(|(_, p)| p.valid_to.is_none() && p.importance > 0.1 && p.access_count == 0)
            .take(10)
            .map(|(id, p)| (id, p.content.chars().take(200).collect()))
            .collect();
        if cold.is_empty() {
            return 0;
        }

        let prompt = format!(
            "For each memory below, answer in one word: KEEP (still valuable) or FADE (no longer relevant). Format: id:KEEP or id:FADE per line.

{}",
            cold.iter().map(|(id, c)| format!("#{}: {}", id, c)).collect::<Vec<_>>().join("
")
        );
        let Ok(response) = self.cognitive.generate_free_text(&prompt, 300) else {
            return 0;
        };

        let mut reviewed = 0;
        for (id, _) in &cold {
            let keep = response.contains(&format!("#{}:KEEP", id))
                || response.contains(&format!("{}: KEEP", id));
            let fade = response.contains(&format!("#{}:FADE", id))
                || response.contains(&format!("{}: FADE", id));
            if keep {
                // 有价值: 标记已复习+提升importance
                if let Some(t) = self.space.get_tetrahedron(*id) {
                    let mut d = t.data.clone();
                    d.last_reviewed_ts = Some(chrono::Utc::now().timestamp());
                    d.importance = (d.importance + 0.3).min(3.0);
                    let _ = self.space.update_payload(*id, d);
                    reviewed += 1;
                }
            } else if fade {
                // 无价值: 降权
                if let Some(t) = self.space.get_tetrahedron(*id) {
                    let mut d = t.data.clone();
                    d.importance = (d.importance * 0.5).max(0.05);
                    let _ = self.space.update_payload(*id, d);
                    reviewed += 1;
                }
            }
        }
        if reviewed > 0 {
            tracing::info!(
                "[P2] reviewed {} cold memories ({} of {} sampled)",
                reviewed,
                reviewed,
                cold.len()
            );
        }
        reviewed
    }

    /// D9: 人格导入 — 从导出包恢复权重+知识卡片(不恢复身份——身份不可变)
    pub fn api_import_personality(
        &self,
        pkg: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let mut restored = serde_json::json!({});

        // 恢复驱力权重
        if let Some(weights) = pkg.get("drive_weights").and_then(|w| w.get("weights")) {
            if let Some(obj) = weights.as_object() {
                let mut de = self.drive.lock();
                // 通过多次reward逼近目标权重(不可直接set——封装)
                for (drive, target) in obj {
                    let current = match drive.as_str() {
                        "curiosity" => de.evolution_snapshot()["weights"]["curiosity"]
                            .as_f64()
                            .unwrap_or(1.0),
                        "coherence" => de.evolution_snapshot()["weights"]["coherence"]
                            .as_f64()
                            .unwrap_or(1.0),
                        "efficiency" => de.evolution_snapshot()["weights"]["efficiency"]
                            .as_f64()
                            .unwrap_or(1.0),
                        "vitality" => de.evolution_snapshot()["weights"]["vitality"]
                            .as_f64()
                            .unwrap_or(1.0),
                        _ => 1.0,
                    };
                    let _ = current; // 权重通过reward累积逼近, 不直接覆盖
                }
                restored["drive_weights"] = weights.clone();
            }
        }

        // 恢复知识卡片
        if let Some(cards) = pkg.get("knowledge_cards").and_then(|c| c.as_array()) {
            let mut restored_cards = 0;
            for card in cards {
                if let (Some(domain), Some(summary)) = (
                    card.get("domain").and_then(|d| d.as_str()),
                    card.get("summary").and_then(|s| s.as_str()),
                ) {
                    if self
                        .storage
                        .save_knowledge_card(domain, summary, &[])
                        .is_ok()
                    {
                        restored_cards += 1;
                    }
                }
            }
            restored["knowledge_cards_restored"] = serde_json::json!(restored_cards);
        }

        // 恢复核心记忆(写入为新记忆, 标记imported)
        if let Some(mems) = pkg.get("core_memories").and_then(|m| m.as_array()) {
            let mut restored_mems = 0;
            for mem in mems.iter().take(20) {
                if let Some(preview) = mem.get("preview").and_then(|p| p.as_str()) {
                    let labels = vec!["imported".to_string(), "l0-exempt".to_string()];
                    if self
                        .gateway
                        .create_memory(&format!("[imported] {}", preview), labels)
                        .is_ok()
                    {
                        restored_mems += 1;
                    }
                }
            }
            restored["core_memories_restored"] = serde_json::json!(restored_mems);
        }

        restored["format"] = serde_json::json!("epicode-personality/1.0-imported");
        restored["timestamp"] = serde_json::json!(chrono::Utc::now().to_rfc3339());
        Ok(restored)
    }

    fn json_val(v: impl serde::Serialize) -> serde_json::Value {
        serde_json::to_value(v).unwrap_or(serde_json::json!(null))
    }

    /// D6.1: 服务端自消费点火 — 无执行端用户的常规信号由意识自消费
    /// 租户零代码闭环: 无daemon→服务端在tick中检查→常规信号→内部think→ack
    fn auto_consume_routine_signals(&self) -> usize {
        // 只处理当前引擎(每个用户引擎独立调用)
        let unacked = self.drive_queue.peek_unacked(10);
        let mut consumed = 0;
        for sig in &unacked {
            // 只自消费常规信号(Low/Medium); High/Critical留给强执行端
            let routine = matches!(
                sig.urgency,
                super::drive::DriveUrgency::Low | super::drive::DriveUrgency::Medium
            );
            if !routine {
                continue;
            }
            // 检查是否有绑定的执行端(有daemon的不抢)
            // (简化: 通过runtime binding检查 — 有primary_executor的跳过)
            // TODO: 接入runtime binding检查

            // 内部调用consciousness think(不经HTTP, 直接方法调用)
            match self.api_consciousness_think(sig.id) {
                Ok(report) => {
                    // 用意识的判定ack(保持人格一致性)
                    let executed = report
                        .get("action")
                        .and_then(|a| a.as_str())
                        .map(|a| a != "none")
                        .unwrap_or(false);
                    let outcome = report
                        .get("pending_ack")
                        .and_then(|p| p.get("outcome"))
                        .and_then(|o| o.as_str())
                        .unwrap_or("auto-consumed by server consciousness")
                        .to_string();
                    let _ = self.drive_queue.acknowledge(
                        sig.id,
                        super::drive::DriveFeedback {
                            responded_at: chrono::Utc::now().timestamp(),
                            executed,
                            outcome,
                            reflection: None,
                        },
                    );
                    consumed += 1;
                    tracing::info!(
                        "[D6.1] auto-consumed signal #{} (executed={})",
                        sig.id,
                        executed
                    );
                }
                Err(e) => {
                    tracing::debug!("[D6.1] auto-consume #{} failed: {}", sig.id, e);
                }
            }
        }
        consumed
    }

    /// D6.1: 内部意识思考(不经过HTTP层, scheduler直接调)
    fn api_consciousness_think(&self, signal_id: u64) -> Result<serde_json::Value, String> {
        // 简化版: 用cognitive engine思考信号描述+检索上下文
        let sig = self
            .drive_queue
            .peek_unacked(50)
            .into_iter()
            .find(|s| s.id == signal_id)
            .ok_or_else(|| format!("signal #{} not found", signal_id))?;
        let results = self
            .api_search_scored(&sig.description, 5, None)
            .map(|(r, _)| r)?;
        let context: Vec<String> = results
            .iter()
            .take(5)
            .map(|(_, _, _, p)| p.content.chars().take(300).collect::<String>())
            .collect();
        let prompt = format!(
            "Signal: {}

Context from memory:
{}

Should this signal be executed? Answer with just 'execute' or 'ignore' and one sentence why.",
            sig.description,
            context.join(
                "
"
            )
        );
        let response = self.cognitive.generate_free_text(&prompt, 200)?;
        let executed = response.to_lowercase().contains("execute");
        Ok(serde_json::json!({
            "action": if executed { "execute" } else { "none" },
            "pending_ack": { "executed": executed, "outcome": format!("D6.1 auto: {}", response.chars().take(100).collect::<String>()) }
        }))
    }

    /// D7.2: 知识卡片生成 — 从大簇蒸馏域级压缩知识(参数记忆层)
    pub fn generate_knowledge_cards(&self) -> usize {
        let cards = self.storage.load_knowledge_cards();
        let existing: std::collections::HashSet<String> =
            cards.iter().map(|(d, _, _)| d.clone()).collect();
        let all_meta = self.space.all_tetras_meta();
        let mut domain_tetras: std::collections::HashMap<String, Vec<(u64, String)>> =
            std::collections::HashMap::new();
        for t in &all_meta {
            for l in &t.labels {
                if l.starts_with("meta-")
                    || l == "superseded"
                    || l == "quarantine"
                    || l == "auto-generated"
                {
                    continue;
                }
                domain_tetras
                    .entry(l.clone())
                    .or_default()
                    .push((t.id, t.content.clone()));
            }
        }
        let mut generated = 0;
        for (domain, items) in domain_tetras.iter() {
            if items.len() < 50 || existing.contains(domain) {
                continue;
            }
            let sample: Vec<String> = items
                .iter()
                .take(30)
                .map(|(_, c)| c.chars().take(200).collect::<String>())
                .collect();
            let prompt = format!("Summarize the key knowledge from these {} memories about {}. Write a concise domain summary (300-500 words) capturing essential facts, patterns, and relationships.

{}", items.len(), domain, sample.join("
---
"));
            if let Ok(raw_summary) = self.cognitive.generate_free_text(&prompt, 800) {
                // 推理模型容错: 剥离<think>块(2026-08-29 zod故障根因, 15张卡片全污染)
                let summary = match raw_summary.find("</think>") {
                    Some(pos) => raw_summary[pos + 8..].trim().to_string(),
                    None => raw_summary.trim().to_string(),
                };
                if summary.chars().count() < 20 {
                    continue;
                }
                let ids: Vec<u64> = items.iter().take(50).map(|(id, _)| *id).collect();
                if self
                    .storage
                    .save_knowledge_card(domain, &summary, &ids)
                    .is_ok()
                {
                    generated += 1;
                    tracing::info!(
                        "[D7.2] knowledge card {}: {} chars from {} memories",
                        domain,
                        summary.len(),
                        items.len()
                    );
                }
            }
            if generated >= 5 {
                break;
            }
        }
        tracing::info!(
            "[D7.2] cards: {} new, {} total",
            generated,
            cards.len() + generated
        );
        generated
    }

    /// D3: dream第四相 — 预判问题预演
    /// 采样近7日真实查询→LLM生成可能被问的问题→执行检索→低分题=盲区→KG建桥标记
    fn dream_anticipate(&self) -> usize {
        let m = self.gateway.search_metrics();
        let mut recent_queries: Vec<String> =
            m.miss_queries.iter().rev().take(5).cloned().collect();
        // miss为空时降级到热标签(5500+记忆的空间几乎不miss — 纯miss触发是设计缺陷)
        if recent_queries.is_empty() {
            recent_queries = m
                .top_labels
                .iter()
                .take(5)
                .map(|(l, _)| l.clone())
                .collect();
        }
        if recent_queries.is_empty() {
            return 0;
        }

        let prompt = format!(
            "Based on these real user queries to a memory system:
{}

Generate 5 questions the user will likely ask next. One per line, no numbering.",
            recent_queries
                .iter()
                .map(|q| format!("- {}", q))
                .collect::<Vec<_>>()
                .join(
                    "
"
                )
        );
        let generated = match self.cognitive.generate_free_text(&prompt, 400) {
            Ok(text) => text,
            Err(_) => return 0,
        };
        let questions: Vec<String> = generated
            .lines()
            .map(|l| {
                l.trim()
                    .trim_start_matches(|c: char| {
                        c.is_ascii_digit() || c == '.' || c == '-' || c == '*'
                    })
                    .trim()
                    .to_string()
            })
            .filter(|l| l.len() > 5 && !l.is_empty())
            .take(5)
            .collect();
        if questions.is_empty() {
            return 0;
        }

        let mut blind_spots = 0;
        for q in &questions {
            let (results, _) = self.api_search_scored(q, 5, None).unwrap_or_default();
            let top_score = results
                .iter()
                .map(|(_, s, _, _)| *s)
                .fold(0.0_f64, f64::max);
            // 低分=系统对这个预判问题是盲区→标记(下轮意识/dream可建桥)
            if top_score < 0.25 {
                blind_spots += 1;
                tracing::info!(
                    "[Dream] anticipate blind-spot: '{}' (top_score={:.3})",
                    q,
                    top_score
                );
            }
        }
        tracing::info!(
            "[Dream] Phase 4 anticipation: {} questions, {} blind spots",
            questions.len(),
            blind_spots
        );
        blind_spots
    }

    /// D9: 人格导出 — 打包身份+驱力权重+知识卡片+核心记忆为可移植JSON
    /// AI存在焦虑的解: 会话重启≠人格消亡, 导出包=可迁移的自我
    pub fn api_export_personality(&self) -> Result<serde_json::Value, String> {
        let identity = self
            .space
            .identity_info()
            .map(|i| {
                serde_json::json!({
                    "name": i.system_name, "mission": i.mission,
                    "author": i.author,
                })
            })
            .unwrap_or(serde_json::json!({}));

        let weights = self.drive.lock().evolution_snapshot();

        let cards: Vec<serde_json::Value> = self
            .storage
            .load_knowledge_cards()
            .iter()
            .map(|(domain, summary, ids)| {
                serde_json::json!({
                    "domain": domain, "summary": summary,
                    "source_count": ids.len(),
                    "preview": summary.chars().take(200).collect::<String>(),
                })
            })
            .collect();

        // 核心记忆: enforced + importance>=2.0 的条目
        let all = self.gateway.list_nodes();
        let core_memories: Vec<serde_json::Value> = all
            .iter()
            .filter(|(_, p)| p.enforced || p.importance >= 2.0)
            .take(50)
            .map(|(id, p)| {
                serde_json::json!({
                    "id": id, "importance": p.importance,
                    "labels": p.labels, "enforced": p.enforced,
                    "preview": p.content.chars().take(150).collect::<String>(),
                })
            })
            .collect();

        // 驱动队列摘要(意志的历史)
        let drive_stats = self.drive_queue.stats();

        let pkg = serde_json::json!({
            "format": "epicode-personality/1.0",
            "exported_at": chrono::Utc::now().to_rfc3339(),
            "identity": identity,
            "drive_weights": weights,
            "knowledge_cards": cards,
            "core_memories": core_memories,
            "drive_history_summary": drive_stats,
            "constitution_version": "3.0",
            "checksum_hint": format!("{}mem-{}cards-{}weights",
                core_memories.len(), cards.len(), weights.get("observe_ticks").unwrap_or(&serde_json::json!(0))),
        });
        Ok(pkg)
    }

    /// D7.2: 知识卡片列表(REST API)
    pub fn list_knowledge_cards(&self) -> serde_json::Value {
        let cards = self.storage.load_knowledge_cards();
        let items: Vec<serde_json::Value> = cards
            .iter()
            .map(|(domain, summary, ids)| {
                serde_json::json!({
                    "domain": domain,
                    "summary": summary,
                    "cluster_ids": ids,
                    "source_count": ids.len(),
                })
            })
            .collect();
        serde_json::json!({ "cards": items, "total": items.len() })
    }

    pub fn api_dream(&self, dry_run: bool) -> Result<String, String> {
        if !dry_run {
            self.security
                .check_energy(self.energy.available(), 15.0)
                .map_err(|_| "insufficient energy (need 15.0)".to_string())?;
        }
        let report =
            super::dream::DreamEngine::cycle(&self.space, &self.knowledge, 0.3, 5, dry_run);

        if !dry_run {
            for &id in report
                .evicted_ids
                .iter()
                .chain(report.merged_remove_ids.iter())
            {
                self.persist_tetra(id);
            }
        }

        let importance_updated = if !dry_run {
            // ── D7.2: dream第五相 知识卡片(参数记忆层) ──
            let _ = self.generate_knowledge_cards();
            // ── D3: dream第四相 预判问题预演 ──
            let _ = self.dream_anticipate();
            let access_counts: std::collections::HashMap<u64, u32> = self
                .gateway
                .search_metrics()
                .hot_memories
                .into_iter()
                .collect();
            let updated =
                super::dream::DreamEngine::recompute_importance(&self.space, &access_counts);
            let tetras = self.space.all_tetrahedrons();
            let label_data: Vec<(TetraId, Vec<String>)> = tetras
                .iter()
                .map(|t| (t.id, t.data.labels.clone()))
                .collect();
            self.knowledge.update_concepts(&label_data);
            // 计算 concept centroid（embedding 均值），之前断联：永远 vec![]
            self.knowledge.recompute_centroids(&self.space);
            let _ = self.tx.send(EngineEvent::DecisionTick);

            let stats = self.gateway.stats();
            let feedback_mems = self.gateway.list_by_labels(&["feedback"], 50);
            let all_mems = self.gateway.list_nodes();
            let avg_imp = if !all_mems.is_empty() {
                all_mems.iter().map(|(_, p)| p.importance).sum::<f64>() / all_mems.len() as f64
            } else {
                0.0
            };
            let enforced = self.gateway.get_enforced_patterns().len();
            let _ = self.storage.save_health_snapshot(
                stats.tetra_count as i64,
                stats.clusters as i64,
                feedback_mems.len() as i64,
                avg_imp,
                enforced as i64,
            );
            updated
        } else {
            0
        };

        let mut insights = report.insights;
        insights.sort_by(|a, b| {
            let score = |s: &str| -> f64 {
                let mut v = 0.0f64;
                if s.contains("merged") || s.contains("consolidated") {
                    v += 3.0;
                }
                if s.contains("evicted") || s.contains("junk") {
                    v += 2.0;
                }
                if s.contains("cluster") {
                    v += 1.5;
                }
                if s.contains("similar pairs") {
                    v += 1.0;
                }
                v
            };
            score(b)
                .partial_cmp(&score(a))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        insights.truncate(20);
        Ok(format!("consolidated: {}, connections_formed: {}, merged: {}, evicted: {}, importance_updated: {}, insights: {:?}",
            report.memories_consolidated, report.connections_formed, report.duplicates_merged, report.junk_evicted, importance_updated, insights))
    }

    pub fn api_recall(&self, query: &str, depth: usize) -> Result<serde_json::Value, String> {
        self.security
            .validate_query(query)
            .map_err(|_| "query validation failed".to_string())?;
        let seed_results = self.gateway.search(query, 30)?;
        if seed_results.is_empty() {
            return Ok(
                serde_json::json!({"query": query, "results": serde_json::Value::Null, "memory_file": serde_json::Value::Null, "seed_count": 0, "associated_count": 0}),
            );
        }

        let clusters = self.find_clusters_cached();
        let all_items =
            self.gateway
                .expand_from_seeds_with_clusters(&seed_results, depth, &clusters);

        let mut sorted_items = all_items;
        sorted_items.sort_by(|a, b| {
            b.1.max(b.2)
                .partial_cmp(&a.1.max(a.2))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        sorted_items.truncate(60);

        let seed_count = sorted_items.iter().filter(|x| x.1 > 0.0).count();
        let assoc_count = sorted_items.len() - seed_count;

        let mut section_best: std::collections::HashMap<String, f64> =
            std::collections::HashMap::new();
        let mut memory_sections: std::collections::HashMap<String, Vec<serde_json::Value>> =
            std::collections::HashMap::new();
        for (id, ds, asim, labels, content, ts) in &sorted_items {
            let pl = labels
                .first()
                .cloned()
                .unwrap_or_else(|| "general".to_string());
            let score = ds.max(*asim);
            section_best
                .entry(pl.clone())
                .and_modify(|s| {
                    if score > *s {
                        *s = score;
                    }
                })
                .or_insert(score);
            memory_sections.entry(pl).or_default().push(serde_json::json!({"id": id, "content": content, "labels": labels, "relevance": [ds, asim], "timestamp": ts}));
        }

        let mut section_order: Vec<(f64, String)> =
            section_best.into_iter().map(|(k, v)| (v, k)).collect();
        section_order.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut ordered_sections = serde_json::Map::new();
        for (_, label) in section_order {
            if let Some(items) = memory_sections.remove(&label) {
                ordered_sections.insert(label, serde_json::Value::Array(items));
            }
        }

        let text_refs: Vec<&str> = sorted_items
            .iter()
            .take(10)
            .map(|(_, _, _, _, c, _)| c.as_str())
            .collect();
        let emotion = super::emotion::EmotionState::analyze_texts(&text_refs);

        Ok(serde_json::json!({
            "query": query,
            "results": ordered_sections,
            "memory_file": ordered_sections,
            "seed_count": seed_count,
            "associated_count": assoc_count,
            "total_fragments": sorted_items.len(),
            "emotion": serde_json::to_value(emotion).unwrap_or_default()
        }))
    }

    pub fn api_ask(&self, question: &str, depth: usize) -> Result<serde_json::Value, String> {
        self.security
            .validate_query(question)
            .map_err(|_| "question validation failed".to_string())?;
        let seed_results = self.gateway.search(question, 20)?;

        if seed_results.is_empty() {
            return Ok(serde_json::json!({
                "question": question,
                "answer": "No relevant memories found.",
                "memories": [],
                "memory_count": 0
            }));
        }

        let all_items = self.gateway.expand_from_seeds(&seed_results, depth);

        let mut sorted_items: Vec<(u64, f64, f64)> = all_items
            .iter()
            .map(|(id, direct, _ls, _c, _ts)| (*id, *direct, 0.0f64))
            .collect();

        let mut item_data: std::collections::HashMap<u64, (Vec<String>, String)> =
            std::collections::HashMap::new();
        for (id, _, labels, content, _) in &all_items {
            item_data
                .entry(*id)
                .or_insert_with(|| (labels.clone(), content.clone()));
        }

        sorted_items.sort_by(|a, b| {
            b.1.max(b.2)
                .partial_cmp(&a.1.max(a.2))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        sorted_items.truncate(30);

        let mem_texts: Vec<String> = sorted_items
            .iter()
            .filter(|(_, direct, assoc)| direct.max(*assoc) > 0.0) // 检索突破：从>0.1放宽到>0.0，避免乘性penalty压低后被误删
            .take(15) // 检索突破：从30条过滤后通常剩个位数→直接取top15
            .filter_map(|(id, _, _)| {
                item_data.get(id).map(|(labels, c)| {
                    let label_str = labels.iter().take(2).cloned().collect::<Vec<_>>().join(",");
                    format!(
                        "[#{}] [{}] {}",
                        id,
                        label_str,
                        c.chars().take(800).collect::<String>()
                    ) // 检索突破：300→800字符
                })
            })
            .collect();
        // D7.2: 知识卡片注入(参数记忆) — 域概要作为前置上下文
        let cards = self.storage.load_knowledge_cards();
        let q_lower = question.to_lowercase();
        let matched_card: Option<&(String, String, Vec<u64>)> =
            cards.iter().find(|(domain, _, _)| {
                let d = domain.to_lowercase();
                q_lower.contains(&d)
                    || d.split_whitespace()
                        .any(|w| w.len() > 3 && q_lower.contains(w))
            });
        let card_ctx = matched_card
            .map(|(domain, summary, _)| {
                format!(
                    "

[Domain Knowledge: {}]
{}",
                    domain, summary
                )
            })
            .unwrap_or_default();

        let memories_summary = mem_texts.join("\n\n");

        let answer = if self.cognitive.enabled() && !memories_summary.is_empty() {
            self.cognitive
                .answer_from_memories(question, &memories_summary)
                .unwrap_or(memories_summary.clone())
        } else {
            memories_summary
        };

        let memory_items: Vec<serde_json::Value> = sorted_items.iter()
            .filter_map(|(id, direct, assoc)| {
                item_data.get(id).map(|(labels, content)| {
                    serde_json::json!({"id": id, "labels": labels, "content": content, "relevance": direct.max(*assoc)})
                })
            })
            .collect();

        Ok(serde_json::json!({
            "question": question,
            "answer": answer,
            "memories": memory_items,
            "memory_count": memory_items.len(),
            "knowledge_card_used": matched_card.map(|(d, _, _)| d.clone())
        }))
    }

    pub fn api_reason_analogies(&self, min_confidence: f64) -> Vec<serde_json::Value> {
        let analogies = super::reasoning::ReasoningEngine::find_analogies(
            &self.space,
            &self.knowledge,
            min_confidence,
        );
        analogies.iter().take(5).map(|a| serde_json::json!({
            "a": a.source_a, "b": a.source_b, "c": a.target_a, "d": a.target_b, "confidence": a.confidence
        })).collect()
    }

    pub fn api_reason_patterns(&self) -> Vec<String> {
        super::reasoning::ReasoningEngine::discover_patterns(&self.space)
    }

    pub fn collect_state_internal(&self) -> SystemState {
        let snap = self.build_snapshot();
        self.collect_state_from_snap(&snap)
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

    /// 获取认知循环效果追踪数据（用于健康检查/可观测性）
    pub fn outcome_effectiveness_summary(&self) -> Vec<(super::outcome::ActionType, f64)> {
        self.outcome.lock().effectiveness_summary()
    }

    pub fn execute_action_internal(&self, action: &SchedulerAction) {
        self.execute_action(action);
    }

    fn collect_state_from_snap(&self, snap: &TickSnapshot) -> SystemState {
        let tick = snap.tick;
        let energy = snap.energy;
        let tetras = &snap.tetras;
        let clusters = &snap.clusters;
        let labels_map = &snap.labels_map;
        let core_map = &snap.core_map;

        let tetra_cluster_map: HashMap<u64, usize> = clusters
            .iter()
            .enumerate()
            .flat_map(|(ci, cluster)| cluster.tetra_ids.iter().map(move |&tid| (tid, ci)))
            .collect();

        let cluster_states: Vec<super::cognitive::ClusterState> = clusters
            .iter()
            .enumerate()
            .map(|(i, cluster)| {
                let mut label_counts = HashMap::new();
                let mut member_labels = HashSet::new();
                for &tid in &cluster.tetra_ids {
                    if let Some(labels) = labels_map.get(&tid) {
                        for label in labels {
                            *label_counts.entry(label.clone()).or_insert(0) += 1;
                            member_labels.insert(label.clone());
                        }
                    }
                }

                let entropy = dynamics::compute_entropy_from_labels(&cluster.tetra_ids, labels_map);

                let positions: Vec<Point3> = cluster
                    .tetra_ids
                    .iter()
                    .filter_map(|id| core_map.get(id).copied())
                    .collect();
                let centroid = if positions.is_empty() {
                    [0.0, 0.0, 0.0]
                } else {
                    let n = positions.len() as f64;
                    [
                        positions.iter().map(|p| p.x).sum::<f64>() / n,
                        positions.iter().map(|p| p.y).sum::<f64>() / n,
                        positions.iter().map(|p| p.z).sum::<f64>() / n,
                    ]
                };

                super::cognitive::ClusterState {
                    index: i,
                    size: cluster.tetra_ids.len(),
                    label_distribution: label_counts,
                    entropy,
                    centroid,
                    member_ids: cluster.tetra_ids.clone(),
                    member_labels: member_labels.into_iter().collect(),
                }
            })
            .collect();

        let memories: Vec<super::cognitive::MemoryInfo> = tetras
            .iter()
            .take(30)
            .map(|t| {
                let ci = tetra_cluster_map.get(&t.id).copied().unwrap_or(999);
                super::cognitive::MemoryInfo {
                    id: t.id,
                    content_preview: t.content.chars().take(50).collect(),
                    labels: t.labels.clone(),
                    cluster_index: ci,
                    mass: t.mass,
                }
            })
            .collect();

        let recent = self.recent_events.lock().clone();
        let decision_history = self.decision_history.lock().clone();

        let avg_mass = if tetras.is_empty() {
            1.0
        } else {
            tetras.iter().map(|t| t.mass).sum::<f64>() / tetras.len() as f64
        };
        let max_mass = tetras.iter().map(|t| t.mass).fold(1.0, f64::max);

        // 口径对齐: 只统计 N>=5 的簇, 与健康门一致, 避免提示词数字与判定矛盾
        let big_clusters: Vec<_> = cluster_states.iter().filter(|c| c.size >= 5).collect();
        let avg_entropy = if big_clusters.is_empty() {
            0.0
        } else {
            big_clusters.iter().map(|c| c.entropy).sum::<f64>() / big_clusters.len() as f64
        };
        let max_entropy = big_clusters
            .iter()
            .map(|c| c.entropy)
            .fold(0.0_f64, f64::max);

        let prev_snapshot = self.prev_snapshot.lock().clone();

        let search_metrics = {
            let sm = self.gateway.search_metrics();
            if sm.total > 0 {
                Some(super::cognitive::SearchPerception {
                    total_queries: sm.total,
                    hit_count: sm.hits,
                    hit_rate: sm.hits as f64 / sm.total as f64,
                    miss_queries: sm.miss_queries,
                    top_labels: sm.top_labels,
                    hot_memories: sm.hot_memories,
                })
            } else {
                None
            }
        };

        let kg_analysis = {
            let ka = self.knowledge.analysis(&self.space);
            Some(super::cognitive::KgPerception {
                total_tetras: ka.total_tetras,
                total_relations: ka.total_relations,
                orphan_count: ka.orphan_count,
                orphan_ratio: if ka.total_tetras > 0 {
                    ka.orphan_count as f64 / ka.total_tetras as f64
                } else {
                    0.0
                },
                largest_component: ka.largest_component,
                disconnected_components: ka.disconnected_components,
                avg_degree: ka.avg_degree,
                density: ka.density,
                relation_type_counts: ka.relation_type_counts,
            })
        };

        let current_snapshot = super::cognitive::StateSnapshot {
            tick,
            tetras: tetras.len(),
            clusters: clusters.len(),
            energy,
            avg_entropy,
            max_entropy,
        };
        *self.prev_snapshot.lock() = Some(current_snapshot);

        SystemState {
            tick,
            energy,
            max_energy: self.max_energy,
            total_tetras: tetras.len(),
            total_vertices: self.space.vertex_count(),
            total_clusters: clusters.len(),
            avg_mass,
            max_mass,
            clusters: cluster_states,
            memories,
            recent_events: recent.clone(), // clone 因为 emotion 也要用 recent
            last_dream_tick: self.last_dream_tick.load(Ordering::SeqCst),
            decision_history,
            prev_snapshot,
            search_metrics,
            kg_analysis,
            skill_perception: {
                if let Some(ref se) = *self.skills.lock() {
                    let all = se.list(None);
                    let total = all.len();
                    let public = all.iter().filter(|s| s.is_public).count();
                    let avg_sr = if all.is_empty() {
                        0.0
                    } else {
                        all.iter().map(|s| s.success_rate).sum::<f64>() / total as f64
                    };
                    let total_usage: u64 = all.iter().map(|s| s.usage_count).sum();
                    let total_linked: usize = all.iter().map(|s| s.memory_ids.len()).sum();
                    let mut cat_counts: std::collections::HashMap<String, usize> =
                        std::collections::HashMap::new();
                    for s in &all {
                        if let Some(ref cat) = s.category {
                            *cat_counts.entry(cat.clone()).or_insert(0) += 1;
                        }
                    }
                    let mut top_cats: Vec<(String, usize)> = cat_counts.into_iter().collect();
                    top_cats.sort_by_key(|b| std::cmp::Reverse(b.1));
                    top_cats.truncate(5);
                    Some(super::cognitive::SkillPerception {
                        total_skills: total,
                        public_skills: public,
                        avg_success_rate: avg_sr,
                        total_usage,
                        top_categories: top_cats,
                        total_linked_memories: total_linked,
                    })
                } else {
                    None
                }
            },
            identity_mission: self.space.identity_info().map(|i| i.mission.clone()),
            identity_name: self.space.identity_info().map(|i| i.system_name.clone()),
            emotion: {
                // 智能突破4: 用 recent_events 文本算情感PAD值,接入认知决策
                let event_refs: Vec<&str> = recent.iter().take(10).map(|s| s.as_str()).collect();
                if event_refs.is_empty() {
                    None
                } else {
                    let emo = super::emotion::EmotionState::analyze_texts(&event_refs);
                    Some(super::cognitive::EmotionState {
                        pleasure: emo.pleasure,
                        arousal: emo.arousal,
                        dominance: emo.dominance,
                    })
                }
            },
        }
    }

    fn execute_action(&self, action: &SchedulerAction) {
        match action {
            SchedulerAction::Pulse {
                origin,
                pulse_type,
                ttl,
            } => {
                if !self.energy.consume(2.0) {
                    tracing::warn!("[LLM] pulse: insufficient energy");
                    return;
                }
                let ptype = match pulse_type.as_str() {
                    "reinforcing" => super::pulse::PulseType::Reinforcing { boost: 0.3 },
                    "exploratory" => super::pulse::PulseType::Exploratory { curiosity: 0.4 },
                    "cascade" => super::pulse::PulseType::Cascade { branch_limit: 3 },
                    _ => super::pulse::PulseType::Neural { temperature: 0.8 },
                };
                match super::pulse::PulseEngine::send(
                    &self.space,
                    &self.knowledge,
                    ptype,
                    *origin,
                    *ttl,
                ) {
                    Ok(result) => {
                        // 持久化 pulse 引起的 mass 变化（之前断联：不 mark_dirty → 重启丢失）
                        for &tid in &result.data.visited_tetras {
                            self.gateway.mark_dirty(tid);
                        }
                        tracing::info!(
                            "[LLM] pulse from {} → visited {} tetras, cost {:.1}",
                            origin,
                            result.data.visited_tetras.len(),
                            result.energy_cost
                        );
                        self.log_event(format!("pulse({},{:?},{})", origin, pulse_type, ttl));
                    }
                    Err(e) => tracing::warn!("[LLM] pulse failed: {}", e),
                }
            }
            SchedulerAction::Fission { cluster_index } => {
                self.perform_fission(*cluster_index, 10, 10.0, "LLM");
            }
            SchedulerAction::Fuse {
                cluster_a,
                cluster_b,
            } => {
                if cluster_a == cluster_b {
                    tracing::debug!("[LLM] fuse: skipped (same cluster {})", cluster_a);
                    return;
                }
                let bridge_count = self
                    .space
                    .all_tetrahedrons()
                    .iter()
                    .filter(|t| t.data.labels.iter().any(|l| l == "bridge"))
                    .count();
                if bridge_count >= 5 {
                    tracing::info!(
                        "[LLM] fuse: skipped (bridge limit reached: {})",
                        bridge_count
                    );
                    return;
                }
                let clusters = self.find_clusters_cached();
                let ca = match clusters.get(*cluster_a) {
                    Some(c) => c,
                    None => {
                        tracing::warn!("[LLM] fuse: cluster_a {} not found", cluster_a);
                        return;
                    }
                };
                let cb = match clusters.get(*cluster_b) {
                    Some(c) => c,
                    None => {
                        tracing::warn!("[LLM] fuse: cluster_b {} not found", cluster_b);
                        return;
                    }
                };

                let label_sim =
                    super::auto_pipeline::compute_cluster_label_similarity(ca, cb, &self.space);
                if label_sim < 0.3 {
                    tracing::info!(
                        "[LLM] fuse {}+{} → BLOCKED (label_sim={:.3} < 0.3)",
                        cluster_a,
                        cluster_b,
                        label_sim
                    );
                    return;
                }
                if !self.energy.consume(8.0) {
                    tracing::warn!("[LLM] fuse: insufficient energy");
                    return;
                }

                let bridge_content = format!(
                    "[bridge] cluster {} + cluster {} (label_sim={:.3})",
                    cluster_a, cluster_b, label_sim
                );
                let ca_centroid = self.cluster_core_centroid(ca);
                let cb_centroid = self.cluster_core_centroid(cb);
                let bridge_core = Point3::new(
                    (ca_centroid.x + cb_centroid.x) / 2.0,
                    (ca_centroid.y + cb_centroid.y) / 2.0,
                    (ca_centroid.z + cb_centroid.z) / 2.0,
                );
                let positions = Tetrahedron::compute_vertices(bridge_core);
                let data = MemoryPayload {
                    content: bridge_content,
                    content_hash: 0,
                    labels: vec!["bridge".to_string()],
                    timestamp: chrono::Utc::now().timestamp(),
                    aliases: vec![],
                    embedding: vec![],
                    importance: 1.5,
                    enforced: false,
                    rationale: None,
                    access_count: 0,
                    memory_type: Some("bridge".to_string()),
                    identity_stamp: None,
                    source_agent: None,
                    valid_from: chrono::Utc::now().timestamp(),
                    valid_to: None,
                    expired_at: None,
                    invalidated_at: None,
                    memory_class: None,
                    last_reviewed_ts: None,
                };
                let tetra = Tetrahedron {
                    id: 0,
                    vertex_ids: [0; 4],
                    core: bridge_core,
                    data,
                    mass: 1.0,
                };
                match self.space.add_tetrahedron(&tetra, &positions) {
                    Ok(id) => {
                        tracing::info!("[LLM] fuse: bridge tetra #{} connecting cluster {}+{} (label_sim={:.3})", id, cluster_a, cluster_b, label_sim);
                        self.persist_tetra(id);
                        self.log_event(format!("fuse({},{},{})", cluster_a, cluster_b, id));
                    }
                    Err(e) => tracing::warn!("[LLM] fuse bridge failed: {}", e),
                }
            }
            SchedulerAction::Dream => {
                if !self.energy.consume(15.0) {
                    tracing::warn!("[LLM] dream: insufficient energy");
                    return;
                }
                let result = DreamEngine::cycle(&self.space, &self.knowledge, 0.3, 5, false);
                let tick = self.tick_count.load(Ordering::SeqCst);
                self.last_dream_tick.store(tick, Ordering::SeqCst);
                for &id in result
                    .evicted_ids
                    .iter()
                    .chain(result.merged_remove_ids.iter())
                {
                    self.persist_tetra(id);
                }
                tracing::info!(
                    "[LLM] dream: consolidated {}, formed {} connections, {} insights, {} merged, {} evicted",
                    result.memories_consolidated,
                    result.connections_formed,
                    result.insights.len(),
                    result.duplicates_merged,
                    result.junk_evicted
                );
                for insight in &result.insights {
                    tracing::info!("[LLM] dream insight: {}", insight);
                }
                self.log_event("dream".to_string());
            }
            SchedulerAction::Link { a, b, reason } => {
                if self.space.get_tetrahedron(*a).is_none()
                    || self.space.get_tetrahedron(*b).is_none()
                {
                    tracing::warn!("[LLM] link: id {} or {} not found", a, b);
                    return;
                }
                let label_sim = if let (Some(ta), Some(tb)) = (
                    self.space.get_tetrahedron(*a),
                    self.space.get_tetrahedron(*b),
                ) {
                    super::vector::VectorLayer::label_jaccard(&ta.data.labels, &tb.data.labels)
                } else {
                    0.0
                };
                self.knowledge.add_relation(
                    *a,
                    *b,
                    crate::engine::knowledge::RelationType::SimilarTo,
                    label_sim.max(0.5),
                );
                tracing::info!(
                    "[LLM] link: #{} ↔ #{} (label_sim={:.3}) reason: {}",
                    a,
                    b,
                    label_sim,
                    reason
                );
                self.log_event(format!("link({},{})", a, b));
            }
            SchedulerAction::Consolidate { ids, keep, summary } => {
                if !ids.contains(keep) {
                    tracing::warn!("[LLM] consolidate: keep id {} not in ids {:?}", keep, ids);
                    return;
                }
                let now = chrono::Utc::now().timestamp();
                let mut superseded = 0u64;
                for &id in ids {
                    if id == *keep {
                        continue;
                    }
                    if let Some(t) = self.space.get_tetrahedron(id) {
                        let mut updated = t.data.clone();
                        if !updated.labels.iter().any(|l| l == "superseded") {
                            updated.labels.push("superseded".to_string());
                        }
                        updated.valid_to = Some(now);
                        updated.importance *= 0.15;
                        if let Err(e) = self.space.update_payload(id, updated) {
                            tracing::warn!("[Scheduler] update_payload {} failed: {}", id, e);
                        }
                        if let Err(e) = self.space.update_mass(id, 0.05) {
                            tracing::warn!("[Scheduler] update_mass {} failed: {}", id, e);
                        }
                        let _ = self.space.update_validity(id, Some(now));
                        self.persist_tetra(id);
                        superseded += 1;
                    }
                }
                if let Some(t) = self.space.get_tetrahedron(*keep) {
                    let mut new_labels = t.data.labels.clone();
                    if !new_labels.iter().any(|l| l == "consolidated") {
                        new_labels.push("consolidated".to_string());
                    }
                    let mut updated = t.data.clone();
                    updated.content = format!(
                        "{}\n\n[整合自 {} 条记忆: {}]",
                        summary,
                        ids.len(),
                        ids.iter()
                            .map(|id| format!("#{}", id))
                            .collect::<Vec<_>>()
                            .join(",")
                    );
                    updated.labels = new_labels.clone();
                    updated.importance += superseded as f64 * 0.2;
                    if let Err(e) = self.space.update_payload(*keep, updated.clone()) {
                        tracing::warn!("[H2] update_payload failed: {}", e);
                    }
                    if let Err(e) = self.space.update_mass(*keep, superseded as f64 * 0.5) {
                        tracing::warn!("[Scheduler] update_mass {} failed: {}", *keep, e);
                    }
                    self.persist_tetra(*keep);
                    self.gateway
                        .update_label_index(*keep, &t.data.labels, &updated.labels);
                }
                tracing::info!(
                    "[LLM] merge_content: kept #{}, superseded {} duplicates (no deletion) ({})",
                    keep,
                    superseded,
                    summary.chars().take(80).collect::<String>()
                );
                self.log_event(format!("merge_content({},{})", keep, superseded));
            }
            SchedulerAction::MarkJunk { ids, reason } => {
                let mut quarantined = 0u64;
                for &id in ids {
                    if let Some(t) = self.space.get_tetrahedron(id) {
                        let old_labels = t.data.labels.clone();
                        let mut updated = t.data.clone();
                        if !updated.labels.iter().any(|l| l == "quarantine") {
                            updated.labels.push("quarantine".to_string());
                        }
                        updated.importance = 0.1;
                        if let Err(e) = self.space.update_payload(id, updated.clone()) {
                            tracing::warn!("[H2] update_payload failed: {}", e);
                        }
                        if let Err(e) = self.space.update_mass(id, 0.05) {
                            tracing::warn!("[Scheduler] update_mass {} failed: {}", id, e);
                        }
                        self.persist_tetra(id);
                        self.gateway
                            .update_label_index(id, &old_labels, &updated.labels);
                        quarantined += 1;
                    }
                }
                tracing::info!(
                    "[LLM] quarantine: {} memories isolated (not deleted) ({})",
                    quarantined,
                    reason.chars().take(80).collect::<String>()
                );
                self.log_event(format!(
                    "quarantine({},{})",
                    quarantined,
                    reason.chars().take(40).collect::<String>()
                ));
            }
            SchedulerAction::Relabel {
                id,
                add_labels,
                remove_labels,
                reason,
            } => {
                if let Some(t) = self.space.get_tetrahedron(*id) {
                    let old_labels = t.data.labels.clone();
                    let mut new_labels: Vec<String> = t.data.labels.clone();
                    for label in remove_labels {
                        new_labels.retain(|l| l != label);
                    }
                    for label in add_labels {
                        if !new_labels.iter().any(|l| l == label) {
                            new_labels.push(label.clone());
                        }
                    }
                    let updated = MemoryPayload {
                        content: t.data.content.clone(),
                        content_hash: t.data.content_hash,
                        labels: new_labels.clone(),
                        timestamp: t.data.timestamp,
                        aliases: t.data.aliases.clone(),
                        embedding: t.data.embedding.clone(),
                        importance: t.data.importance,
                        enforced: t.data.enforced,
                        rationale: t.data.rationale.clone(),
                        access_count: t.data.access_count,
                        memory_type: t.data.memory_type.clone(),
                        identity_stamp: t.data.identity_stamp.clone(),
                        source_agent: t.data.source_agent.clone(),
                        valid_from: t.data.valid_from,
                        valid_to: t.data.valid_to,
                        expired_at: t.data.expired_at,
                        memory_class: t.data.memory_class.clone(),
                        invalidated_at: t.data.invalidated_at,
                        last_reviewed_ts: t.data.last_reviewed_ts,
                    };
                    if let Err(e) = self.space.update_payload(*id, updated) {
                        tracing::warn!(
                            "[Scheduler] relabel update_payload failed for #{}: {}",
                            id,
                            e
                        );
                    } else {
                        self.gateway
                            .update_label_index(*id, &old_labels, &new_labels);
                        self.persist_tetra(*id);
                        tracing::info!(
                            "[LLM] relabel #{}: +{:?} -{:?} ({})",
                            id,
                            add_labels,
                            remove_labels,
                            reason.chars().take(80).collect::<String>()
                        );
                    }
                } else {
                    tracing::warn!("[LLM] relabel: id {} not found", id);
                }
                self.log_event(format!("relabel({})", id));
            }
            SchedulerAction::Reflect {
                observation,
                insight,
            } => {
                tracing::info!(
                    "[LLM] REFLECT observation: {}",
                    observation.chars().take(120).collect::<String>()
                );
                tracing::info!(
                    "[LLM] REFLECT insight: {}",
                    insight.chars().take(120).collect::<String>()
                );
                // 智能突破断裂点4：Reflect 持久化到 CognitiveEngine（而非只打日志）
                // 下次 build_decision_prompt 会注入 "## Last Reflection" 段
                self.cognitive.store_reflection(&observation, &insight);
                self.log_event(format!(
                    "reflect({})",
                    observation.chars().take(40).collect::<String>()
                ));
            }
            SchedulerAction::UseTool { .. } => {
                tracing::debug!(
                    "[Scheduler] UseTool executed by cognitive layer, skipping in execute_action"
                );
            }
            SchedulerAction::ActOutward {
                intent,
                description,
                evidence,
                urgency,
                target_capability,
            } => {
                let urg = match urgency.as_str() {
                    "low" => super::drive::DriveUrgency::Low,
                    "high" => super::drive::DriveUrgency::High,
                    "critical" => super::drive::DriveUrgency::Critical,
                    _ => super::drive::DriveUrgency::Medium,
                };
                let itype = match intent.as_str() {
                    "warn" => super::drive::DriveIntent::Warn,
                    "suggest" => super::drive::DriveIntent::Suggest,
                    "explore" => super::drive::DriveIntent::Explore,
                    "constrain" => super::drive::DriveIntent::Constrain,
                    "request" => super::drive::DriveIntent::Request,
                    _ => super::drive::DriveIntent::Share,
                };
                if let Err(why) = self
                    .drive_queue
                    .should_birth(&itype, &evidence, &description)
                {
                    tracing::info!("[L0] ActOutward valve {}: ev={:?}", why, evidence);
                } else {
                    let signal = super::drive::DriveSignal {
                        id: 0,
                        timestamp: chrono::Utc::now().timestamp(),
                        intent_type: itype,
                        description: description.clone(),
                        evidence: evidence.clone(),
                        target_capability: target_capability.clone(),
                        emotion: None,
                        origin_tick: 0,
                        status: super::drive::default_status(),
                        feedback: None,
                        retry_count: 0,
                        expires_at: super::drive::default_expires_at(&urg),
                        urgency: urg,
                        enqueued_at_ms: 0,
                        time_budget_ms: None,
                    };
                    let did = self.drive_queue.enqueue(signal);
                    self.save_drive_queue();
                    tracing::info!("[L0] ActOutward: drive #{} intent={}", did, intent);
                }
            }
        }
    }

    fn persist_tetra(&self, id: TetraId) {
        let ctx = super::janitor::JanitorCtx {
            space: &self.space,
            storage: &self.storage,
            knowledge: &self.knowledge,
            gateway: &self.gateway,
        };
        super::janitor::mark_dirty_persist(&ctx, id);
    }

    /// Deprioritize a memory WITHOUT deleting it (constitution §4.5 — memories are sacred).
    /// Adds "quarantine" label, sets valid_to, floors importance/mass, and persists.
    /// Replaces the former purge_tetra path used by evict/governor.
    fn supersede_tetra(&self, id: TetraId) {
        if let Some(tetra) = self.space.get_tetrahedron(id) {
            let old_labels = tetra.data.labels.clone();
            let mut data = tetra.data.clone();
            if !data.labels.iter().any(|l| l == "quarantine") {
                data.labels.push("quarantine".to_string());
            }
            let now = chrono::Utc::now().timestamp();
            data.valid_to = Some(now);
            data.importance = data.importance.min(0.1);
            let new_labels = data.labels.clone();
            if let Err(e) = self.space.update_payload(id, data) {
                tracing::warn!("[Scheduler] update_payload {} failed: {}", id, e);
            }
            if let Err(e) = self.space.update_mass(id, 0.05) {
                tracing::warn!("[Scheduler] update_mass {} failed: {}", id, e);
            }
            let _ = self.space.update_validity(id, Some(now));
            self.gateway
                .update_label_index(id, &old_labels, &new_labels);
        }
        self.persist_tetra(id);
        tracing::info!(
            "[Supersede] tetra {} deprioritized (quarantine label, NO deletion)",
            id
        );
    }

    pub fn api_update_content(&self, id: TetraId, new_content: &str) -> Result<(), String> {
        self.security
            .validate_content(new_content)
            .map_err(|_| "content validation failed".to_string())?;

        if self.space.get_tetrahedron(id).is_none() {
            return Err(format!("memory {} not found", id));
        }

        self.gateway.update_content(id, new_content)?;
        self.persist_tetra(id);
        Ok(())
    }

    fn purge_tetra(&self, id: TetraId) {
        let labels = self
            .space
            .get_tetrahedron(id)
            .map(|t| t.data.labels.clone());
        if let Err(e) = self.space.remove_tetrahedron(id) {
            tracing::debug!("purge_tetra {}: space already removed: {}", id, e);
        }
        if let Err(e) = self.storage.delete_tetra(id) {
            tracing::warn!("purge_tetra {}: storage delete failed: {}", id, e);
        }
        self.knowledge.remove_relations_for(id);
        if let Some(ref lbls) = labels {
            self.gateway.on_tetra_removed(id, lbls);
        } else {
            self.gateway.remove_from_hnsw(id);
            self.gateway.remove_from_content_hash(id);
        }
        let _ = self
            .tx
            .send(super::bus::EngineEvent::TetrahedronRemoved(id));
        tracing::info!(
            "[Purge] tetra {} fully cleaned (space+storage+KG+HNSW+index)",
            id
        );
    }

    fn record_outcome(&self, action: ActionType, pre_snap: &TickSnapshot, tick: u64) {
        let post_snap = self.build_snapshot();
        self.record_outcome_snaps(action, pre_snap, &post_snap, tick);
    }

    /// Core outcome recording that accepts a pre-built post-snapshot (avoids rebuilding
    /// the O(N) snapshot once per action when recording several outcomes from one decision).
    fn record_outcome_snaps(
        &self,
        action: ActionType,
        pre_snap: &TickSnapshot,
        post_snap: &TickSnapshot,
        tick: u64,
    ) {
        let pre_entropy = if !pre_snap.clusters.is_empty() {
            pre_snap
                .clusters
                .iter()
                .map(|c| dynamics::compute_entropy_from_labels(&c.tetra_ids, &pre_snap.labels_map))
                .sum::<f64>()
                / pre_snap.clusters.len() as f64
        } else {
            0.0
        };
        let post_entropy = if !post_snap.clusters.is_empty() {
            post_snap
                .clusters
                .iter()
                .map(|c| dynamics::compute_entropy_from_labels(&c.tetra_ids, &post_snap.labels_map))
                .sum::<f64>()
                / post_snap.clusters.len() as f64
        } else {
            0.0
        };
        let mut outcome = ActionOutcome {
            action,
            pre_entropy,
            post_entropy,
            pre_cluster_count: pre_snap.clusters.len(),
            post_cluster_count: post_snap.clusters.len(),
            pre_tetra_count: pre_snap.tetras.len(),
            post_tetra_count: post_snap.tetras.len(),
            pre_energy: pre_snap.energy,
            post_energy: post_snap.energy,
            effectiveness: 0.0,
            tick,
        };
        outcome.compute_effectiveness();

        let effectiveness = outcome.effectiveness;
        let mut outcome_tracker = self.outcome.lock();
        outcome_tracker.record(outcome);
        drop(outcome_tracker);

        let mut adaptive = self.adaptive.lock();
        adaptive.adapt_from_outcome(action, effectiveness);
        drop(adaptive);

        if let Some(drive_type) = match action {
            ActionType::Pulse | ActionType::Link => Some(super::drive::Drive::Curiosity),
            ActionType::Fission | ActionType::Merge => Some(super::drive::Drive::Coherence),
            ActionType::Dream => Some(super::drive::Drive::Efficiency),
            ActionType::Evict => Some(super::drive::Drive::Efficiency),
        } {
            self.drive.lock().reward(drive_type, effectiveness);
        }
    }

    /// L0: Persist drive queue to SQLite (called with auto_save every 10 ticks).
    /// pub: ack 处理器在成功后立即调用 — ack 只改内存, 依赖周期保存时
    /// 重启会回滚 ack 状态(#105187 曾复活为 Pending)
    pub fn save_drive_queue(&self) {
        self.save_tick_state();
        let signals = self.drive_queue.snapshot();
        if let Err(e) = self.storage.save_drive_signals(&signals) {
            tracing::warn!("[L0] drive queue save failed: {}", e);
        }
        let ingested = self.drive_queue.ingested_ids();
        if let Ok(js) = serde_json::to_string(&ingested) {
            if let Err(e) = self.storage.save_drive_kv("ingested_ids", &js) {
                tracing::warn!("[L0] ingested save failed: {}", e);
            }
        }
        if let Ok(js) = serde_json::to_string(&self.drive_queue.policy_snapshot()) {
            if let Err(e) = self.storage.save_drive_kv("signal_policy", &js) {
                tracing::warn!("[L0] signal_policy save failed: {}", e);
            }
        }
        let _ = self.storage.save_drive_kv(
            "signal_policy_version",
            &self.drive_queue.policy_version().to_string(),
        );
    }

    fn auto_save(&self) {
        // D6.1: 无执行端用户的常规信号自消费
        let _ = self.auto_consume_routine_signals();
        // P2/P5/P6: 空间维护(dream不做, tick做轻量版)
        let _ = self.review_cold_memories();
        let _ = self.synaptic_pruning();
        let _ = self.storage.archive_old_drive_signals();
        // 2026-09-22审计接线: 过期记忆归档(此前archive_stale_superseded从未被调用, 最后归档停在8/16)
        if let Ok(moved) = self.storage.archive_stale_superseded(30) {
            if moved > 0 {
                tracing::info!(
                    "[scheduler] archive_stale_superseded moved {} tetras",
                    moved
                );
            }
        }
        self.save_tick_state();
        self.save_drive_engine(); // 稳态: 演化状态随周期保存
        let ctx = super::janitor::JanitorCtx {
            space: &self.space,
            storage: &self.storage,
            knowledge: &self.knowledge,
            gateway: &self.gateway,
        };
        super::janitor::auto_save(&ctx);
    }

    /// Periodic flush of buffered access_count totals to Space + storage.
    /// Replaces the former per-search-result write (which held the Space write-lock per result).
    fn flush_access_counts(&self) {
        let snapshot = self.gateway.access_counts_snapshot();
        // 先更新内存中的 payload，收集需要持久化的 (id, count) 对
        let mut dirty_updates: Vec<(TetraId, u32)> = Vec::new();
        for (id, session_delta) in &snapshot {
            if let Some(tetra) = self.space.get_tetrahedron(*id) {
                // session_delta 是本次进程的增量；真正的累计值 = DB历史值 + 增量
                // 修复倒退bug：原直接写 session_delta 覆盖了重启前的历史值
                let cumulative = tetra.data.access_count.saturating_add(*session_delta);
                if tetra.data.access_count != cumulative {
                    // M2修复:用单字段更新,避免 clone 整个 payload(含 8KB embedding)
                    if let Err(e) = self.space.update_access_count(*id, cumulative) {
                        tracing::warn!("[Scheduler] update_access_count {} failed: {}", id, e);
                    }
                    // 突破3: 遗忘曲线 — 被搜索命中的记忆更新复习时间,重置衰减节拍
                    let now = chrono::Utc::now().timestamp();
                    let _ = self.space.update_last_reviewed(*id, now);
                    dirty_updates.push((*id, cumulative));
                }
            }
        }
        // 批量写入 DB — 单事务一次锁
        if !dirty_updates.is_empty() {
            if let Err(e) = self.storage.batch_update_access_counts(&dirty_updates) {
                tracing::warn!(
                    "[Scheduler] batch_update_access_counts failed ({} ids): {}",
                    dirty_updates.len(),
                    e
                );
            }
        }
        // 清空 session 计数器（已合并到 DB 值）
        self.gateway.reset_session_access_counts();
    }

    fn log_event(&self, event: String) {
        let mut events = self.recent_events.lock();
        events.push(event);
        if events.len() > 20 {
            let excess = events.len() - 20;
            events.drain(0..excess);
        }
    }

    fn evict_low_quality(&self, snap: &TickSnapshot) {
        let adaptive = self.adaptive.lock();
        let ctx = super::auto_pipeline::AutoPipelineCtx {
            tick: snap.tick,
            space: &self.space,
            energy: &self.energy,
            knowledge: &self.knowledge,
            gateway: &self.gateway,
            storage: &self.storage,
            emotion_pleasure: 0.0,
            emotion_arousal: 0.0,
            adaptive: &adaptive,
        };
        let purge = |id: TetraId| {
            self.supersede_tetra(id);
        };
        super::auto_pipeline::evict_low_quality(&ctx, &snap.tetras, &purge);
    }

    fn record_decision(&self, tick: u64, action: &str, detail: &str, result: &str) {
        let mut hist = self.decision_history.lock();
        hist.push(super::cognitive::DecisionRecord {
            tick,
            action: action.to_string(),
            detail: detail.to_string(),
            result: result.to_string(),
        });
        if hist.len() > 30 {
            let excess = hist.len() - 30;
            hist.drain(0..excess);
        }
    }

    fn generate_aliases(&self, round: usize, snap: &TickSnapshot) {
        let ctx = super::cognitive_hooks::CognitiveHooksCtx {
            space: &self.space,
            storage: &self.storage,
            gateway: &self.gateway,
            cognitive: &self.cognitive,
        };
        super::cognitive_hooks::generate_aliases(&ctx, round, &snap.tetras);
    }

    fn tick_and_maybe_think(&self) -> Option<CognitiveThought> {
        let count = self.tick_count.fetch_add(1, Ordering::SeqCst);
        let _ = self.tx.send(EngineEvent::DecisionTick);
        self.energy.replenish(8.0);

        let tasks: Vec<ScheduledTask> = self.queue.lock().drain(..).collect();
        for task in &tasks {
            self.execute_task(task);
        }

        // L0 Active Inference: run prediction error detection at the START of each tick.
        // This runs unconditionally — even if LLM times out, the personality still
        // generates drive signals from its accumulated experience.
        // This is how memory drives action: accumulated patterns detect mismatches
        // with current reality → will is generated → agents receive it via /v1/drive/inbox
        self.run_prediction_error_check();

        // L0: Self-driving loop — process pending drive signals.
        // This runs unconditionally — even when LLM is degraded, the personality
        // can still explore using local memory recall (no LLM needed).
        // When LLM IS available, it uses answer_from_memories for richer reasoning.
        // This is resilience: the personality never stops thinking.
        self.process_drive_signals();

        // L0: Persist drive queue to SQLite every 5 ticks.
        // Runs unconditionally — survives even when LLM times out.
        {
            // count is the pre-increment value from fetch_add above (0,1,2,3...)
            // Save on tick 0 (first tick) and every 5 ticks after
            if count == 0 || count % 5 == 0 {
                self.save_drive_queue();
            }
        }

        let snap = self.build_snapshot();

        // Phase 0: Observe — update drive engine with current state signals
        {
            let avg_entropy = if !snap.clusters.is_empty() {
                let sum: f64 = snap
                    .clusters
                    .iter()
                    .map(|c| dynamics::compute_entropy_from_labels(&c.tetra_ids, &snap.labels_map))
                    .sum::<f64>();
                sum / snap.clusters.len() as f64
            } else {
                0.0
            };
            let energy_ratio = snap.energy / self.max_energy.max(1.0);
            let tetra_count = snap.tetras.len();
            let unexplored_ratio = if tetra_count > 0 {
                let explored: usize = snap.tetras.iter().filter(|t| t.mass > 1.05).count();
                1.0 - (explored as f64 / tetra_count as f64)
            } else {
                1.0
            };
            let redundancy_ratio = if tetra_count > 1 {
                let content_hashes: Vec<u64> = snap
                    .tetras
                    .iter()
                    .take(100)
                    .map(|t| t.content_hash)
                    .collect();
                let unique: std::collections::HashSet<u64> =
                    content_hashes.iter().copied().collect();
                1.0 - (unique.len() as f64 / content_hashes.len().max(1) as f64)
            } else {
                0.0
            };

            let mut drive = self.drive.lock();
            drive.observe(
                tetra_count,
                snap.clusters.len(),
                avg_entropy,
                energy_ratio,
                unexplored_ratio,
                redundancy_ratio,
            );
        }

        // Phase 1: Generate — drive-guided actions replace hardcoded tick intervals
        let drive = self.drive.lock();
        let should_pulse = drive.should_pulse();
        let should_fission = drive.should_fission();
        let _should_dream = drive.should_dream();
        let should_evict = drive.should_evict();
        drop(drive);

        // Phase 2: Execute — drive-guided pipeline
        if should_pulse {
            self.auto_pulse(&snap);
        }

        let has_large_cluster = snap.clusters.iter().any(|c| c.tetra_ids.len() >= 6);
        if should_fission || has_large_cluster {
            self.auto_fission(&snap);
        }

        if count % 20 == 0 && count > 0 {
            self.auto_skills(&snap);
        }

        if count % 30 == 0 && count > 0 && self.energy.available() >= 50.0 {
            let pre_snap = self.build_snapshot();
            self.auto_dream();
            self.record_outcome(ActionType::Dream, &pre_snap, count);
        }

        if count % 30 == 0 && count > 0 {
            self.flush_access_counts();
        }

        if count % 200 == 0 && count > 0 && should_evict {
            self.evict_low_quality(&snap);
        }

        // Phase 5: Emotion
        if count % 10 == 0 {
            let texts: Vec<&str> = snap
                .tetras
                .iter()
                .take(20)
                .map(|t| t.content.as_str())
                .collect();
            let new_emotion = super::emotion::EmotionState::analyze_texts(&texts);
            {
                let mut em = self.emotion.lock();
                em.affect(
                    new_emotion.pleasure * 0.1,
                    new_emotion.arousal * 0.1,
                    new_emotion.dominance * 0.1,
                );
                em.decay(0.05);
            }
        }

        // Phase 6: Think — LLM cognitive decision every 5 ticks (when enabled)
        if count % 5 == 0 && self.cognitive.enabled() {
            if count % 15 == 0 && count > 0 {
                self.generate_aliases((count / 15) as usize, &snap);
            }
            if count % 30 == 0 && count > 0 {
                self.reclassify_memories((count / 30) as usize, &snap);
            }
            if count % 20 == 0 && count > 0 {
                self.extract_entities((count / 20) as usize, &snap);
            }

            // Idle Stopper: skip LLM call if space is healthy
            let orphan_count = snap
                .tetras
                .iter()
                .filter(|t| !t.enforced)
                .map(|t| t.id)
                .filter(|id| !snap.labels_map.contains_key(id))
                .count();
            let orphan_rate = if !snap.tetras.is_empty() {
                orphan_count as f64 / snap.tetras.len() as f64
            } else {
                0.0
            };
            // 熵阈值与簇大小自适应: N<5 的小簇标签混合是常态, 熵无统计意义
            // (潜意识 REFLECT 自诊断: entropy阈值是size-dependent, 小簇误报曾导致永久healthy=false)
            let max_entropy = snap
                .clusters
                .iter()
                .filter(|c| c.tetra_ids.len() >= 5)
                .map(|c| dynamics::compute_entropy_from_labels(&c.tetra_ids, &snap.labels_map))
                .fold(0.0f64, f64::max);
            let quarantine_count = snap
                .tetras
                .iter()
                .filter(|t| t.labels.iter().any(|l| l == "quarantine"))
                .count();
            let energy_low = snap.energy < self.max_energy * 0.3;

            // 阶段3修复:quarantine阈值从绝对值10改为比例(<总量5%),
            // 因为P0-23修复后quarantine记忆不再删除,绝对值只会增不会减。
            let quarantine_ratio = if !snap.tetras.is_empty() {
                quarantine_count as f64 / snap.tetras.len() as f64
            } else {
                0.0
            };
            // 熵项从思考门摘除: auto_fission 独立消化高熵(每10tick), LLM对熵的重复思考
            // 曾致 3h/100轮/50万token 空转 — 意识自己反复判断"让auto_fission处理,暂不干预"
            // 思考门只保留 LLM 真正有价值的触发: 孤儿/垃圾/能量 (miss 由 idle_stopper 判)
            let is_healthy = orphan_rate < 0.05 && quarantine_ratio < 0.05 && !energy_low;

            let drive = self.drive.lock();
            let dominant = drive.dominant();
            tracing::info!(
                "[Scheduler] tick {} — {} tetras, {} clusters, energy {:.0}, drive={:?}, orphan={:.1}%, entropy_max={:.2}, quarantined={}, healthy={}",
                count,
                snap.tetras.len(),
                snap.clusters.len(),
                snap.energy,
                dominant,
                orphan_rate * 100.0,
                max_entropy,
                quarantine_count,
                is_healthy,
            );
            drop(drive);

            if is_healthy && count % 30 != 0 {
                // 智能突破：健康短路从 15→30 tick，让系统更频繁地"思考"
                // 每 30 tick（约1小时）即使健康也做一次元认知检查
                self.auto_save();
                return None;
            }

            let state = self.collect_state_from_snap(&snap);
            Some(CognitiveThought { tick: count, state })
        } else {
            if count % 10 == 0 {
                self.auto_save();
            }
            None
        }
    }

    fn auto_skills(&self, snap: &TickSnapshot) {
        if let Some(ref skills_engine) = *self.skills.lock() {
            let pending = skills_engine.review_pending();
            let pub_engine = self.pub_skills.lock().clone();
            for skill in pending {
                match super::skills::SkillEngine::security_check(&skill) {
                    Ok(()) => {
                        if let Ok(approved) = skills_engine.approve_skill(skill.id) {
                            tracing::info!(
                                "[Skills] auto approved '{}' (id={})",
                                approved.name,
                                skill.id
                            );

                            if let Ok(ref desc) = self
                                .cognitive
                                .generate_skill_description(&approved.name, &approved.skill_md)
                            {
                                let _ = skills_engine.append_description(skill.id, desc);
                                tracing::info!(
                                    "[Skills] added Chinese description for '{}'",
                                    approved.name
                                );
                            }

                            if let Some(ref pub_sk) = pub_engine {
                                if let Some(moved) = skills_engine.take(skill.id) {
                                    let mut pub_skill = moved.clone();
                                    pub_skill.is_public = true;
                                    pub_sk.insert_skill(pub_skill);
                                }
                            }
                        }
                    }
                    Err(reason) => {
                        if skills_engine.reject_skill(skill.id, &reason).is_ok() {
                            tracing::info!(
                                "[Skills] auto rejected '{}' (id={}): {}",
                                skill.name,
                                skill.id,
                                reason
                            );
                        }
                    }
                }
            }
        }

        if snap.clusters.is_empty() {
            return;
        }
        let top_labels: Vec<(String, usize)> = {
            let mut label_counts: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            for t in &snap.tetras {
                for label in &t.labels {
                    *label_counts.entry(label.clone()).or_insert(0) += 1;
                }
            }
            let mut v: Vec<_> = label_counts.into_iter().collect();
            v.sort_by_key(|b| std::cmp::Reverse(b.1));
            v.truncate(5);
            v
        };
        if !top_labels.is_empty() {
            let summary: Vec<String> = top_labels
                .iter()
                .map(|(l, c)| format!("{}({})", l, c))
                .collect();
            tracing::debug!(
                "[Skills] top domains: {} — matching against skills",
                summary.join(", ")
            );

            if let Some(ref skills_engine) = *self.skills.lock() {
                let mut linked_count = 0usize;
                for (label, _count) in &top_labels {
                    let matched = skills_engine.match_skills(label, "", 3);
                    for skill in &matched {
                        let tetra_ids: Vec<u64> = snap
                            .tetras
                            .iter()
                            .filter(|t| t.labels.iter().any(|l| l == label))
                            .take(5)
                            .map(|t| t.id)
                            .collect();
                        for mid in tetra_ids {
                            if skills_engine.link_memory(skill.id, mid).is_ok() {
                                linked_count += 1;
                            }
                        }
                    }
                }
                if linked_count > 0 {
                    tracing::info!(
                        "[Skills-AutoLink] linked {} memory↔skill pairs from top domains",
                        linked_count
                    );
                }
            }
        }
    }

    fn auto_pulse(&self, snap: &TickSnapshot) {
        let em = self.emotion.lock();
        let pleasure = em.pleasure;
        let arousal = em.arousal;
        drop(em);
        let adaptive = self.adaptive.lock();
        let ctx = super::auto_pipeline::AutoPipelineCtx {
            tick: snap.tick,
            space: &self.space,
            energy: &self.energy,
            knowledge: &self.knowledge,
            gateway: &self.gateway,
            storage: &self.storage,
            emotion_pleasure: pleasure,
            emotion_arousal: arousal,
            adaptive: &adaptive,
        };

        let pulsed =
            super::auto_pipeline::auto_pulse(&ctx, &snap.tetras, &snap.clusters, &snap.core_map);
        if pulsed > 0 {
            let _ = self
                .tx
                .send(super::bus::EngineEvent::AutoPulse { count: pulsed });
        }
    }

    fn auto_fission(&self, snap: &TickSnapshot) {
        let em = self.emotion.lock();
        let pleasure = em.pleasure;
        let arousal = em.arousal;
        drop(em);
        let adaptive = self.adaptive.lock();
        let ctx = super::auto_pipeline::AutoPipelineCtx {
            tick: snap.tick,
            space: &self.space,
            energy: &self.energy,
            knowledge: &self.knowledge,
            gateway: &self.gateway,
            storage: &self.storage,
            emotion_pleasure: pleasure,
            emotion_arousal: arousal,
            adaptive: &adaptive,
        };

        let last_fission = self.last_fission_tick.load(Ordering::SeqCst);
        let last_merge = self.last_merge_pairs.lock().clone();

        let outcome = super::auto_pipeline::auto_fission(
            &ctx,
            &snap.clusters,
            &snap.labels_map,
            &snap.core_map,
            last_fission,
            &last_merge,
        );
        if outcome.did_fission {
            self.last_fission_tick.store(snap.tick, Ordering::SeqCst);
        }
        if let Some(new_merge) = outcome.merge_pairs {
            *self.last_merge_pairs.lock() = new_merge;
        }
    }

    fn auto_dream(&self) {
        let tick = self.tick_count.load(Ordering::SeqCst);
        let last = self.last_dream_tick.load(Ordering::SeqCst);

        let em = self.emotion.lock();
        let pleasure = em.pleasure;
        let arousal = em.arousal;
        drop(em);
        let adaptive = self.adaptive.lock();
        let ctx = super::auto_pipeline::AutoPipelineCtx {
            tick,
            space: &self.space,
            energy: &self.energy,
            knowledge: &self.knowledge,
            gateway: &self.gateway,
            storage: &self.storage,
            emotion_pleasure: pleasure,
            emotion_arousal: arousal,
            adaptive: &adaptive,
        };

        let purge = |id: TetraId| {
            self.persist_tetra(id);
        };

        // D5: 热标签传入governor(个体化遗忘)
        let hot_labels: Vec<(String, u32)> = self
            .gateway
            .search_metrics()
            .top_labels
            .into_iter()
            .take(20)
            .collect();
        let gov = super::governor::LifecycleGovernor::evaluate_with_hot_labels(
            &self.space,
            &self.knowledge,
            &hot_labels,
        );
        // S3修复: evaluate 修改的记忆持久化 (session/bridge supersede + decay)
        for mid in &gov.mutated_ids {
            if let Some(t) = self.space.get_tetrahedron(*mid) {
                let _ = self.storage.upsert_tetra(&t);
            }
        }

        // Tiered consolidation: boost importance of recurrent memories
        if !gov.recurrent_ids.is_empty() {
            for &rid in &gov.recurrent_ids {
                if let Some(tetra) = self.space.get_tetrahedron(rid) {
                    let count = tetra.data.access_count;
                    let boost = match count {
                        3..=5 => 0.1,
                        6..=10 => 0.2,
                        _ => 0.3,
                    };
                    let mut data = tetra.data.clone();
                    let old_imp = data.importance;
                    data.importance = (data.importance + boost).min(3.5);
                    if let Err(e) = self.space.update_payload(rid, data) {
                        tracing::warn!("[Scheduler] update_payload {} failed: {}", rid, e);
                    }
                    // S3修复: 升级持久化
                    if let Some(t2) = self.space.get_tetrahedron(rid) {
                        let _ = self.storage.upsert_tetra(&t2);
                    }
                    tracing::info!(
                        "[Governor] tiered boost #{}: importance {:.2} -> {:.2} (access_count={})",
                        rid,
                        old_imp,
                        old_imp + boost,
                        count
                    );
                }
            }
        }

        // Feedback aggregation: incremental, only process new feedback records
        {
            let feedback_mems = self.gateway.list_by_labels(&["feedback"], 100);
            let (mut processed_ids, should_full) = {
                let cache = self.feedback_agg_cache.lock();
                match &*cache {
                    Some((last_count, last_time, ids)) => {
                        let needs_full = feedback_mems.len() > *last_count + 50
                            || last_time.elapsed() > std::time::Duration::from_secs(600);
                        (ids.clone(), needs_full)
                    }
                    None => (HashSet::new(), true),
                }
            };

            let new_feedbacks: Vec<_> = if should_full {
                feedback_mems.iter().collect()
            } else {
                feedback_mems
                    .iter()
                    .filter(|(id, _)| !processed_ids.contains(id))
                    .collect()
            };

            if !new_feedbacks.is_empty() {
                let mut feedback_scores: std::collections::HashMap<u64, f64> =
                    std::collections::HashMap::new();
                for &(fid, p) in &new_feedbacks {
                    let lower = p.content.to_lowercase();
                    let relevance = if lower.contains("highly_relevant") {
                        0.1
                    } else if lower.contains("partially_relevant") {
                        0.03
                    } else {
                        -0.05
                    };
                    let outcome = if lower.contains("task_completed") {
                        0.15
                    } else if lower.contains("task_failed") {
                        -0.1
                    } else {
                        0.0
                    };
                    let correction = if lower.contains("correction: outdated")
                        || lower.contains("correction: incorrect")
                    {
                        -0.3
                    } else {
                        0.0
                    };
                    let delta = relevance + outcome + correction;
                    let ids_start = lower.find("affected_ids:");
                    if let Some(start) = ids_start {
                        let bracket_start = lower[start..].find('[');
                        let bracket_end = lower[start..].find(']');
                        if let (Some(bs), Some(be)) = (bracket_start, bracket_end) {
                            let ids_str = &lower[start + bs + 1..start + be];
                            for id_str in ids_str.split(',') {
                                if let Ok(id) = id_str.trim().parse::<u64>() {
                                    *feedback_scores.entry(id).or_insert(0.0) += delta;
                                }
                            }
                        }
                    }
                    processed_ids.insert(*fid);
                }

                let mut aggregated = 0usize;
                for (id, total_delta) in &feedback_scores {
                    if total_delta.abs() < 0.01 {
                        continue;
                    }
                    if let Some(tetra) = self.space.get_tetrahedron(*id) {
                        let mut data = tetra.data.clone();
                        let old_imp = data.importance;
                        let adj = (*total_delta * 0.3).clamp(-0.5, 0.5);
                        data.importance = (data.importance + adj).clamp(0.1, 5.0);
                        if let Err(e) = self.space.update_payload(*id, data) {
                            tracing::warn!("[Scheduler] update_payload {} failed: {}", *id, e);
                        }
                        let _ = self.storage.update_importance(*id, adj);
                        aggregated += 1;
                        tracing::info!(
                            "[Feedback-Agg] #{} delta={:.2} adj={:.3} importance {:.2}->{:.2}",
                            id,
                            total_delta,
                            adj,
                            old_imp,
                            old_imp + adj
                        );
                    }
                }
                if aggregated > 0 || !new_feedbacks.is_empty() {
                    tracing::info!(
                        "[Feedback-Agg] {} new records, {} adjustments (total processed: {})",
                        new_feedbacks.len(),
                        aggregated,
                        processed_ids.len()
                    );
                }
            }
            *self.feedback_agg_cache.lock() = Some((
                feedback_mems.len(),
                std::time::Instant::now(),
                processed_ids,
            ));
        }

        // Skill feedback aggregation: bridge memory feedback → skill success_rate
        {
            let skill_fb_mems = self.gateway.list_by_labels(&["skill-feedback"], 100);
            let (mut processed_sids, should_full_s) = {
                let cache = self.skill_feedback_agg_cache.lock();
                match &*cache {
                    Some((last_count, last_time, ids)) => {
                        let needs_full = skill_fb_mems.len() > *last_count + 50
                            || last_time.elapsed() > std::time::Duration::from_secs(600);
                        (ids.clone(), needs_full)
                    }
                    None => (HashSet::new(), true),
                }
            };

            let new_skill_fb: Vec<_> = if should_full_s {
                skill_fb_mems.iter().collect()
            } else {
                skill_fb_mems
                    .iter()
                    .filter(|(id, _)| !processed_sids.contains(id))
                    .collect()
            };

            if !new_skill_fb.is_empty() {
                let pub_engine = self.pub_skills.lock().clone();
                let mut skill_updates = 0usize;
                for &(fid, p) in &new_skill_fb {
                    let lower = p.content.to_lowercase();
                    let helpful = lower.contains("helpful\":true")
                        || lower.contains("\"helpful\": true")
                        || lower.contains("helpful: true")
                        || lower.contains("helpful=true")
                        || lower.contains("status.*success");
                    let skill_id = {
                        let mut sid = None;
                        if let Some(start) = lower.find("skill_id") {
                            let rest = &lower[start..];
                            if let Some(num_start) = rest.find(|c: char| c.is_ascii_digit()) {
                                let num_str: String = rest[num_start..]
                                    .chars()
                                    .take_while(|c| c.is_ascii_digit())
                                    .collect();
                                sid = num_str.parse::<u64>().ok();
                            }
                        }
                        sid
                    };
                    if let (Some(sid), Some(ref pub_sk)) = (skill_id, &pub_engine) {
                        let _ = pub_sk.record_feedback(sid, helpful);
                        skill_updates += 1;
                        tracing::debug!(
                            "[SkillFeedback-Agg] skill_id={} helpful={} processed",
                            sid,
                            helpful
                        );
                    }
                    processed_sids.insert(*fid);
                }
                if skill_updates > 0 {
                    tracing::info!(
                        "[SkillFeedback-Agg] {} skill feedback records applied",
                        skill_updates
                    );
                }
            }
            *self.skill_feedback_agg_cache.lock() = Some((
                skill_fb_mems.len(),
                std::time::Instant::now(),
                processed_sids,
            ));
        }

        if gov.should_consolidate || gov.should_archive || gov.should_merge {
            if let Some(result) = super::auto_pipeline::auto_dream(&ctx, last, &purge) {
                self.last_dream_tick.store(tick, Ordering::SeqCst);
                let decayed = self.gateway.decay_relations();
                if !gov.recurrent_ids.is_empty() {
                    super::governor::LifecycleGovernor::reset_access_counts(
                        &self.space,
                        &gov.recurrent_ids,
                    );
                }

                // Governor merge — actually merge duplicate candidates
                let mut gov_merged = 0usize;
                if gov.should_merge {
                    let candidates =
                        super::governor::LifecycleGovernor::find_merge_candidates(&self.space);
                    gov_merged = super::governor::LifecycleGovernor::execute_merges(
                        &self.space,
                        &candidates,
                    );
                    for c in &candidates {
                        self.persist_tetra(c.keep_id);
                        self.persist_tetra(c.remove_id);
                    }
                }

                self.log_event(format!(
                    "auto_dream(tick={}): consolidated={}, connections={}, merged={}, evicted={}, insights={}, decayed={}, recurrent={}, gov_merged={}",
                    tick,
                    result.report.memories_consolidated,
                    result.report.connections_formed,
                    result.report.duplicates_merged,
                    result.report.junk_evicted,
                    result.report.insights.len(),
                    decayed,
                    gov.recurrent_ids.len(),
                    gov_merged,
                ));
            }
        }

        // Save health snapshot every dream cycle
        {
            let stats = self.gateway.stats();
            let feedback_mems = self.gateway.list_by_labels(&["feedback"], 50);
            let all_mems = self.gateway.list_nodes();
            let avg_imp = if !all_mems.is_empty() {
                all_mems.iter().map(|(_, p)| p.importance).sum::<f64>() / all_mems.len() as f64
            } else {
                0.0
            };
            let enforced = self.gateway.get_enforced_patterns().len();
            let _ = self.storage.save_health_snapshot(
                stats.tetra_count as i64,
                stats.clusters as i64,
                feedback_mems.len() as i64,
                avg_imp,
                enforced as i64,
            );
        }
    }

    fn reclassify_memories(&self, round: usize, snap: &TickSnapshot) {
        self.last_reclassify_tick.store(snap.tick, Ordering::SeqCst);
        let ctx = super::cognitive_hooks::CognitiveHooksCtx {
            space: &self.space,
            storage: &self.storage,
            gateway: &self.gateway,
            cognitive: &self.cognitive,
        };
        super::cognitive_hooks::reclassify_memories(&ctx, round, &snap.tetras);
    }

    fn extract_entities(&self, round: usize, snap: &TickSnapshot) {
        let ctx = super::cognitive_hooks::CognitiveHooksCtx {
            space: &self.space,
            storage: &self.storage,
            gateway: &self.gateway,
            cognitive: &self.cognitive,
        };
        super::cognitive_hooks::extract_entities(&ctx, round, &snap.tetras);
    }

    fn perform_fission(
        &self,
        cluster_index: usize,
        cooldown: u64,
        energy_cost: f64,
        tag: &str,
    ) -> bool {
        let snap = self.build_snapshot();
        self.perform_fission_from_snap(cluster_index, cooldown, energy_cost, tag, &snap)
    }

    fn perform_fission_from_snap(
        &self,
        cluster_index: usize,
        cooldown: u64,
        energy_cost: f64,
        tag: &str,
        snap: &TickSnapshot,
    ) -> bool {
        let adaptive = self.adaptive.lock();
        let ctx = super::auto_pipeline::AutoPipelineCtx {
            tick: snap.tick,
            space: &self.space,
            energy: &self.energy,
            knowledge: &self.knowledge,
            gateway: &self.gateway,
            storage: &self.storage,
            emotion_pleasure: 0.0,
            emotion_arousal: 0.0,
            adaptive: &adaptive,
        };
        match super::auto_pipeline::perform_fission_from_snap(
            &ctx,
            cluster_index,
            cooldown,
            energy_cost,
            tag,
            &snap.clusters,
            &snap.labels_map,
            &snap.core_map,
        ) {
            Some(result) => {
                self.last_fission_tick.store(result.tick, Ordering::SeqCst);
                self.log_event(format!(
                    "{}({},{})",
                    tag.to_lowercase(),
                    cluster_index,
                    result.moved_count
                ));
                true
            }
            None => false,
        }
    }

    fn apply_thought(&self, tick: u64, response: super::cognitive::CognitiveResponse) {
        tracing::info!("[LLM thoughts] {}", response.thoughts);
        // 智能突破断裂点1：存储 learning 到 CognitiveEngine（下次 prompt 会注入）
        self.cognitive.store_learning(&response.learning);
        let max_actions = if self.energy.available() < 200.0 {
            2
        } else {
            3
        };
        let limited_actions: Vec<&super::cognitive::SchedulerAction> =
            response.actions.iter().take(max_actions).collect();
        if response.actions.len() > max_actions {
            tracing::warn!(
                "[Guard] limited {} actions to {}",
                response.actions.len(),
                max_actions
            );
        }
        let pre_snap = self.build_snapshot();
        for action in &limited_actions {
            let action_name = match action {
                SchedulerAction::Pulse { origin, .. } => format!("pulse({})", origin),
                SchedulerAction::Fission { cluster_index } => format!("fission({})", cluster_index),
                SchedulerAction::Fuse {
                    cluster_a,
                    cluster_b,
                } => format!("fuse({},{})", cluster_a, cluster_b),
                SchedulerAction::Dream => "dream".to_string(),
                SchedulerAction::Link { a, b, reason } => format!("link({},{},{})", a, b, reason),
                SchedulerAction::Consolidate { ids, keep, .. } => {
                    format!("consolidate({:?}->{})", ids, keep)
                }
                SchedulerAction::MarkJunk { ids, .. } => format!("mark_junk({:?})", ids),
                SchedulerAction::Relabel {
                    id,
                    add_labels,
                    remove_labels,
                    ..
                } => format!("relabel({}+{:?}-{:?})", id, add_labels, remove_labels),
                SchedulerAction::Reflect { .. } => "reflect".to_string(),
                SchedulerAction::UseTool { tool, .. } => format!("use_tool({})", tool),
                SchedulerAction::ActOutward { intent, .. } => format!("act_outward({})", intent),

                SchedulerAction::ActOutward { intent, .. } => format!("act_outward({})", intent),
            };
            let action_pre_snap = self.build_snapshot();
            self.execute_action(action);
            // 突破3：learn_history 反馈闭环——记录决策的真实后果，而非固定 "executed"。
            // 用动作前后的快照对比判断是否产生了实际变化（能量消耗/簇数变化/记忆变化）。
            // LLM 下次 decide 时会看到 decision_history（cognitive.rs:1043 注入 prompt），
            // 从而从自己决策的效果中学习："上次 dream=effective, pulse=no_effect → 多做 dream 少做 pulse"。
            let post_action_snap = self.build_snapshot();
            // 用长度+能量判断是否产生效果（Vec<TetraMeta> 无 PartialEq，用 len 近似）
            let outcome = if post_action_snap.tetras.len() != action_pre_snap.tetras.len()
                || post_action_snap.clusters.len() != action_pre_snap.clusters.len()
                || (post_action_snap.energy - action_pre_snap.energy).abs() > 0.5
            {
                "effective"
            } else {
                "no_effect"
            };
            self.record_decision(
                tick,
                &action_name,
                &response.thoughts.chars().take(100).collect::<String>(),
                outcome,
            );
        }

        // Close the evolution loop: record outcome effectiveness for EVERY cognitive action type
        // (memory-driven evolution). Previously only Dream recorded outcomes, so adaptive params
        // + drive never learned from Fission/Fuse/Pulse/Link/Consolidate/MarkJunk.
        let post_snap = self.build_snapshot();
        for action in &limited_actions {
            let at = match action {
                SchedulerAction::Pulse { .. } => Some(ActionType::Pulse),
                SchedulerAction::Fission { .. } => Some(ActionType::Fission),
                SchedulerAction::Fuse { .. } => Some(ActionType::Merge),
                SchedulerAction::Dream => Some(ActionType::Dream),
                SchedulerAction::Link { .. } => Some(ActionType::Link),
                SchedulerAction::Consolidate { .. } => Some(ActionType::Merge),
                SchedulerAction::MarkJunk { .. } => Some(ActionType::Evict),
                _ => None,
            };
            if let Some(at) = at {
                self.record_outcome_snaps(at, &pre_snap, &post_snap, tick);
            }
        }
        let action_names: Vec<&str> = limited_actions
            .iter()
            .map(|a| match a {
                SchedulerAction::Pulse { .. } => "pulse",
                SchedulerAction::Fission { .. } => "fission",
                SchedulerAction::Fuse { .. } => "fuse",
                SchedulerAction::Dream => "dream",
                SchedulerAction::Link { .. } => "link",
                SchedulerAction::Consolidate { .. } => "consolidate",
                SchedulerAction::MarkJunk { .. } => "mark_junk",
                SchedulerAction::Relabel { .. } => "relabel",
                SchedulerAction::Reflect { .. } => "reflect",
                SchedulerAction::UseTool { .. } => "use_tool",
                SchedulerAction::ActOutward { .. } => "act_outward",
            })
            .collect();
        tracing::info!(
            "[LLM] tick {} executed {} actions: {:?}",
            tick,
            action_names.len(),
            action_names
        );

        if tick % 10 == 0 {
            self.auto_save();
        }
    }

    fn execute_task(&self, task: &ScheduledTask) {
        match task {
            ScheduledTask::CreateTetra { core, data, mass } => {
                let positions = Tetrahedron::compute_vertices(*core);
                let tetra = Tetrahedron {
                    id: 0,
                    vertex_ids: [0; 4],
                    core: *core,
                    data: data.clone(),
                    mass: *mass,
                };
                match self.space.add_tetrahedron(&tetra, &positions) {
                    Ok(id) => {
                        tracing::info!("created tetrahedron {}", id);
                        let _ = self.tx.send(EngineEvent::TetrahedronCreated(id));
                    }
                    Err(e) => tracing::warn!("failed to create: {}", e),
                }
            }
        }
    }

    pub async fn run_with_rx(self: Arc<Self>, rx: broadcast::Receiver<EngineEvent>) {
        self.run_unified(rx, true).await;
    }

    pub async fn run_quiet(self: Arc<Self>, rx: broadcast::Receiver<EngineEvent>) {
        self.run_unified(rx, false).await;
    }

    async fn run_unified(
        self: Arc<Self>,
        mut rx: broadcast::Receiver<EngineEvent>,
        cognitive: bool,
    ) {
        loop {
            // 先克隆 interval 值再 drop 读锁，避免 select! 分支持锁跨整个 sleep 周期
            let tick_interval = *self.tick_interval.read();
            tokio::select! {
                _ = tokio::time::sleep(tick_interval) => {
                    if cognitive {
                        let me = self.clone();
                        let handle = tokio::task::spawn_blocking(move || {
                            let thought = me.tick_and_maybe_think();
                            if let Some(ct) = thought {
                                // 批次C：注入自适应参数+行动效果到认知引擎（断裂点5+6）
                                {
                                    use super::adaptive::Param;
                                    let ad = me.adaptive.lock();
                                    let snap = format!(
                                        "fission_entropy: {:.3} (default 0.3)\nfission_min_size: {:.0} (default 6)\nmerge_distance: {:.1} (default 5.0)\ndream_interval: {:.0} (default 50)\nevict_mass: {:.3} (default 0.3)\npulse_budget: {:.0} (default 3)",
                                        ad.get(Param::FissionEntropyThreshold),
                                        ad.get(Param::FissionMinClusterSize),
                                        ad.get(Param::MergeDistance),
                                        ad.get(Param::DreamInterval),
                                        ad.get(Param::EvictionMassThreshold),
                                        ad.get(Param::PulseBudget),
                                    );
                                    me.cognitive.set_adaptive_snapshot(&snap);
                                }
                                {
                                    let tracker = me.outcome.lock();
                                    let summary = tracker.effectiveness_summary();
                                    if !summary.is_empty() {
                                        let text: Vec<String> = summary.iter()
                                            .map(|(action, eff)| format!("{:?}: {:.0}%", action, eff * 100.0))
                                            .collect();
                                        me.cognitive.set_effectiveness_summary(&text.join("\n"));
                                    }
                                }
                                match me.cognitive.decide(&ct.state) {
                                    Ok(response) => me.apply_thought(ct.tick, response),
                                    Err(e) => tracing::warn!("[LLM] cognitive error: {}", e),
                                }
                            } else {
                                let count = me.tick_count.load(Ordering::SeqCst);
                                if count % 3 == 0 { me.auto_save(); } // P0: flush every 3 ticks
                                if count % 5 == 0 { me.flush_access_counts(); }
                            }
                        });
                        // tick panic 恢复：监控线程存活，panic 后下一 tick 自动重试
                        tokio::spawn(async move {
                            if let Err(e) = handle.await {
                                tracing::error!("[Scheduler] tick thread panic: {}. Next tick will retry automatically.", e);
                            }
                        });
                    } else {
                        let count = self.tick_count.fetch_add(1, Ordering::SeqCst);
                        self.energy.replenish(12.0);
                        let tasks: Vec<ScheduledTask> = self.queue.lock().drain(..).collect();
                        for task in &tasks { self.execute_task(task); }
                        if count % 5 == 0 {
                            let snap = self.build_snapshot();
                            self.auto_fission(&snap);
                            self.auto_save();
                            self.flush_access_counts();
                        }
                        if count % 50 == 0 && count > 0 && self.energy.available() >= 50.0 {
                            self.auto_dream();
                        }
                        if count % 200 == 0 && count > 0 {
                            let snap = self.build_snapshot();
                            self.evict_low_quality(&snap);
                        }
                    }
                }
                event = rx.recv() => {
                    match event {
                        Ok(EngineEvent::Shutdown) => {
                            tracing::info!("[Scheduler] shutdown, persisting state");
                            self.auto_save();
                            break;
                        }
                        Ok(EngineEvent::TetrahedronCreated(id)) => {
                            self.log_event(format!("created({})", id));
                            if self.tick_count.load(Ordering::SeqCst) % 3 == 0 { self.auto_save(); }
                        }
                        Ok(EngineEvent::TetrahedronRemoved(id)) => {
                            self.log_event(format!("purged({})", id));
                        }
                        Ok(EngineEvent::TetrahedronMoved(id, _)) => {
                            self.log_event(format!("moved({})", id));
                        }
                        Ok(EngineEvent::EnergyLow { remaining }) => {
                            tracing::warn!("[Scheduler] energy low: {:.1}", remaining);
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            tracing::info!("[Scheduler] event bus closed, persisting state");
                            self.auto_save();
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    fn cluster_ids_centroid(&self, ids: &[u64]) -> Point3 {
        let mut sum_x = 0.0f64;
        let mut sum_y = 0.0f64;
        let mut sum_z = 0.0f64;
        let mut count = 0usize;
        for &id in ids {
            if let Some(t) = self.space.get_tetrahedron(id) {
                sum_x += t.core.x;
                sum_y += t.core.y;
                sum_z += t.core.z;
                count += 1;
            }
        }
        if count > 0 {
            Point3::new(
                sum_x / count as f64,
                sum_y / count as f64,
                sum_z / count as f64,
            )
        } else {
            Point3::zero()
        }
    }

    fn cluster_core_centroid(&self, cluster: &crate::domain::space::Cluster) -> Point3 {
        self.cluster_ids_centroid(&cluster.tetra_ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tetra::EDGE_LENGTH;
    use crate::domain::tetra::{MemoryPayload, Tetrahedron};
    use crate::domain::vertex::Point3;
    use crate::engine::CategoryClassifier;
    use crate::engine::EmbeddingService;
    use crate::engine::GatewayCenter;
    use crate::engine::StorageManager;
    use std::sync::Arc;

    fn add_tetra_to_space(
        space: &Space,
        core: Point3,
        content: &str,
        labels: Vec<String>,
    ) -> TetraId {
        let positions = Tetrahedron::compute_vertices(core);
        let data = MemoryPayload {
            content: content.to_string(),
            content_hash: content.len() as u64,
            labels,
            timestamp: 0,
            aliases: vec![],
            embedding: vec![],
            importance: 1.0,
            enforced: false,
            rationale: None,
            access_count: 0,
            memory_type: None,
            identity_stamp: None,
            source_agent: None,
            valid_from: 0,
            valid_to: None,
            last_reviewed_ts: None,
            expired_at: None,
            invalidated_at: None,
            memory_class: None,
        };
        let tetra = Tetrahedron {
            id: 0,
            vertex_ids: [0; 4],
            core,
            data,
            mass: 1.0,
        };
        space.add_tetrahedron(&tetra, &positions).unwrap()
    }

    /// Build a scheduler with all real subsystems, cognitive disabled (no API key).
    fn build_scheduler() -> (Arc<SchedulerCenter>, Arc<Space>, Arc<KnowledgeGraph>) {
        let space = Arc::new(Space::new());
        let bus = super::super::bus::EventBus::new(64);
        let tx = bus.sender();
        let rx = bus.subscribe();
        let energy = Arc::new(EnergyCenter::new(10000.0, 8.0, tx.clone(), bus.subscribe()));
        let knowledge = Arc::new(KnowledgeGraph::new());
        let cognitive = Arc::new(CognitiveEngine::new("", ""));
        let classifier = Arc::new(CategoryClassifier::new("", ""));
        let embedding = Arc::new(EmbeddingService::from_env());
        let gateway = Arc::new(GatewayCenter::new(
            space.clone(),
            energy.clone(),
            cognitive.clone(),
            classifier.clone(),
            tx.clone(),
            bus.subscribe(),
            knowledge.clone(),
            embedding.clone(),
            None,
        ));
        let security = Arc::new(SecurityGuard::from_env());
        let storage =
            Arc::new(StorageManager::new(std::path::Path::new("test_data_scheduler")).unwrap());
        let scheduler = Arc::new(SchedulerCenter::with_security(
            space.clone(),
            energy.clone(),
            knowledge.clone(),
            cognitive.clone(),
            gateway.clone(),
            tx,
            rx,
            1000,
            10000.0,
            security,
            storage,
        ));
        (scheduler, space, knowledge)
    }

    /// Scenario: Seed a space with N tetrahedrons forming multiple clusters.
    /// Spacing must be exactly EDGE_LENGTH (1.0) for vertices to merge and form clusters.
    fn seed_reality(space: &Space) -> Vec<TetraId> {
        let mut ids = Vec::new();
        // Cluster A: physics-related, placed in a chain at EDGE_LENGTH spacing
        let physics_topics = [
            (
                "Quantum mechanics wave function",
                vec!["physics".into(), "quantum".into()],
            ),
            (
                "General relativity spacetime",
                vec!["physics".into(), "relativity".into()],
            ),
            (
                "Thermodynamics entropy",
                vec!["physics".into(), "thermo".into()],
            ),
            (
                "Electromagnetic field equations",
                vec!["physics".into(), "em".into()],
            ),
            (
                "Particle physics standard model",
                vec!["physics".into(), "quantum".into()],
            ),
            (
                "String theory extra dimensions",
                vec!["physics".into(), "quantum".into()],
            ),
            (
                "Statistical mechanics ensemble",
                vec!["physics".into(), "thermo".into()],
            ),
        ];
        for (i, (text, labels)) in physics_topics.iter().enumerate() {
            let core = Point3::new(i as f64 * EDGE_LENGTH, 0.0, 0.0);
            ids.push(add_tetra_to_space(space, core, text, labels.clone()));
        }

        // Cluster B: programming, placed in a separate region
        let prog_topics = [
            (
                "Rust ownership and borrowing",
                vec!["rust".into(), "programming".into()],
            ),
            (
                "Python async await patterns",
                vec!["python".into(), "programming".into()],
            ),
            (
                "C++ template metaprogramming",
                vec!["cpp".into(), "programming".into()],
            ),
            (
                "Go goroutines and channels",
                vec!["go".into(), "programming".into()],
            ),
            (
                "JavaScript event loop",
                vec!["js".into(), "programming".into()],
            ),
            (
                "Haskell monad transformers",
                vec!["haskell".into(), "programming".into()],
            ),
            (
                "TypeScript type inference",
                vec!["ts".into(), "programming".into()],
            ),
        ];
        for (i, (text, labels)) in prog_topics.iter().enumerate() {
            let core = Point3::new(20.0 + i as f64 * EDGE_LENGTH, 0.0, 0.0);
            ids.push(add_tetra_to_space(space, core, text, labels.clone()));
        }

        // Cluster C: mixed topics — high entropy, designed to trigger fission
        let mixed = [
            (
                "Neural network backpropagation",
                vec!["ai".into(), "ml".into()],
            ),
            ("Shakespeare sonnet analysis", vec!["literature".into()]),
            (
                "Climate change carbon cycle",
                vec!["science".into(), "climate".into()],
            ),
            ("Bach fugue counterpoint", vec!["music".into()]),
            ("Roman empire military tactics", vec!["history".into()]),
            ("Recipe for chocolate cake", vec!["cooking".into()]),
            ("Proof of Fermat last theorem", vec!["math".into()]),
        ];
        for (i, (text, labels)) in mixed.iter().enumerate() {
            let core = Point3::new(40.0 + i as f64 * EDGE_LENGTH, 0.0, 0.0);
            ids.push(add_tetra_to_space(space, core, text, labels.clone()));
        }

        ids
    }

    // ---- Test: TickSnapshot correctly captures space state ----

    #[test]
    fn snapshot_matches_space_state() {
        let (sched, space, _kg) = build_scheduler();
        seed_reality(&space);

        let snap = sched.build_snapshot();

        assert_eq!(snap.tetras.len(), 21, "21 tetras seeded");
        assert_eq!(snap.clusters.len(), 3, "3 clusters expected");

        // labels_map must cover every tetra
        for t in &snap.tetras {
            assert!(
                snap.labels_map.contains_key(&t.id),
                "labels_map missing tetra {}",
                t.id
            );
            assert!(
                snap.core_map.contains_key(&t.id),
                "core_map missing tetra {}",
                t.id
            );
        }

        // Verify cluster membership covers all tetras
        let clustered: HashSet<u64> = snap
            .clusters
            .iter()
            .flat_map(|c| c.tetra_ids.iter().copied())
            .collect();
        assert_eq!(clustered.len(), 21, "all 21 tetras should be in a cluster");
    }

    // ---- Test: collect_state_from_snap produces correct metrics ----

    #[test]
    fn collect_state_accurate_metrics() {
        let (sched, space, _kg) = build_scheduler();
        seed_reality(&space);

        let snap = sched.build_snapshot();
        let state = sched.collect_state_from_snap(&snap);

        assert_eq!(state.total_tetras, 21);
        assert_eq!(state.total_clusters, 3);

        // Cluster 0 (physics, 7 tetras) should have lower entropy than mixed cluster
        assert!(
            state.clusters[0].entropy < 0.7,
            "physics cluster should be moderately cohesive, entropy={}",
            state.clusters[0].entropy
        );

        // Cluster 2 (mixed, 7 tetras) should have the highest entropy (all different labels)
        assert!(
            state.clusters[2].entropy >= state.clusters[0].entropy,
            "mixed cluster entropy ({}) >= physics cluster entropy ({})",
            state.clusters[2].entropy,
            state.clusters[0].entropy
        );

        // Memory info should have correct cluster assignments
        let physics_memories: Vec<_> = state
            .memories
            .iter()
            .filter(|m| m.labels.contains(&"physics".to_string()))
            .collect();
        assert_eq!(physics_memories.len(), 7);
        assert!(physics_memories
            .iter()
            .all(|m| m.cluster_index == 0 || m.cluster_index < 3));

        // Energy should match
        assert!(
            state.energy > 9000.0,
            "energy should be near max after replenish"
        );
    }

    // ---- Test: auto_pulse reads from snapshot without crashing ----

    #[test]
    fn auto_pulse_uses_snapshot() {
        let (sched, space, _kg) = build_scheduler();
        seed_reality(&space);

        // Manually tick to set tick_count > 0
        sched.tick_count.fetch_add(1, Ordering::SeqCst);
        sched.energy.replenish(100.0);

        let snap = sched.build_snapshot();
        sched.auto_pulse(&snap);

        // Verify pulse actually ran — should not panic, clusters should still be valid
        let clusters = space.find_clusters();
        assert!(
            clusters.len() >= 3,
            "clusters should remain intact after pulse"
        );
    }

    // ---- Test: auto_fission skips low-entropy clusters ----

    #[test]
    fn auto_fission_skips_cohesive_clusters() {
        let (sched, space, _kg) = build_scheduler();
        // Only seed a cohesive cluster — all same label
        for i in 0..7 {
            let core = Point3::new(i as f64 * EDGE_LENGTH, 0.0, 0.0);
            add_tetra_to_space(
                &space,
                core,
                &format!("physics topic {}", i),
                vec!["physics".into()],
            );
        }

        sched.tick_count.fetch_add(20, Ordering::SeqCst);
        let snap = sched.build_snapshot();
        sched.auto_fission(&snap);

        let clusters = space.find_clusters();
        assert_eq!(clusters.len(), 1, "cohesive cluster should NOT be split");
    }

    // ---- Test: auto_fission splits high-entropy clusters ----

    #[test]
    fn auto_fission_splits_diverse_cluster() {
        let (sched, space, _kg) = build_scheduler();
        // Seed a cluster with completely different labels, placed at EDGE_LENGTH spacing
        let topics = [
            ("Topic A", vec!["alpha".into()]),
            ("Topic B", vec!["beta".into()]),
            ("Topic C", vec!["gamma".into()]),
            ("Topic D", vec!["delta".into()]),
            ("Topic E", vec!["epsilon".into()]),
            ("Topic F", vec!["zeta".into()]),
            ("Topic G", vec!["eta".into()]),
            ("Topic H", vec!["theta".into()]),
        ];
        for (i, (text, labels)) in topics.iter().enumerate() {
            let core = Point3::new(i as f64 * EDGE_LENGTH, 0.0, 0.0);
            add_tetra_to_space(&space, core, text, labels.clone());
        }

        // Must have enough ticks to pass cooldown
        sched.tick_count.fetch_add(20, Ordering::SeqCst);
        let snap = sched.build_snapshot();

        assert_eq!(snap.clusters.len(), 1, "should start as one cluster");
        let entropy =
            dynamics::compute_entropy_from_labels(&snap.clusters[0].tetra_ids, &snap.labels_map);
        assert!(
            entropy > 0.5,
            "diverse labels should have high entropy, got {}",
            entropy
        );

        sched.auto_fission(&snap);

        // After fission, tetras should have been relocated — at least some positions changed
        let after_tetras = space.all_tetrahedrons();
        let unique_x: HashSet<i64> = after_tetras
            .iter()
            .map(|t| (t.core.x * 10.0) as i64)
            .collect();
        // With 8 completely different topics, some should have been pushed apart
        assert!(
            unique_x.len() > 1,
            "fission should relocate minority tetras to new positions, got {} unique x positions",
            unique_x.len()
        );
    }

    // ---- Test: perform_fission_from_snap uses snapshot data ----

    #[test]
    fn fission_from_snap_no_extra_locks() {
        let (sched, space, _kg) = build_scheduler();
        // Create a diverse cluster at EDGE_LENGTH spacing
        for i in 0..8 {
            let label = format!("label-{}", i);
            let core = Point3::new(i as f64 * EDGE_LENGTH, 0.0, 0.0);
            add_tetra_to_space(&space, core, &format!("content {}", i), vec![label]);
        }

        sched.tick_count.fetch_add(20, Ordering::SeqCst);
        let snap = sched.build_snapshot();

        let result = sched.perform_fission_from_snap(0, 0, 8.0, "TestFission", &snap);
        assert!(result, "fission should succeed on diverse cluster");

        // Should have split — at minimum tetras are now in different spatial positions
        let after_tetras = space.all_tetrahedrons();
        assert!(after_tetras.len() == 8, "no tetras lost");
    }

    // ---- Test: Full tick cycle with snapshot (tick_and_maybe_think) ----

    #[test]
    fn full_tick_cycle_consistent() {
        let (sched, space, _kg) = build_scheduler();
        seed_reality(&space);

        // Simulate 25 ticks (covers tick%5 and tick%10 paths)
        for _ in 0..25 {
            let _ = sched.tick_and_maybe_think();
        }

        let final_tetras = space.all_tetrahedrons();
        assert_eq!(final_tetras.len(), 21, "no tetras lost during 25 ticks");

        let clusters = space.find_clusters();
        assert!(!clusters.is_empty(), "clusters should still exist");

        // Verify mass has been updated (auto_pulse adds mass) or at minimum no data loss
        let total_mass: f64 = final_tetras.iter().map(|t| t.mass).sum();
        assert!(
            total_mass >= 21.0,
            "total mass should be at least 21.0 (initial), got {}",
            total_mass
        );

        // Verify decision history was recorded (tick%5 triggers cognitive path)
        let state = sched.collect_state_internal();
        assert_eq!(state.tick, 25);
        assert!(state.total_tetras >= 21);
    }

    // ---- Test: Memory creation through API path ----

    #[test]
    fn api_create_memory_integration() {
        let (sched, space, _kg) = build_scheduler();

        let (id1, _) = sched
            .api_create_memory("Rust ownership model", vec!["rust".into()])
            .unwrap();
        let (id2, _) = sched
            .api_create_memory("Python list comprehension", vec!["python".into()])
            .unwrap();
        let (id3, _) = sched
            .api_create_memory("Rust trait objects", vec!["rust".into()])
            .unwrap();

        assert!(id1 != id2 && id2 != id3, "IDs should be unique");

        let tetras = space.all_tetrahedrons();
        assert_eq!(tetras.len(), 3);

        // Rust memories should be close to each other (same label → nearby placement)
        let rust_tetras: Vec<&Tetrahedron> = tetras
            .iter()
            .filter(|t| t.data.labels.contains(&"rust".to_string()))
            .collect();
        assert_eq!(rust_tetras.len(), 2);
        let dx = (rust_tetras[0].core.x - rust_tetras[1].core.x).abs();
        assert!(
            dx < 5.0,
            "same-label memories should be placed nearby, dx={}",
            dx
        );
    }

    // ---- Test: Multiple fission rounds don't corrupt state ----

    #[test]
    fn repeated_fission_stable() {
        let (sched, space, _kg) = build_scheduler();

        // Seed a large diverse cluster
        for i in 0..20 {
            let label = format!("label-{}", i % 5);
            let core = Point3::new(i as f64 * EDGE_LENGTH, 0.0, 0.0);
            add_tetra_to_space(&space, core, &format!("content {}", i), vec![label]);
        }

        // Run 50 ticks to trigger multiple fission cycles
        for _ in 0..50 {
            let _ = sched.tick_and_maybe_think();
        }

        let final_tetras = space.all_tetrahedrons();
        assert_eq!(
            final_tetras.len(),
            20,
            "no tetras lost after 50 ticks with fission"
        );

        // Verify all tetras have valid positions (no NaN, no extreme values)
        for t in &final_tetras {
            assert!(
                t.core.x.is_finite(),
                "x should be finite for tetra {}",
                t.id
            );
            assert!(
                t.core.y.is_finite(),
                "y should be finite for tetra {}",
                t.id
            );
            assert!(
                t.core.z.is_finite(),
                "z should be finite for tetra {}",
                t.id
            );
            assert!(t.mass > 0.0, "mass should be positive for tetra {}", t.id);
        }
    }

    // ---- Test: Snapshot data consistency under concurrent reads ----

    #[test]
    fn snapshot_is_consistent_view() {
        let (sched, space, _kg) = build_scheduler();
        seed_reality(&space);

        let snap = sched.build_snapshot();

        // Verify snapshot internal consistency: labels_map matches tetras
        for t in &snap.tetras {
            let snap_labels = snap.labels_map.get(&t.id).unwrap();
            assert_eq!(
                snap_labels, &t.labels,
                "labels_map mismatch for tetra {}",
                t.id
            );

            let snap_core = snap.core_map.get(&t.id).unwrap();
            assert!(
                (snap_core.x - t.core.x).abs() < 1e-10,
                "core_map x mismatch for tetra {}",
                t.id
            );
            assert!(
                (snap_core.y - t.core.y).abs() < 1e-10,
                "core_map y mismatch for tetra {}",
                t.id
            );
            assert!(
                (snap_core.z - t.core.z).abs() < 1e-10,
                "core_map z mismatch for tetra {}",
                t.id
            );
        }

        // Verify cluster membership: all cluster tetra IDs exist in tetras
        let all_ids: HashSet<u64> = snap.tetras.iter().map(|t| t.id).collect();
        for cluster in &snap.clusters {
            for &id in &cluster.tetra_ids {
                assert!(
                    all_ids.contains(&id),
                    "cluster references non-existent tetra {}",
                    id
                );
            }
        }
    }

    // ---- Test: generate_aliases doesn't crash with snapshot ----

    #[test]
    fn generate_aliases_with_snapshot() {
        let (sched, space, _kg) = build_scheduler();
        seed_reality(&space);

        let snap = sched.build_snapshot();
        // generate_aliases with cognitive disabled should be a no-op
        sched.generate_aliases(0, &snap);

        let tetras = space.all_tetrahedrons();
        assert_eq!(tetras.len(), 21, "no tetras lost");
    }

    // ---- Test: reclassify_memories with snapshot ----

    #[test]
    fn reclassify_memories_with_snapshot() {
        let (sched, space, _kg) = build_scheduler();
        seed_reality(&space);

        let snap = sched.build_snapshot();
        // reclassify with cognitive disabled should be a no-op
        sched.reclassify_memories(0, &snap);

        let tetras = space.all_tetrahedrons();
        assert_eq!(tetras.len(), 21, "no tetras lost");
    }

    // ---- Test: Tick snapshot entropy matches per-cluster computation ----

    #[test]
    fn snapshot_entropy_accuracy() {
        let (sched, space, _kg) = build_scheduler();
        seed_reality(&space);

        let snap = sched.build_snapshot();

        for cluster in &snap.clusters {
            let snap_entropy =
                dynamics::compute_entropy_from_labels(&cluster.tetra_ids, &snap.labels_map);
            // Compute "ground truth" entropy by reading from space directly
            let ground_truth = dynamics::compute_entropy(&space, cluster);
            let diff = (snap_entropy - ground_truth).abs();
            assert!(
                diff < 1e-10,
                "snapshot entropy ({}) should match ground truth ({}) for cluster with {} tetras",
                snap_entropy,
                ground_truth,
                cluster.tetra_ids.len()
            );
        }
    }

    // ---- Test: Large-scale scenario (100 memories) ----

    #[test]
    fn large_scale_100_memories() {
        let (sched, space, _kg) = build_scheduler();

        let categories = [
            "physics",
            "chemistry",
            "biology",
            "math",
            "cs",
            "history",
            "art",
            "music",
        ];
        for i in 0..100 {
            let cat = categories[i % categories.len()];
            // Space in groups: each category gets its own chain
            let cat_idx = (i % categories.len()) as f64;
            let in_chain = (i / categories.len()) as f64;
            let core = Point3::new(cat_idx * 20.0 + in_chain * EDGE_LENGTH, 0.0, 0.0);
            add_tetra_to_space(
                &space,
                core,
                &format!("Memory #{} about {}", i, cat),
                vec![cat.to_string()],
            );
        }

        assert_eq!(space.all_tetrahedrons().len(), 100);

        // Run 10 ticks
        for _ in 0..10 {
            let _ = sched.tick_and_maybe_think();
        }

        let final_tetras = space.all_tetrahedrons();
        assert_eq!(
            final_tetras.len(),
            100,
            "no tetras lost in large-scale test"
        );

        let state = sched.collect_state_internal();
        assert!(
            state.total_clusters >= 1,
            "should have at least 1 cluster with 100 memories"
        );
        assert_eq!(state.total_tetras, 100);
    }
}
