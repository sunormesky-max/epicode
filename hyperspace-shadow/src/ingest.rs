//! Read-only ingestion of a production per-user `tetramem.db` into a shadow
//! `Space4` (S1 shadow mode, zero hot-path impact).
//!
//! Hard safety rule: this module opens SQLite with `SQLITE_OPEN_READ_ONLY`
//! ONLY. It never issues a write statement, never creates the file, and never
//! takes a write lock. The production database is treated as an untrusted,
//! immutable input.
//!
//! Field mapping (production `tetrahedrons` → `CellPayload`):
//!
//! | production column | Space4 payload | notes |
//! |---|---|---|
//! | `labels` (JSON text array) | `labels` | plain JSON only; encrypted rows are skipped + counted |
//! | `timestamp` (unix secs) | `now` arg / era anchor fallback | drives the w anchor when `valid_from` is 0 |
//! | `valid_from` (unix secs, 0 = legacy) | `valid_from` | 0 → falls back to `timestamp` |
//! | `valid_to` | `valid_until` | knowledge-level invalidation |
//! | `expired_at` | `expired_at` | fact-level cease → shadow band |
//! | `importance` | `importance` | |
//!
//! The w coordinate itself is computed by the vendored engine
//! (`engine::time_axis::w_of_unix_secs`, log₂-compressed days since space
//! birth t₀); `space_birth()` derives t₀ from the initial snapshot as the
//! earliest of timestamp/valid_from across rows.

use std::path::Path;
use std::time::Instant;

use rusqlite::{Connection, OpenFlags};

use crate::engine::space4::{CellPayload, Rng, Space4};
use crate::engine::time_axis::w_of_unix_secs;

/// Busy timeout so a concurrently-writing backend (WAL) doesn't make the
/// shadow fail spuriously. Read-only connections still briefly contend on the
/// WAL index; 5s mirrors the production `busy_timeout`.
const BUSY_TIMEOUT_MS: u64 = 5000;

/// One decoded production row. `id` is `INTEGER PRIMARY KEY` (i.e. the rowid)
/// in the production schema, so incremental polling by rowid and by id are
/// the same thing.
#[derive(Clone, Debug)]
pub struct TetraRow {
    /// Kept for diagnostics/tests — the sync cursor itself uses
    /// `IngestBatch::max_rowid_seen` so skipped rows are not re-polled.
    #[allow(dead_code)]
    pub rowid: i64,
    pub labels: Vec<String>,
    pub timestamp: i64,
    pub importance: f64,
    pub valid_from: i64,
    pub valid_to: Option<i64>,
    pub expired_at: Option<i64>,
}

/// Open the production database STRICTLY read-only.
///
/// The file must already exist (a read-only open never creates it). If the db
/// runs in WAL mode the `-wal`/`-shm` sidecar files must be readable by this
/// process as well — see README (deploy notes).
pub fn open_read_only(path: &Path) -> Result<Connection, String> {
    if !path.is_file() {
        return Err(format!("database not found: {}", path.display()));
    }
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("open read-only {}: {e}", path.display()))?;
    conn.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MS))
        .map_err(|e| format!("set busy_timeout: {e}"))?;
    Ok(conn)
}

/// Legacy rows written before the bitemporal migration have `valid_from = 0`
/// (= "true since forever"). For the w anchor such a row uses its creation
/// `timestamp` instead — the same normalization production search applies.
pub fn normalized_valid_from(row: &TetraRow) -> i64 {
    if row.valid_from > 0 {
        row.valid_from
    } else {
        row.timestamp
    }
}

/// The w coordinate this row will be placed at (era anchor of the cell).
/// Exposed for tests and future diagnostics (the engine applies the same
/// mapping internally during `create_full`).
#[allow(dead_code)]
pub fn w_anchor(row: &TetraRow, t0_secs: i64) -> f64 {
    w_of_unix_secs(normalized_valid_from(row), t0_secs)
}

