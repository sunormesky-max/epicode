//! Central scheduler Dispatch: who gets the work, who owns the result.
//!
//! Deterministic assignment (capability match + least-loaded). Clean-room Epicode.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use super::agent_action::{
    ActionFrontier, ActionState, AgentAction, FailureKind, MAX_TRANSIENT_RETRIES,
};

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerRecord {
    pub worker_id: String,
    pub capabilities: Vec<String>,
    /// Max concurrent claimed actions. Default 3.
    pub max_load: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blacklisted_until: Option<i64>,
    pub registered_at: i64,
    pub updated_at: i64,
}

impl WorkerRecord {
    pub fn is_available(&self, now: i64) -> bool {
        self.blacklisted_until.is_none_or(|t| now >= t)
    }

    pub fn matches(&self, required: &[String]) -> bool {
        if required.is_empty() {
            return true;
        }
        required.iter().all(|need| {
            let n = need.to_lowercase();
            self.capabilities
                .iter()
                .any(|c| c.eq_ignore_ascii_case(&n) || c.to_lowercase().contains(&n))
        })
    }
}

#[derive(Default)]
pub struct WorkerRegistry {
    workers: Mutex<HashMap<String, WorkerRecord>>,
}

impl WorkerRegistry {
    pub fn new() -> Self {
        Self {
            workers: Mutex::new(HashMap::new()),
        }
    }

    pub fn register(
        &self,
        worker_id: &str,
        capabilities: Vec<String>,
        max_load: Option<u32>,
    ) -> Result<WorkerRecord, String> {
        let id = worker_id.trim();
        if id.is_empty() {
            return Err("worker_id is required".into());
        }
        let now = now_secs();
        let mut map = self.workers.lock();
        let rec = if let Some(existing) = map.get_mut(id) {
            existing.capabilities = capabilities;
            if let Some(m) = max_load {
                existing.max_load = m.max(1);
            }
            existing.updated_at = now;
            existing.clone()
        } else {
            let rec = WorkerRecord {
                worker_id: id.to_string(),
                capabilities,
                max_load: max_load.unwrap_or(3).max(1),
                blacklisted_until: None,
                registered_at: now,
                updated_at: now,
            };
            map.insert(id.to_string(), rec.clone());
            rec
        };
        Ok(rec)
    }

    pub fn get(&self, worker_id: &str) -> Option<WorkerRecord> {
        self.workers.lock().get(worker_id).cloned()
    }

    pub fn list(&self) -> Vec<WorkerRecord> {
        let mut v: Vec<_> = self.workers.lock().values().cloned().collect();
        v.sort_by(|a, b| a.worker_id.cmp(&b.worker_id));
        v
    }

    pub fn blacklist(&self, worker_id: &str, secs: i64) {
        let until = now_secs().saturating_add(secs.max(0));
        if let Some(w) = self.workers.lock().get_mut(worker_id) {
            w.blacklisted_until = Some(until);
            w.updated_at = now_secs();
        }
    }

    pub fn snapshot(&self) -> Vec<WorkerRecord> {
        self.list()
    }

    pub fn restore(&self, workers: Vec<WorkerRecord>) {
        let mut map = self.workers.lock();
        map.clear();
        for w in workers {
            map.insert(w.worker_id.clone(), w);
        }
    }

    pub fn load_of(frontier: &ActionFrontier, worker_id: &str) -> u32 {
        frontier
            .list(Some(ActionState::Claimed), 1000)
            .into_iter()
            // Expired leases are not live work: they must not pin max_load.
            .filter(|a| frontier.holds_live_claim(a))
            .filter(|a| {
                a.assignee.as_deref() == Some(worker_id)
                    || a.claimed_by.as_deref() == Some(worker_id)
            })
            .count() as u32
    }

    /// Deterministic pick: capable ∧ available ∧ under max_load → least load → worker_id asc.
    pub fn pick(
        &self,
        frontier: &ActionFrontier,
        required: &[String],
    ) -> Result<WorkerRecord, String> {
        let now = now_secs();
        let workers = self.list();
        if workers.is_empty() {
            return Err("no workers registered — call worker_register first".into());
        }
        let mut candidates: Vec<(u32, WorkerRecord)> = workers
            .into_iter()
            .filter(|w| w.is_available(now) && w.matches(required))
            .map(|w| {
                let load = Self::load_of(frontier, &w.worker_id);
                (load, w)
            })
            .filter(|(load, w)| *load < w.max_load)
            .collect();
        if candidates.is_empty() {
            return Err(
                "no capable worker under max_load (or all blacklisted) — register more or wait"
                    .into(),
            );
        }
        candidates.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then_with(|| a.1.worker_id.cmp(&b.1.worker_id))
        });
        Ok(candidates[0].1.clone())
    }
}

