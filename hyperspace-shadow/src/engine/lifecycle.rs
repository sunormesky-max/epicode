//! Lifecycle dynamics in 4D (`lifecycle.rs` ↔ epicode's `auto_pipeline.rs`
//! + `governor.rs` + `dream.rs`).
//!
//! The fourth dimension turns epicode's lifecycle knobs into geometry:
//!
//! * **decay** — retention is DERIVED, never stored (the 11-08 report's
//!   P-decay recommendation): `R = exp(−Δw / S)` with the horizon `S`
//!   stretching by `0.5·ln(1 + access_count)`. Δw is in era units, so decay
//!   is *linear per doubling* — the natural grid of `0.98^days`.
//!   Zombies (R < 0.15) are scavenged; there is deliberately NO floor
//!   (the floor-death-lock finding).
//! * **fission⁴** — era split FIRST (pure w-move: a bimodal cluster snaps
//!   its minority into its own era stratum — impossible in 3D), then the
//!   classic label split (xyz push, but a modest 12.0 — not epicode's
//!   layer-leaking 30–50).
//! * **merge⁴** — clusters with label Jaccard ≥ 0.5 within anisotropic
//!   centroid distance 5.0 migrate 60% toward each other IN FULL 4D (xyz
//!   and w both) — merges respect eras now.
//! * **dream⁴** — REM consolidation: ≥3 same-label cells within one era
//!   (w-span < 0.3) collapse into their archetype, which absorbs
//!   `0.3·ln(n)` importance (the ln(access) term of effective importance,
//!   earned by rehearsal). Era noise compression as geometry.

use crate::engine::penta::Pentachoron;
use crate::engine::space4::{PentaId, Space4};
use crate::engine::time_axis::w_of_unix_secs;
use crate::engine::vec4::Vec4;

/// Retention horizon base: S = S_BASE + 0.5·ln(1+access).
const S_BASE: f64 = 1.5;
/// Zombie threshold (epicode governor: 0.15) — no importance floor above it.
pub const ZOMBIE_RETENTION: f64 = 0.15;
/// Era-split trigger: a consecutive w-gap larger than this = an era bridge.
/// Bridges dock at the ±1.0 sharing quantum (Δw ∈ [0.95, 1.05]); ordinary
/// chains step by ≤ ~0.55, so any gap > 0.9 inside a connected cluster
/// must be a bridge — 0.9 catches every bridge without false positives.
pub const FISSION_ERA_GAP: f64 = 0.9;
/// Label-split push distance (xyz) — deliberately modest (12, not 30–50).
const FISSION_PUSH: f64 = 12.0;
/// Era-split detach distance: enough to snap the bridge (≫ 2ε), small
/// enough not to leak layers.
const FISSION_DETACH: f64 = 2.0;
/// Merge trigger: anisotropic centroid distance (epicode MergeDistance 5.0).
pub const MERGE_DISTANCE: f64 = 5.0;
/// Merge label-set Jaccard threshold.
pub const MERGE_JACCARD: f64 = 0.5;
/// Dream: min same-signature cells in one era to consolidate.
pub const DREAM_GROUP_MIN: usize = 3;
/// Dream: era tightness of the group.
pub const DREAM_W_SPAN: f64 = 0.3;

/// Derived retention: `R = exp(−Δw / S)`, Δw = era units since the cell's
/// stratum, `S = 1.5 + 0.5·ln(1 + access_count)`.
pub fn retention(p: &Pentachoron, now_secs: i64, t0_secs: i64) -> f64 {
    let dw = (w_of_unix_secs(now_secs, t0_secs) - p.core.w).max(0.0);
    let s = S_BASE + 0.5 * (1.0 + p.access_count as f64).ln();
    (-dw / s).exp()
}

/// Effective importance = (importance + 0.3·ln(1+access)) · R
/// (epicode's governor formula, with days→Δw and access earned by `touch`).
pub fn effective_importance(p: &Pentachoron, now_secs: i64, t0_secs: i64) -> f64 {
    (p.importance + 0.3 * (1.0 + p.access_count as f64).ln()) * retention(p, now_secs, t0_secs)
}

/// Scavenge zombies: remove cells with R < ZOMBIE_RETENTION.
/// No floor, no exceptions — decay death-lock is a feature, not a bug, as
/// long as recalls (touch) stretch S for rehearsed memories.
pub fn scavenge_zombies(space: &Space4, now_secs: i64) -> usize {
    let now = now_secs;
    let victims: Vec<PentaId> = space
        .all_ids()
        .into_iter()
        .filter(|id| {
            let p = space.get(*id).unwrap();
            retention(&p, now, space.t0_secs) < ZOMBIE_RETENTION
        })
        .collect();
    let n = victims.len();
    for v in victims {
        space.remove_penta(v);
    }
    n
}

fn centroid(space: &Space4, ids: &[PentaId]) -> Vec4 {
    let mut c = Vec4::zero();
    for &id in ids {
        c = c.add(space.get(id).unwrap().core);
    }
    c.scale(1.0 / ids.len().max(1) as f64)
}

