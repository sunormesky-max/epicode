//! S4 M4 (#398): the shadow engine is now the MAIN-REPO 4D domain —
//! `epicode::domain::{geo4, space4}` (PRs #399/#402/#403/#404).
//!
//! Every 6h shadow cycle therefore feeds production traffic through the
//! exact code M5 will switch the backend onto: the shadow becomes a live
//! A/B verification channel between the main-repo engine and the previous
//! vendored behaviour (S1, issue #369).
//!
//! Kept locally (until their M6/M7 ports): `retrieval.rs` (Space-PPR search)
//! and `lifecycle.rs` (decay/fission/merge/dream) — their APIs were aligned
//! during the port (`snapshot_valid`, `sharing_adjacency_cached`,
//! `create_full`, `Pentachoron` fields).
//!
//! Module aliases preserve the historical import paths
//! (`crate::engine::{vec4, penta, time_axis, space4}`) so the ingest,
//! compare, report and bench call sites stay untouched except where the
//! port intentionally changed a signature (`create_full` dropped `now`).

pub mod lifecycle;
pub mod retrieval;

pub use epicode::domain::geo4 as time_axis;
pub use epicode::domain::geo4 as vec4;
pub use epicode::domain::space4;
pub use epicode::domain::space4 as penta;

pub use space4::{
    CellPayload, PentaId, Pentachoron, Rng, SnapshotCell, Space4, SpaceStats, Validity, VertexId,
};
pub use vec4::{Point4, Vec4};
