//! Library target so `src/bin/bench.rs` (the benchmark-protocol geometry
//! bridge) can reuse the ingest/engine modules. The main shadow binary
//! (`src/main.rs`) consumes this same library.

pub mod compare;
pub mod crypto;
pub mod engine;
pub mod ingest;
pub mod report;
