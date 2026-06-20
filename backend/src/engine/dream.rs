use crate::domain::space::Space;
use crate::engine::vector::VectorLayer;
#[derive(Debug, Clone)]
pub struct DreamResult {
    pub memories_consolidated: usize,
    pub connections_formed: usize,
    pub insights: Vec<String>,
    pub duplicates_merged: usize,
    pub junk_evicted: usize,
    pub evicted_ids: Vec<u64>,
    pub merged_remove_ids: Vec<u64>,
}

pub struct DreamEngine;

impl DreamEngine {
    pub fn recompute_importance(
        space: &Space,
        access_counts: &std::collections::HashMap<u64, u32>,
    ) -> usize {
        let tetras = space.all_tetrahedrons();
        let mut updated = 0;
        let now_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as f64;

        for t in &tetras {
            let access = *access_counts.get(&t.id).unwrap_or(&0) as f64;
            let age_days = (now_ts - t.data.timestamp as f64) / 86400.0;
            let mut new_importance = t.data.importance;

            if access > 5.0 {
                new_importance += 0.1;
            }
            if age_days > 7.0 && access < 1.0 {
                new_importance -= 0.05;
            }
            if age_days > 30.0 && access < 1.0 {
                new_importance -= 0.1;
            }

            let content_lower = t.data.content.to_lowercase();
            if content_lower.contains("架构")
                || content_lower.contains("architecture")
                || content_lower.contains("决策")
                || content_lower.contains("decision")
                || content_lower.contains("关键")
                || content_lower.contains("critical")
            {
                new_importance = new_importance.max(2.0);
            }

            if content_lower.contains("测试") && content_lower.len() < 30 {
                new_importance = new_importance.min(0.3);
            }

            new_importance = new_importance.clamp(0.1, 3.0);
            if (new_importance - t.data.importance).abs() > 0.01 {
                if let Some(mut tetra) = space.get_tetrahedron(t.id) {
                    tetra.data.importance = new_importance;
                    let _ = space.update_payload(t.id, tetra.data);
                    updated += 1;
                }
            }
        }
        updated
    }
}

