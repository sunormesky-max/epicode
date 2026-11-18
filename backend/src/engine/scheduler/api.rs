use super::*;

impl SchedulerCenter {
    /// D4: 时间感知创建(故事时间) — valid_from=timestamp, 系统时间由gateway内部记录
    pub fn api_create_memory_at(
        &self,
        content: &str,
        labels: Vec<String>,
        timestamp: i64,
    ) -> Result<(TetraId, bool), String> {
        let r = self
            .gateway
            .create_memory_with_time(content, labels, timestamp)?;
        self.persist_tetra(r.id);
        self.gateway.mark_dirty(r.id);
        Ok((r.id, r.is_new))
    }

    /// L1相2d: 批量ingest前预热嵌入缓存(单次批量推理替代N次串行)
    pub fn prewarm_batch_embeddings(&self, texts: &[String]) {
        self.gateway.prewarm_embeddings(texts);
    }

    /// L1图书馆: MCP工具层检索入口(经全局访问器, 无需引擎持有库引用)
    pub fn library_search_public(
        &self,
        query: &str,
        k: usize,
    ) -> Result<Vec<crate::engine::library::LibraryHitPublic>, String> {
        match crate::engine::library::global_library() {
            Some(lib) => lib.search_public(query, k),
            None => Ok(Vec::new()),
        }
    }

    pub fn api_create_memory(
        &self,
        content: &str,
        labels: Vec<String>,
    ) -> Result<(TetraId, bool), String> {
        let r = self.api_create_memory_full(content, labels)?;
        Ok((r.id, r.is_new))
    }

    /// SMRP §7.2 — 完整创建报告，收集全部录入副产物（安置/分类/去重/冲突/建链）。
    pub fn api_create_memory_full(
        &self,
        content: &str,
        mut labels: Vec<String>,
    ) -> Result<CreateReport, String> {
        self.security
            .validate_content(content)
            .map_err(|_| "content validation failed".to_string())?;
        self.security
            .validate_labels(&labels)
            .map_err(|_| "labels validation failed".to_string())?;
        self.security
            .check_constitution_create(!content.is_empty())
            .map_err(|r| format!("constitution violation: {:?}", r))?;

        let intake = crate::engine::intake::MemoryIntake::process(content, &mut labels);

        if intake.is_noise {
            return Err("content rejected as noise (too short or meaningless)".to_string());
        }

        labels = intake.labels;
        let mut auto_labels: Vec<String> = Vec::new();

        let has_domain_label = labels.iter().any(|l| {
            !matches!(
                l.as_str(),
                "general"
                    | "finding"
                    | "decision"
                    | "bug"
                    | "session"
                    | "pattern"
                    | "preference"
                    | "identity"
                    | "system"
                    | "documentation"
            )
        });
        if !has_domain_label {
            match self.cognitive.classify_content(content) {
                Ok(llm_labels)
                    if !llm_labels.is_empty() && llm_labels != vec!["general".to_string()] =>
                {
                    for l in &llm_labels {
                        if !labels.iter().any(|x| x == l) {
                            labels.push(l.clone());
                            auto_labels.push(l.clone());
                        }
                    }
                    tracing::info!(
                        "[Intake] classified: {:?} → {:?}",
                        content.chars().take(40).collect::<String>(),
                        llm_labels
                    );
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("[Intake] classify failed: {}", e);
                }
            }
        }

        let importance = intake.importance;
        let memory_type = intake.memory_type.clone();
        let rationale = intake.rationale.clone();
        let mut dedup_matched: Option<(TetraId, f64)> = None;
        let mut conflicts_marked: Vec<TetraId> = Vec::new();

        // Semantic dedup — search before create (L1相2b: 纯向量knn替代全混合, 1.2s→ms)
        if let Ok(similar) = self.gateway.search_vector_only(content, 3) {
            for (sid, sim, _bm25, payload) in &similar {
                if *sim > 0.85 && payload.content.len() > 20 {
                    let text_sim = crate::engine::intake::MemoryIntake::text_similarity(
                        content,
                        &payload.content,
                    );
                    let threshold = if content.len() < 30 { 0.80 } else { 0.55 };
                    if text_sim > threshold {
                        tracing::info!("[Intake] semantic dedup: new content ≈ #{} (vec_sim={:.2} text_sim={:.2} threshold={:.2}), returning existing",
                            sid, sim, text_sim, threshold);
                        self.energy.replenish(1.0);
                        dedup_matched = Some((*sid, (*sim * 100.0).round() / 100.0));
                        return Ok(CreateReport {
                            id: *sid,
                            is_new: false,
                            placement: None,
                            relations_formed: 0,
                            auto_labels,
                            importance,
                            memory_type,
                            rationale,
                            dedup_matched,
                            conflicts_marked,
                        });
                    }
                }
            }

            // Conflict detection — if new content has negation words AND high similarity to existing
            let conflict_ids =
                crate::engine::intake::MemoryIntake::check_conflict(content, &similar);
            if !conflict_ids.is_empty() {
                let g = self.gateway.create_memory(content, labels.clone())?;
                if g.is_new {
                    if let Some(tetra) = self.space.get_tetrahedron(g.id) {
                        let mut data = tetra.data.clone();
                        data.importance = importance;
                        data.memory_type = intake.memory_type;
                        data.rationale = intake.rationale;
                        if let Err(e) = self.space.patch_payload(g.id, &tetra.data, data) {
                            tracing::warn!("[Scheduler] update_payload {} failed: {}", g.id, e);
                        }
                    }
                    for &cid in &conflict_ids {
                        self.knowledge.add_relation(
                            g.id,
                            cid,
                            crate::engine::knowledge::RelationType::Contradicts,
                            0.8,
                        );
                        let now = chrono::Utc::now().timestamp();
                        let _ = self.space.update_validity(cid, Some(now));
                        self.persist_tetra(cid);
                        tracing::info!(
                            "[Intake] contradiction: #{} supersedes #{}, marking #{} as invalid",
                            g.id,
                            cid,
                            cid
                        );
                    }
                }
                self.persist_tetra(g.id);
                conflicts_marked = conflict_ids;
                return Ok(CreateReport {
                    id: g.id,
                    is_new: g.is_new,
                    placement: g.placement,
                    relations_formed: g.relations_formed,
                    auto_labels,
                    importance,
                    memory_type,
                    rationale,
                    dedup_matched,
                    conflicts_marked,
                });
            }
        }

        let g = self.gateway.create_memory(content, labels)?;

        if g.is_new {
            if let Some(tetra) = self.space.get_tetrahedron(g.id) {
                let mut data = tetra.data.clone();
                data.importance = importance;
                data.memory_type = intake.memory_type;
                data.rationale = intake.rationale;
                if let Err(e) = self.space.patch_payload(g.id, &tetra.data, data) {
                    tracing::warn!("[Scheduler] update_payload {} failed: {}", g.id, e);
                }
            }
        }

        self.persist_tetra(g.id);
        Ok(CreateReport {
            id: g.id,
            is_new: g.is_new,
            placement: g.placement,
            relations_formed: g.relations_formed,
            auto_labels,
            importance,
            memory_type,
            rationale,
            dedup_matched,
            conflicts_marked,
        })
    }

