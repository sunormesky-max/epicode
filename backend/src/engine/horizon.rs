//! Horizon cadence for the central scheduler.
//!
//! A fixed tick is a metronome. Horizon treats the next beat as a prediction:
//! coast when the field is quiet, attend when something moved, commit a full
//! cognitive cycle only when pressure or deferred debt says the world changed.
//! Overlapping work is not dropped silently; it becomes debt that pulls the
//! next free beat toward commit.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HorizonPhase {
    Coast,
    Attend,
    Commit,
    Deferred,
}

impl HorizonPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            HorizonPhase::Coast => "coast",
            HorizonPhase::Attend => "attend",
            HorizonPhase::Commit => "commit",
            HorizonPhase::Deferred => "deferred",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HorizonPlan {
    pub phase: HorizonPhase,
    pub sleep_ms: u64,
    pub pressure: f64,
    pub debt: u32,
}

#[derive(Debug, Clone)]
pub struct Horizon {
    base_ms: u64,
    pressure: f64,
    debt: u32,
    phase: HorizonPhase,
}

impl Horizon {
    pub fn new(base_ms: u64) -> Self {
        Self {
            base_ms: base_ms.max(50),
            pressure: 0.0,
            debt: 0,
            phase: HorizonPhase::Coast,
        }
    }

    pub fn set_base_ms(&mut self, base_ms: u64) {
        self.base_ms = base_ms.max(50);
    }

    pub fn note_stimulus(&mut self, weight: f64) {
        self.pressure = (self.pressure + weight.clamp(0.0, 1.0)).min(1.0);
    }

    pub fn note_defer(&mut self) {
        self.debt = self.debt.saturating_add(1);
        self.pressure = (self.pressure + 0.15).min(1.0);
        self.phase = HorizonPhase::Deferred;
    }

    pub fn plan(&mut self, cognitive: bool) -> HorizonPlan {
        // debt与高压力同归Commit(合并同分支, clippy if_same_then_else)
        let commit = (self.debt > 0 && self.pressure >= 0.35) || self.pressure >= 0.55;
        let mut phase = if commit {
            HorizonPhase::Commit
        } else if self.pressure >= 0.18 {
            HorizonPhase::Attend
        } else {
            HorizonPhase::Coast
        };
        if !cognitive && phase == HorizonPhase::Commit {
            phase = HorizonPhase::Attend;
        }
        let stretch = match phase {
            HorizonPhase::Coast => 3,
            HorizonPhase::Attend | HorizonPhase::Commit | HorizonPhase::Deferred => 1,
        };
        let sleep_ms = self
            .base_ms
            .saturating_mul(stretch)
            .clamp(self.base_ms, self.base_ms.saturating_mul(4));
        if phase == HorizonPhase::Commit {
            self.debt = 0;
            self.pressure *= 0.35;
        } else {
            self.pressure *= 0.82;
        }
        self.phase = phase;
        HorizonPlan {
            phase,
            sleep_ms,
            pressure: self.pressure,
            debt: self.debt,
        }
    }

    pub fn phase(&self) -> HorizonPhase {
        self.phase
    }
    pub fn pressure(&self) -> f64 {
        self.pressure
    }
    pub fn debt(&self) -> u32 {
        self.debt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_field_coasts_and_stretches() {
        let mut horizon = Horizon::new(1000);
        let plan = horizon.plan(true);
        assert_eq!(plan.phase, HorizonPhase::Coast);
        assert_eq!(plan.sleep_ms, 3000);
    }

    #[test]
    fn stimulus_then_debt_commits_and_clears_debt() {
        let mut horizon = Horizon::new(1000);
        horizon.note_stimulus(0.4);
        horizon.note_defer();
        let plan = horizon.plan(true);
        assert_eq!(plan.phase, HorizonPhase::Commit);
        assert_eq!(plan.debt, 0);
    }

    #[test]
    fn quiet_mode_never_commits() {
        let mut horizon = Horizon::new(500);
        horizon.note_stimulus(1.0);
        let plan = horizon.plan(false);
        assert_eq!(plan.phase, HorizonPhase::Attend);
        assert_eq!(plan.sleep_ms, 500);
    }
}
