//! `Space4` v0.2 — the TRUE 4D space engine.
//!
//! v0.1 critique (self): w only gated ε-merging; placement anchored in xyz
//! only; no retrieval; no lifecycle — "3.5-dimensional". v0.2 makes all four
//! dimensions carry structure:
//!
//! | capability | v0.1 | v0.2 |
//! |---|---|---|
//! | anchor choice | xyz-nearest-to-origin | **4D era-affinity scoring** via label index |
//! | seeding | origin ball | **(layer, era-band) Port lattice** (`topology.rs`) |
//! | anti-stack | O(P) scan | **core grid**, 81-neighborhood O(1) |
//! | recall | — | `touch()` access counting (feeds retention S) |
//! | retrieval | — | `retrieval.rs` reads the geometry (Space-PPR) |
//! | lifecycle | rejuvenate only | `lifecycle.rs`: decay/fission/merge/dream |
//! | bitemporal | valid window | + **shadow band** (`w_expired`, Zep semantics) |
//!
//! | epicode 3D (`domain/space.rs`) | hyperspace 4D |
//! |---|---|
//! | `Point3` / `Tetrahedron` (edge 1.0) | `Vec4` / `Pentachoron` (edge 1.0) |
//! | grid key `(i64,i64,i64)`, 27-neighborhood | key `[i64;4]`, 81-neighborhood |
//! | ε-merge at 0.05 → shared `VertexId` | identical, anisotropic metric dκ |
//! | clusters = connected components of vertex sharing | identical |
//! | `find_best_placement` label anchoring | label anchoring + era affinity + ports |
//! | bitemporal fields in payload | bitemporal window IS the w-extent (+shadow) |
//!
//! The coupling coefficient `κ` (see `Vec4::aniso_distance`) remains the
//! dial between "epicode today" (κ=0, degenerate limit) and full 4D.

use std::collections::HashMap;
use std::sync::RwLock;

use crate::engine::penta::{
    canonical_offsets, compute_vertices, hypervolume, Pentachoron, EDGE_LENGTH, PENTA_VERTICES,
    VERTEX_MERGE_EPSILON,
};
use crate::engine::time_axis::{w_of_unix_secs, w_range};
use crate::engine::topology::{era_band, port_core, Layer};
use crate::engine::vec4::Vec4;

/// Spatial hash cell size (same constant as `space.rs::GRID_CELL`).
pub const GRID_CELL: f64 = 1.0;

/// Minimum core separation to reject stacked placements
/// (mirrors the anti-stacking guard epicode's P0-6 asked for).
const CORE_MIN_SEPARATION: f64 = VERTEX_MERGE_EPSILON;

/// Era-affinity weight of anchor scoring (w units are ~era doublings).
const ANCHOR_ERA_WEIGHT: f64 = 2.0;

/// Direct-scan cap: same-label anchor candidates at or below this count are
/// all scored exactly (v0.2 behaviour, unchanged).
const ANCHOR_SCAN_CAP: usize = 600;
/// v0.3 two-level anchor scan — level 1 (coarse): deterministic stride
/// sample size used to locate the winning candidate region.
const ANCHOR_COARSE_CAP: usize = 64;
/// v0.3 two-level anchor scan — level 2 (fine): Chebyshev radius in core-grid
/// cells around the coarse winner (r = 2 → 5⁴ = 625 grid keys).
const ANCHOR_FINE_RADIUS: i64 = 2;
/// v0.3 two-level anchor scan — level 2 (fine): cap on same-label candidates
/// examined in the coarse winner's neighbourhood. 2400 ≫ 600: the old
/// stride-cap ceiling is broken, yet the scan cost stays bounded.
const ANCHOR_FINE_CAP: usize = 2400;

/// Beyond this size, `stats()` samples pair-sharing instead of O(P²).
const PAIR_HIST_CENSUS_LIMIT: usize = 2000;

pub type VertexId = u64;
pub type PentaId = u64;

#[derive(Clone, Debug)]
pub struct Vertex {
    pub id: VertexId,
    pub position: Vec4,
    /// Pentachora referencing this vertex (sharing = cluster edges).
    pub penta_ids: Vec<PentaId>,
}

#[derive(Clone, Debug)]
pub struct CellPayload {
    pub labels: Vec<String>,
    pub importance: f64,
    /// Bitemporal window in absolute unix seconds (`valid_until` =
    /// knowledge-level invalidation, i.e. when the system learned).
    pub valid_from: i64,
    pub valid_until: Option<i64>,
    /// Fact-level cease (may precede `valid_until`): the shadow band.
    pub expired_at: Option<i64>,
    /// Cylinder layer; `None` → computed from labels at insert.
    pub layer: Option<u8>,
}