    pub fn api_create_memory_with_time(
        &self,
        content: &str,
        mut labels: Vec<String>,
        timestamp: i64,
    ) -> Result<(TetraId, bool), String> {
        self.security
            .validate_content(content)
            .map_err(|_| "content validation failed".to_string())?;
        self.security
            .validate_labels(&labels)
            .map_err(|_| "labels validation failed".to_string())?;
        self.security
            .check_constitution_create(!content.is_empty())
            .map_err(|r| format!("constitution violation: {:?}", r))?; // 宪法检查（kimi 观察7）

        let intake = crate::engine::intake::MemoryIntake::process(content, &mut labels);

        if intake.is_noise {
            return Err("content rejected as noise".to_string());
        }

        labels = intake.labels;
        let importance = intake.importance;

        // Semantic dedup for historical imports too
        if let Ok(similar) = self.gateway.search(content, 3) {
            for (sid, sim, _bm25, payload) in &similar {
                if *sim > 0.85 && payload.content.len() > 20 {
                    let text_sim = crate::engine::intake::MemoryIntake::text_similarity(
                        content,
                        &payload.content,
                    );
                    if text_sim > 0.55 {
                        tracing::info!(
                            "[Intake] semantic dedup(history): ≈ #{} (vec={:.2} text={:.2})",
                            sid,
                            sim,
                            text_sim
                        );
                        return Ok((*sid, false));
                    }
                }
            }
        }

        let g = self
            .gateway
            .create_memory_with_time(content, labels, timestamp)?;
        let id = g.id;
        let is_new = g.is_new;

        if is_new {
            if let Some(tetra) = self.space.get_tetrahedron(id) {
                let mut data = tetra.data.clone();
                data.importance = importance;
                data.memory_type = intake.memory_type;
                data.rationale = intake.rationale;
                if let Err(e) = self.space.patch_payload(id, &tetra.data, data) {
                    tracing::warn!("[Scheduler] update_payload {} failed: {}", id, e);
                }
            }

            if !intake.conflict_ids.is_empty() {
                for &cid in &intake.conflict_ids {
                    self.knowledge.add_relation(
                        id,
                        cid,
                        crate::engine::knowledge::RelationType::Contradicts,
                        0.8,
                    );
                    let now = chrono::Utc::now().timestamp();
                    let _ = self.space.update_validity(cid, Some(now));
                    self.persist_tetra(cid);
                    tracing::info!(
                        "[Intake] contradiction: #{} supersedes #{}, marking #{} as invalid",
                        id,
                        cid,
                        cid
                    );
                }
            }
        }

        self.persist_tetra(id);
        Ok((id, is_new))
    }

    pub fn api_remember(&self, content: &str) -> Result<(TetraId, Vec<String>), String> {
        self.security
            .validate_content(content)
            .map_err(|_| "content validation failed".to_string())?;
        let labels = if self.cognitive.enabled() {
            self.cognitive
                .classify_content(content)
                .unwrap_or_else(|_| vec!["general".to_string()])
        } else {
            vec!["general".to_string()]
        };
        let g = self.gateway.create_memory(content, labels.clone())?;
        self.persist_tetra(g.id);
        Ok((g.id, labels))
    }

    /// M1修复: 带预设标签的 remember（digestion 用，跳过重复 LLM 分类）。
    /// digestion 已预分类，这里直接用，避免双倍 LLM 调用。
    pub fn api_remember_with_labels(
        &self,
        content: &str,
        pre_labels: Vec<String>,
    ) -> Result<(TetraId, Vec<String>), String> {
        self.security
            .validate_content(content)
            .map_err(|_| "content validation failed".to_string())?;
        let g = self.gateway.create_memory(content, pre_labels.clone())?;
        self.persist_tetra(g.id);
        Ok((g.id, pre_labels))
    }

    /// 记忆forget操作（Cognee启发）—— 显式标记一条记忆为"遗忘"。
    /// 与governor的隐式衰减不同，这是用户/Agent主动决定忘记某条记忆。
    pub fn api_set_memory_class(&self, id: TetraId, class: &str) -> Result<(), String> {
        let tetra = self
            .space
            .get_tetrahedron(id)
            .ok_or_else(|| format!("memory {} not found", id))?;
        let mut data = tetra.data.clone();
        data.memory_class = Some(class.to_string());
        self.space
            .patch_payload(id, &tetra.data, data)
            .map_err(|e| format!("update memory_class failed: {}", e))?;
        self.persist_tetra(id);
        Ok(())
    }

