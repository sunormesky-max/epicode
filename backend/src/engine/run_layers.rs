//! Layered run coordination for the central scheduler.
//!
//! 中央调度器的运行分层(从快到慢)。每层有自己的单飞约束与计数:
//!
//! | 层 | 内容 | 单飞 |
//! |---|---|---|
//! | `Reflex` | 事件总线反应(创建/删除/关机) | 无需,O(1) |
//! | `Routine` | tick:任务队列、预测误差、驱动、裂变、保存 | `cycle_gate` |
//! | `Deliberative` | 调度器发起的模型调用(decide + 别名/重分类/实体) | `deliberative_gate` |
//! | `Consolidation` | dream 周期 | `dream_gate` |
//!
//! 耗时为包含式:Routine 的耗时包含其中嵌套触发的 Consolidation / 认知钩子。
//!
//! 关键约束:慢层不得占用快层的门。以前 `decide`(一次网络往返,数秒到数十秒)
//! 在持有 `cycle_gate` 时执行,期间所有 routine tick 都被推迟。

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunLayer {
    Reflex,
    Routine,
    Deliberative,
    Consolidation,
}

impl RunLayer {
    pub const ALL: [RunLayer; 4] = [
        RunLayer::Reflex,
        RunLayer::Routine,
        RunLayer::Deliberative,
        RunLayer::Consolidation,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            RunLayer::Reflex => "reflex",
            RunLayer::Routine => "routine",
            RunLayer::Deliberative => "deliberative",
            RunLayer::Consolidation => "consolidation",
        }
    }
}

#[derive(Default)]
struct LayerCounters {
    runs: AtomicU64,
    /// 因同层仍在运行而跳过/推迟的次数(背压)
    deferred: AtomicU64,
    busy_us: AtomicU64,
    last_us: AtomicU64,
    max_us: AtomicU64,
}

/// 每层的运行次数、推迟次数与耗时。全部为原子计数,记录不加锁。
#[derive(Default)]
pub struct LayerMetrics {
    layers: [LayerCounters; 4],
    /// 模型返回后,等待重新取得 cycle 门以应用决策超时而放弃的次数
    apply_dropped: AtomicU64,
}

impl LayerMetrics {
    fn slot(&self, layer: RunLayer) -> &LayerCounters {
        &self.layers[layer as usize]
    }

    pub fn record_run(&self, layer: RunLayer, elapsed: Duration) {
        let us = elapsed.as_micros().min(u64::MAX as u128) as u64;
        let c = self.slot(layer);
        c.runs.fetch_add(1, Ordering::Relaxed);
        c.busy_us.fetch_add(us, Ordering::Relaxed);
        c.last_us.store(us, Ordering::Relaxed);
        c.max_us.fetch_max(us, Ordering::Relaxed);
    }

    pub fn record_deferred(&self, layer: RunLayer) {
        self.slot(layer).deferred.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_apply_dropped(&self) {
        self.apply_dropped.fetch_add(1, Ordering::Relaxed);
    }

    pub fn runs(&self, layer: RunLayer) -> u64 {
        self.slot(layer).runs.load(Ordering::Relaxed)
    }

    pub fn deferred(&self, layer: RunLayer) -> u64 {
        self.slot(layer).deferred.load(Ordering::Relaxed)
    }

    pub fn snapshot(&self) -> serde_json::Value {
        let mut out = serde_json::Map::new();
        for layer in RunLayer::ALL {
            let c = self.slot(layer);
            let runs = c.runs.load(Ordering::Relaxed);
            let busy = c.busy_us.load(Ordering::Relaxed);
            out.insert(
                layer.as_str().into(),
                serde_json::json!({
                    "runs": runs,
                    "deferred": c.deferred.load(Ordering::Relaxed),
                    "busy_ms": busy / 1000,
                    "avg_ms": busy.checked_div(runs).map_or(0.0, |us| us as f64 / 1000.0),
                    "last_ms": c.last_us.load(Ordering::Relaxed) as f64 / 1000.0,
                    "max_ms": c.max_us.load(Ordering::Relaxed) as f64 / 1000.0,
                }),
            );
        }
        out.insert(
            "apply_dropped".into(),
            self.apply_dropped.load(Ordering::Relaxed).into(),
        );
        serde_json::Value::Object(out)
    }
}

/// 作用域计时:drop 时记一次运行。
pub struct LayerTimer<'a> {
    metrics: &'a LayerMetrics,
    layer: RunLayer,
    start: std::time::Instant,
}

impl LayerMetrics {
    pub fn time(&self, layer: RunLayer) -> LayerTimer<'_> {
        LayerTimer {
            metrics: self,
            layer,
            start: std::time::Instant::now(),
        }
    }
}

impl Drop for LayerTimer<'_> {
    fn drop(&mut self) {
        self.metrics.record_run(self.layer, self.start.elapsed());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_are_per_layer() {
        let m = LayerMetrics::default();
        m.record_run(RunLayer::Routine, Duration::from_millis(4));
        m.record_run(RunLayer::Routine, Duration::from_millis(2));
        m.record_deferred(RunLayer::Deliberative);
        m.record_apply_dropped();
        assert_eq!(m.runs(RunLayer::Routine), 2);
        assert_eq!(m.runs(RunLayer::Deliberative), 0);
        assert_eq!(m.deferred(RunLayer::Deliberative), 1);
        let s = m.snapshot();
        assert_eq!(s["routine"]["runs"], 2);
        assert_eq!(s["routine"]["max_ms"], 4.0);
        assert_eq!(s["routine"]["avg_ms"], 3.0);
        assert_eq!(s["deliberative"]["deferred"], 1);
        assert_eq!(s["apply_dropped"], 1);
        assert_eq!(s["reflex"]["runs"], 0);
    }
}