impl CellPayload {
    pub fn new(
        labels: &[String],
        importance: f64,
        valid_from: i64,
        valid_until: Option<i64>,
    ) -> Self {
        CellPayload {
            labels: labels.to_vec(),
            importance,
            valid_from,
            valid_until,
            expired_at: None,
            layer: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SpaceStats {
    pub pentas: usize,
    pub vertices: usize,
    pub vertex_shares: usize, // vertices referenced by ≥2 pentas
    /// shared-vertex-count histogram: k → #pairs sharing exactly k vertices.
    /// T1′ predicts mass on {0, 1, 5} only (5 = coincident cores).
    /// Sampled (500 deterministic pairs) beyond PAIR_HIST_CENSUS_LIMIT.
    pub pair_sharing_hist: HashMap<usize, usize>,
    pub pair_hist_sampled: bool,
    pub clusters: usize,
    pub largest_cluster: usize,
    /// Clusters whose w-span ≤ 1.0 (one era) — era purity.
    pub era_pure_clusters: usize,
    pub mean_cluster_w_span: f64,
}

/// Bitemporal validity at a compressed time (shadow semantics).
/// Defined here so `snapshot_valid` can return it without a dependency
/// cycle; `retrieval` re-exports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Validity {
    /// Fact true and believed true at the query time.
    True,
    /// Shadow band (Zep `expired_at`): fact already ceased, but the system
    /// had not learned it yet — believed true, actually false.
    BelievedStale,
}

/// One valid-at cell from a snapshot query (see [`Space4::snapshot_valid`]).
#[derive(Clone, Debug)]
pub struct SnapshotCell {
    pub id: PentaId,
    pub core_w: f64,
    /// |labels ∩ query| (0 = no label relation).
    pub label_overlap: usize,
    pub labels_len: usize,
    pub validity: Validity,
}

/// Per-cluster summary for structure reports.
#[derive(Clone, Debug)]
pub struct ClusterInfo {
    pub ids: Vec<PentaId>,
    pub size: usize,
    pub top_label: String,
    pub w_min: f64,
    pub w_max: f64,
    pub w_span: f64,
}

/// v0.3 era 带压缩报告。
#[derive(Clone, Debug, Default)]
pub struct BandCompaction {
    /// 被整数量子平移搬入保留带的细胞数。
    pub moved: usize,
    /// 合并的带对 (kept, absorbed)：absorbed 带的全体细胞平移
    /// (kept − absorbed) 个 1.0 w-量子（相邻带即 ±1.0），xyz 不变。
    pub merged_pairs: Vec<(i64, i64)>,
}

struct SpaceInner {
    vertices: HashMap<VertexId, Vertex>,
    pentas: HashMap<PentaId, Pentachoron>,
    grid: HashMap<[i64; 4], Vec<VertexId>>,
    /// v0.2: core spatial hash — O(1) anti-stacking via 81-neighborhood.
    core_grid: HashMap<[i64; 4], Vec<PentaId>>,
    /// v0.2: label inverted index (epicode `label_index`).
    label_index: HashMap<String, Vec<PentaId>>,
    /// Bumped on every structural mutation; invalidates the PPR cache.
    structure_version: u64,
    next_vertex: VertexId,
    next_penta: PentaId,
}

/// Deterministic tiny PRNG (xorshift64*) so demos are reproducible.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.max(1))
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Uniform point in the xyz ball of radius r, w fixed.
    pub fn in_ball(&mut self, r: f64, w: f64) -> Vec4 {
        loop {
            let x = (self.next_f64() * 2.0 - 1.0) * r;
            let y = (self.next_f64() * 2.0 - 1.0) * r;
            let z = (self.next_f64() * 2.0 - 1.0) * r;
            if x * x + y * y + z * z <= r * r {
                return Vec4::new(x, y, z, w);
            }
        }
    }
}

pub struct Space4 {
    inner: RwLock<SpaceInner>,
    /// PPR adjacency cache, invalidated by `structure_version`
    /// (the 4D twin of epicode's `search_revision` BM25 cache).
    adj_cache:
        std::sync::Mutex<Option<(u64, std::sync::Arc<HashMap<PentaId, Vec<(PentaId, f64)>>>)>>,
    /// Time-coupling coefficient κ ∈ [0, 1] (see module docs).
    pub w_coupling: f64,
    /// Birth of the memory space — the w = 0 anchor.
    pub t0_secs: i64,
}

impl Space4 {
    pub fn new(w_coupling: f64, t0_secs: i64) -> Self {
        Space4 {
            inner: RwLock::new(SpaceInner {
                vertices: HashMap::new(),
                pentas: HashMap::new(),
                grid: HashMap::new(),
                core_grid: HashMap::new(),
                label_index: HashMap::new(),
                structure_version: 1,
                next_vertex: 1,
                next_penta: 1,
            }),
            adj_cache: std::sync::Mutex::new(None),
            w_coupling: w_coupling.clamp(0.0, 1.0),
            t0_secs,
        }
    }

    // ---- spatial hash ------------------------------------------------

    fn grid_key(p: Vec4) -> [i64; 4] {
        [
            (p.x / GRID_CELL).floor() as i64,
            (p.y / GRID_CELL).floor() as i64,
            (p.z / GRID_CELL).floor() as i64,
            (p.w / GRID_CELL).floor() as i64,
        ]
    }

    /// 3⁴ = 81-cell neighborhood around a grid key.
    fn nearby_keys(k: [i64; 4]) -> Vec<[i64; 4]> {
        Self::keys_within(k, 1)
    }

    /// (2r+1)⁴ grid keys within Chebyshev radius `r` of `k`
    /// (r = 1 → the 81-neighborhood; r = 2 → the anchor fine-scan region).
    fn keys_within(k: [i64; 4], radius: i64) -> Vec<[i64; 4]> {
        let mut keys = Vec::with_capacity((2 * radius + 1).pow(4) as usize);
        for dx in -radius..=radius {
            for dy in -radius..=radius {
                for dz in -radius..=radius {
                    for dw in -radius..=radius {
                        keys.push([k[0] + dx, k[1] + dy, k[2] + dz, k[3] + dw]);
                    }
                }
            }
        }
        keys
    }

    // ---- ε-merge insertion -------------------------------------------

    /// Find an existing vertex within VERTEX_MERGE_EPSILON (anisotropic dκ)
    /// of `p`, scanning the 81-cell neighborhood.
    fn find_merge_target(inner: &SpaceInner, p: Vec4, kappa: f64) -> Option<VertexId> {
        let eps_sq = VERTEX_MERGE_EPSILON * VERTEX_MERGE_EPSILON;
        for key in Self::nearby_keys(Self::grid_key(p)) {
            if let Some(ids) = inner.grid.get(&key) {
                for &vid in ids {
                    let v = &inner.vertices[&vid];
                    if Vec4::aniso_distance_sq(v.position, p, kappa) < eps_sq {
                        return Some(vid);
                    }
                }
            }
        }
        None
    }

