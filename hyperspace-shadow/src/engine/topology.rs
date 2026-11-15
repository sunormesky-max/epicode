//! The hypercylinder: epicode's 6-layer central cylinder, extended by the
//! era axis into a **layer × era-band lattice** (`topology.rs` ↔
//! `domain/cylinder.rs`).
//!
//! 3D: layers are z-bands; one Port per layer, star-connected to clusters.
//! 4D: every layer spans the whole w axis; each (layer, era-band) crossing
//! gets its own deterministic Port4. The skeleton of the true 4D space is a
//! 6 × W lattice — **topics live in layers, eras live in bands, and the
//! space's home address is (layer, band)**.
//!
//! Era bands quantize w at 1.0 — one band = one doubling of age (the same
//! unit the era-quantized sharing theorem T1′ uses), so vertex-sharing
//! never crosses a band boundary except through the ±0.5/±1.0 docking
//! offsets — the lattice and the merge structure share one clock.

use crate::engine::vec4::Vec4;

/// The six cylinder layers (same order and semantics as epicode's
/// `CylinderLayer`): Instinct at the bottom, Identity at the top.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Layer {
    Instinct,
    Relation,
    Cognitive,
    Service,
    Cycle,
    Identity,
}

pub const LAYERS: [Layer; 6] = [
    Layer::Instinct,
    Layer::Relation,
    Layer::Cognitive,
    Layer::Service,
    Layer::Cycle,
    Layer::Identity,
];

/// Height of one layer's z-band (INITIAL_HEIGHT/6 in epicode terms).
pub const LAYER_HEIGHT: f64 = 2.0;

/// Cylinder radius for Port placement (epicode INITIAL_RADIUS = 2.0).
pub const PORT_RADIUS: f64 = 2.0;

impl Layer {
    pub fn index(self) -> u8 {
        match self {
            Layer::Instinct => 0,
            Layer::Relation => 1,
            Layer::Cognitive => 2,
            Layer::Service => 3,
            Layer::Cycle => 4,
            Layer::Identity => 5,
        }
    }

    pub fn from_index(i: u8) -> Layer {
        LAYERS[(i as usize) % 6]
    }

    /// z-band of this layer: [z_min, z_max).
    pub fn zone(self) -> (f64, f64) {
        let lo = self.index() as f64 * LAYER_HEIGHT;
        (lo, lo + LAYER_HEIGHT)
    }

    /// Keyword routing, mirroring `cylinder.rs::from_labels` (English +
    /// Chinese keywords; fallback Instinct, like epicode).
    pub fn from_labels(labels: &[String]) -> Layer {
        let hay: Vec<String> = labels.iter().map(|l| l.to_lowercase()).collect();
        let has = |kw: &[&str]| hay.iter().any(|l| kw.iter().any(|k| l.contains(k)));
        if has(&["knowledge", "graph", "图谱", "知识", "relation", "关系"]) {
            Layer::Relation
        } else if has(&["think", "cognit", "reason", "认知", "推理", "self", "自我"]) {
            Layer::Cognitive
        } else if has(&[
            "architecture",
            "tool",
            "service",
            "deploy",
            "code",
            "架构",
            "工具",
            "部署",
            "代码",
        ]) {
            Layer::Service
        } else if has(&["cycle", "dream", "sleep", "梦境", "循环", "巩固"]) {
            Layer::Cycle
        } else if has(&["identity", "name", "who", "身份", "名字"]) {
            Layer::Identity
        } else {
            Layer::Instinct
        }
    }
}

/// Era band index: one band = one w-unit = one doubling of age.
pub fn era_band(w: f64) -> i64 {
    w.floor() as i64
}

/// Center of a band (used for Port w and era seeding).
pub fn band_center(band: i64) -> f64 {
    band as f64 + 0.5
}

/// A (layer, band) lattice anchor — the 4D evolution of epicode's `Port`.
/// Positions are DETERMINISTIC: angle by layer, radius PORT_RADIUS, z at the
/// layer zone center, w at the band center. No two (layer, band) pairs share
/// a core; every port is > 2R from its lattice neighbours, so ports never
/// ε-merge with each other.
pub fn port_core(layer: Layer, band: i64) -> Vec4 {
    let idx = layer.index() as f64;
    let angle = idx * (std::f64::consts::PI / 3.0) + (band as f64) * 0.61;
    let (s, c) = angle.sin_cos();
    let (_, z_max) = layer.zone();
    Vec4::new(
        PORT_RADIUS * c,
        PORT_RADIUS * s,
        z_max - LAYER_HEIGHT * 0.5,
        band_center(band),
    )
}

/// Layer × band occupancy of the whole space (text matrix for reports).
pub fn occupancy_matrix(cells: &[crate::engine::penta::Pentachoron], bands: i64) -> String {
    let mut grid = vec![vec![0usize; bands.max(1) as usize]; 6];
    for c in cells {
        let li = (c.layer % 6) as usize;
        let b = era_band(c.core.w).clamp(0, bands - 1) as usize;
        grid[li][b] += 1;
    }
    let mut s = String::from("layer\\band ");
    for b in 0..bands {
        s.push_str(&format!("{b:>5}"));
    }
    s.push('\n');
    for (li, row) in grid.iter().enumerate() {
        s.push_str(&format!("{:<10}", LAYERS[li].debug_name()));
        for v in row {
            s.push_str(&format!("{v:>5}"));
        }
        s.push('\n');
    }
    s
}

/// v0.3 era 带人口普查：非空带 (band, 细胞数)，按 band 升序。
/// 空带不出现在序列里 —— 序列中的"相邻"即人口序列相邻（可跨空带）。
pub fn band_populations(cells: &[crate::engine::penta::Pentachoron]) -> Vec<(i64, usize)> {
    let mut pops: std::collections::BTreeMap<i64, usize> = std::collections::BTreeMap::new();
    for c in cells {
        *pops.entry(era_band(c.core.w)).or_insert(0) += 1;
    }
    pops.into_iter().collect()
}

/// v0.3 相邻带合并计划（`Space4::compact_bands` 的纯函数核心）：
/// 人口序列中相邻两带的人口和 < `threshold` 时合并为一带，返回
/// (kept, absorbed) 对 —— 密者保留（平局取低带：历史向过去压缩），
/// 平移量 = kept − absorbed 个 1.0 w-量子（整数，跨空带时 >1）。
/// 每带每趟至多参与一次合并，防止链式塌缩把整条 w 轴并成单带。
pub fn band_merge_plan(pops: &[(i64, usize)], threshold: usize) -> Vec<(i64, i64)> {
    let mut plan = Vec::new();
    let mut consumed = std::collections::HashSet::new();
    let mut i = 0usize;
    while i + 1 < pops.len() {
        let (b0, p0) = pops[i];
        if consumed.contains(&b0) {
            i += 1;
            continue;
        }
        let (b1, p1) = pops[i + 1];
        if consumed.contains(&b1) {
            i += 1;
            continue;
        }
        if p0 + p1 < threshold {
            let (kept, absorbed) = if p1 > p0 { (b1, b0) } else { (b0, b1) };
            plan.push((kept, absorbed));
            consumed.insert(b0);
            consumed.insert(b1);
            i += 2;
        } else {
            i += 1;
        }
    }
    plan
}

impl Layer {
    fn debug_name(self) -> &'static str {
        match self {
            Layer::Instinct => "Instinct",
            Layer::Relation => "Relation",
            Layer::Cognitive => "Cognitive",
            Layer::Service => "Service",
            Layer::Cycle => "Cycle",
            Layer::Identity => "Identity",
        }
    }
}
