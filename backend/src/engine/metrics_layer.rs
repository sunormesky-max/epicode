//! Prometheus metrics 基础设施。
//!
//! 独立模块，只依赖 `metrics` crate，不 import 任何中枢模块。
//! `init()` 在 cloud.rs 启动序列调用一次，返回的 handle 存入 CloudState 供
//! /metrics 端点渲染。`MetricsTimer` 是零侵入 RAII 计时器（调用方一行声明，
//! Drop 时自动记录 histogram + 计数）。

use std::sync::OnceLock;
use std::time::Instant;

static GLOBAL_HANDLE: OnceLock<Option<PrometheusHandle>> = OnceLock::new();

/// 初始化 Prometheus recorder 并返回 handle（供 /metrics 端点 render）。
/// 全局幂等：已初始化则直接返回缓存的 handle（不重复 install，避免 panic）。
pub fn init() -> PrometheusHandle {
    // install_recorder 全局只能成功一次；用 OnceLock 缓存结果。
    // 返回 clone（PrometheusHandle 内部共享，render 跨实例一致）。
    GLOBAL_HANDLE
        .get_or_init(|| {
            metrics_exporter_prometheus::PrometheusBuilder::new()
                .install_recorder()
                .ok()
        })
        .clone()
        .expect("prometheus recorder not initialized")
}

/// 复用类型别名，避免 cloud.rs 直接依赖 metrics-exporter-prometheus。
pub type PrometheusHandle = metrics_exporter_prometheus::PrometheusHandle;

/// 零侵入 RAII 计时器。Drop 时记录 histogram。
/// 用法：`let _t = MetricsTimer::new("epicode_memory_create_duration_seconds");`
pub struct MetricsTimer {
    name: &'static str,
    start: Instant,
}

impl MetricsTimer {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            start: Instant::now(),
        }
    }
}

impl Drop for MetricsTimer {
    fn drop(&mut self) {
        let secs = self.start.elapsed().as_secs_f64();
        metrics::histogram!(self.name).record(secs);
        metrics::counter!(format!("{}_count", self.name)).increment(1);
    }
}

/// 记录业务计数器（无标签）。
pub fn record_counter(name: &'static str, value: u64) {
    metrics::counter!(name).increment(value);
}

/// 记录 gauge。
pub fn record_gauge(name: &'static str, value: f64) {
    metrics::gauge!(name).set(value);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timer_records_without_panic() {
        // 初始化 recorder（测试用），再确保 timer drop 不 panic。
        let _ = init();
        {
            let _t = MetricsTimer::new("epicode_test_timer_seconds");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        // 不 panic 即通过。
    }

    #[test]
    fn counter_helpers_do_not_panic() {
        let _ = init();
        record_counter("epicode_test_counter", 1);
        record_gauge("epicode_test_gauge", 42.0);
    }
}
