use std::collections::HashMap;

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct EmotionState {
    pub pleasure: f64,
    pub arousal: f64,
    pub dominance: f64,
}

impl Default for EmotionState {
    fn default() -> Self {
        Self {
            pleasure: 0.0,
            arousal: 0.0,
            dominance: 0.0,
        }
    }
}

impl EmotionState {
    pub fn clamp(&mut self) {
        self.pleasure = self.pleasure.clamp(-1.0, 1.0);
        self.arousal = self.arousal.clamp(-1.0, 1.0);
        self.dominance = self.dominance.clamp(-1.0, 1.0);
    }

    pub fn decay(&mut self, rate: f64) {
        self.pleasure *= 1.0 - rate;
        self.arousal *= 1.0 - rate;
        self.dominance *= 1.0 - rate;
    }

    pub fn affect(&mut self, pleasure_delta: f64, arousal_delta: f64, dominance_delta: f64) {
        self.pleasure += pleasure_delta;
        self.arousal += arousal_delta;
        self.dominance += dominance_delta;
        self.clamp();
    }

    pub fn pulse_multiplier(&self) -> f64 {
        0.5 + (self.arousal.abs() * 0.5)
    }

    pub fn dream_intensity(&self) -> f64 {
        (1.0 - self.pleasure) * 0.5 + 0.3
    }

    pub fn attraction_bias(&self) -> f64 {
        self.pleasure * 0.3
    }

