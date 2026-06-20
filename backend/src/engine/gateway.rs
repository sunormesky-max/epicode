use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering as AtomicOrdering;
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::domain::space::Space;
use crate::domain::tetra::{MemoryPayload, TetraId, EDGE_LENGTH};
use crate::domain::vertex::Point3;

use super::bus::{EngineEvent, EventSender};
use super::cache::{CacheLayer, CacheStats};
use super::cache::CacheValue;
use super::classifier::CategoryClassifier;
use super::cognitive::CognitiveEngine;
use super::embedding::EmbeddingService;
use super::energy::{EnergyCenter, CREATE_COST, PULSE_COST};
use super::hnsw::HnswIndex;
use super::index_manager::IndexManager;
use super::knowledge::KnowledgeGraph;
use super::search_engine::{SearchCtx, SearchEngineState};
use super::vector::{VectorLayer, EMBEDDING_DIM};

pub struct GatewayCenter {
    space: Arc<Space>,
    energy: Arc<EnergyCenter>,
    cognitive: Arc<CognitiveEngine>,
    classifier: Arc<CategoryClassifier>,
    embedding: Arc<EmbeddingService>,
    vector: Option<Arc<VectorLayer>>,
    tx: EventSender,
    pub knowledge: Arc<KnowledgeGraph>,
    search: SearchEngineState,
    index: IndexManager,
    cache: Arc<CacheLayer>,
    cache_stats: Arc<CacheStats>,
}

impl GatewayCenter {
    pub fn new(
        space: Arc<Space>,
        energy: Arc<EnergyCenter>,
        cognitive: Arc<CognitiveEngine>,
        classifier: Arc<CategoryClassifier>,
        tx: EventSender,
        _rx: broadcast::Receiver<EngineEvent>,
        knowledge: Arc<KnowledgeGraph>,
        embedding: Arc<EmbeddingService>,
        vector: Option<Arc<VectorLayer>>,
    ) -> Self {
        let mut hnsw = HnswIndex::new(EMBEDDING_DIM, 16, 200);
        let mut label_idx: HashMap<String, Vec<TetraId>> = HashMap::new();
        let mut chash_idx: HashMap<u64, TetraId> = HashMap::new();
        {
            let tetras = space.all_tetrahedrons();
            for t in &tetras {
                if !t.data.embedding.is_empty() && t.data.embedding.len() == EMBEDDING_DIM {
                    hnsw.insert(t.id, t.data.embedding.clone());
                }
                for label in &t.data.labels {
                    label_idx.entry(label.clone()).or_default().push(t.id);
                }
                chash_idx.entry(t.data.content_hash).or_insert(t.id);
            }
        }

        let cache_stats = CacheStats::new();
        let cache = Arc::new(CacheLayer::new(cache_stats.clone()));

        Self {
            space,
            energy,
            cognitive,
            classifier,
            embedding,
            vector,
            tx,
            knowledge,
            search: SearchEngineState::new(hnsw),
            index: IndexManager::new(label_idx, chash_idx),
            cache,
            cache_stats,
        }
    }

    pub fn mark_dirty(&self, id: TetraId) {
        self.index.mark_dirty(id);
    }

    pub fn invalidate_placement_cache(&self) {
        self.index.invalidate_placement_cache();
    }

    pub fn drain_dirty(&self) -> Vec<TetraId> {
        self.index.drain_dirty()
    }

    pub fn rebuild_hnsw(&self) {
        self.index.rebuild_hnsw(&self.search.hnsw, &self.space);
    }

    fn compute_embedding(&self, text: &str) -> Vec<f64> {
        if let Some(ref vl) = self.vector {
            match vl.embed(text) {
                Ok(emb) => return emb,
                Err(e) => tracing::warn!("[Gateway] ONNX embed failed: {}", e),
            }
        }
        if self.embedding.enabled() {
            match self.embedding.embed(text) {
                Ok(emb) => return emb,
                Err(e) => tracing::warn!("[Gateway] HTTP embed failed: {}", e),
            }
        }
        vec![]
    }

