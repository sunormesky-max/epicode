use parking_lot::Mutex;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use super::cognitive::{CognitiveEngine, CognitiveResponse, SchedulerAction, SystemState};

/// Tracks the outcome of a decision for learning / deduplication.
#[derive(Debug, Clone)]
pub struct DecisionOutcome {
    pub tick: u64,
    pub actions_taken: Vec<String>,
    pub actions_skipped: Vec<String>,
    pub llm_latency_ms: u64,
    pub cache_hit: bool,
}

/// Configuration for the decision center.
#[derive(Debug, Clone)]
pub struct DecisionConfig {
    /// Enable state-hash deduplication to skip redundant LLM calls.
    pub enable_dedup: bool,
    /// Minimum ticks between two LLM calls even if state changed.
    pub min_ticks_between_llm: u64,
    /// Max actions to execute per tick (safety guard).
    pub max_actions_per_tick: usize,
    /// Enable action validation before execution.
    pub enable_validation: bool,
    /// Fallback to rule-based decisions when LLM is disabled or fails.
    pub enable_fallback: bool,
}

impl Default for DecisionConfig {
    fn default() -> Self {
        Self {
            enable_dedup: true,
            min_ticks_between_llm: 3,
            max_actions_per_tick: 3,
            enable_validation: true,
            enable_fallback: true,
        }
    }
}

/// DecisionCenter orchestrates the cognitive decision pipeline:
/// - deduplication / caching
/// - pre-decision analysis
/// - LLM invocation (via CognitiveEngine)
/// - action validation & filtering
/// - outcome tracking
pub struct DecisionCenter {
    cognitive: Arc<CognitiveEngine>,
    config: DecisionConfig,
    last_state_hash: Mutex<Option<u64>>,
    last_llm_tick: AtomicU64,
    outcomes: Mutex<Vec<DecisionOutcome>>,
    decision_count: AtomicU64,
    skip_count: AtomicU64,
}

impl DecisionCenter {
    pub fn new(cognitive: Arc<CognitiveEngine>) -> Self {
        Self::with_config(cognitive, DecisionConfig::default())
    }

    pub fn with_config(cognitive: Arc<CognitiveEngine>, config: DecisionConfig) -> Self {
        Self {
            cognitive,
            config,
            last_state_hash: Mutex::new(None),
            last_llm_tick: AtomicU64::new(0),
            outcomes: Mutex::new(Vec::with_capacity(256)),
            decision_count: AtomicU64::new(0),
            skip_count: AtomicU64::new(0),
        }
    }