    /// O(1) anti-stacking probe via the core grid (v0.2).
    ///
    /// Two rejection rules:
    /// 1. anisotropic core distance < ε (the P0-6 rule);
    /// 2. **ε-chain rule**: identical xyz pattern (distance < ε) with era
    ///    gap ≤ 2ε — two such cells would each legitimately ε-merge into
    ///    the SAME older pivot vertices (ε-merge transitivity), producing
    ///    degenerate 2/3/4-shares that break T1′ at engine level.
    ///    (Discovered by probe2: co-located xyz, Δw = 0.078, both merging
    ///    pivot vertex 98/94 of an earlier cell.) Same-era near-duplicates
    ///    are duplicates — different eras (Δw ≥ 0.5) still coexist freely.
    fn core_stacks(inner: &SpaceInner, core: Vec4, kappa: f64) -> bool {
        let eps = CORE_MIN_SEPARATION;
        let eps_sq = eps * eps;
        let chain_w = 2.0 * eps;
        for key in Self::nearby_keys(Self::grid_key(core)) {
            if let Some(ids) = inner.core_grid.get(&key) {
                for &pid in ids {
                    let c = inner.pentas[&pid].core;
                    if Vec4::aniso_distance_sq(c, core, kappa) < eps_sq {
                        return true;
                    }
                    let d = c.sub(core);
                    if d.x * d.x + d.y * d.y + d.z * d.z < eps_sq && (kappa * d.w).abs() < chain_w {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// How many of the 5 candidate vertices at `core` would ε-merge with
    /// existing vertices? (placement scoring, mirrors `count_vertex_merges`)
    pub fn count_vertex_merges(&self, core: Vec4) -> usize {
        let inner = self.inner.read().unwrap();
        compute_vertices(core)
            .iter()
            .filter(|v| Self::find_merge_target(&inner, **v, self.w_coupling).is_some())
            .count()
    }

    /// Insert one pentachoron at `core`. Returns `None` if the core stacks
    /// on an existing core (P0-6 防堆叠守卫, the same guard epicode added to
    /// `insert_tetra`): without it, ε-fuzzy accumulations of near-identical
    /// cells produce degenerate 2/3/4-vertex shares and destroy T1′.
    /// On success returns the id and the number of vertices ε-merged with
    /// existing cells (the sharing event).
    pub fn insert_penta(&self, core: Vec4, payload: CellPayload) -> Option<(PentaId, usize)> {
        let mut inner = self.inner.write().unwrap();
        Self::insert_penta_locked(&mut inner, self.w_coupling, self.t0_secs, core, payload, 0)
    }

    /// Locked core of [`Self::insert_penta`] — operates on a held write guard
    /// so multi-step moves (relocate/rejuvenate, v0.3 atomicity) can compose
    /// remove+insert inside ONE lock acquisition.
    fn insert_penta_locked(
        inner: &mut SpaceInner,
        kappa: f64,
        t0_secs: i64,
        core: Vec4,
        payload: CellPayload,
        access_count: u64,
    ) -> Option<(PentaId, usize)> {
        let (w_from, w_to) = w_range(payload.valid_from, payload.valid_until, t0_secs);
        let w_expired = payload.expired_at.map(|e| w_of_unix_secs(e, t0_secs));
        let layer = payload
            .layer
            .unwrap_or_else(|| Layer::from_labels(&payload.labels).index());
        if Self::core_stacks(inner, core, kappa) {
            return None;
        }
        let verts = compute_vertices(core);
        let mut vertex_ids = [0u64; PENTA_VERTICES];
        let mut shared = 0usize;
        for (n, p) in verts.iter().enumerate() {
            match Self::find_merge_target(inner, *p, kappa) {
                Some(vid) => {
                    vertex_ids[n] = vid;
                    shared += 1;
                }
                None => {
                    let vid = inner.next_vertex;
                    inner.next_vertex += 1;
                    inner.grid.entry(Self::grid_key(*p)).or_default().push(vid);
                    inner.vertices.insert(
                        vid,
                        Vertex {
                            id: vid,
                            position: *p,
                            penta_ids: Vec::new(),
                        },
                    );
                    vertex_ids[n] = vid;
                }
            }
        }
        let pid = inner.next_penta;
        inner.next_penta += 1;
        inner.structure_version += 1;
        for &vid in &vertex_ids {
            inner.vertices.get_mut(&vid).unwrap().penta_ids.push(pid);
        }
        inner
            .core_grid
            .entry(Self::grid_key(core))
            .or_default()
            .push(pid);
        for l in &payload.labels {
            inner.label_index.entry(l.clone()).or_default().push(pid);
        }
        inner.pentas.insert(
            pid,
            Pentachoron {
                id: pid,
                vertex_ids,
                core,
                w_from,
                w_to,
                labels: payload.labels,
                importance: payload.importance,
                access_count,
                w_expired,
                layer,
            },
        );
        Some((pid, shared))
    }

    /// Remove a pentachoron, garbage-collecting vertices and indexes.
    pub fn remove_penta(&self, pid: PentaId) -> bool {
        let mut inner = self.inner.write().unwrap();
        Self::remove_penta_locked(&mut inner, pid)
    }

    /// Locked core of [`Self::remove_penta`] (see `insert_penta_locked`).
    fn remove_penta_locked(inner: &mut SpaceInner, pid: PentaId) -> bool {
        let Some(p) = inner.pentas.remove(&pid) else {
            return false;
        };
        inner.structure_version += 1;
        for &vid in &p.vertex_ids {
            let empty = {
                let v = inner.vertices.get_mut(&vid).unwrap();
                v.penta_ids.retain(|&x| x != pid);
                v.penta_ids.is_empty()
            };
            if empty {
                let pos = inner.vertices[&vid].position;
                let key = Self::grid_key(pos);
                if let Some(list) = inner.grid.get_mut(&key) {
                    list.retain(|&x| x != vid);
                    if list.is_empty() {
                        inner.grid.remove(&key);
                    }
                }
                inner.vertices.remove(&vid);
            }
        }
        let ckey = Self::grid_key(p.core);
        if let Some(list) = inner.core_grid.get_mut(&ckey) {
            list.retain(|&x| x != pid);
            if list.is_empty() {
                inner.core_grid.remove(&ckey);
            }
        }
        for l in &p.labels {
            if let Some(v) = inner.label_index.get_mut(l) {
                v.retain(|&x| x != pid);
                if v.is_empty() {
                    inner.label_index.remove(l);
                }
            }
        }
        true
    }

    /// Move a pentachoron to a new core (remove + re-ε-merge elsewhere).
    /// Mirrors `space.rs::relocate_tetrahedron` ("删旧插新").
    /// Returns the NEW penta id (ids are not stable across relocation).
    /// If the target core is stacked, retries along small deterministic
    /// shifts (the P0-6 guard can reject the primary target).
    /// `access_count` survives relocation.
    ///
    /// v0.3 单锁原子化：读 payload → 删旧 → 插新 在**同一次写锁**内完成。
    /// v0.2 的三段式（读锁取 payload → 删锁 → 插锁）会让并发读者观察到
    /// "细胞已消失但尚未落位" 的中途态；现在任何时刻取得锁的读者看到的
    /// 都是完整的搬移前或搬移后状态（测试 relocate_is_atomic… 固化）。
    pub fn relocate_penta(&self, pid: PentaId, new_core: Vec4) -> Option<(PentaId, usize)> {
        let mut inner = self.inner.write().unwrap();
        let (payload, access, old_core) = {
            let Some(p) = inner.pentas.get(&pid) else {
                return None;
            };
            (
                CellPayload {
                    labels: p.labels.clone(),
                    importance: p.importance,
                    // w is stored compressed — decompress to secs, insert recompresses.
                    valid_from: crate::engine::time_axis::unix_secs_of_w(p.w_from, self.t0_secs),
                    valid_until: p
                        .w_to
                        .map(|hi| crate::engine::time_axis::unix_secs_of_w(hi, self.t0_secs)),
                    expired_at: p
                        .w_expired
                        .map(|e| crate::engine::time_axis::unix_secs_of_w(e, self.t0_secs)),
                    layer: Some(p.layer),
                },
                p.access_count,
                p.core,
            )
        };
        Self::remove_penta_locked(&mut inner, pid);
        // Deterministic lattice of fallback spots — a cell must NEVER be
        // lost to a stacked target (relocation is not deletion).
        let mut cands: Vec<Vec4> = Vec::with_capacity(17);
        cands.push(new_core);
        for i in 0..4i64 {
            for j in 0..4i64 {
                cands.push(Vec4::new(
                    new_core.x + i as f64 * 0.7,
                    new_core.y + j as f64 * 0.7,
                    new_core.z,
                    new_core.w,
                ));
            }
        }
        for cand in cands {
            if let Some(res) = Self::insert_penta_locked(
                &mut inner,
                self.w_coupling,
                self.t0_secs,
                cand,
                payload.clone(),
                access,
            ) {
                return Some(res);
            }
        }
        // Pathological density: all 17 fallbacks stacked. Re-insert at the
        // OLD core (just vacated by this very move, so it cannot stack with
        // itself) — the move degrades to a no-op instead of losing the cell.
        Self::insert_penta_locked(
            &mut inner,
            self.w_coupling,
            self.t0_secs,
            old_core,
            payload,
            access,
        )
    }

    /// Register a recall: bumps `access_count` (feeds retention stretch S
    /// and effective importance). v0.2's cheapest lifecycle hook.
    pub fn touch(&self, pid: PentaId) {
        let mut inner = self.inner.write().unwrap();
        if let Some(p) = inner.pentas.get_mut(&pid) {
            p.access_count += 1;
        }
    }

    /// All pentas carrying any of these labels (label index lookup).
    pub fn by_label(&self, labels: &[String]) -> Vec<PentaId> {
        let inner = self.inner.read().unwrap();
        let mut out: Vec<PentaId> = Vec::new();
        for l in labels {
            if let Some(v) = inner.label_index.get(l) {
                out.extend_from_slice(v);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    // ---- placement ----------------------------------------------------

    /// v0.3 两级锚定扫描（two-level anchor scan）。
    ///
    /// 候选 ≤ `ANCHOR_SCAN_CAP`(600) 时全量精评（v0.2 行为不变）；超过后
    /// 旧实现按步进采样截断到 600 —— Zipf 头部话题在万级空间轻松击穿，
    /// 最优锚可能恰好落在被跳过的格点上。两级扫描替代截断：
    ///
    /// * ① 粗格（coarse）：确定性步进采样 ≤ `ANCHOR_COARSE_CAP` 个候选，
    ///   以同一评分 `κ·ERA_WEIGHT·|Δw| + 0.1·d_port` 定位最优候选区；
    /// * ② 精扫（fine）：以粗格赢家核心为中心、核心网格 Chebyshev 半径
    ///   `ANCHOR_FINE_RADIUS` 的 (2r+1)⁴ 格内，所有携带查询标签的细胞
    ///   全量精评（≤ `ANCHOR_FINE_CAP` = 2400 ≫ 600）。粗格赢家自身始终
    ///   参与精评，故两级结果永不差于纯粗格。
    ///
    /// 语义不变式：评分公式、tie-break（同分取小 id）、O(1) 防堆叠
    /// （core_stacks 81 邻域）一概未动 —— 只有候选覆盖面从"600 截断"
    /// 升级为"粗定位 + 邻域全扫"。
    fn anchor_best(
        inner: &SpaceInner,
        labels: &[String],
        w_at: f64,
        kappa: f64,
    ) -> Option<(f64, PentaId, Vec4)> {
        let mut cands: Vec<PentaId> = Vec::new();
        for l in labels {
            if let Some(v) = inner.label_index.get(l) {
                cands.extend_from_slice(v);
            }
        }
        cands.sort_unstable();
        cands.dedup();
        if cands.is_empty() {
            return None;
        }
        let layer = Layer::from_labels(labels);
        let port = port_core(layer, era_band(w_at));
        let mut best: Option<(f64, PentaId, Vec4)> = None;
        let consider = |best: &mut Option<(f64, PentaId, Vec4)>, pid: PentaId| {
            let p = &inner.pentas[&pid];
            let d_port =
                Vec4::new(p.core.x - port.x, p.core.y - port.y, p.core.z - port.z, 0.0).length();
            let score = kappa * ANCHOR_ERA_WEIGHT * (p.core.w - w_at).abs() + 0.1 * d_port;
            if best.map_or(true, |(bs, bid, _)| {
                score < bs || (score == bs && pid < bid)
            }) {
                *best = Some((score, pid, p.core));
            }
        };
        if cands.len() <= ANCHOR_SCAN_CAP {
            for &pid in &cands {
                consider(&mut best, pid);
            }
            return best;
        }
        // ① 粗格：确定性步进采样定位候选区
        let stride = cands.len() / ANCHOR_COARSE_CAP + 1;
        for &pid in cands.iter().step_by(stride) {
            consider(&mut best, pid);
        }
        let (_, _, coarse_core) = best?;
        // ② 精扫：粗格赢家的核心网格邻域内同标签候选全量精评
        let mut scanned = 0usize;
        'fine: for key in Self::keys_within(Self::grid_key(coarse_core), ANCHOR_FINE_RADIUS) {
            if let Some(ids) = inner.core_grid.get(&key) {
                for &pid in ids {
                    if scanned >= ANCHOR_FINE_CAP {
                        break 'fine;
                    }
                    scanned += 1;
                    let p = &inner.pentas[&pid];
                    if !p.labels.iter().any(|l| labels.contains(l)) {
                        continue;
                    }
                    consider(&mut best, pid);
                }
            }
        }
        best
    }

    /// Diagnostic twin of the placement anchor scan: the two-level winner
    /// as `(id, score)`. Read-only; intended for tests and future MCP tooling.
    pub fn best_anchor(&self, labels: &[String], w_at: f64) -> Option<(PentaId, f64)> {
        let inner = self.inner.read().unwrap();
        Self::anchor_best(&inner, labels, w_at, self.w_coupling).map(|(s, id, _)| (id, s))
    }

    /// v0.2 TRUE-4D placement, the mirror of `gateway.rs::find_best_placement`:
    ///
    /// 1. **anchor** = same-label cell minimizing
    ///    `κ·ERA_WEIGHT·|w_a − w_at| + 0.1·d_xyz(a, port_xyz(layer, band))`
    ///    — era affinity participates (same-era anchors preferred, cross-era
    ///    docking allowed when a topic's era is empty — this is what bridges
    ///    era bands into one connected topic structure);
    ///    v0.3: anchor selection is the two-level scan (see `anchor_best`);
    /// 2. candidates = anchor.xyz + (oᵢ − oⱼ).xyz for all 25 offset pairs,
    ///    core.w pinned to the memory's own era `w_at`;
    /// 3. O(1) anti-stack via core grid; keep the max-merge candidate;
    /// 4. no anchor → **Port seeding** near (layer, band) port core
    ///    (the hypercylinder lattice; epicode reserves cylinder ports);
    /// 5. 7% orphan diversity seed at radius 8–15 (unchanged).
    pub fn find_best_placement(
        &self,
        labels: &[String],
        w_at: f64,
        rng: &mut Rng,
        orphan_prob: f64,
    ) -> Vec4 {
        let inner = self.inner.read().unwrap();
        Self::find_best_placement_locked(&inner, labels, w_at, self.w_coupling, rng, orphan_prob)
    }

    /// Locked core of [`Self::find_best_placement`] — takes the read guard as
    /// a parameter so `rejuvenate` (v0.3) can re-place inside its atomic
    /// write-lock section (a write guard also satisfies a read borrow).
    fn find_best_placement_locked(
        inner: &SpaceInner,
        labels: &[String],
        w_at: f64,
        kappa: f64,
        rng: &mut Rng,
        orphan_prob: f64,
    ) -> Vec4 {
        let layer = Layer::from_labels(labels);
        let band = era_band(w_at);
        let port = port_core(layer, band);

        let anchor = Self::anchor_best(inner, labels, w_at, kappa).map(|(_, _, c)| c);

        let Some(anchor) = anchor else {
            // Port seeding: home address (layer, band) + jitter.
            let mut p = rng.in_ball(0.8, w_at);
            p.x += port.x;
            p.y += port.y;
            p.z += port.z;
            p.w = w_at;
            return p;
        };

        if rng.next_f64() < orphan_prob {
            // Diversity seed, radius 8–15 (gateway.rs orphan branch).
            let r = 8.0 + 7.0 * rng.next_f64();
            let mut p = rng.in_ball(r, w_at);
            p.w = w_at;
            return p;
        }

        let offsets = canonical_offsets();
        let mut best: Option<(usize, Vec4)> = None;
        for oi in &offsets {
            for oj in &offsets {
                let cand = Vec4::new(
                    anchor.x + oi.x - oj.x,
                    anchor.y + oi.y - oj.y,
                    anchor.z + oi.z - oj.z,
                    w_at,
                );
                if Self::core_stacks(inner, cand, kappa) {
                    continue;
                }
                let merges = compute_vertices(cand)
                    .iter()
                    .filter(|v| Self::find_merge_target(inner, **v, kappa).is_some())
                    .count();
                if best.map_or(true, |(bm, _)| merges > bm) {
                    best = Some((merges, cand));
                }
            }
        }
        match best {
            Some((_, c)) => c,
            None => {
                let mut p = rng.in_ball(8.0, w_at);
                p.w = w_at;
                p
            }
        }
    }

    /// Full create path: placement + insert (mirrors `create_memory_with_time`).
    ///
    /// The new cell's stratum is `w(valid_from)` — the era the memory is
    /// about. `now` is unused here (recency scoring lives in the real
    /// search engine, not in placement); kept for API symmetry.
    pub fn create(
        &self,
        labels: &[String],
        importance: f64,
        valid_from: i64,
        valid_until: Option<i64>,
        now: i64,
        rng: &mut Rng,
    ) -> Option<PentaId> {
        self.create_full(
            CellPayload::new(labels, importance, valid_from, valid_until),
            now,
            rng,
        )
    }

    /// Create with an explicit orphan-seed probability (0.0 = always anchor;
    /// production uses epicode's 0.07 diversity seed).
    pub fn create_opts(
        &self,
        payload: CellPayload,
        now: i64,
        rng: &mut Rng,
        orphan_prob: f64,
    ) -> Option<PentaId> {
        let _ = now;
        let w_at = w_of_unix_secs(payload.valid_from, self.t0_secs);
        for _ in 0..8 {
            let core = self.find_best_placement(&payload.labels, w_at, rng, orphan_prob);
            if let Some((pid, _)) = self.insert_penta(core, payload.clone()) {
                return Some(pid);
            }
        }
        let core = rng.in_ball(14.0, w_at);
        self.insert_penta(core, payload).map(|(pid, _)| pid)
    }

    /// Create with the full v0.2 payload (shadow window, layer override).
    pub fn create_full(&self, payload: CellPayload, now: i64, rng: &mut Rng) -> Option<PentaId> {
        let _ = now;
        let w_at = w_of_unix_secs(payload.valid_from, self.t0_secs);
        for _ in 0..8 {
            let core = self.find_best_placement(&payload.labels, w_at, rng, 0.07);
            if let Some((pid, _)) = self.insert_penta(core, payload.clone()) {
                return Some(pid);
            }
            // stacked despite the placement pre-filter (dense zone) — re-place
        }
        // last resort: diversity seed far out
        let core = rng.in_ball(14.0, w_at);
        self.insert_penta(core, payload).map(|(pid, _)| pid)
    }

    // ---- clusters / queries -------------------------------------------

    /// Clusters = connected components of the vertex-sharing graph
    /// (mirrors `space.rs::find_clusters` BFS over `vertex_to_tetras`).
    pub fn find_clusters(&self) -> Vec<Vec<PentaId>> {
        self.cluster_infos().into_iter().map(|c| c.ids).collect()
    }

    /// v0.2: clusters with structure summaries (era span, top label).
    pub fn cluster_infos(&self) -> Vec<ClusterInfo> {
        let inner = self.inner.read().unwrap();
        let mut adj: HashMap<PentaId, Vec<PentaId>> = HashMap::new();
        for v in inner.vertices.values() {
            for (i, &a) in v.penta_ids.iter().enumerate() {
                for &b in v.penta_ids.iter().skip(i + 1) {
                    adj.entry(a).or_default().push(b);
                    adj.entry(b).or_default().push(a);
                }
            }
        }
        let mut seen: HashMap<PentaId, bool> = HashMap::new();
        let mut clusters = Vec::new();
        let mut ids: Vec<PentaId> = inner.pentas.keys().copied().collect();
        ids.sort_unstable();
        for pid in ids {
            if seen.get(&pid).copied().unwrap_or(false) {
                continue;
            }
            let mut comp = Vec::new();
            let mut stack = vec![pid];
            seen.insert(pid, true);
            while let Some(cur) = stack.pop() {
                comp.push(cur);
                if let Some(nbrs) = adj.get(&cur) {
                    for &n in nbrs {
                        if !seen.get(&n).copied().unwrap_or(false) {
                            seen.insert(n, true);
                            stack.push(n);
                        }
                    }
                }
            }
            comp.sort_unstable();
            let (mut w_min, mut w_max) = (f64::MAX, f64::MIN);
            let mut label_count: HashMap<&str, usize> = HashMap::new();
            for &c in &comp {
                let p = &inner.pentas[&c];
                w_min = w_min.min(p.core.w);
                w_max = w_max.max(p.core.w);
                for l in &p.labels {
                    *label_count.entry(l.as_str()).or_insert(0) += 1;
                }
            }
            let top_label = label_count
                .into_iter()
                .max_by_key(|(l, n)| (*n, l.to_string()))
                .map(|(l, _)| l.to_string())
                .unwrap_or_default();
            clusters.push(ClusterInfo {
                size: comp.len(),
                w_span: w_max - w_min,
                w_min,
                w_max,
                top_label,
                ids: comp,
            });
        }
        clusters
    }

    /// Exact number of shared vertices between two pentachora (T1′ probe).
    pub fn shared_vertex_count(&self, a: PentaId, b: PentaId) -> usize {
        let inner = self.inner.read().unwrap();
        match (inner.pentas.get(&a), inner.pentas.get(&b)) {
            (Some(pa), Some(pb)) => pa
                .vertex_ids
                .iter()
                .filter(|v| pb.vertex_ids.contains(v))
                .count(),
            _ => 0,
        }
    }

    /// Bitemporal as_of query as a hyperplane slice: pentachora whose
    /// compressed validity window contains `as_of_secs`. This is the
    /// geometric version of epicode's payload-field filter.
    pub fn slice_as_of(&self, as_of_secs: i64) -> Vec<PentaId> {
        let w = w_of_unix_secs(as_of_secs, self.t0_secs);
        let inner = self.inner.read().unwrap();
        let mut ids: Vec<PentaId> = inner
            .pentas
            .values()
            .filter(|p| crate::engine::time_axis::valid_at(p.w_from, p.w_to, w))
            .map(|p| p.id)
            .collect();
        ids.sort_unstable();
        ids
    }

    /// ONE-lock, zero-clone as_of snapshot with label overlap and shadow
    /// validity — the hot path for `retrieval::search` (10k cells in
    /// microseconds instead of 10k lock+clone round-trips).
    pub fn snapshot_valid(&self, q_labels: &[String], as_of_secs: i64) -> Vec<SnapshotCell> {
        let w = w_of_unix_secs(as_of_secs, self.t0_secs);
        let inner = self.inner.read().unwrap();
        let mut out = Vec::new();
        for p in inner.pentas.values() {
            if !crate::engine::time_axis::valid_at(p.w_from, p.w_to, w) {
                continue;
            }
            let validity = match p.w_expired {
                Some(e) if w >= e => Validity::BelievedStale,
                _ => Validity::True,
            };
            let label_overlap = p
                .labels
                .iter()
                .filter(|l| q_labels.iter().any(|m| m == *l))
                .count();
            out.push(SnapshotCell {
                id: p.id,
                core_w: p.core.w,
                labels_len: p.labels.len(),
                label_overlap,
                validity,
            });
        }
        out.sort_by_key(|s| s.id);
        out
    }

    /// Rejuvenation: pull an old cell toward the present stratum
    /// `w → w_now + β·(w_old − w_now)` and RE-ANCHOR it there via the
    /// placement search (not a vertical teleport): geometric reading of
    /// "recall refreshes memory" — the cell rejoins its topic's current-era
    /// cluster, or lands nearby if the topic has moved on.
    ///
    /// v0.3 单锁原子化：删旧 + 重锚定 + 插新 全部在一次写锁内完成
    /// （与 `relocate_penta` 同理，中途态对并发读者不可见）。
    pub fn rejuvenate(
        &self,
        pid: PentaId,
        now: i64,
        beta: f64,
        rng: &mut Rng,
    ) -> Option<(PentaId, usize)> {
        let w_now = w_of_unix_secs(now, self.t0_secs);
        let mut inner = self.inner.write().unwrap();
        let payload = {
            let Some(p) = inner.pentas.get(&pid) else {
                return None;
            };
            CellPayload {
                labels: p.labels.clone(),
                importance: p.importance,
                valid_from: crate::engine::time_axis::unix_secs_of_w(
                    w_now + beta * (p.core.w - w_now),
                    self.t0_secs,
                ),
                valid_until: p
                    .w_to
                    .map(|hi| crate::engine::time_axis::unix_secs_of_w(hi, self.t0_secs)),
                expired_at: p
                    .w_expired
                    .map(|e| crate::engine::time_axis::unix_secs_of_w(e, self.t0_secs)),
                layer: Some(p.layer),
            }
        };
        let w_at = crate::engine::time_axis::w_of_unix_secs(payload.valid_from, self.t0_secs);
        Self::remove_penta_locked(&mut inner, pid);
        for _ in 0..8 {
            let core = Self::find_best_placement_locked(
                &inner,
                &payload.labels,
                w_at,
                self.w_coupling,
                rng,
                0.0,
            );
            if let Some(res) = Self::insert_penta_locked(
                &mut inner,
                self.w_coupling,
                self.t0_secs,
                core,
                payload.clone(),
                0,
            ) {
                return Some(res);
            }
        }
        let core = rng.in_ball(12.0, w_at);
        Self::insert_penta_locked(&mut inner, self.w_coupling, self.t0_secs, core, payload, 0)
    }

    pub fn get(&self, pid: PentaId) -> Option<Pentachoron> {
        self.inner.read().unwrap().pentas.get(&pid).cloned()
    }

    pub fn all_ids(&self) -> Vec<PentaId> {
        let mut ids: Vec<PentaId> = self.inner.read().unwrap().pentas.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    pub fn stats(&self) -> SpaceStats {
        let inner = self.inner.read().unwrap();
        let mut st = SpaceStats {
            pentas: inner.pentas.len(),
            vertices: inner.vertices.len(),
            ..Default::default()
        };
        st.vertex_shares = inner
            .vertices
            .values()
            .filter(|v| v.penta_ids.len() >= 2)
            .count();
        let mut ids: Vec<PentaId> = inner.pentas.keys().copied().collect();
        ids.sort_unstable();
        if ids.len() <= PAIR_HIST_CENSUS_LIMIT {
            for i in 0..ids.len() {
                for &b in ids.iter().skip(i + 1) {
                    let k = self.shared_vertex_count_locked(&inner, ids[i], b);
                    *st.pair_sharing_hist.entry(k).or_insert(0) += 1;
                }
            }
        } else {
            // deterministic row-local sample (a global stride can align
            // with n and sample nothing but the diagonal — found the hard
            // way in the 10k demo)
            st.pair_hist_sampled = true;
            let n = ids.len();
            let row_stride = (n / 40).max(1);
            let mut i = 0usize;
            while i < n {
                let j_max = (i + 12).min(n);
                let mut j = i + 1;
                while j < j_max {
                    let k = self.shared_vertex_count_locked(&inner, ids[i], ids[j]);
                    *st.pair_sharing_hist.entry(k).or_insert(0) += 1;
                    j += 3;
                }
                i += row_stride;
            }
        }
        drop(inner);
        let clusters = self.cluster_infos();
        st.clusters = clusters.len();
        st.largest_cluster = clusters.iter().map(|c| c.size).max().unwrap_or(0);
        st.era_pure_clusters = clusters.iter().filter(|c| c.w_span <= 1.0).count();
        st.mean_cluster_w_span = if clusters.is_empty() {
            0.0
        } else {
            clusters.iter().map(|c| c.w_span).sum::<f64>() / clusters.len() as f64
        };
        st
    }

    fn shared_vertex_count_locked(&self, inner: &SpaceInner, a: PentaId, b: PentaId) -> usize {
        match (inner.pentas.get(&a), inner.pentas.get(&b)) {
            (Some(pa), Some(pb)) => pa
                .vertex_ids
                .iter()
                .filter(|v| pb.vertex_ids.contains(v))
                .count(),
            _ => 0,
        }
    }

    /// Hypervolume constant — one 5-cell of edge 1 (for density reports).
    pub fn unit_cell_hypervolume() -> f64 {
        hypervolume(EDGE_LENGTH)
    }

    /// Vertex-sharing adjacency with edge weight = number of shared vertices
    /// (the graph PPR diffuses over in `retrieval.rs`). Uncached — see
    /// [`Self::sharing_adjacency_cached`] for the hot path.
    pub fn sharing_adjacency(&self) -> HashMap<PentaId, Vec<(PentaId, f64)>> {
        let inner = self.inner.read().unwrap();
        let mut adj: HashMap<(PentaId, PentaId), f64> = HashMap::new();
        for v in inner.vertices.values() {
            for (i, &a) in v.penta_ids.iter().enumerate() {
                for &b in v.penta_ids.iter().skip(i + 1) {
                    let key = if a < b { (a, b) } else { (b, a) };
                    *adj.entry(key).or_insert(0.0) += 1.0;
                }
            }
        }
        let mut out: HashMap<PentaId, Vec<(PentaId, f64)>> = HashMap::new();
        for ((a, b), w) in adj {
            out.entry(a).or_default().push((b, w));
            out.entry(b).or_default().push((a, w));
        }
        out
    }

    /// Cached adjacency (structure-version keyed) — PPR over a 10k-cell
    /// space drops from ~1.6 s/query (rebuild every time) to microseconds
    /// between mutations.
    pub fn sharing_adjacency_cached(
        &self,
    ) -> std::sync::Arc<HashMap<PentaId, Vec<(PentaId, f64)>>> {
        let version = self.inner.read().unwrap().structure_version;
        if let Some(guard) = self.adj_cache.lock().unwrap().as_ref() {
            if guard.0 == version {
                return guard.1.clone();
            }
        }
        let fresh = std::sync::Arc::new(self.sharing_adjacency());
        *self.adj_cache.lock().unwrap() = Some((version, fresh.clone()));
        fresh
    }

    /// Atomically adjust a cell's importance (dream consolidation bonus).
    pub fn adjust_importance(&self, pid: PentaId, f: impl FnOnce(f64) -> f64) -> Option<f64> {
        let mut inner = self.inner.write().unwrap();
        let p = inner.pentas.get_mut(&pid)?;
        p.importance = f(p.importance).clamp(0.0, 100.0);
        Some(p.importance)
    }

    /// v0.3 era 带压缩（band compaction）。
    ///
    /// 动机（大卫评审）：era 带 W 随 log₂(t) 无限增长 —— 空间年龄每翻倍
    /// W+1，而历史深处的带往往只剩个位数细胞：6×W 晶格被稀疏的"鬼带"
    /// 无限稀释。本方法把人口稀薄的相邻带合并：当相邻两带人口和 <
    /// `threshold` 时，稀带的全体细胞整带平移入密带（平移量 = 带号差，
    /// 即 1.0 w-量子的整数倍；xyz 不变），每带每趟至多参与一次合并
    /// （防链式塌缩，重复调用可继续收敛）。
    ///
    /// **合并后共享量子语义不破坏**（定理级论证，测试固化）：顶点共享
    /// 只能发生在 Δcore.w ∈ {0, ±0.5, ±1.0}（T1′ 的偏移差量子，见
    /// `tests/theorem.rs` 与 canonical_offsets 的时间对称性）。对吸收带
    /// 的全体细胞施加同一整数量子平移 k ∈ ℤ，任意细胞对的 Δcore.w 改变
    /// 0 或 ±k —— 平移后的差值集合仍是量子格 {0,±0.5,±1.0} 的平移，
    /// 不会产生格外的共享通道；而每次落位本身经过 ε-merge + 防堆叠
    /// 机器（insert_penta_locked），引擎级 T1′（对共享 ⊆ {0,1,5}）在
    /// 压缩前后严格一致。
    ///
    /// 搬移逐细胞经 [`Self::relocate_penta`]（v0.3 单锁原子），标签、
    /// 双时态窗口、access_count 全部保留；压缩永不删除细胞。
    pub fn compact_bands(&self, threshold: usize) -> BandCompaction {
        // 读取阶段：带人口普查 + 合并计划（topology 纯函数，便于单测）
        let plan: Vec<(i64, i64)> = {
            let inner = self.inner.read().unwrap();
            let cells: Vec<Pentachoron> = inner.pentas.values().cloned().collect();
            let pops = crate::engine::topology::band_populations(&cells);
            crate::engine::topology::band_merge_plan(&pops, threshold)
        };
        let mut rep = BandCompaction::default();
        for (kept, absorbed) in plan {
            // 整数量子平移：kept − absorbed ∈ ℤ \ {0}（相邻非空带 ±1，跨空带为带号差）
            let shift = (kept - absorbed) as f64;
            let ids: Vec<PentaId> = {
                let inner = self.inner.read().unwrap();
                inner
                    .pentas
                    .values()
                    .filter(|p| era_band(p.core.w) == absorbed)
                    .map(|p| p.id)
                    .collect()
            };
            for id in ids {
                let core = {
                    let inner = self.inner.read().unwrap();
                    inner.pentas.get(&id).map(|p| p.core)
                };
                if let Some(c) = core {
                    if self
                        .relocate_penta(id, Vec4::new(c.x, c.y, c.z, c.w + shift))
                        .is_some()
                    {
                        rep.moved += 1;
                    }
                }
            }
            rep.merged_pairs.push((kept, absorbed));
        }
        rep
    }
}
