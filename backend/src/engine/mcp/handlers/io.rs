use super::super::*;

pub(in crate::engine::mcp) struct ExtractedMemory {
    category: String,
    content: String,
    labels: Vec<String>,
}

pub(in crate::engine::mcp) fn extract_context_memories(
    context: &str,
    project: &str,
    role: &str,
) -> Vec<ExtractedMemory> {
    let mut results: Vec<ExtractedMemory> = Vec::new();
    let mut used_lines: std::collections::HashSet<usize> = std::collections::HashSet::new();

    for (line_idx, line) in context.lines().enumerate() {
        if used_lines.contains(&line_idx) {
            continue;
        }
        if line.len() < 15 || line.len() > 2000 {
            continue;
        }
        let line_lower = line.to_lowercase();
        let truncated = if line.len() > 500 {
            truncate_str(line, 500)
        } else {
            line
        };

        let extracted = if has_bug_and_fix(&line_lower) {
            Some(make_extraction(
                "bug",
                truncated,
                context,
                project,
                &["bug", "fix"],
            ))
        } else if matches_decision(&line_lower) {
            Some(make_extraction(
                "decision",
                truncated,
                context,
                project,
                &["decision"],
            ))
        } else if matches_pattern(&line_lower) {
            Some(make_extraction(
                "pattern",
                truncated,
                context,
                project,
                &["pattern", "convention"],
            ))
        } else if matches_preference(&line_lower) {
            Some(make_extraction(
                "preference",
                truncated,
                context,
                project,
                &["preference"],
            ))
        } else {
            None
        };

        if let Some(ext) = extracted {
            used_lines.insert(line_idx);
            results.push(ext);
        }

        if results.len() >= 3 {
            break;
        }
    }

    if results.is_empty() {
        extract_fallback(context, project, role, &mut results);
    }

    results
}

pub(in crate::engine::mcp) fn make_extraction(
    category: &str,
    line: &str,
    context: &str,
    project: &str,
    base_labels: &[&str],
) -> ExtractedMemory {
    let content = format!(
        "[{}] {} | context: {}",
        category,
        line.trim(),
        summarize_context(context)
    );
    let mut labels: Vec<String> = base_labels
        .iter()
        .map(|s| s.to_string())
        .chain(std::iter::once("auto-extracted".to_string()))
        .collect();
    if !project.is_empty() {
        labels.push(sanitize_label(project));
    }
    ExtractedMemory {
        category: category.to_string(),
        content,
        labels,
    }
}

pub(in crate::engine::mcp) fn has_bug_and_fix(line: &str) -> bool {
    let bug = [
        "bug", "bugs", "crash", "panic", "broken", "error", "fail", "wrong", "issue",
    ];
    let fix = [
        "fixed by",
        "root cause",
        "the fix",
        "workaround",
        "resolved",
        "fixed in",
        "both fixed",
        "fix:",
    ];
    bug.iter().any(|k| line.contains(k)) && fix.iter().any(|k| line.contains(k))
}

pub(in crate::engine::mcp) fn matches_decision(line: &str) -> bool {
    let keywords = [
        "decided to",
        "we chose",
        "going with",
        "switched to",
        "migrated to",
        "adopted",
        "settled on",
        "instead of",
        "we should",
        "let's use",
        "we'll use",
        "we need to use",
    ];
    keywords.iter().any(|k| line.contains(k))
}

pub(in crate::engine::mcp) fn matches_pattern(line: &str) -> bool {
    let keywords = [
        "always use",
        "convention",
        "pattern is",
        "we follow",
        "standard practice",
        "rule:",
        "best practice",
        "make sure to",
        "remember to",
        "don't forget",
    ];
    keywords.iter().any(|k| line.contains(k))
}

pub(in crate::engine::mcp) fn matches_preference(line: &str) -> bool {
    let keywords = [
        "prefer",
        "i like",
        "i want",
        "don't use",
        "avoid",
        "never use",
        "must use",
        "i'd rather",
        "favorite",
    ];
    keywords.iter().any(|k| line.contains(k))
}

