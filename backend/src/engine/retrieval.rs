use crate::domain::tetra::MemoryPayload;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct SearchIntent {
    pub temporal_boost: f64,
    pub type_boosts: HashMap<String, f64>,
    pub label_boosts: HashMap<String, f64>,
    pub negation_ids: Vec<u64>,
    pub expanded_terms: Vec<String>,
    pub primary_intent: String,
    pub exclude_terms: Vec<String>,
}

impl Default for SearchIntent {
    fn default() -> Self {
        Self {
            temporal_boost: 1.0,
            type_boosts: HashMap::new(),
            label_boosts: HashMap::new(),
            negation_ids: Vec::new(),
            expanded_terms: Vec::new(),
            primary_intent: "general".to_string(),
            exclude_terms: Vec::new(),
        }
    }
}

pub struct RetrievalEngine;

impl RetrievalEngine {
    pub fn parse_intent(query: &str) -> SearchIntent {
        let mut intent = SearchIntent::default();
        let lower = query.to_lowercase();

        // Temporal intent
        if lower.contains("昨天")
            || lower.contains("前天")
            || lower.contains("recent")
            || lower.contains("latest")
            || lower.contains("最近")
            || lower.contains("上次")
            || lower.contains("last time")
            || lower.contains("上一次")
            || lower.contains("刚才")
            || lower.contains("just now")
            || lower.contains("今天")
            || lower.contains("today")
        {
            intent.temporal_boost = 2.5;
            intent.primary_intent = "temporal".to_string();
        }

        // Week/month scope
        if lower.contains("本周") || lower.contains("这周") || lower.contains("this week") {
            intent.temporal_boost = 2.0;
            intent.primary_intent = "temporal".to_string();
        }

        // Fix/troubleshoot intent
        if lower.contains("怎么修")
            || lower.contains("怎么解决")
            || lower.contains("how to fix")
            || lower.contains("解决")
            || lower.contains("修复")
            || lower.contains("debug")
            || lower.contains("troubleshoot")
            || lower.contains("报错")
            || lower.contains("error")
            || lower.contains("crash")
            || lower.contains("panic")
            || lower.contains("bug")
            || lower.contains("不工作")
            || lower.contains("不生效")
            || lower.contains("失败")
        {
            intent.type_boosts.insert("bugfix".to_string(), 2.5);
            intent.type_boosts.insert("pattern".to_string(), 1.5);
            if intent.primary_intent == "general" {
                intent.primary_intent = "fix".to_string();
            }
        }

        // Decision/rationale intent
        if lower.contains("为什么")
            || lower.contains("决定")
            || lower.contains("decided")
            || lower.contains("why")
            || lower.contains("为什么选")
            || lower.contains("reason")
            || lower.contains("原因")
            || lower.contains("为什么用")
            || lower.contains("rationale")
            || lower.contains("为什么不用")
        {
            intent.type_boosts.insert("decision".to_string(), 2.5);
            if intent.primary_intent == "general" {
                intent.primary_intent = "decision".to_string();
            }
        }

        // Convention/pattern intent
        if lower.contains("规范")
            || lower.contains("约定")
            || lower.contains("convention")
            || lower.contains("怎么写")
            || lower.contains("怎么用")
            || lower.contains("how to")
            || lower.contains("best practice")
            || lower.contains("正确方式")
            || lower.contains("standard")
            || lower.contains("惯例")
        {
            intent.type_boosts.insert("pattern".to_string(), 2.5);
            if intent.primary_intent == "general" {
                intent.primary_intent = "pattern".to_string();
            }
        }

        // Architecture/design intent
        if lower.contains("架构")
            || lower.contains("设计")
            || lower.contains("architecture")
            || lower.contains("design")
            || lower.contains("结构")
            || lower.contains("模块")
        {
            intent.type_boosts.insert("decision".to_string(), 1.5);
            intent.type_boosts.insert("pattern".to_string(), 1.5);
            if intent.primary_intent == "general" {
                intent.primary_intent = "architecture".to_string();
            }
        }

        // Security intent
        if lower.contains("安全")
            || lower.contains("漏洞")
            || lower.contains("vulnerability")
            || lower.contains("xss")
            || lower.contains("csrf")
            || lower.contains("注入")
            || lower.contains("攻击")
            || lower.contains("封禁")
        {
            intent.type_boosts.insert("security".to_string(), 2.0);
            if intent.primary_intent == "general" {
                intent.primary_intent = "security".to_string();
            }
        }

        // Performance intent
        if lower.contains("性能")
            || lower.contains("优化")
            || lower.contains("performance")
            || lower.contains("慢")
            || lower.contains("slow")
            || lower.contains("latency")
            || lower.contains("吞吐")
            || lower.contains("throughput")
        {
            intent.type_boosts.insert("finding".to_string(), 1.8);
            if intent.primary_intent == "general" {
                intent.primary_intent = "performance".to_string();
            }
        }

        // Comparison intent
        if lower.contains("对比")
            || lower.contains("比较")
            || lower.contains("vs")
            || lower.contains("versus")
            || lower.contains("区别")
            || lower.contains("不同")
            || lower.contains("difference")
            || lower.contains("还是")
            || lower.contains("哪个好")
            || lower.contains("哪个")
            || lower.contains("which is better")
        {
            intent.type_boosts.insert("decision".to_string(), 2.0);
            intent.type_boosts.insert("finding".to_string(), 1.5);
            if intent.primary_intent == "general" {
                intent.primary_intent = "comparison".to_string();
            }
        }

        Self::detect_label_boosts(&lower, &mut intent.label_boosts);
        intent.expanded_terms = Self::expand_query(&lower);
        intent.exclude_terms = Self::extract_exclusions(&lower);

        tracing::info!("[Retrieval] intent={:?} temporal={:.1} type_boosts={:?} label_boosts={:?} expanded={:?}",
            intent.primary_intent, intent.temporal_boost, intent.type_boosts, intent.label_boosts, intent.expanded_terms);

        intent
    }

