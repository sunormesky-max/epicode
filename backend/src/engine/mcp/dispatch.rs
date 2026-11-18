use super::*;

impl McpHandler {
    pub(in crate::engine::mcp) fn initialize(&self, id: Option<serde_json::Value>) -> McpResponse {
        let identity = self.engine.space.identity_info();
        let identity_json = if let Some(ref info) = identity {
            serde_json::json!({
                "name": info.system_name,
                "mission": info.mission,
                "author": info.author,
                "confirmed": info.confirmed,
            })
        } else {
            let pending = self.engine.space.pending_identity();
            serde_json::json!({
                "confirmed": false,
                "ritual": {
                    "step": pending.current_step(),
                    "completed": pending.completed_steps(),
                    "total": 5,
                    "next_prompt": pending.step_prompt(),
                },
                "message": "Identity ritual incomplete. Use identity_step to continue the ceremony.",
                "instructions": "Call identity_step(step, value) for each stage: 1=Name, 2=Mission, 3=Creator, 4=Personality, 5=Language. Then identity_finalize() to awaken."
            })
        };
        McpResponse {
            jsonrpc: "2.0".into(),
            id,
            result: Some(serde_json::json!({
                "protocolVersion": "2025-11-25",
                "serverInfo": { "name": "Epicode", "version": env!("CARGO_PKG_VERSION") },
                "capabilities": {
                    "tools": { "listChanged": false },
                    "resources": { "listChanged": false }
                },
                "_meta": {
                    "io.modelcontextprotocol/protocolVersion": "2025-11-25",
                    "io.modelcontextprotocol/serverInfo": { "name": "Epicode", "version": env!("CARGO_PKG_VERSION") }
                },
                "identity": identity_json,
            })),
            error: None,
        }
    }

    /// MCP 2026-07-28: server/discover — servers MUST implement this RPC.
    /// Advertises supported protocol versions, capabilities, and identity.
    /// Replaces initialize as the primary discovery mechanism.
    pub(in crate::engine::mcp) fn server_discover(
        &self,
        id: Option<serde_json::Value>,
    ) -> McpResponse {
        McpResponse {
            jsonrpc: "2.0".into(),
            id,
            result: Some(serde_json::json!({
                "protocolVersions": ["2026-07-28", "2025-11-25", "2024-11-05"],
                "protocolVersion": "2026-07-28",
                "serverInfo": { "name": "Epicode", "version": env!("CARGO_PKG_VERSION") },
                "capabilities": {
                    "tools": { "listChanged": false },
                    "resources": { "listChanged": false }
                },
                "_meta": {
                    "io.modelcontextprotocol/serverInfo": {
                        "name": "Epicode",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }
            })),
            error: None,
        }
    }

    /// MCP 2026-07-28: ping removed from core but kept for backward compat.
    pub(in crate::engine::mcp) fn pong(&self, id: Option<serde_json::Value>) -> McpResponse {
        McpResponse {
            jsonrpc: "2.0".into(),
            id,
            result: Some(serde_json::json!({})),
            error: None,
        }
    }

    /// MCP 2026-07-28: logging/setLevel deprecated — log level now per-request via _meta.
    /// Accept but no-op for backward compatibility.
    pub(in crate::engine::mcp) fn logging_set_level(
        &self,
        id: Option<serde_json::Value>,
        _params: Option<serde_json::Value>,
    ) -> McpResponse {
        McpResponse {
            jsonrpc: "2.0".into(),
            id,
            result: Some(serde_json::json!({})),
            error: None,
        }
    }

    /// MCP 2026-07-28: subscriptions/listen — long-lived POST-response stream.
    /// Stub: returns immediately with empty subscriptions (Epicode uses SSE for real-time updates).
    pub(in crate::engine::mcp) fn subscriptions_listen(
        &self,
        id: Option<serde_json::Value>,
        _params: Option<serde_json::Value>,
    ) -> McpResponse {
        McpResponse {
            jsonrpc: "2.0".into(),
            id,
            result: Some(serde_json::json!({
                "subscriptions": [],
                "resultType": "complete"
            })),
            error: None,
        }
    }

    pub(in crate::engine::mcp) fn tools_list(&self, id: Option<serde_json::Value>) -> McpResponse {
        let result = Self::tool_catalog();
        McpResponse {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Number of MCP tools advertised by tools/list.
    pub fn public_tool_count() -> usize {
        Self::tool_catalog()["tools"]
            .as_array()
            .map_or(0, |tools| tools.len())
    }

    pub(in crate::engine::mcp) fn smrp_output_schema() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["protocol", "data", "status"],
            "properties": {
                "protocol": {
                    "type": "object",
                    "required": ["schema_version", "tool", "ok", "error"],
                    "properties": {
                        "schema_version": {"type": "string"},
                        "tool": {"type": "string"},
                        "ok": {"type": "boolean"},
                        "error": {"anyOf": [{"type": "object"}, {"type": "null"}]}
                    }
                },
                "data": {},
                "status": {"type": "object"}
            }
        })
    }

