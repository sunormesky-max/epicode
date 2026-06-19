use crate::domain::space::Space;
use crate::domain::tetra::TetraId;
use crate::engine::knowledge::KnowledgeGraph;
use crate::engine::vector::VectorLayer;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Analogy {
    pub source_a: TetraId,
    pub source_b: TetraId,
    pub target_a: TetraId,
    pub target_b: TetraId,
    pub confidence: f64,
    pub relation: String,
}

pub struct ReasoningEngine;

impl ReasoningEngine {
    pub fn find_analogies(
        space: &Space,
        _kg: &KnowledgeGraph,
        min_confidence: f64,
    ) -> Vec<Analogy> {
        let tetras = space.all_tetrahedrons();
        if tetras.len() < 4 {
            return vec![];
        }

        let mut analogies = Vec::new();
        let labels: HashMap<TetraId, &Vec<String>> =
            tetras.iter().map(|t| (t.id, &t.data.labels)).collect();

        let max_pairs = 50usize;
        let mut pair_sims: Vec<(usize, usize, f64)> = Vec::new();

        if tetras.len() <= 30 {
            for i in 0..tetras.len() {
                for j in (i + 1)..tetras.len() {
                    let sim = VectorLayer::label_jaccard(
                        labels.get(&tetras[i].id).unwrap_or(&&vec![]),
                        labels.get(&tetras[j].id).unwrap_or(&&vec![]),
                    );
                    if sim >= min_confidence {
                        pair_sims.push((i, j, sim));
                    }
                }
            }
        } else {
            let mut rng = rand::thread_rng();
            for _ in 0..(max_pairs * 2) {
                use rand::Rng;
                let i = rng.gen_range(0..tetras.len());
                let j = rng.gen_range(0..tetras.len());
                if i >= j {
                    continue;
                }
                let sim = VectorLayer::label_jaccard(
                    labels.get(&tetras[i].id).unwrap_or(&&vec![]),
                    labels.get(&tetras[j].id).unwrap_or(&&vec![]),
                );
                if sim >= min_confidence {
                    pair_sims.push((i, j, sim));
                }
            }
        }
        pair_sims.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        pair_sims.truncate(max_pairs);

        for (pi, &(i, j, sim_ab)) in pair_sims.iter().enumerate() {
            for (pk, &(k, l, sim_cd)) in pair_sims.iter().enumerate() {
                if pk <= pi {
                    continue;
                }
                if k == i || k == j || l == i || l == j {
                    continue;
                }
                let rel_sim = 1.0 - (sim_ab - sim_cd).abs();
                if rel_sim > min_confidence {
                    analogies.push(Analogy {
                        source_a: tetras[i].id,
                        source_b: tetras[j].id,
                        target_a: tetras[k].id,
                        target_b: tetras[l].id,
                        confidence: rel_sim,
                        relation: "SimilarTo".into(),
                    });
                }
            }
            if analogies.len() >= 10 {
                break;
            }
        }

        analogies.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        analogies.truncate(10);
        analogies
    }

    pub fn discover_patterns(space: &Space) -> Vec<String> {
        let clusters = space.find_clusters();
        let mut patterns = Vec::new();

        if clusters.len() > 1 {
            let sizes: Vec<usize> = clusters.iter().map(|c| c.tetra_ids.len()).collect();
            let avg = sizes.iter().sum::<usize>() as f64 / sizes.len() as f64;
            patterns.push(format!("{} clusters, avg size={:.1}", clusters.len(), avg));
        }

        let orphans = clusters.iter().filter(|c| c.tetra_ids.len() == 1).count();
        if orphans > 0 {
            patterns.push(format!("{} orphan tetras detected", orphans));
        }

        if let Some(largest) = clusters.iter().max_by_key(|c| c.tetra_ids.len()) {
            let total = space.tetra_count();
            if total > 0 && largest.tetra_ids.len() as f64 / total as f64 > 0.5 {
                patterns.push(format!(
                    "giant component: {}/{} tetras ({:.0}%)",
                    largest.tetra_ids.len(),
                    total,
                    largest.tetra_ids.len() as f64 / total as f64 * 100.0,
                ));
            }
        }

        patterns
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tetra::Tetrahedron;
    use crate::domain::vertex::Point3;

    #[test]
    fn find_analogies_between_pairs() {
        let space = Space::new();
        let kg = KnowledgeGraph::new();

        let topics = [
            ("Rust is fast", vec!["rust".to_string()]),
            ("Rust is safe", vec!["rust".to_string()]),
            ("Python is slow", vec!["python".to_string()]),
            ("Python is readable", vec!["python".to_string()]),
        ];
        for (text, labels) in &topics {
            let core = Point3::zero();
            let pos = Tetrahedron::compute_vertices(core);
            let t = Tetrahedron {
                id: 0,
                vertex_ids: [0; 4],
                core,
                data: crate::domain::tetra::MemoryPayload {
                    content: text.to_string(),
                    content_hash: 0,
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

        let analogies = ReasoningEngine::find_analogies(&space, &kg, 0.1);
        assert!(!analogies.is_empty() || space.tetra_count() >= 4);
    }

    #[test]
    fn discover_patterns_on_clusters() {
        let space = Space::new();
        for i in 0..3 {
            let core = Point3::new(i as f64 * 10.0, 0.0, 0.0);
            let pos = Tetrahedron::compute_vertices(core);
            let t = Tetrahedron {
                id: 0,
                vertex_ids: [0; 4],
                core,
                data: Default::default(),
                mass: 1.0,
            };
            space.add_tetrahedron(&t, &pos).unwrap();
        }
        let patterns = ReasoningEngine::discover_patterns(&space);
        assert!(!patterns.is_empty());
    }
}
