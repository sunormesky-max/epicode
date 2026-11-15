# epicode-hyperspace — S1 shadow integration binary

Issue #369 · S1 of the true-4D engine integration plan
(`research/four-dim/report-true-4d-engine-v02-2026-11-15.md` §6).

`epicode-hyperspace` reads a **production** per-user memory database, mirrors
every `tetrahedrons` row into the four-dimensional **Space4** engine, and
periodically emits structural comparison reports (JSON + Markdown). It is a
pure observer: **zero hot-path impact by construction**.

```
production tetramem.db ──(SQLITE_OPEN_READ_ONLY)──► ingest ──► Space4 (κ)
                                                      │
                              compare (clusters / era purity / T1′ hist / timing)
                                                      │
                                        report (JSON + MD, rolling 60)
```

## Why this crate looks like this (S1 rules)

* **Sibling of `backend/` and `guard/`, not a workspace member.** The main
  repository does not know this crate exists — that is the point of S1
  ("新增 crate，零热路径改动"). Do NOT add it to any root workspace, root
  `Cargo.lock`, or main CI pipeline.
* **Single dependency: `rusqlite` 0.41 (bundled)** — the exact version and
  feature set the backend uses. Reports serialize JSON by hand; SQLite is the
  only third-party code in the binary.
* **The production database is opened `SQLITE_OPEN_READ_ONLY` and never
  written.** See `src/ingest.rs::open_read_only`. A unit test asserts that a
  write statement on the shadow connection fails.
* **The 4D engine is vendored, not linked.** `src/engine/` is a verbatim copy
  of `research/four-dim/hyperspace` v0.2 (only `vec4` / `penta` / `topology`
  / `space4` / `retrieval` / `lifecycle` / `time_axis`; `demo`, `sim`, `rot4`,
  `project` are not needed). Each file carries a
  `VENDORED from research/four-dim v0.2` header. Upstream is the source of
  truth — never fix engine bugs here, re-vendor instead.

## Field mapping (production row → 4D cell)

| `tetrahedrons` column | Space4 payload | notes |
|---|---|---|
| `labels` (JSON text array) | `labels` | plain JSON only; rows whose labels are not parseable JSON (e.g. encrypted on key-protected deployments) are **skipped and counted**, never fatal |
| `timestamp` (unix secs) | era anchor fallback + `now` | drives `w` when `valid_from` is 0 |
| `valid_from` (unix secs) | `valid_from` | `0` (legacy rows) → falls back to `timestamp` |
| `valid_to` | `valid_until` | knowledge-level invalidation |
| `expired_at` | `expired_at` | fact-level cease → shadow band |
| `importance` | `importance` | |

The `w` coordinate is `log₂(1 + days since t₀)` (Weber–Fechner compressed;
see `src/engine/time_axis.rs`). `t₀` (space birth) is anchored at the
earliest moment any row references — NOT the unix epoch — so era boundaries
belong to the account's own history.

Incremental sync polls `WHERE rowid > last_seen` (`id` is `INTEGER PRIMARY
KEY`, i.e. the rowid, so this is a cheap index walk).

## Build

Rust ≥ 1.88. `rusqlite` bundled compiles SQLite from C, so a C compiler is
required (gcc/clang/msvc).

```sh
cd hyperspace-shadow
cargo build --release            # binary: target/release/epicode-hyperspace
cargo test                       # unit tests (uses temp SQLite fixtures, never a real db)
cargo fmt
```

Windows (gnu toolchain, as used during development):

```cmd
cd D:\epicode\epicode\hyperspace-shadow
set CARGO_HOME=D:\cargohome&& set RUSTUP_HOME=D:\rustup&& set PATH=D:\msys64\mingw64\bin;D:\rustup\toolchains\stable-x86_64-pc-windows-gnu\bin;%PATH%&& cargo test --target x86_64-pc-windows-gnu
```

## Run

```text
epicode-hyperspace --db <path to tetramem.db> [OPTIONS]

  --db PATH          production per-user db, deployed convention:
                     <backend data_dir>/users/<user_id>/tetramem.db
  --out DIR          report directory               [default: hyperspace-reports]
  --interval-secs N  seconds between cycles         [default: 21600 (6 h)]
  --kappa F          4D time coupling, 0.0..=1.0    [default: 1.0]
  --once             single cycle then exit         (systemd timer mode)
  --keep N           report pairs retained          [default: 60]
  --seed N           deterministic placement RNG    [default: 20261115]
```

Examples:

```sh
# one-shot comparison against a user's live db
./epicode-hyperspace --db /var/lib/epicode/users/alice/tetramem.db \
    --out /var/lib/epicode-shadow/reports --once

# daemon mode: sync + report every 6 hours
./epicode-hyperspace --db /var/lib/epicode/users/alice/tetramem.db \
    --out /var/lib/epicode-shadow/reports
```

### Read-only caveats (deploy notes)