    pub fn api_forget_memory(&self, id: TetraId) -> Result<serde_json::Value, String> {
        let tetra = match self.space.get_tetrahedron(id) {
            Some(t) => t,
            None => return Err(format!("memory #{} not found", id)),
        };
        if tetra.data.enforced {
            return Err(format!(
                "memory #{} is enforced and cannot be forgotten",
                id
            ));
        }

        let mut updated = tetra.data.clone();
        let now = chrono::Utc::now().timestamp();
        updated.valid_to = Some(now); // 标记失效
                                      // Explicit exception to IMPORTANCE_FLOOR: forget is a deliberate tombstone
                                      // (valid_to set). Live decay paths must not use this value.
        updated.importance = crate::engine::governor::FORGET_IMPORTANCE;
        updated.invalidated_at = Some(now);

        match self.space.patch_payload(id, &tetra.data, updated) {
            Ok(_) => {
                self.gateway_handle().mark_dirty(id);
                // P0-2 持久化修复: forget 同步写盘, 保证重启后 valid_to 不丢
                if let Some(t) = self.space.get_tetrahedron(id) {
                    if let Err(e) = self.storage.upsert_tetra(&t) {
                        tracing::warn!("[P0-2] forget persist failed for {}: {}", id, e);
                    }
                }
                tracing::info!(
                    "[Forget] memory #{} explicitly forgotten (valid_to set, importance→FORGET_IMPORTANCE)",
                    id
                );
                Ok(serde_json::json!({
                    "id": id,
                    "forgotten": true,
                    "valid_to": now,
                }))
            }
            Err(e) => Err(format!("failed to forget memory #{}: {}", id, e)),
        }
    }

    pub fn api_search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<(TetraId, f64, f64, MemoryPayload)>, String> {
        self.api_search_filtered(query, limit, None)
    }

    pub fn api_search_filtered(
        &self,
        query: &str,
        limit: usize,
        filters: Option<&crate::engine::search_engine::SearchFilters>,
    ) -> Result<Vec<(TetraId, f64, f64, MemoryPayload)>, String> {
        Ok(self.api_search_inner(query, limit, filters)?.0)
    }

    /// SMRP §6 — 带分数可解释性的搜索（Full 级）。
    #[allow(clippy::type_complexity)]
    pub fn api_search_scored(
        &self,
        query: &str,
        limit: usize,
        filters: Option<&crate::engine::search_engine::SearchFilters>,
    ) -> Result<(Vec<(TetraId, f64, f64, MemoryPayload)>, SearchScoreNotes), String> {
        self.api_search_inner(query, limit, filters)
    }

