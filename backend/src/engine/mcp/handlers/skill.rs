use super::super::*;

impl McpHandler {
    pub(in crate::engine::mcp) fn tool_skill_get(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() {
            return self.smrp_err("skill_get", 400, "name is required");
        }
        if name.to_lowercase() == "epicode-system" {
            {
                let md = Self::system_manual();
                return self.smrp_ok("skill_get", serde_json::json!({
                    "name": "epicode-system", "skill_md": md, "version": Self::manual_version(&md),
                    "byte_size": md.len(),
                    "note": "canonical executor manual, compiled into the server (single source of truth)",
                }));
            }
        }
        let skills = self.engine.skills.list(None);
        let lname = name.to_lowercase();
        match skills
            .iter()
            .find(|sk| sk.name == name)
            .or_else(|| skills.iter().find(|sk| sk.name.to_lowercase() == lname))
        {
            Some(sk) => {
                // P14 技能生态闭环: 取用即计数(统计指标, 惰性持久化)
                self.engine.skills.increment_usage(sk.id);
                let result = serde_json::json!({
                    "name": sk.name, "skill_md": sk.skill_md, "version": sk.version,
                    "byte_size": sk.skill_md.len(),
                });
                self.smrp_ok("skill_get", result)
            }
            None => self.smrp_err("skill_get", 404, &format!("skill '{}' not found", name)),
        }
    }

    pub(in crate::engine::mcp) fn tool_pattern_learn(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let pattern = args["pattern"].as_str().unwrap_or("");
        if pattern.is_empty() {
            return self.smrp_err("pattern_learn", 400, "pattern is required");
        }
        let language = args["language"].as_str().unwrap_or("");
        let project = args["project"].as_str().unwrap_or("");
        let example = args["example"].as_str().unwrap_or("");
        let when = args["when"].as_str().unwrap_or("");
        let steps = args["steps"].as_str().unwrap_or("");
        let pitfalls = args["pitfalls"].as_str().unwrap_or("");
        let enforced = args["enforced"].as_bool().unwrap_or(false);

        let mut content_parts = vec!["[pattern]".to_string()];
        if !language.is_empty() {
            content_parts.push(format!("lang: {}", language));
        }
        if !project.is_empty() {
            content_parts.push(format!("project: {}", project));
        }
        content_parts.push(format!("rule: {}", pattern));
        if !when.is_empty() {
            content_parts.push(format!("when: {}", when));
        }
        if !steps.is_empty() {
            content_parts.push(format!("steps: {}", steps));
        }
        if !example.is_empty() {
            content_parts.push(format!("example: {}", example));
        }
        if !pitfalls.is_empty() {
            content_parts.push(format!("pitfalls: {}", pitfalls));
        }
        let content = content_parts.join(" | ");

        let mut labels = vec!["pattern".to_string(), "convention".to_string()];
        if !language.is_empty() {
            labels.push(format!("lang-{}", language));
        }
        if !project.is_empty() {
            labels.push(sanitize_label(project));
        }
        if enforced {
            labels.push("enforced".to_string());
        }

        let resp = self.create_echo(
            "pattern_learn",
            &content,
            labels,
            serde_json::json!({"pattern": pattern}),
        );
        if enforced {
            if let Some(id) = resp
                .get("data")
                .and_then(|d| d.get("id"))
                .and_then(|v| v.as_u64())
            {
                let _ = self.engine.space().update_enforced(id, true);
            }
        }
        resp
    }

