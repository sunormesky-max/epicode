//! `epicode-hyperspace` — S1 shadow integration binary (epicode issue #369).
//!
//! Reads a production per-user memory database (`tetramem.db`, STRICTLY
//! read-only), mirrors every tetrahedron row into the vendored true-4D
//! `Space4` engine (`src/engine/`, vendored from research/four-dim v0.2),
//! and periodically emits structural comparison reports (JSON + Markdown).
//!
//! Zero hot-path impact by construction:
//! * separate crate, NOT a workspace member of the main repository;
//! * the only dependency is rusqlite (same version as the backend);
//! * the production database is opened with `SQLITE_OPEN_READ_ONLY` and is
//!   never written, see `ingest::open_read_only`.
//!
//! Usage:
//! ```text
//! epicode-hyperspace --db <path to tetramem.db> [--out <dir>]
//!     [--interval-secs 21600] [--kappa 1.0] [--once] [--keep 60] [--seed N]
//! ```

use hyperspace_shadow::{compare, crypto, engine, ingest, report};

use std::path::PathBuf;
use std::time::{Duration, Instant};

use engine::space4::{Rng, Space4};

/// CLI configuration (hand-rolled parsing — no clap, dependency budget is
/// rusqlite + the crypto set).
struct Config {
    /// Production per-user db file. Convention on a deployed backend:
    /// `<data_dir>/users/<user_id>/tetramem.db`.
    db: PathBuf,
    /// Report output directory (created on demand).
    out: PathBuf,
    /// User id for per-user label decryption (derive key context).
    user: String,
    interval_secs: u64,
    kappa: f64,
    /// Run a single cycle and exit (systemd timer mode). Default: loop.
    once: bool,
    /// Rolling retention for the report directory.
    keep: usize,
    /// Seed for the engine's deterministic placement RNG.
    seed: u64,
}

const USAGE: &str = "epicode-hyperspace — S1 shadow integration for the true-4D Space4 engine

USAGE:
    epicode-hyperspace --db <tetramem.db> --user <user_id> [OPTIONS]

OPTIONS:
    --db <PATH>          production per-user SQLite db (read-only!)
                         deployed convention: <data_dir>/users/<user_id>/tetramem.db
    --user <ID>          user id: derives the per-user AES-GCM key context
                         (with TETRAMEM_MASTER_KEY env) to decrypt labels
    --out <DIR>          report directory [default: hyperspace-reports]
    --interval-secs N    seconds between cycles [default: 21600 (6h)]
    --kappa F            4D time-coupling coefficient, 0.0..=1.0 [default: 1.0]
    --once               run one cycle and exit (systemd timer mode)
    --keep N             report pairs kept in --out [default: 60]
    --seed N             deterministic placement RNG seed [default: 20261115]
    -h, --help           print this help

ENV:
    TETRAMEM_MASTER_KEY  base64 32-byte master key (same env the production
                         cloud binary reads). When set, encrypted labels are
                         decrypted in memory; when unset, only plaintext
                         labels are ingested (encrypted rows are skipped).
";

fn parse_args(args: impl Iterator<Item = String>) -> Result<Config, String> {
    let mut db: Option<PathBuf> = None;
    let mut out = PathBuf::from("hyperspace-reports");
    let mut user = String::new();
    let mut interval_secs: u64 = 21_600;
    let mut kappa: f64 = 1.0;
    let mut once = false;
    let mut keep: usize = report::DEFAULT_KEEP;
    let mut seed: u64 = 20_261_115;

    let mut it = args.peekable();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| -> Result<String, String> {
            it.next()
                .ok_or_else(|| format!("{name} requires a value (see --help)"))
        };
        match arg.as_str() {
            "--db" => db = Some(PathBuf::from(value("--db")?)),
            "--user" => user = value("--user")?,
            "--out" => out = PathBuf::from(value("--out")?),
            "--interval-secs" => {
                interval_secs = value("--interval-secs")?
                    .parse()
                    .map_err(|e| format!("--interval-secs: {e}"))?;
            }
            "--kappa" => {
                kappa = value("--kappa")?
                    .parse()
                    .map_err(|e| format!("--kappa: {e}"))?;
                if !(0.0..=1.0).contains(&kappa) {
                    return Err(format!("--kappa must be within 0.0..=1.0, got {kappa}"));
                }
            }
            "--once" => once = true,
            "--keep" => {
                keep = value("--keep")?
                    .parse()
                    .map_err(|e| format!("--keep: {e}"))?;
                if keep == 0 {
                    return Err("--keep must be >= 1".to_string());
                }
            }
            "--seed" => {
                seed = value("--seed")?
                    .parse()
                    .map_err(|e| format!("--seed: {e}"))?;
            }
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other} (see --help)")),
        }
    }
    let db = db.ok_or_else(|| "--db is required (see --help)".to_string())?;
    if user.trim().is_empty() {
        return Err("--user is required (label decryption context)".to_string());
    }
    Ok(Config {
        db,
        out,
        user,
        interval_secs,
        kappa,
        once,
        keep,
        seed,
    })
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Shadow state that survives across cycles.
struct Shadow {
    space: Option<Space4>,
    /// True once the initial full snapshot (possibly empty) has been loaded.
    initialized: bool,
    last_rowid: i64,
    rows_seen_total: u64,
    rows_skipped_total: u64,
    insert_rejected_total: u64,
    durations: Vec<u64>,
    rng_seed: u64,
}

