//! Per-cycle structural comparison of the shadow `Space4` (S1 report §6):
//! cell/cluster counts, era purity, shared-vertex histogram with the T1′
//! {0,1} verification, mean w-span and insert timing statistics.
//!
//! The retrieval nDCG comparison against production is deliberately a stub —
//! see [`vs_production`].

use std::collections::HashMap;

use crate::engine::space4::Space4;

// ------------------------------------------------------------ timing stats --

/// Cumulative insert-timing statistics (microseconds) over every row fed so
/// far. Percentiles use the nearest-rank method.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TimingStats {
    pub samples: usize,
    pub total_micros: u64,
    pub mean_micros: f64,
    pub p50_micros: u64,
    pub p95_micros: u64,
    pub max_micros: u64,
    /// Insert throughput measured over the samples (0-duration samples are
    /// counted with a 1µs floor so the metric stays finite).
    pub cells_per_sec: f64,
}

pub fn timing_stats(durations_micros: &[u64]) -> TimingStats {
    if durations_micros.is_empty() {
        return TimingStats::default();
    }
    let mut sorted: Vec<u64> = durations_micros.to_vec();
    sorted.sort_unstable();
    let n = sorted.len();
    let total: u64 = sorted.iter().sum();
    let nearest_rank = |p: u64| -> u64 {
        let idx = (((p as f64 / 100.0) * n as f64).ceil() as usize).clamp(1, n);
        sorted[idx - 1]
    };
    TimingStats {
        samples: n,
        total_micros: total,
        mean_micros: total as f64 / n as f64,
        p50_micros: nearest_rank(50),
        p95_micros: nearest_rank(95),
        max_micros: sorted[n - 1],
        cells_per_sec: n as f64 * 1e6 / total.max(1) as f64,
    }
}

// ---------------------------------------------------------- T1′ validation --

/// Verdict of the engine-level T1′ census: two same-orientation 5-cells may
/// share 0, 1 or 5 (coincident) vertices — **never 2, 3 or 4**. The shadow
/// additionally treats 5-shares as failures (the anti-stacking guard should
/// prevent coincident cores entirely), i.e. the expected histogram is ⊆ {0,1}.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct T1Verdict {
    pub strict: bool,
    /// (k shared, #pairs) for every offending k ≥ 2, sorted by k.
    pub offending: Vec<(usize, usize)>,
}

pub fn validate_t1(hist: &HashMap<usize, usize>) -> T1Verdict {
    let mut offending: Vec<(usize, usize)> = hist
        .iter()
        .filter(|(k, _)| **k >= 2)
        .map(|(k, v)| (*k, *v))
        .collect();
    offending.sort_unstable();
    T1Verdict {
        strict: offending.is_empty(),
        offending,
    }
}

// -------------------------------------------------------- structure report --

/// Structural snapshot of the shadow space for one report cycle.
#[derive(Clone, Debug)]
pub struct StructureMetrics {
    pub cells: usize,
    pub vertices: usize,
    /// Vertices referenced by ≥2 cells (the sharing events = cluster edges).
    pub vertex_shares: usize,
    pub clusters: usize,
    pub largest_cluster: usize,
    /// Clusters whose w-span ≤ 1.0 (one era) — the count, plus purity as a
    /// fraction of all clusters.
    pub era_pure_clusters: usize,
    pub era_purity: f64,
    pub mean_cluster_w_span: f64,
    /// Shared-vertex pair histogram: k → #pairs sharing exactly k vertices,
    /// sorted by k. Sampled (not exhaustive) beyond 2000 cells — see
    /// `pair_hist_sampled`.
    pub shared_vertex_hist: Vec<(usize, usize)>,
    pub pair_hist_sampled: bool,
    pub t1_strict: bool,
    pub t1_offending: Vec<(usize, usize)>,
}

pub fn structure_metrics(space: &Space4) -> StructureMetrics {
    let st = space.stats();
    let verdict = validate_t1(&st.pair_sharing_hist);
    let mut hist: Vec<(usize, usize)> =
        st.pair_sharing_hist.iter().map(|(k, v)| (*k, *v)).collect();
    hist.sort_unstable();
    StructureMetrics {
        cells: st.pentas,
        vertices: st.vertices,
        vertex_shares: st.vertex_shares,
        clusters: st.clusters,
        largest_cluster: st.largest_cluster,
        era_pure_clusters: st.era_pure_clusters,
        era_purity: if st.clusters > 0 {
            st.era_pure_clusters as f64 / st.clusters as f64
        } else {
            0.0
        },
        mean_cluster_w_span: st.mean_cluster_w_span,
        shared_vertex_hist: hist,
        pair_hist_sampled: st.pair_hist_sampled,
        t1_strict: verdict.strict,
        t1_offending: verdict.offending,
    }
}