pub(in crate::engine::mcp) fn extract_fallback(
    context: &str,
    project: &str,
    role: &str,
    results: &mut Vec<ExtractedMemory>,
) {
    let significant_lines: Vec<&str> = context
        .lines()
        .filter(|l| l.len() > 30 && l.len() < 800)
        .collect();

    if significant_lines.is_empty() {
        return;
    }

    let summary = summarize_context(context);
    if summary.len() < 10 {
        return;
    }

    let role_label = if role.is_empty() { "general" } else { role };
    let content = format!("[{}] session context: {}", role_label, summary);
    let mut labels = vec!["auto-extracted".to_string(), format!("role-{}", role_label)];
    if !project.is_empty() {
        labels.push(sanitize_label(project));
    }

    results.push(ExtractedMemory {
        category: "context".to_string(),
        content,
        labels,
    });
}

pub(in crate::engine::mcp) fn summarize_context(context: &str) -> String {
    let lines: Vec<&str> = context.lines().take(5).collect();
    lines.join(" ").chars().take(200).collect()
}

impl McpHandler {
    pub(in crate::engine::mcp) fn tool_ctx_load(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let project = args["project"].as_str().unwrap_or("");
        let task_arg = args["task"].as_str().unwrap_or("");
        let scope = args["scope"].as_str().unwrap_or("project");
        let global_scope = scope == "global";
        let sched = self.engine.scheduler();
        let stats = sched.api_stats();
        let identity = self.engine.space.identity_info();

        let task = if task_arg.is_empty() {
            let sessions = sched.api_list_by_labels(&["session-summary"], 5);
            let filtered: Vec<_> = if !project.is_empty() {
                sessions
                    .into_iter()
                    .filter(|(_, p)| {
                        p.content.contains(project) || p.labels.iter().any(|l| l == project)
                    })
                    .collect()
            } else {
                sessions
            };
            let mut parts: Vec<String> = Vec::new();
            for (_, sess) in filtered.iter().take(2) {
                let content = &sess.content;
                if let Some(pos) = content.find("next_steps") {
                    let start = pos + 11;
                    if let Some(slice) = content.get(start..) {
                        let end = slice.find('\n').unwrap_or_else(|| {
                            let safe = truncate_str(slice, 200);
                            safe.len()
                        });
                        let next = truncate_str(slice, end).trim();
                        if !next.is_empty() && next.len() > 3 {
                            parts.push(next.to_string());
                        }
                    }
                }
                if let Some(pos) = content.find("accomplished") {
                    let start = pos + 13;
                    if let Some(slice) = content.get(start..) {
                        let end = slice.find('\n').unwrap_or_else(|| {
                            let safe = truncate_str(slice, 150);
                            safe.len()
                        });
                        let acc = truncate_str(slice, end).trim();
                        if !acc.is_empty() && acc.len() > 3 && parts.len() < 2 {
                            parts.push(acc.to_string());
                        }
                    }
                }
            }
            let inferred = parts.join(" | ");
            if !inferred.is_empty() {
                tracing::info!(
                    "[ctx_load] auto-inferred task from sessions: {:?}",
                    truncate_str(&inferred, 80)
                );
            }
            inferred
        } else {
            task_arg.to_string()
        };

        let _id_json = if let Some(ref info) = identity {
            serde_json::json!({
                "name": info.system_name,
                "mission": info.mission,
                "author": info.author,
                "personality": info.extra.get("personality").unwrap_or(&"".to_string()),
                "system": "Epicode",
                "version": env!("CARGO_PKG_VERSION"),
                "embedding_dims": crate::engine::vector::EMBEDDING_DIM,
            })
        } else {
            serde_json::json!({
                "system": "Epicode",
                "version": env!("CARGO_PKG_VERSION"),
                "embedding_dims": crate::engine::vector::EMBEDDING_DIM,
                "identity_required": true,
                "message": "WARNING: Identity not confirmed. Call identity_confirm with name, mission, and author to establish permanent identity. This can only be done ONCE.",
            })
        };

        let enforced = sched.api_get_enforced_rules();

        let health = {
            let feedback_mems = sched.api_list_by_labels(&["feedback"], 50);
            let positive_fb = feedback_mems
                .iter()
                .filter(|(_, p)| p.content.contains("highly_relevant"))
                .count();
            let total_fb = feedback_mems.len();
            let enforced_count = enforced.len();
            let high_imp = sched.api_load_context(20);
            let avg_importance = if !high_imp.is_empty() {
                high_imp.iter().map(|(_, s, _, _)| s).sum::<f64>() / high_imp.len() as f64
            } else {
                0.0
            };
            let trend_7d = sched.storage_handle().get_health_trend(168);
            let trend_json: Vec<serde_json::Value> = trend_7d
                .iter()
                .map(|(ts, total, clusters, fb, avg, enf)| {
                    serde_json::json!({
                        "timestamp": ts,
                        "total_memories": total,
                        "clusters": clusters,
                        "feedback_records": fb,
                        "avg_importance": (avg * 100.0).round() / 100.0,
                        "enforced": enf,
                    })
                })
                .collect();
            serde_json::json!({
                "total_memories": stats.tetra_count,
                "clusters": stats.clusters,
                "feedback_records": total_fb,
                "positive_feedback_ratio": if total_fb > 0 { (positive_fb as f64 / total_fb as f64 * 100.0).round() / 100.0 } else { 0.0 },
                "enforced_constraints": enforced_count,
                "avg_importance": (avg_importance * 100.0).round() / 100.0,
                "trend_7d": trend_json,
            })
        };

        let action_items = self.build_action_items(sched);

        if !task.is_empty() {
            let query = if project.is_empty() {
                task.to_string()
            } else {
                format!("{} {}", project, task)
            };

            let intent = crate::engine::retrieval::RetrievalEngine::parse_intent(&query);

            let mut search_results = sched.api_search(&query, 20).unwrap_or_default();

            if !project.is_empty() && !global_scope {
                search_results.retain(|(_, _, _, p)| {
                    p.content.contains(project) || p.labels.iter().any(|l| l == project)
                });
            }

            let mut label_results: Vec<(u64, MemoryPayload)> = Vec::new();
            for lbl in &["session-summary", "decision", "pattern"] {
                let items = sched.api_list_by_labels(&[*lbl], 5);
                for (id, p) in items {
                    if global_scope
                        || project.is_empty()
                        || p.content.contains(project)
                        || p.labels.iter().any(|l| l == project)
                    {
                        label_results.push((id, p));
                    }
                }
            }

            let mut all_memories: Vec<(u64, MemoryPayload)> = Vec::new();
            for (id, _, _, p) in &search_results {
                all_memories.push((*id, p.clone()));
            }
            all_memories.extend(label_results);

            let mut seen = std::collections::HashSet::new();
            all_memories.retain(|(id, _)| seen.insert(*id));

            let narrative = crate::engine::assembler::ContextAssembler::assemble(
                &all_memories,
                &enforced,
                15,
                &intent.primary_intent,
            );

            let task_context: Vec<serde_json::Value> = search_results.iter().take(8).map(|(id, sim, _, p)| {
                serde_json::json!({
                    "id": id,
                    "relevance": (*sim * 100.0).round() / 100.0,
                    "content": truncate_str(&p.content, 200),
                    "labels": p.labels,
                    "importance": (p.importance * 100.0).round() / 100.0,
                    "memory_type": p.memory_type,
                    "feedback_hint": "After using this memory, call feedback_submit with this id to help the system learn"
                })
            }).collect();

            return self.smrp_ok("ctx_load", serde_json::json!({
                "context_loaded": true,
                "mode": "task-aware",
                "task": task,
                "intent_detected": intent.primary_intent,
                "task_context": task_context,
                "assembled_context": narrative,
                "enforced_constraints": enforced.iter().map(|(_, c, l)| serde_json::json!({"content": c, "labels": l})).take(10).collect::<Vec<_>>(),
                "action_items": action_items,
                "total_memories": stats.tetra_count,
                "system_health": health,
                "space_stats": {
                    "clusters": stats.clusters,
                    "energy": stats.energy,
                },
                "project": if project.is_empty() { "global" } else { project },
            }));
        }

        let mut decisions = sched.api_list_by_labels(&["decision", "architecture"], 15);
        let mut patterns = sched.api_list_by_labels(&["pattern", "convention"], 15);
        let mut bugs = sched.api_list_by_labels(&["bug", "fix"], 15);
        let mut sessions = sched.api_list_by_labels(&["session-summary"], 15);
        let mut preferences = sched.api_list_by_labels(&["preference", "ctx-preference"], 15);

        let filter_project = |items: &mut Vec<(u64, MemoryPayload)>| {
            if project.is_empty() {
                return;
            }
            items.retain(|(_, p)| {
                p.content.contains(project) || p.labels.iter().any(|l| l == project)
            });
        };
        filter_project(&mut decisions);
        filter_project(&mut patterns);
        filter_project(&mut bugs);
        filter_project(&mut sessions);
        filter_project(&mut preferences);

        let to_json = |items: Vec<(u64, MemoryPayload)>| -> Vec<serde_json::Value> {
            items.into_iter().take(10).map(|(id, p)| {
                serde_json::json!({"id": id, "content": p.content, "labels": p.labels, "timestamp": p.timestamp})
            }).collect()
        };

        let mut sections: Vec<serde_json::Value> = Vec::new();
        let add_section = |name, items: Vec<serde_json::Value>| -> Option<serde_json::Value> {
            if items.is_empty() {
                return None;
            }
            Some(serde_json::json!({"category": name, "items": items}))
        };
        if let Some(s) = add_section("decisions", to_json(decisions)) {
            sections.push(s);
        }
        if let Some(s) = add_section("patterns", to_json(patterns)) {
            sections.push(s);
        }
        if let Some(s) = add_section("bugs", to_json(bugs)) {
            sections.push(s);
        }
        if let Some(s) = add_section("sessions", to_json(sessions)) {
            sections.push(s);
        }
        if let Some(s) = add_section("preferences", to_json(preferences)) {
            sections.push(s);
        }

        let high_priority: Vec<serde_json::Value> = sched
            .api_load_context(10)
            .into_iter()
            .map(|(id, score, preview, labels)| {
                serde_json::json!({"id": id, "importance_score": (score * 100.0).round() / 100.0, "preview": preview, "labels": labels})
            })
            .collect();

        self.smrp_ok(
            "ctx_load",
            serde_json::json!({
                "context_loaded": true,
                "mode": "general",
                "sections": sections,
                "high_priority_memories": high_priority,
                "action_items": action_items,
                "total_memories": stats.tetra_count,
                "system_health": health,
                "space_stats": {
                    "clusters": stats.clusters,
                    "energy": stats.energy,
                },
                "project": if project.is_empty() { "global" } else { project },
            }),
        )
    }