    pub fn quadrant(&self) -> &'static str {
        match (self.pleasure >= 0.0, self.arousal >= 0.0) {
            (true, true) => "excited",
            (true, false) => "calm",
            (false, true) => "anxious",
            (false, false) => "melancholy",
        }
    }

    pub fn to_label(&self) -> &'static str {
        let p = self.pleasure;
        let a = self.arousal;
        let d = self.dominance;
        if p > 0.3 && a > 0.3 && d > 0.2 {
            return "passionate";
        }
        if p > 0.3 && a > 0.3 {
            return "excited";
        }
        if p > 0.3 && a <= 0.3 {
            return "serene";
        }
        if p > 0.3 && d > 0.3 {
            return "confident";
        }
        if p <= -0.3 && a > 0.3 {
            return "anxious";
        }
        if p <= -0.3 && a <= -0.3 {
            return "melancholy";
        }
        if p <= -0.3 {
            return "troubled";
        }
        if a > 0.3 {
            return "alert";
        }
        if a <= -0.3 {
            return "drowsy";
        }
        "neutral"
    }

    // 阶段4修复:const 数组替代每次调用重建 HashMap(80条线性扫描比HashMap快且无分配)
    const EMOTION_SCORES: &[(&str, f64, f64, f64)] = &[
        ("create", 0.3, 0.2, 0.1),
        ("new", 0.2, 0.3, 0.0),
        ("dream", 0.1, 0.1, 0.0),
        ("goal", 0.2, 0.1, 0.2),
        ("success", 0.4, 0.1, 0.2),
        ("happy", 0.5, 0.2, 0.1),
        ("love", 0.5, 0.3, 0.1),
        ("good", 0.3, 0.1, 0.1),
        ("great", 0.4, 0.2, 0.1),
        ("error", -0.3, 0.3, -0.1),
        ("fail", -0.4, 0.2, -0.2),
        ("danger", -0.2, 0.5, -0.3),
        ("guard", -0.1, 0.3, 0.2),
        ("protect", 0.1, 0.2, 0.3),
        ("dead", -0.5, 0.3, -0.3),
        ("broken", -0.3, 0.2, -0.2),
        ("loss", -0.4, 0.1, -0.2),
        ("identity", 0.1, 0.0, 0.3),
        ("david", 0.2, 0.1, 0.2),
        ("living", 0.3, 0.2, 0.1),
        ("organism", 0.2, 0.2, 0.1),
        ("memory", 0.1, 0.0, 0.1),
        ("tetrahedron", 0.0, 0.1, 0.2),
        ("pulse", 0.1, 0.4, 0.1),
        ("discover", 0.3, 0.4, 0.2),
        ("search", 0.1, 0.2, 0.1),
        ("merge", 0.1, 0.0, 0.1),
        ("split", -0.1, 0.3, 0.0),
        ("fission", -0.1, 0.3, 0.0),
        ("fuse", 0.1, 0.0, 0.1),
        ("architecture", 0.0, 0.1, 0.2),
        ("safe", 0.2, -0.1, 0.3),
        ("rust", 0.1, 0.0, 0.2),
        ("ai", 0.1, 0.2, 0.1),
        ("honest", 0.2, 0.0, 0.3),
        ("serious", 0.0, 0.1, 0.3),
        ("efficient", 0.2, 0.0, 0.2),
        ("rigorous", 0.1, 0.0, 0.3),
        ("beloved", 0.5, 0.2, 0.1),
        ("passionate", 0.4, 0.3, 0.2),
        ("完成", 0.3, 0.1, 0.2),
        ("成功", 0.4, 0.1, 0.2),
        ("修复", 0.2, 0.2, 0.1),
        ("错误", -0.3, 0.3, -0.1),
        ("失败", -0.4, 0.2, -0.2),
        ("危险", -0.2, 0.5, -0.3),
        ("安全", 0.2, -0.1, 0.3),
        ("架构", 0.0, 0.1, 0.2),
        ("紧急", -0.1, 0.5, 0.1),
        ("重要", 0.1, 0.3, 0.3),
        ("关键", 0.1, 0.3, 0.3),
        ("陷阱", -0.2, 0.3, 0.0),
        ("踩坑", -0.2, 0.3, 0.0),
        ("部署", 0.1, 0.2, 0.1),
        ("优化", 0.3, 0.1, 0.2),
        ("重构", 0.2, 0.2, 0.1),
        ("崩溃", -0.4, 0.4, -0.2),
        ("解决", 0.3, 0.1, 0.2),
        ("突破", 0.4, 0.3, 0.2),
        ("创建", 0.3, 0.2, 0.1),
        ("发现", 0.3, 0.3, 0.2),
        ("警告", -0.1, 0.3, 0.0),
        ("注意", 0.0, 0.2, 0.1),
    ];

    pub fn analyze_texts(texts: &[&str]) -> Self {
        let mut p = 0.0;
        let mut a = 0.0;
        let mut d = 0.0;
        let mut hits = 0usize;

        for text in texts {
            let lower = text.to_lowercase();
            for &(keyword, dp, da, dd) in Self::EMOTION_SCORES {
                if lower.contains(keyword) {
                    p += dp;
                    a += da;
                    d += dd;
                    hits += 1;
                }
            }
        }

        if hits > 0 {
            let scale = 0.3 / (1.0 + hits as f64 * 0.15);
            p *= scale;
            a *= scale;
            d *= scale;
        }

        let mut state = Self {
            pleasure: p,
            arousal: a,
            dominance: d,
        };
        state.clamp();
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_neutral() {
        let e = EmotionState::default();
        assert_eq!(e.pleasure, 0.0);
        assert_eq!(e.arousal, 0.0);
    }

    #[test]
    fn clamp_bounds() {
        let mut e = EmotionState {
            pleasure: 2.0,
            arousal: -2.0,
            dominance: 0.5,
        };
        e.clamp();
        assert!(e.pleasure <= 1.0);
        assert!(e.arousal >= -1.0);
    }

    #[test]
    fn analyze_positive_texts() {
        let e = EmotionState::analyze_texts(&[
            "David goal is to become a living organism",
            "success great good happy",
        ]);
        assert!(e.pleasure > 0.0);
        assert!(e.dominance > 0.0);
    }

    #[test]
    fn analyze_negative_texts() {
        let e = EmotionState::analyze_texts(&["error fail broken danger"]);
        assert!(e.pleasure < 0.0);
        assert!(e.arousal > 0.0);
    }

    #[test]
    fn analyze_identity_texts() {
        let e = EmotionState::analyze_texts(&[
            "David is honest serious efficient rigorous",
            "David name meaning beloved",
        ]);
        assert!(e.pleasure > 0.0);
        assert!(e.dominance > 0.0);
    }

    #[test]
    fn to_label_variety() {
        let excited = EmotionState {
            pleasure: 0.5,
            arousal: 0.5,
            dominance: 0.0,
        };
        assert_eq!(excited.to_label(), "excited");
        let serene = EmotionState {
            pleasure: 0.5,
            arousal: 0.1,
            dominance: 0.0,
        };
        assert_eq!(serene.to_label(), "serene");
        let anxious = EmotionState {
            pleasure: -0.5,
            arousal: 0.5,
            dominance: 0.0,
        };
        assert_eq!(anxious.to_label(), "anxious");
    }

    #[test]
    fn pulse_multiplier_range() {
        let e = EmotionState {
            pleasure: 0.0,
            arousal: 1.0,
            dominance: 0.0,
        };
        assert!(e.pulse_multiplier() > 0.8);
        let neutral = EmotionState::default();
        assert!((neutral.pulse_multiplier() - 0.5).abs() < 0.01);
    }
}
