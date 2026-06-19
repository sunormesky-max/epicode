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
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RelationType {
    SimilarTo,
    Contradicts,
    Precedes,
    Contains,
    Related,
}

impl std::fmt::Display for RelationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RelationType::SimilarTo => write!(f, "similar"),
            RelationType::Contradicts => write!(f, "contradicts"),
            RelationType::Precedes => write!(f, "precedes"),
            RelationType::Contains => write!(f, "contains"),
            RelationType::Related => write!(f, "related"),
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
    adj_index: RwLock<HashMap<TetraId, Vec<usize>>>,
    concepts: RwLock<Vec<ConceptPrototype>>,
    dirty: std::sync::atomic::AtomicBool,
}

impl KnowledgeGraph {
    pub fn new() -> Self {
        Self {
            relations: RwLock::new(Vec::new()),
            adj_index: RwLock::new(HashMap::new()),
            concepts: RwLock::new(Vec::new()),
            dirty: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn clear_dirty(&self) {
        self.dirty
            .store(false, std::sync::atomic::Ordering::Relaxed);
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
        let exists = relations.iter().any(|r| {
            (r.source == source && r.target == target || r.source == target && r.target == source)
                && r.relation_type == rel_type
        });
        if exists {
            return;
        }
        {
            let adj = self.adj_index.read();
            let src_count = adj.get(&source).map(|v| v.len()).unwrap_or(0);
            let tgt_count = adj.get(&target).map(|v| v.len()).unwrap_or(0);
            if src_count >= MAX_RELATIONS_PER_NODE || tgt_count >= MAX_RELATIONS_PER_NODE {
                return;
            }
        }
        relations.push(Relation {
            source,
            target,
            relation_type: rel_type,
            strength,
            created_tick: tick,
        });
        *self.adj_index.write() = self.rebuild_adj_index(&relations);
        self.dirty.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn remove_relations_for(&self, id: TetraId) {
        let mut relations = self.relations.write();
        let before = relations.len();
        relations.retain(|r| r.source != id && r.target != id);
        if relations.len() < before {
            *self.adj_index.write() = self.rebuild_adj_index(&relations);
            self.dirty.store(true, std::sync::atomic::Ordering::Relaxed);
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

    pub fn multi_hop(&self, seeds: &[TetraId], max_hops: usize) -> Vec<(TetraId, f64)> {
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

    pub fn update_concepts(&self, tetras: &[(TetraId, Vec<String>)]) {
        let mut concepts = self.concepts.write();

        for &(id, ref labels) in tetras {
            let mut best: Option<(usize, f64)> = None;
            for (i, c) in concepts.iter().enumerate() {
                let sim = label_jaccard(
                    labels,
                    &c.member_ids,
                    &self.relations.read(),
                    &self.adj_index.read(),
                );
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
                    let next_id = concepts.len() as u64;
                    let label = labels
                        .first()
                        .cloned()
                        .unwrap_or_else(|| format!("concept_{}", next_id));
                    concepts.push(ConceptPrototype {
                        id: next_id,
                        centroid: vec![],
                        member_count: 1,
                        label,
                        member_ids: vec![id],
                    });
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
        let mut labeled: Vec<(String, u64)> = concepts
            .iter()
            .map(|c| (c.label.clone(), c.member_count))
            .collect();
        labeled.sort_by(|a, b| b.1.cmp(&a.1));
        labeled.truncate(limit);
        labeled
    }

    pub fn export_graph(&self, space: &Space) -> GraphExport {
        let relations = self.relations.read();
        let concepts = self.concepts.read();
        let tetras = space.all_tetrahedrons();

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
            })
            .collect();

        let concept_exports: Vec<ConceptExport> = concepts
            .iter()
            .map(|c| ConceptExport {
                id: c.id,
                label: c.label.clone(),
                member_count: c.member_count,
                member_ids: c.member_ids.clone(),
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
        top_labels.sort_by(|a, b| b.1.cmp(&a.1));
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
                sorted.sort_by(|a, b| b.1.cmp(&a.1));
                ClusterExport {
                    size: c.tetra_ids.len(),
                    member_ids: c.tetra_ids.clone(),
                    top_labels: sorted
                        .iter()
                        .take(3)
                        .map(|(l, c)| serde_json::json!({"label": l, "count": c}))
                        .collect(),
                }
            })
            .collect();

        let mut inter_cluster_edges: Vec<GraphEdgeExport> = Vec::new();
        for i in 0..clusters.len() {
            for j in (i + 1)..clusters.len() {
                let set_i: HashSet<TetraId> = clusters[i].tetra_ids.iter().copied().collect();
                let count = relations
                    .iter()
                    .filter(|r| {
                        (set_i.contains(&r.source) && clusters[j].tetra_ids.contains(&r.target))
                            || (set_i.contains(&r.target)
                                && clusters[j].tetra_ids.contains(&r.source))
                    })
                    .count();
                if count > 0 {
                    inter_cluster_edges.push(GraphEdgeExport {
                        source: clusters[i].tetra_ids.first().copied().unwrap_or(0),
                        target: clusters[j].tetra_ids.first().copied().unwrap_or(0),
                        relation_type: "inter_cluster".to_string(),
                        strength: count as f64,
                    });
                }
            }
        }

        GraphExport {
            nodes: node_map.into_iter().map(|(_, v)| v).collect(),
            edges: edge_exports,
            inter_cluster_edges,
            concepts: concept_exports,
            clusters: cluster_exports,
            top_labels: top_labels
                .into_iter()
                .map(|(l, c)| serde_json::json!({"label": l, "count": c}))
                .collect(),
            total_nodes: tetras.len(),
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

fn label_jaccard(
    labels: &[String],
    concept_member_ids: &[TetraId],
    _relations: &[Relation],
    _adj: &HashMap<TetraId, Vec<usize>>,
) -> f64 {
    if concept_member_ids.is_empty() || labels.is_empty() {
        return 0.0;
    }
    if labels.len() == 1 {
        return if concept_member_ids.len() > 0 {
            0.6
        } else {
            0.0
        };
    }
    let label_set: HashSet<&str> = labels.iter().map(|s| s.as_str()).collect();
    let concept_labels: HashSet<String> = labels.iter().cloned().collect();
    let concept_set: HashSet<&str> = concept_labels.iter().map(|s| s.as_str()).collect();
    let intersection = label_set.intersection(&concept_set).count() as f64;
    let union = label_set.union(&concept_set).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
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
}
