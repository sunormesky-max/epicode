//! `Space4` — the 4D space engine, ported from the production-proven
//! hyperspace-shadow implementation (S1 shadow, 43 cycles, issue #369).
//!
//! Parallel structure next to the current 3D `Space` (S4 path A, issue #398):
//! nothing in the engine reads this yet; the grey dials (`space4_enabled`,
//! `space4_kappa`) land in M5. The coupling coefficient κ remains the dial
//! between "epicode today" (κ = 0, degenerate onto xyz) and full 4D.
//!
//! | 3D `Space` | `Space4` |
//! |---|---|
//! | `Point3` / `Tetrahedron` (edge 1.0) | `Vec4` / `Pentachoron` (edge 1.0) |
//! | grid key `(i64,i64,i64)`, 27-neighborhood | `[i64;4]`, 81-neighborhood |
//! | ε-merge at 0.05 → shared `VertexId` | identical, anisotropic dκ |
//! | clusters = components of vertex sharing | identical |
//! | bitemporal fields in payload | bitemporal window IS the w-extent |
//!
//! Engine-level **T1′**: same-orientation 5-cells share exactly 0, 1 or 5
//! vertices — never 2-4 — enforced by the anti-stacking + ε-chain guard in
//! [`Space4::core_stacks`]. Layer identity resolves through the authoritative
//! production router `CylinderLayer::from_labels`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use super::cylinder::CylinderLayer;
use super::geo4::{compute_vertices, w_of_unix_secs, w_range, Vec4};
use super::vertex::VERTEX_MERGE_EPSILON;

/// Spatial hash cell size (same constant as `space.rs::GRID_CELL`).
pub const GRID_CELL: f64 = 1.0;

/// Number of vertices of a 5-cell (mirror of `tetra::VERTEX_COUNT`).
pub const PENTA_VERTICES: usize = 5;

/// Minimum core separation for the anti-stacking guard.
const CORE_MIN_SEPARATION: f64 = VERTEX_MERGE_EPSILON;

/// Beyond this size, `stats()` samples pair-sharing instead of O(P²).
const PAIR_HIST_CENSUS_LIMIT: usize = 2000;

/// Deterministic pair sample for the large-space sharing histogram.
const PAIR_HIST_SAMPLE: usize = 500;

pub type VertexId = super::vertex::VertexId;
pub type PentaId = u64;

#[derive(Clone, Debug)]
pub struct Vertex {
    pub id: VertexId,
    pub position: Vec4,
    /// Pentachora referencing this vertex (sharing = cluster edges).
    pub penta_ids: Vec<PentaId>,
}

/// The cell payload projection Space4 needs from a memory. The production
/// `MemoryPayload` stays the single source of truth; M4 (shadow rewire)
/// projects into this at the ingest boundary.
#[derive(Clone, Debug)]
pub struct CellPayload {
    pub labels: Vec<String>,
    pub importance: f64,
    /// Bitemporal window in absolute unix seconds (`valid_until` =
    /// knowledge-level invalidation).
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

    /// Layer via the authoritative production router.
    fn resolve_layer(&self) -> u8 {
        self.layer
            .unwrap_or_else(|| CylinderLayer::from_labels(&self.labels).index() as u8)
    }
}

/// One memory cell of `Space4` — the 4D counterpart of `Tetrahedron`.
#[derive(Clone, Debug)]
pub struct Pentachoron {
    pub id: PentaId,
    pub vertex_ids: [VertexId; PENTA_VERTICES],
    /// Translation-only center; orientation is global (T1′ premise).
    pub core: Vec4,
    /// Bitemporal window on the w-axis (log₂-compressed time).
    pub w_from: f64,
    pub w_to: Option<f64>,
    pub labels: Vec<String>,
    pub importance: f64,
    /// Recall count — feeds retention stretch and effective importance.
    pub access_count: u64,
    /// Fact-level cease compressed: `[w_expired, w_to)` is the shadow band —
    /// believed true, actually false (Zep `expired_at` semantics).
    pub w_expired: Option<f64>,
    /// Cylinder layer index (see `CylinderLayer`).
    pub layer: u8,
}

