//! Four-dimensional geometry for the S4 space upgrade (4D memory research).
//!
//! Three pure-geometry sections, no engine/storage dependencies:
//!
//! 1. [`Vec4`]/[`Point4`] — points in E⁴. `xyz` keeps the semantic/label
//!    topology of the current 3D space; `w` carries the bitemporal axis
//!    (log₂-compressed time, see below). The anisotropic metric
//!    `dκ(a,b)² = |Δxyz|² + (κ·Δw)²` degenerates exactly onto the current
//!    [`super::vertex::Point3`] behaviour at `κ = 0` (degeneration theorem)
//!    — the property that makes κ = 0 a lossless rollout starting point.
//! 2. Time axis — `w(t) = log₂(1 + days since space birth t₀)`. One unit of
//!    w is one doubling of age (Weber–Fechner resolution); era boundaries
//!    anchor to the space's own birth, not the unix epoch.
//! 3. 5-cell (pentachoron, {3,3,3}) geometry — the 4D counterpart of the
//!    regular tetrahedron: 5 vertices, 10 edges, circumradius a·√(2/5).
//!    Every cell shares ONE canonical orientation and differs only by
//!    translation, so cross-cell vertex sharing obeys theorem T1′
//!    (two such cells share exactly 0, 1 or 5 vertices — never 2-4).
//! 4. The 6×W port lattice — the 4D evolution of the cylinder's per-layer
//!    ports: every (layer, era-band) crossing gets a deterministic
//!    [`Point4`] anchor. Topics live in layers, eras live in bands; the
//!    home address of a cluster is (layer, band).
//!
//! Layer identity is NOT redefined here: [`super::cylinder::CylinderLayer`]
//! (in particular its `from_labels` keyword routing) stays the single
//! authoritative router. This module only adds the band dimension.

use super::cylinder::CylinderLayer;

/// Seconds per day.
pub const SECS_PER_DAY: f64 = 86400.0;

// ---------------------------------------------------------------------------
// Vec4 / Point4
// ---------------------------------------------------------------------------

/// A point/vector in E⁴. `Point4` is an alias — same arithmetic, different
/// intent. `xyz` keeps the semantic/label topology of the current 3D space;
/// `w` carries the bitemporal axis (log₂-compressed time, see the time-axis
/// section below).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec4 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

pub type Point4 = Vec4;

impl Vec4 {
    pub const fn new(x: f64, y: f64, z: f64, w: f64) -> Self {
        Self { x, y, z, w }
    }

    /// Spatial-only constructor with `w = 0` (the space-birth hyperplane).
    /// Time-aware callers must pass `w` explicitly — this exists to keep
    /// 3D-era call sites and test helpers minimal-diff during the port.
    pub const fn new3(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z, w: 0.0 }
    }

    pub const fn zero() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 0.0,
        }
    }

    pub fn scale(self, s: f64) -> Vec4 {
        Vec4::new(self.x * s, self.y * s, self.z * s, self.w * s)
    }

    pub fn dot(self, o: Vec4) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z + self.w * o.w
    }

    pub fn length_sq(self) -> f64 {
        self.dot(self)
    }

    pub fn length(self) -> f64 {
        self.length_sq().sqrt()
    }

    pub fn distance(a: Vec4, b: Vec4) -> f64 {
        (a - b).length()
    }

    pub fn distance_sq(a: Vec4, b: Vec4) -> f64 {
        (a - b).length_sq()
    }

    /// Linear interpolation, `t = 0` → self, `t = 1` → `o`.
    pub fn lerp(self, o: Vec4, t: f64) -> Vec4 {
        self + (o - self).scale(t)
    }

    /// The midpoint between two points.
    pub fn midpoint(self, o: Vec4) -> Vec4 {
        self.lerp(o, 0.5)
    }

    /// Anisotropic distance used by the 4D ε-merge:
    /// `dκ(a,b)² = |Δxyz|² + (κ · Δw)²`.
    ///
    /// `κ = 0` collapses the metric onto the xyz subspace — the merge
    /// behaviour of the current 3D space, exactly (degeneration theorem).
    /// `κ = 1` is the fully coupled 4D metric; intermediate κ is the
    /// time-coupling dial of the S4 rollout.
    pub fn aniso_distance_sq(a: Vec4, b: Vec4, kappa: f64) -> f64 {
        let d = a - b;
        d.x * d.x + d.y * d.y + d.z * d.z + kappa * kappa * d.w * d.w
    }

    pub fn aniso_distance(a: Vec4, b: Vec4, kappa: f64) -> f64 {
        Self::aniso_distance_sq(a, b, kappa).sqrt()
    }

    /// Euclidean projection onto the xyz subspace (drop w).
    pub fn xyz(self) -> (f64, f64, f64) {
        (self.x, self.y, self.z)
    }
}

