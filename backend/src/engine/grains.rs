//! Grain ledger for the memory facility.
//!
//! An experience is append-only. A later correction is a new assertion that
//! points at the old one; it does not rewrite the experience. Grants and
//! stimuli are their own grains. Embeddings are projection debt, not memory.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StimulusKind {
    Experience,
    Correction,
    GrantChanged,
    Debt,
}

impl StimulusKind {
    pub fn as_str(self) -> &'static str {
        match self {
            StimulusKind::Experience => "experience",
            StimulusKind::Correction => "correction",
            StimulusKind::GrantChanged => "grant_changed",
            StimulusKind::Debt => "debt",
        }
    }
    fn weight(self) -> f64 {
        match self {
            StimulusKind::Experience => 0.35,
            StimulusKind::Correction | StimulusKind::GrantChanged => 0.8,
            StimulusKind::Debt => 0.15,
        }
    }
    fn deferrable(self) -> bool {
        matches!(self, StimulusKind::Experience)
    }
}

#[derive(Debug, Clone)]
pub struct Experience {
    pub id: u64,
    pub subject: String,
    pub observed_at: i64,
    pub valid_from: i64,
    pub valid_to: Option<i64>,
    pub source: String,
    pub payload: String,
    pub content_hash: u64,
    pub actor: String,
}

#[derive(Debug, Clone)]
pub struct Assertion {
    pub id: u64,
    pub experience_id: u64,
    pub subject: String,
    pub text: String,
    pub valid_from: i64,
    pub valid_to: Option<i64>,
    pub supersedes: Option<u64>,
    pub superseded_by: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Grant {
    pub id: u64,
    pub subject: String,
    pub scope: String,
    pub action: String,
    pub allow: bool,
    pub actor: String,
    pub at: i64,
}

#[derive(Debug, Clone)]
pub struct Stimulus {
    pub id: u64,
    pub subject: String,
    pub kind: StimulusKind,
    pub weight: f64,
    pub deferrable: bool,
    pub at: i64,
}

#[derive(Debug, Clone)]
pub struct ProjectionDebt {
    pub experience_id: u64,
    pub kind: String,
    pub model_version: String,
}

#[derive(Debug, Clone)]
pub struct RecallHit {
    pub assertion_id: u64,
    pub experience_id: u64,
    pub text: String,
    pub valid_from: i64,
    pub valid_to: Option<i64>,
    pub source: String,
}

#[derive(Debug, Default)]
pub struct GrainLedger {
    next: u64,
    experiences: HashMap<u64, Experience>,
    assertions: HashMap<u64, Assertion>,
    grants: Vec<Grant>,
    stimuli: Vec<Stimulus>,
    debts: Vec<ProjectionDebt>,
}

impl GrainLedger {
    pub fn append_experience(
        &mut self,
        subject: &str,
        payload: &str,
        source: &str,
        actor: &str,
        observed_at: i64,
        valid_from: i64,
    ) -> (u64, u64) {
        let experience_id = self.alloc();
        let assertion_id = self.alloc();
        self.experiences.insert(
            experience_id,
            Experience {
                id: experience_id,
                subject: subject.to_string(),
                observed_at,
                valid_from,
                valid_to: None,
                source: source.to_string(),
                payload: payload.to_string(),
                content_hash: hash_payload(payload),
                actor: actor.to_string(),
            },
        );
        self.assertions.insert(
            assertion_id,
            Assertion {
                id: assertion_id,
                experience_id,
                subject: subject.to_string(),
                text: payload.to_string(),
                valid_from,
                valid_to: None,
                supersedes: None,
                superseded_by: None,
            },
        );
        self.debts.push(ProjectionDebt {
            experience_id,
            kind: "embedding".into(),
            model_version: "unbound".into(),
        });
        self.push_stimulus(subject, StimulusKind::Experience, observed_at);
        (experience_id, assertion_id)
    }

    pub fn correct(
        &mut self,
        subject: &str,
        assertion_id: u64,
        text: &str,
        actor: &str,
        observed_at: i64,
        valid_from: i64,
    ) -> Result<u64, String> {
        let previous = self
            .assertions
            .get(&assertion_id)
            .cloned()
            .ok_or_else(|| "assertion not found".to_string())?;
        if previous.subject != subject {
            return Err("subject mismatch".into());
        }
        if !self.allows(subject, actor, "write", &previous.text) {
            return Err("grant denied".into());
        }
        let experience_id = self.alloc();
        let next_id = self.alloc();
        self.experiences.insert(
            experience_id,
            Experience {
                id: experience_id,
                subject: subject.to_string(),
                observed_at,
                valid_from,
                valid_to: None,
                source: format!("correction:{assertion_id}"),
                payload: text.to_string(),
                content_hash: hash_payload(text),
                actor: actor.to_string(),
            },
        );
        self.assertions.insert(
            next_id,
            Assertion {
                id: next_id,
                experience_id,
                subject: subject.to_string(),
                text: text.to_string(),
                valid_from,
                valid_to: None,
                supersedes: Some(assertion_id),
                superseded_by: None,
            },
        );
        if let Some(old) = self.assertions.get_mut(&assertion_id) {
            old.superseded_by = Some(next_id);
            if old.valid_to.is_none() {
                old.valid_to = Some(valid_from);
            }
        }
        self.push_stimulus(subject, StimulusKind::Correction, observed_at);
        Ok(next_id)
    }