pub fn auto_assign(
    registry: &WorkerRegistry,
    frontier: &ActionFrontier,
    action_id: u64,
) -> Result<(AgentAction, WorkerRecord), String> {
    let action = frontier
        .get(action_id)
        .ok_or_else(|| format!("action {} not found", action_id))?;
    if action.state != ActionState::Open {
        return Err(format!(
            "action {} is {:?}, expected open",
            action_id, action.state
        ));
    }
    let worker = registry.pick(frontier, &action.required_capabilities)?;
    let assigned = frontier.assign(action_id, &worker.worker_id)?;
    Ok((assigned, worker))
}

pub fn dispatch_batch(
    registry: &WorkerRegistry,
    frontier: &ActionFrontier,
    limit: usize,
) -> Vec<serde_json::Value> {
    let limit = limit.clamp(1, 20);
    let open = frontier.list(Some(ActionState::Open), 100);
    let mut out = Vec::new();
    // Skip actions still inside a transient backoff window.
    for action in open.into_iter().filter(|a| frontier.is_ready(a)) {
        if out.len() >= limit {
            break;
        }
        match auto_assign(registry, frontier, action.id) {
            Ok((a, w)) => out.push(serde_json::json!({
                "ok": true,
                "action": a.to_action_card(),
                "worker_id": w.worker_id,
            })),
            Err(e) => out.push(serde_json::json!({
                "ok": false,
                "action_id": action.id,
                "error": e,
            })),
        }
    }
    out
}

/// Default max requeues before escalate.
pub const MAX_REQUEUES: u32 = 2;
pub const DEFAULT_BLACKLIST_SECS: i64 = 60;

/// On blocked/exhausted: requeue (clear ownership) or escalate. Returns action + disposition.
/// Back-compat wrapper: classifies the failure as Blocked.
pub fn account_failure(
    registry: &WorkerRegistry,
    frontier: &ActionFrontier,
    action_id: u64,
    reason: &str,
) -> Result<(AgentAction, &'static str), String> {
    account_failure_kind(registry, frontier, action_id, FailureKind::Blocked, reason)
}

/// PR-I/PR-J accountability by failure kind:
/// - Transient → `retry_backoff` (no budget, no blacklist) until MAX_TRANSIENT_RETRIES,
///   then treated as Blocked.
/// - Exhausted → requeue/escalate, budget consumed, worker NOT blacklisted.
/// - LeaseExpired / Blocked → requeue/escalate, budget consumed, worker blacklisted.
pub fn account_failure_kind(
    registry: &WorkerRegistry,
    frontier: &ActionFrontier,
    action_id: u64,
    kind: FailureKind,
    reason: &str,
) -> Result<(AgentAction, &'static str), String> {
    let action = frontier
        .get(action_id)
        .ok_or_else(|| format!("action {} not found", action_id))?;
    let mut kind = kind;
    if kind == FailureKind::Transient {
        if action.transient_retries < MAX_TRANSIENT_RETRIES {
            let a = frontier.retry_transient(action_id, reason)?;
            return Ok((a, "retry_backoff"));
        }
        kind = FailureKind::Blocked;
    }
    let responsible = action
        .assignee
        .clone()
        .or_else(|| action.claimed_by.clone());
    if kind.blames_worker() {
        if let Some(ref wid) = responsible {
            registry.blacklist(wid, DEFAULT_BLACKLIST_SECS);
        }
    }
    let res = if action.requeue_count < MAX_REQUEUES {
        frontier
            .requeue(action_id, reason, 1)
            .map(|a| (a, "requeued"))
    } else {
        frontier
            .escalate(action_id, reason)
            .map(|a| (a, "escalated"))
    }?;
    frontier.set_failure_kind(action_id, kind);
    // PR-K: an escalated upstream blocks its dependents (no silent waiting forever).
    if res.1 == "escalated" {
        frontier.cascade_dep_failures();
    }
    let a = frontier.get(action_id).unwrap_or(res.0);
    Ok((a, res.1))
}

/// Failure reason recorded when a claim lease lapses without heartbeat/complete.
pub const LEASE_EXPIRED_REASON: &str = "lease_expired";