impl std::ops::Add for Vec4 {
    type Output = Vec4;
    fn add(self, o: Vec4) -> Vec4 {
        Vec4::new(self.x + o.x, self.y + o.y, self.z + o.z, self.w + o.w)
    }
}

impl std::ops::Sub for Vec4 {
    type Output = Vec4;
    fn sub(self, o: Vec4) -> Vec4 {
        Vec4::new(self.x - o.x, self.y - o.y, self.z - o.z, self.w - o.w)
    }
}

/// Uniform edge length of every 5-cell — same constant as `tetra::EDGE_LENGTH`.
pub const EDGE_LENGTH: f64 = 1.0;

/// Number of vertices of a 4-simplex (5-cell).
pub const PENTA_VERTICES: usize = 5;

/// Number of edges of a 4-simplex.
pub const PENTA_EDGES: usize = 10;

/// Cylinder radius for port placement (mirrors `cylinder::INITIAL_RADIUS`).
pub const PORT_RADIUS: f64 = 2.0;

/// Height of one layer's z-band (mirrors `cylinder::INITIAL_HEIGHT / 6`).
pub const LAYER_HEIGHT: f64 = 2.0;

// ---------------------------------------------------------------------------
// Time axis: the w coordinate
// ---------------------------------------------------------------------------

/// Compressed coordinate of absolute unix time `secs`, anchored at space
/// birth `t0_secs`. Monotone increasing; `w(t0) = 0`.
///
/// Doubling the age of anything costs exactly Δw = 1: recent eras are
/// resolved finely, ancient ones coarsely, at constant *relative* resolution.
/// `w = 0` is the birth moment; the present surface `w(now)` is the largest
/// w in the space — the space becomes a geological record of its own past.
pub fn w_of_unix_secs(secs: i64, t0_secs: i64) -> f64 {
    let days = (secs.max(t0_secs) - t0_secs) as f64 / SECS_PER_DAY;
    (1.0 + days).log2()
}

/// Inverse of [`w_of_unix_secs`]: absolute unix time of a w coordinate.
pub fn unix_secs_of_w(w: f64, t0_secs: i64) -> i64 {
    t0_secs + (((2.0_f64).powf(w) - 1.0) * SECS_PER_DAY) as i64
}

/// Bitemporal window of a memory, converted to w coordinates.
/// `valid_until = None` (still true) maps to an open upper bound.
pub fn w_range(valid_from: i64, valid_until: Option<i64>, t0_secs: i64) -> (f64, Option<f64>) {
    let lo = w_of_unix_secs(valid_from, t0_secs);
    let hi = valid_until.map(|u| w_of_unix_secs(u, t0_secs));
    (lo, hi)
}

/// How many days of linear time one Δw of compressed time covers.
pub fn days_spanned_by(dw: f64) -> f64 {
    (2.0_f64).powf(dw) - 1.0
}

/// Is a cell with window `[w_from, w_to)` valid *at* compressed time `w_as_of`?
/// Same half-open semantics as the bitemporal `as_of` filter.
pub fn valid_at(w_from: f64, w_to: Option<f64>, w_as_of: f64) -> bool {
    w_as_of >= w_from && w_to.is_none_or(|hi| w_as_of < hi)
}