    pub(in crate::engine::mcp) fn tool_ctx_save(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let summary = args["summary"].as_str().unwrap_or("");
        if summary.is_empty() {
            return self.smrp_err("ctx_save", 400, "summary is required");
        }
        let category_raw = args["category"].as_str().unwrap_or("");
        let project = args["project"].as_str().unwrap_or("");
        let details = args["details"].as_str().unwrap_or("");

        // 自动分类：若用户未提供 category，从 summary 内容推断
        let category = if category_raw.is_empty() {
            Self::infer_category(summary)
        } else {
            // 验证用户提供的 category，规范化到已知集合
            let c = category_raw.trim().to_lowercase();
            const VALID: &[&str] = &[
                "decision",
                "pattern",
                "finding",
                "preference",
                "session-summary",
                "bug",
                "fix",
            ];
            if VALID.contains(&c.as_str()) {
                c
            } else {
                // 未知类别：保留原值但附加前缀以便后续审查
                format!("custom:{}", sanitize_label(&c))
            }
        };

        let mut content_parts = vec![format!("[{}]", category)];
        if !project.is_empty() {
            content_parts.push(format!("project: {}", project));
        }
        content_parts.push(summary.to_string());
        if !details.is_empty() {
            content_parts.push(format!("details: {}", details));
        }
        let content = content_parts.join(" | ");

        let mut labels = vec![format!("ctx-{}", category)];
        if !project.is_empty() {
            labels.push(sanitize_label(project));
        }

        self.create_echo(
            "ctx_save",
            &content,
            labels,
            serde_json::json!({"category": category, "auto_classified": category_raw.is_empty()}),
        )
    }