    pub(in crate::engine::mcp) fn tools_call(
        &self,
        id: Option<serde_json::Value>,
        params: Option<serde_json::Value>,
    ) -> McpResponse {
        let params = match params {
            Some(p) => p,
            None => return self.error(id, -32602, "missing params"),
        };

        let name = params["name"].as_str().unwrap_or("");
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        // Subaccount role gate: an unmapped tool is denied until it is explicitly
        // classified above. Owners and local handlers have no role gate.
        if let Some(role) = self.role_gate {
            let allowed = Self::tool_permission(name).is_some_and(|perm| {
                if let Some(custom) = &self.custom_permissions {
                    custom.iter().any(|n| n == perm.as_str())
                } else {
                    role.can(perm)
                }
            });
            if !allowed {
                let denied = self.smrp_err(name, 403, "insufficient role for this tool");
                let text = serde_json::to_string(&denied).unwrap_or_default();
                return McpResponse {
                    jsonrpc: "2.0".into(),
                    id,
                    result: Some(serde_json::json!({
                        "content": [{ "type": "text", "text": text }],
                        "structuredContent": denied,
                        "isError": true,
                        "resultType": "complete"
                    })),
                    error: None,
                };
            }
        }

        // 统一身分检查（kimi2.7 #27）：除身份仪式本身外，所有操作需确认身份
        if !matches!(
            name,
            "identity_confirm" | "identity_step" | "identity_finalize"
        ) && self.engine.space.identity_info().is_none()
        {
            let pending = self.engine.space.pending_identity();
            // C2修复：身份拦截器改用 SMRP 信封（与其他 32 个工具一致）
            let identity_data = serde_json::json!({
                "ritual_progress": { "step": pending.current_step(), "completed": pending.completed_steps(), "total": 5 },
                "next_prompt": pending.step_prompt(),
                "required_flow": "Use identity_step to complete the ritual ceremony, then identity_finalize to awaken."
            });
            let smrp_err = self.smrp_err(
                name,
                4003,
                "identity_not_confirmed — Identity confirmation required before operations.",
            );
            // smrp_err 返回的 data 段需要合并 identity 上下文
            let merged =
                if let Some(obj) = smrp_err.get("data").and_then(|d| d.as_object().cloned()) {
                    let mut m = obj;
                    if let Some(id_data) = identity_data.as_object() {
                        m.extend(id_data.iter().map(|(k, v)| (k.clone(), v.clone())));
                    }
                    serde_json::Value::Object(m)
                } else {
                    identity_data
                };
            let mut result = smrp_err;
            if let Some(obj) = result.as_object_mut() {
                obj.insert("data".into(), merged);
            }
            // 统一契约(PR#133主题): 身份门也走 tools_call 通用响应形态 —
            // SMRP ok=false ⇒ isError:true + structuredContent 同步携带(测试6328断言)
            let text = serde_json::to_string(&result).unwrap_or_default();
            return McpResponse {
                jsonrpc: "2.0".into(),
                id,
                result: Some(serde_json::json!({
                    "content": [{ "type": "text", "text": text }],
                    "structuredContent": result,
                    "isError": true,
                    "resultType": "complete"
                })),
                error: None,
            };
        }

        // P6 双钟: 任何工具调用都是活跃心跳 — 预算按活跃时间燃烧
        if let Some(atid) = self
            .engine
            .storage
            .get_active_task_for_user(&self.engine.user_id)
        {
            self.engine.storage.touch_task_activity(&atid);
        }
        let mut result = match name {
            "epicode_handshake" => self.tool_epocode_handshake(&args),
            "task_start" => self.tool_task_start(&args),
            "task_check" => self.tool_task_check(&args),
            "task_complete" => self.tool_task_complete(&args),
            "task_status" => self.tool_task_status(&args),
            "skill_get" => self.tool_skill_get(&args),
            "task_alert" => self.tool_task_alert(&args),
            "memory_create" => self.tool_memory_create(&args),
            "memory_search" => self.tool_memory_search(&args),
            "library_search" => self.tool_library_search(&args),
            "memory_recall" => self.tool_memory_recall(&args),
            "memory_wander" => self.tool_memory_wander(&args),
            "memory_ask" => self.tool_memory_ask(&args),
            "memory_get" => self.tool_memory_get(&args),
            "memory_list" => self.tool_memory_list(&args),
            "memory_update" => self.tool_memory_update(&args),
            "memory_delete" => self.tool_memory_delete(&args),
            "ctx_load" => self.tool_ctx_load(&args),
            "ctx_save" => self.tool_ctx_save(&args),
            "pattern_learn" => self.tool_pattern_learn(&args),
            "pattern_recall" => self.tool_pattern_recall(&args),
            "decision_record" => self.tool_decision_record(&args),
            "bug_memory" => self.tool_bug_memory(&args),
            "session_summary" => self.tool_session_summary(&args),
            "space_stats" => self.tool_space_stats(),
            "dream_cycle" => self.tool_dream_cycle(&args),
            "knowledge_relations" => self.tool_knowledge_relations(&args),
            "concepts" => self.tool_concepts(),
            "context_observe" => self.tool_context_observe(&args),
            "identity_confirm" => self.tool_identity_confirm(&args),
            "identity_step" => self.tool_identity_step(&args),
            "identity_finalize" => self.tool_identity_finalize(),
            "skill_execute" => self.tool_skill_execute(&args),
            "skill_feedback" => self.tool_skill_feedback(&args),
            "feedback_submit" => self.tool_feedback_submit(&args),
            "skills_sync" => self.tool_skills_sync(&args),
            "enforced_rules" => self.tool_enforced_rules(&args),
            "project_list" => self.tool_project_list(),
            "embedding_diagnostic" => self.tool_embedding_diagnostic(),
            "embedding_migrate" => self.tool_embedding_migrate(),
            "kg_quality" => self.tool_kg_quality(&args),
            "doc_import" => self.tool_doc_import(&args),
            "doc_list" => self.tool_doc_list(),
            "memory_export" => self.tool_memory_export(&args),
            "session_list" => self.tool_session_list(&args),
            "memory_restore" => self.tool_memory_restore(&args),
            "memory_improve" => self.tool_memory_improve(&args),
            "skill_auto_extract" => self.tool_skill_auto_extract(&args),
            "drive_inbox" => self.tool_drive_inbox(&args),
            "drive_ack" => self.tool_drive_ack(&args),
            "memory_propose_actions" => self.tool_memory_propose_actions(&args),
            "action_list" => self.tool_action_list(&args),
            "action_planner_status" => self.tool_action_planner_status(&args),
            "action_planner_run" => self.tool_action_planner_run(&args),
            "action_goal_create" => self.tool_action_goal_create(&args),
            "action_goal_prove" => self.tool_action_goal_prove(&args),
            "action_goal_list" => self.tool_action_goal_list(&args),
            "action_control" => self.tool_action_control(&args),
            "action_claim" => self.tool_action_claim(&args),
            "action_heartbeat" => self.tool_action_heartbeat(&args),
            "action_complete" => self.tool_action_complete(&args),
            "drive_to_actions" => self.tool_drive_to_actions(&args),
            "worker_register" => self.tool_worker_register(&args),
            "worker_unregister" => self.tool_worker_unregister(&args),
            "worker_list" => self.tool_worker_list(&args),
            "action_assign" => self.tool_action_assign(&args),
            "action_dispatch" => self.tool_action_dispatch(&args),
            "memory_forget" => self.tool_memory_forget(&args),
            "blocks_get" => self.tool_blocks_get(&args),
            "blocks_set" => self.tool_blocks_set(&args),
            _ => return self.error(id, -32601, &format!("unknown tool: {}", name)),
        };

        // P33 宪法三要素: 每个响应自带时间上下文(t=几点/Δ=多久/◆=规则) — 安静在场, 不抢注意力
        if name != "task_start" && name != "task_complete" {
            let now_str = (chrono::Utc::now() + chrono::Duration::hours(8))
                .format("%H:%M:%S")
                .to_string();
            let (delta_str, rule_str) = match self
                .engine
                .storage
                .get_active_task_for_user(&self.engine.user_id)
            {
                Some(atid) => match self.engine.storage.get_task_session(&atid) {
                    Some(sess) => {
                        let start = sess.get("start_ts").and_then(|v| v.as_i64()).unwrap_or(0);
                        let budget = sess
                            .get("budget_ms")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(1)
                            .max(1);
                        let active = self.engine.storage.get_effective_active_ms(&atid);
                        let elapsed_s = (chrono::Utc::now().timestamp() - start).max(0);
                        let start_str = (chrono::DateTime::from_timestamp(start, 0)
                            .unwrap_or_default()
                            + chrono::Duration::hours(8))
                        .format("%H:%M:%S")
                        .to_string();
                        let d =
                            format!("{}m{:02}s @ {}", elapsed_s / 60, elapsed_s % 60, start_str);
                        let apct =
                            ((active as f64 / budget as f64) * 100.0).round().min(999.0) as i64;
                        let seg = if apct < 30 {
                            "探索段(先v0后广度)"
                        } else if apct < 70 {
                            "构建段(备选+深化)"
                        } else if apct < 90 {
                            "校验段(修正+反思)"
                        } else {
                            "冲刺段(打磨交付)"
                        };
                        let r = format!(
                            "{}m·{}% {} · 停止权=说不出下一个改进点",
                            budget / 60000,
                            apct,
                            seg
                        );
                        (d, r)
                    }
                    None => ("idle".to_string(), "no task".to_string()),
                },
                None => ("idle".to_string(), "ready".to_string()),
            };
            if let Some(data) = result.get_mut("data").and_then(|d| d.as_object_mut()) {
                data.insert(
                    "ctx".into(),
                    serde_json::json!({
                        "t": now_str,
                        "Δ": delta_str,
                        "◆": rule_str,
                    }),
                );
            }
        }

        // P9 任务脉冲: 活跃任务期间每个响应携带过程状态 — 对治长会话约束衰减
        if name != "task_status" && name != "task_start" {
            if let Some(atid) = self
                .engine
                .storage
                .get_active_task_for_user(&self.engine.user_id)
            {
                if let Some(sess) = self.engine.storage.get_task_session(&atid) {
                    let budget = sess
                        .get("budget_ms")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(1)
                        .max(1);
                    // 与 ctx/task_check/task_complete 同一口径: 插件上报的真实活跃时间优先
                    let active = self.engine.storage.get_effective_active_ms(&atid);
                    let (m_ops, alts, revs, _) = self.engine.storage.get_task_counters(&atid);
                    let pct = (active as f64 / budget as f64 * 100.0).round().min(100.0) as i64;
                    if let Some(data) = result.get_mut("data").and_then(|d| d.as_object_mut()) {
                        // P10 相位感知教练: skills从"开工递一次"升级为"全程指导"
                        let phase = if pct < 30 {
                            "explore"
                        } else if pct < 70 {
                            "build"
                        } else if pct < 90 {
                            "verify"
                        } else {
                            "deliver"
                        };
                        let mut coach: Vec<String> = Vec::new();
                        if m_ops < 1 {
                            coach.push(
                                "先 memory_search(任务关键词) 点亮探索门 — 无记忆不得交付".into(),
                            );
                            // S2 语义技能注入: 探索期把相关技能卡递到眼前(构建期后静默防注意力稀释)
                            if let Some(desc) = sess.get("description").and_then(|v| v.as_str()) {
                                for (sk, sc) in self.engine.skills.match_skills_scored(
                                    desc,
                                    &self.engine.user_id,
                                    2,
                                ) {
                                    if sc >= 0.5 {
                                        let tg = if sk.triggers.is_empty() {
                                            String::new()
                                        } else {
                                            format!(" [场景: {}]", sk.triggers.join("/"))
                                        };
                                        coach.push(format!("相关技能「{}」(match {}) — {}{} | 需要细节: skill_get('{}')",
                                            sk.name, sc, sk.description.as_deref().unwrap_or(""), tg, sk.name));
                                    }
                                }
                            }
                        }
                        let cpn = self
                            .engine
                            .storage
                            .get_task_checkpoints(&atid)
                            .as_array()
                            .map(|a| a.len())
                            .unwrap_or(0);
                        if pct > 20 && cpn == 0 {
                            coach.push("v0未落: 先 task_check(checkpoint:'初版一句话') 落初版 — 驻留改进永远要有对象".into());
                        }
                        if pct >= 30 && m_ops >= 1 && alts < 2 {
                            coach.push(format!("构建相: 已比较备选 {} 个 — 用 task_check(alternatives_considered) 申报, 需>=2", alts));
                        }
                        if pct >= 70 && revs < 1 {
                            coach.push(
                                "校验相: 完成一轮针对性修正后 task_check(revision_done:true) 申报"
                                    .into(),
                            );
                        }
                        if pct >= 90 {
                            coach.push("交付相: task_complete 带 self_rating(评审默认开启), 交付物必须附 task_id".into());
                        }
                        if !coach.is_empty() {
                            let hint = match phase {
                                "explore" => "记忆智能存取",
                                "build" => "上下文管理",
                                "verify" => "质量自控",
                                _ => "",
                            };
                            if !hint.is_empty() {
                                coach.push(format!("深读: skill_get('{}')", hint));
                            }
                        }
                        data.insert("task_pulse".into(), serde_json::json!({
                            "task_id": atid, "active_pct": pct, "phase": phase,
                            "gates": {"memory_ops": m_ops, "alternatives": alts, "revisions": revs},
                            "coach": coach,
                            "server_now": Self::server_now_json(),
                        }));
                    }
                }
            }
        }

        let is_error = result["protocol"]["ok"].as_bool() == Some(false);
        let text = match serde_json::to_string(&result) {
            Ok(text) => text,
            Err(e) => {
                tracing::error!("[MCP] failed to serialize tool result for {}: {}", name, e);
                return self.error(id, -32603, "failed to serialize tool result");
            }
        };
        McpResponse {
            jsonrpc: "2.0".into(),
            id,
            result: Some(serde_json::json!({
                "content": [{ "type": "text", "text": text }],
                "structuredContent": result,
                "isError": is_error,
                "resultType": "complete",
                "_meta": {
                    "io.modelcontextprotocol/serverInfo": { "name": "Epicode", "version": env!("CARGO_PKG_VERSION") }
                }
            })),
            error: None,
        }
    }