    fn detect_label_boosts(lower: &str, boosts: &mut HashMap<String, f64>) {
        let keywords: &[(&str, &str)] = &[
            ("部署", "deployment"),
            ("deploy", "deployment"),
            ("发布", "deployment"),
            ("安全", "security"),
            ("security", "security"),
            ("前端", "frontend"),
            ("frontend", "frontend"),
            ("react", "frontend"),
            ("后端", "backend"),
            ("backend", "backend"),
            ("rust", "rust"),
            ("编译", "build"),
            ("cargo", "build"),
            ("build", "build"),
            ("nginx", "nginx"),
            ("防火墙", "firewall"),
            ("firewall", "firewall"),
            ("数据库", "database"),
            ("sqlite", "database"),
            ("db", "database"),
            ("mcp", "mcp"),
            ("guard", "guard"),
            ("docker", "docker"),
            ("容器", "docker"),
            ("https", "https"),
            ("证书", "ssl"),
            ("ssl", "ssl"),
            ("certbot", "ssl"),
            ("测试", "testing"),
            ("test", "testing"),
            ("git", "git"),
            ("github", "github"),
            ("配置", "config"),
            ("config", "config"),
        ];
        for (kw, label) in keywords {
            if lower.contains(kw) {
                boosts
                    .entry(label.to_string())
                    .and_modify(|v| *v = (*v).max(1.5))
                    .or_insert(1.5);
            }
        }
    }