// ---------------------------------------------------------------------------
// 5-cell (pentachoron) geometry
// ---------------------------------------------------------------------------

/// Circumradius R of a regular 4-simplex with edge `a`: `R = a·√(2/5)`.
pub fn circumradius(a: f64) -> f64 {
    a * (2.0_f64 / 5.0_f64).sqrt()
}

/// Center-to-vertex distance of the unit 5-cell.
pub fn center_to_vertex() -> f64 {
    circumradius(EDGE_LENGTH)
}

/// Hypervolume (4-content) of a regular 4-simplex with edge `a`:
/// `V₄ = a⁴·√5/96`.
pub fn hypervolume(a: f64) -> f64 {
    a.powi(4) * 5.0_f64.sqrt() / 96.0
}

/// 3-volume of one boundary cell — every facet is a regular tetrahedron of
/// the same edge length (compare `tetra::volume` = a³√2/12).
pub fn facet_volume(a: f64) -> f64 {
    a.powi(3) * 2.0_f64.sqrt() / 12.0
}

/// Canonical vertex offsets of the unit 5-cell centered at the origin.
///
/// Construction (deterministic, no magic constants):
/// 1. In ℝ⁵ take the standard basis `e₁..e₅` and project onto the hyperplane
///    `Σx = 0`: `pᵢ = eᵢ − 𝟙/5`. These 5 points form a regular 4-simplex
///    with edge √2 and centroid 0.
/// 2. Gram–Schmidt the first four `pᵢ` into an orthonormal basis of that 4D
///    subspace, producing explicit ℝ⁴ coordinates for every `pᵢ`.
/// 3. Rescale edge √2 → [`EDGE_LENGTH`].
///
/// Result: `|oᵢ| = √(2/5)`, `Σoᵢ = 0`, all 10 pairwise distances = 1.
pub fn canonical_offsets() -> [Vec4; 5] {
    // Step 1: regular 4-simplex with edge √2 embedded in the sum-zero
    // hyperplane of ℝ⁵.
    let mut p5 = [[0.0_f64; 5]; 5];
    for (i, row) in p5.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = if i == j { 0.8 } else { -0.2 };
        }
    }

    // Step 2: Gram–Schmidt over p5[0..4] (p5[4] is linearly dependent on them).
    let dot5 = |a: &[f64; 5], b: &[f64; 5]| a.iter().zip(b.iter()).map(|(x, y)| x * y).sum::<f64>();
    let mut basis: Vec<[f64; 5]> = Vec::with_capacity(4);
    for p in p5.iter().take(4) {
        let mut v = *p;
        for u in &basis {
            let proj = dot5(&v, u);
            for (vk, uk) in v.iter_mut().zip(u.iter()) {
                *vk -= proj * uk;
            }
        }
        let norm = dot5(&v, &v).sqrt();
        for vk in v.iter_mut() {
            *vk /= norm;
        }
        basis.push(v);
    }

    // Step 3: coordinates in ℝ⁴ + rescale edge √2 → 1.
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let mut offsets = [Vec4::zero(); 5];
    for (offset, p) in offsets.iter_mut().zip(p5.iter()) {
        *offset = Vec4::new(
            dot5(p, &basis[0]) * s,
            dot5(p, &basis[1]) * s,
            dot5(p, &basis[2]) * s,
            dot5(p, &basis[3]) * s,
        );
    }
    offsets
}

/// The 5 vertices of a pentachoron centered at `core` (translation only —
/// orientation is global, the key invariant of the model).
pub fn compute_vertices(core: Vec4) -> [Vec4; 5] {
    let o = canonical_offsets();
    [
        core.add(o[0]),
        core.add(o[1]),
        core.add(o[2]),
        core.add(o[3]),
        core.add(o[4]),
    ]
}

