//! The w-axis: bitemporal time compressed logarithmically.
//!
//! `w(t) = log₂(1 + days_since_space_birth)` where `t₀` is the birth of the
//! memory space (first memory ever written), NOT the unix epoch — era
//! boundaries must anchor to the account's own history, not 1970.
//!
//! Properties (all deliberate design choices, see the research report):
//!
//! * **Weber–Fechner resolution**: Δw between two events depends only on the
//!   ratio of their `(elapsed + 1 day)` — not on absolute distance. Doubling
//!   the age of anything costs exactly Δw = 1: recent eras are resolved
//!   finely, ancient ones coarsely, at constant *relative* resolution.
//! * **1 unit of w = one doubling** of elapsed time. The natural width of a
//!   5-cell (≈ 2·√(2/5) ≈ 1.26) therefore spans a factor-2.4 age window:
//!   one cell ≈ one "era" of a topic.
//! * **w = 0 is the birth moment**; the present surface `w(now)` is the
//!   largest w in the space. Older memories sit in deeper strata — the
//!   space becomes a geological record of its own past.
//! * **as_of(t) is a hyperplane**: a bitemporal query at time t intersects
//!   the space with `w = w(t)`, turning Zep-style `as_of` filtering
//!   (epicode: `search_engine.rs:1418`) into pure geometry — no payload scan.
//! * **Decay becomes linear-ish in w**: epicode's `0.98^days` importance
//!   decay is exponential in linear time, i.e. roughly linear per doubling
//!   of age — one doubling of age ≈ a fixed multiplicative decay step, which
//!   is exactly one unit of w.

/// Seconds per day.
pub const SECS_PER_DAY: f64 = 86400.0;

/// Compressed coordinate of absolute unix time `secs`, anchored at space
/// birth `t0_secs`. Monotone increasing; `w(t0) = 0`.
pub fn w_of_unix_secs(secs: i64, t0_secs: i64) -> f64 {
    let days = (secs.max(t0_secs) - t0_secs) as f64 / SECS_PER_DAY;
    (1.0 + days).log2()
}

/// Inverse of [`w_of_unix_secs`]: absolute unix time of a w coordinate.
pub fn unix_secs_of_w(w: f64, t0_secs: i64) -> i64 {
    t0_secs + (((2.0_f64).powf(w) - 1.0) * SECS_PER_DAY) as i64
}

/// Bitemporal window of a memory, converted to w coordinates.
/// `valid_until = None` (still true) maps to an open upper bound.
pub fn w_range(valid_from: i64, valid_until: Option<i64>, t0_secs: i64) -> (f64, Option<f64>) {
    let lo = w_of_unix_secs(valid_from, t0_secs);
    let hi = valid_until.map(|u| w_of_unix_secs(u, t0_secs));
    (lo, hi)
}

/// How many days of linear time one Δw of compressed time covers at this w
/// (resolution shrinks toward the present of old eras… and grows for recent
/// ones only relative to earlier ones).
pub fn days_spanned_by(dw: f64) -> f64 {
    (2.0_f64).powf(dw) - 1.0
}

/// Is a cell with window `[w_from, w_to)` valid *at* compressed time `w_as_of`?
/// Same half-open semantics as epicode's bitemporal `as_of` filter.
pub fn valid_at(w_from: f64, w_to: Option<f64>, w_as_of: f64) -> bool {
    w_as_of >= w_from && w_to.map_or(true, |hi| w_as_of < hi)
}