    /// Decide what actions to take given the current system state.
    /// Returns `Ok(response)` with possibly-empty actions, or `Err` on hard failure.
    pub fn decide(&self, state: &SystemState) -> Result<CognitiveResponse, String> {
        let tick = state.tick;
        let last_llm = self.last_llm_tick.load(Ordering::SeqCst);

        // 1. Rate-limit guard: minimum ticks between LLM calls
        if tick.saturating_sub(last_llm) < self.config.min_ticks_between_llm {
            self.skip_count.fetch_add(1, Ordering::SeqCst);
            tracing::info!(
                "[DecisionCenter] tick {} skipped (rate limit, last_llm={})",
                tick,
                last_llm
            );
            return Ok(CognitiveResponse {
                thoughts: "rate-limited".to_string(),
                actions: vec![],
            });
        }

        // 2. State-hash deduplication
        if self.config.enable_dedup {
            let state_hash = Self::hash_state(state);
            let mut last = self.last_state_hash.lock();
            if let Some(prev) = *last {
                if prev == state_hash {
                    self.skip_count.fetch_add(1, Ordering::SeqCst);
                    tracing::info!(
                        "[DecisionCenter] tick {} skipped (duplicate state hash {})",
                        tick,
                        state_hash
                    );
                    return Ok(CognitiveResponse {
                        thoughts: "duplicate state — no new decision needed".to_string(),
                        actions: vec![],
                    });
                }
            }
            *last = Some(state_hash);
        }

        self.last_llm_tick.store(tick, Ordering::SeqCst);

        // 3. Invoke LLM via CognitiveEngine
        let start = Instant::now();
        let response = self.cognitive.decide(state);
        let latency_ms = start.elapsed().as_millis() as u64;

        match response {
            Ok(mut resp) => {
                // 4. Action validation & filtering
                if self.config.enable_validation {
                    let (valid, invalid): (Vec<SchedulerAction>, Vec<SchedulerAction>) = resp
                        .actions
                        .into_iter()
                        .partition(|a| self.validate_action(state, a));
                    if !invalid.is_empty() {
                        tracing::warn!(
                            "[DecisionCenter] tick {} filtered {} invalid actions",
                            tick,
                            invalid.len()
                        );
                    }
                    resp.actions = valid;
                }

                // 5. Max actions safety guard
                if resp.actions.len() > self.config.max_actions_per_tick {
                    tracing::warn!(
                        "[DecisionCenter] tick {} capped {} actions to {}",
                        tick,
                        resp.actions.len(),
                        self.config.max_actions_per_tick
                    );
                    resp.actions.truncate(self.config.max_actions_per_tick);
                }

                let actions_taken: Vec<String> =
                    resp.actions.iter().map(|a| action_name(a)).collect();

                let outcome = DecisionOutcome {
                    tick,
                    actions_taken,
                    actions_skipped: vec![],
                    llm_latency_ms: latency_ms,
                    cache_hit: false,
                };
                {
                    let mut hist = self.outcomes.lock();
                    if hist.len() >= 256 {
                        hist.drain(0..128);
                    }
                    hist.push(outcome);
                }
                self.decision_count.fetch_add(1, Ordering::SeqCst);

                tracing::info!(
                    "[DecisionCenter] tick {} decided {} actions in {}ms",
                    tick,
                    resp.actions.len(),
                    latency_ms
                );
                Ok(resp)
            }
            Err(e) => {
                // 6. Fallback: if enabled and LLM fails, return empty actions
                if self.config.enable_fallback {
                    tracing::warn!(
                        "[DecisionCenter] tick {} LLM error ({}), falling back to no-op",
                        tick,
                        e
                    );
                    Ok(CognitiveResponse {
                        thoughts: format!("LLM error: {e}"),
                        actions: vec![],
                    })
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Validate a single action against current state constraints.
    fn validate_action(&self, state: &SystemState, action: &SchedulerAction) -> bool {
        match action {
            SchedulerAction::Pulse { origin, .. } => {
                state.memories.iter().any(|m| m.id == *origin)
            }
            SchedulerAction::Fission { cluster_index } => {
                *cluster_index < state.total_clusters
            }
            SchedulerAction::Fuse { cluster_a, cluster_b } => {
                *cluster_a < state.total_clusters && *cluster_b < state.total_clusters
            }
            SchedulerAction::Link { a, b, .. } => {
                let a_ok = state.memories.iter().any(|m| m.id == *a);
                let b_ok = state.memories.iter().any(|m| m.id == *b);
                a_ok && b_ok && a != b
            }
            SchedulerAction::Consolidate { ids, keep, .. } => {
                let id_set: std::collections::HashSet<u64> = ids.iter().copied().collect();
                let all_exist = ids.iter().all(|id| state.memories.iter().any(|m| m.id == *id));
                all_exist && id_set.len() > 1 && id_set.contains(keep)
            }
            SchedulerAction::MarkJunk { ids, .. } => {
                ids.iter().all(|id| state.memories.iter().any(|m| m.id == *id))
            }
            SchedulerAction::Relabel { id, .. } => {
                state.memories.iter().any(|m| m.id == *id)
            }
            SchedulerAction::Reflect { .. } => true,
            SchedulerAction::Dream => true,
            SchedulerAction::UseTool { .. } => true,
        }
    }

    /// Compute a cheap hash of the system state for deduplication.
    fn hash_state(state: &SystemState) -> u64 {
        let mut hasher = DefaultHasher::new();
        state.tick.hash(&mut hasher);
        state.total_tetras.hash(&mut hasher);
        state.total_clusters.hash(&mut hasher);
        state.energy.to_bits().hash(&mut hasher);
        state.total_vertices.hash(&mut hasher);
        for m in &state.memories {
            m.id.hash(&mut hasher);
            m.cluster_index.hash(&mut hasher);
        }
        hasher.finish()
    }

    /// Return summary statistics.
    pub fn stats(&self) -> serde_json::Value {
        let outcomes = self.outcomes.lock();
        let total = self.decision_count.load(Ordering::SeqCst);
        let skipped = self.skip_count.load(Ordering::SeqCst);
        let avg_latency = if !outcomes.is_empty() {
            outcomes.iter().map(|o| o.llm_latency_ms).sum::<u64>() / outcomes.len() as u64
        } else {
            0
        };
        serde_json::json!({
            "total_decisions": total,
            "skipped": skipped,
            "outcomes_tracked": outcomes.len(),
            "avg_latency_ms": avg_latency,
            "last_llm_tick": self.last_llm_tick.load(Ordering::SeqCst),
        })
    }

    pub fn enabled(&self) -> bool {
        self.cognitive.enabled()
    }
}

fn action_name(action: &SchedulerAction) -> String {
    match action {
        SchedulerAction::Pulse { .. } => "pulse".into(),
        SchedulerAction::Fission { .. } => "fission".into(),
        SchedulerAction::Fuse { .. } => "fuse".into(),
        SchedulerAction::Dream => "dream".into(),
        SchedulerAction::Link { .. } => "link".into(),
        SchedulerAction::Consolidate { .. } => "consolidate".into(),
        SchedulerAction::MarkJunk { .. } => "mark_junk".into(),
        SchedulerAction::Relabel { .. } => "relabel".into(),
        SchedulerAction::Reflect { .. } => "reflect".into(),
        SchedulerAction::UseTool { tool, .. } => format!("use_tool:{tool}"),
    }
}
