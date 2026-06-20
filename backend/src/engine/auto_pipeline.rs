use std::collections::{HashMap, HashSet};

use crate::domain::space::{Cluster, Space};
use crate::domain::tetra::{TetraId, Tetrahedron};
use crate::domain::vertex::Point3;

use super::adaptive::AdaptiveParams;
use super::dream::{DreamEngine, DreamResult};
use super::dynamics;
use super::energy::EnergyCenter;
use super::gateway::GatewayCenter;
use super::knowledge::KnowledgeGraph;
use super::pulse::{PulseEngine, PulseType};
use super::storage::StorageManager;

pub struct AutoPipelineCtx<'a> {
    pub tick: u64,
    pub space: &'a Space,
    pub energy: &'a EnergyCenter,
    pub knowledge: &'a KnowledgeGraph,
    pub gateway: &'a GatewayCenter,
    pub storage: &'a StorageManager,
    pub emotion_pleasure: f64,
    pub emotion_arousal: f64,
    pub adaptive: &'a AdaptiveParams,
}

pub struct FissionResult {
    pub moved_count: usize,
    pub tick: u64,
}

pub struct AutoFissionOutcome {
    pub did_fission: bool,
    pub merge_pairs: Option<HashSet<(usize, usize)>>,
}

pub fn auto_pulse(
    ctx: &AutoPipelineCtx,
    tetras: &[Tetrahedron],
    clusters: &[Cluster],
    _core_map: &HashMap<u64, Point3>,
) -> u32 {
    if tetras.is_empty() || clusters.is_empty() {
        return 0;
    }

    let mut cluster_origins: Vec<(usize, Vec<(u64, f64)>)> = Vec::new();
    for (ci, cluster) in clusters.iter().enumerate() {
        if cluster.tetra_ids.len() < 2 {
            continue;
        }
        let mut origins: Vec<(u64, f64)> = cluster
            .tetra_ids
            .iter()
            .filter_map(|&tid| {
                let t = tetras.iter().find(|t| t.id == tid)?;
                if t.data.labels.iter().any(|l| l.starts_with("meta-")) {
                    return None;
                }
                Some((tid, t.mass))
            })
            .collect();
        origins.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        if !origins.is_empty() {
            cluster_origins.push((ci, origins));
        }
    }
    if cluster_origins.is_empty() {
        return 0;
    }

    let budget = cluster_origins
        .len()
        .min(ctx.adaptive.get_u(super::adaptive::Param::PulseBudget));
    let mut pulsed = 0u32;
    let tick_usize = ctx.tick as usize;

    for i in 0..budget {
        if !ctx.energy.consume(2.0) {
            break;
        }

        let cluster_slot = (tick_usize + i) % cluster_origins.len();
        let (_ci, ref origins) = cluster_origins[cluster_slot];
        let origin_slot = tick_usize % origins.len();
        let (origin, mass) = origins[origin_slot];
        let cluster_size = clusters
            .get(cluster_slot)
            .map(|c| c.tetra_ids.len())
            .unwrap_or(0);

        let ptype = if i == 0 {
            let temp = 0.9f64 * (1.0f64 + ctx.emotion_arousal.abs() * 0.2).min(1.5f64);
            PulseType::Neural { temperature: temp }
        } else {
            PulseType::Reinforcing { boost: 0.2 }
        };

        if let Ok(result) = PulseEngine::send(ctx.space, ctx.knowledge, ptype, origin, 12) {
            if result.data.visited_tetras.len() > 1 {
                tracing::info!(
                    "[AutoPulse] {} cluster({}) origin {} (mass={:.2}) → visited {} tetras",
                    if i == 0 { "Neural" } else { "Reinforcing" },
                    cluster_size,
                    origin,
                    mass,
                    result.data.visited_tetras.len()
                );
            }
            pulsed += 1;
        }
    }

    pulsed
}

