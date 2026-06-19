use crate::domain::tetra::MemoryPayload;

fn truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

pub struct IntakeResult {
    pub labels: Vec<String>,
    pub importance: f64,
    pub memory_type: Option<String>,
    pub rationale: Option<String>,
    pub conflict_ids: Vec<u64>,
    pub duplicate_of: Option<u64>,
    pub is_noise: bool,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum MemoryClass {
    Decision,
    Bugfix,
    Pattern,
    Session,
    Finding,
    Experiment,
    Security,
    Identity,
    Research,
    Noise,
    General,
}

pub struct MemoryIntake;

impl MemoryIntake {
    pub fn process(content: &str, labels: &mut Vec<String>) -> IntakeResult {
        let class = Self::classify(content, labels);

        if class == MemoryClass::Noise {
            tracing::info!(
                "[Intake] REJECTED as noise: {:?}",
                truncate_str(content, 60)
            );
            return IntakeResult {
                labels: std::mem::take(labels),
                importance: 0.0,
                memory_type: Some("noise".to_string()),
                rationale: None,
                conflict_ids: vec![],
                duplicate_of: None,
                is_noise: true,
                tags: vec![],
            };
        }

        let memory_type = match &class {
            MemoryClass::Decision => "decision",
            MemoryClass::Bugfix => "bugfix",
            MemoryClass::Pattern => "pattern",
            MemoryClass::Session => "session",
            MemoryClass::Finding => "finding",
            MemoryClass::Experiment => "experiment",
            MemoryClass::Security => "security",
            MemoryClass::Identity => "identity",
            MemoryClass::Research => "research",
            MemoryClass::General => "general",
            MemoryClass::Noise => unreachable!(),
        }
        .to_string();

        let importance = Self::score_importance(content, &class);
        let rationale = Self::extract_rationale(content, &memory_type);
        let tags = Self::extract_tags(content);
        Self::enrich_labels(labels, &memory_type, &tags);

        tracing::info!(
            "[Intake] type={} importance={:.2} rationale={} tags={:?} content={:?}",
            memory_type,
            importance,
            rationale.as_ref().map(|r| r.len()).unwrap_or(0),
            tags,
            truncate_str(content, 80)
        );

        IntakeResult {
            labels: std::mem::take(labels),
            importance,
            memory_type: Some(memory_type),
            rationale,
            conflict_ids: vec![],
            duplicate_of: None,
            is_noise: false,
            tags,
        }
    }

    fn classify(content: &str, labels: &[String]) -> MemoryClass {
        let trimmed = content.trim();

        if trimmed.len() < 5 {
            return MemoryClass::Noise;
        }

        let short = trimmed.len() < 20;
        let lower = trimmed.to_lowercase();

        if short {
            let noise_words = [
                "ok", "好的", "好", "yes", "no", "nope", "test", "测试", "嗯", "哦", "thanks",
                "谢谢", "好嘞", "行", "可以", "done", "收到", "了解",
            ];
            if noise_words
                .iter()
                .any(|w| trimmed.eq_ignore_ascii_case(w) || lower == *w)
            {
                return MemoryClass::Noise;
            }
        }

        let label_set: std::collections::HashSet<&str> =
            labels.iter().map(|s| s.as_str()).collect();
        if label_set.contains("decision") {
            return MemoryClass::Decision;
        }
        if label_set.contains("bug") {
            return MemoryClass::Bugfix;
        }
        if label_set.contains("pattern") {
            return MemoryClass::Pattern;
        }
        if label_set.contains("session-summary") || label_set.contains("session") {
            return MemoryClass::Session;
        }
        if label_set.contains("finding") {
            return MemoryClass::Finding;
        }
        if label_set.contains("experiment") {
            return MemoryClass::Experiment;
        }
        if label_set.contains("security") {
            return MemoryClass::Security;
        }
        if label_set.contains("identity") {
            return MemoryClass::Identity;
        }
        if label_set.contains("research") {
            return MemoryClass::Research;
        }

        if lower.contains("[decision]")
            || lower.contains("决策")
            || lower.contains("chosen:")
            || lower.contains("chose")
            || lower.contains("选用")
            || lower.contains("改用")
            || lower.contains("选定")
            || lower.contains("we decided")
            || lower.contains("we chose")
        {
            return MemoryClass::Decision;
        }
        if lower.contains("[bug]")
            || lower.contains("symptoms:")
            || lower.contains("root_cause:")
            || lower.contains("fix:")
            || lower.contains("修复了")
            || lower.contains("resolved by")
            || lower.contains("workaround:")
            || lower.contains("hotfix:")
        {
            return MemoryClass::Bugfix;
        }
        if lower.contains("[pattern]")
            || lower.contains("convention:")
            || lower.contains("always use")
            || lower.contains("never use")
            || lower.contains("必须")
            || lower.contains("约定")
            || lower.contains("规范")
            || lower.contains("best practice")
        {
            return MemoryClass::Pattern;
        }
        if lower.contains("[session]")
            || lower.contains("accomplished:")
            || lower.contains("会话总结")
            || lower.contains("session summary")
        {
            return MemoryClass::Session;
        }
        if lower.contains("[finding]")
            || lower.contains("发现")
            || lower.contains("insight:")
            || lower.contains("turns out")
            || lower.contains("关键发现")
        {
            return MemoryClass::Finding;
        }
        if lower.starts_with("v") && lower.contains("实验") || lower.contains("experiment") {
            return MemoryClass::Experiment;
        }
        if lower.contains("security ban:")
            || lower.contains("security alert:")
            || lower.contains("cve-")
            || lower.contains("vulnerability")
        {
            return MemoryClass::Security;
        }
        if lower.contains("身份")
            || lower.contains("identity")
            || lower.contains("i am")
            || lower.contains("我是")
            || lower.contains("我的名字")
        {
            return MemoryClass::Identity;
        }
        if lower.contains("[research]")
            || lower.contains("arxiv")
            || lower.contains("论文")
            || lower.contains("前沿")
            || lower.contains("survey")
        {
            return MemoryClass::Research;
        }

        MemoryClass::General
    }

