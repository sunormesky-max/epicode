use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Drive {
    Curiosity,
    Coherence,
    Efficiency,
    Vitality,
}

impl Drive {
    pub fn all() -> &'static [Drive] {
        &[
            Drive::Curiosity,
            Drive::Coherence,
            Drive::Efficiency,
            Drive::Vitality,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Drive::Curiosity => "curiosity",
            Drive::Coherence => "coherence",
            Drive::Efficiency => "efficiency",
            Drive::Vitality => "vitality",
        }
    }
}

#[derive(Debug, Clone)]
struct DriveState {
    value: f64,
    baseline: f64,
    sensitivity: f64,
    cumulative_reward: f64,
}

impl DriveState {
    fn new(baseline: f64, sensitivity: f64) -> Self {
        Self {
            value: baseline,
            baseline,
            sensitivity,
            cumulative_reward: 0.0,
        }
    }

    fn update(&mut self, signal: f64) {
        self.value = self.baseline + self.sensitivity * signal;
        self.value = self.value.clamp(0.0, 1.0);
    }

    fn reward(&mut self, r: f64) {
        self.cumulative_reward = self.cumulative_reward * 0.9 + r * 0.1;
        self.baseline = (self.baseline + r * 0.01).clamp(0.1, 0.9);
    }

    fn urgency(&self) -> f64 {
        (self.value - 0.5).abs() * 2.0
    }
}

pub struct DriveEngine {
    drives: HashMap<Drive, DriveState>,
}

impl Default for DriveEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl DriveEngine {
    pub fn new() -> Self {
        let mut drives = HashMap::new();
        drives.insert(Drive::Curiosity, DriveState::new(0.5, 0.3));
        drives.insert(Drive::Coherence, DriveState::new(0.5, 0.4));
        drives.insert(Drive::Efficiency, DriveState::new(0.5, 0.25));
        drives.insert(Drive::Vitality, DriveState::new(0.5, 0.2));
        Self { drives }
    }

    pub fn observe(
        &mut self,
        tetra_count: usize,
        cluster_count: usize,
        avg_entropy: f64,
        energy_ratio: f64,
        unexplored_ratio: f64,
        redundancy_ratio: f64,
    ) {
        if let Some(cur) = self.drives.get_mut(&Drive::Curiosity) {
            cur.update(unexplored_ratio);
        }
        if let Some(coh) = self.drives.get_mut(&Drive::Coherence) {
            let signal = if cluster_count > 0 { avg_entropy } else { 0.5 };
            coh.update(signal);
        }
        if let Some(eff) = self.drives.get_mut(&Drive::Efficiency) {
            eff.update(redundancy_ratio);
        }
        if let Some(vit) = self.drives.get_mut(&Drive::Vitality) {
            vit.update(1.0 - energy_ratio);
        }

        let _ = (tetra_count, cluster_count);
    }

    pub fn reward(&mut self, drive: Drive, amount: f64) {
        if let Some(ds) = self.drives.get_mut(&drive) {
            ds.reward(amount);
        }
    }

    pub fn value(&self, drive: Drive) -> f64 {
        self.drives.get(&drive).map(|d| d.value).unwrap_or(0.5)
    }

    pub fn urgency(&self, drive: Drive) -> f64 {
        self.drives.get(&drive).map(|d| d.urgency()).unwrap_or(0.0)
    }

    pub fn dominant(&self) -> Drive {
        Drive::all()
            .iter()
            .max_by(|a, b| {
                self.urgency(**a)
                    .partial_cmp(&self.urgency(**b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .copied()
            .unwrap_or(Drive::Curiosity)
    }

    pub fn should_pulse(&self) -> bool {
        self.value(Drive::Curiosity) > 0.55
    }

    pub fn should_fission(&self) -> bool {
        self.value(Drive::Coherence) > 0.6
    }

    pub fn should_dream(&self) -> bool {
        self.value(Drive::Coherence) > 0.7 || self.value(Drive::Efficiency) > 0.6
    }

    pub fn should_evict(&self) -> bool {
        self.value(Drive::Efficiency) > 0.65
    }
}