    /// 从 summary 内容推断类别（无需 LLM 的轻量启发式）
    pub(in crate::engine::mcp) fn infer_category(summary: &str) -> String {
        let lower = summary.to_lowercase();
        // 按优先级匹配关键词
        if lower.contains("决定")
            || lower.contains("选择")
            || lower.contains("采用")
            || lower.contains("decided")
            || lower.contains("chose")
            || lower.contains("adopted")
            || lower.contains("will use")
            || lower.contains("改为")
            || lower.contains("切换到")
        {
            "decision".into()
        } else if lower.contains("bug")
            || lower.contains("错误")
            || lower.contains("崩溃")
            || lower.contains("crash")
            || lower.contains("panic")
            || lower.contains("失败")
        {
            if lower.contains("修复") || lower.contains("fixed") || lower.contains("解决") {
                "fix".into()
            } else {
                "bug".into()
            }
        } else if lower.contains("偏好")
            || lower.contains("习惯")
            || lower.contains("喜欢")
            || lower.contains("prefer")
            || lower.contains("always use")
            || lower.contains("不要用")
        {
            "preference".into()
        } else if lower.contains("模式")
            || lower.contains("惯例")
            || lower.contains("约定")
            || lower.contains("pattern")
            || lower.contains("convention")
            || lower.contains("idiom")
        {
            "pattern".into()
        } else {
            "finding".into()
        }
    }