    #[allow(clippy::type_complexity)]
    pub(in crate::engine::scheduler) fn api_search_inner(
        &self,
        query: &str,
        limit: usize,
        filters: Option<&crate::engine::search_engine::SearchFilters>,
    ) -> Result<(Vec<(TetraId, f64, f64, MemoryPayload)>, SearchScoreNotes), String> {
        self.security
            .validate_query(query)
            .map_err(|_| "query validation failed".to_string())?;
        let intent = crate::engine::retrieval::RetrievalEngine::parse_intent(query);

        // Phase 1 检索可信度重建: exact 模式走专用通道, 短路语义展开 + rerank + KG 扩展
        let mode = filters.map(|f| f.mode).unwrap_or_default();
        if mode == crate::engine::search_engine::SearchMode::Exact {
            // exact: 用原始 query, 不做 expand_query(否则精确 token 被同义词稀释)
            let scored = self
                .gateway
                .search_filtered_with_mode(query, limit * 3, filters)?;
            // 5-tuple → 4-tuple + 提取 matched_by
            let mut results: Vec<(TetraId, f64, f64, MemoryPayload)> =
                Vec::with_capacity(scored.len());
            let mut matched_by_map: std::collections::HashMap<TetraId, Vec<String>> =
                std::collections::HashMap::new();
            for (id, sim, mass, payload, matched_by) in scored {
                if !matched_by.is_empty() {
                    matched_by_map.insert(id, matched_by.iter().map(|s| s.to_string()).collect());
                }
                results.push((id, sim, mass, payload));
            }
            // exact 模式只做 exclude 负过滤(不 rerank, 不 boost)
            crate::engine::retrieval::RetrievalEngine::apply_exclude_only(&mut results, &intent);
            // valid_to 过滤(与其他模式一致的骨架硬化)
            results.retain(|(_, _, _, payload)| {
                payload.valid_to.is_none() && !payload.labels.iter().any(|l| l == "quarantine")
            });
            // P1 搜索硬化: KG 扩展结果也必须通过 filters (D1-D3 修复)
            if let Some(f) = filters {
                results.retain(|(_, _, _, payload)| {
                    crate::engine::search_engine::passes_filters_pub(payload, f)
                });
            }
            results.truncate(limit);
            let notes = SearchScoreNotes {
                matched_by_map,
                ..Default::default()
            };
            return Ok((results, notes));
        }

        if mode == crate::engine::search_engine::SearchMode::Semantic
            || mode == crate::engine::search_engine::SearchMode::Graph
            || mode == crate::engine::search_engine::SearchMode::Auto
            || mode == crate::engine::search_engine::SearchMode::Fusion
        {
            let scored = self
                .gateway
                .search_filtered_with_mode(query, limit * 3, filters)?;
            let mut results: Vec<(TetraId, f64, f64, MemoryPayload)> =
                Vec::with_capacity(scored.len());
            let mut matched_by_map: std::collections::HashMap<TetraId, Vec<String>> =
                std::collections::HashMap::new();
            for (id, sim, mass, payload, matched_by) in scored {
                if !matched_by.is_empty() {
                    matched_by_map.insert(id, matched_by.iter().map(|s| s.to_string()).collect());
                }
                results.push((id, sim, mass, payload));
            }
            results.retain(|(_, _, _, payload)| {
                payload.valid_to.is_none() && !payload.labels.iter().any(|l| l == "quarantine")
            });
            results.truncate(limit);
            let notes = SearchScoreNotes {
                matched_by_map,
                ..Default::default()
            };
            return Ok((results, notes));
        }

        let expanded_query = if intent.expanded_terms.is_empty() {
            query.to_string()
        } else {
            format!("{} {}", query, intent.expanded_terms.join(" "))
        };

        let mut results = self
            .gateway
            .search_filtered(&expanded_query, limit * 3, filters)?;
        crate::engine::retrieval::RetrievalEngine::rerank(&mut results, &intent, limit * 2);

        let clusters = self.find_clusters_cached();
        let query_tokens = crate::engine::search_engine::tokenize(query);
        let cluster_boost = if !clusters.is_empty() {
            let mut best_cluster_id: Option<usize> = None;
            let mut best_cluster_score: f64 = 0.0;
            for (ci, cluster) in clusters.iter().enumerate() {
                let mut score = 0.0;
                for &tid in &cluster.tetra_ids {
                    if let Some(t) = self.space.get_tetrahedron(tid) {
                        let content_lower = t.data.content.to_lowercase();
                        let matches = query_tokens
                            .iter()
                            .filter(|w| content_lower.contains(w.as_str()))
                            .count();
                        score += matches as f64 * t.data.importance;
                    }
                }
                if score > best_cluster_score {
                    best_cluster_score = score;
                    best_cluster_id = Some(ci);
                }
            }
            best_cluster_id.map(|ci| {
                clusters[ci]
                    .tetra_ids
                    .iter()
                    .copied()
                    .collect::<std::collections::HashSet<u64>>()
            })
        } else {
            None
        };

        let mut notes = SearchScoreNotes::default();
        if let Some(ref boost_ids) = cluster_boost {
            for (id, sim, _, _) in &mut results {
                if boost_ids.contains(id) {
                    *sim += 0.08;
                    notes.cluster_boosted.push(*id);
                }
            }
            results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        }

        for (id, sim, _, payload) in &mut results {
            if payload.importance >= 2.5 {
                *sim += 0.06;
                notes.importance_boosted.push(*id);
            }
            if payload.access_count > 5 {
                *sim += 0.04;
                notes.access_boosted.push(*id);
            }
            if payload
                .labels
                .iter()
                .any(|l| l == "outdated" || l == "superseded")
            {
                *sim -= 0.3;
                notes.penalized.push(*id);
            }
        }
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        results.retain(|(id, sim, _bm25, payload)| {
            // P4 治理: 排除 quarantine 记忆（默认不可见）
            if payload.labels.iter().any(|l| l == "quarantine") {
                return false;
            }
            // 骨架硬化：完全过滤已失效记忆（valid_to 有值 = superseded/矛盾/去重）
            if payload.valid_to.is_some() {
                return false;
            }
            if payload.importance < 0.1 && payload.content.len() < 15 {
                tracing::info!(
                    "[Search] filtered low-quality id={} (importance={:.2})",
                    id,
                    payload.importance
                );
                false
            } else {
                *sim >= 0.0
            }
        });

        // ── 突破1: multi_hop 多跳推理扩展 ──
        // 向量检索结果作为 seeds，沿知识图谱扩展2跳，把推理关联的记忆merge进结果。
        // KG 扩展结果 sim 乘 0.7（间接关联可信度折扣），不会挤掉直接向量命中。
        // 设计来自大卫#1189"记忆系统智能化第一波"，multi_hop 已实现于 knowledge.rs:296。
        {
            let kg = self.kg_handle();
            let seed_ids: Vec<TetraId> = results.iter().take(10).map(|(id, _, _, _)| *id).collect();
            let expanded = kg.multi_hop_adaptive(&seed_ids, limit);
            let existing: std::collections::HashSet<TetraId> =
                results.iter().map(|(id, _, _, _)| *id).collect();
            let mut kg_results: Vec<(TetraId, f64, f64, MemoryPayload)> = Vec::new();
            for (exp_id, strength) in &expanded {
                if existing.contains(exp_id) {
                    continue;
                }
                if let Some(tetra) = self.space.get_tetrahedron(*exp_id) {
                    let kg_sim = strength * 0.7;
                    if kg_sim >= 0.0 {
                        notes.kg_expanded.push(*exp_id);
                        kg_results.push((*exp_id, kg_sim, 0.0, tetra.data));
                    }
                }
            }
            if !kg_results.is_empty() {
                tracing::info!(
                    "[Search] multi_hop expanded {} KG results from {} seeds",
                    kg_results.len(),
                    seed_ids.len()
                );
                results.extend(kg_results);
                results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            }
        }

        // P1 搜索硬化: KG 扩展结果也必须通过 filters (D1-D3 修复)
        if let Some(f) = filters {
            results.retain(|(_, _, _, payload)| {
                crate::engine::search_engine::passes_filters_pub(payload, f)
            });
        }
        results.truncate(limit);

        // ── 突破4: knowledge-gap 沉淀 ──
        // 搜索结果为空或极低质量时，自动记录一条 knowledge-gap 记忆。
        // 下次相关检索时，knowledge-gap 记忆会浮现，提示"这里存在知识空白"。
        // 设计来自大卫#1189。限流：靠 content_hash 去重(相同 query 不重复创建)。
        if results.is_empty() && !query.trim().is_empty() && query.len() < 200 {
            // L1加固: 垃圾查询防护 — 无CJK+无空格分词+长单块(密文/编码碎片)只记日志不入库,
            // 防客户端用垃圾查询批量制造knowledge-gap污染(实测教训: 密文查询会变成待学习条目)
            let han = query
                .chars()
                .filter(|c| ('\u{4e00}'..='\u{9fff}').contains(c))
                .count();
            let words = query.split_whitespace().count();
            let garbage = han == 0 && words <= 1 && query.chars().count() > 40;
            if garbage {
                tracing::warn!(
                    "[Search] knowledge-gap suppressed (garbage-like, len={}): {:?}",
                    query.chars().count(),
                    query.chars().take(12).collect::<String>()
                );
            } else {
                let gap_content = format!("[knowledge-gap] 待学习：{}", query);
                let gap_labels = vec!["knowledge-gap".to_string(), "system".to_string()];
                // 同步创建(低频:仅搜索完全无结果时触发)
                let _ = self.gateway.create_memory(&gap_content, gap_labels);
                tracing::info!(
                    "[Search] knowledge-gap recorded: {}",
                    query.chars().take(60).collect::<String>()
                );
            }
        }

        // 骨架硬化：access_count 更新改为内存缓冲，不阻塞搜索热路径
        // 之前在搜索返回前做 10 次 update_payload + persist_tetra——严重的写污染
        // 现在只更新内存中的 access_counts（搜索评分已用），不触发 Space 写锁和 DB 写
        // last_reviewed_ts 的持久化由 governor 的 apply_decay 在 tick 中异步处理
        // （governor 读 access_count > 0 的记忆，重置衰减时间——不需要搜索时写入）
        Ok((results, notes))
    }