    pub(in crate::engine::mcp) fn tool_pattern_recall(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let context = args["context"].as_str().unwrap_or("");
        if context.is_empty() {
            return self.smrp_err("pattern_recall", 400, "context is required");
        }
        let language = args["language"].as_str().unwrap_or("");
        let project = args["project"].as_str().unwrap_or("");

        let mut query = format!("pattern convention {}", context);
        if !language.is_empty() {
            query = format!("{} lang-{}", query, language);
        }
        if !project.is_empty() {
            query = format!("{} {}", query, project);
        }

        match self.engine.scheduler.api_search(&query, 10) {
            Ok(results) => {
                let items: Vec<serde_json::Value> = results
                    .into_iter()
                    .filter(|(_, sim, _, payload)| {
                        if *sim < 0.05 {
                            return false;
                        }
                        payload.labels.contains(&"pattern".to_string())
                            || payload.labels.contains(&"convention".to_string())
                            || payload.content.contains("[pattern]")
                    })
                    .take(10)
                    .map(|(id, sim, _, payload)| {
                        let mut structured = serde_json::json!({
                            "id": id,
                            "pattern": "",
                            "labels": payload.labels,
                            "similarity": (sim * 100.0).round() / 100.0,
                        });
                        let content = &payload.content;
                        let mut rule = String::new();
                        let mut when_val = String::new();
                        let mut steps_val = String::new();
                        let mut example_val = String::new();
                        let mut pitfalls_val = String::new();
                        for part in content.split(" | ") {
                            let part = part.trim();
                            if let Some(stripped) = part.strip_prefix("rule: ") {
                                rule = stripped.to_string();
                            } else if let Some(stripped) = part.strip_prefix("when: ") {
                                when_val = stripped.to_string();
                            } else if let Some(stripped) = part.strip_prefix("steps: ") {
                                steps_val = stripped.to_string();
                            } else if let Some(stripped) = part.strip_prefix("example: ") {
                                example_val = stripped.to_string();
                            } else if let Some(stripped) = part.strip_prefix("pitfalls: ") {
                                pitfalls_val = stripped.to_string();
                            }
                        }
                        if rule.is_empty() {
                            let raw: Vec<&str> = content
                                .split(" | ")
                                .filter(|p| {
                                    !p.starts_with("lang:")
                                        && !p.starts_with("project:")
                                        && !p.starts_with("[pattern]")
                                        && !p.starts_with("when:")
                                        && !p.starts_with("steps:")
                                        && !p.starts_with("example:")
                                        && !p.starts_with("pitfalls:")
                                })
                                .collect();
                            rule = raw.first().unwrap_or(&"").to_string();
                        }
                        structured["pattern"] = serde_json::json!(rule);
                        if !when_val.is_empty() {
                            structured["when"] = serde_json::json!(when_val);
                        }
                        if !steps_val.is_empty() {
                            structured["steps"] = serde_json::json!(steps_val);
                        }
                        if !example_val.is_empty() {
                            structured["example"] = serde_json::json!(example_val);
                        }
                        if !pitfalls_val.is_empty() {
                            structured["pitfalls"] = serde_json::json!(pitfalls_val);
                        }
                        structured
                    })
                    .collect();
                self.smrp_ok("pattern_recall", serde_json::json!({"patterns": items, "count": items.len(), "context": context}))
            }
            Err(e) => self.smrp_err("pattern_recall", 500, &e),
        }
    }

    pub(in crate::engine::mcp) fn tool_decision_record(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let title = args["title"].as_str().unwrap_or("");
        let chosen = args["chosen"].as_str().unwrap_or("");
        let rationale = args["rationale"].as_str().unwrap_or("");
        if title.is_empty() || chosen.is_empty() || rationale.is_empty() {
            return self.smrp_err(
                "decision_record",
                400,
                "title, chosen, and rationale are required",
            );
        }
        let alternatives = args["alternatives"].as_str().unwrap_or("");
        let project = args["project"].as_str().unwrap_or("");

        let mut content_parts = vec!["[decision]".to_string()];
        content_parts.push(format!("title: {}", title));
        content_parts.push(format!("chosen: {}", chosen));
        if !alternatives.is_empty() {
            content_parts.push(format!("rejected: {}", alternatives));
        }
        content_parts.push(format!("rationale: {}", rationale));
        if !project.is_empty() {
            content_parts.push(format!("project: {}", project));
        }
        let content = content_parts.join(" | ");

        let mut labels = vec!["decision".to_string(), "architecture".to_string()];
        if !project.is_empty() {
            labels.push(sanitize_label(project));
        }

        self.create_echo(
            "decision_record",
            &content,
            labels,
            serde_json::json!({"title": title, "chosen": chosen}),
        )
    }

