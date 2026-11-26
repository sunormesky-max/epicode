//! `hyperspace-bench` — benchmark-protocol §6.2 geometry bridge (S1).
//!
//! Reads a mirror corpus (`space.jsonl`, exported by
//! `epicode-hyperspace --export-space`) and a query file (`queries.jsonl`),
//! rebuilds the κ=1 Space4, runs the pure-geometry 4D retrieval
//! (as_of slice + era-neighborhood seeds + spatial PPR) and writes
//! `results.jsonl` with production memory ids — the exact subprocess
//! contract of `research/four-dim/benchmark-protocol.md` §6.2.
//!
//! Side-effect note (contract's read-only guarantee): the vendored
//! `retrieval::search` touches hits (registers recall for the lifecycle).
//! In this one-shot process the space is discarded on exit, so touches are
//! process-local and cannot contaminate repeat experiments — the intent of
//! the guarantee (no cross-run state) holds.
//!
//! Usage:
//! ```text
//! hyperspace-bench search --space <space.jsonl> --queries <queries.jsonl> \
//!     --out <results.jsonl> [--k 10]
//! ```

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::Instant;

use hyperspace_shadow::engine::retrieval::{self, Query, Validity};
use hyperspace_shadow::engine::space4::{CellPayload, Rng, Space4};
use hyperspace_shadow::engine::time_axis::w_of_unix_secs;
use hyperspace_shadow::ingest::{self, TetraRow};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => {}
        Err(Some(code)) => std::process::exit(code),
        Err(None) => {
            eprintln!("usage: hyperspace-bench search --space <s.jsonl> --queries <q.jsonl> --out <r.jsonl> [--k 10]");
            std::process::exit(2);
        }
    }
}

/// Err(Some(code)) = contract exit code; Err(None) = bad CLI.
fn run(args: &[String]) -> Result<(), Option<i32>> {
    if args.first().map(String::as_str) != Some("search") {
        return Err(None);
    }
    let mut space_path: Option<PathBuf> = None;
    let mut queries_path: Option<PathBuf> = None;
    let mut out_path: Option<PathBuf> = None;
    let mut default_k: usize = 10;
    let mut it = args[1..].iter().peekable();
    while let Some(a) = it.next() {
        let val = |it: &mut std::iter::Peekable<std::slice::Iter<'_, String>>,
                   name: &str|
         -> Result<String, Option<i32>> {
            it.next().cloned().ok_or_else(|| {
                eprintln!("{name} requires a value");
                Some(2)
            })
        };
        match a.as_str() {
            "--space" => space_path = Some(PathBuf::from(val(&mut it, "--space")?)),
            "--queries" => queries_path = Some(PathBuf::from(val(&mut it, "--queries")?)),
            "--out" => out_path = Some(PathBuf::from(val(&mut it, "--out")?)),
            "--k" => {
                default_k = val(&mut it, "--k")?.parse().map_err(|_| Some(2))?;
            }
            _ => return Err(None),
        }
    }
    let space_path = space_path.ok_or(None)?;
    let queries_path = queries_path.ok_or(None)?;
    let out_path = out_path.ok_or(None)?;

    let rows = load_space(&space_path).map_err(|e| {
        eprintln!("space contract violation: {e}");
        Some(2)
    })?;
    if rows.is_empty() {
        eprintln!("space file has no rows");
        return Err(Some(2));
    }

    // Rebuild the κ=1 space exactly like the shadow (write-history order).
    let now = unix_now();
    let t0 = ingest::space_birth(&rows, now);
    let space = Space4::new(1.0, t0);
    let mut rng = Rng::new(0xBE4C);
    let mut pid_map: HashMap<u64, (u64, i64)> = HashMap::with_capacity(rows.len());
    for r in &rows {
        let payload = CellPayload {
            labels: r.labels.clone(),
            importance: r.importance,
            valid_from: ingest::normalized_valid_from(r),
            valid_until: r.valid_to,
            expired_at: r.expired_at,
            layer: None,
        };
        if let Some(pid) = space.create_full(payload, &mut rng) {
            pid_map.insert(pid, (r.rowid as u64, r.timestamp));
        }
    }

    let queries = load_queries(&queries_path, default_k).map_err(|e| {
        eprintln!("queries contract violation: {e}");
        Some(2)
    })?;

    let out_file = std::fs::File::create(&out_path).map_err(|e| {
        eprintln!("cannot open {}: {e}", out_path.display());
        Some(3)
    })?;
    let mut w = std::io::BufWriter::new(out_file);
    for q in &queries {
        let started = Instant::now();
        let line = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let hits = retrieval::search(
                &space,
                &Query {
                    labels: q.labels.clone(),
                    as_of_secs: q.as_of,
                    k: q.k,
                },
            );
            render_result(
                &q.qid,
                &hits,
                &pid_map,
                t0,
                started.elapsed().as_secs_f64() * 1000.0,
            )
        })) {
            Ok(line) => line,
            Err(_) => format!(
                "{{\"qid\": {}, \"error\": \"engine panic\"}}",
                json_str(&q.qid)
            ),
        };
        writeln!(w, "{line}").ok();
    }
    w.flush().ok();
    Ok(())
}

struct BenchQuery {
    qid: String,
    labels: Vec<String>,
    as_of: i64,
    k: usize,
}

