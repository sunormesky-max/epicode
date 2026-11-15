//! Report output: JSON + Markdown per cycle, timestamped filenames, rolling
//! retention of the newest [`DEFAULT_KEEP`] report pairs.
//!
//! JSON is hand-rolled (no serde dependency — the crate budget is rusqlite
//! only, see Cargo.toml). All serialized strings pass through
//! [`json_escape`]; numbers are plain Rust formatting.

use std::fs;
use std::path::{Path, PathBuf};

use crate::compare::{QueryOutcome, StructureMetrics, TimingStats};

/// Rolling retention: newest report pairs kept in the output directory.
pub const DEFAULT_KEEP: usize = 60;

/// File name prefix of every report file (both .json and .md).
const PREFIX: &str = "hyperspace-";

/// Everything one cycle emits.
#[derive(Clone, Debug)]
pub struct CycleReport {
    pub generated_unix: i64,
    pub cycle: u64,
    pub db_path: String,
    pub kappa: f64,
    /// Space birth t₀ (unix secs) the shadow anchored the w axis at.
    pub t0_unix: i64,
    pub last_rowid: i64,
    pub rows_seen_total: u64,
    pub rows_this_cycle: u64,
    pub rows_skipped_total: u64,
    pub insert_rejected_total: u64,
    /// Wall-clock seconds spent feeding this cycle's rows into the engine.
    pub cycle_ingest_secs: f64,
    pub metrics: StructureMetrics,
    /// Cumulative insert timing over every row fed so far.
    pub timing: TimingStats,
    /// Result of `compare::vs_production` — empty until the benchmark work
    /// package lands (stub by design).
    pub retrieval_vs_production: Vec<QueryOutcome>,
}

/// Write one report as `<dir>/hyperspace-<UTC ts>-c<cycle>.{json,md}`.
/// Returns the two paths written.
pub fn write_report(dir: &Path, rep: &CycleReport) -> Result<(PathBuf, PathBuf), String> {
    fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let stem = format!(
        "{PREFIX}{}-c{}",
        format_utc_compact(rep.generated_unix),
        rep.cycle
    );
    let json_path = dir.join(format!("{stem}.json"));
    let md_path = dir.join(format!("{stem}.md"));
    fs::write(&json_path, to_json(rep))
        .map_err(|e| format!("write {}: {e}", json_path.display()))?;
    fs::write(&md_path, to_markdown(rep))
        .map_err(|e| format!("write {}: {e}", md_path.display()))?;
    Ok((json_path, md_path))
}

/// Delete the oldest report pairs beyond `keep`. Report files sort
/// chronologically by file name (fixed-width UTC timestamp prefix), so no
/// filesystem mtime games are needed. Returns the number of files removed.
pub fn prune(dir: &Path, keep: usize) -> Result<usize, String> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(0), // nothing to prune (e.g. dir gone)
    };
    let mut stems: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let (stem, ext) = name.rsplit_once('.')?;
            if (ext == "json" || ext == "md") && stem.starts_with(PREFIX) {
                Some(stem.to_string())
            } else {
                None
            }
        })
        .collect();
    stems.sort();
    stems.dedup();

    let mut removed = 0usize;
    let excess = stems.len().saturating_sub(keep);
    for stem in &stems[..excess] {
        for ext in ["json", "md"] {
            let p = dir.join(format!("{stem}.{ext}"));
            if p.is_file() {
                fs::remove_file(&p).map_err(|e| format!("prune {}: {e}", p.display()))?;
                removed += 1;
            }
        }
    }
    Ok(removed)
}

// ------------------------------------------------------------- time format --

/// `1731628800` → `"2024-11-15T00:00:00Z"` (UTC, hand-rolled — no chrono).
pub fn format_utc(unix_secs: i64) -> String {
    let (y, mo, d) = civil_from_days(unix_secs.div_euclid(86_400));
    let secs_of_day = unix_secs.rem_euclid(86_400);
    format!(
        "{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60
    )
}