    pub(in crate::engine::mcp) fn tool_bug_memory(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let symptoms = args["symptoms"].as_str().unwrap_or("");
        let root_cause = args["root_cause"].as_str().unwrap_or("");
        let fix = args["fix"].as_str().unwrap_or("");
        if symptoms.is_empty() || root_cause.is_empty() || fix.is_empty() {
            return self.smrp_err(
                "bug_memory",
                400,
                "symptoms, root_cause, and fix are required",
            );
        }
        let module = args["module"].as_str().unwrap_or("");
        let project = args["project"].as_str().unwrap_or("");

        let mut content_parts = vec!["[bug]".to_string()];
        content_parts.push(format!("symptoms: {}", symptoms));
        content_parts.push(format!("root_cause: {}", root_cause));
        content_parts.push(format!("fix: {}", fix));
        if !module.is_empty() {
            content_parts.push(format!("module: {}", module));
        }
        if !project.is_empty() {
            content_parts.push(format!("project: {}", project));
        }
        let content = content_parts.join(" | ");

        let mut labels = vec!["bug".to_string(), "fix".to_string()];
        if !module.is_empty() {
            labels.push(sanitize_label(module));
        }
        if !project.is_empty() {
            labels.push(sanitize_label(project));
        }

        self.create_echo(
            "bug_memory",
            &content,
            labels,
            serde_json::json!({"symptoms": symptoms}),
        )
    }

    pub(in crate::engine::mcp) fn tool_skill_execute(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let query = args["query"].as_str().unwrap_or("").trim();
        if query.is_empty() {
            return self.smrp_err("skill_execute", 400, "query is required");
        }

        let pub_skills = match &self.pub_skills {
            Some(ps) => ps,
            None => {
                return self.smrp_err("skill_execute", 503, "public skills store not available")
            }
        };

        let context = args["context"].as_str().unwrap_or("");
        let full_query = if context.is_empty() {
            query.to_string()
        } else {
            format!("{} {}", query, context)
        };

        let matched = pub_skills.match_skills(&full_query, "", 5);
        if matched.is_empty() {
            let all_skills = pub_skills.list_public();
            return self.smrp_ok(
                "skill_execute",
                serde_json::json!({
                    "status": "no_match",
                    "message": format!("No skills found matching '{}'", query),
                    "available_count": all_skills.len(),
                    "suggestion": "Try broader terms or browse the skills library"
                }),
            );
        }

        let best = &matched[0];
        pub_skills.increment_usage(best.id);

        let has_vector = pub_skills.has_vector();

        self.smrp_ok(
            "skill_execute",
            serde_json::json!({
                "status": "success",
                "search_method": if has_vector { "semantic" } else { "keyword" },
                "skill": {
                    "id": best.id,
                    "name": best.name,
                    "content": best.skill_md,
                    "version": best.version,
                    "owner": best.owner,
                    "category": best.category,
                    "usage_count": best.usage_count + 1,
                    "success_rate": best.success_rate,
                },
                "alternatives": matched.iter().skip(1).take(3).map(|sk| {
                    serde_json::json!({
                        "name": sk.name,
                        "id": sk.id,
                        "category": sk.category,
                    })
                }).collect::<Vec<_>>(),
                "total_matched": matched.len(),
            }),
        )
    }

    pub(in crate::engine::mcp) fn tool_skill_feedback(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        if args["skill_id"].is_null()
            || (!args["skill_id"].is_number() && !args["skill_id"].is_u64())
        {
            return self.smrp_err("skill_feedback", 400, "skill_id must be a positive integer");
        }
        let skill_id = args["skill_id"].as_u64().unwrap_or(0);
        if skill_id == 0 {
            return self.smrp_err("skill_feedback", 400, "skill_id must be a positive integer");
        }
        if args["helpful"].is_null() {
            return self.smrp_err(
                "skill_feedback",
                400,
                "helpful is required and must be a boolean",
            );
        }
        if !args["helpful"].is_boolean() {
            return self.smrp_err(
                "skill_feedback",
                400,
                "helpful must be a boolean (true/false)",
            );
        }
        let helpful = args["helpful"].as_bool().unwrap();

        let pub_skills = match &self.pub_skills {
            Some(ps) => ps,
            None => {
                return self.smrp_err("skill_feedback", 503, "public skills store not available")
            }
        };

        match pub_skills.record_feedback(skill_id, helpful) {
            Ok(()) => {
                if let Some(skill) = pub_skills.get(skill_id) {
                    tracing::info!(
                        "[SkillFeedback] id={} helpful={} success_rate={:.3} usage_count={}",
                        skill_id,
                        helpful,
                        skill.success_rate,
                        skill.usage_count
                    );
                }
                self.smrp_ok(
                    "skill_feedback",
                    serde_json::json!({"status": "success", "message": "feedback recorded"}),
                )
            }
            Err(e) => self.smrp_err("skill_feedback", 500, &e),
        }
    }

