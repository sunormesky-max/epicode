//! Geometry-reading retrieval (v0.2) — the thing epicode's 3D space never
//! had ("三条检索路径从不读空间坐标", 11-08 report). Here the space IS the
//! index:
//!
//! * **as_of slice** — bitemporal snapshot as a hyperplane cut, with Zep
//!   shadow semantics (`Validity::BelievedStale` inside the
//!   [expired, invalidated) band).
//! * **era neighborhood** — cells whose cores sit within `tol` w-units of a
//!   query stratum (era browsing).
//! * **Space-PPR** — Personalized PageRank over the vertex-sharing graph,
//!   seeded by label match × era proximity, damped 0.15/0.85 with epicode's
//!   fusion constants (0.6·ppr + 0.4·seed). Because sharing is era-quantized
//!   (T1′), diffusion is naturally band-local: rank flows within a
//!   (topic, era) stratum and leaks across eras only through genuine
//!   cross-era dockings.
//!
//! All signals are geometric — no embeddings involved. This is a NEW
//! retrieval channel complementary to HNSW/BM25, not a replacement.

use std::collections::HashMap;

use crate::engine::penta::Pentachoron;
use crate::engine::space4::{PentaId, SnapshotCell, Space4};
use crate::engine::time_axis::{valid_at, w_of_unix_secs};

pub use crate::engine::space4::Validity;

/// PPR damping seed weight (epicode: rank = 0.15·seed + 0.85·M·rank).
const PPR_SEED: f64 = 0.15;
/// PPR iterations (epicode PPR_ITER = 15).
const PPR_ITER: usize = 15;
/// BFS frontier cap for the diffusion subgraph (epicode PPR_FRONTIER = 2000).
const PPR_FRONTIER: usize = 800;
/// Final fusion (epicode: 0.6·ppr_norm + 0.4·seed_norm).
const FUSION_PPR: f64 = 0.6;
/// Era proximity falloff: exp(−ERA_BETA·|Δw|) — one era ≈ ⅛ the weight.
const ERA_BETA: f64 = 2.0;
/// v0.3 影子带降权：BelievedStale 细胞作为 PPR 种子时权重 ×0.5
///（v0.2 报告 §7 明示的下一步）。降权作用于种子值本身，因此
/// PPR 注入（rank += 0.15·seed）、经扩散携带的 rank 与最终融合
///（0.4·seed_norm）同步减半 —— 切片返回 BelievedStale 的同时，
/// 它在扩散中的话语权也被压低：不确定的记忆仍可见，但不再与
/// 确定为真的记忆同权竞争。
pub const STALE_SEED_FACTOR: f64 = 0.5;

/// Bitemporal validity of a cell at compressed time `w`; `None` = not valid
/// (not yet, or invalidated).
pub fn validity_at(p: &Pentachoron, w: f64) -> Option<Validity> {
    if !valid_at(p.w_from, p.w_to, w) {
        return None;
    }
    match p.w_expired {
        Some(e) if w >= e => Some(Validity::BelievedStale),
        _ => Some(Validity::True),
    }
}

/// as_of slice with shadow flags (small-scale convenience API; the hot path
/// is `Space4::snapshot_valid`).
pub fn slice_with_validity(space: &Space4, as_of_secs: i64) -> Vec<(PentaId, Validity)> {
    space
        .snapshot_valid(&[], as_of_secs)
        .into_iter()
        .map(|s| (s.id, s.validity))
        .collect()
}

/// Era neighborhood: cells with |core.w − w| ≤ tol.
pub fn era_neighborhood(space: &Space4, w: f64, tol: f64) -> Vec<PentaId> {
    let mut ids: Vec<PentaId> = space
        .all_ids()
        .into_iter()
        .filter(|id| (space.get(*id).unwrap().core.w - w).abs() <= tol)
        .collect();
    ids.sort_unstable();
    ids
}

#[derive(Clone, Debug)]
pub struct Hit {
    pub id: PentaId,
    pub score: f64,
    pub ppr: f64,
    pub seed: f64,
    pub validity: Validity,
}

#[derive(Clone, Debug)]
pub struct Query {
    pub labels: Vec<String>,
    pub as_of_secs: i64,
    pub k: usize,
}

/// Seed score from a snapshot cell: label overlap × era proximity.
/// Pure geometry — importance/embeddings deliberately excluded.
fn seed_score(s: &SnapshotCell, q_labels_len: usize, w_q: f64) -> f64 {
    if s.label_overlap == 0 || q_labels_len == 0 {
        return 0.0;
    }
    let overlap = s.label_overlap as f64 / q_labels_len.max(s.labels_len) as f64;
    let era = (-(s.core_w - w_q).abs() * ERA_BETA).exp();
    overlap * era
}