/// Compact file-name-safe variant: `"20241115T000000Z"`.
pub fn format_utc_compact(unix_secs: i64) -> String {
    format_utc(unix_secs)
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == 'Z')
        .collect()
}

/// Howard Hinnant's `civil_from_days` — proleptic Gregorian, valid for any
/// i64 day count.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

// ------------------------------------------------------------- JSON output --

/// labels 数组 → JSON 数组字符串（镜像导出与报告共用）。
pub fn to_json_labels(labels: &[String]) -> String {
    let items: Vec<String> = labels
        .iter()
        .map(|l| format!("\"{}\"", json_escape(l)))
        .collect();
    format!("[{}]", items.join(","))
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn hist_json(hist: &[(usize, usize)]) -> String {
    let items: Vec<String> = hist
        .iter()
        .map(|(k, v)| format!("{{\"k\": {k}, \"pairs\": {v}}}"))
        .collect();
    format!("[{}]", items.join(", "))
}

fn outcomes_json(outcomes: &[QueryOutcome]) -> String {
    let items: Vec<String> = outcomes
        .iter()
        .map(|o| {
            let labels: Vec<String> = o
                .labels
                .iter()
                .map(|l| format!("\"{}\"", json_escape(l)))
                .collect();
            let opt = |v: &Option<f64>| match v {
                Some(x) => format!("{x}"),
                None => "null".to_string(),
            };
            format!(
                "{{\"labels\": [{}], \"as_of_secs\": {}, \"ndcg_space4\": {}, \"ndcg_production\": {}}}",
                labels.join(", "),
                o.as_of_secs,
                opt(&o.ndcg_space4),
                opt(&o.ndcg_production)
            )
        })
        .collect();
    format!("[{}]", items.join(", "))
}

pub fn to_json(rep: &CycleReport) -> String {
    let m = &rep.metrics;
    let offending: Vec<String> = m
        .t1_offending
        .iter()
        .map(|(k, v)| format!("{{\"k\": {k}, \"pairs\": {v}}}"))
        .collect();
    let epurity = format!("{:.4}", m.era_purity);
    let wspan = format!("{:.4}", m.mean_cluster_w_span);
    let tmean = format!("{:.1}", rep.timing.mean_micros);
    let tcps = format!("{:.1}", rep.timing.cells_per_sec);
    format!(
        concat!(
            "{{\n",
            "  \"generated_unix\": {ts},\n",
            "  \"generated_utc\": \"{ts_utc}\",\n",
            "  \"cycle\": {cycle},\n",
            "  \"db_path\": \"{db}\",\n",
            "  \"kappa\": {kappa},\n",
            "  \"t0_unix\": {t0},\n",
            "  \"last_rowid\": {last_rowid},\n",
            "  \"rows_seen_total\": {seen},\n",
            "  \"rows_this_cycle\": {this_cycle},\n",
            "  \"rows_skipped_total\": {skipped},\n",
            "  \"insert_rejected_total\": {rejected},\n",
            "  \"cycle_ingest_secs\": {ingest_secs},\n",
            "  \"structure\": {{\n",
            "    \"cells\": {cells},\n",
            "    \"vertices\": {vertices},\n",
            "    \"vertex_shares\": {vshares},\n",
            "    \"clusters\": {clusters},\n",
            "    \"largest_cluster\": {largest},\n",
            "    \"era_pure_clusters\": {epure},\n",
            "    \"era_purity\": {epurity},\n",
            "    \"mean_cluster_w_span\": {wspan},\n",
            "    \"shared_vertex_hist\": {hist},\n",
            "    \"pair_hist_sampled\": {sampled},\n",
            "    \"t1_strict\": {t1},\n",
            "    \"t1_offending\": [{offending}]\n",
            "  }},\n",
            "  \"insert_timing\": {{\n",
            "    \"samples\": {tn},\n",
            "    \"total_micros\": {ttotal},\n",
            "    \"mean_micros\": {tmean},\n",
            "    \"p50_micros\": {t50},\n",
            "    \"p95_micros\": {t95},\n",
            "    \"max_micros\": {tmax},\n",
            "    \"cells_per_sec\": {tcps}\n",
            "  }},\n",
            "  \"retrieval_vs_production\": {outcomes},\n",
            "  \"retrieval_note\": \"pending - benchmark protocol owned by the #369 benchmark work package\"\n",
            "}}\n"
        ),
        ts = rep.generated_unix,
        ts_utc = format_utc(rep.generated_unix),
        cycle = rep.cycle,
        db = json_escape(&rep.db_path),
        kappa = rep.kappa,
        t0 = rep.t0_unix,
        last_rowid = rep.last_rowid,
        seen = rep.rows_seen_total,
        this_cycle = rep.rows_this_cycle,
        skipped = rep.rows_skipped_total,
        rejected = rep.insert_rejected_total,
        ingest_secs = rep.cycle_ingest_secs,
        cells = m.cells,
        vertices = m.vertices,
        vshares = m.vertex_shares,
        clusters = m.clusters,
        largest = m.largest_cluster,
        epure = m.era_pure_clusters,
        epurity = epurity,
        wspan = wspan,
        hist = hist_json(&m.shared_vertex_hist),
        sampled = m.pair_hist_sampled,
        t1 = m.t1_strict,
        offending = offending.join(", "),
        tn = rep.timing.samples,
        ttotal = rep.timing.total_micros,
        tmean = tmean,
        t50 = rep.timing.p50_micros,
        t95 = rep.timing.p95_micros,
        tmax = rep.timing.max_micros,
        tcps = tcps,
        outcomes = outcomes_json(&rep.retrieval_vs_production),
    )
}

// --------------------------------------------------------- Markdown output --

pub fn to_markdown(rep: &CycleReport) -> String {
    let m = &rep.metrics;
    let mut s = String::with_capacity(2048);
    s.push_str(&format!(
        "# epicode-hyperspace shadow report — cycle {}\n\n\
         - generated: {} (unix {})\n\
         - db (read-only): `{}`\n\
         - kappa: {} | space birth t0: {} (unix {})\n\
         - rows: seen {} total / {} this cycle / {} skipped (bad labels) / {} insert-rejected | last rowid {}\n\
         - cycle ingest wall time: {:.3}s\n\n",
        rep.cycle,
        format_utc(rep.generated_unix),
        rep.generated_unix,
        rep.db_path,
        rep.kappa,
        format_utc(rep.t0_unix),
        rep.t0_unix,
        rep.rows_seen_total,
        rep.rows_this_cycle,
        rep.rows_skipped_total,
        rep.insert_rejected_total,
        rep.last_rowid,
        rep.cycle_ingest_secs,
    ));

    s.push_str("## Structure (shadow Space4)\n\n| metric | value |\n|---|---|\n");
    s.push_str(&format!("| cells | {} |\n", m.cells));
    s.push_str(&format!("| vertices | {} |\n", m.vertices));
    s.push_str(&format!(
        "| shared vertices (≥2 cells) | {} |\n",
        m.vertex_shares
    ));
    s.push_str(&format!("| clusters | {} |\n", m.clusters));
    s.push_str(&format!("| largest cluster | {} |\n", m.largest_cluster));
    s.push_str(&format!(
        "| era-pure clusters (w-span ≤ 1) | {} ({:.1}%) |\n",
        m.era_pure_clusters,
        m.era_purity * 100.0
    ));
    s.push_str(&format!(
        "| mean cluster w-span | {:.4} |\n\n",
        m.mean_cluster_w_span
    ));

    s.push_str("## Shared-vertex pair histogram (T1′: must be ⊆ {0,1})\n\n");
    s.push_str("| k shared vertices | pairs |\n|---|---|\n");
    if m.shared_vertex_hist.is_empty() {
        s.push_str("| (no pairs) | 0 |\n");
    }
    for (k, v) in &m.shared_vertex_hist {
        s.push_str(&format!("| {k} | {v} |\n"));
    }
    let verdict = if m.t1_strict { "PASS" } else { "FAIL" };
    s.push_str(&format!(
        "\nT1′ strict: **{verdict}** (no pair shares ≥2 vertices){}\n\n",
        if m.pair_hist_sampled {
            " — histogram SAMPLED (space > 2000 cells)"
        } else {
            ""
        }
    ));
    if !m.t1_offending.is_empty() {
        s.push_str("Offending buckets: ");
        for (k, v) in &m.t1_offending {
            s.push_str(&format!("k={k}×{v} "));
        }
        s.push_str("\n\n");
    }

    s.push_str("## Insert timing (cumulative)\n\n");
    s.push_str("| stat | value |\n|---|---|\n");
    s.push_str(&format!("| samples | {} |\n", rep.timing.samples));
    s.push_str(&format!(
        "| total | {:.3}s |\n",
        rep.timing.total_micros as f64 / 1e6
    ));
    s.push_str(&format!("| mean | {:.1}µs |\n", rep.timing.mean_micros));
    s.push_str(&format!("| p50 | {}µs |\n", rep.timing.p50_micros));
    s.push_str(&format!("| p95 | {}µs |\n", rep.timing.p95_micros));
    s.push_str(&format!("| max | {}µs |\n", rep.timing.max_micros));
    s.push_str(&format!(
        "| throughput | {:.0} cells/s |\n\n",
        rep.timing.cells_per_sec
    ));

    s.push_str("## Retrieval nDCG vs production\n\n");
    if rep.retrieval_vs_production.is_empty() {
        s.push_str(
            "Pending — the benchmark protocol is owned by the #369 benchmark \
             work package (see `compare::vs_production`).\n",
        );
    } else {
        s.push_str("| query | as_of | nDCG Space4 | nDCG production |\n|---|---|---|---|\n");
        for o in &rep.retrieval_vs_production {
            let f = |v: &Option<f64>| v.map(|x| format!("{x:.4}")).unwrap_or("—".into());
            s.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                o.labels.join(", "),
                o.as_of_secs,
                f(&o.ndcg_space4),
                f(&o.ndcg_production)
            ));
        }
    }
    s
}

