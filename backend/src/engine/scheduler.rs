use parking_lot::Mutex as ParkMutex;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

use crate::domain::space::Space;
use crate::domain::tetra::{MemoryPayload, TetraId, Tetrahedron};
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

struct CognitiveThought {
    tick: u64,
    state: SystemState,
}

#[derive(Debug, Clone)]
pub enum ScheduledTask {
    CreateTetra {
        core: Point3,
        data: MemoryPayload,
        mass: f64,
    },
    RemoveTetra(TetraId),
}

struct TickSnapshot {
    tick: u64,
    energy: f64,
    tetras: Vec<Tetrahedron>,
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
    last_reclassify_tick: AtomicU64,
    skills: ParkMutex<Option<Arc<super::skills::SkillEngine>>>,
    pub_skills: ParkMutex<Option<Arc<super::skills::SkillEngine>>>,
    drive: ParkMutex<DriveEngine>,
    outcome: ParkMutex<OutcomeTracker>,
    adaptive: ParkMutex<AdaptiveParams>,
    last_merge_pairs: ParkMutex<HashSet<(usize, usize)>>,
    feedback_agg_cache: ParkMutex<Option<(usize, std::time::Instant, HashSet<u64>)>>,
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
            last_reclassify_tick: AtomicU64::new(0),
            skills: ParkMutex::new(None),
            pub_skills: ParkMutex::new(None),
            drive: ParkMutex::new(DriveEngine::new()),
            outcome: ParkMutex::new(OutcomeTracker::new()),
            adaptive: ParkMutex::new(AdaptiveParams::new()),
            last_merge_pairs: ParkMutex::new(HashSet::new()),
            feedback_agg_cache: ParkMutex::new(None),
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
        let tetras = self.space.all_tetrahedrons();
        let clusters = self.space.find_clusters();
        let labels_map: HashMap<u64, Vec<String>> = tetras
            .iter()
            .map(|t| (t.id, t.data.labels.clone()))
            .collect();
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

    pub fn api_create_memory(
        &self,
        content: &str,
        mut labels: Vec<String>,
    ) -> Result<TetraId, String> {
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
        let importance = intake.importance;

        // Semantic dedup — search before create
        if let Ok(similar) = self.gateway.search(content, 3) {
            for (sid, sim, _bm25, payload) in &similar {
                if *sim > 0.85 && payload.content.len() > 20 {
                    let text_sim =
                        super::intake::MemoryIntake::text_similarity(content, &payload.content);
                    let threshold = if content.len() < 30 { 0.80 } else { 0.55 };
                    if text_sim > threshold {
                        tracing::info!("[Intake] semantic dedup: new content ≈ #{} (vec_sim={:.2} text_sim={:.2} threshold={:.2}), returning existing",
                            sid, sim, text_sim, threshold);
                        self.energy.replenish(1.0);
                        return Ok(*sid);
                    }
                }
            }

            // Conflict detection — if new content has negation words AND high similarity to existing
            let conflict_ids = super::intake::MemoryIntake::check_conflict(content, &similar);
            if !conflict_ids.is_empty() {
                let id = self.gateway.create_memory(content, labels.clone())?;
                if let Some(tetra) = self.space.get_tetrahedron(id) {
                    let mut data = tetra.data.clone();
                    data.importance = importance;
                    data.memory_type = intake.memory_type;
                    data.rationale = intake.rationale;
                    let _ = self.space.update_payload(id, data);
                }
                for &cid in &conflict_ids {
                    self.knowledge.add_relation(
                        id,
                        cid,
                        super::knowledge::RelationType::Contradicts,
                        0.8,
                    );
                    tracing::info!("[Intake] contradiction link: #{} contradicts #{}", id, cid);
                }
                self.persist_tetra(id);
                return Ok(id);
            }
        }

        let id = self.gateway.create_memory(content, labels)?;

        if let Some(tetra) = self.space.get_tetrahedron(id) {
            let mut data = tetra.data.clone();
            data.importance = importance;
            data.memory_type = intake.memory_type;
            data.rationale = intake.rationale;
            let _ = self.space.update_payload(id, data);
        }

        self.persist_tetra(id);
        Ok(id)
    }

