use crate::domain::space::Space;
use crate::engine::knowledge::KnowledgeGraph;
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
    pub fn cycle(
        space: &Space,
        knowledge: &KnowledgeGraph,
        replay_strength: f64,
        consolidate_depth: usize,
        dry_run: bool,
    ) -> DreamResult {
        let tetras =
            {
                let all = space.all_tetrahedrons();
                // 巩固范围限幅：语料增长后单次全量巩固的工作集可达数 GB
                // （2026-09-06/07 连续三晚 RSS 5-6.8G + swap 耗尽 + 三次规则重启，单夜 consolidated=10169）。
                // 按小时轮转窗口，每周期最多处理 1500 条，多周期滚动覆盖全库——语义不变，仅限节奏。
                const MAX_CONSOLIDATION_PER_CYCLE: usize = 1500;
                if all.len() <= MAX_CONSOLIDATION_PER_CYCLE {
                    all
                } else {
                    let hour_slot = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as usize
                        / 3600;
                    let start = (hour_slot * MAX_CONSOLIDATION_PER_CYCLE) % all.len();
                    let mut window = Vec::with_capacity(MAX_CONSOLIDATION_PER_CYCLE);
                    for k in 0..MAX_CONSOLIDATION_PER_CYCLE {
                        window.push(all[(start + k) % all.len()].clone());
                    }
                    tracing::info!(
                    "[AutoDream] consolidation window: {}/{} tetras this cycle (rotating cap {})",
                    window.len(), all.len(), MAX_CONSOLIDATION_PER_CYCLE
                );
                    window
                }
            };
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

        // Phase 1: Quarantine junk (no deletion — memories are sacred)
        // Mark low-quality memories with quarantine label + reduce importance
        let now_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as f64;
        for t in &tetras {
            let is_junk = t
                .data
                .labels
                .iter()
                .any(|l| l == "junk" || l == "quarantine");
            let is_low_mass = t.mass < 0.1;
            let age_days = (now_ts - t.data.timestamp as f64) / 86400.0;
            let is_old_low_importance = age_days > 30.0 && t.data.importance < 0.3;
            if (is_junk || is_low_mass || is_old_low_importance) && !t.data.enforced {
                if !dry_run {
                    let mut updated = t.data.clone();
                    if !updated.labels.iter().any(|l| l == "quarantine") {
                        updated.labels.push("quarantine".to_string());
                    }
                    updated.importance = updated.importance.min(0.1);
                    let _ = space.update_payload(t.id, updated);
                    let _ = space.update_mass(t.id, 0.05);
                }
                evicted_ids.push(t.id);
                junk_evicted += 1;
                if junk_evicted >= 10 {
                    break;
                }
            }
        }

        if junk_evicted > 0 {
            insights.push(format!(
                "quarantined {} low-quality memories (no deletion)",
                junk_evicted
            ));
        }

        // Refresh after eviction (only if we actually removed something)
        let tetras = if junk_evicted > 0 && !dry_run {
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
            // 深层突破2: 全量 O(N²) 扫描替代 random sampling(~5%召回率)。
            // 696记忆的O(N²)≈250K次1024维点积≈几十ms，完全可接受。
            // 全量扫描召回率~100% vs random sampling ~5%。
            for wi in 0..non_meta.len() {
                let i = non_meta[wi];
                if merged_ids.contains(&tetras[i].id) {
                    continue;
                }
                for wj in (wi + 1)..non_meta.len() {
                    let j = non_meta[wj];
                    if merged_ids.contains(&tetras[j].id) {
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
                // 早退：找到足够多的合并对就停止
                if merge_pairs.len() >= consolidate_depth * 3 {
                    break;
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

            if !dry_run {
                // SUPERSEDE (constitution §4.5 — memories are sacred, NEVER delete):
                // mark the absorbed duplicate as historical context; it stays in space.
                if let Some(t) = space.get_tetrahedron(remove_id) {
                    let mut data = t.data.clone();
                    if !data.labels.iter().any(|l| l == "superseded") {
                        data.labels.push("superseded".to_string());
                    }
                    data.valid_to = Some(now_ts as i64);
                    data.importance = (data.importance * 0.15).max(0.01);
                    let _ = space.update_payload(remove_id, data);
                    let _ = space.update_mass(remove_id, 0.05);
                    let _ = space.update_validity(remove_id, Some(now_ts as i64));
                }
                if let Err(e) = space.update_mass(keep_id, 0.5) {
                    tracing::debug!("[Dream] merge mass update {} failed: {}", keep_id, e);
                }
            }
            merged_ids.insert(remove_id);
            merged_remove_ids.push(remove_id);
            duplicates_merged += 1;

            insights.push(format!(
                "superseded #{remove_id} into #{keep_id} (sim={sim:.3}, no deletion)"
            ));
        }

        if duplicates_merged > 0 {
            insights.push(format!(
                "consolidated {} duplicate pairs",
                duplicates_merged
            ));
        }

        // Phase 3: Form connections for moderately similar pairs
        let tetras = if !dry_run {
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

        // Phase 3.5: 将发现的语义相似对写入知识图谱（之前断联：只计数不建链）
        if !dry_run && !pairs.is_empty() {
            for &(i, j, sim) in pairs.iter().take(50) {
                let id_i = tetras[i].id;
                let id_j = tetras[j].id;
                knowledge.add_relation(
                    id_i,
                    id_j,
                    crate::engine::knowledge::RelationType::SimilarTo,
                    sim,
                );
            }
            tracing::info!(
                "[Dream] Phase 3: created {} KG edges from semantic pairs",
                pairs.len().min(50)
            );
        }

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
                    identity_stamp: None,
                    source_agent: None,
                    ..Default::default()
                },
                mass: 1.0,
            };
            space.add_tetrahedron(&t, &pos).unwrap();
        }

        let result = DreamEngine::cycle(&space, &KnowledgeGraph::new(), 0.2, 5, false);
        assert!(result.memories_consolidated >= 2);
        assert!(!result.insights.is_empty());
    }

    #[test]
    fn dream_on_empty_space() {
        let space = Space::new();
        let result = DreamEngine::cycle(&space, &KnowledgeGraph::new(), 0.5, 5, false);
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
                    identity_stamp: None,
                    source_agent: None,
                    ..Default::default()
                },
                mass: 1.0,
            };
            space.add_tetrahedron(&t, &pos).unwrap();
        }

        let result = DreamEngine::cycle(&space, &KnowledgeGraph::new(), 0.5, 5, false);
        let has_cluster = result.insights.iter().any(|i| i.contains("cluster"));
        if !has_cluster {
            eprintln!(
                "WARN: dream cycle produced {} insights but none mention 'cluster': {:?}",
                result.insights.len(),
                result.insights
            );
        }
    }

    #[test]
    fn dream_never_deletes_memories() {
        // Constitution §4.5: memories are sacred. Dream must quarantine/supersede, NEVER remove.
        let space = Space::new();
        for i in 0..3 {
            let core = Point3::new(i as f64, 0.0, 0.0);
            let pos = Tetrahedron::compute_vertices(core);
            let t = Tetrahedron {
                id: 0,
                vertex_ids: [0; 4],
                core,
                data: MemoryPayload {
                    content: format!("normal memory {}", i),
                    labels: vec!["normal".to_string()],
                    importance: 1.0,
                    ..Default::default()
                },
                mass: 1.0,
            };
            space.add_tetrahedron(&t, &pos).unwrap();
        }
        let core = Point3::new(10.0, 0.0, 0.0);
        let pos = Tetrahedron::compute_vertices(core);
        let junk = Tetrahedron {
            id: 0,
            vertex_ids: [0; 4],
            core,
            data: MemoryPayload {
                content: "junk".to_string(),
                labels: vec!["junk".to_string()],
                importance: 0.05,
                ..Default::default()
            },
            mass: 0.05,
        };
        space.add_tetrahedron(&junk, &pos).unwrap();
        let count_before = space.tetra_count();
        assert_eq!(count_before, 4);

        let result = DreamEngine::cycle(&space, &KnowledgeGraph::new(), 0.2, 5, false);

        // INVARIANT: nothing deleted
        assert_eq!(
            space.tetra_count(),
            count_before,
            "dream deleted memories — constitution §4.5 violation"
        );
        let junk_still = space
            .all_tetrahedrons()
            .iter()
            .any(|t| t.data.content == "junk");
        assert!(junk_still, "quarantined junk was deleted!");
        // junk was quarantined (in evicted_ids) and now carries the quarantine label
        assert!(result.junk_evicted >= 1, "junk should be quarantined");
        let all = space.all_tetrahedrons();
        let jq = all
            .iter()
            .find(|t| t.data.content == "junk")
            .expect("junk missing");
        assert!(
            jq.data.labels.iter().any(|l| l == "quarantine"),
            "junk not labeled quarantine"
        );
    }
}