// ------------------------------------------------------------ tiny JSON read --

/// Extract `"key": value` for f64/i64/bool/null keys from one flat JSON
/// object line (the contract's formats are flat; labels is the only array).
fn json_number(line: &str, key: &str) -> Option<f64> {
    let pat = format!("\"{key}\"");
    let i = line.find(&pat)? + pat.len();
    let rest = line[i..].trim_start().strip_prefix(':')?.trim_start();
    let end = rest
        .find(|c: char| {
            !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E')
        })
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn json_opt_int(line: &str, key: &str) -> Option<i64> {
    let n = json_number(line, key)?;
    if n <= 0.0 {
        return None; // null absent or 0 (= legacy "since forever")
    }
    Some(n as i64)
}

/// Span of the first `"[...]"` array after `"key":` — reused with the
/// ingest label parser (single flat array of strings per the contract).
fn json_array_span<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!("\"{key}\"");
    let i = line.find(&pat)? + pat.len();
    let rest = line[i..].trim_start().strip_prefix(':')?.trim_start();
    if !rest.starts_with('[') {
        return None;
    }
    let mut depth = 0usize;
    let mut in_str = false;
    let mut esc = false;
    for (j, c) in rest.char_indices() {
        match c {
            '"' if !esc => in_str = !in_str,
            '\\' if in_str => {
                esc = !esc;
                continue;
            }
            '[' if !in_str => depth += 1,
            ']' if !in_str => {
                depth -= 1;
                if depth == 0 {
                    return Some(&rest[..=j]);
                }
            }
            _ => {}
        }
        esc = false;
    }
    None
}

fn json_string(line: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\"");
    let i = line.find(&pat)? + pat.len();
    let rest = line[i..].trim_start().strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let mut out = String::new();
    let mut esc = false;
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        if esc {
            match c {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Ok(cp) = u32::from_str_radix(&hex, 16) {
                        if let Some(ch) = char::from_u32(cp) {
                            out.push(ch);
                        }
                    }
                }
                other => out.push(other),
            }
            esc = false;
        } else if c == '\\' {
            esc = true;
        } else if c == '"' {
            return Some(out);
        } else {
            out.push(c);
        }
    }
    None
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ------------------------------------------------------------- file loading --

fn load_space(path: &std::path::Path) -> Result<Vec<TetraRow>, String> {
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut rows = Vec::new();
    for line in std::io::BufReader::new(f).lines() {
        let line = line.map_err(|e| e.to_string())?;
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if !t.contains("\"create\"") {
            continue; // contract: only create ops define the corpus
        }
        let id = json_number(t, "id").ok_or("row missing id")? as i64;
        let labels = json_array_span(t, "labels")
            .and_then(ingest::parse_labels_json)
            .unwrap_or_default();
        let timestamp = json_number(t, "timestamp").ok_or("row missing timestamp")? as i64;
        let importance = json_number(t, "importance").unwrap_or(1.0);
        let valid_from = json_opt_int(t, "valid_from").unwrap_or(0);
        let valid_to = json_opt_int(t, "valid_until");
        let expired_at = json_opt_int(t, "expired_at");
        rows.push(TetraRow {
            rowid: id,
            labels,
            timestamp,
            importance,
            valid_from,
            valid_to,
            expired_at,
        });
    }
    // Same write-history order as the shadow's load_all.
    rows.sort_by(|a, b| a.timestamp.cmp(&b.timestamp).then(a.rowid.cmp(&b.rowid)));
    Ok(rows)
}

fn load_queries(path: &std::path::Path, default_k: usize) -> Result<Vec<BenchQuery>, String> {
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let now = unix_now();
    let mut out = Vec::new();
    for line in std::io::BufReader::new(f).lines() {
        let line = line.map_err(|e| e.to_string())?;
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let qid = json_string(t, "qid").ok_or("query missing qid")?;
        let labels = json_array_span(t, "labels")
            .and_then(ingest::parse_labels_json)
            .unwrap_or_default();
        let as_of = json_opt_int(t, "as_of").unwrap_or(now);
        let k = json_opt_int(t, "k")
            .map(|v| v as usize)
            .unwrap_or(default_k)
            .max(1);
        out.push(BenchQuery {
            qid,
            labels,
            as_of,
            k,
        });
    }
    if out.is_empty() {
        return Err("no queries".into());
    }
    Ok(out)
}

fn render_result(
    qid: &str,
    hits: &[retrieval::Hit],
    pid_map: &HashMap<u64, (u64, i64)>,
    t0: i64,
    took_ms: f64,
) -> String {
    let mut out = format!(
        "{{\"qid\": {}, \"took_ms\": {:.3}, \"hits\": [",
        json_str(qid),
        took_ms
    );
    let mut first = true;
    for h in hits {
        let Some((prod_id, ts)) = pid_map.get(&h.id) else {
            continue;
        };
        if !first {
            out.push(',');
        }
        first = false;
        let validity = match h.validity {
            Validity::True => "True",
            Validity::BelievedStale => "BelievedStale",
        };
        out.push_str(&format!(
            "{{\"id\": {prod_id}, \"score\": {:.4}, \"validity\": \"{validity}\", \"w\": {:.3}, \"timestamp\": {ts}}}",
            h.score,
            w_of_unix_secs(*ts, t0),
        ));
    }
    out.push_str("]}");
    out
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