    fn score_importance(content: &str, class: &MemoryClass) -> f64 {
        let base = match class {
            MemoryClass::Identity => 3.0,
            MemoryClass::Decision => 2.5,
            MemoryClass::Experiment => 2.0,
            MemoryClass::Finding => 2.0,
            MemoryClass::Bugfix => 1.5,
            MemoryClass::Pattern => 1.5,
            MemoryClass::Research => 1.5,
            MemoryClass::Security => 1.2,
            MemoryClass::Session => 1.0,
            MemoryClass::General => 0.8,
            MemoryClass::Noise => 0.0,
        };

        let lower = content.to_lowercase();
        let mut boost = 0.0_f64;

        if lower.contains("critical")
            || lower.contains("不能")
            || lower.contains("never")
            || lower.contains("绝对不能")
            || lower.contains("必须")
            || lower.contains("致命")
            || lower.contains("crucial")
            || lower.contains("vital")
        {
            boost += 0.5;
        } else if lower.contains("重要")
            || lower.contains("important")
            || lower.contains("关键")
            || lower.contains("key")
        {
            boost += 0.3;
        }

        let len = content.len();
        let length_bonus = if len > 200 {
            0.2
        } else if len > 50 {
            0.1
        } else {
            0.0
        };

        let has_code = lower.contains("```")
            || lower.contains("fn ")
            || lower.contains("pub fn")
            || lower.contains("impl ")
            || lower.contains("cargo ")
            || lower.contains("use ");
        let code_bonus = if has_code { 0.1 } else { 0.0 };

        (base + boost + length_bonus + code_bonus).clamp(0.1, 3.5)
    }

    fn extract_rationale(content: &str, memory_type: &str) -> Option<String> {
        let lower = content.to_lowercase();

        match memory_type {
            "decision" | "finding" | "pattern" => {}
            _ => return None,
        }

        if let Some(pos) = lower.find("rationale:") {
            let r = content[pos + 10..].trim();
            let end = r
                .char_indices()
                .take_while(|(i, _)| *i < 300)
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(r.len().min(300));
            let extracted = r[..end].trim();
            if !extracted.is_empty() {
                return Some(extracted.to_string());
            }
        }

        let causal_patterns: &[&str] = &[
            "因为",
            "因为 ",
            "由于",
            "由于 ",
            "because",
            "because ",
            "since ",
            "as ",
            "原因是",
            "reason:",
            "why:",
            "所以",
            "therefore",
            "in order to",
            "目的是",
        ];
        for pat in causal_patterns {
            if let Some(pos) = lower.find(pat) {
                let start = pos;
                let end_pos = (start + 250).min(content.len());
                let boundary = content
                    .char_indices()
                    .take_while(|(i, _)| *i < end_pos)
                    .last()
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(end_pos);
                let extracted = content[start..boundary].trim();
                if !extracted.is_empty() && extracted.len() < 300 {
                    return Some(extracted.to_string());
                }
            }
        }

        if let Some(pos) = lower.find("| details:") {
            let r = content[pos + 10..].split('|').next().unwrap_or("").trim();
            if !r.is_empty() && r.len() < 500 {
                return Some(r.to_string());
            }
        }

        None
    }