/// Space birth t₀ = the earliest moment any row references (min over
/// timestamp and normalized valid_from), NOT the unix epoch — era boundaries
/// must anchor to the account's own history (see `engine::time_axis` docs).
/// Empty database → `fallback_now`.
pub fn space_birth(rows: &[TetraRow], fallback_now: i64) -> i64 {
    rows.iter()
        .map(|r| r.timestamp.min(normalized_valid_from(r)))
        .min()
        .unwrap_or(fallback_now)
}

/// Outcome of feeding a batch of rows into the shadow space.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FeedOutcome {
    pub inserted: usize,
    /// Rejected by the engine's anti-stacking guard even after its internal
    /// retries (near-duplicate memory in the same era). Counted, not fatal.
    pub rejected: usize,
}

/// Feed decoded rows into the shadow space via the full 4D create path
/// (era-affinity placement + ε-merge). Per-row insert duration in µs is
/// appended to `durations` for the timing statistics in the report.
pub fn feed_rows(
    space: &Space4,
    rows: &[TetraRow],
    rng: &mut Rng,
    durations: &mut Vec<u64>,
) -> FeedOutcome {
    let mut out = FeedOutcome::default();
    for r in rows {
        let payload = CellPayload {
            labels: r.labels.clone(),
            importance: r.importance,
            valid_from: normalized_valid_from(r),
            valid_until: r.valid_to,
            expired_at: r.expired_at,
            layer: None,
        };
        let started = Instant::now();
        let pid = space.create_full(payload, r.timestamp, rng);
        durations.push(started.elapsed().as_micros() as u64);
        if pid.is_some() {
            out.inserted += 1;
        } else {
            out.rejected += 1;
        }
    }
    out
}

// ---------------------------------------------------------------- reading --

/// Columns the shadow reads. Only what the 4D mapping needs — content,
/// embeddings and PII columns are deliberately NOT read.
const TETRA_SELECT: &str = "SELECT rowid, labels, timestamp, importance, \
     valid_from, valid_to, expired_at FROM tetrahedrons";

struct RawRow {
    rowid: i64,
    labels_json: String,
    timestamp: i64,
    importance: f64,
    valid_from: i64,
    valid_to: Option<i64>,
    expired_at: Option<i64>,
}

/// One read batch: decoded rows plus bookkeeping.
///
/// `max_rowid_seen` covers skipped rows too — a malformed row must not be
/// re-examined (and re-counted as skipped) on every future poll.
pub struct IngestBatch {
    pub rows: Vec<TetraRow>,
    pub skipped: usize,
    pub max_rowid_seen: i64,
}

/// Initial snapshot: every row, in write-history order (timestamp, then id)
/// so the shadow replays the production write order.
pub fn load_all(conn: &Connection) -> Result<IngestBatch, String> {
    let raw = query_raw(
        conn,
        &format!("{TETRA_SELECT} ORDER BY timestamp, rowid"),
        &[],
    )?;
    Ok(finish_batch(raw))
}

/// Incremental poll: rows with rowid greater than `last_rowid`, in rowid
/// order. `INSERT OR REPLACE` rewrites of an existing id keep their rowid and
/// are therefore NOT re-synced (documented limitation — see README).
pub fn poll_since(conn: &Connection, last_rowid: i64) -> Result<IngestBatch, String> {
    let raw = query_raw(
        conn,
        &format!("{TETRA_SELECT} WHERE rowid > ?1 ORDER BY rowid"),
        &[&last_rowid],
    )?;
    Ok(finish_batch(raw))
}

fn finish_batch(raw: Vec<RawRow>) -> IngestBatch {
    // MAX over the batch, not `raw.last()`: `load_all` orders by timestamp
    // (write-history replay), so the last row is not necessarily the
    // largest rowid.
    let max_rowid_seen = raw.iter().map(|r| r.rowid).max().unwrap_or(0);
    let (rows, skipped) = decode_rows(raw);
    IngestBatch {
        rows,
        skipped,
        max_rowid_seen,
    }
}

