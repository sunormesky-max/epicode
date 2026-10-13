//! Server-side planner pass over the action frontier (PR-L).
//!
//! Deterministic, no LLM. Two wake sources:
//! - **edge**: outcome triggers (`ActionOutcomeLanded`) are buffered and coalesced —
//!   a burst inside the debounce window yields ONE pass carrying all triggers;
//! - **heartbeat**: a periodic pass even without triggers, so stalls are detected
//!   when nothing is happening (the dangerous case).
//!
//! Each pass classifies the frontier and emits observable stall signals:
//! - `no_eligible_worker`: ready actions exist, workers are registered, but none is
//!   capable/available/under max_load (dispatch would fail);
//! - `stalled`: live actions exist but none is ready and none is running — all
//!   waiting on backoff/deps with nothing that could unblock them soon;
//! - `escalated`: actions escalated to a human (requeue budget exhausted).
//!
//! Signals are deduplicated by fingerprint so a persisting condition notifies once.
//! Clean-room Epicode design.

use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashSet;
use std::sync::Arc;

use super::agent_action::{ActionFrontier, ActionState, Clock};
use super::dispatch::WorkerRegistry;

pub const DEFAULT_DEBOUNCE_SECS: i64 = 2;
pub const DEFAULT_HEARTBEAT_SECS: i64 = 60;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Trigger {
    pub action_id: u64,
    pub state: String,
    pub at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PassReason {
    Edge,
    Heartbeat,
    Manual,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StallSignal {
    NoEligibleWorker { action_ids: Vec<u64> },
    Stalled { action_ids: Vec<u64> },
    Escalated { action_id: u64, reason: String },
}

impl StallSignal {
    pub fn fingerprint(&self) -> String {
        match self {
            StallSignal::NoEligibleWorker { action_ids } => format!("new:{:?}", action_ids),
            StallSignal::Stalled { action_ids } => format!("stall:{:?}", action_ids),
            StallSignal::Escalated { action_id, .. } => format!("esc:{}", action_id),
        }
    }

    pub fn tag(&self) -> &'static str {
        match self {
            StallSignal::NoEligibleWorker { .. } => "[action_no_eligible_worker]",
            StallSignal::Stalled { .. } => "[action_stalled]",
            StallSignal::Escalated { .. } => "[action_escalated]",
        }
    }

    pub fn describe(&self) -> String {
        let ids = |v: &[u64]| {
            v.iter()
                .map(|i| format!("#{i}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        match self {
            StallSignal::NoEligibleWorker { action_ids } => format!(
                "{} 就绪动作 {} 没有可用 worker（能力不匹配/满载/黑名单）。请 worker_register 补能力或扩容 / ready actions have no eligible worker.",
                self.tag(),
                ids(action_ids)
            ),
            StallSignal::Stalled { action_ids } => format!(
                "{} 动作 {} 全部在等待（依赖/退避），且没有任何动作在执行。请检查依赖链或人工介入 / frontier stalled: nothing running, nothing ready.",
                self.tag(),
                ids(action_ids)
            ),
            StallSignal::Escalated { action_id, reason } => format!(
                "{} 动作 #{} 重排预算耗尽已升级给人工（原因: {}）。请决定重开、改派或放弃 / escalated to human.",
                self.tag(),
                action_id,
                reason
            ),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PassReport {
    pub reason: PassReason,
    pub at: i64,
    pub triggers: Vec<Trigger>,
    pub ready: usize,
    pub claimed: usize,
    pub waiting: usize,
    pub signals: Vec<StallSignal>,
    /// Signals not seen in earlier passes (these are the ones to notify).
    pub new_signals: Vec<StallSignal>,
}

#[derive(Default)]
struct State {
    pending: Vec<Trigger>,
    first_pending_at: Option<i64>,
    last_pass_at: Option<i64>,
    passes: u64,
    coalesced_triggers: u64,
    active: HashSet<String>,
    last: Option<PassReport>,
}

pub struct PlannerPass {
    clock: Clock,
    debounce_secs: i64,
    heartbeat_secs: i64,
    state: Mutex<State>,
}

impl PlannerPass {
    pub fn new(clock: Clock, debounce_secs: i64, heartbeat_secs: i64) -> Self {
        Self {
            clock,
            debounce_secs: debounce_secs.max(0),
            heartbeat_secs: heartbeat_secs.max(1),
            state: Mutex::new(State::default()),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(
            Arc::new(|| chrono::Utc::now().timestamp()),
            DEFAULT_DEBOUNCE_SECS,
            DEFAULT_HEARTBEAT_SECS,
        )
    }

    fn now(&self) -> i64 {
        (self.clock)()
    }

    /// Buffer one wake trigger (cheap; never runs a pass inline).
    pub fn note_trigger(&self, action_id: u64, state: &str) {
        let now = self.now();
        let mut st = self.state.lock();
        st.first_pending_at.get_or_insert(now);
        st.pending.push(Trigger {
            action_id,
            state: state.to_string(),
            at: now,
        });
    }

    /// Edge when the debounce window since the first buffered trigger has elapsed;
    /// heartbeat when no pass ran for `heartbeat_secs`.
    pub fn due(&self) -> Option<PassReason> {
        let now = self.now();
        let st = self.state.lock();
        if let Some(t0) = st.first_pending_at {
            if now - t0 >= self.debounce_secs {
                return Some(PassReason::Edge);
            }
        }
        match st.last_pass_at {
            None => Some(PassReason::Heartbeat),
            Some(t) if now - t >= self.heartbeat_secs => Some(PassReason::Heartbeat),
            _ => None,
        }
    }

    /// Run a pass if due. Returns the report when one ran.
    pub fn maybe_run(
        &self,
        frontier: &ActionFrontier,
        registry: &WorkerRegistry,
    ) -> Option<PassReport> {
        let reason = self.due()?;
        Some(self.run(reason, frontier, registry))
    }

    pub fn run(
        &self,
        reason: PassReason,
        frontier: &ActionFrontier,
        registry: &WorkerRegistry,
    ) -> PassReport {
        let now = self.now();
        let triggers = {
            let mut st = self.state.lock();
            st.first_pending_at = None;
            std::mem::take(&mut st.pending)
        };
        let actions = frontier.snapshot();
        let mut ready_ids = Vec::new();
        let mut waiting_ids = Vec::new();
        let mut claimed = 0usize;
        let mut signals = Vec::new();
        for a in &actions {
            match a.state {
                ActionState::Open if frontier.is_ready(a) => ready_ids.push(a.id),
                ActionState::Open => waiting_ids.push(a.id),
                ActionState::Claimed if frontier.holds_live_claim(a) => claimed += 1,
                ActionState::Claimed => waiting_ids.push(a.id), // expired, awaiting sweep
                _ => {}
            }
            if a.escalated {
                signals.push(StallSignal::Escalated {
                    action_id: a.id,
                    reason: a.last_failure_reason.clone().unwrap_or_default(),
                });
            }
        }
        ready_ids.sort_unstable();
        waiting_ids.sort_unstable();
        if !registry.list().is_empty() {
            let mut stuck: Vec<u64> = ready_ids
                .iter()
                .copied()
                .filter(|id| {
                    frontier
                        .get(*id)
                        .map(|a| registry.pick(frontier, &a.required_capabilities).is_err())
                        .unwrap_or(false)
                })
                .collect();
            stuck.sort_unstable();
            if !stuck.is_empty() {
                signals.push(StallSignal::NoEligibleWorker { action_ids: stuck });
            }
        }
        if ready_ids.is_empty() && claimed == 0 && !waiting_ids.is_empty() {
            signals.push(StallSignal::Stalled {
                action_ids: waiting_ids.clone(),
            });
        }
        let mut st = self.state.lock();
        let current: HashSet<String> = signals.iter().map(|s| s.fingerprint()).collect();
        let new_signals: Vec<StallSignal> = signals
            .iter()
            .filter(|s| !st.active.contains(&s.fingerprint()))
            .cloned()
            .collect();
        // Escalations stay remembered (notify once ever); conditions re-arm once cleared.
        let keep_esc: HashSet<String> = st
            .active
            .iter()
            .filter(|f| f.starts_with("esc:"))
            .cloned()
            .collect();
        st.active = current.union(&keep_esc).cloned().collect();
        st.passes += 1;
        st.coalesced_triggers += triggers.len() as u64;
        st.last_pass_at = Some(now);
        let report = PassReport {
            reason,
            at: now,
            triggers,
            ready: ready_ids.len(),
            claimed,
            waiting: waiting_ids.len(),
            signals,
            new_signals,
        };
        st.last = Some(report.clone());
        report
    }

    /// Mark a signal as already notified (used by the immediate escalation hook).
    /// Returns true when it was new.
    pub fn mark_notified(&self, sig: &StallSignal) -> bool {
        self.state.lock().active.insert(sig.fingerprint())
    }

    pub fn status(&self) -> serde_json::Value {
        let st = self.state.lock();
        serde_json::json!({
            "debounce_secs": self.debounce_secs,
            "heartbeat_secs": self.heartbeat_secs,
            "passes": st.passes,
            "coalesced_triggers": st.coalesced_triggers,
            "pending_triggers": st.pending.len(),
            "last_pass_at": st.last_pass_at,
            "last": st.last,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::agent_action::build_proposals;
    use std::sync::atomic::{AtomicI64, Ordering};

    fn clock() -> (Arc<AtomicI64>, Clock) {
        let t = Arc::new(AtomicI64::new(1_000));
        let tc = t.clone();
        (t, Arc::new(move || tc.load(Ordering::SeqCst)))
    }

    fn act(f: &ActionFrontier) -> u64 {
        f.enqueue(
            build_proposals("q", &[(1, "e".into())], &[], 1)[0]
                .clone()
                .into_action("q"),
        )
        .unwrap()
        .id
    }

    #[test]
    fn burst_coalesces_into_one_edge_pass() {
        let (t, c) = clock();
        let pp = PlannerPass::new(c.clone(), 2, 60);
        let f = ActionFrontier::with_clock(c, 900);
        let reg = WorkerRegistry::new();
        // first heartbeat establishes last_pass_at
        assert_eq!(
            pp.maybe_run(&f, &reg).unwrap().reason,
            PassReason::Heartbeat
        );
        for i in 0..5 {
            pp.note_trigger(i, "done");
        }
        assert!(pp.maybe_run(&f, &reg).is_none(), "inside debounce window");
        t.store(1_001, Ordering::SeqCst);
        pp.note_trigger(9, "blocked");
        assert!(pp.maybe_run(&f, &reg).is_none());
        t.store(1_002, Ordering::SeqCst);
        let r = pp.maybe_run(&f, &reg).unwrap();
        assert_eq!(r.reason, PassReason::Edge);
        assert_eq!(r.triggers.len(), 6, "whole burst in ONE pass");
        assert!(pp.maybe_run(&f, &reg).is_none(), "drained");
        assert_eq!(pp.status()["passes"], 2);
        assert_eq!(pp.status()["coalesced_triggers"], 6);
    }

    #[test]
    fn heartbeat_fires_without_triggers() {
        let (t, c) = clock();
        let pp = PlannerPass::new(c.clone(), 2, 60);
        let f = ActionFrontier::with_clock(c, 900);
        let reg = WorkerRegistry::new();
        pp.maybe_run(&f, &reg).unwrap();
        t.store(1_059, Ordering::SeqCst);
        assert!(pp.maybe_run(&f, &reg).is_none());
        t.store(1_060, Ordering::SeqCst);
        assert_eq!(
            pp.maybe_run(&f, &reg).unwrap().reason,
            PassReason::Heartbeat
        );
    }

    #[test]
    fn detects_ready_work_without_eligible_worker_once() {
        let (_t, c) = clock();
        let pp = PlannerPass::new(c.clone(), 0, 1);
        let f = ActionFrontier::with_clock(c, 900);
        let reg = WorkerRegistry::new();
        reg.register("py", vec!["python".into()], Some(1)).unwrap();
        let a = act(&f);
        f.set_required_capabilities(a, vec!["rust".into()]).unwrap();
        let r = pp.run(PassReason::Manual, &f, &reg);
        assert_eq!(
            r.new_signals,
            vec![StallSignal::NoEligibleWorker {
                action_ids: vec![a]
            }]
        );
        let r2 = pp.run(PassReason::Manual, &f, &reg);
        assert_eq!(r2.signals.len(), 1);
        assert!(
            r2.new_signals.is_empty(),
            "persisting condition notifies once"
        );
        // Condition clears when a capable worker appears, and re-arms later.
        reg.register("rs", vec!["rust".into()], Some(1)).unwrap();
        assert!(pp.run(PassReason::Manual, &f, &reg).signals.is_empty());
    }

    #[test]
    fn detects_stalled_frontier() {
        let (_t, c) = clock();
        let pp = PlannerPass::new(c.clone(), 0, 1);
        let f = ActionFrontier::with_clock(c, 900);
        let reg = WorkerRegistry::new();
        let a = act(&f);
        let mut b = build_proposals("q", &[(1, "e".into())], &[], 1)[0]
            .clone()
            .into_action("q");
        b.depends_on = vec![a];
        let b = f.enqueue(b).unwrap().id;
        // a running → not stalled
        f.claim(a, "w").unwrap();
        assert!(pp.run(PassReason::Manual, &f, &reg).signals.is_empty());
        // a sent to transient backoff: nothing ready, nothing running → stalled
        f.retry_transient(a, "503").unwrap();
        let r = pp.run(PassReason::Manual, &f, &reg);
        assert_eq!(
            r.new_signals,
            vec![StallSignal::Stalled {
                action_ids: vec![a, b]
            }]
        );
    }
}