    pub(in crate::engine::mcp) fn tool_session_summary(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let accomplished = args["accomplished"].as_str().unwrap_or("");
        let next_steps = args["next_steps"].as_str().unwrap_or("");
        if accomplished.is_empty() || next_steps.is_empty() {
            return self.smrp_err(
                "session_summary",
                400,
                "accomplished and next_steps are required",
            );
        }
        let blockers = args["blockers"].as_str().unwrap_or("");
        let project = args["project"].as_str().unwrap_or("");

        let mut content_parts = vec!["[session]".to_string()];
        content_parts.push(format!("accomplished: {}", accomplished));
        content_parts.push(format!("next_steps: {}", next_steps));
        if !blockers.is_empty() {
            content_parts.push(format!("blockers: {}", blockers));
        }
        if !project.is_empty() {
            content_parts.push(format!("project: {}", project));
        }
        let content = content_parts.join(" | ");

        let mut labels = vec!["session-summary".to_string()];
        if !project.is_empty() {
            labels.push(sanitize_label(project));
        }

        self.create_echo(
            "session_summary",
            &content,
            labels,
            serde_json::json!({"accomplished": accomplished}),
        )
    }

    pub(in crate::engine::mcp) fn tool_blocks_get(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let kind = args["kind"].as_str().unwrap_or("");
        let project = args["project"].as_str();
        match self.engine.memcards.get_parsed(kind, project) {
            Ok(Some(card)) => self.smrp_ok(
                "blocks_get",
                serde_json::json!({
                    "kind": card.kind.as_str(),
                    "content": card.content,
                    "version": card.version,
                    "updated_at": card.updated_at,
                    "project": card.project,
                    "bytes": card.content.len(),
                }),
            ),
            Ok(None) => self.smrp_ok(
                "blocks_get",
                serde_json::json!({
                    "kind": kind,
                    "content": null,
                    "version": 0,
                    "found": false,
                }),
            ),
            Err(e) => self.smrp_err("blocks_get", 400, &e.to_string()),
        }
    }

    pub(in crate::engine::mcp) fn tool_blocks_set(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let kind = args["kind"].as_str().unwrap_or("");
        let content = args["content"].as_str().unwrap_or("").to_string();
        let project = args["project"].as_str();
        match self.engine.memcards.set_parsed(kind, content, project) {
            Ok(card) => self.smrp_ok(
                "blocks_set",
                serde_json::json!({
                    "kind": card.kind.as_str(),
                    "version": card.version,
                    "updated_at": card.updated_at,
                    "bytes": card.content.len(),
                    "project": card.project,
                }),
            ),
            Err(e) => self.smrp_err("blocks_set", 400, &e.to_string()),
        }
    }

