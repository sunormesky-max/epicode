use std::collections::{HashMap, HashSet};
use parking_lot::Mutex;
use std::path::Path;
use parking_lot::RwLock;

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
    /// F4增量持久化: 待写/待删关系 — auto_save只写增量, 全量重写仅final_save
    pending_upserts: Mutex<Vec<Relation>>,
    pending_deletes: Mutex<Vec<(TetraId, TetraId, RelationType)>>,
    /// 加载期抑制: load_relations重放不得灌爆增量队列
    pub loading: std::sync::atomic::AtomicBool,
    adj_index: RwLock<HashMap<TetraId, Vec<usize>>>,
    concepts: RwLock<Vec<ConceptPrototype>>,
    dirty: std::sync::atomic::AtomicBool,
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
            pending_upserts: Mutex::new(Vec::new()),
            pending_deletes: Mutex::new(Vec::new()),
            loading: std::sync::atomic::AtomicBool::new(false),
            adj_index: RwLock::new(HashMap::new()),
            concepts: RwLock::new(Vec::new()),
            dirty: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// F4: 取增量(并把队列还给调用方) — 空返回时调用方应回退全量
    pub fn drain_pending_relations(&self) -> (Vec<Relation>, Vec<(TetraId, TetraId, RelationType)>) {
        let ups = std::mem::take(&mut *self.pending_upserts.lock());
        let dels = std::mem::take(&mut *self.pending_deletes.lock());
        (ups, dels)
    }
    pub fn set_loading(&self, v: bool) {
        self.loading.store(v, std::sync::atomic::Ordering::Relaxed);
        if !v { // 加载结束: 清空加载期误入队的残留
            self.pending_upserts.lock().clear();
            self.pending_deletes.lock().clear();
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn clear_dirty(&self) {
        self.dirty.store(false, std::sync::atomic::Ordering::Relaxed);
    }

    fn rebuild_adj_index(&self, relations: &[Relation]) -> HashMap<TetraId, Vec<usize>> {
        let mut idx: HashMap<TetraId, Vec<usize>> = HashMap::new();
        for (i, r) in relations.iter().enumerate() {
            idx.entry(r.source).or_default().push(i);
            idx.entry(r.target).or_default().push(i);
        }
        idx
    }

    pub fn add_relation(&self, source: TetraId, target: TetraId, rel_type: RelationType, strength: f64) {
        self.add_relation_at(source, target, rel_type, strength, 0);
    }

    pub fn add_relation_at(&self, source: TetraId, target: TetraId, rel_type: RelationType, strength: f64, tick: u64) {
        let mut relations = self.relations.write();
        // 归属关系(BelongsTo/MergedInto)做单向去重——只检查相同方向
        // 其他关系(similar/contradicts等)做双向去重
        let exists = if rel_type == RelationType::BelongsTo || rel_type == RelationType::MergedInto {
            relations.iter().any(|r| r.source == source && r.target == target && r.relation_type == rel_type)
        } else {
            relations.iter().any(|r|
                (r.source == source && r.target == target || r.source == target && r.target == source)
                && r.relation_type == rel_type
            )
        };
        if exists {
            return;
        }
        {
            let adj = self.adj_index.read();
            let src_count = adj.get(&source).map(|v| v.len()).unwrap_or(0);
            let tgt_count = adj.get(&target).map(|v| v.len()).unwrap_or(0);
            // 归属关系不受数量限制（否则高连接度的旧节点无法加入档案库层级）
            let is_structural = rel_type == RelationType::BelongsTo || rel_type == RelationType::MergedInto;
            if !is_structural && (src_count >= MAX_RELATIONS_PER_NODE || tgt_count >= MAX_RELATIONS_PER_NODE) {
                return;
            }
        }
        let new_rel = Relation {
            source, target, relation_type: rel_type, strength,
            created_tick: tick,
        };
        relations.push(new_rel.clone());
        *self.adj_index.write() = self.rebuild_adj_index(&relations);
        if !self.loading.load(std::sync::atomic::Ordering::Relaxed) {
            self.pending_upserts.lock().push(new_rel);
        }
        self.dirty.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn remove_relations_for(&self, id: TetraId) {
        let mut relations = self.relations.write();
        let before = relations.len();
        let removed: Vec<(TetraId, TetraId, RelationType)> = relations.iter()
            .filter(|r| r.source == id || r.target == id)
            .map(|r| (r.source, r.target, r.relation_type.clone()))
            .collect();
        relations.retain(|r| r.source != id && r.target != id);
        if relations.len() < before {
            *self.adj_index.write() = self.rebuild_adj_index(&relations);
            if !self.loading.load(std::sync::atomic::Ordering::Relaxed) {
                self.pending_deletes.lock().extend(removed);
            }
        }
        self.dirty.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// 精确删除：只删 source→target 且类型匹配的关系
    pub fn remove_relation(&self, source: TetraId, target: TetraId, rel_type: RelationType) {
        let mut relations = self.relations.write();
        let before = relations.len();
        relations.retain(|r| !(r.source == source && r.target == target && r.relation_type == rel_type));
        if relations.len() < before {
            *self.adj_index.write() = self.rebuild_adj_index(&relations);
            if !self.loading.load(std::sync::atomic::Ordering::Relaxed) {
                self.pending_deletes.lock().push((source, target, rel_type));
            }
        }
        self.dirty.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// 计算/更新所有 concept 的 centroid（成员 embedding 均值）。
    /// 之前断联：centroid 永远 vec![]，概念聚类只能靠标签 Jaccard。
    pub fn recompute_centroids(&self, space: &crate::domain::space::Space) {
        let mut concepts = self.concepts.write();
        for c in concepts.iter_mut() {
            if c.member_ids.is_empty() {
                c.centroid.clear();
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
                c.centroid = sum.iter().map(|v| v / count as f64).collect();
            }
        }
    }


    pub fn query_relations(&self, id: TetraId) -> Vec<(TetraId, RelationType, f64)> {
        let relations = self.relations.read();
        let adj = self.adj_index.read();
        match adj.get(&id) {
            Some(indices) => indices.iter()
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
                    &tetras[i].data.embedding, &tetras[i].data.labels,
                    &tetras[j].data.embedding, &tetras[j].data.labels,
                );
                if sim > threshold {
                    self.add_relation(tetras[i].id, tetras[j].id, RelationType::SimilarTo, sim);
                }
            }
        }
    }

    pub fn auto_link_one(&self, new_id: TetraId, space: &Space, label_index: &std::collections::HashMap<String, Vec<TetraId>>) {
        let new_tetra = match space.get_tetrahedron(new_id) {
            Some(t) => t,
            None => return,
        };
        let mut candidate_ids: std::collections::HashSet<TetraId> = std::collections::HashSet::new();
        for label in &new_tetra.data.labels {
            if let Some(ids) = label_index.get(label) {
                for &id in ids {
                    if id != new_id {
                        candidate_ids.insert(id);
                    }
                }
            }
        }
        let mut candidates: Vec<_> = candidate_ids.iter()
            .filter_map(|&id| space.get_tetrahedron(id))
            .collect();
        if candidates.len() < 5 {
            let all = space.all_tetrahedrons();
            for t in &all {
                if t.id != new_id && !candidates.iter().any(|c| c.id == t.id) {
                    candidates.push(t.clone());
                    if candidates.len() >= 20 { break; }
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
                &new_tetra.data.embedding, &new_tetra.data.labels,
                &t.data.embedding, &t.data.labels,
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
                        if !t.data.labels.contains(label) && !label.starts_with("meta-") && !label.starts_with("entity:") {
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
            self.dirty.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        removed
    }

    pub fn multi_hop(
        &self,
        seeds: &[TetraId],
        max_hops: usize,
    ) -> Vec<(TetraId, f64)> {
        let relations = self.relations.read();
        let adj = self.adj_index.read();
        let mut visited: HashSet<TetraId> = seeds.iter().copied().collect();
        let mut scored: HashMap<TetraId, f64> = HashMap::new();
        let mut frontier: Vec<(TetraId, f64)> = seeds.iter().map(|&s| (s, 1.0)).collect();

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

                        visited.insert(neighbor);
                        let entry = scored.entry(neighbor).or_insert(0.0);
                        *entry = (*entry).max(score);
                        next_frontier.push((neighbor, score));
                    }
                }
            }
            frontier = next_frontier;
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
    pub fn multi_hop_adaptive(
        &self,
        seeds: &[TetraId],
        max_results: usize,
    ) -> Vec<(TetraId, f64)> {
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
            seeds.len(), avg_degree, depth, max_results
        );

        let mut results = self.multi_hop(seeds, depth);
        if results.len() > max_results {
            results.truncate(max_results);
        }
        results
    }

    pub fn update_concepts(&self, tetras: &[(TetraId, Vec<String>)]) {
        let mut concepts = self.concepts.write();
        let labels_map: HashMap<TetraId, &Vec<String>> = tetras.iter().map(|(id, l)| (*id, l)).collect();

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
                    // centroid 在 assign 时无法取 embedding（labels_map 只有标签）
                    // centroid 改为在 save_concepts 时由外部计算（见 save 时传入 embedding）
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
                            label: labels.first().cloned().unwrap_or_else(|| format!("cluster_{}", next_id)),
                            member_ids: vec![id],
                        });
                    }
                    // labels 为空的 tetra：不创建概念（之前会生成 concept_N 垃圾）
                }
            }
        }
        self.dirty.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn get_concepts(&self) -> Vec<ConceptPrototype> {
        self.concepts.read().clone()
    }

    pub fn get_top_concepts(&self, limit: usize) -> Vec<(String, u64)> {
        let concepts = self.concepts.read();
        let mut labeled: Vec<(String, u64)> = concepts.iter()
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
            tetras.sort_by(|a, b| b.mass.partial_cmp(&a.mass).unwrap_or(std::cmp::Ordering::Equal));
            tetras.truncate(node_limit);
        }

        let mut node_map: HashMap<TetraId, GraphNodeExport> = HashMap::new();
        for t in &tetras {
            node_map.insert(t.id, GraphNodeExport {
                id: t.id,
                content: t.data.content.chars().take(200).collect(),
                labels: t.data.labels.clone(),
                mass: t.mass,
                timestamp: t.data.timestamp as u64,
                core_x: t.core.x,
                core_y: t.core.y,
                core_z: t.core.z,
            });
        }

        let edge_exports: Vec<GraphEdgeExport> = relations.iter()
            .filter(|r| node_map.contains_key(&r.source) && node_map.contains_key(&r.target))
            .map(|r| GraphEdgeExport {
                source: r.source,
                target: r.target,
                relation_type: format!("{}", r.relation_type),
                strength: (r.strength * 100.0).round() / 100.0,
            })
            .collect();

        let concept_exports: Vec<ConceptExport> = concepts.iter()
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
        let mut top_labels: Vec<(String, usize)> = label_freq.into_iter()
            .filter(|(l, _)| !l.starts_with("meta-") && !l.starts_with("entity:"))
            .collect();
        top_labels.sort_by_key(|b| std::cmp::Reverse(b.1));
        top_labels.truncate(30);

        let clusters = space.find_clusters();
        let cluster_exports: Vec<ClusterExport> = clusters.iter()
            .take(20)
            .map(|c| {
                let cluster_labels: HashMap<String, usize> = c.tetra_ids.iter()
                    .filter_map(|id| space.get_tetrahedron(*id))
                    .flat_map(|t| t.data.labels.clone())
                    .fold(HashMap::new(), |mut acc, l| { *acc.entry(l).or_insert(0) += 1; acc });
                let mut sorted: Vec<(String, usize)> = cluster_labels.into_iter().collect();
                sorted.sort_by_key(|b| std::cmp::Reverse(b.1));
                ClusterExport {
                    size: c.tetra_ids.len(),
                    member_ids: c.tetra_ids.iter().take(50).copied().collect(),
                    top_labels: sorted.iter().take(3).map(|(l, c)| serde_json::json!({"label": l, "count": c})).collect(),
                }
            })
            .collect();

        // O(relations) 单遍: 先建 tetra→cluster 索引, 一次扫描统计跨簇对(原三重循环42亿次运算=74s超时主因)
        let mut inter_cluster_edges: Vec<GraphEdgeExport> = Vec::new();
        {
            let mut tetra_cluster: HashMap<TetraId, usize> = HashMap::new();
            for (ci, c) in clusters.iter().enumerate() {
                for &tid in &c.tetra_ids { tetra_cluster.insert(tid, ci); }
            }
            let mut pair_count: HashMap<(usize, usize), usize> = HashMap::new();
            for r in relations.iter() {
                if let (Some(&ci), Some(&cj)) = (tetra_cluster.get(&r.source), tetra_cluster.get(&r.target)) {
                    if ci != cj {
                        let key = if ci < cj { (ci, cj) } else { (cj, ci) };
                        *pair_count.entry(key).or_insert(0) += 1;
                    }
                }
            }
            let mut pairs: Vec<_> = pair_count.into_iter().collect();
            pairs.sort_by(|a, b| b.1.cmp(&a.1));
            for ((i, j), count) in pairs.into_iter().take(200) {
                inter_cluster_edges.push(GraphEdgeExport {
                    source: clusters[i].tetra_ids.first().copied().unwrap_or(0),
                    target: clusters[j].tetra_ids.first().copied().unwrap_or(0),
                    relation_type: "inter_cluster".to_string(),
                    strength: count as f64,
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
            top_labels: top_labels.into_iter().map(|(l, c)| serde_json::json!({"label": l, "count": c})).collect(),
            total_nodes: total_count,
            total_edges: relations.len(),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let relations = self.relations.read().clone();
        let concepts = self.concepts.read().clone();
        let snapshot = KgSnapshot { relations, concepts };
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
        if concepts.len() <= 1 { return 0; }

        let mut groups: std::collections::HashMap<String, Vec<usize>> = HashMap::new();
        for (i, c) in concepts.iter().enumerate() {
            let key = c.label.to_lowercase().replace(['-', ' '], "_");
            groups.entry(key).or_default().push(i);
        }

        let mut merged_count = 0usize;
        let mut to_remove: Vec<usize> = Vec::new();
        let mut to_update: Vec<(usize, u64, Vec<u64>)> = Vec::new();

        for indices in groups.values() {
            if indices.len() <= 1 { continue; }
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
            self.dirty.store(true, std::sync::atomic::Ordering::Relaxed);
            tracing::info!("[KG] merged {} duplicate concepts, {} remaining", merged_count, concepts.len());
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
            if visited.contains(&id) { continue; }
            stack = vec![id];
            let mut comp_size = 0usize;
            while let Some(cur) = stack.pop() {
                if visited.contains(&cur) { continue; }
                visited.insert(cur);
                comp_size += 1;
                if let Some(indices) = adj.get(&cur) {
                    for &i in indices {
                        let r = &relations[i];
                        let n = if r.source == cur { r.target } else { r.source };
                        if !visited.contains(&n) { stack.push(n); }
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
            connected.iter().map(|id| adj.get(id).map(|v| v.len()).unwrap_or(0) as f64).sum::<f64>() / total_tetras as f64
        } else { 0.0 };

        let density = if total_tetras > 1 {
            (2.0 * total_relations as f64) / (total_tetras as f64 * (total_tetras as f64 - 1.0))
        } else { 0.0 };

        let rel_type_counts: HashMap<String, usize> = relations.iter()
            .fold(HashMap::new(), |mut m, r| {
                let key = format!("{}", r.relation_type);
                *m.entry(key).or_insert(0) += 1;
                m
            });

        KgAnalysis {
            total_tetras,
            total_relations,
            orphan_count,
            largest_component: components.first().copied().unwrap_or(0),
            disconnected_components,
            avg_degree,
            density,
            relation_type_counts: rel_type_counts,
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
            && token.chars().all(|c| c.is_uppercase() || c == '_' || c.is_ascii_digit())
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
fn label_jaccard(labels: &[String], concept_member_ids: &[TetraId], labels_map: &HashMap<TetraId, &Vec<String>>) -> f64 {
    if concept_member_ids.is_empty() || labels.is_empty() {
        return 0.0;
    }
    // 聚合 concept 成员的 labels（kimi #2：之前错误地用 labels 自比导致恒为 1.0）
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
    if union == 0 { 0.0 } else { intersection as f64 / union as f64 }
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
                id: 0, vertex_ids: [0; 4], core,
                data: crate::domain::tetra::MemoryPayload { content: text.to_string(), content_hash: 0, labels, timestamp: 0, aliases: vec![], embedding: vec![], importance: 1.0, enforced: false, rationale: None, access_count: 0, memory_type: None, identity_stamp: None, source_agent: None, ..Default::default() },
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
        assert_eq!(kg.relation_count(), 0, "weak relation should be decayed away");
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
}