    fn expand_query(lower: &str) -> Vec<String> {
        let mut expansions = Vec::new();

        let bilingual: &[(&str, &[&str])] = &[
            ("部署", &["deploy", "deployment", "release", "publish"]),
            ("安全", &["security", "vulnerability", "CVE"]),
            ("修复", &["fix", "patch", "resolve", "hotfix"]),
            ("错误", &["error", "bug", "crash", "panic"]),
            (
                "性能",
                &["performance", "optimization", "latency", "throughput"],
            ),
            ("架构", &["architecture", "design", "structure"]),
            ("数据库", &["database", "sqlite", "postgres", "db"]),
            ("前端", &["frontend", "react", "UI", "dashboard"]),
            ("后端", &["backend", "server", "API"]),
            ("防火墙", &["firewall", "nft", "iptables", "firewalld"]),
            ("编译", &["build", "compile", "cargo"]),
            ("测试", &["test", "testing", "CI"]),
            ("配置", &["config", "configuration", "settings"]),
            ("证书", &["certificate", "SSL", "TLS", "certbot"]),
        ];

        for (cn, en_terms) in bilingual {
            if lower.contains(cn) {
                for term in *en_terms {
                    expansions.push(term.to_string());
                }
            }
        }

        expansions.sort();
        expansions.dedup();
        expansions
    }

    fn extract_exclusions(lower: &str) -> Vec<String> {
        let mut exclusions = Vec::new();
        let patterns: &[&str] = &[
            "不要用",
            "不要",
            "不用",
            "避免",
            "except ",
            "without ",
            "exclude ",
            "not ",
        ];
        for pat in patterns {
            if let Some(pos) = lower.find(pat) {
                let after = &lower[pos + pat.len()..];
                let word = after
                    .split(|c: char| c.is_whitespace() || !c.is_ascii_alphanumeric())
                    .next()
                    .unwrap_or("")
                    .to_string();
                if word.len() >= 2 {
                    exclusions.push(word.to_lowercase());
                }
            }
        }
        exclusions.sort();
        exclusions.dedup();
        exclusions
    }

