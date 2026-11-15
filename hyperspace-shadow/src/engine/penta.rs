//! The regular 5-cell (pentachoron, 4-simplex, Schläfli {3,3,3}) — the 4D
//! analog of epicode's regular tetrahedron.
//!
//! | | tetrahedron {3,3} | 5-cell {3,3,3} |
//! |---|---|---|
//! | vertices | 4 | 5 |
//! | edges | 6 | 10 |
//! | 2-faces | 4 triangles | 10 triangles |
//! | cells (facets) | — | 5 tetrahedra |
//! | circumradius (edge a) | a√6/4 ≈ 0.6124a | a√(2/5) ≈ 0.6325a |
//! | hypervolume | a³√2/12 (3-volume) | a⁴√5/96 ≈ 0.02329a⁴ |
//!
//! Mirrors epicode's `domain::tetra.rs`: every pentachoron in `Space4` shares
//! ONE canonical orientation and differs from any other only by translation.
//! Cross-pentachoron vertex sharing therefore follows theorem T1′ (see
//! `tests/theorem.rs`): two such 5-cells share exactly 0, 1 or 5 vertices —
//! never 2, 3 or 4 — the direct extension of the 3D theorem T1 (0/1/4) in
//! `research/spatial-memory-synthesis-2026-11-08.md`.

use crate::engine::vec4::Vec4;

/// Uniform edge length of every pentachoron, same constant as `tetra::EDGE_LENGTH`.
pub const EDGE_LENGTH: f64 = 1.0;

/// Vertex ε-merge threshold, same as `vertex::VERTEX_MERGE_EPSILON` in epicode.
pub const VERTEX_MERGE_EPSILON: f64 = 0.05;

/// Number of vertices of a 4-simplex.
pub const PENTA_VERTICES: usize = 5;

/// Circumradius R of a regular 4-simplex with edge `a`: `R = a·√(n/(2(n+1)))`, n = 4.
pub fn circumradius(a: f64) -> f64 {
    a * (2.0_f64 / 5.0_f64).sqrt()
}

/// Center-to-vertex distance of the unit 5-cell (alias of `circumradius(1.0)`).
pub fn center_to_vertex() -> f64 {
    circumradius(EDGE_LENGTH)
}

/// Hypervolume (4-content) of a regular 4-simplex with edge `a`:
/// `V₄ = a⁴·√(n+1)/(n!·√2ⁿ)` with n = 4 → `a⁴·√5/96`.
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
/// 3. Rescale edge √2 → `EDGE_LENGTH`.
///
/// Result: `|oᵢ| = √(2/5)`, `Σoᵢ = 0`, all 10 pairwise distances = 1.
pub fn canonical_offsets() -> [Vec4; 5] {
    // Step 1: regular 4-simplex with edge √2 embedded in the sum-zero
    // hyperplane of ℝ⁵.
    let mut p5 = [[0.0_f64; 5]; 5];
    for i in 0..5 {
        for j in 0..5 {
            p5[i][j] = if i == j { 1.0 - 0.2 } else { -0.2 };
        }
    }

    // Step 2: Gram–Schmidt over p5[0..4] (p5[4] is linearly dependent on them).
    let dot5 = |a: &[f64; 5], b: &[f64; 5]| {
        let mut s = 0.0;
        for k in 0..5 {
            s += a[k] * b[k];
        }
        s
    };
    let mut basis: Vec<[f64; 5]> = Vec::with_capacity(4);
    for i in 0..4 {
        let mut v = p5[i];
        for u in &basis {
            let proj = dot5(&v, u);
            for k in 0..5 {
                v[k] -= proj * u[k];
            }
        }
        let norm = dot5(&v, &v).sqrt();
        for k in 0..5 {
            v[k] /= norm;
        }
        basis.push(v);
    }

    // Step 3: coordinates in ℝ⁴ + rescale edge √2 → 1.
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let mut offsets = [Vec4::zero(); 5];
    for i in 0..5 {
        offsets[i] = Vec4::new(
            dot5(&p5[i], &basis[0]) * s,
            dot5(&p5[i], &basis[1]) * s,
            dot5(&p5[i], &basis[2]) * s,
            dot5(&p5[i], &basis[3]) * s,
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
pub const fn edge_index_pairs() -> [(usize, usize); 10] {
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

/// Shape invariant check: all 10 edges must equal `EDGE_LENGTH` (within 1e-9).
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

/// One memory cell of `Space4` — the 4D counterpart of `Tetrahedron`.
#[derive(Clone, Debug)]
pub struct Pentachoron {
    pub id: u64,
    pub vertex_ids: [u64; PENTA_VERTICES],
    /// Translation-only center; orientation is global (see module docs).
    pub core: Vec4,
    /// Bitemporal window on the w-axis, in w units (log₂-compressed time).
    pub w_from: f64,
    pub w_to: Option<f64>,
    pub labels: Vec<String>,
    pub importance: f64,
    /// Recall count — stretches the retention horizon S (v0.2).
    pub access_count: u64,
    /// Fact-level cease (Zep `expired_at`), compressed: the interval
    /// [w_expired, w_to) is the SHADOW band — believed true, actually false.
    pub w_expired: Option<f64>,
    /// Cylinder layer (see `topology::Layer`), 255 = unassigned.
    pub layer: u8,
}