/// All 10 edges of a vertex set, as index pairs.
pub const fn edge_index_pairs() -> [(usize, usize); PENTA_EDGES] {
    [
        (0, 1),
        (0, 2),
        (0, 3),
        (0, 4),
        (1, 2),
        (1, 3),
        (1, 4),
        (2, 3),
        (2, 4),
        (3, 4),
    ]
}

/// Shape invariant check: all 10 edges must equal [`EDGE_LENGTH`] (within 1e-9).
pub fn validate_shape(vertices: &[Vec4; 5]) -> bool {
    let tol = 1e-9;
    edge_index_pairs()
        .iter()
        .all(|&(i, j)| (Vec4::distance(vertices[i], vertices[j]) - EDGE_LENGTH).abs() < tol)
}

/// Barycentric coordinates of `p` w.r.t. the 5-cell: solve the 5×5 system
/// `Σλᵢvᵢ = p`, `Σλᵢ = 1` by Gaussian elimination with partial pivoting.
/// Returns `None` for a degenerate vertex set.
pub fn barycentric(vertices: &[Vec4; 5], p: Vec4) -> Option<[f64; 5]> {
    // Rows 0..3: coordinates, row 4: normalization Σλ = 1.
    let mut m = [[0.0_f64; 5]; 5]; // m[row][col], col = λ index
    let vs: [(f64, f64, f64, f64); 5] = vertices.map(|v| (v.x, v.y, v.z, v.w));
    let pv = (p.x, p.y, p.z, p.w);
    for c in 0..5 {
        m[0][c] = vs[c].0;
        m[1][c] = vs[c].1;
        m[2][c] = vs[c].2;
        m[3][c] = vs[c].3;
        m[4][c] = 1.0;
    }
    let mut rhs = [pv.0, pv.1, pv.2, pv.3, 1.0];

    for col in 0..5 {
        // Partial pivot.
        let mut best = col;
        for r in col + 1..5 {
            if m[r][col].abs() > m[best][col].abs() {
                best = r;
            }
        }
        if m[best][col].abs() < 1e-12 {
            return None; // degenerate simplex
        }
        m.swap(col, best);
        rhs.swap(col, best);
        let piv = m[col][col];
        for r in col + 1..5 {
            let f = m[r][col] / piv;
            if f == 0.0 {
                continue;
            }
            for c in col..5 {
                m[r][c] -= f * m[col][c];
            }
            rhs[r] -= f * rhs[col];
        }
    }
    // Back substitution.
    let mut lam = [0.0_f64; 5];
    for r in (0..5).rev() {
        let mut s = rhs[r];
        for c in r + 1..5 {
            s -= m[r][c] * lam[c];
        }
        lam[r] = s / m[r][r];
    }
    Some(lam)
}

/// Point-in-cell test via barycentric coordinates (all λᵢ ≥ −1e-9).
pub fn contains(vertices: &[Vec4; 5], p: Vec4) -> bool {
    match barycentric(vertices, p) {
        Some(lam) => lam.iter().all(|&l| l >= -1e-9),
        None => false,
    }
}

// ---------------------------------------------------------------------------
// The 6 × W port lattice
// ---------------------------------------------------------------------------

/// Era band index: one band = one w-unit = one doubling of age.
///
/// Bands quantize w on the same clock the T1′ sharing quanta use, so
/// vertex-sharing never crosses a band boundary except through deliberate
/// docking — the lattice and the merge structure share one clock.
pub fn era_band(w: f64) -> i64 {
    w.floor() as i64
}

/// Center of a band (used for port w and era seeding).
pub fn band_center(band: i64) -> f64 {
    band as f64 + 0.5
}

