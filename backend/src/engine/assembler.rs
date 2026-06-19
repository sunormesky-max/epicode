use crate::domain::tetra::MemoryPayload;
use std::collections::HashSet;

pub struct ContextAssembler;

#[derive(PartialEq, Eq, Hash, Clone, Copy)]
enum SectionKind {
    LatestSession,
    ProjectContext,
    ActiveDecisions,
    Constraints,
    KnownPatterns,
    KnownBlockers,
    Identity,
}

impl SectionKind {
    fn priority(&self, intent: &str) -> u32 {
        match self {
            Self::LatestSession => match intent {
                "temporal" => 100,
                _ => 90,
            },
            Self::ProjectContext => 80,
            Self::ActiveDecisions => match intent {
                "decision" | "comparison" | "architecture" => 95,
                "fix" => 50,
                _ => 70,
            },
            Self::Constraints => match intent {
                "pattern" | "fix" => 85,
                _ => 60,
            },
            Self::KnownPatterns => match intent {
                "pattern" | "fix" | "performance" => 90,
                _ => 50,
            },
            Self::KnownBlockers => match intent {
                "fix" => 95,
                _ => 40,
            },
            Self::Identity => match intent {
                "temporal" => 75,
                _ => 20,
            },
        }
    }
}

struct Section {
    kind: SectionKind,
    title: String,
    body: String,
    token_estimate: usize,
}

