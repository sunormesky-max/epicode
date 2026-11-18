use super::super::*;

impl McpHandler {
    pub(in crate::engine::mcp) fn tool_memory_create(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let raw = args["content"].as_str().unwrap_or("");
        let content = strip_html(raw);
        if content.trim().is_empty() {
            return self.smrp_err("memory_create", 400, "content is required");
        }
        let char_count = content.chars().count();
        let (content, truncated) = if char_count > 5000 {
            let s: String = content.chars().take(5000).collect();
            (s, true)
        } else {
            (content, false)
        };
        let labels: Vec<String> = args["labels"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        // 配额检查(与 REST check_and_increment_memory 对等,B1修复)
        if let Err(err_resp) = self.check_quota("memory_create") {
            return err_resp;
        }
        match self
            .engine
            .scheduler
            .api_create_memory_full(&content, labels)
        {
            Ok(r) => {
                self.rollback_quota(r.is_new); // dedup 回滚配额
                let preview: String = content.chars().take(200).collect();
                let mut data = crate::engine::smrp::create_data(&self.engine, &r, &preview);
                if truncated {
                    data["warning"] = serde_json::json!(format!(
                        "content truncated from {} to 5000 characters",
                        char_count
                    ));
                }
                self.smrp_ok("memory_create", data)
            }
            Err(e) => {
                self.rollback_quota(false); // 创建失败也回滚
                self.smrp_err("memory_create", 500, &e)
            }
        }
    }

    /// L1图书馆: MCP入口 — 检索全局知识资产(文献/手册), 带provenance(书名/章节/arXiv号)
    pub(in crate::engine::mcp) fn tool_library_search(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let query = args["query"].as_str().unwrap_or("");
        if query.is_empty() {
            return self.smrp_err("library_search", 400, "query is required");
        }
        let limit = args["limit"].as_u64().unwrap_or(5).min(20) as usize;
        // 图书馆检索走scheduler(与REST端同一权限/检索路径)
        match self.engine.scheduler.library_search_public(query, limit) {
            Ok(hits) => {
                let items: Vec<serde_json::Value> = hits.iter().map(|h| serde_json::json!({
                    "content": h.content,
                    "title": h.title,
                    "chunk_no": h.chunk_no,
                    "score": h.score,
                    "source": "library",
                    "provenance": format!("{} (arXiv:{}) chunk#{}", h.title, h.client_ref, h.chunk_no),
                })).collect();
                self.smrp_ok("library_search", serde_json::json!({
                    "results": items, "count": items.len(),
                    "note": "图书馆=全局共享知识资产(文献/手册), 结果带来源溯源; 与memory_search(个人记忆)互补",
                }))
            }
            Err(e) => self.smrp_err("library_search", 500, &e),
        }
    }

    pub(in crate::engine::mcp) fn tool_memory_search(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let query = args["query"].as_str().unwrap_or("");
        if query.is_empty() {
            return self.smrp_err("memory_search", 400, "query is required");
        }
        // P0 相位机: 记忆检索计入活跃任务的探索相证据
        if let Some(tid) = self
            .engine
            .storage
            .get_active_task_for_user(&self.engine.user_id)
        {
            self.engine.storage.bump_task_counter(&tid, "memory_ops");
        }
        let limit = args["limit"].as_u64().unwrap_or(10) as usize;
        let offset = args["offset"].as_u64().unwrap_or(0) as usize;
        let requested_limit = limit;
        let limit = requested_limit.min(200);
        let fetch = limit.saturating_add(offset).min(200);
        let filters = self.build_search_filters(args);
        let search_mode = filters
            .as_ref()
            .map(|filters| filters.mode)
            .unwrap_or_default();
        match self
            .engine
            .scheduler
            .api_search_scored(query, fetch, filters.as_ref())
        {
            Ok((results, notes)) => {
                let (total_found, picked) =
                    crate::engine::smrp::paginate_search_results(results, offset, limit);
                // L1合并层: memory_search结果前插入图书馆top-3(带source=library标记, 最多3条不喧宾夺主)
                let lib_hits = self
                    .engine
                    .scheduler
                    .library_search_public(query, 3)
                    .unwrap_or_default();
                let _lib_items: Vec<serde_json::Value> = lib_hits.iter().map(|h| serde_json::json!({
                    "content": h.content,
                    "labels": ["library", "knowledge-asset"],
                    "similarity": h.score,
                    "source": "library",
                    "provenance": format!("{} (arXiv:{}) chunk#{}", h.title, h.client_ref, h.chunk_no),
                })).collect();
                // SMRP §5.1 分桶：primary(强相关) / contextual(弱关联) / experiential(历史经历)。
                let mut flat: Vec<serde_json::Value> = Vec::with_capacity(picked.len());
                let mut primary: Vec<serde_json::Value> = Vec::new();
                let mut contextual: Vec<serde_json::Value> = Vec::new();
                let mut experiential: Vec<serde_json::Value> = Vec::new();
                for (id, sim, _mass, payload) in &picked {
                    let matched_by = notes.matched_by_map.get(id).map(Vec::as_slice);
                    let tier = crate::engine::smrp::tier_search_for_mode(
                        *sim,
                        &payload.labels,
                        search_mode,
                        matched_by,
                    );
                    let mut item = self.memory_item(
                        *id,
                        &payload.content,
                        &payload.labels,
                        payload.timestamp,
                        tier,
                        crate::engine::smrp::search_sources(search_mode, matched_by),
                        *sim,
                        None,
                    );
                    // Phase 1: exact 模式附加 matched_by 命中来源
                    if let Some(matched) = notes.matched_by_map.get(id) {
                        item["matched_by"] = serde_json::json!(matched);
                    }
                    match tier {
                        "primary" => primary.push(item.clone()),
                        "experiential" => experiential.push(item.clone()),
                        _ => contextual.push(item.clone()),
                    }
                    flat.push(item);
                }
                // SMRP §6 score_notes：只报 picked 范围内的 boost 调整（分数可解释性）
                let picked_ids: std::collections::HashSet<u64> =
                    picked.iter().map(|(id, _, _, _)| *id).collect();
                let filter_ids = |v: &[u64]| {
                    v.iter()
                        .filter(|i| picked_ids.contains(i))
                        .copied()
                        .collect::<Vec<_>>()
                };
                let score_base = crate::engine::smrp::search_score_base(search_mode);
                let adjustments = vec![
                    serde_json::json!({"kind": "cluster_boost", "delta": 0.08, "applied_to": filter_ids(&notes.cluster_boosted)}),
                    serde_json::json!({"kind": "importance_boost", "delta": 0.06, "applied_to": filter_ids(&notes.importance_boosted)}),
                    serde_json::json!({"kind": "access_boost", "delta": 0.04, "applied_to": filter_ids(&notes.access_boosted)}),
                    serde_json::json!({"kind": "outdated_penalty", "delta": -0.30, "applied_to": filter_ids(&notes.penalized)}),
                ];
                let mut data = serde_json::json!({
                    "query": query,
                    "tiers": {
                        "primary": primary,
                        "contextual": contextual,
                        "experiential": experiential,
                        "hub": [],
                    },
                    "results": flat,
                    "count": picked.len(),
                    "total_found": total_found,
                    "offset": offset,
                    "score_notes": {
                        "base": score_base,
                        "adjustments": adjustments,
                    },
                });
                if requested_limit > 200 {
                    data["warning"] = serde_json::json!(
                        "limit capped at 200; request a higher offset to paginate"
                    );
                }
                // Spike S1 Slim Envelope (flag/arg); default path unchanged.
                if crate::engine::spike_flags::slim_envelope_requested(args) {
                    data = crate::engine::smrp::to_slim_envelope(
                        &data,
                        crate::engine::smrp::SLIM_MAX_CONTENT_CHARS,
                    );
                }
                self.attach_recall_log("memory_search", query, &mut data, 0, None);
                self.smrp_ok("memory_search", data)
            }
            Err(e) => self.smrp_err("memory_search", 500, &e),
        }
    }

    pub(in crate::engine::mcp) fn tool_memory_recall(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let query = args["query"].as_str().unwrap_or("");
        if query.is_empty() {
            return self.smrp_err("memory_recall", 400, "query is required");
        }
        let depth = args["depth"].as_u64().unwrap_or(2).min(3) as usize;
        // P0 相位机: 深度回忆同样计入探索相证据
        if let Some(tid) = self
            .engine
            .storage
            .get_active_task_for_user(&self.engine.user_id)
        {
            self.engine.storage.bump_task_counter(&tid, "memory_ops");
        }
        match self.engine.scheduler.api_recall(query, depth) {
            Ok(result) => {
                let mut data =
                    crate::engine::smrp::recall_data(&self.engine, &result, query, depth);
                // MCP default is slim (full recall can reach 160-270KB on real
                // spaces); `verbosity:"full"` keeps the tier structure, capped.
                let cap = args["limit"]
                    .as_u64()
                    .map(|n| n.clamp(1, 60) as usize)
                    .unwrap_or(crate::engine::smrp::SLIM_RECALL_MAX_ITEMS);
                let full = args["verbosity"]
                    .as_str()
                    .is_some_and(|s| s.eq_ignore_ascii_case("full"))
                    && !crate::engine::spike_flags::slim_envelope_env();
                if full {
                    crate::engine::smrp::cap_full_recall(&mut data, cap);
                } else {
                    data = crate::engine::smrp::to_slim_envelope(
                        &data,
                        crate::engine::smrp::SLIM_MAX_CONTENT_CHARS,
                    );
                    crate::engine::smrp::cap_slim_items(&mut data, cap);
                }
                let attempt = args["attempt"].as_i64().unwrap_or(0);
                let stage = args["stage"].as_str();
                self.attach_recall_log("memory_recall", query, &mut data, attempt, stage);
                self.smrp_ok("memory_recall", data)
            }
            Err(e) => self.smrp_err("memory_recall", 500, &e),
        }
    }

    /// 四档联想模式(memory_wander): 线索驱动的自由联想游走。
    /// 浅度(3 跳)/中度(8 跳)/深度(16 跳)/自由(智能体自定步数 ≤64)。
    pub(in crate::engine::mcp) fn tool_memory_wander(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let start = match args["start_id"].as_u64() {
            Some(id) => id,
            None => {
                let query = args["query"].as_str().unwrap_or("").trim();
                if query.is_empty() {
                    return self.smrp_err("memory_wander", 400, "start_id or query is required");
                }
                match self.engine.scheduler.api_search(query, 1) {
                    Ok(mut hits) if !hits.is_empty() => hits.remove(0).0,
                    _ => return self.smrp_err("memory_wander", 404, "no seed found for query"),
                }
            }
        };
        if self.engine.scheduler.api_get_node(start).is_none() {
            return self.smrp_err("memory_wander", 404, &format!("memory {} not found", start));
        }
        let depth = args["depth"]
            .as_str()
            .and_then(crate::engine::wander::DepthPreset::parse)
            .unwrap_or(crate::engine::wander::DepthPreset::Medium);
        let strategy = args["strategy"]
            .as_str()
            .and_then(crate::engine::wander::Strategy::parse)
            .unwrap_or(crate::engine::wander::Strategy::Strongest);
        // 深度联想同样是探索相证据
        if let Some(tid) = self
            .engine
            .storage
            .get_active_task_for_user(&self.engine.user_id)
        {
            self.engine.storage.bump_task_counter(&tid, "memory_ops");
        }
        let result = crate::engine::wander::wander(
            |id| self.engine.scheduler.api_get_relations_directed(id),
            |id| self.engine.scheduler.api_get_node(id),
            |from: u64, visited: &[u64]| self.engine.scheduler.api_spectral_far_jump(from, visited),
            |from: u64, visited: &[u64]| self.engine.scheduler.api_era_jump(from, visited),
            crate::engine::wander::WanderParams {
                start,
                depth,
                strategy,
                max_steps: args["max_steps"].as_u64().map(|n| n as usize),
                temperature: args["temperature"].as_f64(),
                far_jumps: args["far_jumps"].as_u64().map(|n| (n as usize).min(4)),
                era_jumps: args["era_jumps"].as_u64().map(|n| (n as usize).min(4)),
                t0_unix: self.engine.scheduler.api_space_t0(),
                seed: args["seed"].as_u64(),
            },
        );
        let data = result.to_json(
            start,
            &format!("{:?}", depth).to_lowercase(),
            &format!("{:?}", strategy).to_lowercase(),
        );
        // S1 基准埋点: wander 查询也进 recall_log(锚定联想查询是真值语料的一部分)
        let log_query = args["query"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| format!("wander#{start}"));
        let mut data = data;
        self.attach_recall_log("memory_wander", &log_query, &mut data, 0, None);
        self.smrp_ok("memory_wander", data)
    }

    pub(in crate::engine::mcp) fn tool_memory_ask(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let question = args["question"].as_str().unwrap_or("");
        if question.trim().is_empty() {
            return self.smrp_err("memory_ask", 400, "question is required");
        }
        let depth = args["depth"].as_u64().unwrap_or(2).min(10) as usize;
        if let Some(task_id) = self
            .engine
            .storage
            .get_active_task_for_user(&self.engine.user_id)
        {
            self.engine
                .storage
                .bump_task_counter(&task_id, "memory_ops");
        }
        match self.engine.scheduler.api_ask(question, depth) {
            Ok(mut result) => {
                self.attach_recall_log("memory_ask", question, &mut result, 0, None);
                self.smrp_ok("memory_ask", result)
            }
            Err(e) => self.smrp_err("memory_ask", 500, &e),
        }
    }

    pub(in crate::engine::mcp) fn tool_memory_get(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let id = match args["id"].as_u64() {
            Some(id) => id,
            None => return self.smrp_err("memory_get", 400, "id is required"),
        };
        match self.engine.scheduler.api_get_node(id) {
            Some(payload) => {
                // 合并 topology + relations_summary（SMRP §7.1：免去消费者额外调用）
                let topo = self.cluster_of(id).map(|(cid, sz)| {
                    let degree = self.engine.scheduler.api_get_relations(id).len();
                    serde_json::json!({"cluster_id": cid, "cluster_size": sz, "is_hub": degree >= 10})
                });
                let rels = self.engine.scheduler.api_get_relations(id);
                let mut type_dist = std::collections::HashMap::new();
                let mut strongest: Option<(&u64, &String, f64)> = None;
                for (tgt, rt, st) in &rels {
                    *type_dist.entry(rt.as_str()).or_insert(0u32) += 1;
                    if strongest.is_none_or(|(_, _, s)| *st > s) {
                        strongest = Some((tgt, rt, *st));
                    }
                }
                let relations_summary = serde_json::json!({
                    "degree": rels.len(),
                    "strongest": strongest.map(|(t,rt,s)| serde_json::json!({"target":t,"type":rt,"strength":(s*100.0).round()/100.0})),
                    "type_distribution": type_dist,
                });
                let data = serde_json::json!({
                    "id": id,
                    "tier": "primary",
                    "source": ["label"],
                    "similarity": 1.0,
                    "content": payload.content,
                    "labels": payload.labels,
                    "aliases": payload.aliases,
                    "timestamp": payload.timestamp,
                    "metrics": {
                        "importance": (payload.importance * 100.0).round() / 100.0,
                        "memory_type": payload.memory_type,
                        "rationale": payload.rationale,
                        "access_count": payload.access_count,
                        "embedding_dims": payload.embedding.len(),
                        "valid_from": payload.valid_from,
                        "valid_to": payload.valid_to,
                    },
                    "topology": topo,
                    "relations_summary": relations_summary,
                });
                self.smrp_ok("memory_get", data)
            }
            None => self.smrp_err("memory_get", 404, &format!("memory {} not found", id)),
        }
    }

    pub(in crate::engine::mcp) fn tool_memory_list(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let label_filters: Vec<String> = args["labels"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let offset = args["offset"].as_u64().unwrap_or(0) as usize;
        let limit = args["limit"].as_u64().unwrap_or(100) as usize;
        let total = self.engine.space.tetra_count();

        let items: Vec<serde_json::Value> = if label_filters.is_empty() {
            self.engine.scheduler().api_list_recent(offset, limit).into_iter()
                .map(|(id, p)| {
                    let preview: String = p.content.chars().take(120).collect();
                    serde_json::json!({"id": id, "content_preview": preview, "labels": p.labels, "timestamp": p.timestamp, "importance": (p.importance * 100.0).round() / 100.0})
                }).collect()
        } else {
            let refs: Vec<&str> = label_filters.iter().map(|s| s.as_str()).collect();
            self.engine.scheduler().api_list_by_labels(&refs, offset + limit).into_iter()
                .skip(offset)
                .map(|(id, p)| {
                    let preview: String = p.content.chars().take(120).collect();
                    serde_json::json!({"id": id, "content_preview": preview, "labels": p.labels, "timestamp": p.timestamp, "importance": (p.importance * 100.0).round() / 100.0})
                }).collect()
        };
        let data =
            serde_json::json!({"items": items, "total": total, "offset": offset, "limit": limit});
        self.smrp_ok("memory_list", data)
    }

    pub(in crate::engine::mcp) fn tool_memory_update(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let id = match args["id"].as_u64() {
            Some(id) => id,
            None => return self.smrp_err("memory_update", 400, "id is required"),
        };
        if self.engine.space().get_tetrahedron(id).is_none() {
            return self.smrp_err("memory_update", 404, &format!("memory {} not found", id));
        }
        let mut updated = Vec::new();
        let mut label_changed = false;
        if let Some(labels) = args["labels"].as_array() {
            let new_labels: Vec<String> = labels
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            let old_labels = self
                .engine
                .space()
                .get_tetrahedron(id)
                .map(|t| t.data.labels.clone())
                .unwrap_or_default();
            if let Err(e) = self.engine.space().update_labels(id, new_labels.clone()) {
                return self.smrp_err(
                    "memory_update",
                    500,
                    &format!("update labels failed: {}", e),
                );
            }
            if let Err(e) = self
                .engine
                .scheduler
                .storage_handle()
                .update_labels(id, &new_labels)
            {
                tracing::warn!("[MCP] label persist failed for {}: {}", id, e);
                let _ = self.engine.space().update_labels(id, old_labels);
                return self.smrp_err("memory_update", 500, &format!("persist failed: {}", e));
            }
            let final_labels = self
                .engine
                .space()
                .get_tetrahedron(id)
                .map(|t| t.data.labels.clone())
                .unwrap_or_default();
            self.engine.scheduler.gateway_handle().update_label_index(
                id,
                &old_labels,
                &final_labels,
            );
            label_changed = true;
            updated.push("labels");
        }
        if let Some(aliases) = args["aliases"].as_array() {
            let new_aliases: Vec<String> = aliases
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            if let Err(e) = self.engine.space().update_aliases(id, new_aliases.clone()) {
                return self.smrp_err(
                    "memory_update",
                    500,
                    &format!("update aliases failed: {}", e),
                );
            }
            if let Err(e) = self
                .engine
                .scheduler
                .storage_handle()
                .update_aliases(id, &new_aliases)
            {
                tracing::warn!("[MCP] alias persist failed for {}: {}", id, e);
                return self.smrp_err("memory_update", 500, &format!("persist failed: {}", e));
            }
            updated.push("aliases");
        }
        // enforced 标志已从 memory_update 移除（审计：硬约束保护）。
        // enforced 只能通过 enforced_rules 系统机制设置，不允许普通 API key 伪造。
        if args["enforced"].as_bool().is_some() {
            return self.smrp_err("memory_update", 403, "enforced flag cannot be set via memory_update. Use the enforced_rules system mechanism instead.");
        }
        if let Some(content) = args["content"].as_str() {
            let clean = strip_html(content);
            if clean.trim().is_empty() {
                return self.smrp_err("memory_update", 400, "content must not be empty");
            }
            if clean.len() > 10000 {
                return self.smrp_err(
                    "memory_update",
                    400,
                    "content must be under 10000 characters",
                );
            }
            match self.engine.scheduler.api_update_content(id, &clean) {
                Ok(()) => updated.push("content"),
                Err(e) => {
                    return self.smrp_err(
                        "memory_update",
                        500,
                        &format!("update content failed: {}", e),
                    )
                }
            }
        }
        if updated.is_empty() {
            return self.smrp_err(
                "memory_update",
                400,
                "no fields to update — provide content, labels, aliases, and/or enforced",
            );
        }
        let data = serde_json::json!({
            "status": "updated",
            "id": id,
            "fields_updated": updated,
            "side_effects": {
                "cluster_changed": label_changed,
                "label_index_updated": label_changed,
            },
        });
        self.smrp_ok("memory_update", data)
    }

    pub(in crate::engine::mcp) fn tool_memory_delete(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let id = match args["id"].as_u64() {
            Some(id) => id,
            None => return self.smrp_err("memory_delete", 400, "id is required"),
        };
        // 捕获删除前副产物（SMRP §7.2 side_effects）
        let vertices_before = self.engine.space.vertex_count();
        let cluster_before = self.cluster_of(id);
        let rels_before = self.engine.scheduler.api_get_relations(id).len();
        // memory_delete = 永久删除(space+storage+KG+HNSW+index)。软删除请用 memory_forget。
        // Previously this called api_forget_memory (soft, valid_to only), so memory_get
        // still returned the row after a "permanent" delete.
        match self.engine.scheduler.api_delete_memory(id) {
            Ok(mut data) => {
                let vertices_after = self.engine.space.vertex_count();
                data["side_effects"] = serde_json::json!({
                    "vertices_freed": vertices_before.saturating_sub(vertices_after),
                    "cluster_affected": cluster_before.map(|(cid, sz)| serde_json::json!({"id": cid, "size_before": sz})),
                    "relations_removed": rels_before,
                });
                self.smrp_ok("memory_delete", data)
            }
            Err(e) => {
                let code = if e.contains("not found") {
                    404
                } else if e.starts_with(crate::engine::scheduler::PROTECTED_ERR_PREFIX) {
                    403
                } else {
                    400
                };
                self.smrp_err("memory_delete", code, &format!("delete failed: {}", e))
            }
        }
    }

    pub(in crate::engine::mcp) fn tool_memory_export(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let limit = args["limit"].as_u64().unwrap_or(100).min(1000) as usize;
        let all = self.engine.scheduler().api_list_nodes_limit(limit);
        let mut exported: Vec<serde_json::Value> = Vec::new();
        for (id, payload) in &all {
            // 过滤已失效
            if payload.valid_to.is_some() {
                continue;
            }
            // 按 labels 过滤
            if let Some(filter_labels) = args.get("labels").and_then(|v| v.as_array()) {
                let filter: Vec<&str> = filter_labels.iter().filter_map(|v| v.as_str()).collect();
                if !filter.is_empty()
                    && !payload.labels.iter().any(|l| filter.contains(&l.as_str()))
                {
                    continue;
                }
            }
            // 按 memory_class 过滤
            if let Some(filter_class) = args.get("memory_class").and_then(|v| v.as_str()) {
                let actual_class = payload.memory_class.as_deref().unwrap_or("permanent");
                if actual_class != filter_class {
                    continue;
                }
            }
            exported.push(serde_json::json!({
                "id": id,
                "content": &payload.content,
                "labels": &payload.labels,
                "importance": payload.importance,
                "memory_type": payload.memory_type,
                "memory_class": payload.memory_class,
                "timestamp": payload.timestamp,
            }));
        }
        self.smrp_ok(
            "memory_export",
            serde_json::json!({
                "exported": exported.len(),
                "total_scanned": all.len(),
                "memories": exported,
            }),
        )
    }

    pub(in crate::engine::mcp) fn tool_memory_restore(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let id = match args["id"].as_u64() {
            Some(v) => v,
            None => {
                return self.smrp_err(
                    "memory_restore",
                    400,
                    "id is required and must be a positive integer",
                )
            }
        };
        match self.engine.space().get_tetrahedron(id) {
            Some(tetra) => {
                if tetra.data.valid_to.is_none() {
                    return self.smrp_err(
                        "memory_restore",
                        400,
                        "memory is not superseded (valid_to is empty)",
                    );
                }
                let mut data = tetra.data.clone();
                data.valid_to = None;
                data.importance = (data.importance.max(0.5) + 0.5).min(3.0);
                data.invalidated_at = None;
                let _ = self.engine.space().patch_payload(id, &tetra.data, data);
                self.engine.scheduler().gateway_handle().mark_dirty(id);
                self.smrp_ok("memory_restore", serde_json::json!({
                    "id": id, "status": "restored", "message": "Memory restored: valid_to cleared, importance boosted"
                }))
            }
            None => self.smrp_err("memory_restore", 404, "memory not found"),
        }
    }

    pub(in crate::engine::mcp) fn tool_memory_improve(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize; // P1-6: default 50
        let result = self.engine.scheduler().api_improve_memory(limit);
        self.smrp_ok("memory_improve", result)
    }

    pub(in crate::engine::mcp) fn tool_memory_forget(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let id = match args.get("id").and_then(|v| v.as_u64()) {
            Some(id) if id > 0 => id,
            _ => {
                return self.smrp_err(
                    "memory_forget",
                    400,
                    "id is required and must be a positive integer",
                )
            }
        };
        match self.engine.scheduler().api_forget_memory(id) {
            Ok(result) => self.smrp_ok("memory_forget", result),
            Err(e) => self.smrp_err("memory_forget", 404, &e),
        }
    }

    pub(in crate::engine::mcp) fn tool_memory_propose_actions(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        if let Some(e) = self.reject_unknown_args(
            "memory_propose_actions",
            args,
            &[
                "query",
                "limit",
                "depends_on",
                "goal_id",
                "chain",
                "actions",
                "title",
                "required_capabilities",
            ],
        ) {
            return e;
        }
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if query.is_empty() {
            return self.smrp_err("memory_propose_actions", 400, "query is required");
        }
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(3)
            .clamp(1, 5) as usize;

        let hits = match self.engine.scheduler.api_search(query, 8) {
            Ok(h) => h,
            Err(e) => return self.smrp_err("memory_propose_actions", 500, &e),
        };
        let evidence: Vec<(u64, String)> = hits
            .into_iter()
            .map(|(id, _sim, _imp, payload)| (id, payload.content))
            .collect();
        if evidence.is_empty() {
            return self.smrp_ok(
                "memory_propose_actions",
                serde_json::json!({
                    "actions": [],
                    "memory_count": 0,
                    "note": "No memories matched — create or import evidence first, then propose again.",
                }),
            );
        }

        let skills: Vec<(String, f64)> = self
            .engine
            .skills
            .match_skills_scored(query, &self.engine.user_id, 5)
            .into_iter()
            .map(|(sk, score)| (sk.name, score))
            .collect();

        // PR-K: explicit upstream deps for every proposed action, and/or `chain`
        // to make the batch a serial plan (step i depends on step i-1).
        let base_deps: Vec<u64> = args
            .get("depends_on")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_u64()).collect())
            .unwrap_or_default();
        let chain = args.get("chain").and_then(|v| v.as_bool()).unwrap_or(false);
        let frontier = self.engine.scheduler.action_frontier();
        let goal_id = args.get("goal_id").and_then(|v| v.as_u64());
        if let Some(g) = goal_id {
            if let Err(e) = frontier.check_goal_open(g) {
                return self.smrp_conflict("memory_propose_actions", &e);
            }
        }
        for d in &base_deps {
            if frontier.get(*d).is_none() {
                return self.smrp_err(
                    "memory_propose_actions",
                    400,
                    &format!("depends_on: action {} not found", d),
                );
            }
        }
        // Explicit plan from the caller wins; auto-generation is only a fallback.
        // Previously `actions` / `title` / `required_capabilities` were ignored and
        // required_capabilities were derived from skill NAMES, so self-registered
        // workers (capabilities like "rust", "ops") never matched → dispatch 0.
        let top_caps = match self.parse_str_list(
            "memory_propose_actions",
            args,
            "required_capabilities",
            true,
        ) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let explicit_caps = args
            .get("required_capabilities")
            .is_some_and(|v| !v.is_null());
        let title = match args.get("title") {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::String(t)) => {
                Some(t.trim().to_string()).filter(|t| !t.is_empty())
            }
            Some(_) => {
                return self.smrp_err(
                    "memory_propose_actions",
                    400,
                    "invalid params: title must be a string",
                )
            }
        };
        let top_ids: Vec<u64> = evidence.iter().take(5).map(|(id, _)| *id).collect();
        let drafts = match args.get("actions") {
            None | Some(serde_json::Value::Null) => {
                let mut d =
                    crate::engine::agent_action::build_proposals(query, &evidence, &skills, limit);
                for (i, dr) in d.iter_mut().enumerate() {
                    if explicit_caps {
                        dr.required_capabilities = top_caps.clone();
                    }
                    if i == 0 {
                        if let Some(t) = &title {
                            dr.summary = t.clone();
                        }
                    }
                }
                d
            }
            Some(serde_json::Value::Array(items)) => {
                if items.is_empty() || items.len() > 5 {
                    return self.smrp_err(
                        "memory_propose_actions",
                        400,
                        "invalid params: actions must have 1..=5 items",
                    );
                }
                let suggested: Vec<crate::engine::agent_action::SuggestedSkill> = skills
                    .iter()
                    .take(5)
                    .map(|(n, sc)| crate::engine::agent_action::SuggestedSkill {
                        name: n.clone(),
                        score: (*sc * 1000.0).round() / 1000.0,
                    })
                    .collect();
                let mut out = Vec::new();
                for (i, it) in items.iter().enumerate() {
                    const T: &str = "memory_propose_actions";
                    if !it.is_object() {
                        return self.smrp_err(
                            T,
                            400,
                            &format!("invalid params: actions[{i}] must be an object"),
                        );
                    }
                    if let Some(e) = self.reject_unknown_args(
                        T,
                        it,
                        &[
                            "title",
                            "summary",
                            "done_when",
                            "required_capabilities",
                            "priority",
                            "evidence_memory_ids",
                            "stop_if",
                        ],
                    ) {
                        return e;
                    }
                    let gs = |k: &str| {
                        it.get(k)
                            .and_then(|v| v.as_str())
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                    };
                    let t = gs("title");
                    let sm = gs("summary");
                    let summary = match (t, sm) {
                        (Some(t), Some(sm)) => format!("{t}: {sm}"),
                        (Some(x), None) | (None, Some(x)) => x,
                        (None, None) => {
                            return self.smrp_err(
                                T,
                                400,
                                &format!("invalid params: actions[{i}] needs title or summary"),
                            )
                        }
                    };
                    let done_when = match self.parse_str_list(T, it, "done_when", true) {
                        Ok(v) => v,
                        Err(e) => return e,
                    };
                    let stop_if = match self.parse_str_list(T, it, "stop_if", true) {
                        Ok(v) => v,
                        Err(e) => return e,
                    };
                    let caps = if it.get("required_capabilities").is_some() {
                        match self.parse_str_list(T, it, "required_capabilities", true) {
                            Ok(v) => v,
                            Err(e) => return e,
                        }
                    } else {
                        top_caps.clone()
                    };
                    let caps: Vec<String> = caps
                        .into_iter()
                        .map(|c| c.trim().to_string())
                        .filter(|c| !c.is_empty())
                        .collect();
                    let priority = match it.get("priority") {
                        None | Some(serde_json::Value::Null) => 6,
                        Some(v) => match v.as_u64() {
                            Some(p) if p <= 10 => p as u8,
                            _ => {
                                return self.smrp_err(
                                    T,
                                    400,
                                    &format!(
                                        "invalid params: actions[{i}].priority must be 0..=10"
                                    ),
                                )
                            }
                        },
                    };
                    let ev_ids = match it.get("evidence_memory_ids") {
                        None | Some(serde_json::Value::Null) => top_ids.clone(),
                        Some(serde_json::Value::Array(a)) => {
                            let mut ids = Vec::new();
                            for x in a {
                                match x.as_u64() {
                                    Some(id) if self.engine.space.get_tetrahedron(id).is_some() => ids.push(id),
                                    _ => return self.smrp_err(T, 400, &format!("invalid params: actions[{i}].evidence_memory_ids has unknown id {x}")),
                                }
                            }
                            if ids.is_empty() { top_ids.clone() } else { ids }
                        }
                        Some(_) => return self.smrp_err(T, 400, &format!("invalid params: actions[{i}].evidence_memory_ids must be an array of ids")),
                    };
                    out.push(crate::engine::agent_action::AgentActionDraft {
                        summary,
                        evidence_memory_ids: ev_ids,
                        suggested_skills: suggested.clone(),
                        done_when,
                        stop_if,
                        priority,
                        required_capabilities: caps,
                    });
                }
                out
            }
            Some(_) => {
                return self.smrp_err(
                    "memory_propose_actions",
                    400,
                    "invalid params: actions must be an array",
                )
            }
        };
        let mut actions = Vec::new();
        let mut errors = Vec::new();
        let mut prev: Option<u64> = None;
        for draft in drafts {
            let mut action = draft.into_action(query);
            action.depends_on = base_deps.clone();
            action.goal_id = goal_id;
            if chain {
                if let Some(p) = prev {
                    action.depends_on.push(p);
                }
            }
            match frontier.enqueue(action) {
                Ok(a) => {
                    prev = Some(a.id);
                    actions.push(frontier.card(&a));
                }
                Err(e) => {
                    errors.push(e);
                    if chain {
                        break; // a broken chain must not leave later steps unanchored
                    }
                }
            }
        }
        self.engine.scheduler.save_action_frontier();
        self.smrp_ok(
            "memory_propose_actions",
            serde_json::json!({
                "actions": actions,
                "cards": actions, // ActionCard alias — same objects, stable schema
                "memory_count": evidence.len(),
                "evidence_memory_ids": evidence.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
                "errors": errors,
                "action_card_schema": "epicode.action_card/v2",
                "loop": ["memory_propose_actions", "action_claim", "execute", "action_complete"],
            }),
        )
    }
}