    fn extract_tags(content: &str) -> Vec<String> {
        let lower = content.to_lowercase();
        let mut tags = Vec::new();

        let tech_keywords: &[(&str, &str)] = &[
            ("rust", "rust"),
            ("cargo", "rust"),
            ("tokio", "async"),
            ("python", "python"),
            ("typescript", "typescript"),
            ("javascript", "javascript"),
            ("react", "frontend"),
            ("tailwind", "frontend"),
            ("css", "frontend"),
            ("nginx", "nginx"),
            ("docker", "docker"),
            ("kubernetes", "k8s"),
            ("sqlite", "database"),
            ("postgres", "database"),
            ("redis", "database"),
            ("ssh", "ssh"),
            ("https", "https"),
            ("certbot", "ssl"),
            ("git", "git"),
            ("github", "github"),
            ("ci/cd", "cicd"),
            ("wasm", "wasm"),
            ("onnx", "ml"),
            ("embedding", "ml"),
            ("xss", "security-vuln"),
            ("csrf", "security-vuln"),
            ("cve", "security-vuln"),
            ("curl", "debugging"),
            ("日志", "logging"),
            ("log::", "logging"),
            ("deploy", "deployment"),
            ("部署", "deployment"),
        ];

        for (kw, tag) in tech_keywords {
            if lower.contains(kw) {
                tags.push(tag.to_string());
            }
        }

        tags.sort();
        tags.dedup();
        tags
    }

    pub fn text_similarity(a: &str, b: &str) -> f64 {
        let lower_a = a.to_lowercase();
        let lower_b = b.to_lowercase();
        let set_a: std::collections::HashSet<&str> = lower_a
            .split(|c: char| !c.is_alphanumeric() && c != '-')
            .filter(|w| w.len() >= 2)
            .collect();
        let set_b: std::collections::HashSet<&str> = lower_b
            .split(|c: char| !c.is_alphanumeric() && c != '-')
            .filter(|w| w.len() >= 2)
            .collect();
        if set_a.is_empty() || set_b.is_empty() {
            return 0.0;
        }
        let intersection = set_a.intersection(&set_b).count();
        let union = set_a.union(&set_b).count();
        intersection as f64 / union as f64
    }

    fn enrich_labels(labels: &mut Vec<String>, memory_type: &str, tags: &[String]) {
        let type_label = memory_type.to_string();
        if !labels.contains(&type_label) && memory_type != "general" {
            labels.push(type_label);
        }
        for tag in tags {
            if !labels.contains(tag) {
                labels.push(tag.clone());
            }
        }
    }

    pub fn check_conflict(content: &str, similar: &[(u64, f64, f64, MemoryPayload)]) -> Vec<u64> {
        let negation_words = [
            "不是",
            "不能",
            "错误",
            "修正",
            "已修正",
            "fix",
            "fixes",
            "fixed",
            "wrong",
            "incorrect",
            "不再",
            "改为",
            "换为",
            "renamed",
            "should not",
            "deprecated",
            "废弃",
            "移除",
            "removed",
            "instead of",
            "替代",
        ];
        let content_lower = content.to_lowercase();
        let content_has_negation = negation_words.iter().any(|w| content_lower.contains(w));

        if !content_has_negation {
            return vec![];
        }

        let mut conflicts = vec![];
        for (id, sim, _bm25, payload) in similar {
            if *sim > 0.75 {
                let payload_lower = payload.content.to_lowercase();
                let topic_overlap = Self::topic_overlap(&content_lower, &payload_lower);
                if topic_overlap > 0.4 {
                    tracing::info!(
                        "[Intake] conflict detected: new memory vs id={} (sim={:.2} overlap={:.2})",
                        id,
                        sim,
                        topic_overlap
                    );
                    conflicts.push(*id);
                }
            }
        }
        conflicts
    }

