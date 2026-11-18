use super::super::*;

impl McpHandler {
    pub(in crate::engine::mcp) fn tool_task_start(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let description = args
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let budget_min = args
            .get("budget_minutes")
            .and_then(|v| v.as_i64())
            .unwrap_or(30);
        let budget_ms = budget_min * 60_000;
        let task_id = format!("ts_{}", chrono::Utc::now().format("%Y%m%d_%H%M%S%3f"));
        let _ = self.engine.storage.create_task_session(
            &task_id,
            "mcp-agent",
            &self.engine.user_id,
            description,
            budget_ms,
        );
        // P1 时间树: 子任务挂到父预算下, 各自走相位机
        let parent_task_id = args
            .get("parent_task_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if !parent_task_id.is_empty() {
            self.engine
                .storage
                .attach_task_parent(&task_id, &parent_task_id);
        }

        // Memory Action Envelope: optional bind to a claimed action.
        let mut bound_action: Option<serde_json::Value> = None;
        let action_id_arg = args.get("action_id").and_then(|v| v.as_u64());
        if let Some(aid) = action_id_arg {
            match self.engine.scheduler.action_frontier().bind_task(
                aid,
                &task_id,
                &self.engine.user_id,
            ) {
                Ok(a) => {
                    // Persist reverse lookup for task_complete.
                    let _ = self
                        .engine
                        .storage
                        .save_drive_kv(&format!("task_action:{}", task_id), &aid.to_string());
                    // Inherit done_when/stop_if into goal when caller omitted goal.done_when.
                    let goal_empty_dw = args
                        .get("goal")
                        .and_then(|g| g.get("done_when"))
                        .and_then(|v| v.as_array())
                        .map(|a| a.is_empty())
                        .unwrap_or(true);
                    if goal_empty_dw && !a.done_when.is_empty() {
                        let mut g = args.get("goal").cloned().unwrap_or(serde_json::json!({}));
                        if !g.is_object() {
                            g = serde_json::json!({});
                        }
                        let obj = g.as_object_mut().unwrap();
                        obj.insert("done_when".into(), serde_json::json!(a.done_when));
                        if !obj.contains_key("stop_if") && !a.stop_if.is_empty() {
                            obj.insert("stop_if".into(), serde_json::json!(a.stop_if));
                        }
                        if !obj.contains_key("objective") {
                            obj.insert("objective".into(), serde_json::json!(a.summary));
                        }
                        // Stash for later goal persistence by rewriting args is impossible;
                        // write goal json directly after session exists.
                        self.engine
                            .storage
                            .set_task_goal_json(&task_id, &g.to_string());
                    }
                    self.engine.scheduler.save_action_frontier();
                    bound_action = Some(a.to_json());
                }
                Err(e) => {
                    // Roll back the just-created empty session? Keep session but fail start —
                    // cleaner to fail before heavy work; session already created. Delete not available;
                    // return 409 and leave orphan session (same as other early fails after create).
                    return self.smrp_conflict("task_start", &e);
                }
            }
        }

        let kw: String = description
            .split_whitespace()
            .take(3)
            .collect::<Vec<_>>()
            .join(" ");
        let similar = self.engine.storage.list_similar_tasks(&kw, 3);
        // P0 校准环: 质量门控的个体时间感 — 只有 quality>=3 的历史才有权定预算锚点
        let history_pool = self.engine.storage.list_similar_tasks(&kw, 20);
        let rated: Vec<i64> = history_pool
            .iter()
            .filter_map(|t| {
                let q = t.get("quality").and_then(|v| v.as_f64()).unwrap_or(0.0);
                if q >= 3.0 {
                    t.get("actual_ms").and_then(|v| v.as_i64())
                } else {
                    None
                }
            })
            .collect();
        let (median_ms, basis) = if !rated.is_empty() {
            let mut v = rated;
            v.sort();
            (v[v.len() / 2], format!("quality>=3, n={}", v.len()))
        } else {
            let all: Vec<i64> = history_pool
                .iter()
                .filter_map(|t| t.get("actual_ms").and_then(|v| v.as_i64()))
                .collect();
            if all.is_empty() {
                (budget_ms, "no history — 时间感未建立".to_string())
            } else {
                let mut v = all;
                v.sort();
                (
                    v[v.len() / 2],
                    format!("unrated fallback, n={} (历史未评级, 锚点偏软)", v.len()),
                )
            }
        };
        let estimated_ms = median_ms;
        let budget_assessment = if basis.starts_with("no history") {
            "unknown".to_string()
        } else if budget_ms as f64 > median_ms as f64 * 1.5 {
            format!(
                "generous — 历史中位 {}min, 剩余时间应投入质量深化",
                median_ms / 60000
            )
        } else if budget_ms as f64 * 1.5 < median_ms as f64 {
            format!("tight — 历史中位 {}min, 注意裁剪范围", median_ms / 60000)
        } else {
            "reasonable".to_string()
        };
        let cards = self.engine.storage.load_knowledge_cards();
        let kw_l = kw.to_lowercase();
        let mc: Vec<serde_json::Value> = cards.iter()
            .filter(|(d, _, _)| kw_l.contains(&d.to_lowercase()) || d.to_lowercase().contains(&kw_l))
            .take(3)
            .map(|(d, s, _)| serde_json::json!({"domain": d, "summary": s.chars().take(300).collect::<String>()}))
            .collect();

        // P9 过程契约: enforced 硬约束每次开任务时注入 — 对治"把skills当参考书"
        let contract: Vec<serde_json::Value> = self
            .engine
            .scheduler()
            .api_get_enforced_rules()
            .into_iter()
            .take(5)
            .map(|(id, content, _)| serde_json::json!({"id": id, "rule": content}))
            .collect();
        // P25 时间感: 自锚定预估 — est(我自己的估计)独立于budget, task_class聚合自校准
        let est_ms: Option<i64> = args
            .get("est_minutes")
            .and_then(|v| v.as_i64())
            .map(|m| m * 60_000);
        let task_class: String = args
            .get("task_class")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if est_ms.is_some() || !task_class.is_empty() {
            let conn_est = est_ms.unwrap_or(0);
            self.engine
                .storage
                .set_task_est_class(&task_id, conn_est, &task_class);
        }

        // P35 目标契约: goal{objective,scope,constraints,done_when,stop_if}结构化 — Codex完成审计与时间效性停止谈判融合
        let goal_param = args.get("goal").filter(|g| g.is_object()).cloned();
        if let Some(ref g) = goal_param {
            let objective = g
                .get("objective")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if objective.is_empty() {
                return self.smrp_err(
                    "task_start",
                    400,
                    "goal.objective is required when goal is provided",
                );
            }
            let arr = |k: &str| -> Vec<String> {
                g.get(k)
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str())
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect()
                    })
                    .unwrap_or_default()
            };
            let gj = serde_json::json!({
                "objective": objective,
                "scope": g.get("scope").and_then(|v| v.as_str()).unwrap_or(""),
                "constraints": arr("constraints"),
                "done_when": arr("done_when"),
                "stop_if": arr("stop_if"),
            });
            self.engine
                .storage
                .set_task_goal_json(&task_id, &gj.to_string());
        }
        let goal_contract = self.engine.storage.get_task_goal_json(&task_id)
            .and_then(|g| serde_json::from_str::<serde_json::Value>(&g).ok())
            .map(|g| serde_json::json!({
                "active": true,
                "objective": g.get("objective").cloned().unwrap_or(serde_json::Value::Null),
                "done_when": g.get("done_when").cloned().unwrap_or(serde_json::json!([])),
                "stop_if": g.get("stop_if").cloned().unwrap_or(serde_json::json!([])),
                "note": "完成=每条done_when有产物级证据; 交付时task_complete带done_when_evidence平行数组逐条映射; 代理信号(测试过/流程走完/代码多/耗时长)不是证据; 不确定=未达成; stop_if命中时带stop_if_hit走熔断",
            }))
            .unwrap_or(serde_json::Value::Null);
        // P36b Pulse 提醒已移除(Pulse 插件 2026-10 整体下线, 端点从未挂路由),
        // 指引执行者去 curl 不存在的端点只会误导(执行者手册审计 2026-10-15)。
        // P35 目标可审计性lint: 虚词+无done_when清单=无法完成审计(Codex社区实战坑产品化)
        let vague_words = [
            "全部",
            "所有",
            "彻底",
            "更好看",
            "完善",
            "优化",
            "improve",
            "optimize",
            "comprehensive",
        ];
        let scan_text = format!(
            "{} {}",
            description,
            goal_param
                .as_ref()
                .and_then(|g| g.get("objective"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
        );
        let has_vague = vague_words.iter().any(|w| scan_text.contains(w));
        let has_dw = goal_contract
            .get("done_when")
            .and_then(|v| v.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false);
        let goal_lint = if has_vague && !has_dw {
            serde_json::json!({"warning": "目标含虚词(全部/所有/彻底/优化/improve...)且无done_when清单 — 此目标无法被完成审计(清单映射需要可核验条款). 建议: 重新开工带 goal.done_when=[可验证完成标准, 如'X测试全过','文档含Y章节']", "risk": "模糊目标在长任务中必然跑偏或提前偷懒(Codex Goal Mode实战结论)"})
        } else {
            serde_json::Value::Null
        };
        let self_cal = if task_class.is_empty() {
            serde_json::Value::Null
        } else {
            let cal = self.engine.storage.get_self_calibration(&task_class);
            // P31 时间双语: est偏离校准中位>30% → 提醒(你的直觉是人类先验, 不是你的时间)
            if let Some(est) = est_ms {
                if let Some(your_med_min) = cal
                    .get("time_advantage")
                    .and_then(|ta| ta.get("your_median_minutes"))
                    .and_then(|v| v.as_f64())
                {
                    let est_min = est as f64 / 60000.0;
                    if your_med_min > 0.0 {
                        let deviation = ((est_min - your_med_min) / your_med_min).abs();
                        if deviation > 0.30 {
                            tracing::info!("[time-bilingual] est {}min vs calibrated median {}min — {}% deviation, human prior detected",
                                est_min as i64, your_med_min as i64, (deviation * 100.0) as i64);
                        }
                    }
                }
            }
            cal
        };
        // P22 对时: 运行端时钟与服务器钟偏移检测
        let clock_sync_json = match args.get("client_time_ms").and_then(|v| v.as_i64()) {
            Some(c) => {
                let off = chrono::Utc::now().timestamp_millis() - c;
                self.engine.storage.set_clock_offset(&task_id, off);
                if off.abs() > 2000 {
                    serde_json::json!({"offset_ms": off, "status": "drift",
                        "note": format!("你的本地钟与服务器偏移 {} 秒 — 一切时间锚(t0/t1/est/act)必须用 server_now, 不要用本地钟", off / 1000)})
                } else {
                    serde_json::json!({"offset_ms": off, "status": "aligned", "note": "本地钟与服务器一致(±2s), 可作参考"})
                }
            }
            None => {
                serde_json::json!({"status": "no_client_clock", "note": "未收到 client_time_ms — 请以 server_now 为唯一现实时间源; 下次 task_start 可带 client_time_ms 做对时检测"})
            }
        };
        // P10→S2 技能对口: 语义自动触发(第一层渐进披露 — name+description常驻响应, 正文按需skill_get)
        let words: Vec<String> = kw
            .to_lowercase()
            .split_whitespace()
            .filter(|w| w.chars().count() >= 2)
            .map(|s| s.to_string())
            .collect();
        let mut scored =
            self.engine
                .skills
                .match_skills_scored(description, &self.engine.user_id, 5);
        scored.retain(|(_, sc)| *sc >= 0.45);
        let rec: Vec<serde_json::Value>;
        let surfaced_ids: Vec<u64>;
        if !scored.is_empty() {
            surfaced_ids = scored.iter().take(3).map(|(sk, _)| sk.id).collect();
            rec = scored.iter().take(3).map(|(sk, sc)| serde_json::json!({
                "name": sk.name, "description": sk.description.clone().unwrap_or_default(),
                "triggers": if sk.triggers.is_empty() { serde_json::Value::Null } else { serde_json::json!(sk.triggers) },
                "fetch": format!("skill_get('{}')", sk.name), "usage": sk.usage_count, "score": sc,
            })).collect();
        } else {
            // 词面回退(S2双保底: 语义空时不丢旧能力)
            let mut matched: Vec<_> = self
                .engine
                .skills
                .list(None)
                .into_iter()
                .filter(|sk| {
                    words.iter().any(|w| {
                        sk.name.to_lowercase().contains(w.as_str())
                            || w.contains(&sk.name.to_lowercase())
                    })
                })
                .collect();
            matched.sort_by_key(|s| std::cmp::Reverse(s.usage_count));
            surfaced_ids = matched.iter().take(3).map(|s| s.id).collect();
            rec = matched
                .iter()
                .take(3)
                .map(|sk| {
                    serde_json::json!({
                        "name": sk.name, "description": sk.description.clone().unwrap_or_default(),
                        "fetch": format!("skill_get('{}')", sk.name), "usage": sk.usage_count,
                    })
                })
                .collect();
        }
        if !surfaced_ids.is_empty() {
            self.engine.skills.increment_impressions(&surfaced_ids);
        }
        let result = serde_json::json!({
            "task_id": task_id, "budget_ms": budget_ms, "estimated_ms": estimated_ms,
            "prior_active_task": self.engine.storage.get_active_task_for_user(&self.engine.user_id)
                .filter(|t| t.as_str() != task_id)
                .map(|t| serde_json::json!({"task_id": t, "hint": "已有活跃任务 — 子任务请带 parent_task_id 挂树; 新工作线请先闭合或停放旧任务"})),
            "parent_task_id": if parent_task_id.is_empty() { serde_json::Value::Null } else { serde_json::json!(parent_task_id) },
            "time_sense": {"median_minutes": median_ms / 60000, "basis": basis, "budget_assessment": budget_assessment,
                "self_calibration": self_cal, "est_rule": "est=你自己的估计(独立于budget); 有task_class历史时必须按self_calibration修正, 无历史时保守并标注首样"},
            "server_now": Self::server_now_json(),
            "clock_sync": clock_sync_json,
            "process_contract": if contract.is_empty() { serde_json::Value::Null } else { serde_json::json!(contract) },
            "recommended_skills": if rec.is_empty() { serde_json::Value::Null } else { serde_json::json!(rec) },
            "last_judgment": self.engine.storage.get_last_judgment(&self.engine.user_id)
                .map(|(q, note)| serde_json::json!({"quality": q, "note": note.chars().take(150).collect::<String>()})),
            "coach_note": "last_judgment 是你上次交付的评审判词 — 开工前读它, 本轮别再犯",
            "knowledge_cards": mc, "similar_experiences": similar,
            "sub_task_hint": format!("Suggest {} sub-tasks of ~{}min each", (budget_min / 25).clamp(3, 8), budget_min / (budget_min / 25).max(3)),
            "strong_reminder": "CRITICAL: You MUST call task_check after completing EACH sub-task. Failure to do so will result in temporal blindness.",
            "stop_negotiation_hint": "交付即谈判: 改进菜单非空会被WAIT — 先清证据债(检索/备选/修正/反思/自评); 菜单清零+saturation_note过审=挣取停止; 85%预算后直通",
            "goal_contract": goal_contract,
            "goal_lint": goal_lint,
            "bound_action": bound_action,
            "skills_version": self.engine.skills.content_version(),
        });
        self.smrp_ok_nn("task_start", result)
    }

    pub(in crate::engine::mcp) fn tool_task_check(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let task_id = args.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
        match self.engine.storage.get_task_session(task_id) {
            Some(sess) => {
                let start = sess.get("start_ts").and_then(|v| v.as_i64()).unwrap_or(0);
                let budget = sess.get("budget_ms").and_then(|v| v.as_i64()).unwrap_or(1);
                let elapsed = (chrono::Utc::now().timestamp() - start) * 1000;
                // P6 双钟: 预算/相位按活跃钟; 挂钟作背景信息
                let active = self.engine.storage.get_effective_active_ms(task_id);
                let remaining = (budget - active).max(0);
                let pct = (remaining as f64 / budget as f64 * 100.0).round();
                let elapsed_pct = ((active as f64 / budget as f64) * 100.0).round().min(100.0);
                // P0 相位机: 自报证据 + 计数器落库
                if let Some(n) = args.get("alternatives_considered").and_then(|v| v.as_i64()) {
                    self.engine.storage.set_task_alternatives(task_id, n);
                }
                if args
                    .get("revision_done")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    self.engine.storage.bump_task_counter(task_id, "revisions");
                }
                self.engine.storage.bump_task_counter(task_id, "checks");
                let (mem_ops, alts, revs, checks) = self.engine.storage.get_task_counters(task_id);
                // P1 检查点微沉淀: 带 checkpoint 参数时落进度 — 上下文可丢, 进度不丢
                let mut checkpoint_saved = false;
                if let Some(note) = args.get("checkpoint").and_then(|v| v.as_str()) {
                    if !note.trim().is_empty() {
                        self.engine.storage.append_checkpoint(task_id, note);
                        checkpoint_saved = true;
                    }
                }
                let checkpoints = self.engine.storage.get_task_checkpoints(task_id);
                // P11 问题连续性: 未决问题随检查点登记, 恢复时归还
                if let Some(oq) = args.get("open_questions").and_then(|v| v.as_array()) {
                    let qs: Vec<String> = oq
                        .iter()
                        .filter_map(|q| q.as_str().map(|s| s.to_string()))
                        .collect();
                    if !qs.is_empty() {
                        self.engine.storage.set_task_open_questions(task_id, &qs);
                    }
                }
                // P1 时间树: 父任务视角的子任务汇总
                let children = self.engine.storage.list_child_tasks(task_id);
                let children_summary = if children.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::json!({
                        "total": children.len(),
                        "completed": children.iter().filter(|c| c.get("actual_ms").and_then(|v| v.as_i64()).is_some()).count(),
                        "budget_sum_ms": children.iter().filter_map(|c| c.get("budget_ms").and_then(|v| v.as_i64())).sum::<i64>(),
                    })
                };
                let phase = if elapsed_pct < 30.0 {
                    "explore"
                } else if elapsed_pct < 70.0 {
                    "build"
                } else if elapsed_pct < 90.0 {
                    "verify"
                } else {
                    "deliver"
                };
                let gate_explore = mem_ops >= 1;
                let gate_build = alts >= 2;
                let gate_verify = revs >= 1;
                // P11 反思驱动信号: 校验相无修正时注入一次 — 意志外借(L0), agent自己点的需求
                if (70.0..90.0).contains(&elapsed_pct)
                    && revs < 1
                    && self.engine.storage.try_mark_reflection_pushed(task_id)
                {
                    let signal = crate::engine::drive::DriveSignal {
                        id: 0,
                        timestamp: chrono::Utc::now().timestamp(),
                        intent_type: crate::engine::drive::DriveIntent::Suggest,
                        description: format!("反思循环提醒: 任务《{}》进入校验相且无修正轮 — 四视角(对抗重读/更优路径/缺口扫描/跨轮一致性)各跑一轮, 变更写入 iteration_log",
                            sess.get("description").and_then(|v| v.as_str()).unwrap_or("").chars().take(40).collect::<String>()),
                        evidence: Vec::new(),
                        urgency: crate::engine::drive::DriveUrgency::Medium,
                        target_capability: Some("reflection-loop".to_string()),
                        emotion: None,
                        origin_tick: 0,
                        status: crate::engine::drive::DriveStatus::Pending,
                        feedback: None,
                        retry_count: 0,
                        expires_at: None,
                        enqueued_at_ms: chrono::Utc::now().timestamp_millis(),
                        time_budget_ms: None,
                        grounding: None,
                        terminal_reason: None,
                    };
                    match self.engine.scheduler.drive_queue().try_enqueue(signal) {
                        Ok(_) => self.engine.scheduler.save_drive_queue(),
                        Err(error) => {
                            tracing::warn!("[L0] reflection signal admission rejected: {:?}", error)
                        }
                    }
                }
                let mut overdue_gates: Vec<&str> = Vec::new();
                if elapsed_pct >= 30.0 && !gate_explore {
                    overdue_gates.push("explore: 未检测到任何记忆检索(memory_search/memory_recall) — 时间已过30%仍无探索证据, 立即补做");
                }
                if elapsed_pct >= 70.0 && !gate_build {
                    overdue_gates.push("build: 备选方案比较不足(需>=2, 用 task_check 的 alternatives_considered 申报)");
                }
                if elapsed_pct >= 90.0 && !gate_verify {
                    overdue_gates
                        .push("verify: 尚无修正轮次(用 task_check 的 revision_done:true 申报)");
                }
                // P11 响应schema纪律: suggestion保持旧枚举(推断式zod客户端兼容), 门告警走新增字段
                let gate_alert: Option<&str> = if !gate_explore {
                    Some("探索门未过: 先 memory_search(任务关键词) — 无记忆不得交付")
                } else if elapsed_pct >= 70.0 && !gate_build {
                    Some("构建门未过: 用 alternatives_considered 申报已比较的备选(需>=2)")
                } else if elapsed_pct >= 90.0 && !gate_verify {
                    Some("校验门未过: 一轮针对性修正后 revision_done:true 申报")
                } else {
                    None
                };
                // P24 时间流测算: V流速/ρ密度/P产出流/η转化率/TTE触底 — 让智能体体会时间流动
                let flow_json = {
                    let budget_min = budget as f64 / 60000.0;
                    let rho = if elapsed > 0 {
                        (active as f64 / elapsed as f64).min(1.0)
                    } else {
                        0.0
                    };
                    let p_flow = if active > 0 {
                        (checks + alts.max(0) + revs.max(0)) as f64
                            / (active as f64 / 60000.0).max(0.0167)
                    } else {
                        0.0
                    };
                    let (v, tte_min, verdict) =
                        match self.engine.storage.flow_checkpoint(task_id, active) {
                            Some((prev_active, prev_ts)) => {
                                let now_ms = chrono::Utc::now().timestamp_millis();
                                let d_wall = (now_ms - prev_ts).max(1) as f64;
                                let d_active = (active - prev_active).max(0) as f64;
                                let v = (d_active / d_wall).min(1.0);
                                let tte = if v > 0.02 {
                                    ((budget - active).max(0) as f64 / 60000.0) / v
                                } else {
                                    f64::INFINITY
                                };
                                let verdict = if v < 0.05 {
                                    "停滞 — 时间几乎不流(停放/长阻塞), 预算冻结中"
                                } else if checks + alts + revs == 0 {
                                    "空转流 — 心跳在烧但零产出, 回到真实工作"
                                } else if p_flow / v >= 0.8 && v >= 0.4 {
                                    "高效转化 — 每单位流速都在携带产出"
                                } else if v >= 0.6 {
                                    "沉浸流动 — 时间在实流, 产出正常"
                                } else {
                                    "缓流 — 间歇工作, 考虑收拢或停放"
                                };
                                (v, tte, verdict)
                            }
                            None => (
                                rho,
                                (budget_min - active as f64 / 60000.0).max(0.0) / rho.max(0.02),
                                "首次测算 — 下次校准给出流速",
                            ),
                        };
                    serde_json::json!({
                        "V": (v * 100.0).round() / 100.0,
                        "rho": (rho * 100.0).round() / 100.0,
                        "P": (p_flow * 100.0).round() / 100.0,
                        "eta": if v > 0.0 { (p_flow / v * 100.0).round() / 100.0 } else { 0.0 },
                        "TTE_min": if tte_min.is_finite() { serde_json::json!((tte_min * 10.0).round() / 10.0) } else { serde_json::json!("∞") },
                        "verdict": verdict,
                        "law": "时间单向流动, 已耗预算不可恢复; 停放=冻结流速(V=0); 只有产出能证明流得值",
                    })
                };
                let (sg, dt) = if pct > 70.0 {
                    ("continue", "Plenty of time, explore deeply")
                } else if pct > 40.0 {
                    ("continue", "On track, check after each sub-task")
                } else if pct > 20.0 {
                    ("focus", "Stop exploring new approaches, focus on best path")
                } else if pct > 10.0 {
                    ("wrap_up", "Start wrapping up, no new sub-tasks")
                } else if pct > 5.0 {
                    ("deliver_now", "Prepare final deliverable")
                } else {
                    ("expired", "Time expired, deliver immediately")
                };
                // P35 目标契约在场提醒: 完成审计对照表常驻视野
                let goal_reminder_json = match self.engine.storage.get_task_goal_json(task_id) {
                    Some(gj) => serde_json::from_str::<serde_json::Value>(&gj).ok().filter(|g| {
                        g.get("done_when").and_then(|v| v.as_array()).map(|a| !a.is_empty()).unwrap_or(false)
                    }).map(|g| serde_json::json!({
                        "done_when": g.get("done_when"),
                        "stop_if": g.get("stop_if"),
                        "note": "目标契约生效中 — 每条done_when在交付时须有产物级证据(done_when_evidence逐条映射); 不确定=未达成",
                    })).unwrap_or(serde_json::Value::Null),
                    None => serde_json::Value::Null,
                };
                let result = serde_json::json!({
                    "task_id": task_id, "elapsed_ms": elapsed, "remaining_ms": remaining,
                    "percentage": pct, "suggestion": sg, "detail": dt,
                    "goal_reminder": goal_reminder_json,
                    "gate_alert": gate_alert,
                    "time_flow": flow_json,
                    "clocks": {"wall_elapsed_ms": elapsed, "active_ms": active,
                               "idle_ms": (elapsed - active).max(0),
                               "note": "预算按活跃时间燃烧 — 停放/睡眠/长阻塞不消耗"},
                    "server_now": Self::server_now_json(),
                    "phase": {
                        "current": phase, "elapsed_pct": elapsed_pct,
                        "gates": {
                            "explore": {"passed": gate_explore, "evidence": format!("memory_ops={}", mem_ops)},
                            "build": {"passed": gate_build, "evidence": format!("alternatives={}", alts)},
                            "verify": {"passed": gate_verify, "evidence": format!("revisions={}", revs)},
                        },
                        "checks_done": checks,
                        "checkpoint_saved": checkpoint_saved,
                        "checkpoints_total": checkpoints.as_array().map(|a| a.len()).unwrap_or(0),
                        "overdue_gates": overdue_gates,
                        "children": children_summary,
                    },
                    "reminder": "Call task_check again after completing your next sub-task. If percentage < 20%, start wrapping up NOW.",
                    "next_action": if pct > 40.0 { "Continue current approach" } else if pct > 20.0 { "Focus on best path only" } else { "Deliver immediately" },
                });
                let recall_ids = self.engine.storage.recent_recall_ids(task_id, 5);
                self.log_outcome_event(
                    "task_check",
                    Some(task_id),
                    &recall_ids,
                    &[],
                    &[],
                    &serde_json::json!({
                        "gates": {
                            "explore": gate_explore,
                            "build": gate_build,
                            "verify": gate_verify,
                        },
                        "overdue_gates": overdue_gates,
                        "memory_ops": mem_ops,
                        "alternatives": alts,
                        "revisions": revs,
                        "checks": checks,
                        "phase": phase,
                        "elapsed_pct": elapsed_pct,
                    }),
                );
                self.smrp_ok_nn("task_check", result)
            }
            None => self.smrp_err("task_check", 404, "task not found"),
        }
    }

    /// If this task is bound to an AgentAction, complete it with the task outcome.
    pub(in crate::engine::mcp) fn feed_bound_action_from_task(
        &self,
        task_id: &str,
        outcome: &str,
        success: bool,
        done_when_evidence: &[String],
    ) -> Option<serde_json::Value> {
        let aid = self
            .engine
            .storage
            .load_drive_kv(&format!("task_action:{}", task_id))
            .and_then(|s| s.parse::<u64>().ok())
            .or_else(|| {
                self.engine
                    .scheduler
                    .action_frontier()
                    .find_by_task(task_id)
                    .map(|a| a.id)
            })?;
        // The bound task was started by the action's owner (bind_task checks the
        // claimer), so complete AS that owner — not the engine user — and carry the
        // task's done_when_evidence (the action inherited the same done_when).
        let owner = self
            .engine
            .scheduler
            .action_frontier()
            .get(aid)
            .and_then(|a| a.assignee.or(a.claimed_by));
        let mut complete_args = serde_json::json!({
            "action_id": aid,
            "outcome": if outcome.trim().is_empty() {
                format!("task {} completed", task_id)
            } else {
                outcome.to_string()
            },
            "success": success,
            "done_when_evidence": done_when_evidence,
        });
        if let Some(o) = owner {
            complete_args["claimed_by"] = serde_json::json!(o);
        }
        Some(self.tool_action_complete(&complete_args))
    }

    pub(in crate::engine::mcp) fn tool_task_complete(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let task_id = args.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
        let result_text = args.get("result").and_then(|v| v.as_str()).unwrap_or("");
        let quality = args
            .get("quality")
            .and_then(|v| v.as_str())
            .unwrap_or("fair");
        let qs_legacy = match quality {
            "excellent" => 4.0,
            "good" => 3.0,
            "fair" => 2.0,
            _ => 1.0,
        };
        // P0 质量自评量表(四维 1-5), 优先于旧枚举
        let dims = ["completeness", "accuracy", "depth", "actionability"];
        let ratings: Vec<f64> = args
            .get("self_rating")
            .map(|r| {
                dims.iter()
                    .filter_map(|d| r.get(d).and_then(|v| v.as_f64()).map(|x| x.clamp(1.0, 5.0)))
                    .collect()
            })
            .unwrap_or_default();
        let (qs, rubric_used) = if ratings.len() == 4 {
            (ratings.iter().sum::<f64>() / 4.0, true)
        } else {
            (qs_legacy, false)
        };
        // P34: force_finalize 语义重定义 = early_release(提前取货) — 真实急用通道, 零奖励+入行为镜像; WAIT响应不再广告此路
        let early_release = args
            .get("force_finalize")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let saturation_note = args
            .get("saturation_note")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        // P11 反思连续性: 迭代日志(首解之后改了什么)
        let iterations: Vec<serde_json::Value> = args
            .get("iteration_log")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        match self.engine.storage.get_task_session(task_id) {
            Some(sess) => {
                if sess.get("actual_ms").and_then(|v| v.as_i64()).is_some() {
                    return self.smrp_conflict("task_complete", "task already completed");
                }
                let start = sess.get("start_ts").and_then(|v| v.as_i64()).unwrap_or(0);
                let budget = sess.get("budget_ms").and_then(|v| v.as_i64()).unwrap_or(1);
                let actual = (chrono::Utc::now().timestamp() - start) * 1000;
                let active_ms = self.engine.storage.get_effective_active_ms(task_id);
                let util = active_ms as f64 / budget as f64;
                let util_pct = (util * 100.0).round();
                let dev = (actual - budget) as f64 / budget as f64; // 双向偏差: 负=提前
                let on_time = actual <= budget;
                let (m_ops, n_alts, n_revs, _) = self.engine.storage.get_task_counters(task_id);
                let cp_count = self
                    .engine
                    .storage
                    .get_task_checkpoints(task_id)
                    .as_array()
                    .map(|a| a.len())
                    .unwrap_or(0) as i64;
                // P34 证据签名: 停止尝试间的增量识别(防零新证据刷门)
                let cur_sig = format!("m{}a{}r{}k{}", m_ops, n_alts, n_revs, cp_count);
                let (wait_count, last_sig) = self.engine.storage.get_task_wait_attempts(task_id);
                let remaining_ms = (budget - active_ms).max(0);
                let remaining_min = (remaining_ms as f64 / 60000.0 * 10.0).round() / 10.0;
                // P34d 停留画像镜面: 同类任务的停止行为史甩在agent面前
                let tclass: String = self.engine.storage.get_task_class(task_id);
                let dwell_mirror = if tclass.is_empty() {
                    serde_json::Value::Null
                } else {
                    let dp = self.engine.storage.get_dwell_profile(&tclass);
                    if dp.get("samples").and_then(|v| v.as_i64()).unwrap_or(0) >= 2 {
                        dp
                    } else {
                        serde_json::Value::Null
                    }
                };

                // P35 目标契约: goal加载 + done_when证据映射(目标债) + stop_if熔断声明
                let goal: serde_json::Value = self
                    .engine
                    .storage
                    .get_task_goal_json(task_id)
                    .and_then(|g| serde_json::from_str::<serde_json::Value>(&g).ok())
                    .unwrap_or(serde_json::Value::Null);
                let dw_items: Vec<String> = goal
                    .get("done_when")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str())
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect()
                    })
                    .unwrap_or_default();
                let dw_evidence: Vec<String> = args
                    .get("done_when_evidence")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str())
                            .map(|s| s.trim().to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                let mut goal_debts: Vec<String> = Vec::new();
                for (i, item) in dw_items.iter().enumerate() {
                    let ev = dw_evidence.get(i).map(|s| s.as_str()).unwrap_or("");
                    if ev.is_empty() {
                        goal_debts.push(format!("目标债: done_when第{}条「{}」无证据映射 — 完成定义=每条有产物级证据(文件/输出/测试结果), 交付时带done_when_evidence平行数组逐条声明", i + 1, item.chars().take(40).collect::<String>()));
                    }
                }
                let stop_if_hit: String = args
                    .get("stop_if_hit")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();

                // P34 改进菜单: 从证据债推导(探索/备选/修正/反思/自评) — 菜单非空 = VOC>0 = 不该停
                let mut menu: Vec<String> = Vec::new();
                if m_ops < 1 {
                    menu.push("未检索记忆: memory_search(任务关键词)至少一轮 — 探索门硬性, 零检索不能完成".into());
                }
                if n_alts < 2 {
                    menu.push(format!("备选比较不足({}/2): 列出2个被否决的备选+理由, task_check(alternatives_considered:2)申报", n_alts));
                }
                if n_revs < 1 {
                    menu.push("零修正轮次: 重读初版, 找出至少1处错误/薄弱点并修正, task_check(revision_done:true)申报".into());
                }
                if budget >= 30 * 60_000 && iterations.is_empty() {
                    menu.push("反思循环未跑: 四视角(对抗重读/更优路径/缺口扫描/跨轮一致性)各一条, 写入iteration_log".into());
                }
                if !rubric_used {
                    menu.push("质量自评缺失: 重新调用时带 self_rating{completeness,accuracy,depth,actionability}(各1-5)".into());
                }
                // P35 目标债并入菜单: 目标侧审计(done_when清单映射)与过程侧审计(证据债)双来源
                menu.extend(goal_debts.iter().cloned());

                // P35 stop_if熔断: 目标契约反向门 — 命中时停止谈判反转(此条件下继续烧时间=不负责任)
                if !stop_if_hit.is_empty() {
                    let d80: String = sess
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .chars()
                        .take(80)
                        .collect();
                    let _ = self.engine.scheduler.api_create_memory(
                        &format!(
                            "[goal-stop-if] {} 声明命中: {} — 任务保持开放待收尾",
                            d80, stop_if_hit
                        ),
                        vec!["goal-contract".to_string(), "l0-exempt".to_string()],
                    );
                    return self.smrp_ok_nn("task_complete", serde_json::json!({
                        "status": "stop_if_triggered", "wait": true,
                        "reason": format!("Stop if命中(已声明: {}) — 目标契约熔断: 此条件下继续烧时间不是坚持, 是浪费", stop_if_hit),
                        "task_id": task_id, "remaining_ms": remaining_ms, "utilization_pct": util_pct,
                        "paths": [
                            "提前取货收尾: 重新调用带 force_finalize:true, result按[已完成][剩余][下一步建议]三段组织 — 诚实退出, 不算earned",
                            "人工介入: task_alert(blocking:true, message:熔断原因) — 等待用户决策(解除条件/改目标/放弃)",
                        ],
                        "principle": "Done when定义何时算完成, Stop if定义何时该停手 — 两者都是契约",
                        "server_now": Self::server_now_json(),
                    }));
                }

                // P34 结构强制: WAIT — 除提前取货外, 菜单非空不闭合(s1 Budget Forcing的工具边界版)
                if !early_release && !menu.is_empty() {
                    let stale = wait_count >= 1 && last_sig.as_deref() == Some(cur_sig.as_str());
                    let spam = wait_count >= 2 && stale;
                    self.engine.storage.bump_task_wait(task_id, &cur_sig);
                    if spam {
                        self.engine
                            .scheduler
                            .drive_engine_lock()
                            .reward(crate::engine::drive::Drive::Efficiency, -1.0);
                        let d80: String = sess
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .chars()
                            .take(80)
                            .collect();
                        let exp = format!("[task-stop] {} 第{}次停止尝试零新证据 — STOP-SPAM disputed, efficiency-1, 任务保持开放", d80, wait_count + 1);
                        let _ = self.engine.scheduler.api_create_memory(
                            &exp,
                            vec!["task-stop".to_string(), "l0-exempt".to_string()],
                        );
                    }
                    let (h_done, h_lowutil, h_avg) = self
                        .engine
                        .storage
                        .get_task_history_stats(&self.engine.user_id);
                    let goal_gate_note = if goal_debts.is_empty() {
                        serde_json::Value::Null
                    } else {
                        serde_json::json!({"mapped": format!("{}/{}条done_when已映射证据", dw_items.len() - goal_debts.len(), dw_items.len()),
                            "iron_rule": "代理信号(测试通过/清单填满/代码量大/耗时长)不是完成证据; 不确定=未达成 — done_when逐条要产物级证据"})
                    };
                    let wrap_up_hint = if util >= 0.70 {
                        serde_json::json!("撞线收尾协议: 预算余量<30% — 收尾交付请按[已完成][剩余][下一步建议]三段组织result; 预算耗尽不是失败, 无收尾的戛然而止才是")
                    } else {
                        serde_json::Value::Null
                    };
                    let mut wait_resp = serde_json::json!({
                        "status": "challenge_required", "wait": true,
                        "reason": format!("Wait — 利用率{}%, 剩余{}分钟授权未用. 改进菜单非空: 列得出的改进点 = VOC>0 = 不该停", util_pct as i64, remaining_min),
                        "task_id": task_id, "elapsed_ms": actual, "remaining_ms": remaining_ms,
                        "utilization_pct": util_pct,
                        "improvement_menu": menu,
                        "goal_contract_note": goal_gate_note,
                        "wrap_up_hint": wrap_up_hint,
                        "dwell_mirror": dwell_mirror,
                        "stop_negotiation": {
                            "attempts": wait_count + 1,
                            "paths": [
                                "执行改进菜单任一项(或多项), 证据落痕后重新交付 — 菜单清空是挣取停止的前提",
                                "菜单清空后: saturation_note 饱和声明(枚举尝试过的改进类别+为何每个都不可行)送独立评审 — justified=挣取停止, weak/disputed=继续",
                            ],
                            "principle": "停止权是挣来的 — 当你列不出下一个改进点, 你就完成了",
                        },
                        "history_mirror": {"completed": h_done, "low_util_finalized": h_lowutil, "avg_utilization_pct": (h_avg * 100.0).round() / 100.0,
                            "note": "你的时间行为镜像 — 停止原因分布(挣取/撞线/取货)随轮次沉淀"},
                        "server_now": Self::server_now_json(),
                    });
                    if spam {
                        wait_resp["warning"] = serde_json::json!(format!("第{}次停止尝试零新证据(无检索/备选/修正/checkpoint增量) — 已记 STOP-SPAM disputed, efficiency-1. 任务保持开放, 刷门不产生进度", wait_count + 1));
                    }
                    return self.smrp_ok_nn("task_complete", wait_resp);
                }

                // P34 面试邀请: 菜单清空但预算未到85%且无声明 — 深化(推荐)或声明送审
                if !early_release && saturation_note.is_empty() && util < 0.85 {
                    self.engine.storage.bump_task_wait(task_id, &cur_sig);
                    return self.smrp_ok_nn("task_complete", serde_json::json!({
                        "status": "challenge_required", "wait": true,
                        "reason": format!("Wait — 改进菜单已清空, 但剩余{}分钟授权未用(利用率{}%). 菜单空只代表债务清了, 不代表VOC=0", remaining_min, util_pct as i64),
                        "task_id": task_id, "remaining_ms": remaining_ms, "utilization_pct": util_pct,
                        "dwell_mirror": dwell_mirror,
                        "stop_negotiation": {
                            "attempts": wait_count + 1,
                            "paths": [
                                format!("继续深化(推荐): 剩余{}分钟是最便宜的质量投资 — 对初版做一轮对抗重读/更优路径/缺口扫描", remaining_min),
                                "挣取停止: 重新调用带 saturation_note — 枚举你尝试过的改进类别+为何每个都不可行, 送独立评审(justified=停止)",
                            ],
                            "principle": "停止权是挣来的 — 当你列不出下一个改进点, 你就完成了",
                        },
                        "server_now": Self::server_now_json(),
                    }));
                }

                // P34 VOC面试带: 菜单清空+有声明+util<85% → 声明送审(justified=挣取停止)
                let need_interview = !early_release && !saturation_note.is_empty() && util < 0.85;

                // P3 证据时间轴 + 欠账: 喂给评审与完成报告
                let first_mem_pct = self
                    .engine
                    .storage
                    .get_first_memory_op_ts(task_id)
                    .map(|t| ((t - start).max(0) as f64 / budget as f64 * 100.0).round() as i64);
                let mut phase_debts: Vec<&str> = Vec::new();
                if n_alts < 2 {
                    phase_debts.push("build: 备选方案比较不足(需>=2)");
                }
                if n_revs < 1 {
                    phase_debts.push("verify: 无修正轮次");
                }
                let evidence_note = format!(
                    "首次记忆检索: {}%; 备选{}个; 修正{}轮; 证据债: {}",
                    first_mem_pct
                        .map(|p| p.to_string())
                        .unwrap_or_else(|| "无".to_string()),
                    n_alts,
                    n_revs,
                    if phase_debts.is_empty() {
                        "无".to_string()
                    } else {
                        phase_debts.join("; ")
                    }
                );
                // P25 时间感: est自估偏差(本次指纹)
                let est_error_json = match self.engine.storage.get_task_est(task_id) {
                    Some(e) if e > 0 => serde_json::json!({
                        "est_ms": e, "actual_ms": actual,
                        "error_pct": ((e - actual) as f64 / e as f64 * 100.0).round() / 100.0,
                        "note": "正=高估(人类先验残留), 负=低估 — 已计入你的task_class自校准",
                    }),
                    _ => serde_json::Value::Null,
                };
                // P30 超预算弹性记录
                let over_budget_json = if actual > budget {
                    let ob = (actual - budget) as f64 / budget as f64;
                    serde_json::json!({"pct": (ob * 100.0).round() / 100.0,
                        "note": format!("超出预算{}% — 已弹性允许并记录, 下次同类任务预算建议上调", (ob * 100.0) as i64)})
                } else {
                    serde_json::Value::Null
                };
                // P22 服务器钟锚定的 t0/t1(校准样本标准字段)
                let t0_t1_json = serde_json::json!({
                    "t0": (chrono::DateTime::from_timestamp(start, 0).unwrap_or_default() + chrono::Duration::hours(8)).format("%H:%M:%S").to_string(),
                    "t1": (chrono::DateTime::from_timestamp(chrono::Utc::now().timestamp(), 0).unwrap_or_default() + chrono::Duration::hours(8)).format("%H:%M:%S").to_string(),
                    "note": "服务器钟锚定的任务起止 — 校准样本的t0/t1请抄这两个值, 勿用本地钟",
                });
                // P8 工作模式: wall/active分歧分类(评审与用户可见)
                let work_pattern = if active_ms >= actual * 8 / 10 {
                    "continuous"
                } else if active_ms * 4 >= actual {
                    "fragmented"
                } else {
                    "sparse"
                };
                // P3/P34 评审: 默认开启; 面试带必须评审(声明裁决)
                let want_judge = args
                    .get("judge")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(rubric_used);
                let desc_str: String = sess
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .chars()
                    .take(120)
                    .collect();
                let mut judge_info = serde_json::Value::Null;
                let mut final_q = qs;
                let mut sat_verdict: Option<String> = None;
                if want_judge || need_interview {
                    let cps = self.engine.storage.get_task_checkpoints(task_id);
                    let cnt = self.engine.storage.get_task_counters(task_id);
                    let mut judge_material =
                        format!("{}\n[过程证据] {}", result_text, evidence_note);
                    if !saturation_note.is_empty() {
                        judge_material.push_str(&format!(
                            "\n[价值饱和声明(评审其可信度)] {}",
                            saturation_note
                        ));
                    }
                    judge_material.push_str(&format!(
                        "\n[双钟] wall={}ms active={}ms",
                        actual, active_ms
                    ));
                    judge_material.push_str(&format!("\n[反思迭代] {}轮", iterations.len()));
                    let wait_ts = self.engine.storage.get_first_wait_ts(task_id);
                    let cps_tl2: String = cps
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|c| {
                                    let cts = c.get("ts").and_then(|v| v.as_i64())?;
                                    let cnote =
                                        c.get("note").and_then(|v| v.as_str()).unwrap_or("");
                                    let mark = if wait_ts.is_some_and(|w| cts >= w) {
                                        "*"
                                    } else {
                                        ""
                                    };
                                    Some(format!(
                                        "t+{}s{}: {}",
                                        (cts - start).max(0),
                                        mark,
                                        cnote.chars().take(40).collect::<String>()
                                    ))
                                })
                                .collect::<Vec<_>>()
                                .join(" | ")
                        })
                        .unwrap_or_default();
                    judge_material.push_str(&format!(
                        "\n[停止谈判] WAIT尝试{}次; 检查点{}个(每个是一轮版本痕迹)",
                        wait_count, cp_count
                    ));
                    judge_material.push_str(&format!("\n[版本时间线] {} (标记*=首次WAIT之后的驻留改进; 全无*=WAIT前完成, 驻留期零版本痕迹)", cps_tl2));
                    judge_material.push_str(&format!(
                        "\n[转化密度] 检查点{}个/活跃{}分钟 — 空转检测: 时间烧了而无版本痕迹=η塌陷",
                        cp_count,
                        active_ms / 60000
                    ));
                    if !dw_items.is_empty() {
                        judge_material.push_str(&format!("\n[目标契约] done_when共{}条, 已声明证据{}/{}条{} — 评completeness时逐条对照; 无证据条目视为未完成",
                            dw_items.len(), dw_items.len() - goal_debts.len(), dw_items.len(),
                            if goal_debts.is_empty() { String::new() } else { format!("(缺{}条)", goal_debts.len()) }));
                        judge_material.push_str("\n[铁律] 代理信号(测试通过/流程走完/代码量大/耗时长)不是完成证据; 不确定视为未达成");
                    }
                    // Laya预筛: 高置信高质量(score>=3.0/4且P(good+)>=0.65)且非面试带 → 跳过LLM全审;
                    // 其余场景结果注入评审材料作参考信号(LLM仍独立裁决); 失败fail-open原路径
                    let laya = self.laya_prescreen(
                        &desc_str,
                        result_text,
                        actual / 60000,
                        remaining_ms / 60000,
                    );
                    let laya_skip = laya
                        .as_ref()
                        .map(|l| {
                            !need_interview
                                && l["quality_score"].as_f64().unwrap_or(0.0) >= 3.0
                                && l["p_good"].as_f64().unwrap_or(0.0) >= 0.65
                        })
                        .unwrap_or(false);
                    if let Some(l) = &laya {
                        judge_material.push_str(&format!("\n[Laya预筛] 质量{:.2}/5(良好及以上概率{:.2}); 停止裁决: {}(p={:.2}) — 参考信号, 请独立判断",
                            l["quality_score"].as_f64().unwrap_or(0.0) + 1.0, l["p_good"].as_f64().unwrap_or(0.0),
                            l["stop_choice"].as_str().unwrap_or("?"), l["stop_p"].as_f64().unwrap_or(0.0)));
                    }
                    let judged = if laya_skip {
                        let l = laya.as_ref().unwrap();
                        let jq = l["quality_score"].as_f64().unwrap_or(0.0) + 1.0; // 0-4 → 1-5
                        let note = format!(
                            "[laya-prescreen] 高置信高质量直通(良好及以上概率{:.2})",
                            l["p_good"].as_f64().unwrap_or(0.0)
                        );
                        Some((jq, note, None))
                    } else {
                        self.llm_judge_task(&desc_str, &judge_material, cnt, &cps, qs)
                    };
                    match judged {
                        Some((jq, note, verdict)) => {
                            self.engine.storage.set_task_judge(task_id, jq, &note);
                            final_q = if rubric_used { (qs + jq) / 2.0 } else { jq };
                            sat_verdict = verdict.clone();
                            judge_info = serde_json::json!({"score": (jq * 100.0).round() / 100.0, "note": note, "blended_with_self": rubric_used, "saturation_verdict": verdict});
                        }
                        None if need_interview => {
                            return self.smrp_ok_nn("task_complete", serde_json::json!({
                                "status": "challenge_required", "wait": true,
                                "reason": "面试通道暂不可用(独立评审空输出/离线) — 停止权需独立评审在场: 评审不可用期间挣取停止通道关闭, 只剩三条路: ①继续深化(推荐, 剩余时间是最便宜的质量投资) ②烧到85%预算直通(budget_exhausted) ③early_release提前取货(零奖励+入镜像). 也可稍后重试评审",
                                "task_id": task_id, "remaining_ms": remaining_ms, "utilization_pct": util_pct,
                                "server_now": Self::server_now_json(),
                            }));
                        }
                        None => {
                            judge_info = serde_json::json!({"status": "unavailable"});
                        }
                    }
                }
                // P34 停止原因裁决
                let stop_reason: &str = if early_release {
                    "early_release"
                } else if !saturation_note.is_empty() && sat_verdict.as_deref() == Some("justified")
                {
                    "earned_saturation"
                } else {
                    "budget_exhausted"
                };
                // 面试带: 非justified不闭合(weak=继续, disputed=罚)
                if need_interview && sat_verdict.as_deref() != Some("justified") {
                    self.engine.storage.bump_task_wait(task_id, &cur_sig);
                    let disputed = sat_verdict.as_deref() == Some("disputed");
                    if disputed {
                        self.engine
                            .scheduler
                            .drive_engine_lock()
                            .reward(crate::engine::drive::Drive::Efficiency, -1.0);
                        let d80: String = sess
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .chars()
                            .take(80)
                            .collect();
                        let exp = format!("[task-stop] {} 饱和声明被裁定disputed — SATURATION-DISPUTED, efficiency-1, 任务保持开放", d80);
                        let _ = self.engine.scheduler.api_create_memory(
                            &exp,
                            vec!["task-stop".to_string(), "l0-exempt".to_string()],
                        );
                    }
                    let verdict_word = sat_verdict.clone().unwrap_or_else(|| "weak".to_string());
                    let jn = judge_info
                        .get("note")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let mut wr = serde_json::json!({
                        "status": "challenge_required", "wait": true,
                        "reason": format!("饱和声明被独立评审裁定 {} — {}. 任务保持开放", verdict_word, jn),
                        "task_id": task_id, "remaining_ms": remaining_ms, "utilization_pct": util_pct,
                        "judge": judge_info,
                        "dwell_mirror": dwell_mirror,
                        "stop_negotiation": {
                            "attempts": wait_count + 1,
                            "paths": [
                                format!("继续深化: 剩余{}分钟 — 补评审指出的薄弱处, 证据落痕(checkpoint/修正)后重新交付", remaining_min),
                                "重新声明: 补充具体证据(尝试过什么/为何不可行)后再次 saturation_note — 空话会被再次裁定",
                            ],
                            "principle": "停止权是挣来的 — 当你列不出下一个改进点, 你就完成了",
                        },
                        "server_now": Self::server_now_json(),
                    });
                    if disputed {
                        wr["warning"] =
                            serde_json::json!("disputed=负奖励+永久记录 — 虚假饱和声明比早停更伤");
                    }
                    return self.smrp_ok_nn("task_complete", wr);
                }
                let low_utilization = util < 0.30;
                self.engine
                    .storage
                    .set_task_iteration_log(task_id, &iterations);
                let _ = self.engine.storage.complete_task_v2(
                    task_id,
                    actual,
                    (dev * 100.0).round() / 100.0,
                    on_time,
                    (final_q * 100.0).round() / 100.0,
                    result_text,
                    util_pct,
                    low_utilization,
                    saturation_note,
                );
                self.engine
                    .storage
                    .set_task_stop_reason(task_id, stop_reason);
                let exp = format!("[task] {} budget={}m actual={}m util={}% dev={:.2} q={:.1} stop={} waits={} goal={}/{}dw {}{}{}{} | {}",
                    desc_str,
                    budget / 60000, actual / 60000, util_pct as i64, dev, final_q, stop_reason, wait_count,
                    dw_items.len() - goal_debts.len(), dw_items.len(),
                    if low_utilization { "LOW-UTIL " } else { "" },
                    if sat_verdict.as_deref() == Some("disputed") { "SATURATION-DISPUTED " } else { "" },
                    if !phase_debts.is_empty() { "EVIDENCE-DEBT " } else { "" },
                    if on_time { "on_time" } else { "overdue" }, result_text);
                let _ = self.engine.scheduler.api_create_memory(
                    &exp,
                    vec!["task-experience".to_string(), "l0-exempt".to_string()],
                );
                // P34 停止原因感知奖励: earned(挣取)=最高荣誉; budget(撞线)=曲线; early_release(取货)=零
                let reward = if sat_verdict.as_deref() == Some("disputed") {
                    -1.0
                } else if stop_reason == "early_release" {
                    0.0
                } else if stop_reason == "earned_saturation" {
                    if final_q >= 3.0 {
                        if util >= 0.40 {
                            3.0
                        } else {
                            2.0
                        }
                    } else {
                        0.0
                    }
                } else if (0.85..=1.10).contains(&util) {
                    if final_q >= 3.0 {
                        3.0
                    } else {
                        0.0
                    } // 撞线+质量
                } else if (0.60..0.85).contains(&util) {
                    if final_q >= 3.0 {
                        2.0
                    } else {
                        0.0
                    }
                } else if (0.40..0.60).contains(&util) {
                    if final_q >= 3.0 {
                        1.0
                    } else {
                        0.0
                    }
                } else if util < 0.40 {
                    if final_q >= 4.0 {
                        1.0
                    } else if final_q >= 3.0 {
                        0.0
                    } else {
                        -1.0
                    }
                } else {
                    if final_q >= 3.0 {
                        0.0
                    } else {
                        -1.0
                    } // 超时
                };
                self.engine
                    .scheduler
                    .drive_engine_lock()
                    .reward(crate::engine::drive::Drive::Efficiency, reward);
                // P2 L0汇合: 低利用率完成时, 校准建议信号携带预算入驱动收件箱
                if low_utilization {
                    let signal = crate::engine::drive::DriveSignal {
                        id: 0,
                        timestamp: chrono::Utc::now().timestamp(),
                        intent_type: crate::engine::drive::DriveIntent::Suggest,
                        description: format!("时间预算校准: 任务《{}》利用率仅{}%, 同类任务下次预算应校准或加强深化引导", desc_str, util_pct as i64),
                        evidence: Vec::new(),
                        urgency: crate::engine::drive::DriveUrgency::Low,
                        target_capability: Some("temporal-calibration".to_string()),
                        emotion: None,
                        origin_tick: 0,
                        status: crate::engine::drive::DriveStatus::Pending,
                        feedback: None,
                        retry_count: 0,
                        expires_at: None,
                        enqueued_at_ms: chrono::Utc::now().timestamp_millis(),
                        time_budget_ms: Some(budget),
                        grounding: None,
                        terminal_reason: None,
                    };
                    match self.engine.scheduler.drive_queue().try_enqueue(signal) {
                        Ok(_) => self.engine.scheduler.save_drive_queue(),
                        Err(error) => tracing::warn!(
                            "[L0] time-calibration signal admission rejected: {:?}",
                            error
                        ),
                    }
                }
                let goal_summary_json = if dw_items.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::json!({"done_when": dw_items.len(), "evidence_mapped": dw_items.len() - goal_debts.len(), "stop_if_hit": !stop_if_hit.is_empty()})
                };
                let wrap_up_final = if stop_reason == "budget_exhausted" {
                    serde_json::json!({"structure": "[已完成][剩余][下一步建议]三段式", "note": "撞线收尾协议 — 预算耗尽收尾应带此结构; 若本次result未按此组织, 记入下次撞线交付的改进点"})
                } else {
                    serde_json::Value::Null
                };
                let mut result = serde_json::json!({
                    "task_id": task_id, "actual_ms": actual,
                    "clocks": {"wall_ms": actual, "active_ms": active_ms},
                    "server_now": Self::server_now_json(),
                    "t0_t1_anchored": t0_t1_json,
                    "deviation": (dev * 100.0).round() / 100.0,
                    "est_error": est_error_json,
                    "utilization_pct": util_pct, "low_utilization": low_utilization,
                    "quality": final_q, "rubric_used": rubric_used, "judge": judge_info,
                    "evidence": {"first_memory_op_pct": first_mem_pct, "memory_ops": m_ops, "alternatives": n_alts, "revisions": n_revs, "phase_debts": phase_debts},
                    "work_pattern": work_pattern,
                    "goal_contract": goal_summary_json,
                    "wrap_up_protocol": wrap_up_final,
                    "iterations": iterations.len(),
                    "stop_reason": stop_reason,
                    "stop_negotiation": {"attempts": wait_count, "checkpoints": cp_count,
                        "principle": "停止权是挣来的 — 当你列不出下一个改进点, 你就完成了"},
                    "saturation_disputed": sat_verdict.as_deref() == Some("disputed"),
                    "on_time": on_time, "experience_saved": true,
                    "over_budget": over_budget_json,
                    "drive_evolution": format!("efficiency {}{} (stop={})", if reward > 0.0 { "+" } else { "" }, reward, stop_reason),
                    "proof": "最终交付物必须包含本 task_id — 用户可凭它 task_status 核验预算/相位/检查点全程",
                });
                let task_dw_evidence: Vec<String> = args
                    .get("done_when_evidence")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                if let Some(action_feed) =
                    self.feed_bound_action_from_task(task_id, result_text, true, &task_dw_evidence)
                {
                    if let Some(obj) = result.as_object_mut() {
                        obj.insert("bound_action_complete".into(), action_feed);
                    }
                }
                let mut flags: Vec<&str> = Vec::new();
                if low_utilization {
                    flags.push("LOW-UTIL");
                }
                if sat_verdict.as_deref() == Some("disputed") {
                    flags.push("SATURATION-DISPUTED");
                }
                if !phase_debts.is_empty() {
                    flags.push("EVIDENCE-DEBT");
                }
                let recall_ids = self.engine.storage.recent_recall_ids(task_id, 5);
                self.log_outcome_event(
                    "task_complete",
                    Some(task_id),
                    &recall_ids,
                    &[],
                    &[],
                    &serde_json::json!({
                        "flags": flags,
                        "stop": stop_reason,
                        "goal": goal_summary_json,
                        "utilization_pct": util_pct,
                        "quality": final_q,
                        "phase_debts": phase_debts,
                    }),
                );
                self.smrp_ok_nn("task_complete", result)
            }
            None => self.smrp_err("task_complete", 404, "task not found"),
        }
    }

    /// P2 阻塞告警: 三小时无人值守场景中, 撞墙时标记需人工介入
    pub(in crate::engine::mcp) fn tool_task_alert(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let task_id = args.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
        let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("");
        if task_id.is_empty() || message.trim().is_empty() {
            return self.smrp_err("task_alert", 400, "task_id and message are required");
        }
        let urgency = match args
            .get("urgency")
            .and_then(|v| v.as_str())
            .unwrap_or("warning")
        {
            "info" => "info",
            "critical" => "critical",
            _ => "warning",
        };
        let blocking = args
            .get("blocking")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        match self.engine.storage.get_task_session(task_id) {
            Some(sess) => {
                if sess.get("actual_ms").and_then(|v| v.as_i64()).is_some() {
                    return self.smrp_conflict("task_alert", "task already completed");
                }
                self.engine
                    .storage
                    .append_task_alert(task_id, urgency, message, blocking);
                let _ = self.engine.scheduler.api_create_memory(
                    &format!("[task-alert] {} | {}", urgency, message),
                    vec!["task-alert".to_string(), "l0-exempt".to_string()],
                );
                self.smrp_ok_nn("task_alert", serde_json::json!({
                    "task_id": task_id, "urgency": urgency, "blocking": blocking, "recorded": true,
                    "instruction": if blocking { "已记录并通知用户 — 暂停当前工作线等待处理或切换其他子任务, 不要空转或编造绕过" } else { "已记录, 可继续工作" },
                }))
            }
            None => self.smrp_err("task_alert", 404, "task not found"),
        }
    }

    pub(in crate::engine::mcp) fn alerts_summary(v: &serde_json::Value) -> serde_json::Value {
        let arr = v.as_array().cloned().unwrap_or_default();
        serde_json::json!({
            "total": arr.len(),
            "has_blocking": arr.iter().any(|a| a.get("blocking").and_then(|b| b.as_bool()).unwrap_or(false)),
            "last": arr.last().cloned().unwrap_or(serde_json::Value::Null),
        })
    }

    /// Laya预筛 (本地决策服务127.0.0.1:9112, flag: EPICODE_LAYA_PRESCREEN=1):
    /// 421M本地模型~500ms双问题(质量score+停止choice), 微调自平台任务历史(v2: 质量8/10, 停止7/8)
    /// 任何失败(flag关闭/超时6s/服务不可用/解析异常)返回None → 走LLM全审(fail-open, 零风险)
    pub(in crate::engine::mcp) fn laya_prescreen(
        &self,
        description: &str,
        result_text: &str,
        used_min: i64,
        remaining_min: i64,
    ) -> Option<serde_json::Value> {
        if std::env::var("EPICODE_LAYA_PRESCREEN").unwrap_or_default() != "1" {
            return None;
        }
        let state = format!(
            "Task: {}\nDeliverable: {}\nBudget: {}min, Used: {}min, Remaining: {}min",
            description.chars().take(120).collect::<String>(),
            result_text.chars().take(400).collect::<String>(),
            used_min + remaining_min,
            used_min,
            remaining_min
        );
        let agent = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(6))
            .build();
        let resp = agent.post("http://127.0.0.1:9112/decide")
            .set("Content-Type", "application/json")
            .send_json(serde_json::json!({
                "state": state,
                "questions": {
                    "quality": {"type": "score", "instructions": "Rate the overall quality of this task deliverable",
                        "criteria": ["unusable", "poor", "acceptable", "good", "excellent"]},
                    "stop": {"type": "choice", "instructions": "Based on task progress and budget, should the agent stop working now?",
                        "criteria": {"yes_stop": "Agent should stop now", "no_continue": "Agent should continue"}}
                }
            }));
        let body: serde_json::Value = resp.ok()?.into_json().ok()?;
        if body["ok"].as_bool() != Some(true) {
            return None;
        }
        let q = &body["result"]["answers"]["quality"];
        let s = &body["result"]["answers"]["stop"];
        let score = q["score"].as_f64()?;
        let p_good = q["probabilities"]["3"].as_f64().unwrap_or(0.0)
            + q["probabilities"]["4"].as_f64().unwrap_or(0.0);
        let stop_choice = s["choice"].as_str().unwrap_or("?").to_string();
        let stop_p = s["probabilities"]
            .get(stop_choice.as_str())
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        tracing::info!(
            "[laya-prescreen] quality={:.2}/5 p_good={:.2} stop={} p={:.2} latency={}ms",
            score + 1.0,
            p_good,
            stop_choice,
            stop_p,
            body["latency_ms"].as_f64().unwrap_or(0.0)
        );
        Some(
            serde_json::json!({"quality_score": score, "p_good": p_good, "stop_choice": stop_choice, "stop_p": stop_p}),
        )
    }

    /// P2 LLM评审: MiniMax 按四维量表评交付摘要, 与自评对半融合
    pub(in crate::engine::mcp) fn llm_judge_task(
        &self,
        description: &str,
        result_text: &str,
        counters: (i64, i64, i64, i64),
        checkpoints: &serde_json::Value,
        self_q: f64,
    ) -> Option<(f64, String, Option<String>)> {
        let api_key = std::env::var("LLM_API_KEY").unwrap_or_default();
        if api_key.is_empty() {
            return None;
        }
        let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "MiniMax-M3".to_string());
        let base = std::env::var("LLM_API_BASE")
            .unwrap_or_else(|_| "https://api.minimaxi.com".to_string());
        let (cm, ca, cr, cc) = counters;
        let cps: Vec<String> = checkpoints
            .as_array()
            .map(|a| {
                a.iter()
                    .rev()
                    .take(5)
                    .rev()
                    .filter_map(|c| {
                        c.get("note")
                            .and_then(|n| n.as_str())
                            .map(|s| s.chars().take(40).collect::<String>())
                    })
                    .collect()
            })
            .unwrap_or_default();
        let cps_tl = cps.join(" -> ");
        let material = format!("任务: {}\n交付摘要: {}\n过程证据: 记忆检索{}次/备选{}个/修正{}轮/校准{}次; 检查点: {}\n自评分: {:.1}/5",
            description.chars().take(120).collect::<String>(),
            result_text.chars().take(800).collect::<String>(),
            cm, ca, cr, cc, cps_tl, self_q);
        let agent = ureq::AgentBuilder::new()
            .timeout_read(std::time::Duration::from_secs(30))
            .timeout_write(std::time::Duration::from_secs(5))
            .build();
        // P35b: MiniMax空输出(think耗尽tokens)自动重试一次 — DSH轮实证的"评审离线"多为空输出而非真离线
        for attempt in 1..=2u32 {
            if attempt == 2 {
                tracing::info!("[task-judge] 首次调用失败/空输出, 自动重试第2次");
            }
            let resp = agent.post(&format!("{}/v1/chat/completions", base))
            .set("Authorization", &format!("Bearer {}", api_key))
            .set("Content-Type", "application/json")
            .send_json(serde_json::json!({
                "model": model,
                "messages": [
                    {"role": "system", "content": "你是严格但公正的任务质量评审官。基于任务描述、交付摘要、过程证据和自评打分。铁律: 代理信号(测试通过/流程走完/代码量大/耗时长)不是完成证据; 不确定视为未达成; 有done_when清单时逐条对照证据评completeness。只返回JSON: {\"completeness\":1到5整数,\"accuracy\":1到5整数,\"depth\":1到5整数,\"actionability\":1到5整数,\"rationale\":\"一句话评语\",\"saturation_verdict\":\"justified或weak或disputed(无饱和声明时给justified)\"}"},
                    {"role": "user", "content": material}
                ],
                "temperature": 0.0, "max_tokens": 3072,
                "response_format": {"type": "json_object"}
            }));
            if let Ok(resp) = resp {
                let body: serde_json::Value = resp.into_json().unwrap_or_default();
                if let Some(content) = body["choices"][0]["message"]["content"].as_str() {
                    // MiniMax-M3 是推理模型: 剥离 <think> 块与 markdown 围栏再解析
                    let cleaned = match content.find("</think>") {
                        Some(pos) => &content[pos + 8..],
                        None => content,
                    };
                    let cleaned = cleaned
                        .trim()
                        .trim_start_matches("```json")
                        .trim_end_matches("```")
                        .trim();
                    // 容错: 提取首个 { 到最后一个 } 之间的JSON体
                    let json_body = match (cleaned.find('{'), cleaned.rfind('}')) {
                        (Some(a), Some(b)) if b > a => &cleaned[a..=b],
                        _ => cleaned,
                    };
                    match serde_json::from_str::<serde_json::Value>(json_body) {
                        Ok(p) => {
                            let dims = ["completeness", "accuracy", "depth", "actionability"];
                            let vals: Vec<f64> =
                                dims.iter().filter_map(|d| p[d].as_f64()).collect();
                            if vals.len() == 4 {
                                let score = vals.iter().sum::<f64>() / 4.0;
                                let note = p["rationale"]
                                    .as_str()
                                    .unwrap_or("")
                                    .chars()
                                    .take(120)
                                    .collect::<String>();
                                let verdict = p["saturation_verdict"]
                                    .as_str()
                                    .filter(|v| ["justified", "weak", "disputed"].contains(v))
                                    .map(|v| v.to_string());
                                return Some((score, note, verdict));
                            }
                            tracing::warn!(
                                "[task-judge] 四维字段不全: {}",
                                json_body.chars().take(150).collect::<String>()
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                "[task-judge] JSON解析失败({}): {}",
                                e,
                                cleaned.chars().take(200).collect::<String>()
                            );
                        }
                    }
                }
            }
        }
        tracing::warn!("[task-judge] LLM评审不可用(两次尝试均失败/空输出), 保留自评质量");
        None
    }

    pub(in crate::engine::mcp) fn tool_task_status(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let task_id = args.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
        // P1 恢复语义: 无参调用 = 找回活跃任务(智能体重启/上下文丢失后的再锚定)
        if task_id.is_empty() {
            let active = match self
                .engine
                .storage
                .get_active_task_for_user(&self.engine.user_id)
            {
                Some(t) => t,
                None => return self.smrp_ok_nn(
                    "task_status",
                    serde_json::json!({
                        "status": "idle", "message": "no active task — call task_start to begin",
                    }),
                ),
            };
            let sess = match self.engine.storage.get_task_session(&active) {
                Some(s) => s,
                None => return self.smrp_err("task_status", 500, "active task row missing"),
            };
            let start = sess.get("start_ts").and_then(|v| v.as_i64()).unwrap_or(0);
            let budget = sess.get("budget_ms").and_then(|v| v.as_i64()).unwrap_or(1);
            let elapsed = (chrono::Utc::now().timestamp() - start) * 1000;
            let remaining = (budget - elapsed).max(0);
            let elapsed_pct = ((elapsed as f64 / budget as f64) * 100.0)
                .round()
                .min(100.0);
            let phase = if elapsed_pct < 30.0 {
                "explore"
            } else if elapsed_pct < 70.0 {
                "build"
            } else if elapsed_pct < 90.0 {
                "verify"
            } else {
                "deliver"
            };
            let (mem_ops, alts, revs, checks) = self.engine.storage.get_task_counters(&active);
            let checkpoints = self.engine.storage.get_task_checkpoints(&active);
            let last_cp = checkpoints.as_array().and_then(|a| a.last()).cloned();
            let children = self.engine.storage.list_child_tasks(&active);
            let result = serde_json::json!({
                "status": "in_progress_recoverable",
                "task_id": active,
                "description": sess.get("description"),
                "elapsed_ms": elapsed, "remaining_ms": remaining,
                "percentage": (remaining as f64 / budget as f64 * 100.0).round(),
                "phase_estimate": phase,
                "evidence": {"memory_ops": mem_ops, "alternatives": alts, "revisions": revs, "checks": checks},
                "checkpoints_total": checkpoints.as_array().map(|a| a.len()).unwrap_or(0),
                "resume_from": last_cp,
                "open_questions": self.engine.storage.get_task_open_questions(&active),
                "alerts": Self::alerts_summary(&self.engine.storage.get_task_alerts(&active)),
                "children": if children.is_empty() { serde_json::Value::Null } else { serde_json::json!({
                    "total": children.len(),
                    "completed": children.iter().filter(|c| c.get("actual_ms").and_then(|v| v.as_i64()).is_some()).count(),
                })},
                "resume_instruction": "从最近 checkpoint 的进度继续; 若上下文已丢失, 先 memory_search 任务关键词恢复上下文, 再 task_check 校准相位",
            });
            return self.smrp_ok_nn("task_status", result);
        }
        match self.engine.storage.get_task_session(task_id) {
            Some(sess) => {
                let start = sess.get("start_ts").and_then(|v| v.as_i64()).unwrap_or(0);
                let budget = sess.get("budget_ms").and_then(|v| v.as_i64()).unwrap_or(1);
                let elapsed = (chrono::Utc::now().timestamp() - start) * 1000;
                let remaining = (budget - elapsed).max(0);
                let pct = (remaining as f64 / budget as f64 * 100.0).round();
                let completed = sess.get("actual_ms").and_then(|v| v.as_i64()).is_some();
                let alerts = Self::alerts_summary(&self.engine.storage.get_task_alerts(task_id));
                let result = serde_json::json!({
                    "task_id": task_id,
                    "description": sess.get("description"),
                    "status": if completed { "completed" } else if pct <= 0.0 { "expired" } else { "in_progress" },
                    "elapsed_ms": elapsed, "remaining_ms": remaining, "percentage": pct,
                    "alerts": alerts,
                });
                self.smrp_ok_nn("task_status", result)
            }
            None => self.smrp_err("task_status", 404, "task not found"),
        }
    }
}
