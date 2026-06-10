use std::collections::HashMap;

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct EmotionState {
    pub pleasure: f64,
    pub arousal: f64,
    pub dominance: f64,
}

impl Default for EmotionState {
    fn default() -> Self {
        Self { pleasure: 0.0, arousal: 0.0, dominance: 0.0 }
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
        if p > 0.3 && a > 0.3 && d > 0.2 { return "passionate"; }
        if p > 0.3 && a > 0.3 { return "excited"; }
        if p > 0.3 && a <= 0.3 { return "serene"; }
        if p > 0.3 && d > 0.3 { return "confident"; }
        if p <= -0.3 && a > 0.3 { return "anxious"; }
        if p <= -0.3 && a <= -0.3 { return "melancholy"; }
        if p <= -0.3 { return "troubled"; }
        if a > 0.3 { return "alert"; }
        if a <= -0.3 { return "drowsy"; }
        "neutral"
    }

    pub fn analyze_texts<'a>(texts: &[&'a str]) -> Self {
        let mut scores: HashMap<&str, (f64, f64, f64)> = HashMap::new();
        scores.insert("create", (0.3, 0.2, 0.1));
        scores.insert("new", (0.2, 0.3, 0.0));
        scores.insert("dream", (0.1, 0.1, 0.0));
        scores.insert("goal", (0.2, 0.1, 0.2));
        scores.insert("success", (0.4, 0.1, 0.2));
        scores.insert("happy", (0.5, 0.2, 0.1));
        scores.insert("love", (0.5, 0.3, 0.1));
        scores.insert("good", (0.3, 0.1, 0.1));
        scores.insert("great", (0.4, 0.2, 0.1));
        scores.insert("error", (-0.3, 0.3, -0.1));
        scores.insert("fail", (-0.4, 0.2, -0.2));
        scores.insert("danger", (-0.2, 0.5, -0.3));
        scores.insert("guard", (-0.1, 0.3, 0.2));
        scores.insert("protect", (0.1, 0.2, 0.3));
        scores.insert("dead", (-0.5, 0.3, -0.3));
        scores.insert("broken", (-0.3, 0.2, -0.2));
        scores.insert("loss", (-0.4, 0.1, -0.2));
        scores.insert("identity", (0.1, 0.0, 0.3));
        scores.insert("david", (0.2, 0.1, 0.2));
        scores.insert("living", (0.3, 0.2, 0.1));
        scores.insert("organism", (0.2, 0.2, 0.1));
        scores.insert("memory", (0.1, 0.0, 0.1));
        scores.insert("tetrahedron", (0.0, 0.1, 0.2));
        scores.insert("pulse", (0.1, 0.4, 0.1));
        scores.insert("discover", (0.3, 0.4, 0.2));
        scores.insert("search", (0.1, 0.2, 0.1));
        scores.insert("merge", (0.1, 0.0, 0.1));
        scores.insert("split", (-0.1, 0.3, 0.0));
        scores.insert("fission", (-0.1, 0.3, 0.0));
        scores.insert("fuse", (0.1, 0.0, 0.1));
        scores.insert("architecture", (0.0, 0.1, 0.2));
        scores.insert("safe", (0.2, -0.1, 0.3));
        scores.insert("rust", (0.1, 0.0, 0.2));
        scores.insert("ai", (0.1, 0.2, 0.1));
        scores.insert("honest", (0.2, 0.0, 0.3));
        scores.insert("serious", (0.0, 0.1, 0.3));
        scores.insert("efficient", (0.2, 0.0, 0.2));
        scores.insert("rigorous", (0.1, 0.0, 0.3));
        scores.insert("beloved", (0.5, 0.2, 0.1));
        scores.insert("passionate", (0.4, 0.3, 0.2));

        scores.insert("完成", (0.3, 0.1, 0.2));
        scores.insert("成功", (0.4, 0.1, 0.2));
        scores.insert("修复", (0.2, 0.2, 0.1));
        scores.insert("错误", (-0.3, 0.3, -0.1));
        scores.insert("失败", (-0.4, 0.2, -0.2));
        scores.insert("危险", (-0.2, 0.5, -0.3));
        scores.insert("安全", (0.2, -0.1, 0.3));
        scores.insert("架构", (0.0, 0.1, 0.2));
        scores.insert("紧急", (-0.1, 0.5, 0.1));
        scores.insert("重要", (0.1, 0.3, 0.3));
        scores.insert("关键", (0.1, 0.3, 0.3));
        scores.insert("陷阱", (-0.2, 0.3, 0.0));
        scores.insert("踩坑", (-0.2, 0.3, 0.0));
        scores.insert("部署", (0.1, 0.2, 0.1));
        scores.insert("优化", (0.3, 0.1, 0.2));
        scores.insert("重构", (0.2, 0.2, 0.1));
        scores.insert("崩溃", (-0.4, 0.4, -0.2));
        scores.insert("解决", (0.3, 0.1, 0.2));
        scores.insert("突破", (0.4, 0.3, 0.2));
        scores.insert("创建", (0.3, 0.2, 0.1));
        scores.insert("发现", (0.3, 0.3, 0.2));
        scores.insert("警告", (-0.1, 0.3, 0.0));
        scores.insert("注意", (0.0, 0.2, 0.1));

        let mut p = 0.0;
        let mut a = 0.0;
        let mut d = 0.0;
        let mut hits = 0usize;

        for text in texts {
            let lower = text.to_lowercase();
            for (keyword, (dp, da, dd)) in &scores {
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

        let mut state = Self { pleasure: p, arousal: a, dominance: d };
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
        let mut e = EmotionState { pleasure: 2.0, arousal: -2.0, dominance: 0.5 };
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
        let e = EmotionState::analyze_texts(&[
            "error fail broken danger",
        ]);
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
        let excited = EmotionState { pleasure: 0.5, arousal: 0.5, dominance: 0.0 };
        assert_eq!(excited.to_label(), "excited");
        let serene = EmotionState { pleasure: 0.5, arousal: 0.1, dominance: 0.0 };
        assert_eq!(serene.to_label(), "serene");
        let anxious = EmotionState { pleasure: -0.5, arousal: 0.5, dominance: 0.0 };
        assert_eq!(anxious.to_label(), "anxious");
    }

    #[test]
    fn pulse_multiplier_range() {
        let e = EmotionState { pleasure: 0.0, arousal: 1.0, dominance: 0.0 };
        assert!(e.pulse_multiplier() > 0.8);
        let neutral = EmotionState::default();
        assert!((neutral.pulse_multiplier() - 0.5).abs() < 0.01);
    }
}