fn query_raw(
    conn: &Connection,
    sql: &str,
    params: &[&dyn rusqlite::ToSql],
) -> Result<Vec<RawRow>, String> {
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| format!("prepare `{sql}`: {e}"))?;
    let rows = stmt
        .query_map(params, |row| {
            Ok(RawRow {
                rowid: row.get(0)?,
                labels_json: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                timestamp: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                importance: row.get::<_, Option<f64>>(3)?.unwrap_or(1.0),
                valid_from: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                valid_to: row.get::<_, Option<i64>>(5)?,
                expired_at: row.get::<_, Option<i64>>(6)?,
            })
        })
        .map_err(|e| format!("query tetrahedrons: {e}"))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| format!("read row: {e}"))?);
    }
    Ok(out)
}

/// Decode label JSON; malformed rows (e.g. encrypted labels on a
/// key-protected deployment, or corrupted text) are skipped and counted
/// rather than aborting the shadow.
fn decode_rows(raw: Vec<RawRow>) -> (Vec<TetraRow>, usize) {
    let mut rows = Vec::with_capacity(raw.len());
    let mut skipped = 0usize;
    for r in raw {
        match parse_labels_json(&r.labels_json) {
            Some(labels) => rows.push(TetraRow {
                rowid: r.rowid,
                labels,
                timestamp: r.timestamp,
                importance: r.importance,
                valid_from: r.valid_from,
                valid_to: r.valid_to,
                expired_at: r.expired_at,
            }),
            None => skipped += 1,
        }
    }
    (rows, skipped)
}

// ------------------------------------------------------- minimal JSON read --

/// Parse a JSON array of strings (the production `labels` column format).
///
/// Hand-rolled on purpose: the crate's dependency budget is rusqlite only
/// (S1 rule), and labels are exactly `[ "str", … ]` — no nested structures.
/// Returns `None` for anything else (including non-string elements).
/// `\uXXXX` escapes above the BMP rely on no surrogate pairing — production
/// labels are UTF-8 text written by serde_json; lone surrogates don't occur.
pub fn parse_labels_json(s: &str) -> Option<Vec<String>> {
    let b = s.as_bytes();
    let n = b.len();
    let mut i = 0usize;

    fn skip_ws(b: &[u8], i: &mut usize) {
        while *i < b.len() && matches!(b[*i], b' ' | b'\t' | b'\n' | b'\r') {
            *i += 1;
        }
    }

    skip_ws(b, &mut i);
    if i >= n || b[i] != b'[' {
        return None;
    }
    i += 1;
    let mut out = Vec::new();
    loop {
        skip_ws(b, &mut i);
        if i >= n {
            return None;
        }
        match b[i] {
            b']' => {
                i += 1;
                break;
            }
            b',' => {
                i += 1;
                continue;
            }
            b'"' => {
                i += 1;
                let mut item = String::new();
                loop {
                    if i >= n {
                        return None;
                    }
                    match b[i] {
                        b'"' => {
                            i += 1;
                            break;
                        }
                        b'\\' => {
                            i += 1;
                            if i >= n {
                                return None;
                            }
                            match b[i] {
                                b'"' => item.push('"'),
                                b'\\' => item.push('\\'),
                                b'/' => item.push('/'),
                                b'b' => item.push('\u{8}'),
                                b'f' => item.push('\u{c}'),
                                b'n' => item.push('\n'),
                                b'r' => item.push('\r'),
                                b't' => item.push('\t'),
                                b'u' => {
                                    if i + 4 >= n {
                                        return None;
                                    }
                                    let hex = std::str::from_utf8(&b[i + 1..i + 5]).ok()?;
                                    let cp = u32::from_str_radix(hex, 16).ok()?;
                                    i += 4;
                                    item.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                                }
                                _ => return None,
                            }
                            i += 1;
                        }
                        _ => {
                            // Copy one full UTF-8 character (never splits a
                            // multibyte sequence: '"', '\\' and control bytes
                            // below 0x80 can't be continuation bytes).
                            let ch = s[i..].chars().next()?;
                            item.push(ch);
                            i += ch.len_utf8();
                        }
                    }
                }
                out.push(item);
            }
            _ => return None,
        }
    }
    skip_ws(b, &mut i);
    if i == n {
        Some(out)
    } else {
        None
    }
}