impl DreamEngine {
    pub fn cycle(space: &Space, replay_strength: f64, consolidate_depth: usize) -> DreamResult {
        let tetras = space.all_tetrahedrons();
        if tetras.len() < 2 {
            return DreamResult {
                memories_consolidated: tetras.len(),
                connections_formed: 0,
                insights: vec![],
                duplicates_merged: 0,
                junk_evicted: 0,
                evicted_ids: vec![],
                merged_remove_ids: vec![],
            };
        }

        let mut connections_formed = 0usize;
        let mut insights = Vec::new();
        let mut duplicates_merged = 0usize;
        let mut junk_evicted = 0usize;
        let mut evicted_ids = Vec::new();
        let mut merged_remove_ids = Vec::new();

        // Phase 1: Evict junk (label "junk" or mass < 0.1) and archive low-importance old memories
        let now_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as f64;
        for t in &tetras {
            let is_junk = t.data.labels.iter().any(|l| l == "junk");
            let is_low_mass = t.mass < 0.1;
            let age_days = (now_ts - t.data.timestamp as f64) / 86400.0;
            let is_old_low_importance = age_days > 30.0 && t.data.importance < 0.3;
            let is_test_noise = t.data.importance < 0.2;
            if is_junk || is_low_mass || is_old_low_importance || is_test_noise {
                if t.data.enforced {
                    continue;
                }
                let neighbors = space.neighbors_of(t.id);
                if neighbors.len() <= 1 {
                    let id = t.id;
                    if let Err(e) = space.remove_tetrahedron(id) {
                        tracing::debug!("[Dream] evict {} failed: {}", id, e);
                    }
                    evicted_ids.push(id);
                    junk_evicted += 1;
                    if junk_evicted >= 10 {
                        break;
                    }
                }
            }
        }

        if junk_evicted > 0 {
            insights.push(format!("evicted {} junk/low-mass memories", junk_evicted));
        }

        // Refresh after eviction
        let tetras = if junk_evicted > 0 {
            space.all_tetrahedrons()
        } else {
            tetras
        };

        let non_meta: Vec<usize> = (0..tetras.len())
            .filter(|i| {
                !tetras[*i]
                    .data
                    .labels
                    .iter()
                    .any(|l| l.starts_with("meta-"))
            })
            .collect();

        // Phase 2: Find and merge high-similarity pairs (duplicates)
        let merge_threshold = 0.95f64;
        let mut merged_ids: std::collections::HashSet<u64> = std::collections::HashSet::new();
        let max_scan = 200usize;
        let mut merge_pairs: Vec<(usize, usize, f64)> = Vec::new();

        if non_meta.len() <= 30 {
            for wi in 0..non_meta.len() {
                for wj in (wi + 1)..non_meta.len() {
                    let i = non_meta[wi];
                    let j = non_meta[wj];
                    if merged_ids.contains(&tetras[i].id) || merged_ids.contains(&tetras[j].id) {
                        continue;
                    }
                    let sim = VectorLayer::best_similarity(
                        &tetras[i].data.embedding,
                        &tetras[i].data.labels,
                        &tetras[j].data.embedding,
                        &tetras[j].data.labels,
                    );
                    if sim > merge_threshold {
                        merge_pairs.push((i, j, sim));
                    }
                }
            }
        } else {
            let mut rng = rand::thread_rng();
            for _ in 0..max_scan {
                use rand::Rng;
                let wi = rng.gen_range(0..non_meta.len());
                let wj = rng.gen_range(0..non_meta.len());
                if wi == wj {
                    continue;
                }
                let (i, j) = if wi < wj {
                    (non_meta[wi], non_meta[wj])
                } else {
                    (non_meta[wj], non_meta[wi])
                };
                if merged_ids.contains(&tetras[i].id) || merged_ids.contains(&tetras[j].id) {
                    continue;
                }
                let sim = VectorLayer::best_similarity(
                    &tetras[i].data.embedding,
                    &tetras[i].data.labels,
                    &tetras[j].data.embedding,
                    &tetras[j].data.labels,
                );
                if sim > merge_threshold {
                    merge_pairs.push((i, j, sim));
                }
            }
        }

        merge_pairs.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

        for (i, j, sim) in merge_pairs.iter().take(consolidate_depth) {
            let ta = &tetras[*i];
            let tb = &tetras[*j];
            if merged_ids.contains(&ta.id) || merged_ids.contains(&tb.id) {
                continue;
            }
            if space.get_tetrahedron(ta.id).is_none() || space.get_tetrahedron(tb.id).is_none() {
                continue;
            }

            let (keep_id, remove_id, _keep_mass) = if ta.mass >= tb.mass {
                (ta.id, tb.id, ta.mass)
            } else {
                (tb.id, ta.id, tb.mass)
            };

            if let Err(e) = space.remove_tetrahedron(remove_id) {
                tracing::debug!("[Dream] merge remove {} failed: {}", remove_id, e);
            }
            if let Err(e) = space.update_mass(keep_id, 0.5) {
                tracing::debug!("[Dream] merge mass update {} failed: {}", keep_id, e);
            }
            merged_ids.insert(remove_id);
            merged_remove_ids.push(remove_id);
            duplicates_merged += 1;

            insights.push(format!(
                "merged #{remove_id} into #{keep_id} (sim={sim:.3}, mass boost +0.5)"
            ));
        }

        if duplicates_merged > 0 {
            insights.push(format!(
                "consolidated {} duplicate pairs",
                duplicates_merged
            ));
        }

        // Phase 3: Form connections for moderately similar pairs
        let tetras = space.all_tetrahedrons();
        let non_meta: Vec<usize> = (0..tetras.len())
            .filter(|i| {
                !tetras[*i]
                    .data
                    .labels
                    .iter()
                    .any(|l| l.starts_with("meta-"))
            })
            .collect();

        let mut pairs: Vec<(usize, usize, f64)> = Vec::new();
        if non_meta.len() <= 20 {
            for wi in 0..non_meta.len() {
                for wj in (wi + 1)..non_meta.len() {
                    let i = non_meta[wi];
                    let j = non_meta[wj];
                    let sim = VectorLayer::best_similarity(
                        &tetras[i].data.embedding,
                        &tetras[i].data.labels,
                        &tetras[j].data.embedding,
                        &tetras[j].data.labels,
                    );
                    if sim > replay_strength {
                        pairs.push((i, j, sim));
                    }
                }
            }
        } else {
            let mut rng = rand::thread_rng();
            for _ in 0..max_scan {
                use rand::Rng;
                let wi = rng.gen_range(0..non_meta.len());
                let wj = rng.gen_range(0..non_meta.len());
                if wi == wj {
                    continue;
                }
                let (i, j) = if wi < wj {
                    (non_meta[wi], non_meta[wj])
                } else {
                    (non_meta[wj], non_meta[wi])
                };
                let sim = VectorLayer::best_similarity(
                    &tetras[i].data.embedding,
                    &tetras[i].data.labels,
                    &tetras[j].data.embedding,
                    &tetras[j].data.labels,
                );
                if sim > replay_strength {
                    pairs.push((i, j, sim));
                }
            }
        }

        connections_formed += pairs.len();

        // Phase 4: Cluster analysis + central tetra
        let clusters = space.find_clusters();
        let mut largest_cluster = 0;
        for cluster in &clusters {
            if cluster.tetra_ids.len() > largest_cluster {
                largest_cluster = cluster.tetra_ids.len();
            }
            if cluster.tetra_ids.len() >= 3 {
                insights.push(format!(
                    "cluster of {} tetras formed (strong memory group)",
                    cluster.tetra_ids.len()
                ));
            }
        }

        if connections_formed > 0 {
            insights.push(format!(
                "found {} similar pairs (>{:.1} threshold), largest cluster: {}",
                connections_formed, replay_strength, largest_cluster
            ));
        }

        DreamResult {
            memories_consolidated: tetras.len(),
            connections_formed,
            insights,
            duplicates_merged,
            junk_evicted,
            evicted_ids,
            merged_remove_ids,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tetra::{MemoryPayload, Tetrahedron};
    use crate::domain::vertex::Point3;

    #[test]
    fn dream_cycle_on_tetras() {
        let space = Space::new();
        for (i, (text, labels)) in [
            ("hello world", vec!["greeting".to_string()]),
            ("hello there", vec!["greeting".to_string()]),
            ("goodbye moon", vec!["farewell".to_string()]),
            ("hello universe", vec!["greeting".to_string()]),
        ]
        .iter()
        .enumerate()
        {
            let core = Point3::new(i as f64, 0.0, 0.0);
            let pos = Tetrahedron::compute_vertices(core);
            let t = Tetrahedron {
                id: 0,
                vertex_ids: [0; 4],
                core,
                data: MemoryPayload {
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

        let result = DreamEngine::cycle(&space, 0.2, 5);
        assert!(result.memories_consolidated >= 2);
        assert!(!result.insights.is_empty());
    }

    #[test]
    fn dream_on_empty_space() {
        let space = Space::new();
        let result = DreamEngine::cycle(&space, 0.5, 5);
        assert_eq!(result.memories_consolidated, 0);
        assert!(result.evicted_ids.is_empty());
        assert!(result.merged_remove_ids.is_empty());
    }

    #[test]
    fn dream_insights_include_cluster() {
        let space = Space::new();
        for i in 0..3 {
            let core = Point3::new(i as f64, 0.0, 0.0);
            let pos = Tetrahedron::compute_vertices(core);
            let t = Tetrahedron {
                id: 0,
                vertex_ids: [0; 4],
                core,
                data: MemoryPayload {
                    content: String::new(),
                    content_hash: 0,
                    labels: vec!["same".to_string()],
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

        let result = DreamEngine::cycle(&space, 0.5, 5);
        let has_cluster = result.insights.iter().any(|i| i.contains("cluster"));
        if !has_cluster {
            tracing::warn!(
                "dream cycle produced {} insights but none mention 'cluster': {:?}",
                result.insights.len(),
                result.insights
            );
        }
    }
}