/// Deterministic core of the port at (layer, band) — the 4D evolution of the
/// cylinder's per-layer ports.
///
/// Angle opens by 60° per layer and advances 0.61 rad per band (spiral), so
/// any two distinct (layer, band) anchors stay far beyond the ε-merge
/// threshold — ports never merge with each other. z stays at the layer's
/// band centre (the z axis keeps its layer semantics); w sits at the era
/// band centre.
pub fn port_core(layer: CylinderLayer, band: i64) -> Vec4 {
    let idx = layer.index() as f64;
    let angle = idx * (std::f64::consts::PI / 3.0) + (band as f64) * 0.61;
    let (s, c) = angle.sin_cos();
    let z = idx * LAYER_HEIGHT + LAYER_HEIGHT * 0.5;
    Vec4::new(PORT_RADIUS * c, PORT_RADIUS * s, z, band_center(band))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() < tol
    }

    // ---- time axis ---------------------------------------------------------

    #[test]
    fn w_axis_birth_and_monotonicity() {
        let t0 = 1_767_225_600_i64; // 2026-01-01T00:00:00Z
        assert_eq!(w_of_unix_secs(t0, t0), 0.0);
        assert!(w_of_unix_secs(t0 + 86_400, t0) > 0.0);
        // One doubling of elapsed days = exactly Δw = 1.
        let d1 = w_of_unix_secs(t0 + 86_400, t0);
        let d2 = w_of_unix_secs(t0 + 2 * 86_400, t0);
        assert!(close(d2 - d1, 1.0, 1e-12));
        // Clamped below birth.
        assert_eq!(w_of_unix_secs(t0 - 10_000, t0), 0.0);
    }

    #[test]
    fn w_axis_roundtrip() {
        let t0 = 1_767_225_600_i64;
        for &days in &[0_i64, 1, 7, 30, 200, 3650] {
            let t = t0 + days * 86_400;
            let w = w_of_unix_secs(t, t0);
            assert_eq!(unix_secs_of_w(w, t0), t, "roundtrip failed for {days}d");
        }
    }

    #[test]
    fn valid_at_half_open() {
        assert!(valid_at(1.0, Some(2.0), 1.0));
        assert!(!valid_at(1.0, Some(2.0), 2.0));
        assert!(valid_at(1.0, None, 99.0));
        assert!(!valid_at(1.0, None, 0.5));
    }

    // ---- metric degeneration ----------------------------------------------

    #[test]
    fn kappa_zero_degenerates_to_3d_distance() {
        // The degeneration theorem, checked numerically: at κ = 0 the aniso
        // metric IS the xyz metric of the current 3D space, exactly.
        let a = Vec4::new(1.5, -2.0, 0.25, 7.3);
        let b = Vec4::new(-0.5, 4.0, 3.75, 0.1);
        let dx = a.x - b.x;
        let dy = a.y - b.y;
        let dz = a.z - b.z;
        let expect = dx * dx + dy * dy + dz * dz;
        assert!(close(Vec4::aniso_distance_sq(a, b, 0.0), expect, 1e-12));
        // w must be irrelevant at κ = 0.
        let a2 = Vec4::new(a.x, a.y, a.z, -100.0);
        assert!(close(Vec4::aniso_distance_sq(a2, b, 0.0), expect, 1e-12));
    }

    #[test]
    fn kappa_one_is_full_4d_distance() {
        let a = Vec4::new(0.0, 0.0, 0.0, 0.0);
        let b = Vec4::new(1.0, 1.0, 1.0, 1.0);
        assert!(close(
            Vec4::aniso_distance(a, b, 1.0),
            2.0_f64.sqrt(),
            1e-12
        ));
    }

    // ---- 5-cell geometry ----------------------------------------------------

    #[test]
    fn canonical_offsets_shape() {
        let o = canonical_offsets();
        // Centroid at origin.
        let mut sum = Vec4::zero();
        for v in &o {
            sum = sum.add(*v);
        }
        assert!(close(sum.length(), 0.0, 1e-12));
        // All vertices on the circumsphere.
        let r = circumradius(EDGE_LENGTH);
        for v in &o {
            assert!(close(v.length(), r, 1e-12));
        }
        // All 10 pairwise distances = 1.
        for i in 0..5 {
            for j in i + 1..5 {
                assert!(
                    close(Vec4::distance(o[i], o[j]), EDGE_LENGTH, 1e-12),
                    "edge ({i},{j})"
                );
            }
        }
    }

    #[test]
    fn compute_vertices_translation_only() {
        for core in [
            Vec4::zero(),
            Vec4::new(12.3, -4.5, 6.7, 3.9),
            Vec4::new(-100.0, 100.0, 0.5, 0.0),
        ] {
            let vs = compute_vertices(core);
            assert!(validate_shape(&vs));
            for (v, o) in vs.iter().zip(canonical_offsets()) {
                assert!(Vec4::distance(*v, core.add(o)) < 1e-12);
            }
        }
    }

    #[test]
    fn contains_core_and_rejects_far_point() {
        let core = Vec4::new(3.0, -1.0, 2.0, 1.5);
        let vs = compute_vertices(core);
        assert!(contains(&vs, core));
        assert!(!contains(&vs, core.add(Vec4::new(10.0, 10.0, 10.0, 10.0))));
    }

    #[test]
    fn shared_vertex_counts_obey_t1_prime_support() {
        // Two same-orientation cells share 0, 1 or 5 vertices — never 2-4.
        // ε-window translations merge ALL vertices (5); everything else
        // merges at most one. Sampled scan (full census belongs to space4).
        let base = Vec4::new(0.0, 0.0, 0.0, 0.0);
        let eps = super::super::vertex::VERTEX_MERGE_EPSILON;
        let mut bad = 0;
        for dx in -4..=4 {
            for dy in -4..=4 {
                for dz in -4..=4 {
                    for dw in -4..=4 {
                        let t = Vec4::new(dx as f64, dy as f64, dz as f64, dw as f64);
                        let shared = {
                            let a = compute_vertices(base);
                            let b = compute_vertices(base + t);
                            a.iter()
                                .filter(|va| {
                                    b.iter()
                                        .any(|vb| Vec4::aniso_distance(**va, *vb, 1.0) < eps)
                                })
                                .count()
                        };
                        // Integer-grid translations either coincide exactly
                        // (5 shared) or sit ≥ 1 apart (≤ 1 within ε).
                        if dx == 0 && dy == 0 && dz == 0 && dw == 0 {
                            assert_eq!(shared, 5);
                        } else if !(shared <= 1) {
                            bad += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(bad, 0, "some non-zero translation shared 2-4 vertices");
    }

    // ---- port lattice -------------------------------------------------------

    #[test]
    fn port_lattice_deterministic_and_separated() {
        let mut min_dist = f64::MAX;
        let mut anchors = Vec::new();
        for layer in CylinderLayer::all() {
            for band in -2_i64..=10 {
                anchors.push((*layer, band, port_core(*layer, band)));
            }
        }
        for i in 0..anchors.len() {
            for j in i + 1..anchors.len() {
                let (l1, b1, p1) = anchors[i];
                let (l2, b2, p2) = anchors[j];
                // Determinism: same address → same point (checked by identity
                // of construction above); distinct addresses must be far.
                if l1 == l2 && b1 == b2 {
                    continue;
                }
                let d = Vec4::aniso_distance(p1, p2, 1.0);
                min_dist = min_dist.min(d);
            }
        }
        // Ports must never ε-merge with each other: comfortably above the
        // merge threshold in the fully-coupled metric.
        assert!(min_dist > 1.0, "port anchors too close: {min_dist}");
    }

    #[test]
    fn port_core_resides_in_layer_band() {
        for layer in CylinderLayer::all() {
            let idx = layer.index() as f64;
            let p = port_core(*layer, 3);
            assert!(p.z >= idx * LAYER_HEIGHT && p.z < (idx + 1.0) * LAYER_HEIGHT);
            assert_eq!(era_band(p.w), 3);
        }
    }

    #[test]
    fn era_band_boundaries() {
        assert_eq!(era_band(0.0), 0);
        assert_eq!(era_band(0.999), 0);
        assert_eq!(era_band(1.0), 1);
        assert_eq!(era_band(-0.5), -1);
        assert_eq!(band_center(4), 4.5);
    }
}
