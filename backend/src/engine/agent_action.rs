//! Agent-facing Memory Action Envelope.
//!
//! Turns recalled memories into claimable next actions and requires a structured
//! outcome write-back. Clean-room design for Epicode (not derived from third-party
//! AGPL sources). Complements drive_inbox (will signals) and task_start (time budget).

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionState {
    Open,
    Claimed,
    Done,
    Blocked,
    Exhausted,
}

impl ActionState {
    pub fn as_str(self) -> &'static str {
        match self {
            ActionState::Open => "open",
            ActionState::Claimed => "claimed",
            ActionState::Done => "done",
            ActionState::Blocked => "blocked",
            ActionState::Exhausted => "exhausted",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "open" => Some(Self::Open),
            "claimed" => Some(Self::Claimed),
            "done" => Some(Self::Done),
            "blocked" => Some(Self::Blocked),
            "exhausted" => Some(Self::Exhausted),
            _ => None,
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            ActionState::Done | ActionState::Blocked | ActionState::Exhausted
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestedSkill {
    pub name: String,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAction {
    pub id: u64,
    pub summary: String,
    pub evidence_memory_ids: Vec<u64>,
    pub suggested_skills: Vec<SuggestedSkill>,
    pub done_when: Vec<String>,
    pub stop_if: Vec<String>,
    pub priority: u8,
    pub state: ActionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>,
    /// Immutable owner once set (assign or first claim). Foreign complete rejected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned_at: Option<i64>,
    /// Capability tags for Dispatch matching (empty = any worker).
    #[serde(default)]
    pub required_capabilities: Vec<String>,
    #[serde(default)]
    pub requeue_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_failure_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_responsible_assignee: Option<String>,
    #[serde(default)]
    pub escalated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome_memory_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_query: Option<String>,
    /// Drive signal that materialized this action (Drive→Action bridge). None for propose path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_drive_id: Option<u64>,
    /// Task session bound via task_start(action_id). Cleared never — audit trail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bound_task_id: Option<String>,
    /// Origin tag: "propose" | "drive"
    #[serde(default = "default_origin")]
    pub origin: String,
    pub created_at: i64,
    pub updated_at: i64,
}

fn default_origin() -> String {
    "propose".into()
}

/// Stable agent-facing card schema (Memory Action Envelope output).
pub const ACTION_CARD_SCHEMA_VERSION: u32 = 2;

impl AgentAction {
    /// Slim but complete ActionCard — agents must not invent their own schema.
    pub fn to_action_card(&self) -> serde_json::Value {
        serde_json::json!({
            "schema": "epicode.action_card",
            "schema_version": ACTION_CARD_SCHEMA_VERSION,
            "id": self.id,
            "summary": self.summary,
            "state": self.state.as_str(),
            "priority": self.priority,
            "evidence_memory_ids": self.evidence_memory_ids,
            "suggested_skills": self.suggested_skills,
            "done_when": self.done_when,
            "stop_if": self.stop_if,
            "required_capabilities": self.required_capabilities,
            "assignee": self.assignee,
            "assigned_at": self.assigned_at,
            "requeue_count": self.requeue_count,
            "last_failure_reason": self.last_failure_reason,
            "escalated": self.escalated,
            "lineage": {
                "origin": self.origin,
                "source_query": self.source_query,
                "source_drive_id": self.source_drive_id,
                "bound_task_id": self.bound_task_id,
                "outcome_memory_id": self.outcome_memory_id,
                "claimed_by": self.claimed_by,
                "assignee": self.assignee,
                "last_responsible_assignee": self.last_responsible_assignee,
                "parents": self.evidence_memory_ids,
            },
            "created_at": self.created_at,
            "updated_at": self.updated_at,
        })
    }

    /// Backward-compatible alias used by MCP tools.
    pub fn to_json(&self) -> serde_json::Value {
        self.to_action_card()
    }
}

/// In-process frontier of agent actions (persisted via scheduler KV).
pub struct ActionFrontier {
    next_id: AtomicU64,
    actions: Mutex<HashMap<u64, AgentAction>>,
    /// Soft cap on non-terminal actions to avoid unbounded growth.
    max_open: usize,
}

impl Default for ActionFrontier {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionFrontier {
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            actions: Mutex::new(HashMap::new()),
            max_open: 64,
        }
    }

    pub fn snapshot(&self) -> Vec<AgentAction> {
        let map = self.actions.lock();
        let mut v: Vec<AgentAction> = map.values().cloned().collect();
        v.sort_by(|a, b| b.priority.cmp(&a.priority).then_with(|| b.id.cmp(&a.id)));
        v
    }

    pub fn restore(&self, actions: Vec<AgentAction>) {
        let mut max_id = 0u64;
        let mut map = self.actions.lock();
        map.clear();
        for a in actions {
            max_id = max_id.max(a.id);
            map.insert(a.id, a);
        }
        self.next_id
            .store(max_id.saturating_add(1).max(1), Ordering::SeqCst);
    }

    pub fn open_count(&self) -> usize {
        self.actions
            .lock()
            .values()
            .filter(|a| !a.state.is_terminal())
            .count()
    }

    pub fn enqueue(&self, mut action: AgentAction) -> Result<AgentAction, String> {
        if self.open_count() >= self.max_open {
            return Err(format!(
                "action frontier full ({} non-terminal); complete or block existing actions first",
                self.max_open
            ));
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let now = now_secs();
        action.id = id;
        action.state = ActionState::Open;
        action.created_at = now;
        action.updated_at = now;
        action.claimed_by = None;
        action.outcome_memory_id = None;
        self.actions.lock().insert(id, action.clone());
        Ok(action)
    }

    pub fn get(&self, id: u64) -> Option<AgentAction> {
        self.actions.lock().get(&id).cloned()
    }

    pub fn list(&self, state: Option<ActionState>, limit: usize) -> Vec<AgentAction> {
        let limit = limit.clamp(1, 100);
        let mut v: Vec<AgentAction> = self
            .actions
            .lock()
            .values()
            .filter(|a| state.is_none_or(|s| a.state == s))
            .cloned()
            .collect();
        v.sort_by(|a, b| b.priority.cmp(&a.priority).then_with(|| b.id.cmp(&a.id)));
        v.truncate(limit);
        v
    }

    /// Compare-and-set claim: only succeeds when current state is Open.
    /// If assignee is already set to someone else → reject (ownership).
    pub fn claim(&self, id: u64, claimed_by: &str) -> Result<AgentAction, String> {
        let mut map = self.actions.lock();
        let action = map
            .get_mut(&id)
            .ok_or_else(|| format!("action {} not found", id))?;
        if action.state != ActionState::Open {
            return Err(format!(
                "action {} is {:?}, expected open",
                id, action.state
            ));
        }
        if let Some(a) = &action.assignee {
            if a != claimed_by {
                return Err(format!(
                    "action {} assigned to '{}', not '{}' (ownership)",
                    id, a, claimed_by
                ));
            }
        }
        let now = now_secs();
        action.state = ActionState::Claimed;
        action.claimed_by = Some(claimed_by.to_string());
        if action.assignee.is_none() {
            action.assignee = Some(claimed_by.to_string());
            action.assigned_at = Some(now);
        }
        action.updated_at = now;
        Ok(action.clone())
    }

    /// Scheduler assign: Open → Claimed with immutable assignee.
    pub fn assign(&self, id: u64, worker_id: &str) -> Result<AgentAction, String> {
        let worker_id = worker_id.trim();
        if worker_id.is_empty() {
            return Err("worker_id is required".into());
        }
        let mut map = self.actions.lock();
        let action = map
            .get_mut(&id)
            .ok_or_else(|| format!("action {} not found", id))?;
        if action.state != ActionState::Open {
            return Err(format!(
                "action {} is {:?}, expected open for assign",
                id, action.state
            ));
        }
        if let Some(a) = &action.assignee {
            return Err(format!("action {} already assigned to '{}'", id, a));
        }
        let now = now_secs();
        action.state = ActionState::Claimed;
        action.assignee = Some(worker_id.to_string());
        action.assigned_at = Some(now);
        action.claimed_by = Some(worker_id.to_string());
        action.updated_at = now;
        Ok(action.clone())
    }

    /// Complete with ownership check. Admin may override.
    pub fn complete_as(
        &self,
        id: u64,
        outcome_memory_id: u64,
        terminal: ActionState,
        actor: &str,
        is_admin: bool,
    ) -> Result<AgentAction, String> {
        if !matches!(
            terminal,
            ActionState::Done | ActionState::Blocked | ActionState::Exhausted
        ) {
            return Err("terminal state must be done|blocked|exhausted".into());
        }
        let mut map = self.actions.lock();
        let action = map
            .get_mut(&id)
            .ok_or_else(|| format!("action {} not found", id))?;
        if action.state.is_terminal() {
            return Err(format!(
                "action {} already terminal ({:?})",
                id, action.state
            ));
        }
        let owner = action.assignee.as_deref().or(action.claimed_by.as_deref());
        if let Some(owner) = owner {
            if owner != actor && !is_admin {
                return Err(format!(
                    "action {} owned by assignee '{}'; '{}' cannot complete (ownership)",
                    id, owner, actor
                ));
            }
        }
        action.state = terminal;
        action.outcome_memory_id = Some(outcome_memory_id);
        action.updated_at = now_secs();
        if matches!(terminal, ActionState::Blocked | ActionState::Exhausted) {
            action.last_failure_reason = Some(format!("terminal:{:?}", terminal).to_lowercase());
            action.last_responsible_assignee = action.assignee.clone();
        }
        Ok(action.clone())
    }

    /// Requeue: clear claim/assignee, bump priority, record failure trail.
    pub fn requeue(&self, id: u64, reason: &str, priority_bump: u8) -> Result<AgentAction, String> {
        let mut map = self.actions.lock();
        let action = map
            .get_mut(&id)
            .ok_or_else(|| format!("action {} not found", id))?;
        if action.state == ActionState::Open {
            return Err(format!("action {} is already open", id));
        }
        if action.state == ActionState::Done {
            return Err(format!("action {} is done — cannot requeue", id));
        }
        let responsible = action
            .assignee
            .clone()
            .or_else(|| action.claimed_by.clone());
        action.last_responsible_assignee = responsible;
        action.last_failure_reason = Some(reason.to_string());
        action.requeue_count = action.requeue_count.saturating_add(1);
        action.state = ActionState::Open;
        action.claimed_by = None;
        action.assignee = None;
        action.assigned_at = None;
        action.escalated = false;
        action.priority = action.priority.saturating_add(priority_bump).min(10);
        action.updated_at = now_secs();
        Ok(action.clone())
    }

    /// Escalate: keep terminal / mark claimed→blocked with escalated flag.
    pub fn escalate(&self, id: u64, reason: &str) -> Result<AgentAction, String> {
        let mut map = self.actions.lock();
        let action = map
            .get_mut(&id)
            .ok_or_else(|| format!("action {} not found", id))?;
        action.escalated = true;
        action.last_failure_reason = Some(reason.to_string());
        action.last_responsible_assignee = action
            .assignee
            .clone()
            .or_else(|| action.claimed_by.clone());
        if !action.state.is_terminal() {
            action.state = ActionState::Blocked;
        }
        action.updated_at = now_secs();
        Ok(action.clone())
    }

    pub fn set_required_capabilities(
        &self,
        id: u64,
        caps: Vec<String>,
    ) -> Result<AgentAction, String> {
        let mut map = self.actions.lock();
        let action = map
            .get_mut(&id)
            .ok_or_else(|| format!("action {} not found", id))?;
        action.required_capabilities = caps;
        action.updated_at = now_secs();
        Ok(action.clone())
    }

    /// Backward-compatible complete without actor check (prefer complete_as).
    pub fn complete(
        &self,
        id: u64,
        outcome_memory_id: u64,
        terminal: ActionState,
    ) -> Result<AgentAction, String> {
        let actor = self
            .get(id)
            .and_then(|a| a.assignee.or(a.claimed_by))
            .unwrap_or_else(|| "system".into());
        self.complete_as(id, outcome_memory_id, terminal, &actor, true)
    }

    /// True when a non-terminal action already cites this drive (anti double-spend).
    pub fn has_live_drive(&self, drive_id: u64) -> bool {
        self.actions
            .lock()
            .values()
            .any(|a| a.source_drive_id == Some(drive_id) && !a.state.is_terminal())
    }

    pub fn find_by_drive(&self, drive_id: u64) -> Vec<AgentAction> {
        let mut v: Vec<AgentAction> = self
            .actions
            .lock()
            .values()
            .filter(|a| a.source_drive_id == Some(drive_id))
            .cloned()
            .collect();
        v.sort_by_key(|a| std::cmp::Reverse(a.id));
        v
    }

    /// Bind a claimed action to a task. Fails if not claimed by `claimer` or already bound.
    pub fn bind_task(&self, id: u64, task_id: &str, claimer: &str) -> Result<AgentAction, String> {
        let mut map = self.actions.lock();
        let action = map
            .get_mut(&id)
            .ok_or_else(|| format!("action {} not found", id))?;
        if action.state != ActionState::Claimed {
            return Err(format!(
                "action {} must be claimed before task_start (state={:?})",
                id, action.state
            ));
        }
        match &action.claimed_by {
            Some(who) if who == claimer => {}
            Some(who) => {
                return Err(format!(
                    "action {} claimed by '{}', not '{}'",
                    id, who, claimer
                ))
            }
            None => return Err(format!("action {} has no claimed_by", id)),
        }
        if let Some(existing) = &action.bound_task_id {
            if existing != task_id {
                return Err(format!("action {} already bound to task {}", id, existing));
            }
        }
        action.bound_task_id = Some(task_id.to_string());
        action.updated_at = now_secs();
        Ok(action.clone())
    }

    pub fn find_by_task(&self, task_id: &str) -> Option<AgentAction> {
        self.actions
            .lock()
            .values()
            .find(|a| a.bound_task_id.as_deref() == Some(task_id))
            .cloned()
    }

    /// Drop terminal actions older than `max_age_secs` (keeps open/claimed forever).
    pub fn prune_terminal_older_than(&self, max_age_secs: i64) -> usize {
        let cutoff = now_secs().saturating_sub(max_age_secs);
        let mut map = self.actions.lock();
        let before = map.len();
        map.retain(|_, a| !(a.state.is_terminal() && a.updated_at < cutoff));
        before.saturating_sub(map.len())
    }

    pub fn stats(&self) -> serde_json::Value {
        let map = self.actions.lock();
        let mut open = 0u64;
        let mut claimed = 0u64;
        let mut done = 0u64;
        let mut blocked = 0u64;
        let mut exhausted = 0u64;
        for a in map.values() {
            match a.state {
                ActionState::Open => open += 1,
                ActionState::Claimed => claimed += 1,
                ActionState::Done => done += 1,
                ActionState::Blocked => blocked += 1,
                ActionState::Exhausted => exhausted += 1,
            }
        }
        serde_json::json!({
            "total": map.len(),
            "open": open,
            "claimed": claimed,
            "done": done,
            "blocked": blocked,
            "exhausted": exhausted,
            "non_terminal": open + claimed,
        })
    }
}

/// Build a single draft from a grounded drive signal description + evidence ids.
pub fn build_drive_draft(
    drive_id: u64,
    description: &str,
    evidence_ids: &[u64],
    skills: &[(String, f64)],
) -> Option<AgentActionDraft> {
    if evidence_ids.is_empty() {
        return None;
    }
    let suggested: Vec<SuggestedSkill> = skills
        .iter()
        .take(5)
        .map(|(n, s)| SuggestedSkill {
            name: n.clone(),
            score: (*s * 1000.0).round() / 1000.0,
        })
        .collect();
    let desc = description.trim();
    Some(AgentActionDraft {
        summary: format!(
            "Drive #{}: {}",
            drive_id,
            if desc.is_empty() {
                "act on grounded evidence"
            } else {
                desc
            }
        ),
        evidence_memory_ids: evidence_ids.to_vec(),
        required_capabilities: suggested.iter().map(|s| s.name.clone()).collect(),
        suggested_skills: suggested,
        done_when: vec![
            format!(
                "Drive #{} intent fulfilled with artifact-level outcome",
                drive_id
            ),
            "Evidence memories cited in the outcome".into(),
        ],
        stop_if: vec![
            "Grounding evidence is stale or contradicted".into(),
            "Drive signal expired before execution".into(),
        ],
        priority: 8,
    })
}

/// P35-style goal proof for actions: each done_when item needs non-empty evidence.
/// Returns Ok(debts) where debts empty means fully mapped; Err never — use debts.
pub fn prove_done_when(done_when: &[String], evidence: &[String]) -> Vec<String> {
    let mut debts = Vec::new();
    for (i, item) in done_when.iter().enumerate() {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let mapped = evidence
            .get(i)
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        if !mapped {
            debts.push(format!(
                "done_when[{}] «{}» has no artifact evidence",
                i,
                item.chars().take(48).collect::<String>()
            ));
        }
    }
    debts
}

/// Build deterministic action proposals from search hits + skill matches.
/// Pure function — no I/O — so unit tests can pin behavior.
pub fn build_proposals(
    query: &str,
    evidence: &[(u64, String)],
    skills: &[(String, f64)],
    max_actions: usize,
) -> Vec<AgentActionDraft> {
    let max_actions = max_actions.clamp(1, 5);
    if evidence.is_empty() {
        return Vec::new();
    }

    let suggested: Vec<SuggestedSkill> = skills
        .iter()
        .take(5)
        .map(|(n, s)| SuggestedSkill {
            name: n.clone(),
            score: (*s * 1000.0).round() / 1000.0,
        })
        .collect();

    // One primary action covering top evidence, plus optional focused follow-ups
    // for later hits so agents can parallelize independent threads.
    let mut drafts = Vec::new();
    let top_ids: Vec<u64> = evidence.iter().take(5).map(|(id, _)| *id).collect();
    let preview: String = evidence
        .first()
        .map(|(_, c)| c.chars().take(80).collect())
        .unwrap_or_default();

    drafts.push(AgentActionDraft {
        summary: format!(
            "Act on memories for «{}»: {}",
            query.trim(),
            if preview.is_empty() {
                "review evidence and produce a concrete deliverable".into()
            } else {
                preview
            }
        ),
        evidence_memory_ids: top_ids,
        required_capabilities: suggested.iter().map(|s| s.name.clone()).collect(),
        suggested_skills: suggested.clone(),
        done_when: vec![
            format!("Outcome recorded for query «{}»", query.trim()),
            "Evidence memories cited in the outcome".into(),
        ],
        stop_if: vec![
            "Evidence contradicts the proposed direction".into(),
            "Required skill or tool is unavailable".into(),
        ],
        priority: 7,
    });

    for (id, content) in evidence.iter().skip(1).take(max_actions.saturating_sub(1)) {
        let snippet: String = content.chars().take(60).collect();
        drafts.push(AgentActionDraft {
            summary: format!("Follow up on memory #{}: {}", id, snippet),
            evidence_memory_ids: vec![*id],
            required_capabilities: suggested.iter().map(|s| s.name.clone()).collect(),
            suggested_skills: suggested.clone(),
            done_when: vec![format!(
                "Follow-up for memory #{} completed with artifact",
                id
            )],
            stop_if: vec!["Memory no longer relevant".into()],
            priority: 5,
        });
    }

    drafts.truncate(max_actions);
    drafts
}

#[derive(Debug, Clone)]
pub struct AgentActionDraft {
    pub summary: String,
    pub evidence_memory_ids: Vec<u64>,
    pub suggested_skills: Vec<SuggestedSkill>,
    pub done_when: Vec<String>,
    pub stop_if: Vec<String>,
    pub priority: u8,
    pub required_capabilities: Vec<String>,
}

impl AgentActionDraft {
    pub fn into_action(self, source_query: &str) -> AgentAction {
        let now = now_secs();
        AgentAction {
            id: 0,
            summary: self.summary,
            evidence_memory_ids: self.evidence_memory_ids,
            suggested_skills: self.suggested_skills,
            done_when: self.done_when,
            stop_if: self.stop_if,
            priority: self.priority,
            state: ActionState::Open,
            claimed_by: None,
            assignee: None,
            assigned_at: None,
            required_capabilities: self.required_capabilities,
            requeue_count: 0,
            last_failure_reason: None,
            last_responsible_assignee: None,
            escalated: false,
            outcome_memory_id: None,
            source_query: Some(source_query.to_string()),
            source_drive_id: None,
            bound_task_id: None,
            origin: "propose".into(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn into_drive_action(self, drive_id: u64, drive_desc: &str) -> AgentAction {
        let mut a = self.into_action(drive_desc);
        a.source_drive_id = Some(drive_id);
        a.origin = "drive".into();
        a.priority = a.priority.max(8);
        a
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn propose_requires_evidence() {
        assert!(build_proposals("q", &[], &[], 3).is_empty());
    }

    #[test]
    fn propose_binds_evidence_and_skills() {
        let drafts = build_proposals(
            "deploy checklist",
            &[
                (10, "prod deploy uses blue-green".into()),
                (11, "rollback runbook".into()),
            ],
            &[("ops-runbook".into(), 0.9)],
            2,
        );
        assert_eq!(drafts.len(), 2);
        assert!(drafts[0].evidence_memory_ids.contains(&10));
        assert_eq!(drafts[0].suggested_skills[0].name, "ops-runbook");
        assert!(!drafts[0].done_when.is_empty());
    }

    #[test]
    fn claim_is_single_flight() {
        let f = ActionFrontier::new();
        let drafts = build_proposals("q", &[(1, "e".into())], &[], 1);
        let a = f.enqueue(drafts[0].clone().into_action("q")).unwrap();
        assert!(f.claim(a.id, "agent-a").is_ok());
        let err = f.claim(a.id, "agent-b").unwrap_err();
        assert!(err.contains("expected open"));
    }

    #[test]
    fn complete_sets_outcome_and_terminal() {
        let f = ActionFrontier::new();
        let drafts = build_proposals("q", &[(1, "e".into())], &[], 1);
        let a = f.enqueue(drafts[0].clone().into_action("q")).unwrap();
        f.claim(a.id, "agent-a").unwrap();
        let done = f.complete(a.id, 99, ActionState::Done).unwrap();
        assert_eq!(done.state, ActionState::Done);
        assert_eq!(done.outcome_memory_id, Some(99));
        assert!(f.complete(a.id, 100, ActionState::Done).is_err());
    }

    #[test]
    fn drive_draft_requires_evidence() {
        assert!(build_drive_draft(1, "x", &[], &[]).is_none());
    }

    #[test]
    fn live_drive_blocks_double_spend() {
        let f = ActionFrontier::new();
        let draft = build_drive_draft(42, "check deploy", &[7, 8], &[]).unwrap();
        let a = f
            .enqueue(draft.into_drive_action(42, "check deploy"))
            .unwrap();
        assert_eq!(a.source_drive_id, Some(42));
        assert_eq!(a.origin, "drive");
        assert!(f.has_live_drive(42));
        // Completing frees the drive for a future materialization.
        f.claim(a.id, "w").unwrap();
        f.complete(a.id, 1, ActionState::Done).unwrap();
        assert!(!f.has_live_drive(42));
    }

    #[test]
    fn bind_task_rejects_unclaimed_and_foreign() {
        let f = ActionFrontier::new();
        let drafts = build_proposals("q", &[(1, "e".into())], &[], 1);
        let a = f.enqueue(drafts[0].clone().into_action("q")).unwrap();
        assert!(f
            .bind_task(a.id, "ts_1", "alice")
            .unwrap_err()
            .contains("must be claimed"));
        f.claim(a.id, "alice").unwrap();
        assert!(f
            .bind_task(a.id, "ts_1", "bob")
            .unwrap_err()
            .contains("claimed by"));
        let bound = f.bind_task(a.id, "ts_1", "alice").unwrap();
        assert_eq!(bound.bound_task_id.as_deref(), Some("ts_1"));
        assert_eq!(f.find_by_task("ts_1").unwrap().id, a.id);
    }

    #[test]
    fn prove_done_when_blocks_false_complete() {
        let debts = prove_done_when(
            &["Ship changelog".into(), "Smoke green".into()],
            &["CHANGELOG.md updated".into(), "   ".into()],
        );
        assert_eq!(debts.len(), 1);
        assert!(debts[0].contains("done_when[1]"));
        assert!(prove_done_when(&["A".into()], &["artifact".into()]).is_empty());
    }

    #[test]
    fn prune_keeps_live_drops_old_terminal() {
        let f = ActionFrontier::new();
        let drafts = build_proposals("q", &[(1, "e".into())], &[], 1);
        let a = f.enqueue(drafts[0].clone().into_action("q")).unwrap();
        f.claim(a.id, "w").unwrap();
        f.complete(a.id, 9, ActionState::Done).unwrap();
        // Force old updated_at
        {
            let mut map = f.actions.lock();
            map.get_mut(&a.id).unwrap().updated_at = 1;
        }
        assert_eq!(f.prune_terminal_older_than(10), 1);
        assert!(f.get(a.id).is_none());
    }

    #[test]
    fn action_card_has_stable_schema() {
        let drafts = build_proposals("q", &[(3, "e".into())], &[("s".into(), 0.5)], 1);
        let card = drafts[0].clone().into_action("q").to_action_card();
        assert_eq!(card["schema"], "epicode.action_card");
        assert_eq!(card["schema_version"], ACTION_CARD_SCHEMA_VERSION);
        assert!(card["lineage"]["parents"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!(3)));
        assert!(card.get("evidence_memory_ids").is_some());
        assert!(card.get("done_when").is_some());
        assert!(card.get("suggested_skills").is_some());
    }
}