    /// Strip a leading YAML frontmatter block (--- ... ---) from a skill body.
    /// skills_sync regenerates its own frontmatter, so the embedded one must go.
    pub(in crate::engine::mcp) fn strip_frontmatter(md: &str) -> String {
        let t = md.trim_start();
        if !t.starts_with("---") {
            return t.to_string();
        }
        let mut closed = false;
        let body: Vec<&str> = t
            .lines()
            .skip(1)
            .filter(|l| {
                if closed {
                    true
                } else {
                    let lt = l.trim();
                    if lt.starts_with("---") && lt.trim_matches('-').is_empty() {
                        closed = true;
                    }
                    false
                }
            })
            .collect();
        if !closed {
            return t.to_string();
        }
        body.join("\n").trim_start_matches('\n').to_string()
    }

    pub(in crate::engine::mcp) fn tool_skills_sync(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let format = args["format"].as_str().unwrap_or("manifest");
        let all_skills = self.engine.skills.list(None);
        if all_skills.is_empty() {
            return self.smrp_ok(
                "skills_sync",
                serde_json::json!({
                    "status": "empty",
                    "message": "No skills in your private library",
                    "skills": []
                }),
            );
        }

        let slugify = |name: &str, md: &str| -> String {
            let from_name = regex_captures(name);
            if !from_name.is_empty() {
                return from_name;
            }
            let title = md.lines().next().unwrap_or("");
            let from_title = regex_captures(title);
            if !from_title.is_empty() {
                return from_title;
            }
            name.to_lowercase()
                .replace(&[':', '/', '\\'][..], "-")
                .split_whitespace()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("-")
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '-' {
                        c
                    } else {
                        '-'
                    }
                })
                .collect::<String>()
                .split('-')
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("-")
        };

        // 统一系统手册: 从文件动态注入(单一事实源), 不落各空间存储 —
        // 保证 skills_sync 是完整目录, 错过握手的智能体也能经库路径取到。
        let mut skills_data: Vec<serde_json::Value> = Vec::new();
        {
            let md = Self::system_manual();
            if format == "manifest" {
                skills_data.push(serde_json::json!({
                    "slug": "epicode-system", "name": "epicode-system", "version": Self::manual_version(&md),
                    "description": "Epicode 系统操作手册 — 统一系统技能(记忆I/O·时间效性·任务循环·交付标准·KG导航·上下文·对话)",
                    "is_system": true, "byte_size": md.len(),
                }));
            } else if format == "opencode" {
                let body = Self::strip_frontmatter(&md);
                let desc = body
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .map(|l| l.trim_start_matches('#').trim().to_string())
                    .unwrap_or_default()
                    .replace('\n', " ")
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"");
                let content = format!("---\nname: epicode-system\ndescription: \"Epicode skill - {}\"\nversion: 1.0.0\nis_system: true\n---\n\n{}", desc, body);
                skills_data.push(serde_json::json!({
                    "slug": "epicode-system", "filename": "SKILL.md", "content": content,
                    "name": "epicode-system",
                }));
            }
        }
        let skills_data_final: Vec<serde_json::Value> = {
            skills_data.extend(all_skills.iter().map(|sk| {
                let slug = slugify(&sk.name, &sk.skill_md);
                let body_md = Self::strip_frontmatter(&sk.skill_md);
                let first_line = || {
                    body_md
                        .lines()
                        .find(|l| !l.trim().is_empty())
                        .map(|l| l.trim_start_matches('#').trim().to_string())
                        .unwrap_or_else(|| sk.name.clone())
                };
                let description_yaml = first_line()
                    .replace('\n', " ")
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"");
                // S2: manifest描述真值化(触发描述优先, 缺失回退首行)
                let desc_real = sk.description.clone().unwrap_or_else(first_line);

                if format == "manifest" {
                    serde_json::json!({
                        "slug": slug,
                        "name": sk.name,
                        "version": sk.version,
                        "description": desc_real,
                        "is_system": sk.is_system,
                        "byte_size": body_md.len(),
                    })
                } else if format == "opencode" {
                    let mut frontmatter = format!(
                        "---\nname: {}\ndescription: \"Epicode skill - {}\"\nversion: {}\n",
                        slug, description_yaml, sk.version
                    );
                    if let Some(ref cat) = sk.category {
                        frontmatter.push_str(&format!("category: \"{}\"\n", cat));
                    }
                    if !sk.requires.is_empty() {
                        frontmatter.push_str(&format!(
                            "requires: [{}]\n",
                            sk.requires
                                .iter()
                                .map(|r| format!("\"{}\"", r))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                    }
                    if !sk.produces.is_empty() {
                        frontmatter.push_str(&format!(
                            "produces: [{}]\n",
                            sk.produces
                                .iter()
                                .map(|p| format!("\"{}\"", p))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                    }
                    if !sk.capabilities.is_empty() {
                        frontmatter.push_str(&format!(
                            "capabilities: [{}]\n",
                            sk.capabilities
                                .iter()
                                .map(|c| format!("\"{}\"", c))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                    }
                    frontmatter.push_str(&format!(
                        "success_rate: {:.2}\nusage_count: {}\ncreated_at: {}\n",
                        sk.success_rate, sk.usage_count, sk.created_at
                    ));
                    frontmatter.push_str("---\n\n");
                    let content = format!("{}{}", frontmatter, body_md);
                    serde_json::json!({
                        "slug": slug,
                        "filename": "SKILL.md",
                        "content": content,
                        "skill_id": sk.id,
                        "name": sk.name,
                    })
                } else if format == "json" {
                    serde_json::json!({
                        "slug": slug,
                        "filename": format!("{}.json", slug),
                        "content": serde_json::json!({
                            "name": sk.name,
                            "slug": slug,
                            "version": sk.version,
                            "category": sk.category,
                            "requires": sk.requires,
                            "produces": sk.produces,
                            "capabilities": sk.capabilities,
                            "success_rate": sk.success_rate,
                            "usage_count": sk.usage_count,
                            "skill_md": sk.skill_md,
                            "is_system": sk.is_system,
                            "created_at": sk.created_at,
                        }),
                        "skill_id": sk.id,
                        "name": sk.name,
                    })
                } else {
                    serde_json::json!({
                        "slug": slug,
                        "filename": format!("{}.md", slug),
                        "content": sk.skill_md,
                        "skill_id": sk.id,
                        "name": sk.name,
                    })
                }
            }));
            skills_data
        };

        let total = skills_data_final.len();
        // Agent UX: paginate by default so manifest does not dump hundreds of skills
        // into the agent context. limit=0 means unlimited (explicit escape hatch).
        let offset = args["offset"].as_u64().unwrap_or(0) as usize;
        let limit_arg = args.get("limit").and_then(|v| v.as_u64());
        let limit = match limit_arg {
            Some(0) => usize::MAX, // unlimited
            Some(n) => (n as usize).min(1000),
            None => 100, // default page size for all formats
        };
        let page: Vec<serde_json::Value> = skills_data_final
            .into_iter()
            .skip(offset)
            .take(if limit == usize::MAX {
                total.saturating_sub(offset)
            } else {
                limit
            })
            .collect();
        let returned = page.len();
        let truncated = offset + returned < total;
        let next_offset = if truncated {
            Some(offset + returned)
        } else {
            None
        };

        let mut payload = serde_json::json!({
            "status": "success",
            "total": total,
            "returned": returned,
            "offset": offset,
            "truncated": truncated,
            "skills": page,
        });
        if let Some(n) = next_offset {
            payload["next_offset"] = serde_json::json!(n);
            if format == "manifest" {
                payload["next_step"] = serde_json::json!(format!(
                    "Manifest truncated ({}/{}). Call skills_sync(offset:{}, limit:...) for more, or skill_get(name) for full content.",
                    returned, total, n
                ));
            }
        } else if format == "manifest" {
            payload["next_step"] = serde_json::json!(
                "Call skill_get(name) to fetch any skill's full content on demand."
            );
        }
        self.smrp_ok("skills_sync", payload)
    }

    pub(in crate::engine::mcp) fn tool_skill_auto_extract(
        &self,
        _args: &serde_json::Value,
    ) -> serde_json::Value {
        let result = self.engine.scheduler().api_extract_skill();
        self.smrp_ok("skill_auto_extract", result)
    }
}