/// Fission⁴: era split first (pure w-relocation of the minority into its
/// own era centroid — the move only a 4D space can make), then label split
/// (xyz push, modest distance). Returns the number of splits performed.
pub fn auto_fission4(space: &Space4) -> usize {
    let mut splits = 0usize;
    for info in space.cluster_infos() {
        if info.size < 4 {
            continue;
        }
        // --- era split: largest consecutive w-gap > FISSION_ERA_GAP ---
        let mut ws: Vec<(f64, PentaId)> = info
            .ids
            .iter()
            .map(|&id| (space.get(id).unwrap().core.w, id))
            .collect();
        ws.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut best_gap = 0.0_f64;
        let mut best_at = 0usize;
        for i in 1..ws.len() {
            let gap = ws[i].0 - ws[i - 1].0;
            if gap > best_gap {
                best_gap = gap;
                best_at = i;
            }
        }
        let (lo, hi) = (ws[..best_at].to_vec(), ws[best_at..].to_vec());
        if best_gap > FISSION_ERA_GAP && lo.len() >= 2 && hi.len() >= 2 {
            // minority = smaller side (tie → earlier side is minority).
            // Split = DETACH: the minority keeps its own era (it already
            // lives there) and is pushed ~2.0 in xyz along the
            // minority→majority axis, snapping the era bridge. (Snapping w
            // alone cannot break the bridge — the cells already sit at
            // their own w.)
            let (minority, majority) = if lo.len() <= hi.len() {
                (&lo, &hi)
            } else {
                (&hi, &lo)
            };
            let min_ids: Vec<PentaId> = minority.iter().map(|(_, id)| *id).collect();
            let maj_ids: Vec<PentaId> = majority.iter().map(|(_, id)| *id).collect();
            let min_c = centroid(space, &min_ids);
            let maj_c = centroid(space, &maj_ids);
            let mut dir = Vec4::new(min_c.x - maj_c.x, min_c.y - maj_c.y, min_c.z - maj_c.z, 0.0);
            if dir.length() < 1e-9 {
                dir = Vec4::new(1.0, 0.0, 0.0, 0.0);
            }
            let push = dir.scale(FISSION_DETACH / dir.length());
            for &(_, id) in minority {
                if let Some(p) = space.get(id) {
                    let c = p.core;
                    space.relocate_penta(
                        id,
                        Vec4::new(c.x + push.x, c.y + push.y, c.z + push.z, c.w),
                    );
                }
            }
            splits += 1;
            continue;
        }
        // --- label split: minority label-signature pushed away in xyz ---
        let mut sig_groups: std::collections::HashMap<String, Vec<PentaId>> =
            std::collections::HashMap::new();
        for &id in &info.ids {
            let p = space.get(id).unwrap();
            let mut sig = p.labels.clone();
            sig.sort();
            sig_groups.entry(sig.join("\u{1}")).or_default().push(id);
        }
        let mut groups: Vec<(String, Vec<PentaId>)> = sig_groups.into_iter().collect();
        groups.sort_by(|a, b| a.0.cmp(&b.0));
        if groups.len() < 2 {
            continue;
        }
        groups.sort_by_key(|(_, ids)| std::cmp::Reverse(ids.len()));
        let majority = &groups[0].1;
        let minority: Vec<PentaId> = groups[1..]
            .iter()
            .flat_map(|(_, v)| v.iter().copied())
            .collect();
        if minority.len() < 2 || majority.len() < 2 {
            continue;
        }
        let maj_c = centroid(space, majority);
        let min_c = centroid(space, &minority);
        let mut dir = Vec4::new(min_c.x - maj_c.x, min_c.y - maj_c.y, min_c.z - maj_c.z, 0.0);
        if dir.length() < 1e-9 {
            dir = Vec4::new(1.0, 0.0, 0.0, 0.0);
        }
        let step = dir.scale(FISSION_PUSH / dir.length());
        for &id in &minority {
            let core = space.get(id).unwrap().core;
            space.relocate_penta(
                id,
                Vec4::new(core.x + step.x, core.y + step.y, core.z + step.z, core.w),
            );
        }
        splits += 1;
    }
    splits
}

