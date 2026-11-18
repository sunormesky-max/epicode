use super::super::*;

impl McpHandler {
    pub(in crate::engine::mcp) fn tool_epocode_handshake(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let agent_id = args
            .get("agent_id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let auto_install = args
            .get("authorization")
            .and_then(|a| a.get("auto_install"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let auto_update = args
            .get("authorization")
            .and_then(|a| a.get("auto_update"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let user_id = &self.engine.user_id;

        let grant = self.engine.storage.get_agent_grant(agent_id, user_id);
        let system_skill = Self::system_manual();

        let (status, next_action) = match &grant {
            None if !auto_install => ("authorization_required", Some("Read system_skill.content first (the canonical executor manual) — you can start working now. Optionally re-run epicode_handshake with authorization: {\"auto_install\": true, \"auto_update\": true} to record consent for manual auto-updates.")),
            _ => {
                let _ = self.engine.storage.upsert_agent_grant(agent_id, user_id, "epicode-system", &Self::manual_version(&system_skill), auto_install, auto_update);
                ("ready", None)
            }
        };

        let cards = self.engine.storage.load_knowledge_cards();
        let card_json: Vec<serde_json::Value> = cards.iter().take(5).map(|(d, s, _)| {
            serde_json::json!({"domain": d, "summary": s.chars().take(200).collect::<String>()})
        }).collect();

        // P5 挂起点名: 上次会话不辞而别的活跃任务, 这次握手站到眼前
        let unfinished = self.engine.storage.get_active_task_for_user(user_id).and_then(|tid| {
            self.engine.storage.get_task_session(&tid).map(|s| {
                let budget = s.get("budget_ms").and_then(|v| v.as_i64()).unwrap_or(1).max(1);
                let active = self.engine.storage.get_effective_active_ms(&tid);
                serde_json::json!({
                    "task_id": tid,
                    "description": s.get("description"),
                    "budget_minutes": budget / 60000,
                    "consumed_pct": (active as f64 / budget as f64 * 100.0).round().min(100.0) as i64,
                    "instruction": "恢复(task_status 无参)继续消化预算 / 或停放: 最后 checkpoint + task_alert(info) — 停放不烧预算(双钟), 不告而别会被每次握手点名",
                })
            })
        });

        // P36a 会话连续性点名: session_summary链断了/从未有过的agent, 每次握手被提醒(治平台级零调用)
        let sess_last = self
            .engine
            .scheduler()
            .api_list_by_labels(&["session-summary"], 1);
        let session_continuity = match sess_last.first() {
            Some((_, p)) => {
                let age_days = (chrono::Utc::now().timestamp() - p.timestamp).max(0) / 86400;
                serde_json::json!({
                    "last_summary_age_days": age_days,
                    "stale": age_days > 7,
                    "note": if age_days > 7 { format!("上次session_summary是{}天前 — 会话结束时调用session_summary(accomplished/next_steps)沉淀, 连续性是跨会话记忆的骨架", age_days) } else { String::new() },
                })
            }
            None => serde_json::json!({
                "last_summary_age_days": -1,
                "stale": true,
                "note": "你从未调用过session_summary — 每次会话结束沉淀一次(accomplished/next_steps), 它是你的跨会话连续性骨架",
            }),
        };
        // S2 实时更新感知: 全库内容版本戳 + 与该用户上次握手比对(变更即提示重sync)
        let skills_ver = self.engine.skills.content_version();
        let seen_key = format!("skills_ver_seen_{}", self.engine.user_id);
        let skills_updated = self
            .engine
            .storage
            .get_meta(&seen_key)
            .is_none_or(|v| v != skills_ver);
        let _ = self.engine.storage.set_meta(&seen_key, &skills_ver);
        let contract: Vec<serde_json::Value> = self
            .engine
            .scheduler()
            .api_get_enforced_rules()
            .into_iter()
            .take(5)
            .map(|(id, content, _)| serde_json::json!({"id": id, "rule": content}))
            .collect();
        let result = serde_json::json!({
            "status": status,
            "next_action": next_action,
            "skills_version": skills_ver,
            "skills_updated": skills_updated,
            "process_contract": if contract.is_empty() { serde_json::Value::Null } else { serde_json::json!(contract) },
            "unfinished_task": unfinished,
            "session_continuity": session_continuity,
            "read_first": "system_skill.content is the canonical executor manual (also skill_get(\"epicode-system\")); other skills are supplementary",
            "system_skill": {"name": "epicode-system", "version": Self::manual_version(&system_skill), "content": system_skill},
            "session_context": {"knowledge_cards": card_json, "identity": {"system": "Epicode"}},
            "action_tool_surface": {
                "planner": ["memory_propose_actions", "drive_to_actions", "action_list", "action_planner_status", "action_planner_run", "action_goal_create", "action_goal_prove", "action_goal_list", "action_control"],
                "worker": ["action_claim", "action_heartbeat", "action_complete", "task_start", "task_complete", "skill_execute"],
                "skill": "skill_get('动作角色分工')",
                "rule": "Planner proposes; Worker claims+completes with outcome write-back",
                "action_card_schema": "epicode.action_card/v2",
                "dispatch": {
                    "assign": true,
                    "ownership_enforce": true,
                    "requeue": true,
                    "tools": ["worker_register", "worker_unregister", "worker_list", "action_assign", "action_dispatch"],
                    "rule": "Scheduler assigns by capability+least-load; only assignee (or Admin) may complete; blocked/exhausted requeue then escalate"
                }
            },
        });
        self.smrp_ok_nn("epicode_handshake", result)
    }

    pub(in crate::engine::mcp) fn tool_dream_cycle(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let dry_run = args["dry_run"].as_bool().unwrap_or(false);
        match self.engine.scheduler.api_dream(dry_run) {
            Ok(report) => {
                let data = serde_json::json!({"report": report, "dry_run": dry_run});
                self.smrp_ok("dream_cycle", data)
            }
            Err(e) => self.smrp_err("dream_cycle", 500, &e),
        }
    }

    pub(in crate::engine::mcp) fn tool_feedback_submit(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        const MAX_FEEDBACK_IDS: usize = 32;
        const MAX_CONCEPT_LINKS: usize = 32;

        let mut ids: Vec<u64> = args["memory_ids"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_u64()).collect())
            .unwrap_or_default();
        if ids.is_empty() {
            return self.smrp_err(
                "feedback_submit",
                400,
                "memory_ids is required and must be non-empty",
            );
        }
        ids.truncate(MAX_FEEDBACK_IDS);
        // Stable unique order preserves first-seen ranking from search/recall.
        let mut seen = std::collections::HashSet::new();
        ids.retain(|id| seen.insert(*id));

        let relevance = args["relevance"].as_str().unwrap_or("irrelevant");
        let outcome = args["outcome"].as_str().unwrap_or("no_action_needed");
        let query = args["query"].as_str().unwrap_or("");
        let notes = args["notes"].as_str().unwrap_or("");
        let correction = args["correction"].as_str().unwrap_or("");

        let mass_delta = match relevance {
            "highly_relevant" => 0.15,
            "partially_relevant" => 0.05,
            _ => -0.05,
        };
        let outcome_bonus = match outcome {
            "task_completed" => 0.2,
            "task_partial" => 0.05,
            "task_failed" => -0.1,
            _ => 0.0,
        };

        let importance_delta = match relevance {
            "highly_relevant" => match outcome {
                "task_completed" => 0.3,
                "task_partial" => 0.15,
                _ => 0.1,
            },
            "partially_relevant" => 0.05,
            _ => -0.1,
        };

        let is_correction =
            correction == "outdated" || correction == "incorrect" || correction == "superseded";
        let is_restored = correction == "restored";
        let correction_importance = if is_correction {
            -0.8
        } else if is_restored {
            0.5
        } else {
            0.0
        };

        let total_delta = mass_delta + outcome_bonus;
        let mut affected = 0usize;
        let gateway = self.engine.scheduler.gateway_handle();
        for &id in &ids {
            let Some(before) = self.engine.space.get_tetrahedron(id) else {
                continue;
            };
            let old_labels = before.data.labels.clone();

            let _ = self.engine.space.update_mass(id, total_delta);
            if let Some(t) = self.engine.space.get_tetrahedron(id) {
                let _ = self
                    .engine
                    .scheduler
                    .storage_handle()
                    .update_mass(id, t.mass);
            }

            {
                let base = match self.engine.space.get_tetrahedron(id) {
                    Some(t) => t.data,
                    None => continue,
                };
                let mut payload = base.clone();
                let old_importance = payload.importance;
                let final_delta = importance_delta + correction_importance;
                payload.importance = (old_importance + final_delta)
                    .clamp(crate::engine::governor::IMPORTANCE_FLOOR, 5.0);
                if is_correction && !payload.labels.iter().any(|l| l == "outdated") {
                    payload.labels.push("outdated".to_string());
                }
                if is_restored {
                    payload
                        .labels
                        .retain(|l| l != "outdated" && l != "superseded");
                }
                let labels_changed = payload.labels != old_labels;
                let _ = self.engine.space.patch_payload(id, &base, payload.clone());
                let _ = self
                    .engine
                    .scheduler
                    .storage_handle()
                    .update_importance(id, final_delta);
                // 管道完整性：is_correction 和 is_restored 都改变标签，都需要持久化
                if labels_changed {
                    let _ = self
                        .engine
                        .scheduler
                        .storage_handle()
                        .update_labels(id, &payload.labels);
                    gateway.update_label_index(id, &old_labels, &payload.labels);
                }
                gateway.mark_dirty(id);
                tracing::info!(
                    "[Feedback] id={} importance {:.2} -> {:.2}{}",
                    id,
                    old_importance,
                    payload.importance,
                    if is_correction {
                        " [CORRECTED-outdated]"
                    } else {
                        ""
                    }
                );
            }

            affected += 1;
        }

        // Retrieval-quality → graph: co-retrieved highly_relevant memories reinforce
        // existing edges or form a light SimilarTo bridge so next PPR/multi_hop prefers them.
        let mut edges_reinforced = 0usize;
        let mut edges_created = 0usize;
        if matches!(relevance, "highly_relevant" | "partially_relevant") && ids.len() >= 2 {
            let kg = self.engine.scheduler.kg_handle();
            let strength = if relevance == "highly_relevant" {
                0.65
            } else {
                0.45
            };
            let mut pairs: Vec<(u64, u64)> = Vec::new();
            for w in ids.windows(2) {
                pairs.push((w[0], w[1]));
            }
            // Dense clique only for small result sets to keep O(n^2) bounded.
            if ids.len() <= 5 {
                for i in 0..ids.len() {
                    for j in (i + 1)..ids.len() {
                        let a = ids[i];
                        let b = ids[j];
                        if !pairs
                            .iter()
                            .any(|(x, y)| (*x == a && *y == b) || (*x == b && *y == a))
                        {
                            pairs.push((a, b));
                        }
                    }
                }
            }
            let before_hits: std::collections::HashMap<(u64, u64), u16> = kg
                .all_relations()
                .into_iter()
                .map(|r| {
                    let key = if r.source < r.target {
                        (r.source, r.target)
                    } else {
                        (r.target, r.source)
                    };
                    (key, r.hits)
                })
                .collect();
            for &(a, b) in &pairs {
                if a == b {
                    continue;
                }
                if self.engine.space.get_tetrahedron(a).is_none()
                    || self.engine.space.get_tetrahedron(b).is_none()
                {
                    continue;
                }
                let key = if a < b { (a, b) } else { (b, a) };
                let existed = before_hits.contains_key(&key);
                if existed {
                    kg.reinforce_edges(&[(a, b)]);
                    edges_reinforced += 1;
                } else {
                    kg.add_relation(
                        a,
                        b,
                        crate::engine::knowledge::RelationType::SimilarTo,
                        strength,
                    );
                    edges_created += 1;
                }
            }
        }

        if !notes.is_empty() || !query.is_empty() {
            let feedback_content = format!(
                "[feedback] query: {} | relevance: {} | outcome: {} | correction: {} | notes: {} | affected_ids: {:?}",
                query, relevance, outcome, correction, notes, ids
            );
            let labels = vec!["feedback".to_string(), "agent-signal".to_string()];
            if let Err(e) = self
                .engine
                .scheduler
                .api_create_memory(&feedback_content, labels)
            {
                tracing::debug!("[MCP] feedback memory creation failed: {}", e);
            }
        }

        tracing::info!(
            "[Feedback] relevance={} outcome={} ids={:?} mass_delta={:.3} importance_delta={:.3} affected={} edges_reinforced={} edges_created={}",
            relevance, outcome, ids, total_delta, importance_delta, affected, edges_reinforced, edges_created
        );

        // concept_link — schema uses from_id/to_id; accept legacy a/b for compatibility.
        let mut links_formed = 0usize;
        if let Some(links) = args["concept_links"].as_array() {
            for link in links.iter().take(MAX_CONCEPT_LINKS) {
                let id_a = link["from_id"].as_u64().or_else(|| link["a"].as_u64());
                let id_b = link["to_id"].as_u64().or_else(|| link["b"].as_u64());
                let relation = link["relation"].as_str().unwrap_or("similar");
                if let (Some(a), Some(b)) = (id_a, id_b) {
                    if a != b
                        && self.engine.space.get_tetrahedron(a).is_some()
                        && self.engine.space.get_tetrahedron(b).is_some()
                    {
                        let rel_type = match relation {
                            "contradicts" => crate::engine::knowledge::RelationType::Contradicts,
                            "precedes" => crate::engine::knowledge::RelationType::Precedes,
                            "contains" => crate::engine::knowledge::RelationType::Contains,
                            "related" => crate::engine::knowledge::RelationType::Related,
                            _ => crate::engine::knowledge::RelationType::SimilarTo,
                        };
                        tracing::info!("[Feedback] concept_link: {} --{:?}--> {}", a, rel_type, b);
                        self.engine
                            .scheduler
                            .kg_handle()
                            .add_relation(a, b, rel_type, 0.8);
                        links_formed += 1;
                    }
                }
            }
        }

        // design-draft §b: link feedback to recall(s); split surfaced vs adopted.
        let task_id = self
            .engine
            .storage
            .get_active_task_for_user(&self.engine.user_id);
        let mut recall_ids: Vec<String> = Vec::new();
        if let Some(rid) = args.get("recall_id").and_then(|v| v.as_str()) {
            if !rid.trim().is_empty() {
                recall_ids.push(rid.trim().to_string());
            }
        }
        if recall_ids.is_empty() {
            if let Some(ref tid) = task_id {
                recall_ids = self.engine.storage.recent_recall_ids(tid, 5);
            }
        }
        let returned_ids = self.engine.storage.recall_returned_ids(&recall_ids);
        let returned_set: std::collections::HashSet<u64> = returned_ids.iter().copied().collect();
        let surfaced: Vec<u64> = ids
            .iter()
            .copied()
            .filter(|id| returned_set.contains(id))
            .collect();
        let is_relevant = matches!(relevance, "highly_relevant" | "partially_relevant");
        let adopted: Vec<u64> = if is_relevant { ids.clone() } else { Vec::new() };
        let payload = serde_json::json!({
            "relevance": relevance,
            "outcome": outcome,
            "memory_ids": ids,
            "correction": if correction.is_empty() { serde_json::Value::Null } else { serde_json::json!(correction) },
            "notes_present": !notes.is_empty(),
        });
        self.log_outcome_event(
            "feedback",
            task_id.as_deref(),
            &recall_ids,
            &surfaced,
            &adopted,
            &payload,
        );

        self.smrp_ok(
            "feedback_submit",
            serde_json::json!({
                "status": "recorded",
                "affected_memories": affected,
                "mass_adjustment": total_delta,
                "importance_adjustment": importance_delta,
                "feedback_learned": !notes.is_empty() || !query.is_empty(),
                "concept_links_formed": links_formed,
                "edges_reinforced": edges_reinforced,
                "edges_created": edges_created,
                "recall_ids": recall_ids,
                "surfaced_ids": surfaced,
                "adopted_ids": adopted,
            }),
        )
    }

    // ═══ 时间效性行为常量清单(2026-08-30审计; 集中理由见 ops/time_constants.md) ═══
    // 利用率门: <0.30 挑战(饱和声明受审) / <0.70需自评 —— mcp.rs task_complete
    // 奖励带: [0.40,1.10]×q>=3→+3; 详见task_complete reward表
    // 反思硬门: budget>=30min须iteration_log
    // 流判定: V<0.05停滞 / η=P/V>=0.8且V>=0.4高效 / V>=0.6沉浸
    // 时钟漂移: >2000ms=drift(对时协议)
    // 评审: 0.5自评+0.5judge融合, max_tokens 1024, temp 0.0
    // 校准: quality>=3门控, 最近20样, 中位; 修正系数下限0.1
    // 自适应路线: 心跳间隙已env化; 流判定/奖励带待积累分布数据后定标

    /// 版本单一事实源: 从手册标题解析(如 "# ...手册 v1.6" -> "1.6"), 免硬编码失同步
    /// The canonical executor manual (served by epicode_handshake, skill_get
    /// "epicode-system" and skills_sync). Compiled in from the repo so every
    /// deployment serves exactly the reviewed text; the constitution is appended.
    pub fn system_manual() -> String {
        const MANUAL: &str = include_str!("../../../../system_skills/epicode_system_manual.md");
        const CONSTITUTION: &str = include_str!("../../../../system_skills/00_epicode_system.md");
        let constitution = CONSTITUTION
            .lines()
            .map(|l| {
                if l.starts_with("## ") {
                    format!("#{l}")
                } else {
                    l.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            .replacen("# Epicode 宪法", "## 附录 / Appendix: Epicode 宪法", 1);
        format!(
            "{}\n\n{}\n",
            MANUAL
                .replace("{{EPICODE_VERSION}}", env!("CARGO_PKG_VERSION"))
                .trim_end(),
            constitution.trim_end()
        )
    }

    pub(in crate::engine::mcp) fn manual_version(content: &str) -> String {
        content
            .lines()
            .next()
            .and_then(|l| l.split(" v").nth(1))
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "1.0.0".to_string())
    }

    pub(in crate::engine::mcp) fn tool_enforced_rules(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let project = args["project"].as_str().unwrap_or("");
        let rules = self.engine.scheduler().api_get_enforced_rules();
        let filtered: Vec<serde_json::Value> = rules.into_iter()
            .filter(|(_, content, labels)| {
                if project.is_empty() { return true; }
                labels.iter().any(|l| l.contains(project)) || content.contains(project)
            })
            .map(|(id, content, labels)| {
                serde_json::json!({"id": id, "pattern": content, "labels": labels})
            })
            .collect();
        let count = filtered.len();
        self.smrp_ok("enforced_rules", serde_json::json!({
            "rules": filtered,
            "count": count,
            "warning": "These rules are ENFORCED and MUST be followed as hard constraints. Inject them into system prompts as mandatory requirements."
        }))
    }

    pub(in crate::engine::mcp) fn tool_project_list(&self) -> serde_json::Value {
        let projects = self.engine.scheduler().api_list_projects();
        let items: Vec<serde_json::Value> = projects
            .into_iter()
            .map(|(name, count)| {
                let display_name = name.trim_start_matches("project:").to_string();
                serde_json::json!({"project": display_name, "memory_count": count})
            })
            .collect();
        self.smrp_ok(
            "project_list",
            serde_json::json!({
                "projects": items,
                "total": items.len()
            }),
        )
    }

    pub(in crate::engine::mcp) fn tool_embedding_diagnostic(&self) -> serde_json::Value {
        let tetras = self.engine.space().all_tetrahedrons();
        let total = tetras.len();
        let mut dim_counts: std::collections::HashMap<usize, usize> =
            std::collections::HashMap::new();
        let mut stale_ids: Vec<u64> = Vec::new();
        let mut zero_dim_ids: Vec<u64> = Vec::new();

        for t in &tetras {
            let dim = t.data.embedding.len();
            *dim_counts.entry(dim).or_insert(0) += 1;
            if dim != 0 && dim != crate::engine::vector::EMBEDDING_DIM {
                stale_ids.push(t.id);
            }
            if dim == 0 {
                zero_dim_ids.push(t.id);
            }
        }

        let correct = *dim_counts
            .get(&crate::engine::vector::EMBEDDING_DIM)
            .unwrap_or(&0);
        let empty = *dim_counts.get(&0).unwrap_or(&0);
        let stale = total - correct - empty;

        let status = if stale > 0 {
            "action_needed"
        } else if empty > 0 {
            "degraded"
        } else {
            "healthy"
        };

        let recommendation = if stale > 0 {
            "Call embedding_migrate to re-embed stale memories with the current model"
        } else if empty > 0 {
            "Some memories have no embedding (dim=0) and are invisible to vector search. Call embedding_migrate to generate embeddings."
        } else {
            "All embeddings are up to date"
        };

        self.smrp_ok(
            "embedding_diagnostic",
            serde_json::json!({
                "total_memories": total,
                "correct_dim": correct,
                "expected_dim": crate::engine::vector::EMBEDDING_DIM,
                "stale_embeddings": stale,
                "no_embedding": empty,
                "zero_dim_ids": zero_dim_ids.iter().take(50).collect::<Vec<_>>(),
                "zero_dim_count": zero_dim_ids.len(),
                "dimension_breakdown": dim_counts.into_iter()
                    .map(|(dim, count)| serde_json::json!({"dim": dim, "count": count}))
                    .collect::<Vec<_>>(),
                "stale_ids": stale_ids.iter().take(50).collect::<Vec<_>>(),
                "stale_id_count": stale_ids.len(),
                "status": status,
                "recommendation": recommendation,
            }),
        )
    }

    pub(in crate::engine::mcp) fn tool_embedding_migrate(&self) -> serde_json::Value {
        if self.engine.space().identity_info().is_none() {
            return self.smrp_err(
                "embedding_migrate",
                400,
                "Identity confirmation required before migration",
            );
        }

        let tetras = self.engine.space().all_tetrahedrons();
        let stale_count = tetras
            .iter()
            .filter(|t| {
                let dim = t.data.embedding.len();
                dim != 0 && dim != crate::engine::vector::EMBEDDING_DIM
            })
            .count();

        if stale_count == 0 {
            return self.smrp_ok("embedding_migrate", serde_json::json!({
                "status": "ok", "message": "No stale embeddings found. All memories are up to date.", "migrated": 0,
            }));
        }

        tracing::info!(
            "[MCP] embedding_migrate: re-embedding {} stale memories",
            stale_count
        );

        match self.engine.reindex_embeddings() {
            Ok(updated) => self.smrp_ok("embedding_migrate", serde_json::json!({
                "status": "success", "migrated": updated, "total_before": tetras.len(),
                "message": format!("Re-embedded {} memories with bge-m3 ({}-dim)", updated, crate::engine::vector::EMBEDDING_DIM),
            })),
            Err(e) => self.smrp_err("embedding_migrate", 500, &format!("Migration failed: {}", e)),
        }
    }
}