/// Bitemporal validity at a compressed time (shadow semantics).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Validity {
    /// Fact true and believed true at the query time.
    True,
    /// Shadow band: fact already ceased, but the system had not learned it
    /// yet — believed true, actually false.
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

#[derive(Clone, Debug, Default)]
pub struct SpaceStats {
    pub pentas: usize,
    pub vertices: usize,
    /// vertices referenced by ≥2 pentas.
    pub vertex_shares: usize,
    /// shared-vertex-count histogram: k → #pairs sharing exactly k vertices.
    /// T1′ predicts mass on {0, 1, 5} only (5 = coincident cores).
    pub pair_sharing_hist: HashMap<usize, usize>,
    pub pair_hist_sampled: bool,
    pub clusters: usize,
    pub largest_cluster: usize,
    /// Clusters whose w-span ≤ 1.0 (one era) — era purity.
    pub era_pure_clusters: usize,
    pub mean_cluster_w_span: f64,
}

struct SpaceInner {
    vertices: HashMap<VertexId, Vertex>,
    pentas: HashMap<PentaId, Pentachoron>,
    /// Vertex spatial hash (ε-merge lookups).
    grid: HashMap<[i64; 4], Vec<VertexId>>,
    /// Core spatial hash — O(1) anti-stacking via 81-neighborhood.
    core_grid: HashMap<[i64; 4], Vec<PentaId>>,
    /// Label inverted index.
    label_index: HashMap<String, Vec<PentaId>>,
    /// Bumped on every structural mutation; invalidates the PPR cache.
    structure_version: u64,
    next_vertex: VertexId,
    next_penta: PentaId,
}

/// The 4D space. `w_coupling` (κ) and `t0_secs` come from the storage meta
/// dials installed by S4 M3 (`space4_kappa`, `space_t0_secs`).
pub struct Space4 {
    inner: RwLock<SpaceInner>,
    /// PPR adjacency cache, invalidated by `structure_version`
    /// (the 4D twin of `Space`'s `search_revision` BM25 cache).
    adj_cache: Mutex<Option<(u64, Arc<HashMap<PentaId, Vec<(PentaId, f64)>>>)>>,
    /// Time-coupling coefficient κ ∈ [0, 1].
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
            adj_cache: Mutex::new(None),
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

    /// (2r+1)⁴ grid keys within Chebyshev radius `r` of `k`.
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

