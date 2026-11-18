use super::*;

impl SchedulerCenter {
    /// D3: dream第四相 — 预判问题预演
    /// 采样近7日真实查询→LLM生成可能被问的问题→执行检索→低分题=盲区→KG建桥标记
    pub(in crate::engine::scheduler) fn dream_anticipate(&self) -> usize {
        let m = self.gateway.search_metrics();
        let mut recent_queries: Vec<String> =
            m.miss_queries.iter().rev().take(5).cloned().collect();
        // miss为空时降级到热标签(5500+记忆的空间几乎不miss — 纯miss触发是设计缺陷)
        if recent_queries.is_empty() {
            recent_queries = m
                .top_labels
                .iter()
                .take(5)
                .map(|(l, _)| l.clone())
                .collect();
        }
        if recent_queries.is_empty() {
            return 0;
        }

        let prompt = format!(
            "Based on these real user queries to a memory system:
{}

Generate 5 questions the user will likely ask next. One per line, no numbering.",
            recent_queries
                .iter()
                .map(|q| format!("- {}", q))
                .collect::<Vec<_>>()
                .join(
                    "
"
                )
        );
        let generated = match self.cognitive.generate_free_text(&prompt, 400) {
            Ok(text) => text,
            Err(_) => return 0,
        };
        let questions: Vec<String> = generated
            .lines()
            .map(|l| {
                l.trim()
                    .trim_start_matches(|c: char| {
                        c.is_ascii_digit() || c == '.' || c == '-' || c == '*'
                    })
                    .trim()
                    .to_string()
            })
            .filter(|l| l.len() > 5 && !l.is_empty())
            .take(5)
            .collect();
        if questions.is_empty() {
            return 0;
        }

        let mut blind_spots = 0;
        for q in &questions {
            let (results, _) = self.api_search_scored(q, 5, None).unwrap_or_default();
            let top_score = results
                .iter()
                .map(|(_, s, _, _)| *s)
                .fold(0.0_f64, f64::max);
            // 低分=系统对这个预判问题是盲区→标记(下轮意识/dream可建桥)
            if top_score < 0.25 {
                blind_spots += 1;
                tracing::info!(
                    "[Dream] anticipate blind-spot: '{}' (top_score={:.3})",
                    q,
                    top_score
                );
            }
        }
        tracing::info!(
            "[Dream] Phase 4 anticipation: {} questions, {} blind spots",
            questions.len(),
            blind_spots
        );
        blind_spots
    }

