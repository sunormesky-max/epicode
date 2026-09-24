use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering as AtomicOrdering;
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::domain::space::Space;
use crate::domain::tetra::{MemoryPayload, TetraId};
use crate::domain::vertex::Point3;

use super::bus::{EngineEvent, EventSender};
use super::classifier::CategoryClassifier;
use super::cognitive::CognitiveEngine;
use super::embedding::EmbeddingService;
use super::energy::{EnergyCenter, CREATE_COST, PULSE_COST};
use super::hnsw::HnswIndex;
use super::index_manager::IndexManager;
use super::knowledge::KnowledgeGraph;
use super::layer_pipeline::{
    audit_to_string, memorialize_security_event, LayerPipeline, RequestContext,
};
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
    pipeline: LayerPipeline,
}

impl GatewayCenter {
    #[allow(clippy::too_many_arguments)]
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
        Self {
            space: space.clone(),
            energy,
            cognitive,
            classifier,
            embedding,
            vector,
            tx,
            knowledge,
            search: SearchEngineState::new(hnsw),
            index: IndexManager::new(label_idx, chash_idx),
            pipeline: LayerPipeline::new(space),
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

    pub fn vector_clone(&self) -> Option<Arc<VectorLayer>> {
        self.vector.clone()
    }

    /// L1相2d: 批量预热嵌入缓存 — 键与compute_embedding一致(strip_meta_prefix后的clean文本)
    pub fn prewarm_embeddings(&self, texts: &[String]) -> usize {
        let cleans: Vec<String> = texts.iter().map(|t| Self::strip_meta_prefix(t)).collect();
        if let Some(ref vl) = self.vector {
            match vl.embed_batch(&cleans) {
                Ok(n) => {
                    if n > 0 {
                        tracing::info!(
                            "[Gateway] prewarm embeddings: {}/{} warmed",
                            n,
                            texts.len()
                        );
                    }
                    n
                }
                Err(e) => {
                    tracing::warn!("[Gateway] prewarm failed: {}", e);
                    0
                }
            }
        } else {
            0
        }
    }

    fn compute_embedding(&self, text: &str) -> Vec<f64> {
        let clean = Self::strip_meta_prefix(text);
        if let Some(ref vl) = self.vector {
            match vl.embed_passage(&clean) {
                Ok(emb) => return emb,
                Err(e) => tracing::warn!("[Gateway] ONNX embed failed: {}", e),
            }
        }
        if self.embedding.enabled() {
            match self.embedding.embed(&clean) {
                Ok(emb) => return emb,
                Err(e) => tracing::warn!("[Gateway] HTTP embed failed: {}", e),
            }
        }
        vec![]
    }

    pub fn strip_meta_prefix(text: &str) -> String {
        let trimmed = text.trim_start();
        if !trimmed.starts_with('[') {
            return text.to_string();
        }
        if let Some(end) = trimmed.find(']') {
            let bracket = &trimmed[..end + 1];
            if bracket.starts_with("[session_")
                || bracket.starts_with("[session|")
                || bracket.starts_with("[finding]")
                || bracket.starts_with("[decision]")
                || bracket.starts_with("[pattern]")
                || bracket.starts_with("[preference]")
            {
                let after = trimmed[end + 1..].trim_start();
                return after.to_string();
            }
            if bracket.contains("|")
                && (bracket.contains("am") || bracket.contains("pm"))
                && bracket.len() < 80
            {
                let after = trimmed[end + 1..].trim_start();
                if !after.is_empty() {
                    return after.to_string();
                }
            }
        }
        text.to_string()
    }

    pub fn create_memory(
        &self,
        content: &str,
        labels: Vec<String>,
    ) -> Result<CreateOutcome, String> {
        self.create_memory_with_time(content, labels, 0)
    }

    pub fn create_memory_with_time(
        &self,
        content: &str,
        labels: Vec<String>,
        timestamp: i64,
    ) -> Result<CreateOutcome, String> {
        if !self.energy.consume(CREATE_COST) {
            return Err("insufficient energy".into());
        }

        let mut ctx = RequestContext::new_create(content, labels);
        let decision = self.pipeline.process_create(&mut ctx);
        if !decision.allowed {
            self.energy.replenish(CREATE_COST);
            tracing::warn!(
                "[Gateway] request denied by pipeline: {} | trail: {}",
                decision.reason,
                audit_to_string(&ctx)
            );
            memorialize_security_event(
                &self.space,
                "create",
                &decision.reason,
                &audit_to_string(&ctx),
            );
            return Err(decision.reason);
        }

        let content = &ctx.content;
        let mut labels = std::mem::take(&mut ctx.labels);
        for boost in &decision.boost_labels {
            if !labels.iter().any(|l| l == boost) {
                labels.push(boost.clone());
            }
        }

        let content_hash = super::search_engine::hash_content(content);

        {
            if let Some(existing_id) = self.index.check_content_hash(content_hash) {
                if let Some(t) = self.space.get_tetrahedron(existing_id) {
                    if t.data.content == content.as_str() {
                        tracing::info!(
                            "duplicate detected (hash index), returning existing tetra {}",
                            t.id
                        );
                        self.energy.replenish(CREATE_COST);
                        let rels = self.knowledge.query_relations(t.id).len();
                        return Ok(CreateOutcome {
                            id: t.id,
                            is_new: false,
                            placement: None,
                            relations_formed: rels,
                        });
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
        let placement = self.find_best_placement(&labels, layer);
        let core = Point3::new(placement.core[0], placement.core[1], placement.core[2]);
        let has_port = placement.has_port;

        let embedding = self.compute_embedding(content);
        tracing::debug!(
            "[Gateway] embedding result: {} dims (vector={}, embed_svc={})",
            embedding.len(),
            self.vector.is_some(),
            self.embedding.enabled()
        );

        // ── 突破2 + 能力C: Mem0 记忆调和（ADD/UPDATE/DELETE/NOOP 四操作）──
        // 设计来自大卫#1189 + Mem0 两阶段调和。embedding 算完后、插入前，做 top-1 相似度检测。
        // 四操作决策：
        //   sim > 0.92  → DELETE（supersede 旧记忆：标失效+降权，新记忆照常创建）
        //   0.75-0.92   → UPDATE（旧记忆补充新标签 + 矛盾检测，新记忆照常创建）
        //   < 0.75      → NOOP（纯新增 ADD，无调和）
        if embedding.len() == EMBEDDING_DIM {
            let hnsw = self.search.hnsw.read();
            let neighbors = hnsw.search_knn(&embedding, 1, 50);
            if let Some((dup_id, dup_sim)) = neighbors.first() {
                let sim = *dup_sim;
                // D1: 对话类内容(轮次前缀/对话标签)用更高supersede阈值 —
                // 知识陈述近重复=该替换, 对话轮同话题≠重复(曾致87%写入被连环吞噬)
                let is_dialogue = content.starts_with("[user]")
                    || content.starts_with("[assistant]")
                    || labels
                        .iter()
                        .any(|l| l == "lme" || l == "chat" || l == "dialogue");
                let supersede_at = if is_dialogue {
                    super::adaptive::MEM0_SUPERSEDE_DIALOGUE
                } else {
                    super::adaptive::MEM0_SUPERSEDE_KNOWLEDGE
                };
                if sim > supersede_at {
                    // DELETE 操作：高相似度 → supersede 旧记忆
                    tracing::info!(
                        "[Gateway] Mem0 DELETE: new {:.3} similar to tetra {}, superseding old",
                        sim,
                        dup_id
                    );
                    if let Some(mut old_tetra) = self.space.get_tetrahedron(*dup_id) {
                        if !old_tetra.data.labels.iter().any(|l| l == "superseded") {
                            old_tetra.data.labels.push("superseded".into());
                        }
                        old_tetra.data.importance = (old_tetra.data.importance * 0.15).max(0.01);
                        old_tetra.data.valid_to = Some(ts);
                        // 能力B：双时序——记录失效得知时间
                        old_tetra.data.invalidated_at = Some(ts);
                        let _ = self.space.update_payload(*dup_id, old_tetra.data);
                        // S2修复: Mem0 调和的旧记忆进脏集, auto_save 持久化 (重启不回滚)
                        self.mark_dirty(*dup_id);
                    }
                } else if sim > 0.75 {
                    // UPDATE 操作：中等相似度 → 旧记忆补充新标签（Mem0 调和）
                    if let Some(mut old_tetra) = self.space.get_tetrahedron(*dup_id) {
                        let mut added = Vec::new();
                        for l in &labels {
                            if !old_tetra.data.labels.contains(l)
                                && !l.starts_with("meta-")
                                && old_tetra.data.labels.len() < 15
                            {
                                old_tetra.data.labels.push(l.clone());
                                added.push(l.clone());
                            }
                        }
                        if !added.is_empty() {
                            tracing::info!("[Gateway] Mem0 UPDATE: tetra {} enriched with labels {:?} (sim={:.3})", dup_id, added, sim);
                            let _ = self.space.update_payload(*dup_id, old_tetra.data);
                            // S2修复: 标签富集同样持久化
                            self.mark_dirty(*dup_id);
                        }
                    }
                }
                // sim < 0.75: NOOP（纯新增）
            }
        }

        let importance = if let Some(imp) = decision.modified_importance {
            imp
        } else {
            Self::compute_importance(content, &labels)
        };
        let positions = crate::domain::tetra::Tetrahedron::compute_vertices(core);
        let identity_stamp = ctx.identity_hash.clone();
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
            identity_stamp,
            source_agent: ctx.source_agent.clone(),
            valid_from: ts,
            valid_to: None,
            expired_at: None,
            invalidated_at: None,
            memory_class: None,
            last_reviewed_ts: None,
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
                    // 精确按 vid 连接（kimi2.7 #1：替代 sentinel 匹配，防并发泄漏）
                    if let Some(vid) = placement.port_vid {
                        self.space.assign_specific_port(vid, id);
                    } else {
                        self.space.reassign_cylinder_port(Self::PORT_SENTINEL, id);
                    }
                }
                {
                    let t = self.space.get_tetrahedron(id);
                    if let Some(t) = &t {
                        if !t.data.embedding.is_empty() && t.data.embedding.len() == EMBEDDING_DIM {
                            self.search
                                .hnsw
                                .write()
                                .insert(id, t.data.embedding.clone());
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
                let relations_formed = self.knowledge.query_relations(id).len();
                tracing::info!(
                    "memory created: tetra {} hash={} rels={}",
                    id,
                    content_hash,
                    relations_formed
                );
                Ok(CreateOutcome {
                    id,
                    is_new: true,
                    placement: Some(placement),
                    relations_formed,
                })
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

        score.clamp(0.1, 3.0)
    }

    fn find_best_placement(
        &self,
        labels: &[String],
        layer: crate::domain::cylinder::CylinderLayer,
    ) -> PlacementOutcome {
        let mk = |core: Point3, has_port: bool, is_seed: bool, is_orphan: bool| {
            let verts = crate::domain::tetra::Tetrahedron::compute_vertices(core);
            PlacementOutcome {
                layer: layer.as_str(),
                core: [core.x, core.y, core.z],
                has_port,
                is_seed,
                is_orphan,
                vertices_shared: self.space.count_vertex_merges(&verts) as usize,
                port_vid: None,
            }
        };
        if let Some(pos) = self.index.get_cached_placement(labels) {
            return mk(pos, false, false, false);
        }

        let tetras = self.space.all_tetrahedrons();
        let zone = self.space.zone_for_layer(layer);

        let in_layer: Vec<crate::domain::tetra::Tetrahedron> = tetras
            .iter()
            .filter(|t| zone.contains_z(t.core.z))
            .cloned()
            .collect();

        let clamp_z = |mut p: Point3| -> Point3 {
            p.z = p.z.max(zone.z_min + 0.1).min(zone.z_max - 0.1);
            p
        };

        if !in_layer.is_empty() {
            use rand::Rng;
            let orphan_roll: f64 = rand::thread_rng().gen();
            if orphan_roll < 0.07 {
                let z = zone.center_z();
                use rand::Rng;
                let mut rng = rand::thread_rng();
                let angle: f64 = rng.gen_range(0.0..std::f64::consts::TAU);
                let dist: f64 = rng.gen_range(8.0..15.0);
                let orphan_pos = Point3::new(angle.cos() * dist, angle.sin() * dist, z);
                let result = clamp_z(self.find_adjacent_position(orphan_pos, &in_layer));
                self.index.cache_placement(labels, result);
                tracing::debug!(
                    "[Gateway] orphan placement at ({:.1},{:.1},{:.1})",
                    result.x,
                    result.y,
                    result.z
                );
                return mk(result, false, false, true);
            }
        }

        if in_layer.is_empty() {
            let z = zone.center_z();
            let port_opt = self.space.assign_cylinder_port(layer, Self::PORT_SENTINEL);
            let port_vid = port_opt.map(|(vid, _)| vid);
            let anchor = if let Some((_vid, pos)) = port_opt {
                pos
            } else {
                Point3::new(0.0, 0.0, z)
            };
            let result = clamp_z(self.find_adjacent_position(anchor, &in_layer));
            self.index.cache_placement(labels, result);
            let mut outcome = mk(result, port_opt.is_some(), true, false);
            outcome.port_vid = port_vid; // 精确 port vid（kimi2.7 #1）
            return outcome;
        }

        let anchor_center = self.find_anchor_by_labels(labels, &in_layer);
        let anchor_verts = crate::domain::tetra::Tetrahedron::compute_vertices(anchor_center);

        let mut best_result = clamp_z(self.find_adjacent_position(anchor_verts[0], &in_layer));
        let mut best_merges = {
            let v = crate::domain::tetra::Tetrahedron::compute_vertices(best_result);
            self.space.count_vertex_merges(&v)
        };
        for &av in &anchor_verts[1..] {
            let candidate = clamp_z(self.find_adjacent_position(av, &in_layer));
            let cv = crate::domain::tetra::Tetrahedron::compute_vertices(candidate);
            let merges = self.space.count_vertex_merges(&cv);
            if merges > best_merges {
                best_merges = merges;
                best_result = candidate;
            }
        }

        self.index.cache_placement(labels, best_result);
        // Joining an existing cluster: the new tetra shares vertices with the anchor and is
        // reachable from the cluster's seed-port via BFS — it does NOT need its own port.
        // One-port-per-polyhedron keeps Port count ≈ cluster count (avoids per-tetra port
        // exhaustion + keeps Port = cluster handle, not a per-tetra admin flag).
        mk(best_result, false, false, false)
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
        let offsets = crate::domain::tetra::Tetrahedron::compute_vertices(Point3::zero());

        let mut best_pos = None;
        let mut best_merges = -1i32;

        for offset in &offsets {
            let center = Point3::new(
                anchor.x - offset.x,
                anchor.y - offset.y,
                anchor.z - offset.z,
            );
            let verts = crate::domain::tetra::Tetrahedron::compute_vertices(center);
            let merges = self.space.count_vertex_merges(&verts);

            if merges > best_merges {
                best_merges = merges;
                best_pos = Some(center);
            }
        }

        best_pos.unwrap_or(anchor)
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
        let mut ctx = RequestContext::new_search(query);
        let decision = self.pipeline.process_search(&mut ctx);
        if !decision.allowed {
            tracing::warn!(
                "[Gateway] search denied by pipeline: {} | trail: {}",
                decision.reason,
                audit_to_string(&ctx)
            );
            return Err(decision.reason);
        }

        let ctx_s = SearchCtx {
            state: &self.search,
            space: &self.space,
            knowledge: &self.knowledge,
            cognitive: &self.cognitive,
            embedding: &self.embedding,
            label_index: &self.index.label_index,
        };
        super::search_engine::search(&ctx_s, query, k, self.vector.as_deref(), filters)
    }

    /// Phase 1 检索可信度重建: 带模式的搜索(返回 5-tuple 含 matched_by)
    ///
    /// mode 从 filters 里提取(未传 filters 时默认 Hybrid)。
    /// exact 模式走 search_exact 纯 BM25 通道, 其他模式走老 search()。
    /// L1相2b: 创建路径专用去重检索 — 纯向量topK(HNSW knn+过期过滤), 不走BM25/KG全混合管线。
    /// 每条写入的内嵌去重不该花1.2s混合检索; 精确重复已有content_hash索引拦截。
    pub fn search_vector_only(
        &self,
        query: &str,
        k: usize,
    ) -> Result<Vec<(TetraId, f64, f64, MemoryPayload)>, String> {
        let mut ctx = RequestContext::new_search(query);
        let decision = self.pipeline.process_search(&mut ctx);
        if !decision.allowed {
            return Err(decision.reason);
        }
        let embedding = self.compute_embedding(query);
        if embedding.len() != EMBEDDING_DIM {
            return Ok(Vec::new());
        }
        let now = chrono::Utc::now().timestamp();
        let hnsw = self.search.hnsw.read();
        let knn = hnsw.search_knn(&embedding, k, 50);
        let mut out = Vec::with_capacity(knn.len());
        for (id, sim) in knn {
            if let Some(t) = self.space.get_tetrahedron(id) {
                let expired = t.data.valid_to.is_some_and(|v| v <= now);
                let superseded = t.data.labels.iter().any(|l| l == "superseded");
                if !expired && !superseded {
                    out.push((id, sim, 0.0, t.data));
                }
            }
        }
        Ok(out)
    }

    pub fn search_filtered_with_mode(
        &self,
        query: &str,
        k: usize,
        filters: Option<&super::search_engine::SearchFilters>,
    ) -> Result<Vec<super::search_engine::ScoredWithMatch>, String> {
        let mut ctx = RequestContext::new_search(query);
        let decision = self.pipeline.process_search(&mut ctx);
        if !decision.allowed {
            tracing::warn!(
                "[Gateway] search denied by pipeline: {} | trail: {}",
                decision.reason,
                audit_to_string(&ctx)
            );
            return Err(decision.reason);
        }

        let ctx_s = SearchCtx {
            state: &self.search,
            space: &self.space,
            knowledge: &self.knowledge,
            cognitive: &self.cognitive,
            embedding: &self.embedding,
            label_index: &self.index.label_index,
        };
        let mode = filters.map(|f| f.mode).unwrap_or_default();
        super::search_engine::search_with_mode(
            &ctx_s,
            query,
            k,
            self.vector.as_deref(),
            filters,
            mode,
        )
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
        clusters: &[crate::domain::space::Cluster],
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

        let cluster_map: HashMap<u64, usize> = clusters
            .iter()
            .enumerate()
            .flat_map(|(ci, c)| c.tetra_ids.iter().map(move |&id| (id, ci)))
            .collect();

        let max_expand = 80;
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

    pub fn export_graph(&self, node_limit: usize) -> super::knowledge::GraphExport {
        self.knowledge.export_graph(&self.space, node_limit)
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
        results.sort_by_key(|b| std::cmp::Reverse(b.1.timestamp));
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
        all.sort_by_key(|b| std::cmp::Reverse(b.1.timestamp));
        all.into_iter().skip(offset).take(limit).collect()
    }

    pub fn list_projects(&self) -> Vec<(String, usize)> {
        let label_idx = self.index.label_index.lock();
        let mut projects: Vec<(String, usize)> = label_idx
            .iter()
            .filter(|(label, _)| label.starts_with("project:"))
            .map(|(label, ids)| (label.clone(), ids.len()))
            .collect();
        projects.sort_by_key(|b| std::cmp::Reverse(b.1));
        projects
    }

    pub fn access_counts_snapshot(&self) -> Vec<(TetraId, u32)> {
        self.search.access_counts_snapshot()
    }

    /// 清空 session access_counts（flush 后调用，防止重启后覆写历史值）
    pub fn reset_session_access_counts(&self) {
        self.search.access_counts.lock().clear();
    }

    pub fn search_metrics(&self) -> SearchMetrics {
        let total = self.search.search_total.load(AtomicOrdering::Relaxed);
        let hits = self.search.search_hits.load(AtomicOrdering::Relaxed);
        let miss_queries: Vec<String> = self
            .search
            .search_miss_queries
            .lock()
            .iter()
            .cloned()
            .collect();
        let top_labels: Vec<(String, u32)> = {
            let mut v: Vec<_> = self
                .search
                .search_top_labels
                .lock()
                .iter()
                .map(|(k, &v)| (k.clone(), v))
                .collect();
            v.sort_by_key(|b| std::cmp::Reverse(b.1));
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
            v.sort_by_key(|b| std::cmp::Reverse(b.1));
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
        self.search.hnsw.write().remove(id);
    }

    pub fn remove_from_content_hash(&self, id: TetraId) {
        self.index.remove_from_content_hash(id);
    }

    pub fn update_content(&self, id: TetraId, new_content: &str) -> Result<(), String> {
        let existing = self
            .space
            .get_tetrahedron(id)
            .ok_or_else(|| format!("tetrahedron {} not found", id))?;

        let new_hash = super::search_engine::hash_content(new_content);
        let new_embedding = self.compute_embedding(new_content);

        self.search.hnsw.write().remove(id);
        self.index.remove_from_content_hash(id);

        let mut new_payload = existing.data.clone();
        new_payload.content = new_content.to_string();
        new_payload.content_hash = new_hash;
        new_payload.embedding = new_embedding.clone();
        self.space.update_payload(id, new_payload)?;

        if !new_embedding.is_empty() && new_embedding.len() == EMBEDDING_DIM {
            self.search.hnsw.write().insert(id, new_embedding);
        }
        self.index.insert_content_hash(new_hash, id);

        self.search.invalidate_df_cache();
        self.index.invalidate_placement_cache();

        tracing::info!("[Gateway] content updated: tetra {}", id);
        Ok(())
    }

    pub fn on_tetra_removed(&self, id: TetraId, labels: &[String]) {
        self.search.hnsw.write().remove(id);
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

/// SMRP §7.2 — 安置副产物：一条新记忆"住进了社区的什么位置"。
#[derive(Debug, Clone, serde::Serialize)]
pub struct PlacementOutcome {
    pub layer: &'static str,
    pub core: [f64; 3],
    pub has_port: bool,
    pub is_seed: bool,
    pub is_orphan: bool,
    pub vertices_shared: usize,
    /// 领取的 Port vertex id（kimi2.7 #1：精确 reassign 而非 sentinel 匹配）
    pub port_vid: Option<u64>,
}

/// SMRP §7.2 — 创建结果（含安置 + 建链计数）。
/// gateway 层填 placement/relations_formed；scheduler 层补 intake/classify/dedup/conflict。
#[derive(Debug, Clone, serde::Serialize)]
pub struct CreateOutcome {
    pub id: TetraId,
    pub is_new: bool,
    pub placement: Option<PlacementOutcome>,
    pub relations_formed: usize,
}

#[derive(Debug, Clone)]
pub struct SearchMetrics {
    pub total: u64,
    pub hits: u64,
    pub miss_queries: Vec<String>,
    pub top_labels: Vec<(String, u32)>,
    pub hot_memories: Vec<(TetraId, u32)>,
}