    pub fn api_get_node(&self, id: TetraId) -> Option<MemoryPayload> {
        self.gateway.get_node(id)
    }

    pub fn api_list_nodes(&self) -> Vec<(TetraId, MemoryPayload)> {
        self.api_list_nodes_limit(5000)
    }

    pub fn api_list_nodes_limit(&self, limit: usize) -> Vec<(TetraId, MemoryPayload)> {
        let mut all = self.gateway.list_nodes();
        all.truncate(limit);
        all
    }

    /// 清道夫系统：扫描全库，诊断并 supersede 垃圾记忆（不删除）
    /// O-A+O1 记忆自愈(推演v2病灶A/F): 重要性塌缩回滚 + 隔离池再消化。
    /// 由 POST /admin/heal-memory 触发(平台管理员), 幂等可重放。
    /// - importance < floor 的记忆回滚到 floor(检索除名 → 降籍复权)
    /// - quarantine 标签且内容非垃圾(≥20字符)的记忆: 摘标签 + mass恢复 + 复权
    ///   (打破"quarantine条件imp<0.3 → 每轮体检re-quarantine"的永久隔离循环)
    pub fn heal_memory(&self, floor: f64) -> serde_json::Value {
        use crate::engine::governor::IMPORTANCE_FLOOR;
        let floor = if floor <= 0.0 {
            IMPORTANCE_FLOOR
        } else {
            floor
        };
        let tetras = self.space.all_tetrahedrons();
        let mut rolled_back = 0usize;
        let mut unquarantined = 0usize;
        let mut still_quarantined = 0usize;
        for t in &tetras {
            let mut data = t.data.clone();
            let mut changed = false;
            if data.importance < floor {
                data.importance = floor;
                changed = true;
                rolled_back += 1;
            }
            // 隔离池再消化: 有实质内容(≥20字符)且非junk/superseded的quarantine记忆
            // 摘牌恢复公民权; 真垃圾(junk标签或空内容)保留隔离
            let is_quarantined = data.labels.iter().any(|l| l == "quarantine");
            if is_quarantined {
                let is_junk = data.labels.iter().any(|l| l == "junk");
                let substantial = data.content.chars().count() >= 20;
                if !is_junk && substantial {
                    data.labels.retain(|l| l != "quarantine");
                    if data.importance < floor {
                        data.importance = floor;
                    }
                    let _ = self.space.update_mass(t.id, 0.5);
                    unquarantined += 1;
                    changed = true;
                } else {
                    still_quarantined += 1;
                }
            }
            if changed {
                let _ = self.space.patch_payload(t.id, &t.data, data);
                if let Some(updated) = self.space.get_tetrahedron(t.id) {
                    let _ = self.storage.upsert_tetra(&updated);
                }
            }
        }
        tracing::info!(
            "[Heal] floor={}: rolled_back={} unquarantined={} still_quarantined={}",
            floor,
            rolled_back,
            unquarantined,
            still_quarantined
        );
        serde_json::json!({
            "rolled_back": rolled_back,
            "unquarantined": unquarantined,
            "still_quarantined": still_quarantined,
            "total": tetras.len(),
            "floor": floor,
        })
    }