// ------------------------------------------------------------------ tests --

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compare::StructureMetrics;

    fn dummy_report(cycle: u64) -> CycleReport {
        CycleReport {
            generated_unix: 1_794_700_800 + cycle as i64 * 3600,
            cycle,
            db_path: "C:\\data\\users\\alice\\tetramem.db".into(),
            kappa: 1.0,
            t0_unix: 1_700_000_000,
            last_rowid: 1234,
            rows_seen_total: 1234,
            rows_this_cycle: 12,
            rows_skipped_total: 3,
            insert_rejected_total: 0,
            cycle_ingest_secs: 0.42,
            metrics: StructureMetrics {
                cells: 1231,
                vertices: 4100,
                vertex_shares: 90,
                clusters: 220,
                largest_cluster: 64,
                era_pure_clusters: 215,
                era_purity: 0.977,
                mean_cluster_w_span: 0.13,
                shared_vertex_hist: vec![(0, 44_623), (1, 227)],
                pair_hist_sampled: false,
                t1_strict: true,
                t1_offending: vec![],
            },
            timing: Default::default(),
            retrieval_vs_production: vec![],
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hyperspace-report-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn format_utc_known_values() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc(1), "1970-01-01T00:00:01Z");
        assert_eq!(format_utc(86_399), "1970-01-01T23:59:59Z");
        assert_eq!(format_utc(86_400), "1970-01-02T00:00:00Z");
        // 2024-11-15T00:00:00Z and 2026-11-15T00:00:00Z (leap-year checked).
        assert_eq!(format_utc(1_731_628_800), "2024-11-15T00:00:00Z");
        assert_eq!(format_utc(1_794_700_800), "2026-11-15T00:00:00Z");
        // Leap day: 2024-02-29T12:00:00Z.
        assert_eq!(format_utc(1_709_208_000), "2024-02-29T12:00:00Z");
        assert_eq!(format_utc_compact(1_794_700_800), "20261115T000000Z");
    }

    #[test]
    fn json_escapes_strings() {
        assert_eq!(json_escape("a\"b\\c\nd"), "a\\\"b\\\\c\\nd");
        assert_eq!(json_escape("\u{1}"), "\\u0001");
        assert_eq!(json_escape("知识"), "知识"); // non-ASCII passes through
    }

    #[test]
    fn json_report_shape() {
        let j = to_json(&dummy_report(7));
        assert!(j.starts_with('{') && j.trim_end().ends_with('}'));
        assert!(j.contains("\"cells\": 1231"));
        assert!(j.contains("\"t1_strict\": true"));
        assert!(j.contains("\"k\": 0, \"pairs\": 44623"));
        // Windows path backslashes must be escaped, not raw.
        assert!(j.contains("C:\\\\data\\\\users\\\\alice\\\\tetramem.db"));
        assert!(!j.contains("C:\\data\\users\\")); // no raw single-backslash path
    }

    #[test]
    fn markdown_report_shape() {
        let md = to_markdown(&dummy_report(7));
        assert!(md.contains("# epicode-hyperspace shadow report — cycle 7"));
        assert!(md.contains("T1′ strict: **PASS**"));
        assert!(md.contains("Pending"));
    }

    #[test]
    fn write_report_creates_both_files() {
        let dir = temp_dir("write");
        let (j, m) = write_report(&dir, &dummy_report(3)).unwrap();
        assert!(j.is_file() && j.extension().unwrap() == "json");
        assert!(m.is_file() && m.extension().unwrap() == "md");
        let name = j.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.starts_with("hyperspace-2026111"), "name={name}");
        assert!(name.ends_with("-c3.json"), "name={name}");
    }

    #[test]
    fn prune_keeps_newest_n_pairs() {
        let dir = temp_dir("prune");
        // 70 older reports via the real writer (names sort chronologically).
        for i in 0..70u64 {
            let mut r = dummy_report(i + 1);
            r.generated_unix = 1_700_000_000 + i as i64 * 3600; // Nov 2023, hourly
            write_report(&dir, &r).unwrap();
        }
        // One newest report (2026) on top.
        write_report(&dir, &dummy_report(99)).unwrap();

        let removed = prune(&dir, DEFAULT_KEEP).unwrap();
        assert_eq!(removed, 2 * 11, "71 pairs total, keep 60 → 11 pairs gone");

        let mut remaining: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().into_string().unwrap())
            .filter(|n| n.starts_with(PREFIX))
            .collect();
        remaining.sort();
        assert_eq!(remaining.len(), 2 * DEFAULT_KEEP);
        // Oldest 11 stems dropped (cycles 1..=11), newest kept (cycle 99).
        assert!(!remaining.iter().any(|n| n.contains("-c1.")));
        assert!(!remaining.iter().any(|n| n.contains("-c11.")));
        assert!(remaining.iter().any(|n| n.contains("-c12.")));
        assert!(remaining.iter().any(|n| n.contains("-c99.")));
        assert!(
            remaining.iter().any(|n| n.contains("202611")),
            "newest 2026 report survives"
        );
        // Pruning again is a no-op.
        assert_eq!(prune(&dir, DEFAULT_KEEP).unwrap(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_tolerates_empty_and_foreign_files() {
        let dir = temp_dir("prune-empty");
        assert_eq!(prune(&dir, 5).unwrap(), 0);
        fs::write(dir.join("unrelated.txt"), "x").unwrap();
        fs::write(dir.join("hyperspace-notes.md"), "x").unwrap(); // no .json pair, has prefix though
        assert_eq!(prune(&dir, 5).unwrap(), 0, "below keep threshold");
        let _ = fs::remove_dir_all(&dir);
    }
}