    pub fn create_memory(&self, content: &str, labels: Vec<String>) -> Result<TetraId, String> {
        self.create_memory_with_time(content, labels, 0)
    }

    pub fn create_memory_with_time(
        &self,
        content: &str,
        labels: Vec<String>,
        timestamp: i64,
    ) -> Result<TetraId, String> {
        if !self.energy.consume(CREATE_COST) {
            return Err("insufficient energy".into());
        }

        let content_hash = super::search_engine::hash_content(content);

        {
            if let Some(existing_id) = self.index.check_content_hash(content_hash) {
                if let Some(t) = self.space.get_tetrahedron(existing_id) {
                    if t.data.content == content {
                        tracing::info!(
                            "duplicate detected (hash index), returning existing tetra {}",
                            t.id
                        );
                        self.energy.replenish(CREATE_COST);
                        return Ok(t.id);
                    }
                }
            }
        }

        let ts = if timestamp > 0 {
            timestamp
        } else {
            chrono::Utc::now().timestamp()
        };

        let layer = crate::domain::cylinder::CylinderLayer::from_labels(&labels);
        let (core, has_port) = self.find_best_placement(&labels, layer);

        let embedding = self.compute_embedding(content);
        tracing::info!(
            "[Gateway] embedding result: {} dims (vector={}, embed_svc={})",
            embedding.len(),
            self.vector.is_some(),
            self.embedding.enabled()
        );

        let importance = Self::compute_importance(content, &labels);
        let positions = crate::domain::tetra::Tetrahedron::compute_vertices(core);
        let data = MemoryPayload {
            content: content.to_string(),
            content_hash,
            labels,
            timestamp: ts,
            aliases: vec![],
            embedding,
            importance,
            enforced: false,
            rationale: None,
            access_count: 0,
            memory_type: None,
        };
        let tetra = crate::domain::tetra::Tetrahedron {
            id: 0,
            vertex_ids: [0; 4],
            core,
            data,
            mass: 1.0,
        };

        match self.space.add_tetrahedron(&tetra, &positions) {
            Ok(id) => {
                if has_port {
                    self.space.reassign_cylinder_port(Self::PORT_SENTINEL, id);
                }
                {
                    let t = self.space.get_tetrahedron(id);
                    if let Some(t) = &t {
                        if !t.data.embedding.is_empty() && t.data.embedding.len() == EMBEDDING_DIM {
                            self.search.hnsw.lock().insert(id, t.data.embedding.clone());
                        }
                    }
                }
                self.knowledge
                    .auto_link_one(id, &self.space, &self.index.label_index.lock());
                let created_labels = self
                    .space
                    .get_tetrahedron(id)
                    .map(|t| t.data.labels.clone())
                    .unwrap_or_default();
                self.index.insert_labels(id, &created_labels);
                self.index.insert_content_hash(content_hash, id);
                {
                    let classifier = self.classifier.clone();
                    let c = content.to_string();
                    let l = created_labels;
                    let spawned = loop {
                        let current = classifier
                            .thread_count
                            .load(std::sync::atomic::Ordering::Acquire);
                        if current >= 4 {
                            break false;
                        }
                        if classifier
                            .thread_count
                            .compare_exchange_weak(
                                current,
                                current + 1,
                                std::sync::atomic::Ordering::AcqRel,
                                std::sync::atomic::Ordering::Acquire,
                            )
                            .is_ok()
                        {
                            break true;
                        }
                    };
                    if spawned {
                        std::thread::spawn(move || {
                            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                let _ = classifier.classify(&c, &l);
                            }));
                            classifier
                                .thread_count
                                .fetch_sub(1, std::sync::atomic::Ordering::Release);
                        });
                    }
                }
                let _ = self.tx.send(EngineEvent::TetrahedronCreated(id));
                self.search.invalidate_df_cache();
                let cache = self.cache.clone();
                if let Ok(handle) = tokio::runtime::Handle::try_current() {
                    handle.spawn(async move {
                        cache.invalidate("search:*").await;
                    });
                } else {
                    futures::executor::block_on(cache.invalidate("search:*"));
                }
                tracing::info!("memory created: tetra {} hash={}", id, content_hash);
                Ok(id)
            }
            Err(e) => {
                if has_port {
                    self.space.release_cylinder_port(Self::PORT_SENTINEL);
                }
                self.energy.replenish(CREATE_COST);
                Err(e)
            }
        }
    }

    const PORT_SENTINEL: TetraId = u64::MAX;

    fn compute_importance(content: &str, labels: &[String]) -> f64 {
        let mut score: f64 = 1.0;
        let lower = content.to_lowercase();

        let high_value_keywords = [
            "架构",
            "architecture",
            "决策",
            "decision",
            "关键",
            "critical",
            "重要",
            "important",
            "核心",
            "core",
            "设计",
            "design",
            "安全",
            "security",
            "部署",
            "deploy",
            "production",
            "生产",
            "数据库",
            "database",
            "密钥",
            "secret",
            "密钥",
            "key",
            "约束",
            "constraint",
            "不能改",
            "陷阱",
            "坑",
            "pitfall",
            "血的教训",
            "lesson",
            "bug",
            "修复",
            "fix",
        ];
        for kw in &high_value_keywords {
            if lower.contains(kw) {
                score += 0.3;
            }
        }

        let low_value_keywords = [
            "测试",
            "test",
            "tmp",
            "临时",
            "scratch",
            "实验",
            "experiment",
            "随便",
            "hello world",
            "测试内容",
            "testing 123",
        ];
        for kw in &low_value_keywords {
            if lower.contains(kw) {
                score -= 0.3;
            }
        }

        let high_value_labels = [
            "decision",
            "architecture",
            "security",
            "critical",
            "project-context",
            "deployment",
            "configuration",
        ];
        for label in labels {
            let label_lower = label.to_lowercase();
            for hv in &high_value_labels {
                if label_lower.contains(hv) {
                    score += 0.4;
                }
            }
        }

        let low_value_labels = ["test", "testing", "tmp", "scratch", "junk"];
        for label in labels {
            let label_lower = label.to_lowercase();
            for lv in &low_value_labels {
                if label_lower == *lv {
                    score -= 0.5;
                }
            }
        }

        let content_len = content.len();
        if content_len > 500 {
            score += 0.2;
        }
        if content_len > 1500 {
            score += 0.3;
        }
        if content_len < 30 {
            score -= 0.2;
        }

        score.clamp(0.1, 3.0)
    }

    fn find_best_placement(
        &self,
        labels: &[String],
        layer: crate::domain::cylinder::CylinderLayer,
    ) -> (Point3, bool) {
        if let Some(pos) = self.index.get_cached_placement(labels) {
            return (pos, false);
        }

        let tetras = self.space.all_tetrahedrons();
        let zone = self.space.zone_for_layer(layer);

        let in_layer: Vec<crate::domain::tetra::Tetrahedron> = tetras
            .iter()
            .filter(|t| zone.contains_z(t.core.z))
            .cloned()
            .collect();

        if in_layer.is_empty() {
            let z = zone.center_z();
            let port_opt = self.space.assign_cylinder_port(layer, Self::PORT_SENTINEL);
            let anchor = if let Some((_vid, pos)) = port_opt {
                pos
            } else {
                Point3::new(0.0, 0.0, z)
            };
            let result = self.find_adjacent_position(anchor, &in_layer);
            self.index.cache_placement(labels, result);
            return (result, port_opt.is_some());
        }

        let port_opt = self.space.assign_cylinder_port(layer, Self::PORT_SENTINEL);
        let anchor = self.find_anchor_by_labels(labels, &in_layer);
        let result = self.find_adjacent_position(anchor, &in_layer);
        self.index.cache_placement(labels, result);
        (result, port_opt.is_some())
    }

    fn find_anchor_by_labels(
        &self,
        labels: &[String],
        tetras: &[crate::domain::tetra::Tetrahedron],
    ) -> Point3 {
        let label_idx = self.index.label_index.lock();
        let mut score_map: HashMap<TetraId, usize> = HashMap::new();
        for label in labels {
            if let Some(ids) = label_idx.get(label) {
                for &id in ids {
                    *score_map.entry(id).or_insert(0) += 1;
                }
            }
        }

        if let Some((&best_id, _)) = score_map.iter().max_by_key(|(_, s)| *s) {
            if let Some(t) = tetras.iter().find(|t| t.id == best_id) {
                return t.core;
            }
        }

        if let Some(nearest) = self.space.nearest_tetrahedron_to(Point3::zero()) {
            if let Some(t) = tetras.iter().find(|t| t.id == nearest.0) {
                return t.core;
            }
        }

        Point3::zero()
    }

    fn find_adjacent_position(
        &self,
        anchor: Point3,
        _tetras: &[crate::domain::tetra::Tetrahedron],
    ) -> Point3 {
        let anchor_verts = crate::domain::tetra::Tetrahedron::compute_vertices(anchor);

        let mut best_pos = None;
        let mut best_merges = 0i32;

        for &av in &anchor_verts {
            let dir = Point3::new(av.x - anchor.x, av.y - anchor.y, av.z - anchor.z);
            let dir_len = (dir.x * dir.x + dir.y * dir.y + dir.z * dir.z).sqrt();
            if dir_len < 1e-10 {
                continue;
            }
            let candidate = Point3::new(
                anchor.x + dir.x / dir_len * EDGE_LENGTH,
                anchor.y + dir.y / dir_len * EDGE_LENGTH,
                anchor.z + dir.z / dir_len * EDGE_LENGTH,
            );

            let candidate_verts = crate::domain::tetra::Tetrahedron::compute_vertices(candidate);
            let merges = self.space.count_vertex_merges(&candidate_verts);

            if merges > best_merges {
                best_merges = merges;
                best_pos = Some(candidate);
            }
        }

        if best_merges > 0 {
            return best_pos.unwrap();
        }

        if let Some(pos) = best_pos {
            return pos;
        }

        let dx = anchor.x;
        let dy = anchor.y;
        let dz = anchor.z;
        let dist = (dx * dx + dy * dy + dz * dz).sqrt();
        if dist > 1e-10 {
            let s = EDGE_LENGTH / dist;
            Point3::new(anchor.x + dx * s, anchor.y + dy * s, anchor.z + dz * s)
        } else {
            Point3::new(EDGE_LENGTH, 0.0, 0.0)
        }
    }

    pub fn search(
        &self,
        query: &str,
        k: usize,
    ) -> Result<Vec<(TetraId, f64, f64, MemoryPayload)>, String> {
        self.search_filtered(query, k, None)
    }

    pub fn search_filtered(
        &self,
        query: &str,
        k: usize,
        filters: Option<&super::search_engine::SearchFilters>,
    ) -> Result<Vec<(TetraId, f64, f64, MemoryPayload)>, String> {
        let cache_key = CacheLayer::generate_query_key(query, filters);
        if let Some(cached) = futures::executor::block_on(self.cache.get(&cache_key)) {
            return Ok(cached.results);
        }

        let ctx = SearchCtx {
            state: &self.search,
            space: &self.space,
            knowledge: &self.knowledge,
            cognitive: &self.cognitive,
            embedding: &self.embedding,
            label_index: &self.index.label_index,
        };
        let results = super::search_engine::search(&ctx, query, k, self.vector.as_deref(), filters)?;
        futures::executor::block_on(self.cache.set(
            cache_key,
            CacheValue {
                results: results.clone(),
                timestamp: chrono::Utc::now().timestamp(),
            },
        ));
        Ok(results)
    }

    pub fn cache_stats_snapshot(&self) -> (u64, u64, u64, u64, u64, f64, f64, f64) {
        (
            self.cache_stats.l1_hits.load(AtomicOrdering::Relaxed),
            self.cache_stats.l1_misses.load(AtomicOrdering::Relaxed),
            self.cache_stats.l2_hits.load(AtomicOrdering::Relaxed),
            self.cache_stats.l2_misses.load(AtomicOrdering::Relaxed),
            self.cache_stats.evictions.load(AtomicOrdering::Relaxed),
            self.cache_stats.hit_ratio(),
            self.cache_stats.l1_hit_ratio(),
            self.cache_stats.l2_hit_ratio(),
        )
    }

    pub fn clear_query_cache(&self) {
        futures::executor::block_on(self.cache.clear());
    }

    pub fn expand_from_seeds(
        &self,
        seed_results: &[(TetraId, f64, f64, MemoryPayload)],
        depth: usize,
    ) -> Vec<(TetraId, f64, Vec<String>, String, i64)> {
        let mut collected: HashMap<u64, (f64, Vec<String>, String, i64)> = HashMap::new();
        for (id, sim, _mass, payload) in seed_results {
            collected.insert(
                *id,
                (
                    *sim,
                    payload.labels.clone(),
                    payload.content.clone(),
                    payload.timestamp,
                ),
            );
        }

        let mut frontier: Vec<(u64, usize, f64)> = seed_results
            .iter()
            .map(|(id, sim, _, _)| (*id, 0, *sim))
            .collect();
        let mut visited: HashSet<u64> = seed_results.iter().map(|(id, _, _, _)| *id).collect();

        while let Some((current_id, d, inherited_sim)) = frontier.pop() {
            if d >= depth {
                continue;
            }
            for (target_id, _rel_type, strength) in self.get_relations(current_id) {
                if visited.contains(&target_id) {
                    continue;
                }
                visited.insert(target_id);
                if let Some(payload) = self.get_node(target_id) {
                    let assoc = inherited_sim.max(strength);
                    collected.insert(
                        target_id,
                        (0.0, payload.labels, payload.content, payload.timestamp),
                    );
                    frontier.push((target_id, d + 1, assoc));
                }
            }
        }

        collected
            .into_iter()
            .map(|(id, (ds, ls, c, ts))| (id, ds, ls, c, ts))
            .collect()
    }

    pub fn expand_from_seeds_with_clusters(
        &self,
        seed_results: &[(TetraId, f64, f64, MemoryPayload)],
        depth: usize,
    ) -> Vec<(TetraId, f64, f64, Vec<String>, String, i64)> {
        let mut collected: HashMap<u64, (f64, Vec<String>, String, i64, f64)> = HashMap::new();
        for (id, sim, _mass, payload) in seed_results {
            collected.insert(
                *id,
                (
                    *sim,
                    payload.labels.clone(),
                    payload.content.clone(),
                    payload.timestamp,
                    0.0,
                ),
            );
        }

        let mut frontier: Vec<(u64, usize, f64)> = seed_results
            .iter()
            .map(|(id, sim, _, _)| (*id, 0, *sim))
            .collect();
        let mut visited: HashSet<u64> = seed_results.iter().map(|(id, _, _, _)| *id).collect();

        let clusters = self.space.find_clusters();
        let cluster_map: HashMap<u64, usize> = clusters
            .iter()
            .enumerate()
            .flat_map(|(ci, c)| c.tetra_ids.iter().map(move |&id| (id, ci)))
            .collect();

        let max_expand = 30;
        let mut expanded = 0;
        while let Some((current_id, d, inherited_sim)) = frontier.pop() {
            if expanded >= max_expand {
                break;
            }
            if d >= depth {
                continue;
            }
            for (target_id, _rel_type, strength) in self.get_relations(current_id) {
                if visited.contains(&target_id) {
                    if let Some(entry) = collected.get_mut(&target_id) {
                        let new_assoc = inherited_sim.max(strength);
                        if new_assoc > entry.4 {
                            entry.4 = new_assoc;
                        }
                    }
                    continue;
                }
                visited.insert(target_id);
                if let Some(payload) = self.get_node(target_id) {
                    let assoc = inherited_sim.max(strength);
                    collected.insert(
                        target_id,
                        (
                            0.0,
                            payload.labels,
                            payload.content,
                            payload.timestamp,
                            assoc,
                        ),
                    );
                    frontier.push((target_id, d + 1, assoc));
                    expanded += 1;
                }
            }

            if let Some(&ci) = cluster_map.get(&current_id) {
                if d + 1 < depth && ci < clusters.len() {
                    for &nid in &clusters[ci].tetra_ids {
                        if nid != current_id && !visited.contains(&nid) && expanded < max_expand {
                            visited.insert(nid);
                            if let Some(p) = self.get_node(nid) {
                                let cs = inherited_sim * 0.5;
                                collected.insert(nid, (0.0, p.labels, p.content, p.timestamp, cs));
                                frontier.push((nid, d + 1, cs));
                                expanded += 1;
                            }
                        }
                    }
                }
            }
        }
        collected
            .into_iter()
            .map(|(id, (ds, ls, c, ts, a))| (id, ds, a, ls, c, ts))
            .collect()
    }

    pub fn get_relations(&self, id: TetraId) -> Vec<(TetraId, String, f64)> {
        self.knowledge
            .query_relations(id)
            .into_iter()
            .map(|(tid, rt, s)| (tid, format!("{}", rt), s))
            .collect()
    }

    pub fn get_concepts(&self) -> Vec<(String, usize)> {
        self.knowledge
            .get_concepts()
            .into_iter()
            .map(|c| (c.label, c.member_count as usize))
            .collect()
    }

    pub fn relation_count_kg(&self) -> usize {
        self.knowledge.relation_count()
    }

    pub fn concept_count_kg(&self) -> usize {
        self.knowledge.concept_count()
    }

    pub fn export_graph(&self) -> super::knowledge::GraphExport {
        self.knowledge.export_graph(&self.space)
    }

    pub fn decay_relations(&self) -> usize {
        self.knowledge.decay_relations()
    }

    pub fn get_top_concepts(&self, limit: usize) -> Vec<(String, u64)> {
        self.knowledge.get_top_concepts(limit)
    }

    pub fn get_node(&self, id: TetraId) -> Option<MemoryPayload> {
        self.space.get_tetrahedron(id).map(|t| t.data)
    }

    pub fn pulse(
        &self,
        origin: TetraId,
        ttl: u32,
    ) -> Result<crate::domain::pulse::PulseResult, String> {
        if !self.energy.consume(PULSE_COST) {
            return Err("insufficient energy".into());
        }
        let result = super::pulse::PulseEngine::send(
            &self.space,
            &self.knowledge,
            super::pulse::PulseType::Neural { temperature: 0.8 },
            origin,
            ttl,
        )?;
        let _ = self.tx.send(EngineEvent::PulseSent { origin, ttl });
        Ok(result)
    }

    pub fn stats(&self) -> SpaceStats {
        SpaceStats {
            tetra_count: self.space.tetra_count(),
            vertex_count: self.space.vertex_count(),
            energy: self.energy.available(),
            clusters: self.space.find_clusters().len(),
        }
    }

    pub fn load_context(&self, limit: usize) -> Vec<(TetraId, f64, String, Vec<String>)> {
        let tetras = self.space.all_tetrahedrons();
        let mut scored: Vec<(TetraId, f64, String, Vec<String>)> = tetras
            .into_iter()
            .filter(|t| t.data.importance >= 0.3)
            .filter(|t| !t.data.labels.iter().any(|l| l == "junk"))
            .map(|t| {
                let score = t.data.importance * (1.0 + (t.mass - 1.0).max(0.0) * 0.1);
                let preview: String = t.data.content.chars().take(200).collect();
                (t.id, score, preview, t.data.labels)
            })
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        scored
    }

    pub fn get_enforced_patterns(&self) -> Vec<(TetraId, String, Vec<String>)> {
        let label_idx = self.index.label_index.lock();
        let ids = label_idx.get("enforced").cloned().unwrap_or_default();
        drop(label_idx);
        ids.into_iter()
            .filter_map(|id| self.space.get_tetrahedron(id).filter(|t| t.data.enforced))
            .map(|t| (t.id, t.data.content, t.data.labels))
            .collect()
    }

    pub fn list_by_labels(&self, labels: &[&str], limit: usize) -> Vec<(TetraId, MemoryPayload)> {
        let label_idx = self.index.label_index.lock();
        let mut seen = HashSet::new();
        let mut results = Vec::new();
        for label in labels {
            if let Some(ids) = label_idx.get(*label) {
                for &id in ids {
                    if seen.insert(id) {
                        if let Some(t) = self.space.get_tetrahedron(id) {
                            results.push((id, t.data.clone()));
                        }
                    }
                }
            }
        }
        drop(label_idx);
        results.sort_by(|a, b| b.1.timestamp.cmp(&a.1.timestamp));
        results.truncate(limit);
        results
    }

    pub fn list_recent(&self, offset: usize, limit: usize) -> Vec<(TetraId, MemoryPayload)> {
        let mut all: Vec<(TetraId, MemoryPayload)> = self
            .space
            .all_tetrahedrons()
            .into_iter()
            .map(|t| (t.id, t.data))
            .collect();
        all.sort_by(|a, b| b.1.timestamp.cmp(&a.1.timestamp));
        all.into_iter().skip(offset).take(limit).collect()
    }

    pub fn list_projects(&self) -> Vec<(String, usize)> {
        let label_idx = self.index.label_index.lock();
        let mut projects: Vec<(String, usize)> = label_idx
            .iter()
            .filter(|(label, _)| label.starts_with("project:"))
            .map(|(label, ids)| (label.clone(), ids.len()))
            .collect();
        projects.sort_by(|a, b| b.1.cmp(&a.1));
        projects
    }

    pub fn search_metrics(&self) -> SearchMetrics {
        let total = self.search.search_total.load(AtomicOrdering::Relaxed);
        let hits = self.search.search_hits.load(AtomicOrdering::Relaxed);
        let miss_queries = self.search.search_miss_queries.lock().clone();
        let top_labels: Vec<(String, u32)> = {
            let mut v: Vec<_> = self
                .search
                .search_top_labels
                .lock()
                .iter()
                .map(|(k, &v)| (k.clone(), v))
                .collect();
            v.sort_by(|a, b| b.1.cmp(&a.1));
            v.truncate(10);
            v
        };
        let hot_memories: Vec<(TetraId, u32)> = {
            let mut v: Vec<_> = self
                .search
                .access_counts
                .lock()
                .iter()
                .map(|(&k, &v)| (k, v))
                .collect();
            v.sort_by(|a, b| b.1.cmp(&a.1));
            v.truncate(10);
            v
        };
        SearchMetrics {
            total,
            hits,
            miss_queries,
            top_labels,
            hot_memories,
        }
    }

    pub fn list_nodes(&self) -> Vec<(TetraId, MemoryPayload)> {
        self.space
            .all_tetrahedrons()
            .into_iter()
            .map(|t| (t.id, t.data))
            .collect()
    }

    pub fn update_label_index(&self, id: TetraId, old_labels: &[String], new_labels: &[String]) {
        self.index.update_label_index(id, old_labels, new_labels);
    }

    pub fn remove_from_label_index(&self, id: TetraId, labels: &[String]) {
        self.index.remove_from_label_index(id, labels);
    }

    pub fn remove_from_hnsw(&self, id: TetraId) {
        self.search.hnsw.lock().remove(id);
    }

    pub fn remove_from_content_hash(&self, id: TetraId) {
        self.index.remove_from_content_hash(id);
    }

    pub fn on_tetra_removed(&self, id: TetraId, labels: &[String]) {
        self.search.hnsw.lock().remove(id);
        self.index.remove_from_label_index(id, labels);
        self.index.remove_from_content_hash(id);
        self.index.remove_dirty(id);
        self.index.invalidate_placement_cache();
        self.search.invalidate_df_cache();
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SpaceStats {
    pub tetra_count: usize,
    pub vertex_count: usize,
    pub energy: f64,
    pub clusters: usize,
}

#[derive(Debug, Clone)]
pub struct SearchMetrics {
    pub total: u64,
    pub hits: u64,
    pub miss_queries: Vec<String>,
    pub top_labels: Vec<(String, u32)>,
    pub hot_memories: Vec<(TetraId, u32)>,
}