pub fn auto_fission(
    ctx: &AutoPipelineCtx,
    clusters: &[Cluster],
    labels_map: &HashMap<u64, Vec<String>>,
    core_map: &HashMap<u64, Point3>,
    last_fission_tick: u64,
    last_merge_pairs: &HashSet<(usize, usize)>,
) -> AutoFissionOutcome {
    if ctx.tick.saturating_sub(last_fission_tick) < 10 {
        return AutoFissionOutcome {
            did_fission: false,
            merge_pairs: None,
        };
    }
    let mut sorted_clusters: Vec<(usize, usize)> = clusters
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            c.tetra_ids.len()
                >= ctx
                    .adaptive
                    .get_u(super::adaptive::Param::FissionMinClusterSize)
        })
        .map(|(i, c)| (i, c.tetra_ids.len()))
        .collect();
    sorted_clusters.sort_by_key(|b| std::cmp::Reverse(b.1));

    for (ci, size) in &sorted_clusters {
        let entropy = dynamics::compute_entropy_from_labels(&clusters[*ci].tetra_ids, labels_map);
        if (*size >= 30
            || entropy
                >= ctx
                    .adaptive
                    .get(super::adaptive::Param::FissionEntropyThreshold))
            && perform_fission_from_snap(
                ctx,
                *ci,
                0,
                8.0,
                "AutoFission",
                clusters,
                labels_map,
                core_map,
            )
            .is_some()
        {
            return AutoFissionOutcome {
                did_fission: true,
                merge_pairs: None,
            };
        }
    }

    if clusters.len() >= 2 {
        AutoFissionOutcome {
            did_fission: false,
            merge_pairs: Some(auto_merge(ctx, core_map, clusters, last_merge_pairs)),
        }
    } else {
        AutoFissionOutcome {
            did_fission: false,
            merge_pairs: None,
        }
    }
}

pub fn auto_merge(
    ctx: &AutoPipelineCtx,
    core_map: &HashMap<u64, Point3>,
    clusters: &[Cluster],
    last_merge_pairs: &HashSet<(usize, usize)>,
) -> HashSet<(usize, usize)> {
    let mut new_cooldown = HashSet::new();
    let mut merged = 0usize;
    let mut total_moved = 0usize;

    let mut pairs: Vec<(usize, usize, f64, f64)> = Vec::new();
    for i in 0..clusters.len() {
        for j in (i + 1)..clusters.len() {
            let ci = centroid_from_core_map(&clusters[i].tetra_ids, core_map);
            let cj = centroid_from_core_map(&clusters[j].tetra_ids, core_map);
            let dist = ci.distance_to(&cj);
            if dist >= ctx.adaptive.get(super::adaptive::Param::MergeDistance) {
                continue;
            }
            let label_sim = compute_cluster_label_similarity(&clusters[i], &clusters[j], ctx.space);
            if label_sim
                < ctx
                    .adaptive
                    .get(super::adaptive::Param::MergeLabelSimilarity)
            {
                continue;
            }
            let key = if i < j { (i, j) } else { (j, i) };
            if last_merge_pairs.contains(&key) {
                continue;
            }
            pairs.push((i, j, dist, label_sim));
        }
    }
    pairs.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

    for (i, j, _dist, _label_sim) in pairs.iter().take(5) {
        if !ctx.energy.consume(3.0) {
            break;
        }
        let small = if clusters[*i].tetra_ids.len() <= clusters[*j].tetra_ids.len() {
            *i
        } else {
            *j
        };
        let large = if small == *i { *j } else { *i };
        let target_centroid = centroid_from_core_map(&clusters[large].tetra_ids, core_map);

        let mut moved = 0usize;
        for &id in &clusters[small].tetra_ids {
            if let Some(original) = core_map.get(&id) {
                let dx = target_centroid.x - original.x;
                let dy = target_centroid.y - original.y;
                let dz = target_centroid.z - original.z;
                let d = (dx * dx + dy * dy + dz * dz).sqrt();
                if d < 0.5 {
                    continue;
                }
                let step = d * 0.6;
                let new_core = Point3::new(
                    original.x + dx / d.max(0.01) * step,
                    original.y + dy / d.max(0.01) * step,
                    original.z + dz / d.max(0.01) * step,
                );
                if ctx.space.relocate_tetrahedron(id, new_core).is_ok() {
                    persist_tetra_id(ctx.space, ctx.storage, ctx.gateway, id);
                    moved += 1;
                }
            }
        }
        if moved > 0 {
            let key = if *i < *j { (*i, *j) } else { (*j, *i) };
            new_cooldown.insert(key);
            merged += 1;
            total_moved += moved;
        }
    }

    if total_moved > 0 {
        ctx.gateway.rebuild_hnsw();
        tracing::info!(
            "[AutoMerge] {} pairs, moved {} tetras total",
            merged,
            total_moved
        );
    }

    new_cooldown
}

pub struct DreamOutcome {
    pub total_removed: usize,
    pub report: DreamResult,
}