// ------------------------------------------------------- nDCG stub (S1) -----

/// One benchmark query for the future production comparison. The exact query
/// sourcing (production recall log? synthetic era-graded set?) is part of the
/// benchmark protocol owned by the other work package.
#[allow(dead_code)] // stub types for the pending benchmark protocol (see vs_production)
#[derive(Clone, Debug)]
pub struct BenchmarkQuery {
    pub labels: Vec<String>,
    pub as_of_secs: i64,
}

/// One query's outcome: nDCG of the 4D geometric retrieval vs the production
/// retrieval (labels + payload time filter, system B in the research report).
#[allow(dead_code)] // stub types for the pending benchmark protocol (see vs_production)
#[derive(Clone, Debug)]
pub struct QueryOutcome {
    pub labels: Vec<String>,
    pub as_of_secs: i64,
    pub ndcg_space4: Option<f64>,
    pub ndcg_production: Option<f64>,
}

/// Retrieval-quality comparison shadow Space4 vs production retrieval.
///
/// TODO(#369 benchmark work package): this is an intentional empty stub. The
/// real benchmark protocol — query set construction, graded ground truth,
/// nDCG cutoff, and how production retrieval is invoked offline — is being
/// designed by a separate work package. Do NOT improvise a protocol here: a
/// wrong baseline would poison the S3 verdict. Until the protocol lands,
/// this function returns an empty vector and the report prints "pending".
pub fn vs_production(_space: &Space4, _queries: &[BenchmarkQuery]) -> Vec<QueryOutcome> {
    Vec::new()
}

// ------------------------------------------------------------------ tests --

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::space4::Rng;
    use crate::engine::time_axis::unix_secs_of_w;

    #[test]
    fn timing_stats_basics() {
        assert_eq!(timing_stats(&[]), TimingStats::default());

        let t = timing_stats(&[100]);
        assert_eq!(t.samples, 1);
        assert_eq!(t.p50_micros, 100);
        assert_eq!(t.p95_micros, 100);
        assert_eq!(t.max_micros, 100);

        // 1..=100 → nearest-rank p50 = 50, p95 = 95, mean = 50.5.
        let durs: Vec<u64> = (1..=100u64).collect();
        let t = timing_stats(&durs);
        assert_eq!(t.p50_micros, 50);
        assert_eq!(t.p95_micros, 95);
        assert_eq!(t.max_micros, 100);
        assert!((t.mean_micros - 50.5).abs() < 1e-9);
        assert_eq!(t.total_micros, 5050);
    }

    #[test]
    fn validate_t1_verdicts() {
        let clean: HashMap<usize, usize> = [(0usize, 44_623usize), (1, 227)].into();
        assert!(validate_t1(&clean).strict);

        let mut bad: HashMap<usize, usize> = [(0, 100), (1, 5), (2, 1), (5, 2)].into();
        let v = validate_t1(&bad);
        assert!(!v.strict);
        assert_eq!(v.offending, vec![(2, 1), (5, 2)]);

        bad.clear();
        assert!(validate_t1(&bad).strict);
    }

    #[test]
    fn structure_metrics_small_space() {
        // Build a tiny space: same-label cells in the same era should share
        // vertices and cluster; histogram must stay ⊆ {0,1} (T1′ strict).
        let t0 = 1_700_000_000i64;
        let space = Space4::new(1.0, t0);
        let mut rng = Rng::new(42);
        let day = |w: f64| unix_secs_of_w(w, t0);
        for i in 0..6 {
            let pid = space
                .create(
                    &["rust".to_string(), format!("topic-{i}")],
                    1.0,
                    day(2.0),
                    None,
                    day(2.0),
                    &mut rng,
                )
                .expect("create");
            assert!(pid > 0);
        }
        let m = structure_metrics(&space);
        assert_eq!(m.cells, 6);
        assert!(m.clusters >= 1);
        assert!(
            m.largest_cluster >= 2,
            "same-label same-era cells should share vertices"
        );
        assert!(
            m.t1_strict,
            "histogram must be subset of {{0,1}}: {:?}",
            m.shared_vertex_hist
        );
        assert!(m.era_purity > 0.0);
        // All cells in one era → mean w-span well below 1.
        assert!(m.mean_cluster_w_span < 1.0);
        assert!(
            !m.pair_hist_sampled,
            "census is exhaustive below the 2000-cell cap"
        );
    }
}