impl Shadow {
    fn new(rng_seed: u64) -> Self {
        Shadow {
            space: None,
            initialized: false,
            last_rowid: 0,
            rows_seen_total: 0,
            rows_skipped_total: 0,
            insert_rejected_total: 0,
            durations: Vec::new(),
            rng_seed,
        }
    }

    /// One sync cycle: (re)load or poll rows, feed the engine.
    /// Returns the per-cycle delta (rows fed this cycle).
    fn sync(
        &mut self,
        conn: &rusqlite::Connection,
        kappa: f64,
        dec: Option<&crypto::ShadowCrypto>,
        user_id: &str,
    ) -> Result<usize, String> {
        let batch = if self.initialized {
            ingest::poll_since(conn, self.last_rowid, dec, user_id)?
        } else {
            let batch = ingest::load_all(conn, dec, user_id)?;
            self.initialized = true;
            let t0 = ingest::space_birth(&batch.rows, now_unix());
            self.space = Some(Space4::new(kappa, t0));
            batch
        };
        self.last_rowid = self.last_rowid.max(batch.max_rowid_seen);
        self.rows_skipped_total += batch.skipped as u64;
        let rows = batch.rows;
        if !rows.is_empty() {
            // The very first poll after an initially-empty database re-anchors
            // t₀ (the space was born just now, not when the shadow started).
            if self.space.as_ref().is_none_or(|s| s.all_ids().is_empty()) {
                let t0 = ingest::space_birth(&rows, now_unix());
                self.space = Some(Space4::new(kappa, t0));
            }
            self.rows_seen_total += rows.len() as u64;
            let space = self.space.as_ref().expect("space built above");
            let mut rng = Rng::new(self.rng_seed);
            self.rng_seed = self.rng_seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let feed = ingest::feed_rows(space, &rows, &mut rng, &mut self.durations);
            self.insert_rejected_total += feed.rejected as u64;
        }
        Ok(rows.len())
    }
}

fn run(cfg: &Config) -> Result<(), String> {
    let conn = ingest::open_read_only(&cfg.db)?;
    let dec = crypto::ShadowCrypto::from_env().map_err(|e| format!("crypto env: {e}"))?;
    if dec.is_none() {
        eprintln!(
            "note: TETRAMEM_MASTER_KEY not set — encrypted labels will be skipped \
             (plaintext rows still ingest)"
        );
    }
    std::fs::create_dir_all(&cfg.out).map_err(|e| format!("create {}: {e}", cfg.out.display()))?;
    let mut shadow = Shadow::new(cfg.seed);
    let mut cycle: u64 = 0;

    loop {
        cycle += 1;
        let started = Instant::now();
        let rows_this_cycle = shadow.sync(&conn, cfg.kappa, dec.as_ref(), &cfg.user)?;
        let space = shadow
            .space
            .as_ref()
            .ok_or("shadow space not initialized")?;

        let metrics = compare::structure_metrics(space);
        let timing = compare::timing_stats(&shadow.durations);
        let outcomes = compare::vs_production(space, &[]);
        let rep = report::CycleReport {
            generated_unix: now_unix(),
            cycle,
            db_path: cfg.db.display().to_string(),
            kappa: cfg.kappa,
            t0_unix: space.t0_secs,
            last_rowid: shadow.last_rowid,
            rows_seen_total: shadow.rows_seen_total,
            rows_this_cycle: rows_this_cycle as u64,
            rows_skipped_total: shadow.rows_skipped_total,
            insert_rejected_total: shadow.insert_rejected_total,
            cycle_ingest_secs: started.elapsed().as_secs_f64(),
            metrics,
            timing,
            retrieval_vs_production: outcomes,
        };

        let (json_path, _) = report::write_report(&cfg.out, &rep)?;
        let pruned = report::prune(&cfg.out, cfg.keep)?;
        println!(
            "[cycle {cycle}] cells={} clusters={} largest={} era-purity={:.1}% mean-w-span={:.3} \
             T1'={} | rows: +{} (total {}, skipped {}, rejected {}) | insert {:.0} cells/s \
             | report {}{}",
            rep.metrics.cells,
            rep.metrics.clusters,
            rep.metrics.largest_cluster,
            rep.metrics.era_purity * 100.0,
            rep.metrics.mean_cluster_w_span,
            if rep.metrics.t1_strict {
                "PASS"
            } else {
                "FAIL"
            },
            rep.rows_this_cycle,
            rep.rows_seen_total,
            rep.rows_skipped_total,
            rep.insert_rejected_total,
            rep.timing.cells_per_sec,
            json_path.display(),
            if pruned > 0 {
                format!(" (pruned {pruned} old files)")
            } else {
                String::new()
            },
        );

        if cfg.once {
            break;
        }
        std::thread::sleep(Duration::from_secs(cfg.interval_secs));
    }
    Ok(())
}

