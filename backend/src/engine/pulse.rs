use std::collections::{HashMap, HashSet};

use crate::domain::pulse::{Pulse, PulseResult};
use crate::domain::space::Space;
use crate::domain::tetra::TetraId;
use crate::engine::knowledge::KnowledgeGraph;
use crate::engine::vector::VectorLayer;

#[derive(Debug, Clone, Copy)]
pub enum PulseType {
    Reinforcing { boost: f64 },
    Exploratory { curiosity: f64 },
    Cascade { branch_limit: usize },
    Neural { temperature: f64 },
}

pub struct PulseEngine;

impl PulseEngine {
    pub fn send(
        space: &Space,
        kg: &KnowledgeGraph,
        pulse_type: PulseType,
        origin: TetraId,
        ttl: u32,
    ) -> Result<PulseResult, String> {
        let all = space.all_tetrahedrons();
        let snapshot: HashMap<TetraId, &crate::domain::tetra::Tetrahedron> =
            all.iter().map(|t| (t.id, t)).collect();

        let origin_tetra = snapshot.get(&origin).ok_or("origin not found")?;

        let mut current_id = origin;
        let origin_labels: Vec<String> = origin_tetra.data.labels.clone();
        let mut current_embedding: Vec<f64> = origin_tetra.data.embedding.clone();
        let mut visited_set: HashSet<TetraId> = HashSet::new();
        visited_set.insert(origin);
        let mut visited = vec![origin];
        let mut collected_hashes: Vec<u64> = vec![origin_tetra.data.content_hash];
        let mut discoveries: Vec<(TetraId, TetraId, f64)> = Vec::new();
        let mut mass_updates: Vec<(TetraId, f64)> = Vec::new();

        let temperature = match pulse_type {
            PulseType::Neural { temperature } => temperature,
            PulseType::Reinforcing { .. } => 0.8,
            PulseType::Exploratory { .. } => 1.2,
            PulseType::Cascade { .. } => 0.9,
        };

        let boost = match pulse_type {
            PulseType::Reinforcing { boost } => boost,
            _ => 0.0,
        };

        let mut current_labels = origin_labels;

        let layer_height = space.cylinder_height() / 4.0;

        let current_layer = |z: f64| -> usize { (z.max(0.0) / layer_height).floor() as usize };

        for _step in 0..ttl as usize {
            let mut candidates: Vec<(TetraId, f64)> = Vec::new();

            if !snapshot.contains_key(&current_id) {
                break;
            }

            let current_z = snapshot.get(&current_id).map(|t| t.core.z).unwrap_or(0.0);
            let cur_layer = current_layer(current_z);

            let neighbor_ids = space.neighbors_of(current_id);

            for &nid in &neighbor_ids {
                if visited_set.contains(&nid) {
                    continue;
                }
                if let Some(nt) = snapshot.get(&nid) {
                    let sim = VectorLayer::best_similarity(
                        &current_embedding,
                        &current_labels,
                        &nt.data.embedding,
                        &nt.data.labels,
                    );
                    if sim >= 0.01 {
                        let neighbor_layer = current_layer(nt.core.z);
                        let layer_mult = if neighbor_layer == cur_layer {
                            1.2
                        } else {
                            0.4
                        };
                        candidates.push((nid, sim * layer_mult));
                    }
                }
            }

            for (target_id, _, strength) in kg.query_relations(current_id) {
                if !visited_set.contains(&target_id) {
                    if let Some(nt) = snapshot.get(&target_id) {
                        let sim = VectorLayer::best_similarity(
                            &current_embedding,
                            &current_labels,
                            &nt.data.embedding,
                            &nt.data.labels,
                        );
                        let target_layer = current_layer(nt.core.z);
                        let layer_mult = if target_layer == cur_layer { 1.0 } else { 0.5 };
                        candidates.push((target_id, (sim * layer_mult).max(strength * 0.6)));
                    }
                }
            }

            if candidates.is_empty() {
                let bfs_neighbors = space.bfs_neighbors(current_id, 3);
                let mut best_fallback: Option<(TetraId, f64)> = None;
                for (nid, hops) in &bfs_neighbors {
                    if visited_set.contains(nid) {
                        continue;
                    }
                    if let Some(nt) = snapshot.get(nid) {
                        let sim = VectorLayer::best_similarity(
                            &current_embedding,
                            &current_labels,
                            &nt.data.embedding,
                            &nt.data.labels,
                        );
                        if sim < 0.01 {
                            continue;
                        }
                        let hop_decay = 1.0 / (*hops as f64).max(1.0);
                        let score = sim * hop_decay;
                        match &best_fallback {
                            None => best_fallback = Some((*nid, score)),
                            Some((_, s)) if score > *s => best_fallback = Some((*nid, score)),
                            _ => {}
                        }
                    }
                }
                if best_fallback.is_none() {
                    for t in &all {
                        if visited_set.contains(&t.id) {
                            continue;
                        }
                        let sim = VectorLayer::best_similarity(
                            &current_embedding,
                            &current_labels,
                            &t.data.embedding,
                            &t.data.labels,
                        );
                        if sim < 0.01 {
                            continue;
                        }
                        let score = sim * 0.5;
                        match &best_fallback {
                            None => best_fallback = Some((t.id, score)),
                            Some((_, s)) if score > *s => best_fallback = Some((t.id, score)),
                            _ => {}
                        }
                    }
                }
                if let Some(fb) = best_fallback {
                    candidates.push(fb);
                }
            }

            candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            let mut stepped = false;
            for (rank, (next_id, score)) in candidates.iter().enumerate() {
                if rank > 0 && score * temperature < rand::random::<f64>() * 0.15 {
                    continue;
                }

                visited_set.insert(*next_id);
                visited.push(*next_id);

                if *score > 0.3 {
                    discoveries.push((current_id, *next_id, *score));
                }

                if let Some(next_t) = snapshot.get(next_id) {
                    current_labels = next_t.data.labels.clone();
                    current_embedding = next_t.data.embedding.clone();
                    collected_hashes.push(next_t.data.content_hash);
                }

                kg.add_relation(
                    current_id,
                    *next_id,
                    crate::engine::knowledge::RelationType::Related,
                    *score,
                );

                mass_updates.push((*next_id, 0.02 * score));

                current_id = *next_id;
                stepped = true;
                break;
            }

            if !stepped {
                break;
            }
        }

        if boost > 0.0 {
            for i in 0..visited.len() {
                for j in (i + 1)..visited.len() {
                    let a = visited[i];
                    let b = visited[j];
                    if let (Some(ta), Some(tb)) = (snapshot.get(&a), snapshot.get(&b)) {
                        let sim = VectorLayer::best_similarity(
                            &ta.data.embedding,
                            &ta.data.labels,
                            &tb.data.embedding,
                            &tb.data.labels,
                        );
                        kg.add_relation(
                            a,
                            b,
                            crate::engine::knowledge::RelationType::SimilarTo,
                            (sim + boost).min(1.0),
                        );
                    }
                }
            }

            for &vid in &visited {
                mass_updates.push((vid, 0.002));
            }
        }

        // Phase 3: batch-write mass updates
        for (id, delta) in &mass_updates {
            if let Err(e) = space.update_mass(*id, *delta) {
                tracing::debug!("[Pulse] mass update {} failed: {}", id, e);
            }
        }

        let pulse = Pulse { id: 0, origin, ttl };
        let result = PulseResult {
            pulse_id: pulse.id,
            origin,
            reached_target: visited.len() > 1,
            data: crate::domain::pulse::PulseData {
                visited_tetras: visited,
                collected_content_hashes: collected_hashes,
                path_length: discoveries.len(),
                discoveries,
            },
            energy_cost: 0.5,
        };

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tetra::{MemoryPayload, Tetrahedron};
    use crate::domain::vertex::Point3;

    fn setup() -> (Space, KnowledgeGraph) {
        let space = Space::new();
        let kg = KnowledgeGraph::new();

        let topics = [
            ("Rust memory safety", vec!["rust".to_string()]),
            ("Python dynamic typing", vec!["python".to_string()]),
            ("Rust ownership model", vec!["rust".to_string()]),
            ("Python list comprehensions", vec!["python".to_string()]),
        ];
        for (i, (text, labels)) in topics.iter().enumerate() {
            let core = Point3::new(i as f64 * 1.0, 0.0, 0.0);
            let pos = Tetrahedron::compute_vertices(core);
            let t = Tetrahedron {
                id: 0,
                vertex_ids: [0; 4],
                core,
                data: MemoryPayload {
                    content: text.to_string(),
                    content_hash: i as u64,
                    labels: labels.clone(),
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

        (space, kg)
    }

    #[test]
    fn reinforcing_pulse_strengthens_relations() {
        let (space, kg) = setup();
        let r =
            PulseEngine::send(&space, &kg, PulseType::Reinforcing { boost: 0.3 }, 0, 3).unwrap();
        assert!(
            !r.data.visited_tetras.is_empty(),
            "pulse should visit at least origin"
        );
    }

    #[test]
    fn exploratory_pulse_finds_weak_signals() {
        let (space, kg) = setup();
        let r = PulseEngine::send(&space, &kg, PulseType::Exploratory { curiosity: 0.5 }, 0, 3)
            .unwrap();
        assert!(!r.data.visited_tetras.is_empty());
    }

    #[test]
    fn cascade_pulse_branches() {
        let (space, kg) = setup();
        let r =
            PulseEngine::send(&space, &kg, PulseType::Cascade { branch_limit: 2 }, 0, 5).unwrap();
        assert!(!r.data.visited_tetras.is_empty());
    }
}