    pub fn api_dream(&self, dry_run: bool) -> Result<String, String> {
        // R4-S04: align with auto_pipeline::auto_dream — actually consume, not only check.
        // dry_run stays free so agents can preview consolidation without draining budget.
        let _dream_permit = if dry_run {
            None
        } else {
            match self.dream_gate.try_acquire() {
                Some(p) => Some(p),
                None => return Err("dream cycle already running".to_string()),
            }
        };
        if !dry_run && !self.energy.consume(15.0) {
            return Err("insufficient energy (need 15.0)".to_string());
        }
        if !dry_run {
            // 手动 dream 也计入节拍,避免紧接着 auto_dream 重复跑一轮
            self.last_dream_tick
                .store(self.tick_count.load(Ordering::SeqCst), Ordering::SeqCst);
        }
        let report =
            crate::engine::dream::DreamEngine::cycle(&self.space, &self.knowledge, 0.3, 5, dry_run);

        if !dry_run {
            for &id in report
                .evicted_ids
                .iter()
                .chain(report.merged_remove_ids.iter())
            {
                self.persist_tetra(id);
            }
        }

        let importance_updated = if !dry_run {
            // ── D7.2: dream第五相 知识卡片(参数记忆层) ──
            let _ = self.generate_knowledge_cards();
            // ── D3: dream第四相 预判问题预演 ──
            let _ = self.dream_anticipate();
            let access_counts: std::collections::HashMap<u64, u32> = self
                .gateway
                .search_metrics()
                .hot_memories
                .into_iter()
                .collect();
            let updated = crate::engine::dream::DreamEngine::recompute_importance(
                &self.space,
                &access_counts,
            );
            let tetras = self.space.all_tetrahedrons();
            let label_data: Vec<(TetraId, Vec<String>)> = tetras
                .iter()
                .map(|t| (t.id, t.data.labels.clone()))
                .collect();
            self.knowledge.replace_concepts(&label_data);
            // 计算 concept centroid（embedding 均值），之前断联：永远 vec![]
            self.knowledge.recompute_centroids(&self.space);
            let _ = self.tx.send(EngineEvent::DecisionTick);

            let stats = self.gateway.stats();
            let feedback_mems = self.gateway.list_by_labels(&["feedback"], 50);
            let all_mems = self.gateway.list_nodes();
            let avg_imp = if !all_mems.is_empty() {
                all_mems.iter().map(|(_, p)| p.importance).sum::<f64>() / all_mems.len() as f64
            } else {
                0.0
            };
            let enforced = self.gateway.get_enforced_patterns().len();
            let _ = self.storage.save_health_snapshot(
                stats.tetra_count as i64,
                stats.clusters as i64,
                feedback_mems.len() as i64,
                avg_imp,
                enforced as i64,
            );
            updated
        } else {
            0
        };

        let mut insights = report.insights;
        insights.sort_by(|a, b| {
            let score = |s: &str| -> f64 {
                let mut v = 0.0f64;
                if s.contains("merged") || s.contains("consolidated") {
                    v += 3.0;
                }
                if s.contains("evicted") || s.contains("junk") {
                    v += 2.0;
                }
                if s.contains("cluster") {
                    v += 1.5;
                }
                if s.contains("similar pairs") {
                    v += 1.0;
                }
                // P3-3: 皮层化洞察必须可见——0 分会被大量 "cluster of N" 洞察
                // (各 1.5 分)在 truncate(20) 时挤掉, 报告里永远看不到 digest 数
                if s.contains("corticalized") {
                    v += 4.0;
                }
                // Phase 4.6: 联想重放同理——睡眠巩固证据必须可见
                if s.contains("associative replay") {
                    v += 4.0;
                }
                v
            };
            score(b)
                .partial_cmp(&score(a))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        insights.truncate(20);
        Ok(format!("consolidated: {}, connections_formed: {}, merged: {}, evicted: {}, importance_updated: {}, insights: {:?}",
            report.memories_consolidated, report.connections_formed, report.duplicates_merged, report.junk_evicted, importance_updated, insights))
    }

    pub fn api_recall(&self, query: &str, depth: usize) -> Result<serde_json::Value, String> {
        self.security
            .validate_query(query)
            .map_err(|_| "query validation failed".to_string())?;
        let seed_results = self.gateway.search(query, 30)?;
        if seed_results.is_empty() {
            return Ok(
                serde_json::json!({"query": query, "results": serde_json::Value::Null, "memory_file": serde_json::Value::Null, "seed_count": 0, "associated_count": 0}),
            );
        }

        let clusters = self.find_clusters_cached();
        let all_items =
            self.gateway
                .expand_from_seeds_with_clusters(&seed_results, depth, &clusters);

        let mut sorted_items = all_items;
        sorted_items.sort_by(|a, b| {
            b.1.max(b.2)
                .partial_cmp(&a.1.max(a.2))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        sorted_items.truncate(60);

        let seed_count = sorted_items.iter().filter(|x| x.1 > 0.0).count();
        let assoc_count = sorted_items.len() - seed_count;

        let mut section_best: std::collections::HashMap<String, f64> =
            std::collections::HashMap::new();
        let mut memory_sections: std::collections::HashMap<String, Vec<serde_json::Value>> =
            std::collections::HashMap::new();
        for (id, ds, asim, labels, content, ts) in &sorted_items {
            let pl = labels
                .first()
                .cloned()
                .unwrap_or_else(|| "general".to_string());
            let score = ds.max(*asim);
            section_best
                .entry(pl.clone())
                .and_modify(|s| {
                    if score > *s {
                        *s = score;
                    }
                })
                .or_insert(score);
            memory_sections.entry(pl).or_default().push(serde_json::json!({"id": id, "content": content, "labels": labels, "relevance": [ds, asim], "timestamp": ts}));
        }

        let mut section_order: Vec<(f64, String)> =
            section_best.into_iter().map(|(k, v)| (v, k)).collect();
        section_order.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut ordered_sections = serde_json::Map::new();
        for (_, label) in section_order {
            if let Some(items) = memory_sections.remove(&label) {
                ordered_sections.insert(label, serde_json::Value::Array(items));
            }
        }

        let text_refs: Vec<&str> = sorted_items
            .iter()
            .take(10)
            .map(|(_, _, _, _, c, _)| c.as_str())
            .collect();
        let emotion = crate::engine::emotion::EmotionState::analyze_texts(&text_refs);

        Ok(serde_json::json!({
            "query": query,
            "results": ordered_sections,
            "memory_file": ordered_sections,
            "seed_count": seed_count,
            "associated_count": assoc_count,
            "total_fragments": sorted_items.len(),
            "emotion": serde_json::to_value(emotion).unwrap_or_default()
        }))
    }

    pub fn api_ask(&self, question: &str, depth: usize) -> Result<serde_json::Value, String> {
        self.security
            .validate_query(question)
            .map_err(|_| "question validation failed".to_string())?;
        let seed_results = self.gateway.search(question, 20)?;

        if seed_results.is_empty() {
            return Ok(serde_json::json!({
                "question": question,
                "answer": "No relevant memories found.",
                "memories": [],
                "memory_count": 0
            }));
        }

        // 与 api_recall 共用同一扩展/打分路径：KG + 同簇联想，assoc = 父分 × 边强度（#267），
        // 排序按 max(direct, assoc)。此前这里 assoc 被写死为 0.0，联想条目全被 `max>0` 过滤掉。
        let clusters = self.find_clusters_cached();
        let all_items =
            self.gateway
                .expand_from_seeds_with_clusters(&seed_results, depth, &clusters);

        let mut sorted_items: Vec<(u64, f64, f64)> = all_items
            .iter()
            .map(|(id, direct, assoc, _ls, _c, _ts)| (*id, *direct, *assoc))
            .collect();

        let mut item_data: std::collections::HashMap<u64, (Vec<String>, String)> =
            std::collections::HashMap::new();
        for (id, _, _, labels, content, _) in &all_items {
            item_data
                .entry(*id)
                .or_insert_with(|| (labels.clone(), content.clone()));
        }

        sorted_items.sort_by(|a, b| {
            b.1.max(b.2)
                .partial_cmp(&a.1.max(a.2))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        sorted_items.truncate(30);

        let mem_texts: Vec<String> = sorted_items
            .iter()
            .filter(|(_, direct, assoc)| direct.max(*assoc) > 0.0) // 检索突破：从>0.1放宽到>0.0，避免乘性penalty压低后被误删
            .take(15) // 检索突破：从30条过滤后通常剩个位数→直接取top15
            .filter_map(|(id, _, _)| {
                item_data.get(id).map(|(labels, c)| {
                    let label_str = labels.iter().take(2).cloned().collect::<Vec<_>>().join(",");
                    format!(
                        "[#{}] [{}] {}",
                        id,
                        label_str,
                        c.chars().take(800).collect::<String>()
                    ) // 检索突破：300→800字符
                })
            })
            .collect();
        // D7.2: 知识卡片注入(参数记忆) — 域概要作为前置上下文
        let cards = self.storage.load_knowledge_cards();
        let q_lower = question.to_lowercase();
        let matched_card: Option<&(String, String, Vec<u64>)> =
            cards.iter().find(|(domain, _, _)| {
                let d = domain.to_lowercase();
                q_lower.contains(&d)
                    || d.split_whitespace()
                        .any(|w| w.len() > 3 && q_lower.contains(w))
            });
        let _card_ctx = matched_card
            .map(|(domain, summary, _)| {
                format!(
                    "

[Domain Knowledge: {}]
{}",
                    domain, summary
                )
            })
            .unwrap_or_default();

        let memories_summary = mem_texts.join("\n\n");

        let answer = if self.cognitive.enabled() && !memories_summary.is_empty() {
            self.cognitive
                .answer_from_memories(question, &memories_summary)
                .unwrap_or(memories_summary.clone())
        } else {
            memories_summary
        };

        let memory_items: Vec<serde_json::Value> = sorted_items.iter()
            .filter_map(|(id, direct, assoc)| {
                item_data.get(id).map(|(labels, content)| {
                    serde_json::json!({"id": id, "labels": labels, "content": content, "relevance": direct.max(*assoc)})
                })
            })
            .collect();

        Ok(serde_json::json!({
            "question": question,
            "answer": answer,
            "memories": memory_items,
            "memory_count": memory_items.len(),
            "knowledge_card_used": matched_card.map(|(d, _, _)| d.clone())
        }))
    }

    pub fn api_reason_analogies(&self, min_confidence: f64) -> Vec<serde_json::Value> {
        let analogies = crate::engine::reasoning::ReasoningEngine::find_analogies(
            &self.space,
            &self.knowledge,
            min_confidence,
        );
        analogies.iter().take(5).map(|a| serde_json::json!({
            "a": a.source_a, "b": a.source_b, "c": a.target_a, "d": a.target_b, "confidence": a.confidence
        })).collect()
    }

    pub fn api_reason_patterns(&self) -> Vec<String> {
        crate::engine::reasoning::ReasoningEngine::discover_patterns(&self.space)
    }

    pub fn collect_state_internal(&self) -> SystemState {
        let snap = self.build_snapshot();
        self.collect_state_from_snap(&snap)
    }

    /// 获取认知循环效果追踪数据（用于健康检查/可观测性）
    pub fn outcome_effectiveness_summary(&self) -> Vec<(crate::engine::outcome::ActionType, f64)> {
        self.outcome.lock().effectiveness_summary()
    }

    pub fn execute_action_internal(&self, action: &SchedulerAction) {
        self.execute_action(action);
    }

    pub(in crate::engine::scheduler) fn collect_state_from_snap(
        &self,
        snap: &TickSnapshot,
    ) -> SystemState {
        let tick = snap.tick;
        let energy = snap.energy;
        let tetras = &snap.tetras;
        let clusters = &snap.clusters;
        let labels_map = &snap.labels_map;
        let core_map = &snap.core_map;

        let tetra_cluster_map: HashMap<u64, usize> = clusters
            .iter()
            .enumerate()
            .flat_map(|(ci, cluster)| cluster.tetra_ids.iter().map(move |&tid| (tid, ci)))
            .collect();

        let cluster_states: Vec<crate::engine::cognitive::ClusterState> = clusters
            .iter()
            .enumerate()
            .map(|(i, cluster)| {
                let mut label_counts = HashMap::new();
                let mut member_labels = HashSet::new();
                for &tid in &cluster.tetra_ids {
                    if let Some(labels) = labels_map.get(&tid) {
                        for label in labels {
                            *label_counts.entry(label.clone()).or_insert(0) += 1;
                            member_labels.insert(label.clone());
                        }
                    }
                }

                let entropy = dynamics::compute_entropy_from_labels(&cluster.tetra_ids, labels_map);

                let positions: Vec<Point3> = cluster
                    .tetra_ids
                    .iter()
                    .filter_map(|id| core_map.get(id).copied())
                    .collect();
                let centroid = if positions.is_empty() {
                    [0.0, 0.0, 0.0]
                } else {
                    let n = positions.len() as f64;
                    [
                        positions.iter().map(|p| p.x).sum::<f64>() / n,
                        positions.iter().map(|p| p.y).sum::<f64>() / n,
                        positions.iter().map(|p| p.z).sum::<f64>() / n,
                    ]
                };

                crate::engine::cognitive::ClusterState {
                    index: i,
                    size: cluster.tetra_ids.len(),
                    label_distribution: label_counts,
                    entropy,
                    centroid,
                    member_ids: cluster.tetra_ids.clone(),
                    member_labels: member_labels.into_iter().collect(),
                }
            })
            .collect();

        let memories: Vec<crate::engine::cognitive::MemoryInfo> = tetras
            .iter()
            .take(30)
            .map(|t| {
                let ci = tetra_cluster_map.get(&t.id).copied().unwrap_or(999);
                crate::engine::cognitive::MemoryInfo {
                    id: t.id,
                    content_preview: t.content.chars().take(50).collect(),
                    labels: t.labels.clone(),
                    cluster_index: ci,
                    mass: t.mass,
                }
            })
            .collect();

        let recent = self.recent_events.lock().clone();
        let decision_history = self.decision_history.lock().clone();

        let avg_mass = if tetras.is_empty() {
            1.0
        } else {
            tetras.iter().map(|t| t.mass).sum::<f64>() / tetras.len() as f64
        };
        let max_mass = tetras.iter().map(|t| t.mass).fold(1.0, f64::max);

        // 口径对齐: 只统计 N>=5 的簇, 与健康门一致, 避免提示词数字与判定矛盾
        let big_clusters: Vec<_> = cluster_states.iter().filter(|c| c.size >= 5).collect();
        let avg_entropy = if big_clusters.is_empty() {
            0.0
        } else {
            big_clusters.iter().map(|c| c.entropy).sum::<f64>() / big_clusters.len() as f64
        };
        let max_entropy = big_clusters
            .iter()
            .map(|c| c.entropy)
            .fold(0.0_f64, f64::max);

        let prev_snapshot = self.prev_snapshot.lock().clone();

        let search_metrics = {
            let sm = self.gateway.search_metrics();
            if sm.total > 0 {
                Some(crate::engine::cognitive::SearchPerception {
                    total_queries: sm.total,
                    hit_count: sm.hits,
                    hit_rate: sm.hits as f64 / sm.total as f64,
                    miss_queries: sm.miss_queries,
                    top_labels: sm.top_labels,
                    hot_memories: sm.hot_memories,
                })
            } else {
                None
            }
        };

        let kg_analysis = {
            let ka = self.knowledge.analysis(&self.space);
            Some(crate::engine::cognitive::KgPerception {
                total_tetras: ka.total_tetras,
                total_relations: ka.total_relations,
                orphan_count: ka.orphan_count,
                orphan_ratio: if ka.total_tetras > 0 {
                    ka.orphan_count as f64 / ka.total_tetras as f64
                } else {
                    0.0
                },
                largest_component: ka.largest_component,
                disconnected_components: ka.disconnected_components,
                avg_degree: ka.avg_degree,
                density: ka.density,
                relation_type_counts: ka.relation_type_counts,
            })
        };

        let current_snapshot = crate::engine::cognitive::StateSnapshot {
            tick,
            tetras: tetras.len(),
            clusters: clusters.len(),
            energy,
            avg_entropy,
            max_entropy,
        };
        *self.prev_snapshot.lock() = Some(current_snapshot);

        SystemState {
            tick,
            energy,
            max_energy: self.max_energy,
            total_tetras: tetras.len(),
            total_vertices: self.space.vertex_count(),
            total_clusters: clusters.len(),
            avg_mass,
            max_mass,
            clusters: cluster_states,
            memories,
            recent_events: recent.clone(), // clone 因为 emotion 也要用 recent
            last_dream_tick: self.last_dream_tick.load(Ordering::SeqCst),
            decision_history,
            prev_snapshot,
            search_metrics,
            kg_analysis,
            skill_perception: {
                if let Some(ref se) = *self.skills.lock() {
                    let all = se.list(None);
                    let total = all.len();
                    let public = all.iter().filter(|s| s.is_public).count();
                    let avg_sr = if all.is_empty() {
                        0.0
                    } else {
                        all.iter().map(|s| s.success_rate).sum::<f64>() / total as f64
                    };
                    let total_usage: u64 = all.iter().map(|s| s.usage_count).sum();
                    let total_linked: usize = all.iter().map(|s| s.memory_ids.len()).sum();
                    let mut cat_counts: std::collections::HashMap<String, usize> =
                        std::collections::HashMap::new();
                    for s in &all {
                        if let Some(ref cat) = s.category {
                            *cat_counts.entry(cat.clone()).or_insert(0) += 1;
                        }
                    }
                    let mut top_cats: Vec<(String, usize)> = cat_counts.into_iter().collect();
                    top_cats.sort_by_key(|b| std::cmp::Reverse(b.1));
                    top_cats.truncate(5);
                    Some(crate::engine::cognitive::SkillPerception {
                        total_skills: total,
                        public_skills: public,
                        avg_success_rate: avg_sr,
                        total_usage,
                        top_categories: top_cats,
                        total_linked_memories: total_linked,
                    })
                } else {
                    None
                }
            },
            identity_mission: self.space.identity_info().map(|i| i.mission.clone()),
            identity_name: self.space.identity_info().map(|i| i.system_name.clone()),
            emotion: {
                // 智能突破4: 用 recent_events 文本算情感PAD值,接入认知决策
                let event_refs: Vec<&str> = recent.iter().take(10).map(|s| s.as_str()).collect();
                if event_refs.is_empty() {
                    None
                } else {
                    let emo = crate::engine::emotion::EmotionState::analyze_texts(&event_refs);
                    Some(crate::engine::cognitive::EmotionState {
                        pleasure: emo.pleasure,
                        arousal: emo.arousal,
                        dominance: emo.dominance,
                    })
                }
            },
        }
    }

    pub(in crate::engine::scheduler) fn execute_action(&self, action: &SchedulerAction) {
        match action {
            SchedulerAction::Pulse {
                origin,
                pulse_type,
                ttl,
            } => {
                if !self.energy.consume(2.0) {
                    tracing::warn!("[LLM] pulse: insufficient energy");
                    return;
                }
                let ptype = match pulse_type.as_str() {
                    "reinforcing" => crate::engine::pulse::PulseType::Reinforcing { boost: 0.3 },
                    "exploratory" => {
                        crate::engine::pulse::PulseType::Exploratory { curiosity: 0.4 }
                    }
                    "cascade" => crate::engine::pulse::PulseType::Cascade { branch_limit: 3 },
                    _ => crate::engine::pulse::PulseType::Neural { temperature: 0.8 },
                };
                match crate::engine::pulse::PulseEngine::send(
                    &self.space,
                    &self.knowledge,
                    ptype,
                    *origin,
                    *ttl,
                ) {
                    Ok(result) => {
                        // 持久化 pulse 引起的 mass 变化（之前断联：不 mark_dirty → 重启丢失）
                        for &tid in &result.data.visited_tetras {
                            self.gateway.mark_dirty(tid);
                        }
                        tracing::info!(
                            "[LLM] pulse from {} → visited {} tetras, cost {:.1}",
                            origin,
                            result.data.visited_tetras.len(),
                            result.energy_cost
                        );
                        self.log_event(format!("pulse({},{:?},{})", origin, pulse_type, ttl));
                    }
                    Err(e) => tracing::warn!("[LLM] pulse failed: {}", e),
                }
            }
            SchedulerAction::Fission { cluster_index } => {
                self.perform_fission(*cluster_index, 10, 10.0, "LLM");
            }
            SchedulerAction::Fuse {
                cluster_a,
                cluster_b,
            } => {
                if cluster_a == cluster_b {
                    tracing::debug!("[LLM] fuse: skipped (same cluster {})", cluster_a);
                    return;
                }
                let bridge_count = self
                    .space
                    .all_tetrahedrons()
                    .iter()
                    .filter(|t| t.data.labels.iter().any(|l| l == "bridge"))
                    .count();
                if bridge_count >= 5 {
                    tracing::info!(
                        "[LLM] fuse: skipped (bridge limit reached: {})",
                        bridge_count
                    );
                    return;
                }
                let clusters = self.find_clusters_cached();
                let ca = match clusters.get(*cluster_a) {
                    Some(c) => c,
                    None => {
                        tracing::warn!("[LLM] fuse: cluster_a {} not found", cluster_a);
                        return;
                    }
                };
                let cb = match clusters.get(*cluster_b) {
                    Some(c) => c,
                    None => {
                        tracing::warn!("[LLM] fuse: cluster_b {} not found", cluster_b);
                        return;
                    }
                };

                let label_sim = crate::engine::auto_pipeline::compute_cluster_label_similarity(
                    ca,
                    cb,
                    &self.space,
                );
                if label_sim < 0.3 {
                    tracing::info!(
                        "[LLM] fuse {}+{} → BLOCKED (label_sim={:.3} < 0.3)",
                        cluster_a,
                        cluster_b,
                        label_sim
                    );
                    return;
                }
                if !self.energy.consume(8.0) {
                    tracing::warn!("[LLM] fuse: insufficient energy");
                    return;
                }

                let bridge_content = format!(
                    "[bridge] cluster {} + cluster {} (label_sim={:.3})",
                    cluster_a, cluster_b, label_sim
                );
                let ca_centroid = self.cluster_core_centroid(ca);
                let cb_centroid = self.cluster_core_centroid(cb);
                let bridge_core = Point3::new(
                    (ca_centroid.x + cb_centroid.x) / 2.0,
                    (ca_centroid.y + cb_centroid.y) / 2.0,
                    (ca_centroid.z + cb_centroid.z) / 2.0,
                );
                let positions = Tetrahedron::compute_vertices(bridge_core);
                let data = MemoryPayload {
                    content: bridge_content,
                    content_hash: 0,
                    labels: vec!["bridge".to_string()],
                    timestamp: chrono::Utc::now().timestamp(),
                    aliases: vec![],
                    embedding: vec![],
                    importance: 1.5,
                    enforced: false,
                    rationale: None,
                    access_count: 0,
                    memory_type: Some("bridge".to_string()),
                    identity_stamp: None,
                    source_agent: None,
                    valid_from: chrono::Utc::now().timestamp(),
                    valid_to: None,
                    expired_at: None,
                    invalidated_at: None,
                    memory_class: None,
                    last_reviewed_ts: None,
                };
                let tetra = Tetrahedron {
                    id: 0,
                    vertex_ids: [0; 4],
                    core: bridge_core,
                    data,
                    mass: 1.0,
                };
                match self.space.add_tetrahedron(&tetra, &positions) {
                    Ok(id) => {
                        tracing::info!("[LLM] fuse: bridge tetra #{} connecting cluster {}+{} (label_sim={:.3})", id, cluster_a, cluster_b, label_sim);
                        self.persist_tetra(id);
                        self.log_event(format!("fuse({},{},{})", cluster_a, cluster_b, id));
                    }
                    Err(e) => tracing::warn!("[LLM] fuse bridge failed: {}", e),
                }
            }
            SchedulerAction::Dream => {
                let Some(_dream_permit) = self.dream_gate.try_acquire() else {
                    tracing::info!("[LLM] dream: another dream cycle is running, skipped");
                    return;
                };
                if !self.energy.consume(15.0) {
                    tracing::warn!("[LLM] dream: insufficient energy");
                    return;
                }
                let result = DreamEngine::cycle(&self.space, &self.knowledge, 0.3, 5, false);
                let tick = self.tick_count.load(Ordering::SeqCst);
                self.last_dream_tick.store(tick, Ordering::SeqCst);
                for &id in result
                    .evicted_ids
                    .iter()
                    .chain(result.merged_remove_ids.iter())
                {
                    self.persist_tetra(id);
                }
                tracing::info!(
                    "[LLM] dream: consolidated {}, formed {} connections, {} insights, {} merged, {} evicted",
                    result.memories_consolidated,
                    result.connections_formed,
                    result.insights.len(),
                    result.duplicates_merged,
                    result.junk_evicted
                );
                for insight in &result.insights {
                    tracing::info!("[LLM] dream insight: {}", insight);
                }
                self.log_event("dream".to_string());
            }
            SchedulerAction::Link { a, b, reason } => {
                if self.space.get_tetrahedron(*a).is_none()
                    || self.space.get_tetrahedron(*b).is_none()
                {
                    tracing::warn!("[LLM] link: id {} or {} not found", a, b);
                    return;
                }
                let label_sim = if let (Some(ta), Some(tb)) = (
                    self.space.get_tetrahedron(*a),
                    self.space.get_tetrahedron(*b),
                ) {
                    crate::engine::vector::VectorLayer::label_jaccard(
                        &ta.data.labels,
                        &tb.data.labels,
                    )
                } else {
                    0.0
                };
                self.knowledge.add_relation(
                    *a,
                    *b,
                    crate::engine::knowledge::RelationType::SimilarTo,
                    label_sim.max(0.5),
                );
                tracing::info!(
                    "[LLM] link: #{} ↔ #{} (label_sim={:.3}) reason: {}",
                    a,
                    b,
                    label_sim,
                    reason
                );
                self.log_event(format!("link({},{})", a, b));
            }
            SchedulerAction::Consolidate { ids, keep, summary } => {
                if !ids.contains(keep) {
                    tracing::warn!("[LLM] consolidate: keep id {} not in ids {:?}", keep, ids);
                    return;
                }
                let now = chrono::Utc::now().timestamp();
                let mut superseded = 0u64;
                for &id in ids {
                    if id == *keep {
                        continue;
                    }
                    if let Some(t) = self.space.get_tetrahedron(id) {
                        let mut updated = t.data.clone();
                        if !updated.labels.iter().any(|l| l == "superseded") {
                            updated.labels.push("superseded".to_string());
                        }
                        updated.valid_to = Some(now);
                        updated.importance = (updated.importance * 0.15)
                            .max(crate::engine::governor::IMPORTANCE_FLOOR); // O-A
                        if let Err(e) = self.space.patch_payload(id, &t.data, updated) {
                            tracing::warn!("[Scheduler] update_payload {} failed: {}", id, e);
                        }
                        if let Err(e) = self.space.update_mass(id, 0.05) {
                            tracing::warn!("[Scheduler] update_mass {} failed: {}", id, e);
                        }
                        let _ = self.space.update_validity(id, Some(now));
                        self.persist_tetra(id);
                        superseded += 1;
                    }
                }
                if let Some(t) = self.space.get_tetrahedron(*keep) {
                    let mut new_labels = t.data.labels.clone();
                    if !new_labels.iter().any(|l| l == "consolidated") {
                        new_labels.push("consolidated".to_string());
                    }
                    let mut updated = t.data.clone();
                    updated.content = format!(
                        "{}\n\n[整合自 {} 条记忆: {}]",
                        summary,
                        ids.len(),
                        ids.iter()
                            .map(|id| format!("#{}", id))
                            .collect::<Vec<_>>()
                            .join(",")
                    );
                    updated.labels = new_labels.clone();
                    updated.importance += superseded as f64 * 0.2;
                    if let Err(e) = self.space.patch_payload(*keep, &t.data, updated.clone()) {
                        tracing::warn!("[H2] update_payload failed: {}", e);
                    }
                    if let Err(e) = self.space.update_mass(*keep, superseded as f64 * 0.5) {
                        tracing::warn!("[Scheduler] update_mass {} failed: {}", *keep, e);
                    }
                    self.persist_tetra(*keep);
                    self.gateway
                        .update_label_index(*keep, &t.data.labels, &updated.labels);
                }
                tracing::info!(
                    "[LLM] merge_content: kept #{}, superseded {} duplicates (no deletion) ({})",
                    keep,
                    superseded,
                    summary.chars().take(80).collect::<String>()
                );
                self.log_event(format!("merge_content({},{})", keep, superseded));
            }
            SchedulerAction::MarkJunk { ids, reason } => {
                let mut quarantined = 0u64;
                for &id in ids {
                    if let Some(t) = self.space.get_tetrahedron(id) {
                        let old_labels = t.data.labels.clone();
                        let mut updated = t.data.clone();
                        if !updated.labels.iter().any(|l| l == "quarantine") {
                            updated.labels.push("quarantine".to_string());
                        }
                        updated.importance = 0.1;
                        if let Err(e) = self.space.patch_payload(id, &t.data, updated.clone()) {
                            tracing::warn!("[H2] update_payload failed: {}", e);
                        }
                        if let Err(e) = self.space.update_mass(id, 0.05) {
                            tracing::warn!("[Scheduler] update_mass {} failed: {}", id, e);
                        }
                        self.persist_tetra(id);
                        self.gateway
                            .update_label_index(id, &old_labels, &updated.labels);
                        quarantined += 1;
                    }
                }
                tracing::info!(
                    "[LLM] quarantine: {} memories isolated (not deleted) ({})",
                    quarantined,
                    reason.chars().take(80).collect::<String>()
                );
                self.log_event(format!(
                    "quarantine({},{})",
                    quarantined,
                    reason.chars().take(40).collect::<String>()
                ));
            }
            SchedulerAction::Relabel {
                id,
                add_labels,
                remove_labels,
                reason,
            } => {
                if let Some(t) = self.space.get_tetrahedron(*id) {
                    let old_labels = t.data.labels.clone();
                    let mut new_labels: Vec<String> = t.data.labels.clone();
                    for label in remove_labels {
                        new_labels.retain(|l| l != label);
                    }
                    for label in add_labels {
                        if !new_labels.iter().any(|l| l == label) {
                            new_labels.push(label.clone());
                        }
                    }
                    let updated = MemoryPayload {
                        content: t.data.content.clone(),
                        content_hash: t.data.content_hash,
                        labels: new_labels.clone(),
                        timestamp: t.data.timestamp,
                        aliases: t.data.aliases.clone(),
                        embedding: t.data.embedding.clone(),
                        importance: t.data.importance,
                        enforced: t.data.enforced,
                        rationale: t.data.rationale.clone(),
                        access_count: t.data.access_count,
                        memory_type: t.data.memory_type.clone(),
                        identity_stamp: t.data.identity_stamp.clone(),
                        source_agent: t.data.source_agent.clone(),
                        valid_from: t.data.valid_from,
                        valid_to: t.data.valid_to,
                        expired_at: t.data.expired_at,
                        memory_class: t.data.memory_class.clone(),
                        invalidated_at: t.data.invalidated_at,
                        last_reviewed_ts: t.data.last_reviewed_ts,
                    };
                    if let Err(e) = self.space.patch_payload(*id, &t.data, updated) {
                        tracing::warn!(
                            "[Scheduler] relabel update_payload failed for #{}: {}",
                            id,
                            e
                        );
                    } else {
                        self.gateway
                            .update_label_index(*id, &old_labels, &new_labels);
                        self.persist_tetra(*id);
                        tracing::info!(
                            "[LLM] relabel #{}: +{:?} -{:?} ({})",
                            id,
                            add_labels,
                            remove_labels,
                            reason.chars().take(80).collect::<String>()
                        );
                    }
                } else {
                    tracing::warn!("[LLM] relabel: id {} not found", id);
                }
                self.log_event(format!("relabel({})", id));
            }
            SchedulerAction::Reflect {
                observation,
                insight,
            } => {
                tracing::info!(
                    "[LLM] REFLECT observation: {}",
                    observation.chars().take(120).collect::<String>()
                );
                tracing::info!(
                    "[LLM] REFLECT insight: {}",
                    insight.chars().take(120).collect::<String>()
                );
                // 智能突破断裂点4：Reflect 持久化到 CognitiveEngine（而非只打日志）
                // 下次 build_decision_prompt 会注入 "## Last Reflection" 段
                self.cognitive.store_reflection(observation, insight);
                self.log_event(format!(
                    "reflect({})",
                    observation.chars().take(40).collect::<String>()
                ));
            }
            SchedulerAction::UseTool { .. } => {
                tracing::debug!(
                    "[Scheduler] UseTool executed by cognitive layer, skipping in execute_action"
                );
            }
            SchedulerAction::ActOutward {
                intent,
                description,
                evidence,
                urgency,
                target_capability,
            } => {
                let urg = match urgency.as_str() {
                    "low" => crate::engine::drive::DriveUrgency::Low,
                    "high" => crate::engine::drive::DriveUrgency::High,
                    "critical" => crate::engine::drive::DriveUrgency::Critical,
                    _ => crate::engine::drive::DriveUrgency::Medium,
                };
                let mut itype = match intent.as_str() {
                    "warn" => crate::engine::drive::DriveIntent::Warn,
                    "suggest" => crate::engine::drive::DriveIntent::Suggest,
                    "explore" => crate::engine::drive::DriveIntent::Explore,
                    "constrain" => crate::engine::drive::DriveIntent::Constrain,
                    "request" => crate::engine::drive::DriveIntent::Request,
                    _ => crate::engine::drive::DriveIntent::Share,
                };
                let mut evidence = evidence.clone();
                let now = chrono::Utc::now().timestamp();
                let mut conflicts: Vec<u64> = evidence
                    .iter()
                    .flat_map(|id| self.active_contradiction_ids(*id, now))
                    .collect();
                conflicts.sort_unstable();
                conflicts.dedup();
                let mut desc = description.clone();
                if !conflicts.is_empty()
                    && matches!(itype, crate::engine::drive::DriveIntent::Suggest)
                {
                    itype = crate::engine::drive::DriveIntent::Warn;
                    desc = format!(
                        "Potential contradiction in current memory evidence {:?}; withholding action-oriented proposal. Original proposal: {}",
                        conflicts,
                        description
                    );
                }
                evidence.extend(conflicts.iter().copied());
                evidence.sort_unstable();
                evidence.dedup();
                let grounding = self.grounding_for_evidence(
                    &evidence,
                    "Model-originated outward proposal; evidence IDs are references, not a confidence estimate.",
                    &conflicts,
                    None,
                );
                let has_complete_grounding = grounding
                    .as_ref()
                    .is_some_and(|grounding| grounding.complete && !grounding.evidence.is_empty());
                if matches!(&itype, crate::engine::drive::DriveIntent::Suggest)
                    && !has_complete_grounding
                {
                    tracing::info!(
                        "[L0] ActOutward suppressed: suggestion lacks complete persistent-memory evidence (ev={:?})",
                        evidence
                    );
                } else if let Err(why) = self.drive_queue.should_birth(&itype, &evidence, &desc) {
                    tracing::info!("[L0] ActOutward valve {}: ev={:?}", why, evidence);
                } else {
                    let signal = crate::engine::drive::DriveSignal {
                        id: 0,
                        timestamp: chrono::Utc::now().timestamp(),
                        intent_type: itype,
                        description: desc,
                        evidence: evidence.clone(),
                        target_capability: target_capability.clone(),
                        emotion: None,
                        origin_tick: 0,
                        status: crate::engine::drive::default_status(),
                        feedback: None,
                        retry_count: 0,
                        expires_at: crate::engine::drive::default_expires_at(&urg),
                        urgency: urg,
                        enqueued_at_ms: 0,
                        time_budget_ms: None,
                        grounding,
                        terminal_reason: None,
                    };
                    match self.drive_queue.try_enqueue(signal) {
                        Ok(id) => {
                            self.save_drive_queue();
                            tracing::info!("[L0] ActOutward: drive #{} intent={}", id, intent);
                        }
                        Err(error) => {
                            tracing::warn!("[L0] ActOutward admission rejected: {:?}", error)
                        }
                    }
                }
            }
        }
    }
}