impl ContextAssembler {
    pub fn assemble(
        memories: &[(u64, MemoryPayload)],
        enforced: &[(u64, String, Vec<String>)],
        limit: usize,
        intent: &str,
    ) -> String {
        let now = chrono::Utc::now().timestamp();
        let day_secs: i64 = 86400;
        let token_budget = limit * 180;

        let mut latest_session: Option<&(u64, MemoryPayload)> = None;
        let mut recent_decisions: Vec<&(u64, MemoryPayload)> = Vec::new();
        let mut patterns: Vec<&(u64, MemoryPayload)> = Vec::new();
        let mut identities: Vec<&(u64, MemoryPayload)> = Vec::new();
        let mut project_ctx: Vec<&(u64, MemoryPayload)> = Vec::new();
        let mut constraints: Vec<String> = Vec::new();
        let mut blockers: Vec<&(u64, MemoryPayload)> = Vec::new();

        for m in memories {
            let labels: HashSet<&str> = m.1.labels.iter().map(|s| s.as_str()).collect();

            if labels.contains("session-summary") || labels.contains("session") {
                if latest_session.is_none() || m.1.timestamp > latest_session.unwrap().1.timestamp {
                    latest_session = Some(m);
                }
            }
            if labels.contains("decision") && (now - m.1.timestamp) < day_secs * 14 {
                recent_decisions.push(m);
            }
            if labels.contains("pattern") && (now - m.1.timestamp) < day_secs * 30 {
                patterns.push(m);
            }
            if labels.contains("identity") || labels.contains("system") {
                identities.push(m);
            }
            if labels.contains("project-context") || labels.contains("architecture") {
                project_ctx.push(m);
            }
            if labels.contains("bug") {
                let lower = m.1.content.to_lowercase();
                if lower.contains("blocked")
                    || lower.contains("阻塞")
                    || lower.contains("unresolved")
                    || lower.contains("未解决")
                {
                    blockers.push(m);
                }
            }
        }

        for (_id, content, _labels) in enforced {
            constraints.push(content.clone());
        }

        recent_decisions.sort_by(|a, b| b.1.timestamp.cmp(&a.1.timestamp));
        patterns.sort_by(|a, b| b.1.timestamp.cmp(&a.1.timestamp));

        let mut used_ids: HashSet<u64> = HashSet::new();
        let mut sections: Vec<Section> = Vec::new();

        // Build sections with dedup tracking
        if let Some(session) = latest_session {
            used_ids.insert(session.0);
            let age_hours = (now - session.1.timestamp) / 3600;
            let time_desc = format_time_ago(age_hours);
            sections.push(Section {
                kind: SectionKind::LatestSession,
                title: format!("## Latest Session ({})", time_desc),
                body: truncate(&session.1.content, 800),
                token_estimate: estimate_tokens(&session.1.content, 800),
            });
        }

        if !project_ctx.is_empty() {
            let ctx = project_ctx[0];
            if !used_ids.contains(&ctx.0) {
                used_ids.insert(ctx.0);
                sections.push(Section {
                    kind: SectionKind::ProjectContext,
                    title: "## Project Context".to_string(),
                    body: truncate(&ctx.1.content, 600),
                    token_estimate: estimate_tokens(&ctx.1.content, 600),
                });
            }
        }

        if !recent_decisions.is_empty() {
            let mut body = String::new();
            for d in recent_decisions.iter().take(8) {
                if used_ids.contains(&d.0) {
                    continue;
                }
                used_ids.insert(d.0);
                let line = if let Some(ref r) = d.1.rationale {
                    format!(
                        "\n- {} — why: {}",
                        truncate(&d.1.content, 120),
                        truncate(r, 80)
                    )
                } else {
                    format!("\n- {}", truncate(&d.1.content, 150))
                };
                body.push_str(&line);
            }
            if !body.is_empty() {
                let tokens = body.len() / 4;
                sections.push(Section {
                    kind: SectionKind::ActiveDecisions,
                    title: "## Active Decisions".to_string(),
                    body,
                    token_estimate: tokens,
                });
            }
        }

        if !constraints.is_empty() {
            let mut body = String::new();
            for c in constraints.iter().take(10) {
                body.push_str(&format!("\n- {}", truncate(c, 150)));
            }
            let tokens = body.len() / 4;
            sections.push(Section {
                kind: SectionKind::Constraints,
                title: "## Hard Constraints (enforced)".to_string(),
                body,
                token_estimate: tokens,
            });
        }

        if !patterns.is_empty() {
            let mut body = String::new();
            for p in patterns.iter().take(6) {
                if used_ids.contains(&p.0) {
                    continue;
                }
                used_ids.insert(p.0);
                body.push_str(&format!("\n- {}", truncate(&p.1.content, 120)));
            }
            if !body.is_empty() {
                let tokens = body.len() / 4;
                sections.push(Section {
                    kind: SectionKind::KnownPatterns,
                    title: "## Known Patterns".to_string(),
                    body,
                    token_estimate: tokens,
                });
            }
        }

        if !blockers.is_empty() {
            let mut body = String::new();
            for b in blockers.iter().take(5) {
                if used_ids.contains(&b.0) {
                    continue;
                }
                used_ids.insert(b.0);
                body.push_str(&format!("\n- {}", truncate(&b.1.content, 120)));
            }
            if !body.is_empty() {
                let tokens = body.len() / 4;
                sections.push(Section {
                    kind: SectionKind::KnownBlockers,
                    title: "## Known Blockers".to_string(),
                    body,
                    token_estimate: tokens,
                });
            }
        }

        if !identities.is_empty() {
            let id = &identities[0];
            if !used_ids.contains(&id.0) && id.1.content.len() > 10 {
                sections.push(Section {
                    kind: SectionKind::Identity,
                    title: "## Identity".to_string(),
                    body: truncate(&id.1.content, 200),
                    token_estimate: estimate_tokens(&id.1.content, 200),
                });
            }
        }

        // Sort by priority and build output within token budget
        sections.sort_by_key(|s| std::cmp::Reverse(s.kind.priority(intent)));

        let mut output = String::new();
        let mut tokens_used = 0;

        for section in &sections {
            if tokens_used + section.token_estimate > token_budget {
                // Try to fit a truncated version
                let remaining = token_budget.saturating_sub(tokens_used);
                if remaining > 50 {
                    let max_chars = remaining * 4;
                    output.push_str(&section.title);
                    output.push('\n');
                    output.push_str(&truncate(&section.body, max_chars));
                    output.push_str("\n...\n");
                }
                break;
            }
            output.push_str(&section.title);
            output.push('\n');
            output.push_str(&section.body);
            output.push_str("\n\n");
            tokens_used += section.token_estimate;
        }

        output.trim_end().to_string()
    }
}