* The backend runs the db in WAL mode. A read-only connection needs read
  access to `tetramem.db-wal` / `tetramem.db-shm` as well — run the shadow as
  a user with those permissions (the `tetramem` service account below has
  them). If the backend is stopped and no `-wal` file exists, a plain
  read-only open works fine.
* `INSERT OR REPLACE` rewrites of an existing id keep their rowid, so
  updated rows are **not** re-synced; production archive/delete flows are
  likewise not mirrored (the shadow observes the create stream). Restart the
  shadow (fresh full load) after bulk maintenance jobs if that matters.
* If the shadow starts against an empty db, `t₀` re-anchors when the first
  rows appear (handled internally).

## systemd timer example

Run one cycle every 6 hours under the `tetramem` user. The service unit is
hardened: the whole filesystem is read-only (`ProtectSystem=strict`,
`ReadOnlyPaths=` — the legacy alias `ReadOnlyDirectories=` still works on
older systemd) except the report directory, so even a bug in this binary
cannot write to the production data directory.

`/etc/systemd/system/epicode-hyperspace.service`:

```ini
[Unit]
Description=Epicode hyperspace S1 shadow (Space4 structural comparison)
After=network.target

[Service]
Type=oneshot
User=tetramem
Group=tetramem
# The shadow must only ever READ the production database.
# (ReadOnlyDirectories= is the deprecated alias of ReadOnlyPaths=.)
ReadOnlyPaths=/var/lib/epicode
# The only writable location: reports.
ReadWritePaths=/var/lib/epicode-shadow
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
NoNewPrivileges=true
ExecStart=/usr/local/bin/epicode-hyperspace \
    --db /var/lib/epicode/users/alice/tetramem.db \
    --out /var/lib/epicode-shadow/reports \
    --kappa 1.0 \
    --once
```

`/etc/systemd/system/epicode-hyperspace.timer`:

```ini
[Unit]
Description=Run the epicode hyperspace shadow every 6 hours

[Timer]
OnCalendar=*-*-* 00/6:00:00
Persistent=true
Unit=epicode-hyperspace.service

[Install]
WantedBy=timers.target
```

```sh
sudo systemctl daemon-reload
sudo systemctl enable --now epicode-hyperspace.timer
```

## Reading the reports

Each cycle writes a pair `hyperspace-<UTC timestamp>-c<cycle>.{json,md}`; the
newest 60 pairs are kept (configurable via `--keep`).

| metric | meaning | what to look for |
|---|---|---|
| cells | memories mirrored into Space4 | should equal seen rows − skipped − rejected |
| clusters / largest cluster | connected components of the vertex-sharing graph | compare against the production cluster count in `health_snapshots`; a healthy 4D space is *more*, *smaller* clusters than 3D (era stratification) |
| era purity | fraction of clusters with w-span ≤ 1.0 (one doubling of age) | research κ=1 runs reached 96–98%; a sustained drop means era bridging is out of control |
| shared-vertex histogram | k → #cell pairs sharing exactly k vertices | **must be ⊆ {0, 1}** (engine-level theorem T1′). Report says `T1′ strict: PASS/FAIL`; any k ≥ 2 bucket is a defect signal |
| mean cluster w-span | average cluster thickness on the era axis | κ=1 ≈ 0.1–0.15; growing values indicate cross-era bridges |
| insert timing | µs stats + cells/s per create | compare against production create throughput (research prototype measured ~1.5k cells/s single-threaded) |

Notes on fidelity:

* Beyond 2000 cells the pair histogram is **sampled** (`"pair_hist_sampled":
  true` in JSON) — a PASS on a sampled census is still a strong signal, but
  a FAIL is unambiguous.
* `retrieval_vs_production` is intentionally **empty** ("pending"): the real
  nDCG benchmark protocol is owned by a separate work package (see
  `compare::vs_production`). Do not improvise one.
* Compared to production semantics the shadow measures what a *pure-create*
Space4 build of the same traffic looks like — it does not replay production
lifecycle (decay/fission/merge/dream), searches, or deletes.

## Development status on this machine

* `cargo test --target x86_64-pc-windows-gnu`: **all tests green** (2026-11-15,
  stable-gnu toolchain, mingw64 gcc for bundled SQLite).
* `cargo fmt`: applied.

(If a local build fails on another machine — typically the bundled SQLite C
compile needs a working gcc/clang — the code and tests are still complete;
CI on ubuntu builds this crate standalone.)

## Layout

```
hyperspace-shadow/
├── Cargo.toml         # bin epicode-hyperspace, dep: rusqlite 0.41 bundled
├── README.md
└── src/
    ├── main.rs        # CLI + shadow loop
    ├── ingest.rs      # read-only SQLite → TetraRow → Space4 feed (+ tests)
    ├── compare.rs     # structure metrics, T1′ validation, timing (+ tests)
    ├── report.rs      # JSON+MD output, rolling retention (+ tests)
    └── engine/        # VENDORED from research/four-dim v0.2 — do not edit
```

<!-- bench-trigger: real file change to fire paths-filtered CI -->
