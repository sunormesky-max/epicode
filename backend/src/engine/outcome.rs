use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionType {
    Pulse,
    Fission,
    Merge,
    Dream,
    Link,
    Evict,
}

impl ActionType {}

#[derive(Debug, Clone)]
pub struct ActionOutcome {
    pub action: ActionType,
    pub pre_entropy: f64,
    pub post_entropy: f64,
    pub pre_cluster_count: usize,
    pub post_cluster_count: usize,
    pub pre_tetra_count: usize,
    pub post_tetra_count: usize,
    pub pre_energy: f64,
    pub post_energy: f64,
    pub effectiveness: f64,
    pub tick: u64,
}

impl ActionOutcome {
    pub fn compute_effectiveness(&mut self) {
        let entropy_delta = self.pre_entropy - self.post_entropy;
        let energy_cost = (self.pre_energy - self.post_energy).max(0.01);
        self.effectiveness = match self.action {
            ActionType::Fission => {
                if self.pre_entropy > 0.3 {
                    entropy_delta / energy_cost.sqrt()
                } else {
                    0.0
                }
            }
            ActionType::Dream => {
                let cluster_improvement = if self.pre_cluster_count > 0 {
                    (self.post_cluster_count as f64 - self.pre_cluster_count as f64).abs()
                        / self.pre_cluster_count.max(1) as f64
                } else {
                    0.0
                };
                entropy_delta * 0.6 + cluster_improvement * 0.4
            }
            ActionType::Pulse => entropy_delta.max(0.0) * 0.5,
            ActionType::Merge => {
                let similarity_preserved = self.post_entropy <= self.pre_entropy * 1.1;
                if similarity_preserved {
                    0.7
                } else {
                    0.2
                }
            }
            ActionType::Evict => {
                if self.post_tetra_count < self.pre_tetra_count {
                    0.6
                } else {
                    0.1
                }
            }
            ActionType::Link => 0.5,
        };
        self.effectiveness = self.effectiveness.clamp(-1.0, 1.0);
    }
}

pub struct OutcomeTracker {
    history: VecDeque<ActionOutcome>,
    avg_effectiveness: std::collections::HashMap<ActionType, f64>,
}

impl Default for OutcomeTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl OutcomeTracker {
    pub fn new() -> Self {
        let mut avg_effectiveness = std::collections::HashMap::new();
        for a in [
            ActionType::Pulse,
            ActionType::Fission,
            ActionType::Merge,
            ActionType::Dream,
            ActionType::Link,
            ActionType::Evict,
        ] {
            avg_effectiveness.insert(a, 0.5);
        }
        Self {
            history: VecDeque::new(),
            avg_effectiveness,
        }
    }

    pub fn record(&mut self, mut outcome: ActionOutcome) {
        outcome.compute_effectiveness();
        if let Some(avg) = self.avg_effectiveness.get_mut(&outcome.action) {
            *avg = *avg * 0.85 + outcome.effectiveness * 0.15;
        }
        self.history.push_back(outcome);
        while self.history.len() > 100 {
            self.history.pop_front();
        }
    }
}