fn format_time_ago(age_hours: i64) -> String {
    if age_hours < 1 {
        "just now".to_string()
    } else if age_hours < 24 {
        format!("{}h ago", age_hours)
    } else {
        format!("{}d ago", age_hours / 24)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let boundary = s
            .char_indices()
            .take_while(|(i, _)| *i < max)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(max);
        let end = s[..boundary].rfind('\n').unwrap_or(boundary);
        s[..end.min(boundary)].to_string() + "..."
    }
}

fn estimate_tokens(content: &str, max_chars: usize) -> usize {
    let text = if content.len() <= max_chars {
        content
    } else {
        let mut end = max_chars;
        while end > 0 && !content.is_char_boundary(end) {
            end -= 1;
        }
        &content[..end]
    };
    text.len() / 3 + text.chars().filter(|c| c.is_ascii()).count() / 5
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tetra::MemoryPayload;

    fn make_mem(id: u64, content: &str, labels: Vec<&str>) -> (u64, MemoryPayload) {
        (
            id,
            MemoryPayload {
                content: content.to_string(),
                content_hash: 0,
                labels: labels.iter().map(|s| s.to_string()).collect(),
                timestamp: chrono::Utc::now().timestamp(),
                aliases: vec![],
                embedding: vec![],
                importance: 1.0,
                enforced: false,
                rationale: None,
                access_count: 0,
                memory_type: None,
            },
        )
    }

    #[test]
    fn assemble_basic_sections() {
        let memories = vec![
            make_mem(
                1,
                "Session summary for today: deployed v1.0.0",
                vec!["session-summary"],
            ),
            make_mem(2, "Use nft instead of firewalld", vec!["decision"]),
            make_mem(
                3,
                "Always use atomic replace for deployment",
                vec!["pattern"],
            ),
        ];
        let enforced: Vec<(u64, String, Vec<String>)> = vec![];
        let result = ContextAssembler::assemble(&memories, &enforced, 10, "general");
        assert!(result.contains("Latest Session"));
        assert!(result.contains("Active Decisions"));
        assert!(result.contains("Known Patterns"));
    }

    #[test]
    fn dedup_no_repeat() {
        let mem = make_mem(
            1,
            "Important session + decision hybrid content",
            vec!["session-summary", "decision"],
        );
        let enforced: Vec<(u64, String, Vec<String>)> = vec![];
        let result = ContextAssembler::assemble(&[mem], &enforced, 10, "general");
        let count = result.matches("Important session").count();
        assert!(
            count <= 1,
            "content should not appear more than once, got {} times",
            count
        );
    }

    #[test]
    fn fix_intent_prioritizes_blockers() {
        let session = make_mem(1, "Session summary", vec!["session-summary"]);
        let bug = make_mem(
            2,
            "Bug in deployment — blocked by port 9111 still occupied. Unresolved.",
            vec!["bug"],
        );
        let enforced: Vec<(u64, String, Vec<String>)> = vec![];
        let result = ContextAssembler::assemble(&[session, bug], &enforced, 10, "fix");
        let blocker_pos = result.find("Known Blockers");
        let session_pos = result.find("Latest Session");
        if let (Some(bp), Some(sp)) = (blocker_pos, session_pos) {
            assert!(
                bp < sp,
                "blockers should come before session for fix intent"
            );
        }
    }

    #[test]
    fn truncate_utf8_safe() {
        let chinese = "这是一个中文字符串，用于测试UTF8截断是否安全";
        let truncated = truncate(chinese, 20);
        assert!(!truncated.is_empty());
        for ch in truncated.chars() {
            assert_ne!(ch, '\u{fffd}', "should not contain replacement characters");
        }
    }
}
