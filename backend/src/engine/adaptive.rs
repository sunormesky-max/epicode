use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Param {
    FissionEntropyThreshold,
    FissionMinClusterSize,
    MergeDistance,
    MergeLabelSimilarity,
    DreamInterval,
    EvictionMassThreshold,
    PulseBudget,
}

impl Param {
    pub fn all() -> &'static [Param] {
        &[
            Param::FissionEntropyThreshold,
            Param::FissionMinClusterSize,
            Param::MergeDistance,
            Param::MergeLabelSimilarity,
            Param::DreamInterval,
            Param::EvictionMassThreshold,
            Param::PulseBudget,
        ]
    }

    fn default_value(&self) -> f64 {
        match self {
            Param::FissionEntropyThreshold => 0.3,
            Param::FissionMinClusterSize => 6.0,
            Param::MergeDistance => 5.0,
            Param::MergeLabelSimilarity => 0.2,
            Param::DreamInterval => 50.0,
            Param::EvictionMassThreshold => 0.3,
            Param::PulseBudget => 3.0,
        }
    }

    fn min_value(&self) -> f64 {
        match self {
            Param::FissionEntropyThreshold => 0.1,
            Param::FissionMinClusterSize => 3.0,
            Param::MergeDistance => 2.0,
            Param::MergeLabelSimilarity => 0.05,
            Param::DreamInterval => 20.0,
            Param::EvictionMassThreshold => 0.1,
            Param::PulseBudget => 1.0,
        }
    }

    fn max_value(&self) -> f64 {
        match self {
            Param::FissionEntropyThreshold => 0.8,
            Param::FissionMinClusterSize => 15.0,
            Param::MergeDistance => 15.0,
            Param::MergeLabelSimilarity => 0.5,
            Param::DreamInterval => 200.0,
            Param::EvictionMassThreshold => 0.6,
            Param::PulseBudget => 6.0,
        }
    }

    fn learning_rate(&self) -> f64 {
        match self {
            Param::FissionEntropyThreshold => 0.02,
            Param::FissionMinClusterSize => 0.01,
            Param::MergeDistance => 0.03,
            Param::MergeLabelSimilarity => 0.02,
            Param::DreamInterval => 0.05,
            Param::EvictionMassThreshold => 0.01,
            Param::PulseBudget => 0.02,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Param::FissionEntropyThreshold => "fission_entropy",
            Param::FissionMinClusterSize => "fission_min_size",
            Param::MergeDistance => "merge_distance",
            Param::MergeLabelSimilarity => "merge_label_sim",
            Param::DreamInterval => "dream_interval",
            Param::EvictionMassThreshold => "evict_mass",
            Param::PulseBudget => "pulse_budget",
        }
    }
}

struct ParamState {
    value: f64,
    momentum: f64,
}

impl ParamState {
    fn new(param: Param) -> Self {
        Self {
            value: param.default_value(),
            momentum: 0.0,
        }
    }

    fn adapt(&mut self, param: Param, effectiveness: f64) {
        let lr = param.learning_rate();
        let gradient = (effectiveness - 0.5) * 2.0;
        self.momentum = self.momentum * 0.8 + gradient * lr;
        self.value += self.momentum;
        self.value = self.value.clamp(param.min_value(), param.max_value());
    }
}

pub struct AdaptiveParams {
    params: HashMap<Param, ParamState>,
}

impl AdaptiveParams {
    pub fn new() -> Self {
        let mut params = HashMap::new();
        for p in Param::all() {
            params.insert(*p, ParamState::new(*p));
        }
        Self { params }
    }

    pub fn get(&self, param: Param) -> f64 {
        self.params
            .get(&param)
            .map(|s| s.value)
            .unwrap_or_else(|| param.default_value())
    }

    pub fn get_u(&self, param: Param) -> usize {
        self.get(param).round() as usize
    }

    pub fn adapt(&mut self, param: Param, effectiveness: f64) {
        if let Some(state) = self.params.get_mut(&param) {
            state.adapt(param, effectiveness);
        }
    }

    pub fn adapt_from_outcome(&mut self, action: super::outcome::ActionType, effectiveness: f64) {
        use super::outcome::ActionType;
        match action {
            ActionType::Fission => {
                self.adapt(Param::FissionEntropyThreshold, effectiveness);
                self.adapt(Param::FissionMinClusterSize, effectiveness * 0.5);
            }
            ActionType::Merge => {
                self.adapt(Param::MergeDistance, effectiveness);
                self.adapt(Param::MergeLabelSimilarity, effectiveness * 0.5);
            }
            ActionType::Dream => {
                self.adapt(Param::DreamInterval, effectiveness);
            }
            ActionType::Evict => {
                self.adapt(Param::EvictionMassThreshold, effectiveness);
            }
            ActionType::Pulse => {
                self.adapt(Param::PulseBudget, effectiveness);
            }
            ActionType::Link => {}
        }
    }
}