pub fn auto_dream(
    ctx: &AutoPipelineCtx,
    last_dream_tick: u64,
    purge_fn: &dyn Fn(TetraId),
) -> Option<DreamOutcome> {
    if ctx.tick.saturating_sub(last_dream_tick)
        < ctx.adaptive.get_u(super::adaptive::Param::DreamInterval) as u64
    {
        return None;
    }
    if !ctx.energy.consume(15.0) {
        tracing::warn!("[AutoDream] insufficient energy");
        return None;
    }
    let report = DreamEngine::cycle(ctx.space, 0.3, 2);

    let total_removed = report.evicted_ids.len() + report.merged_remove_ids.len();
    for &id in report
        .evicted_ids
        .iter()
        .chain(report.merged_remove_ids.iter())
    {
        purge_fn(id);
    }
    if total_removed > 0 {
        tracing::info!(
            "[AutoDream] purged {} tetrahedrons (KG+HNSW+indexes cleaned)",
            total_removed
        );
    }

    tracing::info!(
        "[AutoDream] tick {} — consolidated {}, formed {} connections, {} insights, {} duplicates merged, {} junk evicted",
        ctx.tick,
        report.memories_consolidated,
        report.connections_formed,
        report.insights.len(),
        report.duplicates_merged,
        report.junk_evicted
    );
    for insight in &report.insights {
        tracing::info!("[AutoDream] insight: {}", insight);
    }

    let tetras = ctx.space.all_tetrahedrons();
    let label_data: Vec<(TetraId, Vec<String>)> = tetras
        .iter()
        .map(|t| (t.id, t.data.labels.clone()))
        .collect();
    ctx.knowledge.update_concepts(&label_data);

    Some(DreamOutcome {
        total_removed,
        report,
    })
}

pub fn evict_low_quality(
    ctx: &AutoPipelineCtx,
    tetras: &[Tetrahedron],
    purge_fn: &dyn Fn(TetraId),
) -> usize {
    if tetras.len() <= 50 {
        return 0;
    }

    let candidates: Vec<u64> = tetras
        .iter()
        .filter(|t| {
            let is_junk = t.data.labels.iter().any(|l| l == "junk");
            let is_auto = t.data.labels.iter().any(|l| l == "auto-extracted");
            let low_mass = t.mass
                < ctx
                    .adaptive
                    .get(super::adaptive::Param::EvictionMassThreshold);
            let is_test = t.data.content.len() < 20
                || t.data.content.starts_with("test ")
                || t.data.content.starts_with("persistence-test")
                || t.data.content.starts_with("[session] accomplished: test");
            is_junk || (is_auto && low_mass) || (low_mass && is_test)
        })
        .take(20)
        .map(|t| t.id)
        .collect();

    let mut evicted = 0;
    for &id in &candidates {
        let has_connections = !ctx.knowledge.query_relations(id).is_empty();
        if !has_connections {
            purge_fn(id);
            evicted += 1;
            if evicted >= 10 {
                break;
            }
        }
    }

    if evicted > 0 {
        tracing::info!(
            "[Scheduler] evicted {} low-quality memories (junk/auto-extracted/test)",
            evicted
        );
    }
    evicted
}