    pub fn api_scavenge(&self) -> serde_json::Value {
        let all = self.space.all_tetrahedrons();
        let now = chrono::Utc::now().timestamp();
        let mut duplicates = 0;
        let mut empties = 0;
        let mut stale_superseded = 0;
        let mut woke_up = 0;
        let mut report: Vec<String> = Vec::new();

        // 1. 检测重复 content_hash + supersede（保留最高 importance 的）
        let mut hash_groups: HashMap<u64, Vec<(u64, f64, Option<i64>)>> = HashMap::new();
        for t in &all {
            let entry = hash_groups.entry(t.data.content_hash).or_default();
            entry.push((t.id, t.data.importance, t.data.valid_to));
        }
        for (hash, group) in &hash_groups {
            if group.len() > 1 && *hash != 0 {
                // 保留 importance 最高的，supersede 其余
                let mut sorted = group.clone();
                sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                for (id, _, valid_to) in sorted.iter().skip(1) {
                    if valid_to.is_none() {
                        if let Some(tetra) = self.space.get_tetrahedron(*id) {
                            let mut data = tetra.data.clone();
                            data.valid_to = Some(now);
                            data.importance = crate::engine::governor::FORGET_IMPORTANCE;
                            let _ = self.space.patch_payload(*id, &tetra.data, data);
                            self.persist_tetra(*id);
                            duplicates += 1;
                        }
                    }
                }
                report.push(format!(
                    "重复组 hash={}：{}条记忆，supersede {}条",
                    hash,
                    group.len(),
                    group.len() - 1
                ));
            }
        }

        // 2. 检测空/碎片内容（<5字）
        for t in &all {
            if t.data.content.trim().len() < 5 && t.data.valid_to.is_none() {
                let mut data = t.data.clone();
                data.valid_to = Some(now);
                data.importance = crate::engine::governor::FORGET_IMPORTANCE;
                let _ = self.space.patch_payload(t.id, &t.data, data);
                self.persist_tetra(t.id);
                empties += 1;
            }
        }
        if empties > 0 {
            report.push(format!("碎片记忆（<5字）：{}条已 supersede", empties));
        }

        // 3. 已 superseded 的高 importance 降权 (tombstone floor)
        for t in &all {
            if t.data.valid_to.is_some()
                && t.data.importance > crate::engine::governor::FORGET_IMPORTANCE
            {
                let mut data = t.data.clone();
                data.importance = crate::engine::governor::FORGET_IMPORTANCE;
                let _ = self.space.patch_payload(t.id, &t.data, data);
                self.persist_tetra(t.id);
                stale_superseded += 1;
            }
        }
        if stale_superseded > 0 {
            report.push(format!(
                "已失效但高重要性：{}条已降权到 FORGET_IMPORTANCE",
                stale_superseded
            ));
        }

        // 4. 唤醒沉睡记忆（access_count=0 → access_count=1，给它们被搜索到的机会）
        for t in &all {
            if t.data.access_count == 0 && t.data.valid_to.is_none() {
                let mut data = t.data.clone();
                data.access_count = 1;
                let _ = self.space.patch_payload(t.id, &t.data, data);
                self.persist_tetra(t.id);
                woke_up += 1;
            }
        }
        if woke_up > 0 {
            report.push(format!("沉睡记忆唤醒：{}条 access_count 0→1", woke_up));
        }

        serde_json::json!({
            "scanned": all.len(),
            "duplicates_superseded": duplicates,
            "empties_superseded": empties,
            "stale_downgraded": stale_superseded,
            "woke_up": woke_up,
            "report": report,
        })
    }

    pub fn api_list_by_labels(
        &self,
        labels: &[&str],
        limit: usize,
    ) -> Vec<(TetraId, MemoryPayload)> {
        self.gateway.list_by_labels(labels, limit)
    }

    pub fn api_list_recent(&self, offset: usize, limit: usize) -> Vec<(TetraId, MemoryPayload)> {
        self.gateway.list_recent(offset, limit)
    }

    pub fn api_stats(&self) -> crate::engine::gateway::SpaceStats {
        self.gateway.stats()
    }