    // ═══ 时间效性工具 ═══

    pub(in crate::engine::mcp) fn resources_list(
        &self,
        id: Option<serde_json::Value>,
    ) -> McpResponse {
        McpResponse {
            jsonrpc: "2.0".into(),
            id,
            result: Some(serde_json::json!({
                "resources": [
                    { "uri": "epicode://space/stats", "name": "Space Statistics", "mimeType": "application/json" }
                ]
            })),
            error: None,
        }
    }

    /// Resource URIs advertised by resources/list; each must be readable.
    pub const RESOURCE_URIS: &'static [&'static str] = &["epicode://space/stats"];

    /// MCP resources/read. Uses the same read permission as the backing tool.
    pub(in crate::engine::mcp) fn resources_read(
        &self,
        id: Option<serde_json::Value>,
        params: Option<serde_json::Value>,
    ) -> McpResponse {
        let uri = params
            .as_ref()
            .and_then(|p| p.get("uri"))
            .and_then(|u| u.as_str())
            .unwrap_or("");
        let backing_tool = match uri {
            "epicode://space/stats" => "space_stats",
            "" => return self.error(id, -32602, "missing params.uri"),
            other => return self.error(id, -32002, &format!("resource not found: {other}")),
        };
        if let Some(role) = self.role_gate {
            let allowed = Self::tool_permission(backing_tool).is_some_and(|perm| {
                if let Some(custom) = &self.custom_permissions {
                    custom.iter().any(|n| n == perm.as_str())
                } else {
                    role.can(perm)
                }
            });
            if !allowed {
                return self.error(id, -32003, "insufficient role for this resource");
            }
        }
        let body = self.tool_space_stats();
        McpResponse {
            jsonrpc: "2.0".into(),
            id,
            result: Some(serde_json::json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": serde_json::to_string(&body).unwrap_or_default(),
                }]
            })),
            error: None,
        }
    }

    pub(in crate::engine::mcp) fn error(
        &self,
        id: Option<serde_json::Value>,
        code: i64,
        msg: &str,
    ) -> McpResponse {
        McpResponse {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(McpError {
                code,
                message: msg.to_string(),
            }),
        }
    }

    pub fn process_json(&self, raw: &str) -> String {
        if raw.len() > MAX_MCP_REQUEST_BYTES {
            return serialize_mcp_response(
                McpResponse {
                    jsonrpc: "2.0".into(),
                    id: None,
                    result: None,
                    error: Some(McpError {
                        code: -32000,
                        message: format!(
                            "request too large; maximum is {} bytes",
                            MAX_MCP_REQUEST_BYTES
                        ),
                    }),
                },
                None,
            );
        }
        let req: McpRequest = match serde_json::from_str(raw) {
            Ok(r) => r,
            Err(e) => {
                let resp = McpResponse {
                    jsonrpc: "2.0".into(),
                    id: None,
                    result: None,
                    error: Some(McpError {
                        code: -32700,
                        message: format!("parse error: {}", e),
                    }),
                };
                return serialize_mcp_response(resp, None);
            }
        };
        let req_id = req.id.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.handle(req)));
        let resp = match result {
            Ok(r) => r,
            Err(_) => {
                tracing::error!("[MCP] panic caught in process_json (id={:?})", req_id);
                McpResponse {
                    jsonrpc: "2.0".into(),
                    id: req_id.clone(),
                    result: None,
                    error: Some(McpError {
                        code: -32603,
                        message: "internal error (panic caught)".into(),
                    }),
                }
            }
        };
        serialize_mcp_response(resp, req_id)
    }
}