fn main() {
    match parse_args(std::env::args().skip(1)) {
        Ok(cfg) => {
            if let Err(e) = run(&cfg) {
                eprintln!("epicode-hyperspace: error: {e}");
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("epicode-hyperspace: {e}");
            eprintln!("run with --help for usage");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> std::vec::IntoIter<String> {
        v.iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn cli_defaults_and_overrides() {
        let c = parse_args(args(&["--db", "/tmp/x/tetramem.db", "--user", "sunorme"])).unwrap();
        assert_eq!(c.db, PathBuf::from("/tmp/x/tetramem.db"));
        assert_eq!(c.user, "sunorme");
        assert_eq!(c.out, PathBuf::from("hyperspace-reports"));
        assert_eq!(c.interval_secs, 21_600);
        assert_eq!(c.kappa, 1.0);
        assert!(!c.once);
        assert_eq!(c.keep, 60);
        assert_eq!(c.seed, 20_261_115);

        let c = parse_args(args(&[
            "--db",
            "d.db",
            "--user",
            "u1",
            "--out",
            "/var/reports",
            "--interval-secs",
            "60",
            "--kappa",
            "0.3",
            "--once",
            "--keep",
            "5",
            "--seed",
            "7",
        ]))
        .unwrap();
        assert_eq!(c.out, PathBuf::from("/var/reports"));
        assert_eq!(c.interval_secs, 60);
        assert_eq!(c.kappa, 0.3);
        assert!(c.once);
        assert_eq!(c.keep, 5);
        assert_eq!(c.seed, 7);
    }

    #[test]
    fn cli_rejects_bad_input() {
        assert!(parse_args(args(&[])).is_err(), "--db required");
        assert!(parse_args(args(&["--db"])).is_err(), "value required");
        assert!(parse_args(args(&["--db", "x"])).is_err(), "--user required");
        assert!(parse_args(args(&["--db", "x", "--kappa", "1.5"])).is_err());
        assert!(parse_args(args(&["--db", "x", "--kappa", "-0.1"])).is_err());
        assert!(parse_args(args(&["--db", "x", "--interval-secs", "zero"])).is_err());
        assert!(parse_args(args(&["--db", "x", "--keep", "0"])).is_err());
        assert!(parse_args(args(&["--wat"])).is_err());
    }

    /// Full `run()` smoke test: --once against a production-shaped fixture
    /// must ingest, write both report files and exit cleanly.
    #[test]
    fn run_once_smoke_end_to_end() {
        let dir = std::env::temp_dir().join(format!(
            "hyperspace-run-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("tetramem.db");
        {
            let conn = rusqlite::Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE tetrahedrons (
                    id INTEGER PRIMARY KEY, labels TEXT NOT NULL DEFAULT '[]',
                    timestamp INTEGER NOT NULL DEFAULT 0,
                    importance REAL NOT NULL DEFAULT 1.0,
                    valid_from INTEGER NOT NULL DEFAULT 0,
                    valid_to INTEGER, expired_at INTEGER);",
            )
            .unwrap();
            let t0 = 1_700_000_000i64;
            for i in 0..12 {
                conn.execute(
                    "INSERT INTO tetrahedrons (labels, timestamp) VALUES (?1, ?2)",
                    rusqlite::params![format!(r#"["rust","topic-{i}"]"#), t0 + i * 86_400],
                )
                .unwrap();
            }
        }
        let out = dir.join("reports");
        let cfg = Config {
            db: db.clone(),
            out: out.clone(),
            user: "test-user".to_string(),
            interval_secs: 21_600,
            kappa: 1.0,
            once: true,
            keep: 60,
            seed: 1,
        };
        run(&cfg).expect("one-shot run");

        let files: Vec<String> = std::fs::read_dir(&out)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().into_string().unwrap())
            .collect();
        assert_eq!(files.len(), 2, "one json + one md: {files:?}");
        let json =
            std::fs::read_to_string(out.join(files.iter().find(|f| f.ends_with(".json")).unwrap()))
                .unwrap();
        assert!(json.contains("\"cells\": 12"), "{json}");
        assert!(json.contains("\"t1_strict\": true"), "{json}");
        assert!(json.contains("\"rows_skipped_total\": 0"), "{json}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