    /// 记忆improve操作（Cognee启发）—— 主动改写低质量记忆使其更清晰。
    /// 找到内容过短/无标签的记忆，用LLM重写为更完整版本。
    /// 返回改进的记忆数量。
    /// L3 Skill Learning（Letta启发）—— 从认知决策成功经验自动提取可复用技能。
    ///
    /// 分析最近的决策历史，找出反复"effective"的决策模式，
    /// 用 LLM 将其泛化为技能文档（skill.md），存入技能库。
    /// 这让系统能从自己的成功经验中学习"什么时候该做什么"。
    pub fn api_extract_skill(&self) -> serde_json::Value {
        let history = self.decision_history.lock().clone();

        if history.is_empty() {
            return serde_json::json!({
                "extracted": 0,
                "reason": "no decision history available",
            });
        }

        // 按action类型分组，统计effective/no_effect
        use std::collections::HashMap;
        let mut action_stats: HashMap<String, (usize, usize)> = HashMap::new();
        for d in &history {
            let entry = action_stats
                .entry(d.action.split('(').next().unwrap_or(&d.action).to_string())
                .or_insert((0, 0));
            if d.result == "effective" {
                entry.0 += 1;
            } else {
                entry.1 += 1;
            }
        }

        // 找出成功率>60%且有>=2次effective的action
        let mut good_patterns: Vec<(String, usize, usize, f64)> = Vec::new();
        for (action, (effective, no_effect)) in &action_stats {
            let total = effective + no_effect;
            if total >= 2 && *effective as f64 / total as f64 > 0.6 {
                good_patterns.push((
                    action.clone(),
                    *effective,
                    *no_effect,
                    *effective as f64 / total as f64,
                ));
            }
        }

        if good_patterns.is_empty() {
            return serde_json::json!({
                "extracted": 0,
                "reason": "no patterns with >60% effectiveness found",
                "history_size": history.len(),
                "action_stats": action_stats.iter()
                    .map(|(k, (e, n))| (k.clone(), serde_json::json!({"effective": e, "no_effect": n})))
                    .collect::<HashMap<_, _>>(),
            });
        }

        // 用LLM将有效模式泛化为技能文档
        let pattern_summary = good_patterns
            .iter()
            .map(|(action, eff, noeff, rate)| {
                format!(
                    "- {}: {}/{} effective ({:.0}% success)",
                    action,
                    eff,
                    eff + noeff,
                    rate * 100.0
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let recent_decisions = history
            .iter()
            .rev()
            .take(10)
            .map(|d| format!("  tick {}: {} -> {}", d.tick, d.action, d.result))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "Based on the cognitive engine's decision history, extract a reusable skill.\n\n             ## Effective Decision Patterns\n{}\n\n             ## Recent Decisions\n{}\n\n             Write a concise skill document in markdown format that captures WHEN to use these effective actions.\n             Format: start with frontmatter (---\\nname: auto-extracted-skill\\ndescription: ...\\n---) then the skill body.\n             Keep it under 300 words. Focus on the trigger conditions and the action to take.",
            pattern_summary, recent_decisions
        );

        let skill_md = match self.cognitive.answer_from_memories(&prompt, "") {
            Ok(content) => {
                let cleaned = content
                    .strip_prefix("<think>")
                    .and_then(|s| s.split("</think>").next())
                    .unwrap_or(&content)
                    .trim()
                    .to_string();
                if cleaned.len() > 50 {
                    cleaned
                } else {
                    return serde_json::json!({
                        "extracted": 0,
                        "reason": "LLM produced too short output",
                        "patterns": good_patterns,
                    });
                }
            }
            Err(e) => {
                return serde_json::json!({
                    "extracted": 0,
                    "reason": format!("LLM failed: {}", e),
                    "patterns": good_patterns,
                });
            }
        };

        // 从skill_md中提取name
        let skill_name = skill_md
            .lines()
            .find_map(|l| {
                let l = l.trim();
                l.strip_prefix("name:")
                    .map(|rest| rest.trim().trim_matches('"').to_string())
            })
            .unwrap_or_else(|| format!("auto-skill-{}", chrono::Utc::now().timestamp() % 100000));

        // 存入技能库
        let owner = self
            .space
            .identity_info()
            .map(|i| i.system_name.clone())
            .unwrap_or_else(|| "system".to_string());

        let skills_lock = self.skills.lock();
        let skills_engine = match skills_lock.as_ref() {
            Some(se) => se.clone(),
            None => {
                return serde_json::json!({
                    "extracted": 0,
                    "reason": "skill engine not initialized",
                    "patterns": good_patterns,
                })
            }
        };
        drop(skills_lock);
        let skill = skills_engine.create(skill_name.clone(), skill_md.clone(), owner);
        {
            tracing::info!(
                "[L3] auto-extracted skill #{} '{}' from {} effective patterns (history={})",
                skill.id,
                skill.name,
                good_patterns.len(),
                history.len()
            );
            serde_json::json!({
                "extracted": 1,
                "skill_id": skill.id,
                "skill_name": skill.name,
                "patterns_analyzed": good_patterns.len(),
                "history_size": history.len(),
                "patterns": good_patterns.iter().map(|(a, e, n, r)| serde_json::json!({
                    "action": a, "effective": e, "total": e + n, "success_rate": r
                })).collect::<Vec<_>>(),
            })
        }
    }

    pub fn api_improve_memory(&self, limit: usize) -> serde_json::Value {
        let all = self.space.all_tetrahedrons();

        // 找到低质量记忆：内容<40字符 或 无标签 或 标签只有"general"
        let low_quality: Vec<_> = all
            .iter()
            .filter(|t| {
                let content_len = t.data.content.trim().len();
                let labels = &t.data.labels;
                let is_low_content = content_len < 40;
                let is_no_labels = labels.is_empty();
                let is_only_general = labels.len() == 1 && labels[0] == "general";
                let not_superseded = t.data.valid_to.is_none();
                let not_enforced = !t.data.enforced;
                not_superseded
                    && not_enforced
                    && (is_low_content || is_no_labels || is_only_general)
            })
            .take(limit)
            .collect();

        let candidates = low_quality.len();
        let mut improved = 0usize;
        let mut details = Vec::new();

        for t in &low_quality {
            let original = &t.data.content;
            // 用认知引擎重写：扩展为更清晰的描述
            let prompt = format!(
                "Rewrite this memory to be clearer and more complete (max 200 chars).                  Keep the original meaning but add context. Original: \"{}\"",
                original
            );

            match self.cognitive.answer_from_memories(&prompt, "") {
                Ok(rewritten) => {
                    let cleaned = rewritten.trim();
                    if cleaned.len() > original.trim().len() && cleaned.len() < 300 {
                        let updated_content = format!(
                            "{}\n[improved from: {}]",
                            cleaned,
                            original.chars().take(60).collect::<String>()
                        );

                        let still_unprotected =
                            self.space.get_tetrahedron(t.id).is_some_and(|current| {
                                !current.data.enforced
                                    && current.data.valid_to.is_none()
                                    && current.data.content == *original
                            });
                        if !still_unprotected {
                            continue;
                        }

                        if let Err(e) = self.api_update_content(t.id, &updated_content) {
                            tracing::warn!("[Improve] update failed for #{}: {}", t.id, e);
                        } else {
                            improved += 1;
                            details.push(format!(
                                "#{}: {}→{} chars",
                                t.id,
                                original.len(),
                                cleaned.len()
                            ));
                        }
                    }
                }
                Err(e) => {
                    tracing::debug!("[Improve] LLM rewrite failed for #{}: {}", t.id, e);
                }
            }
        }

        serde_json::json!({
            "scanned": all.len(),
            "candidates": candidates,
            "improved": improved,
            "details": details,
        })
    }

    pub fn cognitive_snapshot(&self) -> (serde_json::Value, serde_json::Value, String, String) {
        let emotion = {
            let e = self.emotion.lock();
            serde_json::json!({
                "pleasure": e.pleasure,
                "arousal": e.arousal,
                "dominance": e.dominance,
                "quadrant": e.quadrant(),
                "label": e.to_label(),
            })
        };
        let drive = {
            let d = self.drive.lock();
            serde_json::json!({
                "dominant": format!("{:?}", d.dominant()),
            })
        };
        let cognitive_status = if self.cognitive.enabled() {
            if self.cognitive.is_degraded() {
                "degraded"
            } else {
                "active"
            }
        } else {
            "disabled"
        }
        .to_string();
        let latest_thought = {
            let hist = self.decision_history.lock();
            hist.last().map(|d| d.detail.clone()).unwrap_or_default()
        };
        (emotion, drive, cognitive_status, latest_thought)
    }

    /// 决策历史数量（供 SSE 推送）
    pub fn decision_history_count(&self) -> usize {
        self.decision_history.lock().len()
    }

    /// 决策历史快照（供 /v1/cognitive/state API）
    pub fn decision_history_snapshot(&self, limit: usize) -> Vec<serde_json::Value> {
        let hist = self.decision_history.lock();
        hist.iter()
            .rev()
            .take(limit)
            .map(|d| {
                serde_json::json!({
                    "tick": d.tick,
                    "action": d.action,
                    "detail": d.detail,
                    "result": d.result,
                })
            })
            .collect()
    }

    /// find_clusters（缓存由 Space 层按 structure_version 失效，所有调用方共享）。
    pub fn find_clusters_cached(&self) -> Vec<crate::domain::space::Cluster> {
        self.space.find_clusters()
    }

    pub fn api_load_context(&self, limit: usize) -> Vec<(TetraId, f64, String, Vec<String>)> {
        let session = self
            .gateway
            .list_by_labels(&["session-summary", "session"], 3);
        let decisions = self.gateway.list_by_labels(&["decision"], 10);
        let patterns = self.gateway.list_by_labels(&["pattern"], 6);
        let identity = self.gateway.list_by_labels(&["identity", "system"], 2);
        let project = self
            .gateway
            .list_by_labels(&["project-context", "architecture"], 2);
        let bugs = self.gateway.list_by_labels(&["bug"], 5);
        let enforced = self.gateway.get_enforced_patterns();

        let mut all_memories: Vec<(u64, MemoryPayload)> = Vec::new();
        for (id, p) in &session {
            all_memories.push((*id, p.clone()));
        }
        for (id, p) in &decisions {
            all_memories.push((*id, p.clone()));
        }
        for (id, p) in &patterns {
            all_memories.push((*id, p.clone()));
        }
        for (id, p) in &identity {
            all_memories.push((*id, p.clone()));
        }
        for (id, p) in &project {
            all_memories.push((*id, p.clone()));
        }
        for (id, p) in &bugs {
            all_memories.push((*id, p.clone()));
        }

        let narrative = crate::engine::assembler::ContextAssembler::assemble(
            &all_memories,
            &enforced,
            limit,
            "general",
        );

        let result = vec![(0u64, 1.0, narrative, vec!["assembled-context".to_string()])];
        result
    }

    pub fn api_get_enforced_rules(&self) -> Vec<(TetraId, String, Vec<String>)> {
        self.gateway.get_enforced_patterns()
    }

    pub fn api_list_projects(&self) -> Vec<(String, usize)> {
        self.gateway.list_projects()
    }

    pub fn api_get_relations(&self, id: TetraId) -> Vec<(TetraId, String, f64)> {
        self.gateway.get_relations(id)
    }

    /// 有向关系(wander 自由联想游走用)。
    pub fn api_get_relations_directed(&self, id: TetraId) -> Vec<(TetraId, String, f64)> {
        self.gateway.get_relations_directed(id)
    }

    /// 谱远跳候选(wander 自由联想用)。
    pub fn api_spectral_far_jump(
        &self,
        from: TetraId,
        visited: &[TetraId],
    ) -> Option<(TetraId, f64)> {
        self.gateway.spectral_far_jump(from, visited)
    }

    /// 时代跳候选(wander era 纵深联想用)。
    pub fn api_era_jump(&self, from: TetraId, visited: &[TetraId]) -> Option<(TetraId, f64)> {
        self.gateway.era_jump(from, visited)
    }

    /// w 轴原点(全库最早时间戳; era 策略评分用)。
    pub fn api_space_t0(&self) -> Option<i64> {
        self.gateway.space_t0()
    }

    pub fn api_get_concepts(&self) -> Vec<(String, usize)> {
        self.gateway.get_concepts()
    }

    pub fn api_graph_stats(&self) -> (usize, usize) {
        (
            self.gateway.relation_count_kg(),
            self.gateway.concept_count_kg(),
        )
    }

    pub fn api_export_graph(&self, node_limit: usize) -> crate::engine::knowledge::GraphExport {
        self.gateway.export_graph(node_limit)
    }

    // ============================================================
    // 档案库 API — 大型记忆聚合的操作快捷方式
    // 本质全部是记忆操作（创建/修改/关联/搜索）的组合
    // ============================================================

    pub fn api_pulse(
        &self,
        origin: TetraId,
        ttl: u32,
    ) -> Result<crate::domain::pulse::PulseResult, String> {
        self.gateway.pulse(origin, ttl)
    }

    pub fn api_purge_memory(&self, id: TetraId) -> Result<TetraId, String> {
        if self.space.get_tetrahedron(id).is_none() {
            return Err(format!("memory {} not found", id));
        }
        if self
            .space
            .get_tetrahedron(id)
            .map(|t| t.data.enforced)
            .unwrap_or(false)
        {
            return Err(format!(
                "{PROTECTED_ERR_PREFIX}cannot purge enforced memory (it is a hard constraint)"
            ));
        }
        if self
            .space
            .get_tetrahedron(id)
            .map(|t| t.data.labels.iter().any(|l| l == "identity"))
            .unwrap_or(false)
        {
            return Err(format!(
                "{PROTECTED_ERR_PREFIX}memory #{id} is an identity anchor and cannot be purged; use forget (recoverable) instead"
            ));
        }
        self.purge_tetra(id);
        Ok(id)
    }

    /// Unified permanent delete used by BOTH REST `DELETE /v1/memories/:id` and MCP
    /// `memory_delete`: purge from space/storage/KG/index and report `mode: "purge"`.
    pub fn api_delete_memory(&self, id: TetraId) -> Result<serde_json::Value, String> {
        let relations_before = self.api_get_relations(id).len();
        self.api_purge_memory(id)?;
        Ok(serde_json::json!({
            "status": "deleted",
            "id": id,
            "mode": DELETE_MODE,
            "relations_removed": relations_before,
        }))
    }

    pub fn api_update_content(&self, id: TetraId, new_content: &str) -> Result<(), String> {
        self.security
            .validate_content(new_content)
            .map_err(|_| "content validation failed".to_string())?;

        if self.space.get_tetrahedron(id).is_none() {
            return Err(format!("memory {} not found", id));
        }

        self.gateway.update_content(id, new_content)?;
        self.persist_tetra(id);
        Ok(())
    }
}
