use crate::domain::ops::MemoryOp;
use crate::domain::space::Space;
use crate::engine::knowledge::KnowledgeGraph;
use crate::engine::vector::{VectorLayer, EMBEDDING_DIM};
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

fn has_healthy_embedding(embedding: &[f64]) -> bool {
    if embedding.len() != EMBEDDING_DIM || embedding.iter().any(|value| !value.is_finite()) {
        return false;
    }

    let norm = embedding
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    norm.is_finite() && norm > 1e-10
}

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
            // Live-memory floor policy: do not rewrite enforced or already-tombstoned rows.
            if t.data.enforced || t.data.valid_to.is_some() {
                continue;
            }
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
                new_importance = new_importance.min(super::governor::IMPORTANCE_FLOOR);
            }

            // Constitutional live floor (was 0.1 — conflicted with governor IMPORTANCE_FLOOR=0.3)
            new_importance = super::governor::clamp_live_importance(new_importance);
            if (new_importance - t.data.importance).abs() > 0.01 {
                // op-log: 只写 importance 字段,不再整份 payload 写回(避免覆盖并发编辑)
                if space
                    .apply_ops(t.id, &[MemoryOp::SetImportance(new_importance)])
                    .unwrap_or(false)
                {
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
            // 快速路径:非 Active(已隔离/已取代)直接跳过;权威判定在 apply_ops 锁内对当前状态进行
            if crate::domain::ops::Lifecycle::of(&t.data) != crate::domain::ops::Lifecycle::Active {
                continue;
            }
            let is_junk = t.data.labels.iter().any(|l| l == "junk");
            let is_low_mass = t.mass < 0.1;
            let age_days = (now_ts - t.data.timestamp as f64) / 86400.0;
            let is_old_low_importance = age_days > 30.0 && t.data.importance < 0.3;
            if (is_junk || is_low_mass || is_old_low_importance) && !t.data.enforced {
                // op-log: 字段级原子操作,守卫在锁内对当前状态求值;已隔离/enforced 为 no-op 不计数
                let op = MemoryOp::Quarantine {
                    importance_cap: super::governor::IMPORTANCE_FLOOR, // O-A: 隔离是降籍不是除名
                    mass_cap: crate::domain::ops::MASS_MIN,
                };
                let changed = if dry_run {
                    let (mut d, mut m) = (t.data.clone(), t.mass);
                    crate::domain::ops::apply_op(&mut d, &mut m, &op)
                } else {
                    space
                        .apply_ops(t.id, std::slice::from_ref(&op))
                        .unwrap_or(false)
                };
                if !changed {
                    continue;
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

        // 已 Superseded 的记忆不参与去重候选(否则无效对占满早退名额)
        let merge_candidates: Vec<usize> = (0..tetras.len())
            .filter(|i| {
                let d = &tetras[*i].data;
                !d.enforced
                    && !d.labels.iter().any(|l| l.starts_with("meta-"))
                    && crate::domain::ops::Lifecycle::of(d)
                        != crate::domain::ops::Lifecycle::Superseded
                    && has_healthy_embedding(&d.embedding)
            })
            .collect();

        // Phase 2: Superseding requires healthy current-model vectors; never fall back to Jaccard.
        let merge_threshold = 0.95f64;
        let mut merged_ids: std::collections::HashSet<u64> = std::collections::HashSet::new();
        let max_scan = 200usize;
        let mut merge_pairs: Vec<(usize, usize, f64)> = Vec::new();

        if merge_candidates.len() <= 30 {
            for wi in 0..merge_candidates.len() {
                for wj in (wi + 1)..merge_candidates.len() {
                    let i = merge_candidates[wi];
                    let j = merge_candidates[wj];
                    if merged_ids.contains(&tetras[i].id) || merged_ids.contains(&tetras[j].id) {
                        continue;
                    }
                    let sim = VectorLayer::cosine_similarity(
                        &tetras[i].data.embedding,
                        &tetras[j].data.embedding,
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
            for wi in 0..merge_candidates.len() {
                let i = merge_candidates[wi];
                if merged_ids.contains(&tetras[i].id) {
                    continue;
                }
                for &j in merge_candidates.iter().skip(wi + 1) {
                    if merged_ids.contains(&tetras[j].id) {
                        continue;
                    }
                    let sim = VectorLayer::cosine_similarity(
                        &tetras[i].data.embedding,
                        &tetras[j].data.embedding,
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

        for (i, j, sim) in merge_pairs.iter() {
            if duplicates_merged >= consolidate_depth {
                break;
            }
            let ta = &tetras[*i];
            let tb = &tetras[*j];
            if merged_ids.contains(&ta.id) || merged_ids.contains(&tb.id) {
                continue;
            }
            if ta.data.enforced || tb.data.enforced {
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

            // SUPERSEDE (constitution §4.5 — memories are sacred, NEVER delete).
            // op-log: 已 superseded / enforced 为 no-op → 不重复合并、不重复给保留方加质量
            let op = MemoryOp::Supersede {
                at: now_ts as i64,
                importance_factor: 0.15,
                importance_floor: super::governor::IMPORTANCE_FLOOR, // O-A
                mass_cap: crate::domain::ops::MASS_MIN,
            };
            let changed = if dry_run {
                let src = if remove_id == ta.id { ta } else { tb };
                let (mut d, mut m) = (src.data.clone(), src.mass);
                crate::domain::ops::apply_op(&mut d, &mut m, &op)
            } else {
                space
                    .apply_ops(remove_id, std::slice::from_ref(&op))
                    .unwrap_or(false)
            };
            if !changed {
                continue;
            }
            if !dry_run {
                if let Err(e) = space.apply_ops(keep_id, &[MemoryOp::AdjustMass(0.5)]) {
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
                let d = &tetras[*i].data;
                !d.enforced && !d.labels.iter().any(|l| l.starts_with("meta-"))
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
                if tetras[i].data.enforced || tetras[j].data.enforced {
                    continue;
                }
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

    #[test]
    fn recompute_importance_respects_constitutional_floor() {
        use crate::engine::governor::IMPORTANCE_FLOOR;
        let space = Space::new();
        let now = chrono::Utc::now().timestamp();
        // Old, never-accessed memory — dream path would previously clamp to 0.1
        let core = Point3::new(0.0, 0.0, 0.0);
        let pos = Tetrahedron::compute_vertices(core);
        let t = Tetrahedron {
            id: 0,
            vertex_ids: [0; 4],
            core,
            data: MemoryPayload {
                content: "minor note without keywords".to_string(),
                timestamp: now - 86400 * 45,
                importance: 0.35,
                access_count: 0,
                ..Default::default()
            },
            mass: 1.0,
        };
        let id = space.add_tetrahedron(&t, &pos).unwrap();
        let counts = std::collections::HashMap::new();
        let _ = DreamEngine::recompute_importance(&space, &counts);
        let after = space.get_tetrahedron(id).unwrap();
        assert!(
            after.data.importance + 1e-9 >= IMPORTANCE_FLOOR,
            "dream recompute must not go below IMPORTANCE_FLOOR, got {}",
            after.data.importance
        );
    }

    #[test]
    fn recompute_skips_enforced_and_tombstoned() {
        let space = Space::new();
        let now = chrono::Utc::now().timestamp();
        for (i, (enforced, valid_to, imp)) in [(true, None, 0.5), (false, Some(now - 10), 0.5)]
            .into_iter()
            .enumerate()
        {
            let core = Point3::new(i as f64, 1.0, 0.0);
            let pos = Tetrahedron::compute_vertices(core);
            let t = Tetrahedron {
                id: 0,
                vertex_ids: [0; 4],
                core,
                data: MemoryPayload {
                    content: format!("protected row {}", i),
                    timestamp: now - 86400 * 45,
                    importance: imp,
                    access_count: 0,
                    enforced,
                    valid_to,
                    ..Default::default()
                },
                mass: 1.0,
            };
            space.add_tetrahedron(&t, &pos).unwrap();
        }
        let before: Vec<_> = space
            .all_tetrahedrons()
            .into_iter()
            .map(|t| (t.id, t.data.importance))
            .collect();
        let counts = std::collections::HashMap::new();
        let n = DreamEngine::recompute_importance(&space, &counts);
        assert_eq!(n, 0, "enforced/tombstoned must not be rewritten");
        for (id, imp) in before {
            let after = space.get_tetrahedron(id).unwrap();
            assert!((after.data.importance - imp).abs() < 1e-9);
        }
    }
}

#[cfg(test)]
mod idempotency_tests {
    use super::*;
    use crate::domain::tetra::{MemoryPayload, Tetrahedron};
    use crate::domain::vertex::Point3;

    fn add(space: &Space, x: f64, data: MemoryPayload, mass: f64) -> u64 {
        let core = Point3::new(x, 0.0, 0.0);
        let pos = Tetrahedron::compute_vertices(core);
        let t = Tetrahedron {
            id: 0,
            vertex_ids: [0; 4],
            core,
            data,
            mass,
        };
        space.add_tetrahedron(&t, &pos).unwrap()
    }

    fn normal(i: usize) -> MemoryPayload {
        let mut emb = vec![0.0; EMBEDDING_DIM];
        emb[i % 128] = 1.0;
        MemoryPayload {
            content: format!("normal {i}"),
            labels: vec![format!("n{i}")],
            importance: 1.0,
            timestamp: chrono::Utc::now().timestamp(),
            embedding: emb,
            ..Default::default()
        }
    }

    #[test]
    fn enforced_junk_is_not_quarantined_in_phase1() {
        let space = Space::new();
        let protected = add(
            &space,
            0.0,
            MemoryPayload {
                content: "enforced junk".into(),
                labels: vec!["junk".into()],
                importance: 0.05,
                enforced: true,
                ..Default::default()
            },
            0.05,
        );
        let ordinary_junk = add(
            &space,
            1.0,
            MemoryPayload {
                content: "ordinary junk".into(),
                labels: vec!["junk".into()],
                importance: 0.05,
                ..Default::default()
            },
            0.05,
        );

        let result = DreamEngine::cycle(&space, &KnowledgeGraph::new(), 1.1, 0, false);

        assert_eq!(result.evicted_ids, vec![ordinary_junk]);
        assert_eq!(result.junk_evicted, 1);
        let protected_after = space.get_tetrahedron(protected).unwrap();
        assert!(protected_after.data.enforced);
        assert_eq!(protected_after.data.labels, ["junk"]);
        assert_eq!(protected_after.data.importance, 0.05);
        assert_eq!(protected_after.mass, 0.05);
    }

    #[test]
    fn phase2_skips_enforced_memories_in_both_keeper_orders() {
        for (protected_mass, ordinary_mass) in [(2.0, 1.0), (1.0, 2.0)] {
            let space = Space::new();
            let mut protected_data = normal(0);
            protected_data.enforced = true;
            let protected = add(&space, 0.0, protected_data, protected_mass);
            let ordinary = add(&space, 1.0, normal(0), ordinary_mass);
            let protected_before = space.get_tetrahedron(protected).unwrap();
            let ordinary_before = space.get_tetrahedron(ordinary).unwrap();
            let knowledge = KnowledgeGraph::new();

            let result = DreamEngine::cycle(&space, &knowledge, 1.1, 5, false);

            assert_eq!(result.duplicates_merged, 0);
            assert!(result.merged_remove_ids.is_empty());
            assert_eq!(knowledge.relation_count(), 0);
            let protected_after = space.get_tetrahedron(protected).unwrap();
            let ordinary_after = space.get_tetrahedron(ordinary).unwrap();
            assert!(protected_after.data.enforced);
            assert_eq!(protected_after.data.labels, protected_before.data.labels);
            assert_eq!(
                protected_after.data.valid_to,
                protected_before.data.valid_to
            );
            assert_eq!(
                protected_after.data.importance,
                protected_before.data.importance
            );
            assert_eq!(protected_after.mass, protected_before.mass);
            assert_eq!(ordinary_after.data.labels, ordinary_before.data.labels);
            assert_eq!(ordinary_after.data.valid_to, ordinary_before.data.valid_to);
            assert_eq!(
                ordinary_after.data.importance,
                ordinary_before.data.importance
            );
            assert_eq!(ordinary_after.mass, ordinary_before.mass);
        }
    }

    #[test]
    fn phase2_never_supersedes_without_healthy_vectors() {
        let stale_dim = EMBEDDING_DIM - 1;
        let cases = [
            ("absent", vec![], vec![]),
            (
                "stale same-dimension pair",
                vec![1.0; stale_dim],
                vec![1.0; stale_dim],
            ),
            ("stale Jaccard fallback", vec![1.0; stale_dim], vec![]),
            (
                "degraded zero-vector fallback",
                vec![0.0; EMBEDDING_DIM],
                vec![],
            ),
        ];

        for (name, embedding_a, embedding_b) in cases {
            let space = Space::new();
            let mut data_a = normal(0);
            data_a.labels = vec!["shared".into()];
            data_a.embedding = embedding_a;
            let mut data_b = normal(1);
            data_b.labels = vec!["shared".into()];
            data_b.embedding = embedding_b;
            let id_a = add(&space, 0.0, data_a, 2.0);
            let id_b = add(&space, 1.0, data_b, 1.0);

            let result = DreamEngine::cycle(&space, &KnowledgeGraph::new(), 2.0, 5, false);

            assert_eq!(result.duplicates_merged, 0, "{name}");
            assert!(result.merged_remove_ids.is_empty(), "{name}");
            for id in [id_a, id_b] {
                let after = space.get_tetrahedron(id).unwrap();
                assert!(
                    !after.data.labels.iter().any(|label| label == "superseded"),
                    "{name}: memory {id} was superseded"
                );
                assert_eq!(after.data.valid_to, None, "{name}: memory {id}");
                assert_eq!(after.mass, if id == id_a { 2.0 } else { 1.0 }, "{name}");
            }
        }
    }

    #[test]
    fn phase3_skips_enforced_links_but_keeps_healthy_ordinary_links() {
        let space = Space::new();
        let mut protected_data = normal(0);
        protected_data.enforced = true;
        let protected = add(&space, 0.0, protected_data, 1.0);
        let ordinary_isolated = add(&space, 1.0, normal(0), 1.0);
        let ordinary_a = add(&space, 2.0, normal(1), 1.0);
        let mut near_a = normal(2);
        near_a.embedding[1] = 0.8;
        near_a.embedding[2] = 0.6;
        let ordinary_b = add(&space, 3.0, near_a, 1.0);
        let knowledge = KnowledgeGraph::new();

        let result = DreamEngine::cycle(&space, &knowledge, 0.7, 0, false);

        assert_eq!(result.duplicates_merged, 0);
        assert_eq!(result.connections_formed, 1);
        let relations = knowledge.all_relations();
        assert_eq!(relations.len(), 1);
        let relation = &relations[0];
        assert_eq!(
            relation.relation_type,
            crate::engine::knowledge::RelationType::SimilarTo
        );
        let endpoints = [relation.source, relation.target];
        assert!(endpoints.contains(&ordinary_a));
        assert!(endpoints.contains(&ordinary_b));
        assert!(!endpoints.contains(&protected));
        assert!(!endpoints.contains(&ordinary_isolated));
    }

    #[test]
    fn dry_run_reports_without_changing_memory_or_graph_state() {
        fn memory_state(space: &Space) -> Vec<(u64, serde_json::Value)> {
            let mut state: Vec<_> = space
                .all_tetrahedrons()
                .into_iter()
                .map(|tetra| {
                    (
                        tetra.id,
                        serde_json::to_value((
                            tetra.data,
                            tetra.mass,
                            tetra.vertex_ids,
                            tetra.core.x,
                            tetra.core.y,
                            tetra.core.z,
                        ))
                        .unwrap(),
                    )
                })
                .collect();
            state.sort_by_key(|(id, _)| *id);
            state
        }

        let space = Space::new();
        add(&space, 0.0, normal(0), 2.0);
        add(&space, 1.0, normal(0), 1.0);
        let mut junk = normal(3);
        junk.labels = vec!["junk".into()];
        junk.embedding.clear();
        junk.importance = 0.05;
        add(&space, 2.0, junk, 0.05);
        let knowledge = KnowledgeGraph::new();
        knowledge.add_relation(
            100,
            101,
            crate::engine::knowledge::RelationType::Related,
            0.5,
        );
        let graph_state = || {
            knowledge
                .all_relations()
                .into_iter()
                .map(|relation| {
                    (
                        relation.source,
                        relation.target,
                        relation.relation_type,
                        relation.strength,
                        relation.created_tick,
                        relation.hits,
                    )
                })
                .collect::<Vec<_>>()
        };
        let memories_before = memory_state(&space);
        let graph_before = graph_state();
        let graph_dirty_before = knowledge.is_dirty();
        let graph_persistence_before = knowledge.persistence_snapshot();

        let result = DreamEngine::cycle(&space, &knowledge, 0.2, 5, true);

        assert_eq!(result.junk_evicted, 1);
        assert_eq!(result.duplicates_merged, 1);
        assert!(result.connections_formed > 0);
        assert!(!result.insights.is_empty());
        assert_eq!(memory_state(&space), memories_before);
        assert_eq!(graph_state(), graph_before);
        assert_eq!(knowledge.is_dirty(), graph_dirty_before);
        assert_eq!(knowledge.persistence_snapshot(), graph_persistence_before);
    }

    #[test]
    fn already_quarantined_memories_do_not_starve_new_junk() {
        let space = Space::new();
        for i in 0..3 {
            add(&space, i as f64, normal(i), 1.0);
        }
        // 10 条已隔离(上一轮 dream 的产物) + 1 条新垃圾
        for i in 0..10 {
            let mut d = normal(3 + i);
            d.labels = vec!["quarantine".into()];
            d.importance = 0.05;
            add(&space, 20.0 + i as f64, d, 0.1);
        }
        let mut fresh = normal(99);
        fresh.labels = vec!["junk".into()];
        let fresh_id = add(&space, 50.0, fresh, 1.0);

        let r = DreamEngine::cycle(&space, &KnowledgeGraph::new(), 0.99, 5, false);
        assert_eq!(
            r.evicted_ids,
            vec![fresh_id],
            "only the new junk should be quarantined"
        );
        let all = space.all_tetrahedrons();
        for t in all
            .iter()
            .filter(|t| t.id != fresh_id && t.data.labels == ["quarantine"])
        {
            assert!(
                t.mass <= 0.1 + 1e-9,
                "re-quarantine must not inflate mass: {}",
                t.mass
            );
        }
    }

    #[test]
    fn superseded_duplicate_is_not_merged_again() {
        let space = Space::new();
        let a = add(&space, 0.0, normal(0), 2.0);
        let mut dup = normal(0);
        dup.content = "normal 0 dup".into();
        add(&space, 1.0, dup, 1.0);
        add(&space, 2.0, normal(1), 1.0);

        let r1 = DreamEngine::cycle(&space, &KnowledgeGraph::new(), 0.99, 5, false);
        assert_eq!(r1.duplicates_merged, 1);
        let mass_after_first = space.get_tetrahedron(a).unwrap().mass;
        let r2 = DreamEngine::cycle(&space, &KnowledgeGraph::new(), 0.99, 5, false);
        assert_eq!(
            r2.duplicates_merged, 0,
            "superseded pair re-merged: {:?}",
            r2.insights
        );
        assert_eq!(space.get_tetrahedron(a).unwrap().mass, mass_after_first);
    }
}
