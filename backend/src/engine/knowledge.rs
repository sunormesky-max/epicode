use parking_lot::Mutex;
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::domain::space::Space;
use crate::domain::tetra::TetraId;
use crate::engine::vector::VectorLayer;

const DECAY_FACTOR: f64 = 0.9995;
const MIN_STRENGTH: f64 = 0.05;
const MAX_RELATIONS_PER_NODE: usize = 50;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Relation {
    pub source: TetraId,
    pub target: TetraId,
    pub relation_type: RelationType,
    pub strength: f64,
    pub created_tick: u64,
    /// 检索强化计数(记忆巩固/testing effect): 被检索路径(PPR扩散/multi_hop扩展)
    /// 走过的边累积命中。均匀衰减下高命中"主干道"被持续推回高强度——
    /// 与时间效性P系列(earned importance)同构: 使用即续命, 不用即流失。
    #[serde(default)]
    pub hits: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RelationType {
    SimilarTo,
    Contradicts,
    Precedes,
    Contains,
    Related,
    BelongsTo,
    MergedInto,
    /// P3 Agentic GraphRAG: two memories share the same extracted entity
    /// (code symbol, proper noun, identifier). Created by entity-level
    /// schema induction in auto_link_one.
    SameEntity,
}

type RelationKey = (TetraId, TetraId, RelationType);

enum PendingRelationChange {
    Upsert(Relation),
    Delete,
}

#[derive(Default)]
struct PersistenceState {
    revision: u64,
    dirty: bool,
    full_save_required: bool,
}

impl RelationType {
    /// 判别值(去重索引用) — 手写match: 新增variant时编译器强制覆盖(无通配分支)
    pub fn discriminant(&self) -> u8 {
        match self {
            RelationType::SimilarTo => 0,
            RelationType::Contradicts => 1,
            RelationType::Precedes => 2,
            RelationType::Contains => 3,
            RelationType::Related => 4,
            RelationType::BelongsTo => 5,
            RelationType::MergedInto => 6,
            RelationType::SameEntity => 7,
        }
    }
}

impl std::fmt::Display for RelationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RelationType::SimilarTo => write!(f, "similar"),
            RelationType::Contradicts => write!(f, "contradicts"),
            RelationType::Precedes => write!(f, "precedes"),
            RelationType::Contains => write!(f, "contains"),
            RelationType::Related => write!(f, "related"),
            RelationType::BelongsTo => write!(f, "belongs_to"),
            RelationType::MergedInto => write!(f, "merged_into"),
            RelationType::SameEntity => write!(f, "same_entity"),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConceptPrototype {
    pub id: u64,
    pub centroid: Vec<f64>,
    pub member_count: u64,
    pub label: String,
    pub member_ids: Vec<TetraId>,
}

pub struct KnowledgeGraph {
    relations: RwLock<Vec<Relation>>,
    pending_relations: Mutex<HashMap<RelationKey, PendingRelationChange>>,
    /// 加载期抑制: load_relations重放不得灌爆增量队列
    pub loading: std::sync::atomic::AtomicBool,
    adj_index: RwLock<HashMap<TetraId, Vec<usize>>>,
    /// 查重索引(生产性能修复 2026-09-27): (min(src,tgt),max(src,tgt),rel_type)集合 —
    /// 大账户加载曾因 exists 线性扫描 O(n²): 25万关系=31亿次比较=33分钟冷启动
    rel_dedup: RwLock<HashSet<(TetraId, TetraId, u8)>>,
    concepts: RwLock<Vec<ConceptPrototype>>,
    persistence: Mutex<PersistenceState>,
}

impl Default for KnowledgeGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl KnowledgeGraph {
    pub fn new() -> Self {
        Self {
            relations: RwLock::new(Vec::new()),
            pending_relations: Mutex::new(HashMap::new()),
            loading: std::sync::atomic::AtomicBool::new(false),
            adj_index: RwLock::new(HashMap::new()),
            rel_dedup: RwLock::new(HashSet::new()),
            concepts: RwLock::new(Vec::new()),
            persistence: Mutex::new(PersistenceState::default()),
        }
    }

    /// Take the coalesced final change for each directed edge.
    pub fn drain_pending_relations(
        &self,
    ) -> (Vec<Relation>, Vec<(TetraId, TetraId, RelationType)>) {
        let pending = std::mem::take(&mut *self.pending_relations.lock());
        let mut ups = Vec::new();
        let mut dels = Vec::new();
        for ((source, target, relation_type), change) in pending {
            match change {
                PendingRelationChange::Upsert(relation) => ups.push(relation),
                PendingRelationChange::Delete => dels.push((source, target, relation_type)),
            }
        }
        (ups, dels)
    }
    pub fn set_loading(&self, v: bool) {
        self.loading.store(v, std::sync::atomic::Ordering::Relaxed);
        if !v {
            // 加载结束: 清空加载期误入队的残留
            self.pending_relations.lock().clear();
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.persistence.lock().dirty
    }

    pub fn persistence_snapshot(&self) -> Option<(u64, bool)> {
        let state = self.persistence.lock();
        state
            .dirty
            .then_some((state.revision, state.full_save_required))
    }

    pub fn mark_saved_if_unchanged(&self, revision: u64) -> bool {
        let mut state = self.persistence.lock();
        if state.revision != revision {
            return false;
        }
        state.dirty = false;
        state.full_save_required = false;
        true
    }

    fn mark_dirty(&self, full_save_required: bool) {
        let mut state = self.persistence.lock();
        state.revision = state.revision.wrapping_add(1);
        state.dirty = true;
        state.full_save_required |= full_save_required;
    }

    fn queue_relation_upsert(&self, relation: Relation) {
        let key = (
            relation.source,
            relation.target,
            relation.relation_type.clone(),
        );
        self.pending_relations
            .lock()
            .insert(key, PendingRelationChange::Upsert(relation));
    }

    fn queue_relation_delete(&self, source: TetraId, target: TetraId, relation_type: RelationType) {
        self.pending_relations.lock().insert(
            (source, target, relation_type),
            PendingRelationChange::Delete,
        );
    }

    fn rebuild_rel_dedup(&self, relations: &[Relation]) -> HashSet<(TetraId, TetraId, u8)> {
        let mut set = HashSet::with_capacity(relations.len() * 2);
        for r in relations {
            let rt = r.relation_type.discriminant();
            set.insert((r.source, r.target, rt));
            if r.relation_type != RelationType::BelongsTo
                && r.relation_type != RelationType::MergedInto
            {
                set.insert((r.target, r.source, rt));
            }
        }
        set
    }
    fn rebuild_adj_index(&self, relations: &[Relation]) -> HashMap<TetraId, Vec<usize>> {
        let mut idx: HashMap<TetraId, Vec<usize>> = HashMap::new();
        for (i, r) in relations.iter().enumerate() {
            idx.entry(r.source).or_default().push(i);
            idx.entry(r.target).or_default().push(i);
        }
        idx
    }

    pub fn add_relation(
        &self,
        source: TetraId,
        target: TetraId,
        rel_type: RelationType,
        strength: f64,
    ) {
        self.add_relation_at(source, target, rel_type, strength, 0);
    }

    pub fn add_relation_at(
        &self,
        source: TetraId,
        target: TetraId,
        rel_type: RelationType,
        strength: f64,
        tick: u64,
    ) {
        let mut relations = self.relations.write();
        // 归属关系(BelongsTo/MergedInto)做单向去重——只检查相同方向
        // 其他关系(similar/contradicts等)做双向去重
        // 性能修复(2026-09-27): 原线性扫描 O(n) → 去重索引 O(1);
        // 大账户(25万关系)冷启动从33分钟降回秒级
        let rt = rel_type.discriminant();
        let (key_fwd, key_rev) =
            if rel_type == RelationType::BelongsTo || rel_type == RelationType::MergedInto {
                ((source, target, rt), (source, target, rt)) // 单向: 逆键不用
            } else {
                ((source, target, rt), (target, source, rt))
            };
        let exists = {
            let dedup = self.rel_dedup.read();
            dedup.contains(&key_fwd) || (key_fwd != key_rev && dedup.contains(&key_rev))
        };
        if exists {
            return;
        }
        {
            let adj = self.adj_index.read();
            let src_count = adj.get(&source).map(|v| v.len()).unwrap_or(0);
            let tgt_count = adj.get(&target).map(|v| v.len()).unwrap_or(0);
            // 归属关系不受数量限制（否则高连接度的旧节点无法加入档案库层级）
            let is_structural =
                rel_type == RelationType::BelongsTo || rel_type == RelationType::MergedInto;
            if !is_structural
                && (src_count >= MAX_RELATIONS_PER_NODE || tgt_count >= MAX_RELATIONS_PER_NODE)
            {
                return;
            }
        }
        let new_rel = Relation {
            source,
            target,
            relation_type: rel_type,
            strength,
            created_tick: tick,
            hits: 0,
        };
        relations.push(new_rel.clone());
        {
            let mut dedup = self.rel_dedup.write();
            dedup.insert(key_fwd);
            if key_fwd != key_rev {
                dedup.insert(key_rev);
            }
        }
        // 增量更新邻接索引(原全量rebuild O(n) — 25万关系时每次插入都重建=又一处平方)
        {
            let mut adj = self.adj_index.write();
            let idx = relations.len() - 1;
            adj.entry(source).or_default().push(idx);
            adj.entry(target).or_default().push(idx);
        }
        if !self.loading.load(std::sync::atomic::Ordering::Relaxed) {
            self.queue_relation_upsert(new_rel);
        }
        self.mark_dirty(false);
    }

    pub fn remove_relations_for(&self, id: TetraId) {
        let mut relations = self.relations.write();
        let before = relations.len();
        let removed: Vec<(TetraId, TetraId, RelationType)> = relations
            .iter()
            .filter(|r| r.source == id || r.target == id)
            .map(|r| (r.source, r.target, r.relation_type.clone()))
            .collect();
        relations.retain(|r| r.source != id && r.target != id);
        if relations.len() < before {
            *self.adj_index.write() = self.rebuild_adj_index(&relations);
            *self.rel_dedup.write() = self.rebuild_rel_dedup(&relations);
            if !self.loading.load(std::sync::atomic::Ordering::Relaxed) {
                for (source, target, relation_type) in removed {
                    self.queue_relation_delete(source, target, relation_type);
                }
            }
            self.mark_dirty(false);
        }
    }

    /// 精确删除：只删 source→target 且类型匹配的关系
    pub fn remove_relation(&self, source: TetraId, target: TetraId, rel_type: RelationType) {
        let mut relations = self.relations.write();
        let before = relations.len();
        relations
            .retain(|r| !(r.source == source && r.target == target && r.relation_type == rel_type));
        if relations.len() < before {
            *self.adj_index.write() = self.rebuild_adj_index(&relations);
            *self.rel_dedup.write() = self.rebuild_rel_dedup(&relations);
            if !self.loading.load(std::sync::atomic::Ordering::Relaxed) {
                self.queue_relation_delete(source, target, rel_type.clone());
            }
            self.mark_dirty(false);
        }
    }

    /// 计算/更新所有 concept 的 centroid（成员 embedding 均值）。
    /// 之前断联：centroid 永远 vec![]，概念聚类只能靠标签 Jaccard。
    pub fn recompute_centroids(&self, space: &crate::domain::space::Space) {
        let mut concepts = self.concepts.write();
        let mut changed = false;
        for c in concepts.iter_mut() {
            if c.member_ids.is_empty() {
                if !c.centroid.is_empty() {
                    c.centroid.clear();
                    changed = true;
                }
                continue;
            }
            let mut sum = vec![0.0_f64; 1024]; // bge-m3 1024 dim
            let mut count = 0usize;
            for &mid in &c.member_ids {
                if let Some(t) = space.get_tetrahedron(mid) {
                    if t.data.embedding.len() == 1024 {
                        for (i, &v) in t.data.embedding.iter().enumerate() {
                            sum[i] += v;
                        }
                        count += 1;
                    }
                }
            }
            if count > 0 {
                let centroid: Vec<f64> = sum.iter().map(|v| v / count as f64).collect();
                if c.centroid != centroid {
                    c.centroid = centroid;
                    changed = true;
                }
            }
        }
        drop(concepts);
        if changed {
            self.mark_dirty(false);
        }
    }

    pub fn query_relations(&self, id: TetraId) -> Vec<(TetraId, RelationType, f64)> {
        let relations = self.relations.read();
        let adj = self.adj_index.read();
        match adj.get(&id) {
            Some(indices) => indices
                .iter()
                .filter_map(|&i| relations.get(i))
                .map(|r| {
                    let other = if r.source == id { r.target } else { r.source };
                    (other, r.relation_type.clone(), r.strength)
                })
                .collect(),
            None => Vec::new(),
        }
    }

    pub fn auto_link(&self, space: &Space, threshold: f64) {
        let tetras = space.all_tetrahedrons();
        for i in 0..tetras.len() {
            for j in (i + 1)..tetras.len() {
                let sim = VectorLayer::best_similarity(
                    &tetras[i].data.embedding,
                    &tetras[i].data.labels,
                    &tetras[j].data.embedding,
                    &tetras[j].data.labels,
                );
                if sim > threshold {
                    self.add_relation(tetras[i].id, tetras[j].id, RelationType::SimilarTo, sim);
                }
            }
        }
    }

    pub fn auto_link_one(
        &self,
        new_id: TetraId,
        space: &Space,
        label_index: &std::collections::HashMap<String, Vec<TetraId>>,
    ) {
        let new_tetra = match space.get_tetrahedron(new_id) {
            Some(t) => t,
            None => return,
        };
        let mut candidate_ids: std::collections::HashSet<TetraId> =
            std::collections::HashSet::new();
        for label in &new_tetra.data.labels {
            if let Some(ids) = label_index.get(label) {
                for &id in ids {
                    if id != new_id {
                        candidate_ids.insert(id);
                    }
                }
            }
        }
        let mut candidates: Vec<_> = candidate_ids
            .iter()
            .filter_map(|&id| space.get_tetrahedron(id))
            .collect();
        if candidates.len() < 5 {
            let all = space.all_tetrahedrons();
            for t in &all {
                if t.id != new_id && !candidates.iter().any(|c| c.id == t.id) {
                    candidates.push(t.clone());
                    if candidates.len() >= 20 {
                        break;
                    }
                }
            }
        } else {
            candidates.truncate(20);
        }
        // P3 Agentic GraphRAG: entity-level schema induction.
        let new_entities = extract_entities(&new_tetra.data.content);
        let new_entity_set: std::collections::HashSet<&str> =
            new_entities.iter().map(|s| s.as_str()).collect();

        for t in &candidates {
            let sim = VectorLayer::best_similarity(
                &new_tetra.data.embedding,
                &new_tetra.data.labels,
                &t.data.embedding,
                &t.data.labels,
            );
            if sim > 0.3 {
                self.add_relation(new_id, t.id, RelationType::SimilarTo, sim);
            }

            // P3: Entity-based linking - shared entities create SameEntity relations
            // even when vector similarity is only moderate. This captures semantic
            // connections that pure vector distance misses (e.g. two memories both
            // referencing the MemoryPayload struct).
            if !new_entities.is_empty() {
                let t_entities = extract_entities(&t.data.content);
                let shared_count = t_entities
                    .iter()
                    .filter(|e| new_entity_set.contains(e.as_str()))
                    .count();
                if shared_count > 0 {
                    let t_set_size = t_entities.len();
                    let union = new_entities.len() + t_set_size - shared_count;
                    let entity_sim = if union > 0 {
                        shared_count as f64 / union as f64
                    } else {
                        0.0
                    };
                    if entity_sim > 0.15 {
                        let strength = (entity_sim * 0.6 + sim * 0.4).min(0.9);
                        if strength > 0.2 {
                            self.add_relation(new_id, t.id, RelationType::SameEntity, strength);
                        }
                    }
                }
            }

            if sim > 0.25 {
                let (older, newer) = if new_tetra.data.timestamp <= t.data.timestamp {
                    (new_id, t.id)
                } else {
                    (t.id, new_id)
                };
                let ts_gap = (new_tetra.data.timestamp - t.data.timestamp).unsigned_abs();
                if ts_gap > 60 {
                    let strength = (0.4 + sim * 0.4).min(0.8);
                    self.add_relation(older, newer, RelationType::Precedes, strength);
                }

                let longer = if new_tetra.data.content.len() >= t.data.content.len() {
                    (&new_tetra.data.content, new_id, t.id)
                } else {
                    (&t.data.content, t.id, new_id)
                };
                if longer.0.len() > 50 {
                    let shorter_content = if new_tetra.data.content.len() >= t.data.content.len() {
                        &t.data.content
                    } else {
                        &new_tetra.data.content
                    };
                    if shorter_content.len() > 20 && longer.0.contains(&shorter_content[..]) {
                        self.add_relation(longer.1, longer.2, RelationType::Contains, sim * 0.7);
                    }
                }

                // 能力A：A-MEM 记忆进化 — 高相似度时，新记忆的独有标签回流到历史记忆。
                // 这让历史记忆随着系统积累变得更丰富（A-MEM 论文的核心机制）。
                // 只对高相似度(>0.7)且新记忆更"新"的情况触发，避免标签膨胀。
                if sim > 0.7 && new_tetra.data.timestamp > t.data.timestamp {
                    let mut new_labels_to_add: Vec<String> = Vec::new();
                    for label in &new_tetra.data.labels {
                        if !t.data.labels.contains(label)
                            && !label.starts_with("meta-")
                            && !label.starts_with("entity:")
                        {
                            new_labels_to_add.push(label.clone());
                        }
                    }
                    if !new_labels_to_add.is_empty() && t.data.labels.len() < 15 {
                        // 限制每次进化最多补 3 个标签（避免标签爆炸）
                        let to_add: Vec<String> = new_labels_to_add.into_iter().take(3).collect();
                        let mut updated_labels = t.data.labels.clone();
                        updated_labels.extend(to_add);
                        let _ = space.update_labels(t.id, updated_labels);
                        tracing::debug!(
                            "[A-MEM] evolved tetra #{}: added labels from new tetra #{} (sim={:.3})",
                            t.id, new_id, sim
                        );
                    }
                }
            }
        }
    }

    pub fn decay_relations(&self) -> usize {
        let mut relations = self.relations.write();
        let before = relations.len();
        for r in relations.iter_mut() {
            r.strength *= DECAY_FACTOR;
        }
        relations.retain(|r| r.strength >= MIN_STRENGTH);
        let removed = before - relations.len();
        if removed > 0 {
            *self.adj_index.write() = self.rebuild_adj_index(&relations);
            *self.rel_dedup.write() = self.rebuild_rel_dedup(&relations);
        }
        if before > 0 {
            self.mark_dirty(true);
        }
        removed
    }

    pub fn multi_hop(&self, seeds: &[TetraId], max_hops: usize) -> Vec<(TetraId, f64)> {
        let relations = self.relations.read();
        let adj = self.adj_index.read();
        let mut visited: HashSet<TetraId> = seeds.iter().copied().collect();
        let mut scored: HashMap<TetraId, f64> = HashMap::new();
        let mut frontier: Vec<(TetraId, f64)> = seeds.iter().map(|&s| (s, 1.0)).collect();
        // 检索强化: 记录本轮真正走过的关系(GraphRAG local expansion的主干道)
        let mut traversed: Vec<usize> = Vec::new();

        for _hop in 0..max_hops {
            let mut next_frontier = Vec::new();
            for &(current, accumulated) in &frontier {
                if let Some(indices) = adj.get(&current) {
                    for &i in indices {
                        let r = match relations.get(i) {
                            Some(r) => r,
                            None => continue,
                        };
                        let neighbor = if r.source == current {
                            r.target
                        } else if r.target == current {
                            r.source
                        } else {
                            continue;
                        };

                        if visited.contains(&neighbor) {
                            continue;
                        }

                        let score = accumulated * r.strength;
                        if score < 0.1 {
                            continue;
                        }

                        traversed.push(i);
                        visited.insert(neighbor);
                        let entry = scored.entry(neighbor).or_insert(0.0);
                        *entry = (*entry).max(score);
                        next_frontier.push((neighbor, score));
                    }
                }
            }
            frontier = next_frontier;
        }
        drop(relations);
        drop(adj);

        // 走过的边回报强化(读锁已释放, reinforce内部自取写锁)
        if !traversed.is_empty() {
            let pairs: Vec<(TetraId, TetraId)> = {
                let relations = self.relations.read();
                traversed
                    .iter()
                    .filter_map(|&i| relations.get(i).map(|r| (r.source, r.target)))
                    .collect()
            };
            self.reinforce_edges(&pairs);
        }

        let mut result: Vec<(TetraId, f64)> = scored.into_iter().collect();
        result.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        result
    }

    /// P3 Agentic GraphRAG: Adaptive multi-hop traversal.
    ///
    /// Dynamically decides traversal depth based on graph density and seed count:
    /// - Few seeds (1-3) + sparse graph (avg_degree < 5): 3 hops for deeper reach
    /// - Dense graph (avg_degree > 20): 1 hop to avoid explosion
    /// - Default: 2 hops (balanced)
    ///
    /// Then trims to max_results.
    pub fn multi_hop_adaptive(&self, seeds: &[TetraId], max_results: usize) -> Vec<(TetraId, f64)> {
        if seeds.is_empty() {
            return Vec::new();
        }

        let adj = self.adj_index.read();
        let total_nodes = adj.len();
        let total_edges: usize = adj.values().map(|v| v.len()).sum();
        let avg_degree = if total_nodes > 0 {
            total_edges as f64 / total_nodes as f64
        } else {
            0.0
        };
        drop(adj);

        let depth = if seeds.len() <= 3 && avg_degree < 5.0 {
            3
        } else if avg_degree > 20.0 {
            1
        } else {
            2
        };

        tracing::debug!(
            "[P3 multi_hop_adaptive] seeds={} avg_degree={:.1} -> depth={} (cap={})",
            seeds.len(),
            avg_degree,
            depth,
            max_results
        );

        let mut results = self.multi_hop(seeds, depth);
        if results.len() > max_results {
            results.truncate(max_results);
        }
        results
    }

    /// 检索强化入口: 把本轮检索实际走过的边(u,v)回报给图谱。
    /// 每条被走的边 strength += 25%·剩余差距(封顶1.0)、hits+1;
    /// 批量加载期间跳过(整批载入后的覆写会吞掉强化)。
    pub fn reinforce_edges(&self, pairs: &[(TetraId, TetraId)]) {
        if pairs.is_empty() || self.loading.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        // 邻接索引定位: 每个端点扫自己的度(有界), 不碰全表
        let mut hits_idx: Vec<usize> = Vec::new();
        {
            let relations = self.relations.read();
            let adj = self.adj_index.read();
            for &(a, b) in pairs {
                for &i in adj.get(&a).into_iter().flatten() {
                    if let Some(r) = relations.get(i) {
                        if (r.source == a && r.target == b) || (r.source == b && r.target == a) {
                            hits_idx.push(i);
                            break;
                        }
                    }
                }
            }
        }
        if hits_idx.is_empty() {
            return;
        }
        let mut updated_relations = Vec::with_capacity(hits_idx.len());
        {
            let mut relations = self.relations.write();
            for i in hits_idx {
                if let Some(r) = relations.get_mut(i) {
                    if r.strength < 1.0 {
                        r.strength += (1.0 - r.strength) * 0.25;
                        if r.strength > 1.0 {
                            r.strength = 1.0;
                        }
                    }
                    r.hits = r.hits.saturating_add(1);
                    updated_relations.push(r.clone());
                }
            }
        }
        if !updated_relations.is_empty() {
            for relation in updated_relations {
                self.queue_relation_upsert(relation);
            }
            self.mark_dirty(false);
        }
    }

    /// 异步标签传播社区发现(Raghavan et al. 2007, 近线性)——GraphRAG式社区层的轻量地基。
    /// 与 space.find_clusters() 的空间聚类互补: 这里是关系边上的拓扑社区。
    /// 确定性保证: 节点按 TetraId 升序遍历, 平票取最小标签, 投票按边 strength 加权。
    pub fn detect_communities(&self, max_rounds: usize) -> HashMap<TetraId, TetraId> {
        let relations = self.relations.read();
        let adj = self.adj_index.read();
        let mut nodes: Vec<TetraId> = adj.keys().copied().collect();
        nodes.sort_unstable();
        let mut labels: HashMap<TetraId, TetraId> = nodes.iter().copied().map(|n| (n, n)).collect();

        for _round in 0..max_rounds {
            let mut changed = false;
            for &n in &nodes {
                let mut votes: HashMap<TetraId, f64> = HashMap::new();
                if let Some(indices) = adj.get(&n) {
                    for &i in indices {
                        let r = match relations.get(i) {
                            Some(r) => r,
                            None => continue,
                        };
                        let nb = if r.source == n {
                            r.target
                        } else if r.target == n {
                            r.source
                        } else {
                            continue;
                        };
                        if let Some(&l) = labels.get(&nb) {
                            *votes.entry(l).or_insert(0.0) += r.strength.max(0.0);
                        }
                    }
                }
                if votes.is_empty() {
                    continue;
                }
                // 加权最高票; 平票取最小标签(确定性, 与遍历序解耦)
                let best = votes
                    .into_iter()
                    .max_by(|a, b| {
                        a.1.partial_cmp(&b.1)
                            .unwrap_or(std::cmp::Ordering::Equal)
                            .then(b.0.cmp(&a.0))
                    })
                    .map(|(l, _)| l)
                    .expect("votes non-empty");
                if labels.get(&n) != Some(&best) {
                    labels.insert(n, best);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        labels
    }

    /// 加权模块度 Q ∈ [-0.5, 1]: 社区结构强度标准度量(Newman)。
    /// Q = Σ_c [ w_in(c)/M - (K(c)/2M)² ], M=总边权, K(c)=社区内节点度权和。
    fn community_modularity(&self, labels: &HashMap<TetraId, TetraId>) -> (usize, usize, f64) {
        let relations = self.relations.read();
        let adj = self.adj_index.read();
        let m_total: f64 = relations.iter().map(|r| r.strength.max(0.0)).sum();
        if m_total <= 0.0 {
            let n = labels.len();
            return (n.max(1), 0, 0.0);
        }
        // 社区索引
        let mut comm_ids: HashMap<TetraId, usize> = HashMap::new();
        for l in labels.values() {
            let next = comm_ids.len();
            comm_ids.entry(*l).or_insert(next);
        }
        let n_comm = comm_ids.len();
        let mut w_in: Vec<f64> = vec![0.0; n_comm];
        let mut k_sum: Vec<f64> = vec![0.0; n_comm];
        let mut sizes: Vec<usize> = vec![0; n_comm];
        for (&n, l) in labels.iter() {
            let c = comm_ids[l];
            sizes[c] += 1;
            if let Some(indices) = adj.get(&n) {
                for &i in indices {
                    let r = match relations.get(i) {
                        Some(r) => r,
                        None => continue,
                    };
                    let w = r.strength.max(0.0);
                    k_sum[c] += w; // 每条边在两端各计一次度
                    if let (Some(&ca), Some(&cb)) = (labels.get(&r.source), labels.get(&r.target)) {
                        if ca == cb {
                            // 无向边只计一次内部权: 按source侧计入
                            if r.source == n {
                                w_in[comm_ids[&ca]] += w;
                            }
                        }
                    }
                }
            }
        }
        let q: f64 = (0..n_comm)
            .map(|c| w_in[c] / m_total - (k_sum[c] / (2.0 * m_total)).powi(2))
            .sum();
        let largest = sizes.iter().copied().max().unwrap_or(0);
        (n_comm, largest, q)
    }

    pub fn update_concepts(&self, tetras: &[(TetraId, Vec<String>)]) {
        // 性能修复(2026-09-28, #100同族): 原实现每个tetra线性扫全部概念并
        // 逐一聚合成员标签算Jaccard → O(T×C×M)。批级倒排索引:
        //   eff_labels[ci] = 概念ci的有效标签集(批次内成员的标签并集)
        //   postings[label] = 拥有该标签的概念idx列表
        // 每个tetra只碰自己的标签命中过的概念 → O(C×M + T×|L|×postings)。
        // 顺序语义与旧实现精确等价(见测试 update_concepts_inverted_matches_reference):
        // 指派/新建会同步更新索引, 模拟旧实现"后面的tetra看到已变异的概念"。
        let mut concepts = self.concepts.write();
        let labels_map: HashMap<TetraId, &Vec<String>> =
            tetras.iter().map(|(id, l)| (*id, l)).collect();

        let mut eff_labels: Vec<HashSet<&str>> = Vec::with_capacity(concepts.len());
        let mut postings: HashMap<&str, Vec<usize>> = HashMap::new();
        for (ci, c) in concepts.iter().enumerate() {
            let mut set: HashSet<&str> = HashSet::new();
            for &mid in &c.member_ids {
                if let Some(ml) = labels_map.get(&mid) {
                    for l in ml.iter() {
                        set.insert(l.as_str());
                    }
                }
            }
            for l in &set {
                postings.entry(*l).or_default().push(ci);
            }
            eff_labels.push(set);
        }
        // 概念ci吸收新标签(指派后), 同步入索引
        // (无捕获, 用内嵌fn统一生命周期参数——闭包的不变性会拒绝混装两个来源的&str)
        fn absorb<'a>(
            ci: usize,
            labels: &'a [String],
            eff_labels: &mut [HashSet<&'a str>],
            postings: &mut HashMap<&'a str, Vec<usize>>,
        ) {
            let target = &mut eff_labels[ci];
            for l in labels {
                let ls = l.as_str();
                if target.insert(ls) {
                    postings.entry(ls).or_default().push(ci);
                }
            }
        }

        for &(id, ref labels) in tetras {
            let label_set: HashSet<&str> = labels.iter().map(|s| s.as_str()).collect();
            // 交集计数: 每个标签在哪些概念的有效集里
            let mut shared: HashMap<usize, usize> = HashMap::new();
            for l in &label_set {
                if let Some(cis) = postings.get(*l) {
                    for &ci in cis {
                        *shared.entry(ci).or_insert(0) += 1;
                    }
                }
            }
            // 候选排序: sim降序, 平票取最小idx(=旧实现"先见者胜"的确定性等价)
            let mut cands: Vec<(usize, f64)> = shared
                .into_iter()
                .filter_map(|(ci, n)| {
                    let union = label_set.len() + eff_labels[ci].len() - n;
                    if union == 0 {
                        None
                    } else {
                        Some((ci, n as f64 / union as f64))
                    }
                })
                .collect();
            cands.sort_by(|a, b| {
                b.1.partial_cmp(&a.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.0.cmp(&b.0))
            });
            let best = cands.first().copied();

            match best {
                Some((idx, sim)) if sim > 0.3 => {
                    concepts[idx].member_count += 1;
                    concepts[idx].member_ids.push(id);
                    if concepts[idx].member_ids.len() > 100 {
                        // 驱逐最老成员后重建该概念的有效标签集(与旧实现按当前成员重算等价)
                        let drained: Vec<TetraId> = concepts[idx].member_ids.drain(0..10).collect();
                        for l in eff_labels[idx].iter() {
                            if let Some(v) = postings.get_mut(*l) {
                                v.retain(|&c| c != idx);
                            }
                        }
                        let mut set: HashSet<&str> = HashSet::new();
                        for &mid in concepts[idx].member_ids.iter().chain(drained.iter()) {
                            // drained成员已不在member_ids, 但仍在批次内——旧实现
                            // jaccard只看当前member_ids, 所以这里排除drained
                            if drained.contains(&mid) {
                                continue;
                            }
                            if let Some(ml) = labels_map.get(&mid) {
                                for l in ml.iter() {
                                    set.insert(l.as_str());
                                }
                            }
                        }
                        for l in &set {
                            postings.entry(*l).or_default().push(idx);
                        }
                        eff_labels[idx] = set;
                    } else {
                        absorb(idx, labels, &mut eff_labels, &mut postings);
                    }
                }
                _ => {
                    // 修复：不为孤立 tetra 创建 member_count=1 的垃圾 concept_N。
                    // 只有当 tetra 有有意义的 labels 时才创建概念（避免 concept_N 噪音）。
                    // 孤立 tetra（无标签或标签太特殊）不归属任何概念，等未来有相似记忆时再聚类。
                    if !labels.is_empty() {
                        let next_id = concepts.len() as u64;
                        // 用第一个 label 作为概念名，而不是 concept_N（更有语义意义）
                        concepts.push(ConceptPrototype {
                            id: next_id,
                            centroid: vec![],
                            member_count: 1,
                            label: labels
                                .first()
                                .cloned()
                                .unwrap_or_else(|| format!("cluster_{}", next_id)),
                            member_ids: vec![id],
                        });
                        let ci = concepts.len() - 1;
                        let mut set: HashSet<&str> = HashSet::new();
                        for l in labels {
                            set.insert(l.as_str());
                        }
                        for l in &set {
                            postings.entry(*l).or_default().push(ci);
                        }
                        eff_labels.push(set);
                    }
                    // labels 为空的 tetra：不创建概念（之前会生成 concept_N 垃圾）
                }
            }
        }
        self.mark_dirty(false);
    }

    pub fn get_concepts(&self) -> Vec<ConceptPrototype> {
        self.concepts.read().clone()
    }

    pub fn get_top_concepts(&self, limit: usize) -> Vec<(String, u64)> {
        let concepts = self.concepts.read();
        let mut labeled: Vec<(String, u64)> = concepts
            .iter()
            .map(|c| (c.label.clone(), c.member_count))
            .collect();
        labeled.sort_by_key(|b| std::cmp::Reverse(b.1));
        labeled.truncate(limit);
        labeled
    }

    /// node_limit>0 时按质量取top-N(图谱export曾12MB/74s致客户端499超时)
    pub fn export_graph(&self, space: &Space, node_limit: usize) -> GraphExport {
        let relations = self.relations.read();
        let concepts = self.concepts.read();
        let mut tetras = space.all_tetrahedrons();
        let total_count = tetras.len();
        let truncated = node_limit > 0 && total_count > node_limit;
        if truncated {
            tetras.sort_by(|a, b| {
                b.mass
                    .partial_cmp(&a.mass)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            tetras.truncate(node_limit);
        }

        let mut node_map: HashMap<TetraId, GraphNodeExport> = HashMap::new();
        for t in &tetras {
            node_map.insert(
                t.id,
                GraphNodeExport {
                    id: t.id,
                    content: t.data.content.chars().take(200).collect(),
                    labels: t.data.labels.clone(),
                    mass: t.mass,
                    timestamp: t.data.timestamp as u64,
                    core_x: t.core.x,
                    core_y: t.core.y,
                    core_z: t.core.z,
                },
            );
        }

        let edge_exports: Vec<GraphEdgeExport> = relations
            .iter()
            .filter(|r| node_map.contains_key(&r.source) && node_map.contains_key(&r.target))
            .map(|r| GraphEdgeExport {
                source: r.source,
                target: r.target,
                relation_type: format!("{}", r.relation_type),
                strength: (r.strength * 100.0).round() / 100.0,
                hits: r.hits,
            })
            .collect();

        let concept_exports: Vec<ConceptExport> = concepts
            .iter()
            .map(|c| {
                let mut ids = c.member_ids.clone();
                ids.truncate(50);
                ConceptExport {
                    id: c.id,
                    label: c.label.clone(),
                    member_count: c.member_count,
                    member_ids: ids,
                }
            })
            .collect();

        let mut label_freq: HashMap<String, usize> = HashMap::new();
        for t in &tetras {
            for l in &t.data.labels {
                *label_freq.entry(l.clone()).or_insert(0) += 1;
            }
        }
        let mut top_labels: Vec<(String, usize)> = label_freq
            .into_iter()
            .filter(|(l, _)| !l.starts_with("meta-") && !l.starts_with("entity:"))
            .collect();
        top_labels.sort_by_key(|b| std::cmp::Reverse(b.1));
        top_labels.truncate(30);

        let clusters = space.find_clusters();
        let cluster_exports: Vec<ClusterExport> = clusters
            .iter()
            .take(20)
            .map(|c| {
                let cluster_labels: HashMap<String, usize> = c
                    .tetra_ids
                    .iter()
                    .filter_map(|id| space.get_tetrahedron(*id))
                    .flat_map(|t| t.data.labels.clone())
                    .fold(HashMap::new(), |mut acc, l| {
                        *acc.entry(l).or_insert(0) += 1;
                        acc
                    });
                let mut sorted: Vec<(String, usize)> = cluster_labels.into_iter().collect();
                sorted.sort_by_key(|b| std::cmp::Reverse(b.1));
                ClusterExport {
                    size: c.tetra_ids.len(),
                    member_ids: c.tetra_ids.iter().take(50).copied().collect(),
                    top_labels: sorted
                        .iter()
                        .take(3)
                        .map(|(l, c)| serde_json::json!({"label": l, "count": c}))
                        .collect(),
                }
            })
            .collect();

        // O(relations) 单遍: 先建 tetra→cluster 索引, 一次扫描统计跨簇对(原三重循环42亿次运算=74s超时主因)
        let mut inter_cluster_edges: Vec<GraphEdgeExport> = Vec::new();
        {
            let mut tetra_cluster: HashMap<TetraId, usize> = HashMap::new();
            for (ci, c) in clusters.iter().enumerate() {
                for &tid in &c.tetra_ids {
                    tetra_cluster.insert(tid, ci);
                }
            }
            let mut pair_count: HashMap<(usize, usize), usize> = HashMap::new();
            for r in relations.iter() {
                if let (Some(&ci), Some(&cj)) =
                    (tetra_cluster.get(&r.source), tetra_cluster.get(&r.target))
                {
                    if ci != cj {
                        let key = if ci < cj { (ci, cj) } else { (cj, ci) };
                        *pair_count.entry(key).or_insert(0) += 1;
                    }
                }
            }
            let mut pairs: Vec<_> = pair_count.into_iter().collect();
            pairs.sort_by_key(|p| std::cmp::Reverse(p.1));
            for ((i, j), count) in pairs.into_iter().take(200) {
                inter_cluster_edges.push(GraphEdgeExport {
                    source: clusters[i].tetra_ids.first().copied().unwrap_or(0),
                    target: clusters[j].tetra_ids.first().copied().unwrap_or(0),
                    relation_type: "inter_cluster".to_string(),
                    strength: count as f64,
                    hits: 0,
                });
            }
        }

        GraphExport {
            truncated,
            nodes: node_map.into_values().collect(),
            edges: edge_exports,
            inter_cluster_edges,
            concepts: concept_exports,
            clusters: cluster_exports,
            top_labels: top_labels
                .into_iter()
                .map(|(l, c)| serde_json::json!({"label": l, "count": c}))
                .collect(),
            total_nodes: total_count,
            total_edges: relations.len(),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let relations = self.relations.read().clone();
        let concepts = self.concepts.read().clone();
        let snapshot = KgSnapshot {
            relations,
            concepts,
        };
        let json = serde_json::to_string_pretty(&snapshot).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, &json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn load(&self, path: &Path) -> Result<(), String> {
        if !path.exists() {
            return Ok(());
        }
        let data = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        if data.trim().is_empty() {
            return Ok(());
        }
        let snapshot: KgSnapshot = serde_json::from_str(&data).map_err(|e| e.to_string())?;
        *self.relations.write() = snapshot.relations;
        *self.concepts.write() = snapshot.concepts;
        {
            let relations = self.relations.read();
            *self.adj_index.write() = self.rebuild_adj_index(&relations);
            *self.rel_dedup.write() = self.rebuild_rel_dedup(&relations);
        }
        Ok(())
    }

    pub fn relation_count(&self) -> usize {
        self.relations.read().len()
    }

    pub fn concept_count(&self) -> usize {
        self.concepts.read().len()
    }

    pub fn all_relations(&self) -> Vec<Relation> {
        self.relations.read().clone()
    }

    pub fn restore_concepts(&self, concepts: Vec<ConceptPrototype>) {
        *self.concepts.write() = concepts;
    }

    pub fn merge_duplicate_concepts(&self) -> usize {
        let mut concepts = self.concepts.write();
        if concepts.len() <= 1 {
            return 0;
        }

        let mut groups: std::collections::HashMap<String, Vec<usize>> = HashMap::new();
        for (i, c) in concepts.iter().enumerate() {
            let key = c.label.to_lowercase().replace(['-', ' '], "_");
            groups.entry(key).or_default().push(i);
        }

        let mut merged_count = 0usize;
        let mut to_remove: Vec<usize> = Vec::new();
        let mut to_update: Vec<(usize, u64, Vec<u64>)> = Vec::new();

        for indices in groups.values() {
            if indices.len() <= 1 {
                continue;
            }
            let primary = indices[0];
            let mut total_count = concepts[primary].member_count;
            let mut all_ids = concepts[primary].member_ids.clone();

            for &dup_idx in &indices[1..] {
                total_count += concepts[dup_idx].member_count;
                all_ids.extend(concepts[dup_idx].member_ids.iter().copied());
                to_remove.push(dup_idx);
                merged_count += 1;
            }

            if all_ids.len() > 100 {
                all_ids = all_ids.split_off(all_ids.len() - 100);
            }
            to_update.push((primary, total_count, all_ids));
        }

        for (idx, count, ids) in to_update {
            concepts[idx].member_count = count;
            concepts[idx].member_ids = ids;
        }

        to_remove.sort_unstable_by(|a, b| b.cmp(a));
        for idx in to_remove {
            concepts.remove(idx);
        }

        for (i, c) in concepts.iter_mut().enumerate() {
            c.id = i as u64;
        }

        if merged_count > 0 {
            self.mark_dirty(false);
            tracing::info!(
                "[KG] merged {} duplicate concepts, {} remaining",
                merged_count,
                concepts.len()
            );
        }
        merged_count
    }

    pub fn analysis(&self, space: &Space) -> KgAnalysis {
        let relations = self.relations.read();
        let tetras = space.all_tetrahedrons();
        let total_tetras = tetras.len();
        let total_relations = relations.len();

        let mut connected: HashSet<TetraId> = HashSet::new();
        for r in relations.iter() {
            connected.insert(r.source);
            connected.insert(r.target);
        }
        let orphan_count = total_tetras.saturating_sub(connected.len());

        let adj = self.adj_index.read();
        let mut visited: HashSet<TetraId> = HashSet::new();
        let mut components: Vec<usize> = Vec::new();
        let mut stack: Vec<TetraId>;
        for &id in connected.iter() {
            if visited.contains(&id) {
                continue;
            }
            stack = vec![id];
            let mut comp_size = 0usize;
            while let Some(cur) = stack.pop() {
                if visited.contains(&cur) {
                    continue;
                }
                visited.insert(cur);
                comp_size += 1;
                if let Some(indices) = adj.get(&cur) {
                    for &i in indices {
                        let r = &relations[i];
                        let n = if r.source == cur { r.target } else { r.source };
                        if !visited.contains(&n) {
                            stack.push(n);
                        }
                    }
                }
            }
            components.push(comp_size);
        }
        components.sort_by(|a, b| b.cmp(a));
        let disconnected_components = if components.len() > 1 {
            components[1..].to_vec()
        } else {
            vec![]
        };

        let avg_degree = if total_tetras > 0 {
            connected
                .iter()
                .map(|id| adj.get(id).map(|v| v.len()).unwrap_or(0) as f64)
                .sum::<f64>()
                / total_tetras as f64
        } else {
            0.0
        };

        let density = if total_tetras > 1 {
            (2.0 * total_relations as f64) / (total_tetras as f64 * (total_tetras as f64 - 1.0))
        } else {
            0.0
        };

        let rel_type_counts: HashMap<String, usize> =
            relations.iter().fold(HashMap::new(), |mut m, r| {
                let key = format!("{}", r.relation_type);
                *m.entry(key).or_insert(0) += 1;
                m
            });

        // 关系拓扑社区(标签传播) + 模块度: 与空间聚类互补的结构健康信号
        let comm_labels = self.detect_communities(5);
        let (community_count, largest_community, modularity) =
            self.community_modularity(&comm_labels);

        KgAnalysis {
            total_tetras,
            total_relations,
            orphan_count,
            largest_component: components.first().copied().unwrap_or(0),
            disconnected_components,
            avg_degree,
            density,
            relation_type_counts: rel_type_counts,
            community_count,
            largest_community,
            modularity,
        }
    }
}

/// P3 Agentic GraphRAG: Extract entity-like tokens from memory content.
///
/// Lightweight heuristic entity extractor (no LLM needed for hot path).
/// Identifies:
/// - PascalCase identifiers (e.g. MemoryPayload, VectorLayer)
/// - UPPER_SNAKE_CASE constants (e.g. VERTEX_MERGE_EPSILON)
/// - snake_case identifiers with underscores (e.g. search_engine, valid_to)
///
/// Returns deduplicated lowercase entity strings.
fn extract_entities(content: &str) -> Vec<String> {
    use std::collections::HashSet;
    let mut entities: HashSet<String> = HashSet::new();

    for token in content.split(|c: char| !c.is_alphanumeric() && c != '_') {
        let token = token.trim();
        if token.len() < 4 || token.len() > 40 {
            continue;
        }

        let chars: Vec<char> = token.chars().collect();
        let starts_upper = !chars.is_empty() && chars[0].is_uppercase();
        let has_lower = chars.iter().any(|c| c.is_lowercase());
        let has_upper_inside = chars.len() > 1 && chars[1..].iter().any(|c| c.is_uppercase());
        let has_underscore = token.contains('_');
        let all_alpha_or_underscore = token.chars().all(|c| c.is_alphanumeric() || c == '_');

        if !all_alpha_or_underscore {
            continue;
        }

        // PascalCase: starts uppercase, has lowercase, has uppercase inside
        if starts_upper && has_lower && has_upper_inside {
            entities.insert(token.to_lowercase());
            continue;
        }

        // UPPER_SNAKE_CASE: all uppercase + underscores, length > 4
        if has_underscore
            && token
                .chars()
                .all(|c| c.is_uppercase() || c == '_' || c.is_ascii_digit())
            && token.len() > 4
        {
            entities.insert(token.to_lowercase());
            continue;
        }

        // snake_case: has underscore, reasonable length
        if has_underscore && token.len() > 6 {
            entities.insert(token.to_lowercase());
            continue;
        }
    }

    entities.into_iter().collect()
}
#[derive(serde::Serialize, serde::Deserialize)]
struct KgSnapshot {
    relations: Vec<Relation>,
    concepts: Vec<ConceptPrototype>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct KgAnalysis {
    pub total_tetras: usize,
    pub total_relations: usize,
    pub orphan_count: usize,
    pub largest_component: usize,
    pub disconnected_components: Vec<usize>,
    pub avg_degree: f64,
    pub density: f64,
    pub relation_type_counts: HashMap<String, usize>,
    /// 关系拓扑社区数(异步标签传播, Raghavan 2007)
    pub community_count: usize,
    pub largest_community: usize,
    /// 加权模块度 Q(Newman): >0.3 即显著社区结构
    pub modularity: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GraphExport {
    pub truncated: bool,
    pub nodes: Vec<GraphNodeExport>,
    pub edges: Vec<GraphEdgeExport>,
    pub inter_cluster_edges: Vec<GraphEdgeExport>,
    pub concepts: Vec<ConceptExport>,
    pub clusters: Vec<ClusterExport>,
    pub top_labels: Vec<serde_json::Value>,
    pub total_nodes: usize,
    pub total_edges: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GraphNodeExport {
    pub id: TetraId,
    pub content: String,
    pub labels: Vec<String>,
    pub mass: f64,
    pub timestamp: u64,
    pub core_x: f64,
    pub core_y: f64,
    pub core_z: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GraphEdgeExport {
    pub source: TetraId,
    pub target: TetraId,
    pub relation_type: String,
    pub strength: f64,
    /// 检索命中计数: 前端以此高亮"主干道"(被检索巩固的边)
    #[serde(default)]
    pub hits: u16,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ConceptExport {
    pub id: u64,
    pub label: String,
    pub member_count: u64,
    pub member_ids: Vec<TetraId>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ClusterExport {
    pub size: usize,
    pub member_ids: Vec<TetraId>,
    pub top_labels: Vec<serde_json::Value>,
}

#[cfg(test)]
#[cfg(test)]
fn label_jaccard(
    labels: &[String],
    concept_member_ids: &[TetraId],
    labels_map: &HashMap<TetraId, &Vec<String>>,
) -> f64 {
    if concept_member_ids.is_empty() || labels.is_empty() {
        return 0.0;
    }
    let mut concept_labels: HashSet<&str> = HashSet::new();
    for &mid in concept_member_ids {
        if let Some(ml) = labels_map.get(&mid) {
            for l in ml.iter() {
                concept_labels.insert(l.as_str());
            }
        }
    }
    if concept_labels.is_empty() {
        return 0.0;
    }
    let label_set: HashSet<&str> = labels.iter().map(|s| s.as_str()).collect();
    let intersection = label_set.intersection(&concept_labels).count();
    let union = label_set.union(&concept_labels).count();
    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tetra::Tetrahedron;
    use crate::domain::vertex::Point3;

    #[test]
    fn auto_link_creates_relations() {
        let space = Space::new();
        let kg = KnowledgeGraph::new();

        for (text, labels) in [
            ("hello world", vec!["greeting".to_string()]),
            ("hello there", vec!["greeting".to_string()]),
            ("goodbye moon", vec!["farewell".to_string()]),
        ] {
            let core = Point3::zero();
            let pos = Tetrahedron::compute_vertices(core);
            let t = Tetrahedron {
                id: 0,
                vertex_ids: [0; 4],
                core,
                data: crate::domain::tetra::MemoryPayload {
                    content: text.to_string(),
                    content_hash: 0,
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
                    ..Default::default()
                },
                mass: 1.0,
            };
            space.add_tetrahedron(&t, &pos).unwrap();
        }

        kg.auto_link(&space, 0.2);
        let count = kg.relations.read().len();
        assert!(count > 0, "should create at least one relation");
    }

    #[test]
    fn multi_hop_finds_expanded_results() {
        let kg = KnowledgeGraph::new();
        kg.add_relation(0, 1, RelationType::SimilarTo, 0.9);
        kg.add_relation(1, 2, RelationType::SimilarTo, 0.8);

        let results = kg.multi_hop(&[0], 2);
        assert!(results.iter().any(|(id, _)| *id == 2));
    }

    #[test]
    fn concepts_update_incrementally() {
        let kg = KnowledgeGraph::new();

        kg.update_concepts(&[(0, vec!["rust".to_string()]), (1, vec!["rust".to_string()])]);
        let concepts = kg.get_concepts();
        assert_eq!(concepts.len(), 1);
        assert_eq!(concepts[0].member_count, 2);
    }

    #[test]
    fn decay_removes_weak_relations() {
        let kg = KnowledgeGraph::new();
        kg.add_relation(0, 1, RelationType::SimilarTo, MIN_STRENGTH + 0.001);
        assert_eq!(kg.relation_count(), 1);
        for _ in 0..100 {
            kg.decay_relations();
        }
        assert_eq!(
            kg.relation_count(),
            0,
            "weak relation should be decayed away"
        );
    }

    #[test]
    fn adjacency_index_accelerates_query() {
        let kg = KnowledgeGraph::new();
        kg.add_relation(0, 1, RelationType::SimilarTo, 0.9);
        kg.add_relation(0, 2, RelationType::Related, 0.5);
        kg.add_relation(3, 4, RelationType::SimilarTo, 0.8);

        let rels_0 = kg.query_relations(0);
        assert_eq!(rels_0.len(), 2);

        let rels_3 = kg.query_relations(3);
        assert_eq!(rels_3.len(), 1);

        let rels_99 = kg.query_relations(99);
        assert_eq!(rels_99.len(), 0);
    }

    #[test]
    fn max_relations_per_node() {
        let kg = KnowledgeGraph::new();
        for i in 1..=60 {
            kg.add_relation(0, i, RelationType::SimilarTo, 0.5);
        }
        assert!(kg.relation_count() <= MAX_RELATIONS_PER_NODE);
    }

    #[test]
    fn structural_relation_direction_survives_dedup_rebuild() {
        let kg = KnowledgeGraph::new();
        for relation_type in [RelationType::BelongsTo, RelationType::MergedInto] {
            kg.add_relation(1, 2, relation_type.clone(), 0.8);
            kg.add_relation(3, 4, RelationType::SimilarTo, 0.7);
            kg.remove_relation(3, 4, RelationType::SimilarTo);
            kg.add_relation(2, 1, relation_type, 0.6);
        }

        assert_eq!(kg.relation_count(), 4);
    }

    #[test]
    fn pending_relation_delta_coalesces_remove_and_readd() {
        let kg = KnowledgeGraph::new();
        kg.add_relation(1, 2, RelationType::SimilarTo, 0.5);
        kg.drain_pending_relations();

        kg.remove_relation(1, 2, RelationType::SimilarTo);
        kg.add_relation(1, 2, RelationType::SimilarTo, 0.8);
        let (upserts, deletes) = kg.drain_pending_relations();

        assert_eq!(upserts.len(), 1);
        assert_eq!(upserts[0].strength, 0.8);
        assert!(deletes.is_empty());
    }
}

#[cfg(test)]
mod kg_research_tests {
    use super::*;

    // ───────── 检索强化衰减 ─────────

    #[test]
    fn reinforcement_outlives_uniform_decay() {
        let kg = KnowledgeGraph::new();
        kg.add_relation(1, 2, RelationType::SimilarTo, 0.5);
        kg.add_relation(3, 4, RelationType::SimilarTo, 0.5);

        // 检索路径只走 (1,2) 两次
        kg.reinforce_edges(&[(1, 2)]);
        kg.reinforce_edges(&[(1, 2)]);

        for _ in 0..500 {
            kg.decay_relations();
        }
        let relations = kg.relations.read();
        let r12 = relations
            .iter()
            .find(|r| r.source == 1 && r.target == 2)
            .expect("edge 1-2");
        let r34 = relations
            .iter()
            .find(|r| r.source == 3 && r.target == 4)
            .expect("edge 3-4 (未强化必须存活: 0.5×0.9995^500≈0.39 > 0.05)");
        let (h12, s12, s34) = (r12.hits, r12.strength, r34.strength);
        drop(relations);

        assert_eq!(h12, 2, "两次检索命中应记数");
        assert!(s12 > s34, "强化边({s12})应强于同起点未强化边({s34})");
        assert!(s34 > 0.3, "未强化边在500轮衰减后仍有0.39左右");
    }

    #[test]
    fn multi_hop_reinforces_only_traversed_edges() {
        let kg = KnowledgeGraph::new();
        // 链 1—2—3—4 (强度足够过0.1剪枝)
        kg.add_relation(1, 2, RelationType::SimilarTo, 0.9);
        kg.add_relation(2, 3, RelationType::SimilarTo, 0.9);
        kg.add_relation(3, 4, RelationType::SimilarTo, 0.9);

        let reached = kg.multi_hop(&[1], 2);
        let relations = kg.relations.read();
        let hits_of = |a: TetraId, b: TetraId| -> u16 {
            relations
                .iter()
                .find(|r| (r.source == a && r.target == b) || (r.source == b && r.target == a))
                .map(|r| r.hits)
                .expect("edge exists")
        };
        let h12 = hits_of(1, 2);
        let h23 = hits_of(2, 3);
        let h34 = hits_of(3, 4);
        drop(relations);

        assert_eq!(h12, 1, "1跳走过的边");
        assert_eq!(h23, 1, "2跳走过的边");
        assert_eq!(h34, 0, "3跳之外的边不被强化");
        assert!(reached.iter().any(|(id, _)| *id == 3), "2跳应达3");
        assert!(!reached.iter().any(|(id, _)| *id == 4), "2跳不应达4");
    }

    // ───────── 标签传播社区 + 模块度 ─────────

    #[test]
    fn label_propagation_separates_two_communities() {
        let kg = KnowledgeGraph::new();
        // 两个三角团 + 一条弱桥
        for &(a, b) in &[(1u64, 2u64), (2, 3), (3, 1)] {
            kg.add_relation(a, b, RelationType::SimilarTo, 0.9);
        }
        for &(a, b) in &[(10u64, 11u64), (11, 12), (12, 10)] {
            kg.add_relation(a, b, RelationType::SimilarTo, 0.9);
        }
        kg.add_relation(3, 10, RelationType::Related, 0.1);

        let labels = kg.detect_communities(5);
        assert_eq!(labels.len(), 6, "六个节点都在图上");
        let l1 = labels[&1];
        assert_eq!(labels[&2], l1, "三角1同社区");
        assert_eq!(labels[&3], l1, "三角1同社区");
        let l10 = labels[&10];
        assert_eq!(labels[&11], l10, "三角2同社区");
        assert_eq!(labels[&12], l10, "三角2同社区");
        assert_ne!(l1, l10, "两个团应分属不同社区");

        let (n_comm, largest, q) = kg.community_modularity(&labels);
        assert_eq!(n_comm, 2);
        assert_eq!(largest, 3);
        assert!(q > 0.3, "双团+弱桥的模块度应显著(实测Q={q})");
    }

    // ───────── 概念聚合倒排: 与旧实现精确等价 ─────────

    /// 旧实现逐字拷贝(独立操作裸Vec, 作等价性参照)
    fn update_concepts_reference(
        concepts: &mut Vec<ConceptPrototype>,
        tetras: &[(TetraId, Vec<String>)],
    ) {
        let labels_map: HashMap<TetraId, &Vec<String>> =
            tetras.iter().map(|(id, l)| (*id, l)).collect();
        for &(id, ref labels) in tetras {
            let mut best: Option<(usize, f64)> = None;
            for (i, c) in concepts.iter().enumerate() {
                let sim = label_jaccard(labels, &c.member_ids, &labels_map);
                match &best {
                    None => best = Some((i, sim)),
                    Some((_, s)) if sim > *s => best = Some((i, sim)),
                    _ => {}
                }
            }
            match best {
                Some((idx, sim)) if sim > 0.3 => {
                    concepts[idx].member_count += 1;
                    concepts[idx].member_ids.push(id);
                    if concepts[idx].member_ids.len() > 100 {
                        concepts[idx].member_ids.drain(0..10);
                    }
                }
                _ => {
                    if !labels.is_empty() {
                        let next_id = concepts.len() as u64;
                        concepts.push(ConceptPrototype {
                            id: next_id,
                            centroid: vec![],
                            member_count: 1,
                            label: labels
                                .first()
                                .cloned()
                                .unwrap_or_else(|| format!("cluster_{}", next_id)),
                            member_ids: vec![id],
                        });
                    }
                }
            }
        }
    }

    fn assert_same_concepts(a: &[ConceptPrototype], b: &[ConceptPrototype], ctx: &str) {
        assert_eq!(a.len(), b.len(), "{ctx}: 概念数");
        for (ca, cb) in a.iter().zip(b.iter()) {
            assert_eq!(ca.id, cb.id, "{ctx}: id");
            assert_eq!(ca.label, cb.label, "{ctx}: label");
            assert_eq!(ca.member_count, cb.member_count, "{ctx}: member_count");
            assert_eq!(ca.member_ids, cb.member_ids, "{ctx}: member_ids");
        }
    }

    #[test]
    fn update_concepts_inverted_matches_reference() {
        // 批次覆盖: 新概念创建/聚拢/空标签/标签漂移/跨越100成员触发drain/平票
        let mut batch: Vec<(TetraId, Vec<String>)> = Vec::new();
        // 105个同标签 → 单概念膨胀跨100触发drain
        for i in 0..105u64 {
            batch.push((1000 + i, vec!["rust".into(), "memory".into()]));
        }
        // 空标签 ×3 (不建概念)
        for i in 0..3u64 {
            batch.push((2000 + i, vec![]));
        }
        // 第二族群: 部分重叠标签(会算出<0.3或>0.3的各种sim)
        for i in 0..30u64 {
            batch.push((3000 + i, vec!["rust".into(), "search".into()]));
        }
        // 孤立标签族
        for i in 0..5u64 {
            batch.push((4000 + i, vec![format!("solo{i}")]));
        }
        // 交错打入顺序(确定性LCG)
        let mut seed = 42u64;
        let mut i = batch.len();
        while i > 1 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let j = (seed >> 33) as usize % i;
            i -= 1;
            batch.swap(i, j);
        }

        let kg = KnowledgeGraph::new();
        kg.update_concepts(&batch);
        let mut reference: Vec<ConceptPrototype> = Vec::new();
        update_concepts_reference(&mut reference, &batch);

        assert_same_concepts(&kg.get_concepts(), &reference, "首批");

        // 第二批(已有概念状态下的指派/drain再触发): 语义等价的真正考验
        let batch2: Vec<(TetraId, Vec<String>)> = (0..40u64)
            .map(|i| {
                if i % 3 == 0 {
                    (5000 + i, vec!["rust".into()])
                } else if i % 3 == 1 {
                    (5000 + i, vec!["search".into(), "rust".into()])
                } else {
                    (5000 + i, vec![])
                }
            })
            .collect();
        kg.update_concepts(&batch2);
        update_concepts_reference(&mut reference, &batch2);
        assert_same_concepts(&kg.get_concepts(), &reference, "次批");
    }
}