/// Routine/restart sweep: every Claimed action past its lease goes through the
/// same accountability path as blocked/exhausted (requeue ≤ MAX_REQUEUES, else
/// escalate). Returns (action_id, disposition).
pub fn sweep_expired_leases(
    registry: &WorkerRegistry,
    frontier: &ActionFrontier,
) -> Vec<(u64, &'static str)> {
    frontier
        .expired_claims()
        .into_iter()
        .filter_map(|id| {
            account_failure_kind(
                registry,
                frontier,
                id,
                FailureKind::LeaseExpired,
                LEASE_EXPIRED_REASON,
            )
            .ok()
            .map(|(_, d)| (id, d))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::agent_action::build_proposals;

    #[test]
    fn pick_least_loaded_capable() {
        let reg = WorkerRegistry::new();
        reg.register("alice", vec!["ops".into(), "rust".into()], Some(5))
            .unwrap();
        reg.register("bob", vec!["ops".into()], Some(5)).unwrap();
        let f = ActionFrontier::new();
        let busy = f
            .enqueue(
                build_proposals("job", &[(9, "x".into())], &[], 1)[0]
                    .clone()
                    .into_action("job"),
            )
            .unwrap();
        f.assign(busy.id, "alice").unwrap();
        let open = f
            .enqueue(
                build_proposals("job2", &[(8, "y".into())], &[], 1)[0]
                    .clone()
                    .into_action("job2"),
            )
            .unwrap();
        f.set_required_capabilities(open.id, vec!["ops".into()])
            .unwrap();
        let open = f.get(open.id).unwrap();
        let picked = reg.pick(&f, &open.required_capabilities).unwrap();
        assert_eq!(picked.worker_id, "bob");
    }

    #[test]
    fn foreign_complete_rejected() {
        let f = ActionFrontier::new();
        let a = f
            .enqueue(
                build_proposals("q", &[(1, "e".into())], &[], 1)[0]
                    .clone()
                    .into_action("q"),
            )
            .unwrap();
        f.assign(a.id, "alice").unwrap();
        let err = f
            .complete_as(a.id, 99, ActionState::Done, "bob", false)
            .unwrap_err();
        assert!(err.contains("assignee") || err.contains("alice"), "{err}");
        assert!(f
            .complete_as(a.id, 99, ActionState::Done, "alice", false)
            .is_ok());
    }

    #[test]
    fn requeue_then_escalate_after_max() {
        let reg = WorkerRegistry::new();
        reg.register("alice", vec!["ops".into()], Some(3)).unwrap();
        let f = ActionFrontier::new();
        let a = f
            .enqueue(
                build_proposals("q", &[(1, "e".into())], &[], 1)[0]
                    .clone()
                    .into_action("q"),
            )
            .unwrap();
        f.assign(a.id, "alice").unwrap();
        let (a1, d1) = account_failure(&reg, &f, a.id, "timeout").unwrap();
        assert_eq!(d1, "requeued");
        assert_eq!(a1.requeue_count, 1);
        f.assign(a.id, "alice").unwrap();
        let (a2, d2) = account_failure(&reg, &f, a.id, "timeout").unwrap();
        assert_eq!(d2, "requeued");
        assert_eq!(a2.requeue_count, 2);
        f.assign(a.id, "alice").unwrap();
        // After 2 requeues, next failure escalates
        let (a3, d3) = account_failure(&reg, &f, a.id, "timeout").unwrap();
        assert_eq!(d3, "escalated");
        assert!(a3.escalated);
    }

    /// PR-I: a claim restored from storage whose owner vanished long ago must not
    /// count toward worker load (fails on #255: stale claims pin max_load forever).
    #[test]
    fn stale_restored_claim_does_not_pin_load() {
        let reg = WorkerRegistry::new();
        reg.register("ghost", vec![], Some(1)).unwrap();
        let f = ActionFrontier::new();
        let a = f
            .enqueue(
                build_proposals("job", &[(9, "x".into())], &[], 1)[0]
                    .clone()
                    .into_action("job"),
            )
            .unwrap();
        f.claim(a.id, "ghost").unwrap();
        let mut snap = f.snapshot();
        for x in snap.iter_mut() {
            x.updated_at = 1_000;
            x.assigned_at = Some(1_000);
            // Legacy (#255) snapshot shape: no lease fields persisted.
            x.claimed_at = None;
            x.lease_until = None;
        }
        let restored = ActionFrontier::new();
        restored.restore(snap);
        // Expired lease is not live load even before the sweep runs.
        assert_eq!(WorkerRegistry::load_of(&restored, "ghost"), 0);
        // Restart recovery: expired claim goes back to Open with accountability trail.
        let swept = sweep_expired_leases(&reg, &restored);
        assert_eq!(swept, vec![(a.id, "requeued")]);
        let back = restored.get(a.id).unwrap();
        assert_eq!(back.state, ActionState::Open);
        assert_eq!(back.requeue_count, 1);
        assert_eq!(
            back.last_failure_reason.as_deref(),
            Some(LEASE_EXPIRED_REASON)
        );
        assert_eq!(back.last_responsible_assignee.as_deref(), Some("ghost"));
        assert!(back.lease_until.is_none() && back.assignee.is_none());
        // Ghost is blacklisted; a live worker picks the reclaimed action up.
        reg.register("spare", vec![], Some(1)).unwrap();
        let (_, w) = auto_assign(&reg, &restored, a.id).unwrap();
        assert_eq!(w.worker_id, "spare");
        assert!(sweep_expired_leases(&reg, &restored).is_empty());
    }

    fn one_action(f: &ActionFrontier) -> AgentAction {
        f.enqueue(
            build_proposals("job", &[(9, "x".into())], &[], 1)[0]
                .clone()
                .into_action("job"),
        )
        .unwrap()
    }

    /// PR-J: failure kinds have distinct budget/blame semantics.
    #[test]
    fn failure_kinds_budget_and_blame() {
        use crate::engine::agent_action::{transient_backoff_secs, FailureKind};
        use std::sync::atomic::{AtomicI64, Ordering};
        use std::sync::Arc;
        let t = Arc::new(AtomicI64::new(50_000));
        let tc = t.clone();
        let reg = WorkerRegistry::new();
        reg.register("w", vec![], Some(5)).unwrap();
        let f = ActionFrontier::with_clock(Arc::new(move || tc.load(Ordering::SeqCst)), 900);

        // Transient: backoff gate, no budget, no blacklist; capped then real.
        let a = one_action(&f);
        for n in 1..=MAX_TRANSIENT_RETRIES {
            f.claim(a.id, "w").unwrap();
            let (x, d) =
                account_failure_kind(&reg, &f, a.id, FailureKind::Transient, "503").unwrap();
            assert_eq!(d, "retry_backoff");
            assert_eq!(x.requeue_count, 0);
            let now = t.load(Ordering::SeqCst);
            assert_eq!(x.not_before, Some(now + transient_backoff_secs(n)));
            assert!(
                f.claim(a.id, "w").is_err(),
                "claim inside backoff must fail"
            );
            assert!(
                dispatch_batch(&reg, &f, 5).is_empty(),
                "dispatch skips backoff"
            );
            t.store(x.not_before.unwrap(), Ordering::SeqCst);
        }
        assert!(reg.get("w").unwrap().blacklisted_until.is_none());
        f.claim(a.id, "w").unwrap();
        let (x, d) = account_failure_kind(&reg, &f, a.id, FailureKind::Transient, "503").unwrap();
        assert_eq!(d, "requeued", "transient past cap counts as real failure");
        assert_eq!(x.requeue_count, 1);
        assert_eq!(x.last_failure_kind, Some(FailureKind::Blocked));
        assert!(reg.get("w").unwrap().blacklisted_until.is_some());

        // Exhausted: consumes budget, does not blacklist.
        let reg2 = WorkerRegistry::new();
        reg2.register("e", vec![], Some(5)).unwrap();
        let b = one_action(&f);
        f.claim(b.id, "e").unwrap();
        let (y, d) =
            account_failure_kind(&reg2, &f, b.id, FailureKind::Exhausted, "max_steps").unwrap();
        assert_eq!(d, "requeued");
        assert_eq!(y.requeue_count, 1);
        assert_eq!(y.last_failure_kind, Some(FailureKind::Exhausted));
        assert!(reg2.get("e").unwrap().blacklisted_until.is_none());
    }

    #[test]
    fn transient_backoff_is_exponential_and_capped() {
        use crate::engine::agent_action::transient_backoff_secs;
        assert_eq!(transient_backoff_secs(1), 30);
        assert_eq!(transient_backoff_secs(2), 60);
        assert_eq!(transient_backoff_secs(3), 120);
        assert_eq!(transient_backoff_secs(10), 900);
        assert_eq!(transient_backoff_secs(40), 900);
    }
}
