//! 分级告警模块。
//!
//! 独立模块，只依赖 `ureq`（已有）发 HTTP，不 import 任何中枢模块。
//! 启动时从环境变量构造一次 Alerter，存入 CloudState。

/// 告警严重度。从低到高：Info < Warning < Critical。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

impl Severity {
    fn as_str(&self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Critical => "critical",
        }
    }
}

/// 告警器。启动时从 env 构造一次，存入 CloudState 共享。
pub struct Alerter {
    webhook: Option<String>,
    min_severity: Severity,
    host: String,
}

impl Alerter {
    /// 从环境变量构造。webhook 缺失时告警功能禁用（不报错，仅 debug 日志）。
    pub fn from_env() -> Self {
        let webhook = std::env::var("EPICODE_ALERT_WEBHOOK")
            .ok()
            .filter(|s| !s.is_empty());
        let min = match std::env::var("EPICODE_ALERT_MIN_SEVERITY")
            .unwrap_or_default()
            .as_str()
        {
            "critical" => Severity::Critical,
            "info" => Severity::Info,
            _ => Severity::Warning,
        };
        let host = hostname_string();
        Self {
            webhook,
            min_severity: min,
            host,
        }
    }

    /// 发送告警。失败仅 log，绝不拖垮调用方（告警本身不能成为新故障源）。
    pub fn send(&self, sev: Severity, title: &str, detail: &str) {
        if !should_send(&sev, &self.min_severity) {
            return;
        }
        let Some(url) = &self.webhook else {
            tracing::debug!("[alert] webhook not configured, dropping {}: {}", title, detail);
            return;
        };
        let payload = if url.contains("feishu") || url.contains("larksuite") {
            Self::build_feishu_payload(&sev, title, detail, &self.host)
        } else {
            Self::build_payload(&sev, title, detail, &self.host)
        };
        let resp = ureq::post(url)
            .set("Content-Type", "application/json")
            .timeout(std::time::Duration::from_secs(5))
            .send_string(&payload);
        if let Err(e) = resp {
            tracing::warn!("[alert] webhook send failed ({}): {}", title, e);
        }
    }

    /// 通用 JSON payload。
    pub fn build_payload(sev: &Severity, title: &str, detail: &str, host: &str) -> String {
        let ts = iso_now();
        serde_json::json!({
            "severity": sev.as_str(),
            "service": "epicode",
            "title": title,
            "detail": detail,
            "host": host,
            "timestamp": ts,
        })
        .to_string()
    }

    /// 飞书自定义机器人 text 消息格式。
    pub fn build_feishu_payload(sev: &Severity, title: &str, detail: &str, host: &str) -> String {
        let emoji = match sev {
            Severity::Critical => "🔴",
            Severity::Warning => "🟡",
            Severity::Info => "🔵",
        };
        let ts = iso_now();
        serde_json::json!({
            "msg_type": "text",
            "content": {
                "text": format!(
                    "{} [{}] {}\n{}\nhost: {} | {}",
                    emoji,
                    sev.as_str(),
                    title,
                    detail,
                    host,
                    ts
                )
            }
        })
        .to_string()
    }
}

/// severity 是否达到发送门槛（pub 供测试）。
pub fn should_send(sev: &Severity, min: &Severity) -> bool {
    sev >= min
}

fn iso_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}

fn hostname_string() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("EPICODE_HOST"))
        .unwrap_or_else(|_| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_ordering() {
        assert!(Severity::Critical > Severity::Warning);
        assert!(Severity::Warning > Severity::Info);
    }

    #[test]
    fn severity_filter_blocks_below_min() {
        // min=warning 时，info 级别应被过滤（不发）
        assert!(!should_send(&Severity::Info, &Severity::Warning));
        assert!(should_send(&Severity::Warning, &Severity::Warning));
        assert!(should_send(&Severity::Critical, &Severity::Warning));
    }

    #[test]
    fn payload_is_valid_json_with_required_fields() {
        let p = Alert::build_payload_pub(&Severity::Critical, "storage", "disk full", "host1");
        let v: serde_json::Value = serde_json::from_str(&p).unwrap();
        assert_eq!(v["severity"], "critical");
        assert_eq!(v["service"], "epicode");
        assert_eq!(v["title"], "storage");
        assert_eq!(v["detail"], "disk full");
        assert_eq!(v["host"], "host1");
        assert!(v["timestamp"].as_str().unwrap().len() > 10);
    }

    #[test]
    fn feishu_payload_has_text_msg_type() {
        let p = Alert::build_feishu_payload_pub(&Severity::Critical, "storage", "disk full", "host1");
        let v: serde_json::Value = serde_json::from_str(&p).unwrap();
        assert_eq!(v["msg_type"], "text");
        assert!(v["content"]["text"].as_str().unwrap().contains("storage"));
        assert!(v["content"]["text"].as_str().unwrap().contains("🔴"));
    }

    // 测试辅助：包装静态方法为可测试形式（避免在测试里重复构造长签名）
    struct Alert;
    impl Alert {
        fn build_payload_pub(sev: &Severity, title: &str, detail: &str, host: &str) -> String {
            Alerter::build_payload(sev, title, detail, host)
        }
        fn build_feishu_payload_pub(sev: &Severity, title: &str, detail: &str, host: &str) -> String {
            Alerter::build_feishu_payload(sev, title, detail, host)
        }
    }
}