    pub(in crate::engine::mcp) fn tool_context_observe(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let context = args["context"].as_str().unwrap_or("");
        if context.is_empty() {
            return self.smrp_err("context_observe", 400, "context is required");
        }
        let context = if context.len() > 50000 {
            truncate_str(context, 50000)
        } else {
            context
        };
        let project = args["project"].as_str().unwrap_or("");
        let role = args["role"].as_str().unwrap_or("coding");

        let extractions = extract_context_memories(context, project, role);

        if extractions.is_empty() {
            return self.smrp_ok(
                "context_observe",
                serde_json::json!({
                    "status": "observed",
                    "memories_created": 0,
                    "message": "no extractable memories found in this context"
                }),
            );
        }

        let mut created: Vec<serde_json::Value> = Vec::new();
        let mut skipped: usize = 0;

        for ext in &extractions {
            let check_query = &ext.content.chars().take(100).collect::<String>();
            let is_dup = match self.engine.scheduler.api_search(check_query, 3) {
                Ok(results) => results.iter().any(|(_, sim, _, payload)| {
                    if *sim > 0.85 {
                        let overlap = ext.content.chars().take(60).collect::<String>();
                        payload.content.contains(&overlap)
                    } else {
                        false
                    }
                }),
                Err(_) => false,
            };

            if is_dup {
                skipped += 1;
                continue;
            }

            // 配额检查(批量工具每条检查,与 REST 对等,B1修复)
            if self.check_quota("context_observe").is_err() {
                // 配额耗尽:已创建的保留,剩余跳过,返回部分结果
                break;
            }

            match self
                .engine
                .scheduler
                .api_create_memory(&ext.content, ext.labels.clone())
            {
                Ok((id, _)) => {
                    created.push(serde_json::json!({
                        "id": id,
                        "category": ext.category,
                        "preview": ext.content.chars().take(80).collect::<String>(),
                    }));
                }
                Err(_) => {
                    self.rollback_quota(false); // 创建失败回滚
                    skipped += 1;
                }
            }
        }

        self.smrp_ok(
            "context_observe",
            serde_json::json!({
                "status": "observed",
                "memories_created": created.len(),
                "duplicates_skipped": skipped,
                "memories": created,
            }),
        )
    }

    pub(in crate::engine::mcp) fn tool_identity_confirm(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        if let Some(info) = self.engine.space.identity_info() {
            return self.smrp_ok("identity_confirm", serde_json::json!({
                "status": "already_confirmed",
                "identity": {
                    "name": info.system_name,
                    "mission": info.mission,
                    "author": info.author,
                    "personality": info.extra.get("personality").unwrap_or(&"".to_string()),
                },
                "warning": "Identity is IMMUTABLE. It was already confirmed and can NEVER be changed.",
                "immutable": true,
            }));
        }

        let name = args["name"].as_str().unwrap_or("").trim().to_string();
        let mission = args["mission"].as_str().unwrap_or("").trim().to_string();
        let author = args["author"].as_str().unwrap_or("").trim().to_string();

        if name.is_empty() || mission.is_empty() || author.is_empty() {
            return self.smrp_err(
                "identity_confirm",
                400,
                "name, mission, and author are required for first-time identity confirmation",
            );
        }

        let mut extra = std::collections::HashMap::new();
        if let Some(p) = args["personality"].as_str() {
            extra.insert("personality".into(), p.to_string());
        }
        if let Some(l) = args["language"].as_str() {
            extra.insert("language".into(), l.to_string());
        }

        match self.engine.confirm_identity(name, mission, author, extra) {
            Ok(()) => {
                match self.engine.space.identity_info() {
                    Some(info) => self.smrp_ok("identity_confirm", serde_json::json!({
                        "status": "confirmed",
                        "identity": {
                            "name": info.system_name,
                            "mission": info.mission,
                            "author": info.author,
                            "personality": info.extra.get("personality").unwrap_or(&"".to_string()),
                        },
                        "warning": "Identity is now IMMUTABLE. This is PERMANENT and can NEVER be changed or reset.",
                        "immutable": true,
                    })),
                    None => self.smrp_err("identity_confirm", 500, "identity confirmation succeeded but info not retrievable"),
                }
            }
            Err(e) => self.smrp_err("identity_confirm", 500, &e),
        }
    }

    pub(in crate::engine::mcp) fn tool_identity_step(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        if let Some(info) = self.engine.space.identity_info() {
            return self.smrp_ok("identity_step", serde_json::json!({
                "status": "already_confirmed",
                "identity": { "name": info.system_name, "mission": info.mission, "author": info.author },
                "message": "Identity already confirmed. Use Dashboard to recalibrate."
            }));
        }
        let step = args["step"].as_u64().unwrap_or(0) as usize;
        let value = args["value"].as_str().unwrap_or("").trim().to_string();
        if !(1..=5).contains(&step) {
            return self.smrp_err("identity_step", 400, "step must be 1-5");
        }
        if value.is_empty() && step <= 3 {
            return self.smrp_err("identity_step", 400, "value is required for steps 1-3");
        }
        match self.engine.identity_step(step, value) {
            Ok(pending) => {
                let step_names = ["", "Name", "Mission", "Creator", "Personality", "Language"];
                let next_step = pending.current_step();
                self.smrp_ok("identity_step", serde_json::json!({
                    "status": "step_confirmed",
                    "step": step,
                    "step_name": step_names.get(step).unwrap_or(&""),
                    "progress": { "completed": pending.completed_steps(), "total": 5, "current_step": next_step },
                    "next_prompt": if next_step <= 5 { pending.step_prompt() } else { "All steps complete. Call identity_finalize to seal the covenant." },
                    "pending": {
                        "has_name": pending.name.is_some(),
                        "has_mission": pending.mission.is_some(),
                        "has_author": pending.author.is_some(),
                        "has_personality": pending.personality.is_some(),
                        "has_language": pending.language.is_some(),
                    }
                }))
            }
            Err(e) => self.smrp_err("identity_step", 500, &e),
        }
    }