    fn topic_overlap(a: &str, b: &str) -> f64 {
        let lower_a = a.to_lowercase();
        let lower_b = b.to_lowercase();
        let keywords_a: Vec<&str> = lower_a
            .split(|c: char| !c.is_alphanumeric() && c != '-')
            .filter(|w| w.len() >= 2)
            .collect();
        if keywords_a.is_empty() {
            return 0.0;
        }
        let keywords_b: std::collections::HashSet<&str> = lower_b
            .split(|c: char| !c.is_alphanumeric() && c != '-')
            .filter(|w| w.len() >= 2)
            .collect();
        let common = keywords_a
            .iter()
            .filter(|w| keywords_b.contains(**w))
            .count();
        common as f64 / keywords_a.len() as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_rejection() {
        let mut labels = vec!["test".to_string()];
        let result = MemoryIntake::process("ok", &mut labels);
        assert!(result.is_noise);
        assert_eq!(result.memory_type.as_deref(), Some("noise"));

        let mut labels2 = vec![];
        let result2 = MemoryIntake::process("好的", &mut labels2);
        assert!(result2.is_noise);
    }

    #[test]
    fn decision_classification() {
        let mut labels = vec!["decision".to_string()];
        let result = MemoryIntake::process(
            "Use PostgreSQL instead of SQLite because we need concurrent writes",
            &mut labels,
        );
        assert!(!result.is_noise);
        assert_eq!(result.memory_type.as_deref(), Some("decision"));
        assert!(result.importance >= 2.5);
    }

    #[test]
    fn bugfix_classification() {
        let mut labels = vec!["bug".to_string()];
        let result = MemoryIntake::process(
            "Fixed memory leak in WebSocket handler. root_cause: connection pool never closed",
            &mut labels,
        );
        assert!(!result.is_noise);
        assert_eq!(result.memory_type.as_deref(), Some("bugfix"));
        assert!(result.labels.contains(&"bugfix".to_string()));
    }

    #[test]
    fn rationale_extraction_from_keyword() {
        let mut labels = vec!["decision".to_string()];
        let result = MemoryIntake::process("Use nft instead of iptables. rationale: iptables is deprecated and nft is the replacement", &mut labels);
        assert!(result.rationale.is_some());
        let r = result.rationale.unwrap();
        assert!(r.contains("iptables is deprecated"));
    }

    #[test]
    fn rationale_extraction_because() {
        let mut labels = vec!["decision".to_string()];
        let result = MemoryIntake::process(
            "We chose Rust because memory safety is critical for our use case",
            &mut labels,
        );
        assert!(result.rationale.is_some());
        assert!(result.rationale.unwrap().contains("memory safety"));
    }

    #[test]
    fn tag_extraction() {
        let mut labels = vec![];
        let result = MemoryIntake::process(
            "Deploy new nginx config with HTTPS and SSL certbot on the server",
            &mut labels,
        );
        assert!(result.tags.contains(&"nginx".to_string()));
        assert!(result.tags.contains(&"ssl".to_string()));
        assert!(result.tags.contains(&"deployment".to_string()));
    }

    #[test]
    fn importance_critical_boost() {
        let mut labels1 = vec!["decision".to_string()];
        let r1 = MemoryIntake::process(
            "Critical: never use unwrap in production code",
            &mut labels1,
        );
        let mut labels2 = vec!["decision".to_string()];
        let r2 = MemoryIntake::process("Changed log level from info to debug", &mut labels2);
        assert!(r1.importance > r2.importance);
    }

    #[test]
    fn text_similarity_basic() {
        let sim = MemoryIntake::text_similarity(
            "Use Redis for session cache",
            "Use Redis for session caching",
        );
        assert!(sim > 0.5);

        let sim2 = MemoryIntake::text_similarity("Deploy nginx config", "Fix React component bug");
        assert!(sim2 < 0.3);
    }

    #[test]
    fn text_similarity_case_insensitive() {
        let sim = MemoryIntake::text_similarity(
            "Use Firewalld for blocking",
            "use firewalld for blocking",
        );
        assert!(sim > 0.9);
    }

    #[test]
    fn noise_short_content() {
        let mut labels = vec![];
        let result = MemoryIntake::process("嗯", &mut labels);
        assert!(result.is_noise);

        let mut labels2 = vec![];
        let result2 = MemoryIntake::process("a", &mut labels2);
        assert!(result2.is_noise);
    }

    #[test]
    fn general_content_passes() {
        let mut labels = vec![];
        let result = MemoryIntake::process(
            "This is a general note about the project status and timeline for next quarter",
            &mut labels,
        );
        assert!(!result.is_noise);
        assert_eq!(result.memory_type.as_deref(), Some("general"));
    }

    #[test]
    fn conflict_detection() {
        let payload = MemoryPayload {
            content: "Use firewalld for port blocking on the server".to_string(),
            content_hash: 0,
            labels: vec![],
            timestamp: 0,
            aliases: vec![],
            embedding: vec![],
            importance: 1.0,
            enforced: false,
            rationale: None,
            access_count: 0,
            memory_type: None,
        };
        let similar = vec![(1u64, 0.85f64, 0.5f64, payload)];
        let conflicts = MemoryIntake::check_conflict(
            "Fix: removed firewalld,改为 use nft instead of iptables for port blocking",
            &similar,
        );
        assert!(
            !conflicts.is_empty(),
            "should detect conflict with negation + topic overlap"
        );
    }
}