#[allow(clippy::too_many_arguments)]
pub fn perform_fission_from_snap(
    ctx: &AutoPipelineCtx,
    cluster_index: usize,
    _cooldown: u64,
    energy_cost: f64,
    tag: &str,
    clusters: &[Cluster],
    labels_map: &HashMap<u64, Vec<String>>,
    core_map: &HashMap<u64, Point3>,
) -> Option<FissionResult> {
    let tick = ctx.tick;
    let cluster = match clusters.get(cluster_index) {
        Some(c) => c,
        None => {
            tracing::warn!("[{}] fission: cluster {} not found", tag, cluster_index);
            return None;
        }
    };
    if cluster.tetra_ids.len() < 2 {
        return None;
    }
    let entropy = dynamics::compute_entropy_from_labels(&cluster.tetra_ids, labels_map);
    if !ctx.energy.consume(energy_cost) {
        tracing::warn!("[{}] fission: insufficient energy", tag);
        return None;
    }

    let mut label_map: HashMap<String, Vec<u64>> = HashMap::new();
    for &id in &cluster.tetra_ids {
        if let Some(labels) = labels_map.get(&id) {
            if labels
                .iter()
                .any(|l| l.starts_with("meta-") || l.starts_with("bridge"))
            {
                continue;
            }
            let key = labels.first().map(|s| s.as_str()).unwrap_or("general");
            label_map.entry(key.to_string()).or_default().push(id);
        }
    }
    let mut sorted_groups: Vec<(String, Vec<u64>)> = label_map.into_iter().collect();
    sorted_groups.sort_by_key(|b| std::cmp::Reverse(b.1.len()));
    if sorted_groups.len() < 2 {
        return None;
    }
    let dominant_label = &sorted_groups[0].0;
    let dominant_ids: HashSet<u64> = sorted_groups[0].1.iter().copied().collect();
    let minority_ids: Vec<u64> = sorted_groups
        .iter()
        .skip(1)
        .flat_map(|(_, ids)| ids.iter().copied())
        .collect();
    if minority_ids.is_empty() {
        return None;
    }

    let dominant_centroid =
        centroid_from_core_map(&dominant_ids.iter().copied().collect::<Vec<_>>(), core_map);
    let minority_centroid = centroid_from_core_map(&minority_ids, core_map);
    let mut moved_count: usize = 0;
    for &id in &minority_ids {
        if let Some(original) = core_map.get(&id) {
            let new_core = fission_placement(
                original,
                &dominant_centroid,
                &minority_centroid,
                ctx.space.tetra_count(),
            );
            match ctx.space.relocate_tetrahedron(id, new_core) {
                Ok(_) => {
                    moved_count += 1;
                    persist_tetra_id(ctx.space, ctx.storage, ctx.gateway, id);
                }
                Err(e) => tracing::warn!("[{}] fission move tetra {} failed: {}", tag, id, e),
            }
        }
    }

    let minority_labels: Vec<&str> = sorted_groups
        .iter()
        .skip(1)
        .map(|(l, _)| l.as_str())
        .take(3)
        .collect();
    if moved_count > 0 {
        ctx.gateway.rebuild_hnsw();
    }
    tracing::info!(
        "[{}] fission cluster {} COMPLETE: kept '{}' ({}), moved {} from [{}], entropy={:.3}",
        tag,
        cluster_index,
        dominant_label,
        dominant_ids.len(),
        moved_count,
        minority_labels.join(","),
        entropy
    );
    Some(FissionResult { moved_count, tick })
}

pub fn fission_placement(
    original: &Point3,
    larger_centroid: &Point3,
    smaller_centroid: &Point3,
    tetra_count: usize,
) -> Point3 {
    use crate::domain::tetra::EDGE_LENGTH;
    let dx = smaller_centroid.x - larger_centroid.x;
    let dy = smaller_centroid.y - larger_centroid.y;
    let dz = smaller_centroid.z - larger_centroid.z;
    let dist = (dx * dx + dy * dy + dz * dz).sqrt();
    let base_dist = EDGE_LENGTH * 10.0 * ((tetra_count as f64).sqrt().clamp(3.0, 20.0));
    let push_dist = base_dist.min(EDGE_LENGTH * 50.0);
    if dist < 1e-10 {
        return Point3::new(original.x + push_dist, original.y, original.z);
    }
    let scale = push_dist / dist;
    let push_dir = Point3::new(dx * scale, dy * scale, dz * scale);
    Point3::new(
        original.x + push_dir.x,
        original.y + push_dir.y,
        original.z + push_dir.z,
    )
}

fn persist_tetra_id(space: &Space, storage: &StorageManager, gateway: &GatewayCenter, id: TetraId) {
    if let Some(tetra) = space.get_tetrahedron(id) {
        if let Err(e) = storage.upsert_tetra(&tetra) {
            tracing::warn!("persist_tetra {} failed: {}", id, e);
        }
    }
    gateway.mark_dirty(id);
}

pub fn centroid_from_core_map(ids: &[u64], core_map: &HashMap<u64, Point3>) -> Point3 {
    let points: Vec<&Point3> = ids.iter().filter_map(|id| core_map.get(id)).collect();
    if points.is_empty() {
        return Point3::zero();
    }
    Point3::centroid(&points)
}

pub fn compute_cluster_label_similarity(ca: &Cluster, cb: &Cluster, space: &Space) -> f64 {
    let mut set_a: HashSet<String> = HashSet::new();
    for id in &ca.tetra_ids {
        if let Some(t) = space.get_tetrahedron(*id) {
            for l in t.data.labels {
                set_a.insert(l);
            }
        }
    }
    let mut set_b: HashSet<String> = HashSet::new();
    for id in &cb.tetra_ids {
        if let Some(t) = space.get_tetrahedron(*id) {
            for l in t.data.labels {
                set_b.insert(l);
            }
        }
    }
    if set_a.is_empty() && set_b.is_empty() {
        return 1.0;
    }
    let intersection = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();
    if union == 0 {
        return 0.0;
    }
    intersection as f64 / union as f64
}