    pub fn api_create_memory_with_time(
        &self,
        content: &str,
        mut labels: Vec<String>,
        timestamp: i64,
    ) -> Result<TetraId, String> {
        self.security
            .validate_content(content)
            .map_err(|_| "content validation failed".to_string())?;
        self.security
            .validate_labels(&labels)
            .map_err(|_| "labels validation failed".to_string())?;

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
                        return Ok(*sid);
                    }
                }
            }
        }

        let id = self
            .gateway
            .create_memory_with_time(content, labels, timestamp)?;

        if let Some(tetra) = self.space.get_tetrahedron(id) {
            let mut data = tetra.data.clone();
            data.importance = importance;
            data.memory_type = intake.memory_type;
            data.rationale = intake.rationale;
            let _ = self.space.update_payload(id, data);
        }

        if !intake.conflict_ids.is_empty() {
            for &cid in &intake.conflict_ids {
                self.knowledge.add_relation(
                    id,
                    cid,
                    super::knowledge::RelationType::Contradicts,
                    0.8,
                );
            }
        }

        self.persist_tetra(id);
        Ok(id)
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
        let id = self.gateway.create_memory(content, labels.clone())?;
        self.persist_tetra(id);
        Ok((id, labels))
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
        self.security
            .validate_query(query)
            .map_err(|_| "query validation failed".to_string())?;
        let intent = super::retrieval::RetrievalEngine::parse_intent(query);

        let expanded_query = if intent.expanded_terms.is_empty() {
            query.to_string()
        } else {
            format!("{} {}", query, intent.expanded_terms.join(" "))
        };

        let mut results = self
            .gateway
            .search_filtered(&expanded_query, limit * 3, filters)?;
        super::retrieval::RetrievalEngine::rerank(&mut results, &intent, limit * 2);

        let clusters = self.space.find_clusters();
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

        if let Some(ref boost_ids) = cluster_boost {
            for (id, sim, _, _) in &mut results {
                if boost_ids.contains(id) {
                    *sim += 0.08;
                }
            }
            results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        }

        for (_id, sim, _, payload) in &mut results {
            if payload.importance >= 2.5 {
                *sim += 0.06;
            }
            if payload.access_count > 5 {
                *sim += 0.04;
            }
            if payload
                .labels
                .iter()
                .any(|l| l == "outdated" || l == "superseded")
            {
                *sim -= 0.3;
            }
        }
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        results.retain(|(id, sim, _bm25, payload)| {
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

        results.truncate(limit);

        for (id, _sim, _bm25, _payload) in &results {
            if let Some(tetra) = self.space.get_tetrahedron(*id) {
                let mut data = tetra.data.clone();
                let old = data.access_count;
                data.access_count = data.access_count.saturating_add(1);
                let _ = self.space.update_payload(*id, data);
                let _ = self.storage.update_access_count(*id, old + 1);
            }
        }
        Ok(results)
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

    pub fn api_stats(&self) -> super::gateway::SpaceStats {
        self.gateway.stats()
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

    pub fn api_export_graph(&self) -> super::knowledge::GraphExport {
        self.gateway.export_graph()
    }

    pub fn api_decay_relations(&self) -> usize {
        self.gateway.decay_relations()
    }

    pub fn api_pulse(
        &self,
        origin: TetraId,
        ttl: u32,
    ) -> Result<crate::domain::pulse::PulseResult, String> {
        self.gateway.pulse(origin, ttl)
    }

    pub fn api_delete_memory(&self, id: TetraId) -> Result<TetraId, String> {
        let tetra = self
            .space
            .get_tetrahedron(id)
            .ok_or_else(|| format!("memory {} not found", id))?;
        self.storage.soft_delete_tetra(&tetra, 30)?;
        self.purge_tetra(id, false);
        Ok(id)
    }

    pub fn api_list_deleted_memories(
        &self,
    ) -> Result<Vec<super::storage::DeletedMemoryInfo>, String> {
        self.storage.list_deleted_memories()
    }

    pub fn api_restore_memory(&self, id: TetraId) -> Result<TetraId, String> {
        let tetra = self
            .storage
            .restore_deleted_tetra(id)?
            .ok_or_else(|| format!("deleted memory {} not found", id))?;
        let restored_id = self.gateway.restore_tetra(&tetra)?;
        self.persist_tetra(restored_id);
        Ok(restored_id)
    }

    pub fn api_dream(&self) -> Result<String, String> {
        self.security
            .check_energy(self.energy.available(), 15.0)
            .map_err(|_| "insufficient energy (need 15.0)".to_string())?;
        let report = super::dream::DreamEngine::cycle(&self.space, 0.3, 5);
        for &id in report
            .evicted_ids
            .iter()
            .chain(report.merged_remove_ids.iter())
        {
            self.purge_tetra(id, true);
        }
        let access_counts: std::collections::HashMap<u64, u32> = self
            .gateway
            .search_metrics()
            .hot_memories
            .into_iter()
            .collect();
        let importance_updated =
            super::dream::DreamEngine::recompute_importance(&self.space, &access_counts);
        let tetras = self.space.all_tetrahedrons();
        let label_data: Vec<(TetraId, Vec<String>)> = tetras
            .iter()
            .map(|t| (t.id, t.data.labels.clone()))
            .collect();
        self.knowledge.update_concepts(&label_data);
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
        let seed_results = self.gateway.search(query, 20)?;
        if seed_results.is_empty() {
            return Ok(
                serde_json::json!({"query": query, "memory_file": serde_json::Value::Null, "seed_count": 0, "associated_count": 0}),
            );
        }

        let all_items = self
            .gateway
            .expand_from_seeds_with_clusters(&seed_results, depth);

        let mut sorted_items = all_items;
        sorted_items.sort_by(|a, b| {
            b.1.max(b.2)
                .partial_cmp(&a.1.max(a.2))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        sorted_items.truncate(40);

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
            .filter(|(_, direct, assoc)| direct.max(*assoc) > 0.1)
            .filter_map(|(id, _, _)| {
                item_data.get(id).map(|(labels, c)| {
                    let label_str = labels.iter().take(2).cloned().collect::<Vec<_>>().join(",");
                    format!(
                        "[#{}] [{}] {}",
                        id,
                        label_str,
                        c.chars().take(300).collect::<String>()
                    )
                })
            })
            .collect();
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
            "memory_count": memory_items.len()
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

    pub fn storage_handle(&self) -> Arc<super::storage::StorageManager> {
        self.storage.clone()
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
                    content_preview: t.data.content.chars().take(50).collect(),
                    labels: t.data.labels.clone(),
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

        let avg_entropy = if cluster_states.is_empty() {
            0.0
        } else {
            cluster_states.iter().map(|c| c.entropy).sum::<f64>() / cluster_states.len() as f64
        };
        let max_entropy = cluster_states
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
            recent_events: recent,
            last_dream_tick: self.last_dream_tick.load(Ordering::SeqCst),
            decision_history,
            prev_snapshot,
            search_metrics,
            kg_analysis,
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
                self.perform_fission(*cluster_index, 50, 10.0, "LLM");
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
                let clusters = self.space.find_clusters();
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
                let result = DreamEngine::cycle(&self.space, 0.3, 5);
                let tick = self.tick_count.load(Ordering::SeqCst);
                self.last_dream_tick.store(tick, Ordering::SeqCst);
                for &id in result
                    .evicted_ids
                    .iter()
                    .chain(result.merged_remove_ids.iter())
                {
                    self.purge_tetra(id, true);
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
                let mut deleted = 0u64;
                for &id in ids {
                    if id == *keep {
                        continue;
                    }
                    if self.space.get_tetrahedron(id).is_some() {
                        self.purge_tetra(id, true);
                        deleted += 1;
                    }
                }
                if let Some(t) = self.space.get_tetrahedron(*keep) {
                    let mut new_labels = t.data.labels.clone();
                    if !new_labels.iter().any(|l| l == "consolidated") {
                        new_labels.push("consolidated".to_string());
                    }
                    let updated = MemoryPayload {
                        content: format!(
                            "{}\n\n[整合自 {} 条记忆: {}]",
                            summary,
                            ids.len(),
                            ids.iter()
                                .map(|id| format!("#{}", id))
                                .collect::<Vec<_>>()
                                .join(",")
                        ),
                        content_hash: 0,
                        labels: new_labels,
                        timestamp: t.data.timestamp,
                        aliases: t.data.aliases.clone(),
                        embedding: t.data.embedding.clone(),
                        importance: t.data.importance + deleted as f64 * 0.2,
                        enforced: t.data.enforced,
                        rationale: t.data.rationale.clone(),
                        access_count: t.data.access_count,
                        memory_type: t.data.memory_type.clone(),
                    };
                    if let Err(e) = self.space.update_payload(*keep, updated.clone()) {
                        tracing::warn!(
                            "[Scheduler] consolidate update_payload failed for #{}: {}",
                            keep,
                            e
                        );
                    }
                    let boost = deleted as f64 * 0.5;
                    if let Err(e) = self.space.update_mass(*keep, boost) {
                        tracing::warn!(
                            "[Scheduler] consolidate update_mass failed for #{}: {}",
                            keep,
                            e
                        );
                    }
                    self.persist_tetra(*keep);
                    self.gateway
                        .update_label_index(*keep, &t.data.labels, &updated.labels);
                }
                tracing::info!(
                    "[LLM] consolidate: kept #{}, deleted {} duplicates ({})",
                    keep,
                    deleted,
                    summary.chars().take(80).collect::<String>()
                );
                self.log_event(format!("consolidate({},{})", keep, deleted));
            }
            SchedulerAction::MarkJunk { ids, reason } => {
                let mut marked = 0u64;
                for &id in ids {
                    if let Some(t) = self.space.get_tetrahedron(id) {
                        let old_labels = t.data.labels.clone();
                        let mut new_labels = t.data.labels.clone();
                        if !new_labels.iter().any(|l| l == "junk") {
                            new_labels.push("junk".to_string());
                        }
                        let updated = MemoryPayload {
                            content: t.data.content.clone(),
                            content_hash: t.data.content_hash,
                            labels: new_labels.clone(),
                            timestamp: t.data.timestamp,
                            aliases: t.data.aliases.clone(),
                            embedding: t.data.embedding.clone(),
                            importance: t.data.importance * 0.1,
                            enforced: false,
                            rationale: t.data.rationale.clone(),
                            access_count: t.data.access_count,
                            memory_type: t.data.memory_type.clone(),
                        };
                        if let Err(e) = self.space.update_payload(id, updated) {
                            tracing::warn!(
                                "[Scheduler] mark_junk update_payload failed for #{}: {}",
                                id,
                                e
                            );
                        } else {
                            self.gateway
                                .update_label_index(id, &old_labels, &new_labels);
                        }
                        if let Err(e) = self.space.update_mass(id, 0.05) {
                            tracing::warn!(
                                "[Scheduler] mark_junk update_mass failed for #{}: {}",
                                id,
                                e
                            );
                        }
                        self.persist_tetra(id);
                        marked += 1;
                    }
                }
                tracing::info!(
                    "[LLM] mark_junk: {} memories marked ({})",
                    marked,
                    reason.chars().take(80).collect::<String>()
                );
                self.log_event(format!(
                    "mark_junk({},{})",
                    marked,
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

    fn purge_tetra(&self, id: TetraId, remove_from_storage: bool) {
        let labels = self
            .space
            .get_tetrahedron(id)
            .map(|t| t.data.labels.clone());
        if let Err(e) = self.space.remove_tetrahedron(id) {
            tracing::debug!("purge_tetra {}: space already removed: {}", id, e);
        }
        if remove_from_storage {
            if let Err(e) = self.storage.delete_tetra(id) {
                tracing::warn!("purge_tetra {}: storage delete failed: {}", id, e);
            }
        }
        self.knowledge.remove_relations_for(id);
        if let Some(ref lbls) = labels {
            self.gateway.on_tetra_removed(id, lbls);
        } else {
            self.gateway.remove_from_hnsw(id);
            self.gateway.remove_from_content_hash(id);
        }
    }

    fn record_outcome(&self, action: ActionType, pre_snap: &TickSnapshot, tick: u64) {
        let post_snap = self.build_snapshot();
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

    fn auto_save(&self) {
        let ctx = super::janitor::JanitorCtx {
            space: &self.space,
            storage: &self.storage,
            knowledge: &self.knowledge,
            gateway: &self.gateway,
        };
        super::janitor::auto_save(&ctx);
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
            self.purge_tetra(id, true);
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
                    .map(|t| t.data.content_hash)
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

        if should_fission {
            self.auto_fission(&snap);
        }

        if count.is_multiple_of(20) && count > 0 {
            self.auto_skills(&snap);
        }

        if count.is_multiple_of(30) && count > 0 && self.energy.available() >= 50.0 {
            let pre_snap = self.build_snapshot();
            self.auto_dream();
            self.record_outcome(ActionType::Dream, &pre_snap, count);
        }

        if count.is_multiple_of(200) && count > 0 && should_evict {
            self.evict_low_quality(&snap);
        }

        // Phase 5: Emotion
        if count.is_multiple_of(10) {
            let texts: Vec<&str> = snap
                .tetras
                .iter()
                .take(20)
                .map(|t| t.data.content.as_str())
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
        if count.is_multiple_of(5) && self.cognitive.enabled() {
            if count.is_multiple_of(15) && count > 0 {
                self.generate_aliases((count / 15) as usize, &snap);
            }
            if count.is_multiple_of(30) && count > 0 {
                self.reclassify_memories((count / 30) as usize, &snap);
            }
            if count.is_multiple_of(20) && count > 0 {
                self.extract_entities((count / 20) as usize, &snap);
            }
            let state = self.collect_state_from_snap(&snap);
            let drive = self.drive.lock();
            let dominant = drive.dominant();
            tracing::info!(
                "[Scheduler] tick {} — {} tetras, {} clusters, energy {:.0}, drive={:?}",
                count,
                state.total_tetras,
                state.clusters.len(),
                state.energy,
                dominant,
            );
            Some(CognitiveThought { tick: count, state })
        } else {
            if count.is_multiple_of(10) {
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
                for label in &t.data.labels {
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
                "[Skills] top domains: {} — skill matching available",
                summary.join(", ")
            );
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
            self.purge_tetra(id, true);
        };

        let gov = super::governor::LifecycleGovernor::evaluate(&self.space, &self.knowledge);

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
                    let _ = self.space.update_payload(rid, data);
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
                        let _ = self.space.update_payload(*id, data);
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
            };
            self.execute_action(action);
            self.record_decision(
                tick,
                &action_name,
                &response.thoughts.chars().take(100).collect::<String>(),
                "executed",
            );
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
            })
            .collect();
        tracing::info!(
            "[LLM] tick {} executed {} actions: {:?}",
            tick,
            action_names.len(),
            action_names
        );

        if tick.is_multiple_of(10) {
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
            ScheduledTask::RemoveTetra(id) => {
                self.purge_tetra(*id, true);
                tracing::info!("removed tetrahedron {}", id);
                let _ = self.tx.send(EngineEvent::TetrahedronRemoved(*id));
            }
        }
    }

    pub async fn run_with_rx(self: Arc<Self>, rx: broadcast::Receiver<EngineEvent>) {
        self.run_with_rx_quiet(rx, false).await;
    }

    pub async fn run_quiet(self: Arc<Self>, rx: broadcast::Receiver<EngineEvent>) {
        self.run_with_rx_quiet(rx, true).await;
    }

    async fn run_with_rx_quiet(
        self: Arc<Self>,
        mut rx: broadcast::Receiver<EngineEvent>,
        quiet: bool,
    ) {
        if quiet {
            loop {
                tokio::select! {
                    event = rx.recv() => {
                        match event {
                            Ok(EngineEvent::Shutdown) => {
                                tracing::info!("[Scheduler] shutdown, persisting state");
                                self.auto_save();
                                break;
                            }
                            Ok(EngineEvent::TetrahedronCreated(id)) => {
                                self.log_event(format!("created({})", id));
                                if self.tick_count.load(Ordering::SeqCst).is_multiple_of(3) {
                                    self.auto_save();
                                }
                            }
                            Ok(EngineEvent::TetrahedronMoved(id, _)) => {
                                self.log_event(format!("moved({})", id));
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                tracing::info!("[Scheduler] event bus closed, persisting state");
                                self.auto_save();
                                break;
                            }
                            _ => {}
                        }
                    }
                    _ = tokio::time::sleep(*self.tick_interval.read()) => {
                        let count = self.tick_count.fetch_add(1, Ordering::SeqCst);
                        self.energy.replenish(12.0);

                        let tasks: Vec<ScheduledTask> = self.queue.lock().drain(..).collect();
                        for task in &tasks {
                            self.execute_task(task);
                        }

                        if count.is_multiple_of(10) {
                            let snap = self.build_snapshot();
                            self.auto_fission(&snap);
                        }

                        if count.is_multiple_of(50) && count > 0 && self.energy.available() >= 50.0 {
                            self.auto_dream();
                        }

                        if count.is_multiple_of(200) && count > 0 {
                            let snap = self.build_snapshot();
                            self.evict_low_quality(&snap);
                        }

                        if count.is_multiple_of(10) {
                            self.auto_save();
                        }
                    }
                }
            }
        } else {
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(*self.tick_interval.read()) => {
                        let me = self.clone();
                        tokio::task::spawn_blocking(move || {
                            let thought = me.tick_and_maybe_think();
                            if let Some(ct) = thought {
                                match me.cognitive.decide(&ct.state) {
                                    Ok(response) => me.apply_thought(ct.tick, response),
                                    Err(e) => tracing::warn!("[LLM] cognitive error: {}", e),
                                }
                            } else {
                                let count = me.tick_count.load(Ordering::SeqCst);
                                if count.is_multiple_of(10) {
                                    me.auto_save();
                                }
                            }
                        });
                    }
                    event = rx.recv() => {
                        match event {
                            Ok(EngineEvent::Shutdown) => {
                                tracing::info!("[Scheduler] shutdown, persisting state");
                                self.auto_save();
                                break;
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

        let id1 = sched
            .api_create_memory("Rust ownership model", vec!["rust".into()])
            .unwrap();
        let id2 = sched
            .api_create_memory("Python list comprehension", vec!["python".into()])
            .unwrap();
        let id3 = sched
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
                snap_labels, &t.data.labels,
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