    /// O(1) anti-stacking probe via the core grid.
    ///
    /// Two rejection rules:
    /// 1. anisotropic core distance < ε;
    /// 2. **ε-chain rule**: identical xyz pattern (distance < ε) with era
    ///    gap ≤ 2ε — two such cells would each legitimately ε-merge into the
    ///    SAME older pivot vertices (ε-merge transitivity), producing
    ///    degenerate 2/3/4-shares that break T1′ at engine level. Same-era
    ///    near-duplicates are duplicates; different eras (Δw ≥ 0.5) coexist.
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
                    let d = c - core;
                    if d.x * d.x + d.y * d.y + d.z * d.z < eps_sq && (kappa * d.w).abs() < chain_w {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// How many of the 5 candidate vertices at `core` would ε-merge with
    /// existing vertices? (placement scoring input; M5).
    pub fn count_vertex_merges(&self, core: Vec4) -> usize {
        let inner = self.inner.read().unwrap();
        compute_vertices(core)
            .iter()
            .filter(|v| Self::find_merge_target(&inner, **v, self.w_coupling).is_some())
            .count()
    }

    /// Insert one pentachoron at `core`. Returns `None` if the core stacks on
    /// an existing core (anti-stacking + ε-chain guard — without it, ε-fuzzy
    /// accumulations of near-identical cells produce degenerate 2/3/4-vertex
    /// shares and destroy T1′). On success returns the id and the number of
    /// vertices ε-merged with existing cells (the sharing event).
    pub fn insert_penta(&self, core: Vec4, payload: CellPayload) -> Option<(PentaId, usize)> {
        let mut inner = self.inner.write().unwrap();
        Self::insert_penta_locked(&mut inner, self.w_coupling, self.t0_secs, core, payload, 0)
    }

    /// Locked core of [`Self::insert_penta`] — operates on a held write guard
    /// so multi-step moves (relocate, and later lifecycle ops) can compose
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
        let layer = payload.resolve_layer();
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

    /// Locked core of [`Self::remove_penta`].
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
    /// Mirrors `Space::relocate_tetrahedron` ("删旧插新"). Returns the NEW id
    /// (ids are not stable across relocation). If the target core is stacked,
    /// retries along small deterministic shifts — a cell must NEVER be lost
    /// to a stacked target (relocation is not deletion). `access_count`
    /// survives relocation.
    ///
    /// Single-lock atomicity: read payload → remove → re-insert all inside
    /// ONE write acquisition; concurrent readers never observe a mid-move
    /// "cell vanished" state.
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
                    // w is stored compressed — decompress, insert recompresses.
                    valid_from: unix_secs_of_w(p.w_from, self.t0_secs),
                    valid_until: p.w_to.map(|hi| unix_secs_of_w(hi, self.t0_secs)),
                    expired_at: p.w_expired.map(|e| unix_secs_of_w(e, self.t0_secs)),
                    layer: Some(p.layer),
                },
                p.access_count,
                p.core,
            )
        };
        Self::remove_penta_locked(&mut inner, pid);
        // Deterministic lattice of fallback spots.
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
        // OLD core (just vacated by this very move) — the move degrades to a
        // no-op instead of losing the cell.
        Self::insert_penta_locked(
            &mut inner,
            self.w_coupling,
            self.t0_secs,
            old_core,
            payload,
            access,
        )
    }

    /// Register a recall: bumps `access_count` (feeds retention stretch and
    /// effective importance). The cheapest lifecycle hook.
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

    // ---- clusters / queries -------------------------------------------

    /// Clusters = connected components of the vertex-sharing graph
    /// (mirrors `Space::find_clusters`).
    pub fn find_clusters(&self) -> Vec<Vec<PentaId>> {
        self.cluster_infos().into_iter().map(|c| c.ids).collect()
    }

    /// Clusters with structure summaries (era span, top label).
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
    /// compressed validity window contains `as_of_secs`.
    pub fn slice_as_of(&self, as_of_secs: i64) -> Vec<PentaId> {
        let w = w_of_unix_secs(as_of_secs, self.t0_secs);
        let inner = self.inner.read().unwrap();
        let mut ids: Vec<PentaId> = inner
            .pentas
            .values()
            .filter(|p| valid_at(p.w_from, p.w_to, w))
            .map(|p| p.id)
            .collect();
        ids.sort_unstable();
        ids
    }

    /// ONE-lock, zero-clone as_of snapshot with label overlap and shadow
    /// validity — the hot path for the future Space-PPR retrieval channel
    /// (M6). 10k cells in microseconds instead of 10k lock+clone trips.
    pub fn snapshot_valid(&self, q_labels: &[String], as_of_secs: i64) -> Vec<SnapshotCell> {
        let w = w_of_unix_secs(as_of_secs, self.t0_secs);
        let inner = self.inner.read().unwrap();
        let mut out = Vec::new();
        for p in inner.pentas.values() {
            if !valid_at(p.w_from, p.w_to, w) {
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

    /// Point lookup with clone (M4 load path / MCP tooling).
    pub fn get(&self, pid: PentaId) -> Option<Pentachoron> {
        let inner = self.inner.read().unwrap();
        inner.pentas.get(&pid).cloned()
    }

    /// All cell ids, ascending.
    pub fn all_ids(&self) -> Vec<PentaId> {
        let inner = self.inner.read().unwrap();
        let mut ids: Vec<PentaId> = inner.pentas.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// Adjust importance under the write lock (governor decay hook, M7).
    pub fn adjust_importance(&self, pid: PentaId, f: impl FnOnce(f64) -> f64) -> Option<f64> {
        let mut inner = self.inner.write().unwrap();
        let p = inner.pentas.get_mut(&pid)?;
        p.importance = f(p.importance);
        Some(p.importance)
    }

    // ---- structure stats ----------------------------------------------

    /// Structure report: sizes, sharing histogram (T1′ census), clusters,
    /// era purity. Beyond `PAIR_HIST_CENSUS_LIMIT` cells the pair histogram
    /// samples deterministic pairs instead of O(P²).
    pub fn stats(&self) -> SpaceStats {
        let inner = self.inner.read().unwrap();
        let pentas = inner.pentas.len();
        let vertices = inner.vertices.len();
        let vertex_shares = inner
            .vertices
            .values()
            .filter(|v| v.penta_ids.len() >= 2)
            .count();

        let ids = Self::sorted_ids(&inner);
        let (pair_sharing_hist, pair_hist_sampled) = if pentas <= PAIR_HIST_CENSUS_LIMIT {
            let mut hist: HashMap<usize, usize> = HashMap::new();
            for (i, &a) in ids.iter().enumerate() {
                for &b in ids.iter().skip(i + 1) {
                    *hist
                        .entry(Self::shared_count_locked(&inner, a, b))
                        .or_insert(0) += 1;
                }
            }
            (hist, false)
        } else {
            let mut hist: HashMap<usize, usize> = HashMap::new();
            let mut s = 0x9E3779B97F4A7C15u64;
            for _ in 0..PAIR_HIST_SAMPLE {
                s ^= s >> 12;
                s ^= s << 25;
                s ^= s >> 27;
                let r = s.wrapping_mul(0x2545F4914F6CDD1D);
                let a = ids[(r % pentas as u64) as usize];
                let b = ids[((r >> 32) % pentas as u64) as usize];
                if a == b {
                    continue;
                }
                *hist
                    .entry(Self::shared_count_locked(&inner, a, b))
                    .or_insert(0) += 1;
            }
            (hist, true)
        };

        // Cluster summaries (reuse cluster_infos logic inline under this lock).
        let clusters = self.cluster_infos_locked(&inner);
        let era_pure = clusters.iter().filter(|c| c.w_span <= 1.0).count();
        let mean_span = if clusters.is_empty() {
            0.0
        } else {
            clusters.iter().map(|c| c.w_span).sum::<f64>() / clusters.len() as f64
        };

        SpaceStats {
            pentas,
            vertices,
            vertex_shares,
            pair_sharing_hist,
            pair_hist_sampled,
            clusters: clusters.len(),
            largest_cluster: clusters.iter().map(|c| c.size).max().unwrap_or(0),
            era_pure_clusters: era_pure,
            mean_cluster_w_span: mean_span,
        }
    }

    fn sorted_ids(inner: &SpaceInner) -> Vec<PentaId> {
        let mut ids: Vec<PentaId> = inner.pentas.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    fn shared_count_locked(inner: &SpaceInner, a: PentaId, b: PentaId) -> usize {
        match (inner.pentas.get(&a), inner.pentas.get(&b)) {
            (Some(pa), Some(pb)) => pa
                .vertex_ids
                .iter()
                .filter(|v| pb.vertex_ids.contains(v))
                .count(),
            _ => 0,
        }
    }

    fn cluster_infos_locked(&self, inner: &SpaceInner) -> Vec<ClusterInfo> {
        let mut adj: HashMap<PentaId, Vec<PentaId>> = HashMap::new();
        for v in inner.vertices.values() {
            for (i, &a) in v.penta_ids.iter().enumerate() {
                for &b in v.penta_ids.iter().skip(i + 1) {
                    adj.entry(a).or_default().push(b);
                    adj.entry(b).or_default().push(a);
                }
            }
        }
        let mut clusters = Vec::new();
        let mut seen: HashMap<PentaId, bool> = HashMap::new();
        for pid in Self::sorted_ids(inner) {
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
            for &c in &comp {
                let p = &inner.pentas[&c];
                w_min = w_min.min(p.core.w);
                w_max = w_max.max(p.core.w);
            }
            clusters.push(ClusterInfo {
                size: comp.len(),
                w_span: w_max - w_min,
                w_min,
                w_max,
                top_label: inner.pentas[&comp[0]]
                    .labels
                    .first()
                    .cloned()
                    .unwrap_or_default(),
                ids: comp,
            });
        }
        clusters
    }

    // ---- PPR adjacency (M6 retrieval foundation) ----------------------

    /// Vertex-sharing adjacency with weights = shared vertex count
    /// (the diffusion graph for Space-PPR).
    pub fn sharing_adjacency(&self) -> HashMap<PentaId, Vec<(PentaId, f64)>> {
        let inner = self.inner.read().unwrap();
        Self::sharing_adjacency_locked(&inner)
    }

    fn sharing_adjacency_locked(inner: &SpaceInner) -> HashMap<PentaId, Vec<(PentaId, f64)>> {
        let mut adj: HashMap<PentaId, HashMap<PentaId, f64>> = HashMap::new();
        for v in inner.vertices.values() {
            for (i, &a) in v.penta_ids.iter().enumerate() {
                for &b in v.penta_ids.iter().skip(i + 1) {
                    *adj.entry(a).or_default().entry(b).or_insert(0.0) += 1.0;
                    *adj.entry(b).or_default().entry(a).or_insert(0.0) += 1.0;
                }
            }
        }
        adj.into_iter()
            .map(|(k, m)| {
                let mut v: Vec<(PentaId, f64)> = m.into_iter().collect();
                v.sort_unstable_by_key(|(id, _)| *id);
                (k, v)
            })
            .collect()
    }

    /// Cached [`Self::sharing_adjacency`], invalidated by structure_version.
    pub fn sharing_adjacency_cached(&self) -> Arc<HashMap<PentaId, Vec<(PentaId, f64)>>> {
        let version = {
            let inner = self.inner.read().unwrap();
            inner.structure_version
        };
        {
            let cache = self.adj_cache.lock().unwrap();
            if let Some((v, adj)) = cache.as_ref() {
                if *v == version {
                    return Arc::clone(adj);
                }
            }
        }
        let adj = Arc::new(self.sharing_adjacency());
        *self.adj_cache.lock().unwrap() = Some((version, Arc::clone(&adj)));
        adj
    }

    /// Current structure version (cache-coherence diagnostics).
    pub fn structure_version(&self) -> u64 {
        self.inner.read().unwrap().structure_version
    }
}

/// Inverse of `geo4::w_of_unix_secs` (re-exported for relocate/rejuvenate).
fn unix_secs_of_w(w: f64, t0_secs: i64) -> i64 {
    super::geo4::unix_secs_of_w(w, t0_secs)
}

/// Half-open bitemporal validity on the compressed axis.
fn valid_at(w_from: f64, w_to: Option<f64>, w_as_of: f64) -> bool {
    super::geo4::valid_at(w_from, w_to, w_as_of)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::geo4::canonical_offsets;

    const T0: i64 = 1_767_225_600; // 2026-01-01T00:00:00Z
    const DAY: i64 = 86_400;

    fn payload(labels: &[&str], valid_from_day: i64) -> CellPayload {
        CellPayload::new(
            &labels.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            1.0,
            T0 + valid_from_day * DAY,
            None,
        )
    }

    /// The docking translation: a cell placed at `anchor + (o₁ − o₀)` has its
    /// vertex 0 exactly on the anchor's vertex 1 — the canonical 1-share.
    fn dock_step() -> Vec4 {
        let o = canonical_offsets();
        o[1] - o[0]
    }

    #[test]
    fn insert_merges_vertices_within_epsilon() {
        let s = Space4::new(1.0, T0);
        let a = Vec4::new(0.0, 0.0, 0.0, 3.0);
        let b = Vec4::new(0.0, 0.0, 0.0, 6.0); // far: separate cell
        let (ida, shared_a) = s.insert_penta(a, payload(&["kg"], 10)).unwrap();
        let (idb, shared_b) = s.insert_penta(b, payload(&["kg"], 10)).unwrap();
        assert_eq!(shared_a, 0, "first cell merges nothing");
        assert_eq!(shared_b, 0);
        // Coincident core is REJECTED by anti-stack; the docking offset
        // (o₁ − o₀) lands one vertex exactly on the anchor's vertex.
        let dock = a + dock_step();
        let (idc, shared_c) = s.insert_penta(dock, payload(&["kg"], 10)).unwrap();
        assert_eq!(shared_c, 1, "docking shares exactly one vertex");
        assert_eq!(s.shared_vertex_count(ida, idb), 0);
        assert_eq!(s.shared_vertex_count(ida, idc), 1);
    }

    #[test]
    fn anti_stack_rejects_duplicate_core() {
        let s = Space4::new(1.0, T0);
        let core = Vec4::new(1.5, -2.0, 0.5, 4.0);
        assert!(s.insert_penta(core, payload(&["ai"], 20)).is_some());
        assert!(
            s.insert_penta(core, payload(&["ai"], 20)).is_none(),
            "same core must be rejected (anti-stacking)"
        );
    }

    #[test]
    fn epsilon_chain_guard_rejects_same_xyz_small_dw() {
        // The probe2 regression: co-located xyz with tiny Δw would both
        // ε-merge into the same older pivot vertices → degenerate 2/3/4
        // shares → T1′ broken at engine level.
        let s = Space4::new(1.0, T0);
        let a = Vec4::new(0.0, 0.0, 0.0, 3.0);
        assert!(s.insert_penta(a, payload(&["kg"], 10)).is_some());
        let near = Vec4::new(0.01, 0.01, 0.01, 3.04);
        assert!(
            s.insert_penta(near, payload(&["kg"], 10)).is_none(),
            "same-era near-duplicate must be rejected by the ε-chain rule"
        );
        // Different era at the same xyz coexists freely (Δw ≥ 0.5).
        let other_era = Vec4::new(0.0, 0.0, 0.0, 5.0);
        assert!(s.insert_penta(other_era, payload(&["kg"], 10)).is_some());
    }

    #[test]
    fn clusters_follow_vertex_sharing() {
        let s = Space4::new(1.0, T0);
        let a = Vec4::new(0.0, 0.0, 0.0, 3.0);
        let docked = a + dock_step();
        s.insert_penta(a, payload(&["kg"], 10)).unwrap();
        s.insert_penta(docked, payload(&["kg"], 10)).unwrap();
        s.insert_penta(Vec4::new(50.0, 50.0, 50.0, 3.0), payload(&["ai"], 10))
            .unwrap();
        let clusters = s.find_clusters();
        assert_eq!(clusters.len(), 2, "docked pair + loner");
        assert_eq!(
            clusters.iter().map(|c| c.len()).max().unwrap(),
            2,
            "docked pair clusters together"
        );
    }

    #[test]
    fn snapshot_valid_windows_and_shadow_band() {
        let s = Space4::new(1.0, T0);
        // Window [day 10, day 40)
        let mut p = payload(&["kg"], 10);
        p.valid_until = Some(T0 + 40 * DAY);
        s.insert_penta(Vec4::new(0.0, 0.0, 0.0, w10()), p).unwrap();
        // Fact ceased at day 20 but knowledge valid until day 40 (shadow band)
        let mut q = payload(&["kg"], 10);
        q.valid_until = Some(T0 + 40 * DAY);
        q.expired_at = Some(T0 + 20 * DAY);
        s.insert_penta(Vec4::new(9.0, 0.0, 0.0, w10()), q).unwrap();

        let at = |d: i64| s.snapshot_valid(&[], T0 + d * DAY);
        assert_eq!(at(5).len(), 0, "before window");
        assert_eq!(at(15).len(), 2, "inside window");
        assert_eq!(
            at(15)
                .iter()
                .filter(|c| c.validity == Validity::True)
                .count(),
            2
        );
        let at25 = at(25);
        assert_eq!(at25.len(), 2, "still believed valid at 25");
        assert_eq!(
            at25.iter()
                .filter(|c| c.validity == Validity::BelievedStale)
                .count(),
            1,
            "shadow band marks the ceased fact"
        );
        assert_eq!(at(45).len(), 0, "after window");
    }

    fn w10() -> f64 {
        w_of_unix_secs(T0 + 10 * DAY, T0)
    }

    #[test]
    fn relocate_preserves_payload_and_never_loses_cells() {
        let s = Space4::new(1.0, T0);
        let (ida, _) = s
            .insert_penta(Vec4::new(0.0, 0.0, 0.0, 3.0), payload(&["identity"], 10))
            .unwrap();
        s.touch(ida);
        s.touch(ida);
        let (idb, _) = s
            .relocate_penta(ida, Vec4::new(20.0, 20.0, 20.0, 3.0))
            .unwrap();
        assert_ne!(ida, idb, "ids are not stable across relocation");
        let moved = s.get(idb).unwrap();
        assert_eq!(moved.labels, vec!["identity".to_string()]);
        assert_eq!(moved.access_count, 2, "access survives relocation");
        assert!((moved.core.x - 20.0).abs() < 1e-9);
        assert!(s.get(ida).is_none(), "old id vacated");

        // Relocate onto a stacked target: fallback lattice must still land.
        let (idc, _) = s
            .insert_penta(Vec4::new(0.0, 0.0, 0.0, 7.0), payload(&["ai"], 30))
            .unwrap();
        let (idd, _) = s
            .relocate_penta(idc, Vec4::new(0.0, 0.0, 0.0, 7.0))
            .expect("relocation to a stacked target degrades to no-op, never loses the cell");
        assert!(s.get(idd).is_some());
    }

    #[test]
    fn layer_routing_uses_production_from_labels() {
        let s = Space4::new(1.0, T0);
        s.insert_penta(Vec4::new(0.0, 0.0, 0.0, 3.0), payload(&["identity"], 10))
            .unwrap();
        s.insert_penta(Vec4::new(9.0, 0.0, 0.0, 3.0), payload(&["random-xyz"], 10))
            .unwrap();
        let ids = s.all_ids();
        assert_eq!(s.get(ids[0]).unwrap().layer, 5, "identity → Identity layer");
        assert_eq!(
            s.get(ids[1]).unwrap().layer,
            0,
            "unknown → Instinct fallback"
        );
    }

    #[test]
    fn adjacency_cache_invalidates_on_mutation() {
        let s = Space4::new(1.0, T0);
        let anchor = Vec4::new(0.0, 0.0, 0.0, 3.0);
        s.insert_penta(anchor, payload(&["kg"], 10)).unwrap();
        let a1 = s.sharing_adjacency_cached();
        assert!(a1.is_empty(), "single cell has no sharing edges");
        s.insert_penta(anchor + dock_step(), payload(&["kg"], 10))
            .unwrap();
        let a2 = s.sharing_adjacency_cached();
        assert_eq!(a2.len(), 2, "cache must reflect the new sharing edge");
    }

    #[test]
    fn stats_pair_histogram_supports_t1_prime() {
        let s = Space4::new(1.0, T0);
        // A chain of 4 cells, each docked on its predecessor by one vertex.
        let mut core = Vec4::new(0.0, 0.0, 0.0, 3.0);
        for _ in 0..4 {
            assert!(s.insert_penta(core, payload(&["kg"], 10)).is_some());
            core = core + dock_step();
        }
        let st = s.stats();
        assert_eq!(st.pentas, 4);
        assert_eq!(st.clusters, 1, "the chain is one connected cluster");
        // All pair-share counts must fall on {0, 1, 5} (T1′ support).
        for k in st.pair_sharing_hist.keys() {
            assert!(
                matches!(k, 0 | 1 | 5),
                "engine-level T1′ violated: share count {k}"
            );
        }
    }

    #[test]
    fn by_label_dedups_across_labels() {
        let s = Space4::new(1.0, T0);
        s.insert_penta(Vec4::new(0.0, 0.0, 0.0, 3.0), payload(&["kg", "ai"], 10))
            .unwrap();
        s.insert_penta(Vec4::new(9.0, 0.0, 0.0, 3.0), payload(&["kg"], 10))
            .unwrap();
        let ids = s.by_label(&["kg".to_string(), "ai".to_string()]);
        assert_eq!(ids.len(), 2, "union, deduped");
    }

    #[test]
    fn era_band_helper_agrees_with_grid() {
        // Sanity for the band bookkeeping used by stats/reporting.
        let w = w_of_unix_secs(T0 + 3 * DAY, T0);
        assert_eq!(crate::domain::geo4::era_band(w), w.floor() as i64);
    }
}
