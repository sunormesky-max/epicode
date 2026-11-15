//! Vendored copy of the hyperspace true-4D engine (**v0.3**, 2026-11-15) for the
//! S1 shadow integration binary (epicode issue #369).
//!
//! v0.3 = v0.2 + 单锁原子 relocate / 影子带 PPR 降权 ×0.5 / era 带压缩
//! (compact_bands) / 两级锚定网格（research crate 48/48 tests）。
//!
//! Source of truth: `D:\epicode\research\four-dim\hyperspace\src\` — see
//! `research/four-dim/report-true-4d-engine-v02-2026-11-15.md` §6 for the
//! shadow-mode plan. Each file below is a verbatim copy except for one
//! mechanical edit:
//!
//! 1. `crate::…` module paths rewritten to `crate::engine::…`;
//!
//! Trimmed relative to upstream: `bin/` (demo), `sim.rs`, `rot4.rs`,
//! `project.rs` are NOT vendored — the shadow only needs placement, structure
//! and (for the future nDCG work package) retrieval + lifecycle.
//!
//! `dead_code` is allowed wholesale: this is unmodified research code and the
//! shadow binary uses only part of the API surface.

#![allow(dead_code)]
// The re-exports below mirror upstream `lib.rs` for API parity; in a binary
// crate unused `pub use` items warn, hence the blanket allow.
#![allow(unused_imports)]
// Vendored research code (see header): production clippy gates do not apply
// to the upstream snapshot; quality is enforced in the research crate itself.
#![allow(clippy::all)]

pub mod lifecycle;
pub mod penta;
pub mod retrieval;
pub mod space4;
pub mod time_axis;
pub mod topology;
pub mod vec4;

pub use penta::Pentachoron;
pub use space4::{CellPayload, PentaId, Rng, Space4, SpaceStats, VertexId};
pub use vec4::{Point4, Vec4};