    pub fn grant(
        &mut self,
        subject: &str,
        scope: &str,
        action: &str,
        allow: bool,
        actor: &str,
        at: i64,
    ) -> u64 {
        let id = self.alloc();
        self.grants.push(Grant {
            id,
            subject: subject.to_string(),
            scope: scope.to_string(),
            action: action.to_string(),
            allow,
            actor: actor.to_string(),
            at,
        });
        self.push_stimulus(subject, StimulusKind::GrantChanged, at);
        id
    }

    /// No grant grain means the subject still has its local default.
    /// Once any grant exists, only an explicit allow matches. An empty scope
    /// allow-list therefore denies, instead of falling back to the default.
    pub fn allows(&self, subject: &str, _actor: &str, action: &str, text: &str) -> bool {
        let relevant: Vec<&Grant> = self
            .grants
            .iter()
            .filter(|g| g.subject == subject)
            .collect();
        if relevant.is_empty() {
            return true;
        }
        relevant.iter().any(|g| {
            g.allow
                && (g.action == "*" || g.action == action)
                && (g.scope == "*" || text.contains(&g.scope) || g.scope == subject)
        })
    }

    pub fn recall(
        &self,
        subject: &str,
        actor: &str,
        valid_at: i64,
    ) -> Result<Vec<RecallHit>, String> {
        if !self.grants.iter().any(|g| g.subject == subject) {
            // default allow
        } else if !self.allows(subject, actor, "read", "*")
            && !self.allows(subject, actor, "read", subject)
        {
            let any_read = self
                .assertions
                .values()
                .any(|a| a.subject == subject && self.allows(subject, actor, "read", &a.text));
            if !any_read && !self.allows(subject, actor, "read", "*") {
                return Err("grant denied".into());
            }
        }
        let mut hits: Vec<RecallHit> = self
            .assertions
            .values()
            .filter(|a| a.subject == subject)
            .filter(|a| a.valid_from <= valid_at && a.valid_to.unwrap_or(i64::MAX) > valid_at)
            .filter(|a| self.allows(subject, actor, "read", &a.text))
            .map(|a| RecallHit {
                assertion_id: a.id,
                experience_id: a.experience_id,
                text: a.text.clone(),
                valid_from: a.valid_from,
                valid_to: a.valid_to,
                source: self
                    .experiences
                    .get(&a.experience_id)
                    .map(|e| e.source.clone())
                    .unwrap_or_default(),
            })
            .collect();
        hits.sort_by_key(|h| h.assertion_id);
        Ok(hits)
    }

    pub fn note_defer(&mut self, subject: &str, at: i64) {
        self.push_stimulus(subject, StimulusKind::Debt, at);
    }

    pub fn pressure(&self) -> f64 {
        self.stimuli.iter().map(|s| s.weight).sum::<f64>().min(1.0)
    }

    pub fn projection_debt(&self) -> usize {
        self.debts.len()
    }

    pub fn mark_projected(&mut self, experience_id: u64, kind: &str) {
        self.debts
            .retain(|d| !(d.experience_id == experience_id && d.kind == kind));
    }

    pub fn experience(&self, id: u64) -> Option<&Experience> {
        self.experiences.get(&id)
    }

    fn alloc(&mut self) -> u64 {
        self.next += 1;
        self.next
    }

    fn push_stimulus(&mut self, subject: &str, kind: StimulusKind, at: i64) {
        let id = self.alloc();
        self.stimuli.push(Stimulus {
            id,
            subject: subject.to_string(),
            kind,
            weight: kind.weight(),
            deferrable: kind.deferrable(),
            at,
        });
    }
}

fn hash_payload(payload: &str) -> u64 {
    payload.bytes().fold(0xcbf29ce484222325u64, |acc, b| {
        acc.wrapping_mul(0x100000001b3) ^ b as u64
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correction_does_not_rewrite_experience_and_splits_by_time() {
        let mut ledger = GrainLedger::default();
        let (experience_id, assertion_id) =
            ledger.append_experience("owner", "deadline is Friday", "user", "owner", 10, 10);
        let original = ledger.experience(experience_id).unwrap().payload.clone();
        let next = ledger
            .correct(
                "owner",
                assertion_id,
                "deadline is next month",
                "owner",
                20,
                20,
            )
            .unwrap();
        assert_eq!(ledger.experience(experience_id).unwrap().payload, original);
        assert!(ledger.experience(next_experience(&ledger, next)).is_some());
        let now = ledger.recall("owner", "owner", 25).unwrap();
        assert_eq!(now.len(), 1);
        assert_eq!(now[0].text, "deadline is next month");
        let then = ledger.recall("owner", "owner", 15).unwrap();
        assert_eq!(then.len(), 1);
        assert_eq!(then[0].text, "deadline is Friday");
    }

    #[test]
    fn explicit_grant_list_does_not_fall_back() {
        let mut ledger = GrainLedger::default();
        ledger.append_experience("owner", "secret plan", "user", "owner", 1, 1);
        ledger.grant("owner", "public", "read", true, "owner", 2);
        let hits = ledger.recall("owner", "reader", 3).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn debt_stimulus_raises_pressure_and_projection_is_not_memory() {
        let mut ledger = GrainLedger::default();
        let (experience_id, _) = ledger.append_experience("owner", "a", "user", "owner", 1, 1);
        assert_eq!(ledger.projection_debt(), 1);
        let before = ledger.pressure();
        ledger.note_defer("owner", 2);
        assert!(ledger.pressure() > before);
        ledger.mark_projected(experience_id, "embedding");
        assert_eq!(ledger.projection_debt(), 0);
        assert!(ledger.experience(experience_id).is_some());
    }

    fn next_experience(ledger: &GrainLedger, assertion_id: u64) -> u64 {
        ledger.assertions.get(&assertion_id).unwrap().experience_id
    }
}