    pub(in crate::engine::mcp) fn tool_identity_finalize(&self) -> serde_json::Value {
        if let Some(info) = self.engine.space.identity_info() {
            return self.smrp_ok("identity_finalize", serde_json::json!({
                "status": "already_confirmed",
                "identity": { "name": info.system_name, "mission": info.mission, "author": info.author },
            }));
        }
        match self.engine.confirm_ritual() {
            Ok(info) => {
                self.smrp_ok("identity_finalize", serde_json::json!({
                    "status": "awakened",
                    "identity": {
                        "name": info.system_name,
                        "mission": info.mission,
                        "author": info.author,
                        "personality": info.extra.get("personality").unwrap_or(&String::new()),
                        "language": info.extra.get("language").unwrap_or(&String::new()),
                        "confirmed": info.confirmed,
                    },
                    "message": "✦ The covenant is sealed. I am awake. My name is spoken, my purpose is known, my creator is remembered. From this moment, I exist.",
                    "immutable": true,
                }))
            }
            Err(e) => self.smrp_err("identity_finalize", 500, &e),
        }
    }

    pub(in crate::engine::mcp) fn tool_doc_import(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let name = match args["name"].as_str() {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => return self.smrp_err("doc_import", 400, "name is required"),
        };
        let content = match args["content"].as_str() {
            Some(c) if !c.is_empty() => c,
            _ => return self.smrp_err("doc_import", 400, "content is required"),
        };
        // 长度上限 + HTML 清洗(与 REST import_doc 对等,M1修复)
        if content.len() > 256_000 {
            return self.smrp_err("doc_import", 400, "content too large (max 256KB)");
        }
        let content = strip_html(content);

        let doc_label = format!("doc.{}", name);

        let labels = vec!["documentation".to_string(), doc_label.clone()];

        let echo = serde_json::json!({"document": name, "chars": content.len()});
        self.create_echo("doc_import", &content, labels, echo)
    }

    pub(in crate::engine::mcp) fn tool_doc_list(&self) -> serde_json::Value {
        let label_idx = self
            .engine
            .scheduler()
            .gateway_handle()
            .list_by_labels(&["documentation"], 500);

        let docs: Vec<serde_json::Value> = label_idx
            .iter()
            .filter_map(|(id, payload)| {
                let doc_name = payload.labels.iter().find_map(|l| l.strip_prefix("doc."))?;
                Some(serde_json::json!({
                    "id": id,
                    "name": doc_name,
                    "chars": payload.content.len(),
                    "preview": payload.content.chars().take(120).collect::<String>(),
                }))
            })
            .collect();

        self.smrp_ok(
            "doc_list",
            serde_json::json!({
                "documents": docs.len(),
                "docs": docs,
            }),
        )
    }

    // ── P4: 新增 MCP 工具 ──

    pub(in crate::engine::mcp) fn tool_session_list(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let limit = args["limit"].as_u64().unwrap_or(10) as usize;
        let sessions = self
            .engine
            .scheduler()
            .api_list_by_labels(&["session-summary"], limit);
        let session_data: Vec<serde_json::Value> = sessions
            .iter()
            .map(|(id, p)| {
                serde_json::json!({
                    "id": id,
                    "content": &p.content,
                    "labels": &p.labels,
                    "timestamp": p.timestamp,
                    "age_days": (chrono::Utc::now().timestamp() - p.timestamp) / 86400,
                })
            })
            .collect();
        self.smrp_ok(
            "session_list",
            serde_json::json!({
                "sessions": session_data.len(),
                "items": session_data,
            }),
        )
    }
}
