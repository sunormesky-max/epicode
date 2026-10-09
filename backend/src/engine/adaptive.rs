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

/// D1发现的缺陷修复: Mem0 调和阈值类别感知 — 对话类记忆(同会话轮次天然高相似)
/// 曾用统一0.92, LongMemEval试点628写入仅存86(87%被连环supersede且不进检索)
pub const MEM0_SUPERSEDE_KNOWLEDGE: f64 = 0.92;
pub const MEM0_SUPERSEDE_DIALOGUE: f64 = 0.985; // 对话类: 仅近乎完全重复才替换
/// F7单一事实源: 提示词与代码共用同一份参数默认值, 手抄即bug(50vs10谎言教训)
pub const DEFAULT_FISSION_ENTROPY: f64 = 0.3;
pub const DEFAULT_FISSION_MIN_SIZE: f64 = 6.0;
pub const FISSION_LLM_COOLDOWN_TICKS: u64 = 10;

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
            Param::FissionEntropyThreshold => DEFAULT_FISSION_ENTROPY,
            Param::FissionMinClusterSize => DEFAULT_FISSION_MIN_SIZE,
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

impl Default for AdaptiveParams {
    fn default() -> Self {
        Self::new()
    }
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

    /// 持久化快照:`{label: [value, momentum]}`。学到的阈值以前只在内存里,
    /// 每次重启都回到默认值,自适应等于每次从零开始。
    pub fn to_json(&self) -> serde_json::Value {
        let mut out = serde_json::Map::new();
        for p in Param::all() {
            let (v, m) = self
                .params
                .get(p)
                .map(|s| (s.value, s.momentum))
                .unwrap_or((p.default_value(), 0.0));
            out.insert(p.label().into(), serde_json::json!([v, m]));
        }
        serde_json::Value::Object(out)
    }

    /// 从快照恢复;未知键忽略,非有限值回落默认,数值夹到参数的 [min, max]。
    /// 返回实际恢复的参数个数。
    pub fn restore_json(&mut self, snapshot: &serde_json::Value) -> usize {
        let mut restored = 0;
        for p in Param::all() {
            let Some(pair) = snapshot.get(p.label()).and_then(|v| v.as_array()) else {
                continue;
            };
            let value = pair.first().and_then(|v| v.as_f64());
            let momentum = pair.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0);
            let Some(value) = value.filter(|v| v.is_finite()) else {
                continue;
            };
            let momentum = if momentum.is_finite() { momentum } else { 0.0 };
            // 动量最多一步学习率的若干倍,防止坏快照把参数一步推到边界
            let lr = p.learning_rate();
            let state = self.params.entry(*p).or_insert_with(|| ParamState::new(*p));
            state.value = value.clamp(p.min_value(), p.max_value());
            state.momentum = momentum.clamp(-5.0 * lr, 5.0 * lr);
            restored += 1;
        }
        restored
    }

    /// 观测用:每个参数的当前值、默认值与边界。
    pub fn describe(&self) -> serde_json::Value {
        let mut out = serde_json::Map::new();
        for p in Param::all() {
            out.insert(
                p.label().into(),
                serde_json::json!({
                    "value": self.get(*p),
                    "default": p.default_value(),
                    "min": p.min_value(),
                    "max": p.max_value(),
                }),
            );
        }
        serde_json::Value::Object(out)
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

#[cfg(test)]
mod persistence_tests {
    use super::*;
    use crate::engine::outcome::ActionType;

    #[test]
    fn learned_values_round_trip_through_json() {
        let mut learned = AdaptiveParams::new();
        for _ in 0..40 {
            learned.adapt_from_outcome(ActionType::Dream, 1.0);
            learned.adapt_from_outcome(ActionType::Evict, 0.0);
        }
        assert!(learned.get(Param::DreamInterval) > 50.0);
        assert!(learned.get(Param::EvictionMassThreshold) < 0.3);
        let snapshot = learned.to_json();
        let mut fresh = AdaptiveParams::new();
        assert_eq!(fresh.restore_json(&snapshot), Param::all().len());
        for p in Param::all() {
            assert_eq!(fresh.get(*p), learned.get(*p), "{}", p.label());
        }
        // momentum also survives: one more identical step lands on the same value
        learned.adapt_from_outcome(ActionType::Dream, 1.0);
        fresh.adapt_from_outcome(ActionType::Dream, 1.0);
        assert_eq!(
            fresh.get(Param::DreamInterval),
            learned.get(Param::DreamInterval)
        );
    }

    #[test]
    fn corrupt_or_out_of_range_snapshots_are_clamped_or_ignored() {
        let mut p = AdaptiveParams::new();
        let n = p.restore_json(&serde_json::json!({
            "dream_interval": [10_000.0, 99.0],
            "evict_mass": ["x", 0.0],
            "pulse_budget": [],
            "merge_distance": [3.0],
            "unknown_param": [1.0, 0.0],
        }));
        assert_eq!(n, 2);
        assert_eq!(p.get(Param::DreamInterval), 200.0);
        assert_eq!(p.get(Param::EvictionMassThreshold), 0.3);
        assert_eq!(p.get(Param::MergeDistance), 3.0);
        assert_eq!(p.restore_json(&serde_json::json!("garbage")), 0);
        assert_eq!(
            p.restore_json(&serde_json::json!({"dream_interval": [f64::MAX, 0.0]})),
            1
        );
        assert_eq!(p.get(Param::DreamInterval), 200.0);
    }
}