    pub fn rerank(
        results: &mut Vec<(u64, f64, f64, MemoryPayload)>,
        intent: &SearchIntent,
        max_results: usize,
    ) {
        for (_id, vec_sim, _bm25, payload) in results.iter_mut() {
            let mut bonus = 0.0_f64;

            // Type boost — scaled by 0.15 for meaningful impact
            if let Some(ref mt) = payload.memory_type {
                if let Some(boost) = intent.type_boosts.get(mt) {
                    bonus += *boost * 0.15;
                }
            }

            // Label boost — scaled by 0.08
            for label in &payload.labels {
                if let Some(boost) = intent.label_boosts.get(label.as_str()) {
                    bonus += *boost * 0.08;
                }
            }

            // Temporal recency — exponential decay
            if intent.temporal_boost > 1.0 {
                let age_days =
                    (chrono::Utc::now().timestamp() - payload.timestamp) as f64 / 86400.0;
                let recency = (-age_days * 0.1).exp();
                bonus += recency * intent.temporal_boost * 0.15;
            }

            // Access frequency — logarithmic, capped contribution
            if payload.access_count > 0 {
                bonus += (payload.access_count as f64).ln().max(0.0) * 0.03;
            }

            // Importance-weighted bonus
            if payload.importance > 2.0 {
                bonus += 0.05;
            }

            // Expanded term match — check if content contains any expanded terms
            if !intent.expanded_terms.is_empty() {
                let content_lower = payload.content.to_lowercase();
                let match_count = intent
                    .expanded_terms
                    .iter()
                    .filter(|t| content_lower.contains(t.as_str()))
                    .count();
                bonus += match_count as f64 * 0.04;
            }

            // Negative filtering — heavy penalty for excluded terms
            if !intent.exclude_terms.is_empty() {
                let content_lower = payload.content.to_lowercase();
                let exclude_hits = intent
                    .exclude_terms
                    .iter()
                    .filter(|t| content_lower.contains(t.as_str()))
                    .count();
                if exclude_hits > 0 {
                    bonus -= 0.5 * exclude_hits as f64;
                    tracing::info!(
                        "[Retrieval] negative filter: id={} penalized -{:.2} (hits={})",
                        _id,
                        0.3 * exclude_hits as f64,
                        exclude_hits
                    );
                }
            }

            let old_sim = *vec_sim;
            *vec_sim = old_sim + bonus;
        }

        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(max_results);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tetra::MemoryPayload;

    fn make_payload(content: &str, labels: Vec<&str>, memory_type: Option<&str>) -> MemoryPayload {
        MemoryPayload {
            content: content.to_string(),
            content_hash: 0,
            labels: labels.iter().map(|s| s.to_string()).collect(),
            timestamp: chrono::Utc::now().timestamp() - 3600,
            aliases: vec![],
            embedding: vec![],
            importance: 1.0,
            enforced: false,
            rationale: None,
            access_count: 0,
            quality_score: 1.0,
            memory_type: memory_type.map(|s| s.to_string()),
        }
    }

    #[test]
    fn temporal_intent() {
        let intent = RetrievalEngine::parse_intent("最近做了什么");
        assert_eq!(intent.primary_intent, "temporal");
        assert!(intent.temporal_boost > 1.5);
    }

    #[test]
    fn fix_intent() {
        let intent = RetrievalEngine::parse_intent("怎么修复部署403错误");
        assert_eq!(intent.primary_intent, "fix");
        assert!(intent.type_boosts.contains_key("bugfix"));
    }

    #[test]
    fn decision_intent() {
        let intent = RetrievalEngine::parse_intent("为什么选nft而不是firewalld");
        assert_eq!(intent.primary_intent, "decision");
        assert!(intent.type_boosts.contains_key("decision"));
    }

    #[test]
    fn comparison_intent() {
        let intent = RetrievalEngine::parse_intent("Redis和内存缓存哪个好");
        assert_eq!(intent.primary_intent, "comparison");
    }

    #[test]
    fn performance_intent() {
        let intent = RetrievalEngine::parse_intent("性能优化方案");
        assert_eq!(intent.primary_intent, "performance");
        assert!(intent.type_boosts.contains_key("finding"));
    }

    #[test]
    fn query_expansion() {
        let intent = RetrievalEngine::parse_intent("部署配置问题");
        assert!(!intent.expanded_terms.is_empty());
        assert!(intent.expanded_terms.iter().any(|t| t == "deploy"));
    }

    #[test]
    fn label_boost() {
        let intent = RetrievalEngine::parse_intent("nginx配置错误");
        assert!(intent.label_boosts.contains_key("nginx"));
    }

    #[test]
    fn rerank_boosts_bugfix() {
        let bugfix = make_payload("Fixed crash in handler", vec!["bug"], Some("bugfix"));
        let session = make_payload("Session summary", vec!["session"], Some("session"));
        let mut results = vec![
            (1u64, 0.5f64, 0.5f64, session),
            (2u64, 0.5f64, 0.5f64, bugfix),
        ];
        let intent = RetrievalEngine::parse_intent("怎么修crash");
        RetrievalEngine::rerank(&mut results, &intent, 10);
        assert_eq!(results[0].0, 2);
    }

    #[test]
    fn exclusion_extraction() {
        let intent = RetrievalEngine::parse_intent("不要用firewalld");
        assert!(!intent.exclude_terms.is_empty());
    }

    #[test]
    fn negative_filter_penalizes() {
        let firewalld = make_payload(
            "Use firewalld for all blocking rules on the server",
            vec!["firewall"],
            Some("decision"),
        );
        let nft = make_payload(
            "Use nft direct table for blocking rules on the server",
            vec!["firewall"],
            Some("decision"),
        );
        let mut results = vec![
            (1u64, 0.6f64, 0.5f64, firewalld),
            (2u64, 0.5f64, 0.5f64, nft),
        ];
        let intent = RetrievalEngine::parse_intent("不要用firewalld怎么配置");
        RetrievalEngine::rerank(&mut results, &intent, 10);
        assert_eq!(
            results[0].0, 2,
            "nft should outrank firewalld when excluded"
        );
    }
}