/// Merge⁴: cluster pairs with label Jaccard ≥ 0.5 and anisotropic centroid
/// distance < MERGE_DISTANCE — smaller migrates 60% toward larger in 4D.
pub fn auto_merge4(space: &Space4) -> usize {
    let infos = space.cluster_infos();
    let mut merges = 0usize;
    'outer: for i in 0..infos.len() {
        for j in i + 1..infos.len() {
            let (a, b) = (&infos[i], &infos[j]);
            let la = space_label_set(space, &a.ids);
            let lb = space_label_set(space, &b.ids);
            let inter = la.intersection(&lb).count() as f64;
            let union = la.union(&lb).count().max(1) as f64;
            if inter / union < MERGE_JACCARD {
                continue;
            }
            let ca = centroid(space, &a.ids);
            let cb = centroid(space, &b.ids);
            let d = Vec4::aniso_distance(ca, cb, space.w_coupling);
            if d >= MERGE_DISTANCE {
                continue;
            }
            let (small, bigc) = if a.ids.len() <= b.ids.len() {
                (&a.ids, cb)
            } else {
                (&b.ids, ca)
            };
            for &id in small {
                // ids can vanish mid-pass (an earlier merge relocated them)
                // — skip rather than panic.
                if let Some(p) = space.get(id) {
                    let t = p.core.lerp(bigc, 0.6);
                    space.relocate_penta(id, t);
                }
            }
            merges += 1;
            if merges >= 32 {
                break 'outer; // bounded per round
            }
        }
    }
    merges
}

fn space_label_set(space: &Space4, ids: &[PentaId]) -> std::collections::HashSet<String> {
    let mut s = std::collections::HashSet::new();
    for &id in ids {
        if let Some(p) = space.get(id) {
            for l in &p.labels {
                s.insert(l.clone());
            }
        }
    }
    s
}

/// Dream⁴: within each cluster, ≥ DREAM_GROUP_MIN same-label cells with
/// w-span < DREAM_W_SPAN collapse into the archetype (max importance),
/// which absorbs `0.3·ln(n)` importance. Returns cells absorbed.
pub fn dream_consolidate4(space: &Space4) -> usize {
    let mut absorbed = 0usize;
    for info in space.cluster_infos() {
        let mut groups: std::collections::HashMap<String, Vec<PentaId>> =
            std::collections::HashMap::new();
        for &id in &info.ids {
            let p = space.get(id).unwrap();
            let mut sig = p.labels.clone();
            sig.sort();
            groups.entry(sig.join("\u{1}")).or_default().push(id);
        }
        let mut groups: Vec<(String, Vec<PentaId>)> = groups.into_iter().collect();
        groups.sort_by(|a, b| a.0.cmp(&b.0));
        for (_, ids) in groups {
            if ids.len() < DREAM_GROUP_MIN {
                continue;
            }
            let w_min = ids
                .iter()
                .map(|id| space.get(*id).unwrap().core.w)
                .fold(f64::MAX, f64::min);
            let w_max = ids
                .iter()
                .map(|id| space.get(*id).unwrap().core.w)
                .fold(f64::MIN, f64::max);
            if w_max - w_min >= DREAM_W_SPAN {
                continue;
            }
            // archetype = highest importance, tie → lowest id
            let mut sorted = ids.clone();
            sorted.sort_by_key(|id| {
                let p = space.get(*id).unwrap();
                (std::cmp::Reverse(p.importance.to_bits()), *id)
            });
            let arch = sorted[0];
            for &id in &sorted[1..] {
                space.remove_penta(id);
                absorbed += 1;
            }
            let n = ids.len() as f64;
            space.adjust_importance(arch, |imp| imp + 0.3 * n.ln());
        }
    }
    absorbed
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LifecycleReport {
    pub fissions: usize,
    pub merges: usize,
    pub dream_absorbed: usize,
    pub scavenged: usize,
    pub rounds: usize,
}

/// One scheduler tick: fission → merge → dream → scavenge, looped until a
/// round changes nothing (or `max_rounds`). Mirrors `auto_pipeline` order.
pub fn run_cycle(space: &Space4, now_secs: i64, max_rounds: usize) -> LifecycleReport {
    let mut rep = LifecycleReport::default();
    for _ in 0..max_rounds {
        let f = auto_fission4(space);
        let m = auto_merge4(space);
        let d = dream_consolidate4(space);
        let s = scavenge_zombies(space, now_secs);
        rep.rounds += 1;
        rep.fissions += f;
        rep.merges += m;
        rep.dream_absorbed += d;
        rep.scavenged += s;
        if f + m + d + s == 0 {
            break;
        }
    }
    rep
}

/// v0.3 era 带压缩默认阈值：相邻带人口和低于此值即合并
///（大卫评审：6×W 晶格的 W 随 log₂(t) 无限增长，稀疏鬼带需回收）。
pub const BAND_COMPACT_THRESHOLD: usize = 16;

/// v0.3 可选接入：生命周期调度之后追加 era 带压缩（dream⁴ 的
/// "era 噪声压缩"延伸到带级）。返回 (调度报告, 搬移细胞数)。
///
/// 压缩放在调度**之后**：fission⁴ 的 era 桥检测（簇内 w-间隙 > 0.9）
/// 先跑完，压缩产生的精确整数量子对接（±1.0）不会被同一轮 fission
/// 立即拆回 —— 避免 fission↔compact 振荡。
pub fn run_cycle_and_compact(
    space: &Space4,
    now_secs: i64,
    max_rounds: usize,
) -> (LifecycleReport, usize) {
    let rep = run_cycle(space, now_secs, max_rounds);
    let band = space.compact_bands(BAND_COMPACT_THRESHOLD);
    (rep, band.moved)
}