/// The 4D search: slice → seed → Space-PPR → fusion. Touches (registers
/// recall for) the returned hits — retrieval feeds the lifecycle.
pub fn search(space: &Space4, q: &Query) -> Vec<Hit> {
    let w_q = w_of_unix_secs(q.as_of_secs, space.t0_secs);

    // 1) one-lock snapshot slice → seeds (label overlap × era proximity)
    let snap = space.snapshot_valid(&q.labels, q.as_of_secs);
    let mut seeds: HashMap<PentaId, f64> = HashMap::new();
    let mut validity: HashMap<PentaId, Validity> = HashMap::new();
    for s in &snap {
        let score = seed_score(s, q.labels.len(), w_q);
        if score > 0.0 {
            // v0.3 影子带降权：BelievedStale 种子 ×0.5，注入与融合同步减半
            let score = if s.validity == Validity::BelievedStale {
                score * STALE_SEED_FACTOR
            } else {
                score
            };
            seeds.insert(s.id, score);
            validity.insert(s.id, s.validity);
        }
    }
    if seeds.is_empty() {
        return Vec::new();
    }

    // 2) bounded BFS over the sharing graph from the seeds
    let adj_all = space.sharing_adjacency_cached();
    let mut subgraph: HashMap<PentaId, Vec<(PentaId, f64)>> = HashMap::new();
    let mut seen: Vec<PentaId> = Vec::new();
    let mut seen_set: HashMap<PentaId, bool> = HashMap::new();
    let mut queue: Vec<PentaId> = seeds.keys().copied().collect();
    queue.sort_unstable();
    for &s in &queue {
        seen_set.insert(s, true);
    }
    while let Some(cur) = queue.first().copied() {
        queue.remove(0);
        seen.push(cur);
        if seen.len() >= PPR_FRONTIER {
            break;
        }
        if let Some(nbrs) = adj_all.get(&cur) {
            let mut sorted: Vec<(PentaId, f64)> = nbrs.clone();
            sorted.sort_by_key(|(n, _)| *n);
            for (n, _) in sorted {
                if !seen_set.get(&n).copied().unwrap_or(false) {
                    seen_set.insert(n, true);
                    queue.push(n);
                }
            }
        }
    }
    // adjacency restricted to the frontier
    for &a in &seen {
        if let Some(nbrs) = adj_all.get(&a) {
            let filt: Vec<(PentaId, f64)> = nbrs
                .iter()
                .filter(|(b, _)| seen_set.get(b).copied().unwrap_or(false))
                .copied()
                .collect();
            if !filt.is_empty() {
                subgraph.insert(a, filt);
            }
        }
    }

    // 3) power iteration: rank = 0.15·seed + 0.85·Mᵀ·rank (row-normalized M)
    let mut rank: HashMap<PentaId, f64> = HashMap::new();
    for &id in &seen {
        rank.insert(id, *seeds.get(&id).unwrap_or(&0.0));
    }
    for _ in 0..PPR_ITER {
        let mut next: HashMap<PentaId, f64> = HashMap::new();
        for (&id, &s) in &seeds {
            *next.entry(id).or_insert(0.0) += PPR_SEED * s;
        }
        for (&src, nbrs) in &subgraph {
            let r = *rank.get(&src).unwrap_or(&0.0);
            if r <= 0.0 || nbrs.is_empty() {
                continue;
            }
            let wsum: f64 = nbrs.iter().map(|(_, w)| *w).sum();
            for (dst, w) in nbrs {
                *next.entry(*dst).or_insert(0.0) += (1.0 - PPR_SEED) * r * (w / wsum);
            }
        }
        rank = next;
    }

    // 4) fusion + validity filter + top-k
    let ppr_max = rank.values().cloned().fold(0.0_f64, f64::max).max(1e-12);
    let seed_max = seeds.values().cloned().fold(0.0_f64, f64::max).max(1e-12);
    let mut hits: Vec<Hit> = rank
        .iter()
        .filter(|(id, _)| seeds.contains_key(id))
        .map(|(id, &pr)| {
            let s = seeds[id];
            Hit {
                id: *id,
                score: FUSION_PPR * (pr / ppr_max) + (1.0 - FUSION_PPR) * (s / seed_max),
                ppr: pr,
                seed: s,
                validity: validity.get(id).copied().unwrap_or(Validity::True),
            }
        })
        .collect();
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hits.truncate(q.k);

    // retrieval feeds lifecycle: register recalls
    for h in &hits {
        space.touch(h.id);
    }
    hits
}