// ------------------------------------------------------------------ tests --

#[cfg(test)]
mod tests {
    use super::*;

    // ---- labels JSON --------------------------------------------------

    #[test]
    fn parse_labels_basic() {
        assert_eq!(
            parse_labels_json(r#"["rust","memory"]"#),
            Some(vec!["rust".to_string(), "memory".to_string()])
        );
        assert_eq!(parse_labels_json("[]"), Some(vec![]));
        assert_eq!(parse_labels_json("  [ ]  "), Some(vec![]));
        assert_eq!(
            parse_labels_json(r#"["知识图谱","认知"]"#),
            Some(vec!["知识图谱".to_string(), "认知".to_string()])
        );
    }

    #[test]
    fn parse_labels_escapes() {
        assert_eq!(
            parse_labels_json(r#"["a\"b","c\\d","e\nf","caf\u00e9"]"#),
            Some(vec![
                "a\"b".to_string(),
                "c\\d".to_string(),
                "e\nf".to_string(),
                "café".to_string()
            ])
        );
    }

    #[test]
    fn parse_labels_rejects_garbage() {
        assert_eq!(parse_labels_json(""), None);
        assert_eq!(parse_labels_json("not json"), None);
        assert_eq!(parse_labels_json(r#"[123,"a"]"#), None);
        assert_eq!(parse_labels_json(r#"["a"] trailing"#), None);
        assert_eq!(parse_labels_json(r#"["unclosed]"#), None);
    }

    // ---- timestamp → w mapping (engine time_axis, via row mapping) ----

    const DAY: i64 = 86400;

    fn row(valid_from: i64, timestamp: i64) -> TetraRow {
        TetraRow {
            rowid: 1,
            labels: vec!["x".into()],
            timestamp,
            importance: 1.0,
            valid_from,
            valid_to: None,
            expired_at: None,
        }
    }

    #[test]
    fn w_mapping_known_values() {
        let t0 = 1_700_000_000i64;
        // w(t0) = 0 — the birth moment.
        assert_eq!(w_anchor(&row(t0, t0), t0), 0.0);
        // +1 day → log2(1+1) = 1 doubling.
        assert_eq!(w_anchor(&row(t0 + DAY, t0 + DAY), t0), 1.0);
        // +3 days → log2(4) = 2 doublings.
        assert_eq!(w_anchor(&row(t0 + 3 * DAY, t0 + 3 * DAY), t0), 2.0);
        // Monotone in time.
        assert!(
            w_anchor(&row(t0 + DAY, t0 + DAY), t0) < w_anchor(&row(t0 + 2 * DAY, t0 + 2 * DAY), t0)
        );
    }

    #[test]
    fn valid_from_zero_falls_back_to_timestamp() {
        let t0 = 1_700_000_000i64;
        // Legacy row: valid_from=0, created 1 day after birth → w = 1.
        let legacy = row(0, t0 + DAY);
        assert_eq!(normalized_valid_from(&legacy), t0 + DAY);
        assert_eq!(w_anchor(&legacy, t0), 1.0);
        // Pre-birth timestamps clamp to w=0 (engine behaviour).
        let early = row(0, t0 - 100 * DAY);
        assert_eq!(w_anchor(&early, t0), 0.0);
    }

    #[test]
    fn space_birth_is_earliest_referenced_moment() {
        let t0 = 1_700_000_000i64;
        let rows = vec![
            row(t0 + 2 * DAY, t0 + 3 * DAY),
            row(t0 + DAY, t0 + DAY),
            row(0, t0 + 5 * DAY),
        ];
        assert_eq!(space_birth(&rows, t0 + 99 * DAY), t0 + DAY);
        assert_eq!(space_birth(&[], 42), 42);
    }

    // ---- SQLite fixture end-to-end ------------------------------------

    /// Unique temp dir (no tempfile dependency — budget is rusqlite only).
    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hyperspace-shadow-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Production-shaped fixture: the exact column set `TETRA_SELECT` reads.
    fn fixture_db(dir: &std::path::Path) -> std::path::PathBuf {
        let path = dir.join("tetramem.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE tetrahedrons (
                id          INTEGER PRIMARY KEY,
                core_x      REAL NOT NULL DEFAULT 0,
                core_y      REAL NOT NULL DEFAULT 0,
                core_z      REAL NOT NULL DEFAULT 0,
                content     TEXT NOT NULL DEFAULT '',
                content_hash INTEGER NOT NULL DEFAULT 0,
                labels      TEXT NOT NULL DEFAULT '[]',
                mass        REAL NOT NULL DEFAULT 1.0,
                timestamp   INTEGER NOT NULL DEFAULT 0,
                valid_from  INTEGER NOT NULL DEFAULT 0,
                valid_to    INTEGER,
                expired_at  INTEGER,
                importance  REAL NOT NULL DEFAULT 1.0
            );",
        )
        .unwrap();
        path
    }

    fn insert_row(path: &std::path::Path, labels: &str, timestamp: i64, valid_from: i64) -> i64 {
        let conn = Connection::open(path).unwrap();
        conn.execute(
            "INSERT INTO tetrahedrons (labels, timestamp, valid_from) VALUES (?1, ?2, ?3)",
            rusqlite::params![labels, timestamp, valid_from],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn readonly_open_never_creates_and_rejects_missing() {
        let dir = temp_dir("ro-missing");
        let missing = dir.join("nope.db");
        assert!(open_read_only(&missing).is_err());
        assert!(!missing.exists(), "read-only open must not create files");
    }

    #[test]
    fn load_all_feed_and_incremental_poll() {
        let dir = temp_dir("e2e");
        let db = fixture_db(&dir);
        let t0 = 1_700_000_000i64;

        insert_row(&db, r#"["rust","space"]"#, t0, 0);
        insert_row(&db, r#"["rust","space"]"#, t0 + 86_400, 0);
        insert_row(&db, r#"["认知"]"#, t0 + 2 * 86_400, 0);
        insert_row(&db, "<<ciphertext-not-json>>", t0, 0); // skipped row

        let conn = open_read_only(&db).unwrap();
        let batch = load_all(&conn).unwrap();
        let rows = &batch.rows;
        assert_eq!(rows.len(), 3);
        assert_eq!(batch.skipped, 1);
        // max_rowid_seen covers the skipped ciphertext row (rowid 4).
        assert_eq!(batch.max_rowid_seen, 4);
        assert_eq!(space_birth(rows, t0), t0);

        // The read-only connection must reject any write attempt.
        assert!(conn.execute("DELETE FROM tetrahedrons", []).is_err());

        let space = Space4::new(1.0, space_birth(rows, t0));
        let mut rng = Rng::new(7);
        let mut durations = Vec::new();
        let feed = feed_rows(&space, rows, &mut rng, &mut durations);
        assert_eq!(feed.inserted, 3);
        assert_eq!(feed.rejected, 0);
        assert_eq!(durations.len(), 3);
        assert_eq!(space.stats().pentas, 3);

        // Incremental: nothing new — the malformed row must NOT reappear
        // (rowid cursor advanced past it during the initial load).
        let empty = poll_since(&conn, batch.max_rowid_seen).unwrap();
        assert!(empty.rows.is_empty());
        assert_eq!(empty.skipped, 0);
        assert_eq!(empty.max_rowid_seen, 0);

        // Incremental: one new row appears.
        let new_id = insert_row(&db, r#"["rust","new-era"]"#, t0 + 40 * 86_400, 0);
        let fresh = poll_since(&conn, batch.max_rowid_seen).unwrap();
        assert_eq!(fresh.skipped, 0);
        assert_eq!(fresh.rows.len(), 1);
        assert_eq!(fresh.rows[0].rowid, new_id);
        assert_eq!(fresh.max_rowid_seen, new_id);
        // New era: 40 days after birth ≈ log2(41) ≈ 5.36 w — far from w=0.
        assert!(w_anchor(&fresh.rows[0], space_birth(rows, t0)) > 5.0);
    }
}
