use super::*;

impl SchedulerCenter {
    /// 学到的自适应阈值(值 + 默认 + 边界),供稳态观测。
    pub fn adaptive_params_snapshot(&self) -> serde_json::Value {
        self.adaptive.lock().describe()
    }

    pub(in crate::engine::scheduler) fn tick_and_maybe_think(&self) -> Option<CognitiveThought> {
        let count = self.tick_count.fetch_add(1, Ordering::SeqCst);
        let _ = self.tx.send(EngineEvent::DecisionTick);
        self.energy.replenish(8.0);

        let tasks: Vec<ScheduledTask> = self.queue.lock().drain(..).collect();
        for task in &tasks {
            self.execute_task(task);
        }

        // L0 Active Inference: run prediction error detection at the START of each tick.
        // This runs unconditionally — even if LLM times out, the personality still
        // generates drive signals from its accumulated experience.
        // This is how memory drives action: accumulated patterns detect mismatches
        // with current reality → will is generated → agents receive it via /v1/drive/inbox
        self.run_prediction_error_check();

        // L0: Self-driving loop — process pending drive signals.
        // This runs unconditionally — even when LLM is degraded, the personality
        // can still explore using local memory recall (no LLM needed).
        // When LLM IS available, it uses answer_from_memories for richer reasoning.
        // This is resilience: the personality never stops thinking.
        self.process_drive_signals();

        // L0: Persist drive queue to SQLite every 5 ticks.
        // Runs unconditionally — survives even when LLM times out.
        {
            // count is the pre-increment value from fetch_add above (0,1,2,3...)
            // Save on tick 0 (first tick) and every 5 ticks after
            if count == 0 || count.is_multiple_of(5) {
                self.save_drive_queue();
            }
        }

        let snap = self.build_snapshot();

        // Phase 0: Observe — update drive engine with current state signals
        {
            let avg_entropy = if !snap.clusters.is_empty() {
                let sum: f64 = snap
                    .clusters
                    .iter()
                    .map(|c| dynamics::compute_entropy_from_labels(&c.tetra_ids, &snap.labels_map))
                    .sum::<f64>();
                sum / snap.clusters.len() as f64
            } else {
                0.0
            };
            let energy_ratio = snap.energy / self.max_energy.max(1.0);
            let tetra_count = snap.tetras.len();
            let unexplored_ratio = if tetra_count > 0 {
                let explored: usize = snap.tetras.iter().filter(|t| t.mass > 1.05).count();
                1.0 - (explored as f64 / tetra_count as f64)
            } else {
                1.0
            };
            let redundancy_ratio = if tetra_count > 1 {
                let content_hashes: Vec<u64> = snap
                    .tetras
                    .iter()
                    .take(100)
                    .map(|t| t.content_hash)
                    .collect();
                let unique: std::collections::HashSet<u64> =
                    content_hashes.iter().copied().collect();
                1.0 - (unique.len() as f64 / content_hashes.len().max(1) as f64)
            } else {
                0.0
            };

            let mut drive = self.drive.lock();
            drive.observe(
                tetra_count,
                snap.clusters.len(),
                avg_entropy,
                energy_ratio,
                unexplored_ratio,
                redundancy_ratio,
            );
        }

        // Phase 1: Generate — drive-guided actions replace hardcoded tick intervals
        let drive = self.drive.lock();
        let should_pulse = drive.should_pulse();
        let should_fission = drive.should_fission();
        let should_dream = drive.should_dream();
        let should_evict = drive.should_evict();
        drop(drive);

        // Phase 2: Execute — drive-guided pipeline
        if should_pulse {
            self.auto_pulse(&snap);
        }

        let has_large_cluster = snap.clusters.iter().any(|c| c.tetra_ids.len() >= 6);
        if should_fission || has_large_cluster {
            self.auto_fission(&snap);
        }

        if count.is_multiple_of(20) && count > 0 {
            self.auto_skills(&snap);
        }

        // Honor DecisionCenter/drive dream hook: Coherence-dominant ticks may dream
        // on a 10-tick cadence; otherwise keep the periodic 30-tick floor.
        // auto_dream still respects DreamInterval + energy.consume(15).
        if count > 0 && self.energy.available() >= 50.0 {
            let due_periodic = count.is_multiple_of(30);
            let due_drive = should_dream && count.is_multiple_of(10);
            if due_periodic || due_drive {
                let pre_snap = self.build_snapshot();
                self.auto_dream();
                self.record_outcome(ActionType::Dream, &pre_snap, count);
            }
        }

        // 回写节奏: 每 10 tick(tick=120s 时约 20 分钟)——生产实测 30 tick(60 分钟)
        // 使检索命中的 access_count/mass 滋养落盘过慢, 可塑性回路近乎不可观测。
        if count.is_multiple_of(10) && count > 0 {
            self.flush_access_counts();
        }

        if count.is_multiple_of(200) && count > 0 && should_evict {
            self.evict_low_quality(&snap);
        }

        // Phase 5: Emotion
        if count.is_multiple_of(10) {
            let texts: Vec<&str> = snap
                .tetras
                .iter()
                .take(20)
                .map(|t| t.content.as_str())
                .collect();
            let new_emotion = crate::engine::emotion::EmotionState::analyze_texts(&texts);
            {
                let mut em = self.emotion.lock();
                em.affect(
                    new_emotion.pleasure * 0.1,
                    new_emotion.arousal * 0.1,
                    new_emotion.dominance * 0.1,
                );
                em.decay(0.05);
            }
        }

        // Phase 6: Think — LLM cognitive decision every 5 ticks (when enabled)
        if count.is_multiple_of(5) && self.cognitive.enabled() {
            let hooks_due = count > 0
                && (count.is_multiple_of(15)
                    || count.is_multiple_of(30)
                    || count.is_multiple_of(20));
            // 认知钩子也是模型调用:与 decide 共用单飞门,上一轮模型调用未返回时本轮跳过
            if hooks_due {
                if let Some(_deliberative) = self.deliberative_gate.try_acquire() {
                    let _timer = self.layers.time(RunLayer::Deliberative);
                    if count.is_multiple_of(15) {
                        self.generate_aliases((count / 15) as usize, &snap);
                    }
                    if count.is_multiple_of(30) {
                        self.reclassify_memories((count / 30) as usize, &snap);
                    }
                    if count.is_multiple_of(20) {
                        self.extract_entities((count / 20) as usize, &snap);
                    }
                } else {
                    self.layers.record_deferred(RunLayer::Deliberative);
                }
            }

            // Idle Stopper: skip LLM call if space is healthy
            let orphan_count = snap
                .tetras
                .iter()
                .filter(|t| !t.enforced)
                .map(|t| t.id)
                .filter(|id| !snap.labels_map.contains_key(id))
                .count();
            let orphan_rate = if !snap.tetras.is_empty() {
                orphan_count as f64 / snap.tetras.len() as f64
            } else {
                0.0
            };
            // 熵阈值与簇大小自适应: N<5 的小簇标签混合是常态, 熵无统计意义
            // (潜意识 REFLECT 自诊断: entropy阈值是size-dependent, 小簇误报曾导致永久healthy=false)
            let max_entropy = snap
                .clusters
                .iter()
                .filter(|c| c.tetra_ids.len() >= 5)
                .map(|c| dynamics::compute_entropy_from_labels(&c.tetra_ids, &snap.labels_map))
                .fold(0.0f64, f64::max);
            let quarantine_count = snap
                .tetras
                .iter()
                .filter(|t| t.labels.iter().any(|l| l == "quarantine"))
                .count();
            let energy_low = snap.energy < self.max_energy * 0.3;

            // 阶段3修复:quarantine阈值从绝对值10改为比例(<总量5%),
            // 因为P0-23修复后quarantine记忆不再删除,绝对值只会增不会减。
            let quarantine_ratio = if !snap.tetras.is_empty() {
                quarantine_count as f64 / snap.tetras.len() as f64
            } else {
                0.0
            };
            // 熵项从思考门摘除: auto_fission 独立消化高熵(每10tick), LLM对熵的重复思考
            // 曾致 3h/100轮/50万token 空转 — 意识自己反复判断"让auto_fission处理,暂不干预"
            // 思考门只保留 LLM 真正有价值的触发: 孤儿/垃圾/能量 (miss 由 idle_stopper 判)
            let is_healthy = orphan_rate < 0.05 && quarantine_ratio < 0.05 && !energy_low;

            let drive = self.drive.lock();
            let dominant = drive.dominant();
            tracing::info!(
                "[Scheduler] tick {} — {} tetras, {} clusters, energy {:.0}, drive={:?}, orphan={:.1}%, entropy_max={:.2}, quarantined={}, healthy={}",
                count,
                snap.tetras.len(),
                snap.clusters.len(),
                snap.energy,
                dominant,
                orphan_rate * 100.0,
                max_entropy,
                quarantine_count,
                is_healthy,
            );
            drop(drive);

            if is_healthy && !count.is_multiple_of(30) {
                // 智能突破：健康短路从 15→30 tick，让系统更频繁地"思考"
                // 每 30 tick（约1小时）即使健康也做一次元认知检查
                self.auto_save();
                return None;
            }

            let state = self.collect_state_from_snap(&snap);
            Some(CognitiveThought { tick: count, state })
        } else {
            if count.is_multiple_of(10) {
                self.auto_save();
            }
            None
        }
    }

    pub(in crate::engine::scheduler) fn auto_skills(&self, snap: &TickSnapshot) {
        if let Some(ref skills_engine) = *self.skills.lock() {
            let pending = skills_engine.review_pending();
            let pub_engine = self.pub_skills.lock().clone();
            for skill in pending {
                match crate::engine::skills::SkillEngine::security_check(&skill) {
                    Ok(()) => {
                        if let Ok(approved) = skills_engine.approve_skill(skill.id) {
                            tracing::info!(
                                "[Skills] auto approved '{}' (id={})",
                                approved.name,
                                skill.id
                            );

                            if let Ok(ref desc) = self
                                .cognitive
                                .generate_skill_description(&approved.name, &approved.skill_md)
                            {
                                let _ = skills_engine.append_description(skill.id, desc);
                                tracing::info!(
                                    "[Skills] added Chinese description for '{}'",
                                    approved.name
                                );
                            }

                            if let Some(ref pub_sk) = pub_engine {
                                if let Some(moved) = skills_engine.take(skill.id) {
                                    let mut pub_skill = moved.clone();
                                    pub_skill.is_public = true;
                                    pub_sk.insert_skill(pub_skill);
                                }
                            }
                        }
                    }
                    Err(reason) => {
                        if skills_engine.reject_skill(skill.id, &reason).is_ok() {
                            tracing::info!(
                                "[Skills] auto rejected '{}' (id={}): {}",
                                skill.name,
                                skill.id,
                                reason
                            );
                        }
                    }
                }
            }
        }

        if snap.clusters.is_empty() {
            return;
        }
        let top_labels: Vec<(String, usize)> = {
            let mut label_counts: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            for t in &snap.tetras {
                for label in &t.labels {
                    *label_counts.entry(label.clone()).or_insert(0) += 1;
                }
            }
            let mut v: Vec<_> = label_counts.into_iter().collect();
            v.sort_by_key(|b| std::cmp::Reverse(b.1));
            v.truncate(5);
            v
        };
        if !top_labels.is_empty() {
            let summary: Vec<String> = top_labels
                .iter()
                .map(|(l, c)| format!("{}({})", l, c))
                .collect();
            tracing::debug!(
                "[Skills] top domains: {} — matching against skills",
                summary.join(", ")
            );

            if let Some(ref skills_engine) = *self.skills.lock() {
                let mut linked_count = 0usize;
                for (label, _count) in &top_labels {
                    let matched = skills_engine.match_skills(label, "", 3);
                    for skill in &matched {
                        let tetra_ids: Vec<u64> = snap
                            .tetras
                            .iter()
                            .filter(|t| t.labels.iter().any(|l| l == label))
                            .take(5)
                            .map(|t| t.id)
                            .collect();
                        for mid in tetra_ids {
                            if skills_engine.link_memory(skill.id, mid).is_ok() {
                                linked_count += 1;
                            }
                        }
                    }
                }
                if linked_count > 0 {
                    tracing::info!(
                        "[Skills-AutoLink] linked {} memory↔skill pairs from top domains",
                        linked_count
                    );
                }
            }
        }
    }

    pub(in crate::engine::scheduler) fn auto_pulse(&self, snap: &TickSnapshot) {
        let em = self.emotion.lock();
        let pleasure = em.pleasure;
        let arousal = em.arousal;
        drop(em);
        let adaptive = self.adaptive.lock();
        let ctx = crate::engine::auto_pipeline::AutoPipelineCtx {
            tick: snap.tick,
            space: &self.space,
            energy: &self.energy,
            knowledge: &self.knowledge,
            gateway: &self.gateway,
            storage: &self.storage,
            emotion_pleasure: pleasure,
            emotion_arousal: arousal,
            adaptive: &adaptive,
        };

        let pulsed = crate::engine::auto_pipeline::auto_pulse(
            &ctx,
            &snap.tetras,
            &snap.clusters,
            &snap.core_map,
        );
        if pulsed > 0 {
            let _ = self
                .tx
                .send(crate::engine::bus::EngineEvent::AutoPulse { count: pulsed });
        }
    }

    pub(in crate::engine::scheduler) fn auto_fission(&self, snap: &TickSnapshot) {
        let em = self.emotion.lock();
        let pleasure = em.pleasure;
        let arousal = em.arousal;
        drop(em);
        let adaptive = self.adaptive.lock();
        let ctx = crate::engine::auto_pipeline::AutoPipelineCtx {
            tick: snap.tick,
            space: &self.space,
            energy: &self.energy,
            knowledge: &self.knowledge,
            gateway: &self.gateway,
            storage: &self.storage,
            emotion_pleasure: pleasure,
            emotion_arousal: arousal,
            adaptive: &adaptive,
        };

        let last_fission = self.last_fission_tick.load(Ordering::SeqCst);
        let last_merge = self.last_merge_pairs.lock().clone();

        let outcome = crate::engine::auto_pipeline::auto_fission(
            &ctx,
            &snap.clusters,
            &snap.labels_map,
            &snap.core_map,
            last_fission,
            &last_merge,
        );
        if outcome.did_fission {
            self.last_fission_tick.store(snap.tick, Ordering::SeqCst);
        }
        if let Some(new_merge) = outcome.merge_pairs {
            *self.last_merge_pairs.lock() = new_merge;
        }
    }

    pub(in crate::engine::scheduler) fn auto_dream(&self) {
        let Some(_dream_permit) = self.dream_gate.try_acquire() else {
            tracing::debug!("[AutoDream] another dream cycle is running, skipped");
            self.layers.record_deferred(RunLayer::Consolidation);
            return;
        };
        let _timer = self.layers.time(RunLayer::Consolidation);
        let tick = self.tick_count.load(Ordering::SeqCst);
        let last = self.last_dream_tick.load(Ordering::SeqCst);

        let em = self.emotion.lock();
        let pleasure = em.pleasure;
        let arousal = em.arousal;
        drop(em);
        let adaptive = self.adaptive.lock();
        let ctx = crate::engine::auto_pipeline::AutoPipelineCtx {
            tick,
            space: &self.space,
            energy: &self.energy,
            knowledge: &self.knowledge,
            gateway: &self.gateway,
            storage: &self.storage,
            emotion_pleasure: pleasure,
            emotion_arousal: arousal,
            adaptive: &adaptive,
        };

        let purge = |id: TetraId| {
            self.persist_tetra(id);
        };

        // D5: 热标签传入governor(个体化遗忘)
        let hot_labels: Vec<(String, u32)> = self
            .gateway
            .search_metrics()
            .top_labels
            .into_iter()
            .take(20)
            .collect();
        let gov = crate::engine::governor::LifecycleGovernor::evaluate_with_hot_labels(
            &self.space,
            &self.knowledge,
            &hot_labels,
        );
        // S3修复: evaluate 修改的记忆持久化 (session/bridge supersede + decay)
        for mid in &gov.mutated_ids {
            if let Some(t) = self.space.get_tetrahedron(*mid) {
                let _ = self.storage.upsert_tetra(&t);
            }
        }

        // Tiered consolidation: boost importance of recurrent memories
        if !gov.recurrent_ids.is_empty() {
            for &rid in &gov.recurrent_ids {
                if let Some(tetra) = self.space.get_tetrahedron(rid) {
                    let count = tetra.data.access_count;
                    let boost = match count {
                        3..=5 => 0.1,
                        6..=10 => 0.2,
                        _ => 0.3,
                    };
                    let old_imp = tetra.data.importance;
                    // 字段级:锁内基于当前值加成,不整包写回
                    if let Err(e) = self.space.apply_ops(
                        rid,
                        &[crate::domain::ops::MemoryOp::AddImportance {
                            delta: boost,
                            max: 3.5,
                        }],
                    ) {
                        tracing::warn!("[Scheduler] tiered boost {} failed: {}", rid, e);
                    }
                    // S3修复: 升级持久化
                    if let Some(t2) = self.space.get_tetrahedron(rid) {
                        let _ = self.storage.upsert_tetra(&t2);
                    }
                    tracing::info!(
                        "[Governor] tiered boost #{}: importance {:.2} -> {:.2} (access_count={})",
                        rid,
                        old_imp,
                        old_imp + boost,
                        count
                    );
                }
            }
        }

        // Feedback aggregation: incremental, only process new feedback records
        {
            let feedback_mems = self.gateway.list_by_labels(&["feedback"], 100);
            let (mut processed_ids, should_full) = {
                let cache = self.feedback_agg_cache.lock();
                match &*cache {
                    Some((last_count, last_time, ids)) => {
                        let needs_full = feedback_mems.len() > *last_count + 50
                            || last_time.elapsed() > std::time::Duration::from_secs(600);
                        (ids.clone(), needs_full)
                    }
                    None => (HashSet::new(), true),
                }
            };

            let new_feedbacks: Vec<_> = if should_full {
                feedback_mems.iter().collect()
            } else {
                feedback_mems
                    .iter()
                    .filter(|(id, _)| !processed_ids.contains(id))
                    .collect()
            };

            if !new_feedbacks.is_empty() {
                let mut feedback_scores: std::collections::HashMap<u64, f64> =
                    std::collections::HashMap::new();
                for &(fid, p) in &new_feedbacks {
                    let lower = p.content.to_lowercase();
                    let relevance = if lower.contains("highly_relevant") {
                        0.1
                    } else if lower.contains("partially_relevant") {
                        0.03
                    } else {
                        -0.05
                    };
                    let outcome = if lower.contains("task_completed") {
                        0.15
                    } else if lower.contains("task_failed") {
                        -0.1
                    } else {
                        0.0
                    };
                    let correction = if lower.contains("correction: outdated")
                        || lower.contains("correction: incorrect")
                    {
                        -0.3
                    } else {
                        0.0
                    };
                    let delta = relevance + outcome + correction;
                    let ids_start = lower.find("affected_ids:");
                    if let Some(start) = ids_start {
                        let bracket_start = lower[start..].find('[');
                        let bracket_end = lower[start..].find(']');
                        if let (Some(bs), Some(be)) = (bracket_start, bracket_end) {
                            let ids_str = &lower[start + bs + 1..start + be];
                            for id_str in ids_str.split(',') {
                                if let Ok(id) = id_str.trim().parse::<u64>() {
                                    *feedback_scores.entry(id).or_insert(0.0) += delta;
                                }
                            }
                        }
                    }
                    processed_ids.insert(*fid);
                }

                let mut aggregated = 0usize;
                for (id, total_delta) in &feedback_scores {
                    if total_delta.abs() < 0.01 {
                        continue;
                    }
                    if let Some(tetra) = self.space.get_tetrahedron(*id) {
                        let mut data = tetra.data.clone();
                        let old_imp = data.importance;
                        let adj = (*total_delta * 0.3).clamp(-0.5, 0.5);
                        data.importance = (data.importance + adj)
                            .clamp(crate::engine::governor::IMPORTANCE_FLOOR, 5.0);
                        if let Err(e) = self.space.patch_payload(*id, &tetra.data, data) {
                            tracing::warn!("[Scheduler] update_payload {} failed: {}", *id, e);
                        }
                        let _ = self.storage.update_importance(*id, adj);
                        aggregated += 1;
                        tracing::info!(
                            "[Feedback-Agg] #{} delta={:.2} adj={:.3} importance {:.2}->{:.2}",
                            id,
                            total_delta,
                            adj,
                            old_imp,
                            old_imp + adj
                        );
                    }
                }
                if aggregated > 0 || !new_feedbacks.is_empty() {
                    tracing::info!(
                        "[Feedback-Agg] {} new records, {} adjustments (total processed: {})",
                        new_feedbacks.len(),
                        aggregated,
                        processed_ids.len()
                    );
                }
            }
            *self.feedback_agg_cache.lock() = Some((
                feedback_mems.len(),
                std::time::Instant::now(),
                processed_ids,
            ));
        }

        // Skill feedback aggregation: bridge memory feedback → skill success_rate
        {
            let skill_fb_mems = self.gateway.list_by_labels(&["skill-feedback"], 100);
            let (mut processed_sids, should_full_s) = {
                let cache = self.skill_feedback_agg_cache.lock();
                match &*cache {
                    Some((last_count, last_time, ids)) => {
                        let needs_full = skill_fb_mems.len() > *last_count + 50
                            || last_time.elapsed() > std::time::Duration::from_secs(600);
                        (ids.clone(), needs_full)
                    }
                    None => (HashSet::new(), true),
                }
            };

            let new_skill_fb: Vec<_> = if should_full_s {
                skill_fb_mems.iter().collect()
            } else {
                skill_fb_mems
                    .iter()
                    .filter(|(id, _)| !processed_sids.contains(id))
                    .collect()
            };

            if !new_skill_fb.is_empty() {
                let pub_engine = self.pub_skills.lock().clone();
                let mut skill_updates = 0usize;
                for &(fid, p) in &new_skill_fb {
                    let lower = p.content.to_lowercase();
                    let helpful = lower.contains("helpful\":true")
                        || lower.contains("\"helpful\": true")
                        || lower.contains("helpful: true")
                        || lower.contains("helpful=true")
                        || lower.contains("status.*success");
                    let skill_id = {
                        let mut sid = None;
                        if let Some(start) = lower.find("skill_id") {
                            let rest = &lower[start..];
                            if let Some(num_start) = rest.find(|c: char| c.is_ascii_digit()) {
                                let num_str: String = rest[num_start..]
                                    .chars()
                                    .take_while(|c| c.is_ascii_digit())
                                    .collect();
                                sid = num_str.parse::<u64>().ok();
                            }
                        }
                        sid
                    };
                    if let (Some(sid), Some(ref pub_sk)) = (skill_id, &pub_engine) {
                        let _ = pub_sk.record_feedback(sid, helpful);
                        skill_updates += 1;
                        tracing::debug!(
                            "[SkillFeedback-Agg] skill_id={} helpful={} processed",
                            sid,
                            helpful
                        );
                    }
                    processed_sids.insert(*fid);
                }
                if skill_updates > 0 {
                    tracing::info!(
                        "[SkillFeedback-Agg] {} skill feedback records applied",
                        skill_updates
                    );
                }
            }
            *self.skill_feedback_agg_cache.lock() = Some((
                skill_fb_mems.len(),
                std::time::Instant::now(),
                processed_sids,
            ));
        }

        if gov.should_consolidate || gov.should_archive || gov.should_merge {
            if let Some(result) = crate::engine::auto_pipeline::auto_dream(&ctx, last, &purge) {
                self.last_dream_tick.store(tick, Ordering::SeqCst);
                let decayed = self.gateway.decay_relations();
                if !gov.recurrent_ids.is_empty() {
                    crate::engine::governor::LifecycleGovernor::reset_access_counts(
                        &self.space,
                        &gov.recurrent_ids,
                    );
                }

                // Governor merge — actually merge duplicate candidates
                let mut gov_merged = 0usize;
                if gov.should_merge {
                    let candidates =
                        crate::engine::governor::LifecycleGovernor::find_merge_candidates(
                            &self.space,
                        );
                    gov_merged = crate::engine::governor::LifecycleGovernor::execute_merges(
                        &self.space,
                        &candidates,
                    );
                    for c in &candidates {
                        self.persist_tetra(c.keep_id);
                        self.persist_tetra(c.remove_id);
                    }
                }

                self.log_event(format!(
                    "auto_dream(tick={}): consolidated={}, connections={}, merged={}, evicted={}, insights={}, decayed={}, recurrent={}, gov_merged={}",
                    tick,
                    result.report.memories_consolidated,
                    result.report.connections_formed,
                    result.report.duplicates_merged,
                    result.report.junk_evicted,
                    result.report.insights.len(),
                    decayed,
                    gov.recurrent_ids.len(),
                    gov_merged,
                ));
            }
        }

        // Save health snapshot every dream cycle
        {
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
        }
    }

    pub(in crate::engine::scheduler) fn reclassify_memories(
        &self,
        round: usize,
        snap: &TickSnapshot,
    ) {
        self.last_reclassify_tick.store(snap.tick, Ordering::SeqCst);
        let ctx = crate::engine::cognitive_hooks::CognitiveHooksCtx {
            space: &self.space,
            storage: &self.storage,
            gateway: &self.gateway,
            cognitive: &self.cognitive,
        };
        crate::engine::cognitive_hooks::reclassify_memories(&ctx, round, &snap.tetras);
    }

    pub(in crate::engine::scheduler) fn extract_entities(&self, round: usize, snap: &TickSnapshot) {
        let ctx = crate::engine::cognitive_hooks::CognitiveHooksCtx {
            space: &self.space,
            storage: &self.storage,
            gateway: &self.gateway,
            cognitive: &self.cognitive,
        };
        crate::engine::cognitive_hooks::extract_entities(&ctx, round, &snap.tetras);
    }

    pub(in crate::engine::scheduler) fn perform_fission(
        &self,
        cluster_index: usize,
        cooldown: u64,
        energy_cost: f64,
        tag: &str,
    ) -> bool {
        let snap = self.build_snapshot();
        self.perform_fission_from_snap(cluster_index, cooldown, energy_cost, tag, &snap)
    }

    pub(in crate::engine::scheduler) fn perform_fission_from_snap(
        &self,
        cluster_index: usize,
        cooldown: u64,
        energy_cost: f64,
        tag: &str,
        snap: &TickSnapshot,
    ) -> bool {
        let adaptive = self.adaptive.lock();
        let ctx = crate::engine::auto_pipeline::AutoPipelineCtx {
            tick: snap.tick,
            space: &self.space,
            energy: &self.energy,
            knowledge: &self.knowledge,
            gateway: &self.gateway,
            storage: &self.storage,
            emotion_pleasure: 0.0,
            emotion_arousal: 0.0,
            adaptive: &adaptive,
        };
        match crate::engine::auto_pipeline::perform_fission_from_snap(
            &ctx,
            cluster_index,
            cooldown,
            energy_cost,
            tag,
            &snap.clusters,
            &snap.labels_map,
            &snap.core_map,
        ) {
            Some(result) => {
                self.last_fission_tick.store(result.tick, Ordering::SeqCst);
                self.log_event(format!(
                    "{}({},{})",
                    tag.to_lowercase(),
                    cluster_index,
                    result.moved_count
                ));
                true
            }
            None => false,
        }
    }

    pub(in crate::engine::scheduler) fn apply_thought(
        &self,
        tick: u64,
        response: crate::engine::cognitive::CognitiveResponse,
    ) {
        tracing::info!("[LLM thoughts] {}", response.thoughts);
        // 智能突破断裂点1：存储 learning 到 CognitiveEngine（下次 prompt 会注入）
        self.cognitive.store_learning(&response.learning);
        let max_actions = if self.energy.available() < 200.0 {
            2
        } else {
            3
        };
        let limited_actions: Vec<&crate::engine::cognitive::SchedulerAction> =
            response.actions.iter().take(max_actions).collect();
        if response.actions.len() > max_actions {
            tracing::warn!(
                "[Guard] limited {} actions to {}",
                response.actions.len(),
                max_actions
            );
        }
        let pre_snap = self.build_snapshot();
        for action in &limited_actions {
            let action_name = match action {
                SchedulerAction::Pulse { origin, .. } => format!("pulse({})", origin),
                SchedulerAction::Fission { cluster_index } => format!("fission({})", cluster_index),
                SchedulerAction::Fuse {
                    cluster_a,
                    cluster_b,
                } => format!("fuse({},{})", cluster_a, cluster_b),
                SchedulerAction::Dream => "dream".to_string(),
                SchedulerAction::Link { a, b, reason } => format!("link({},{},{})", a, b, reason),
                SchedulerAction::Consolidate { ids, keep, .. } => {
                    format!("consolidate({:?}->{})", ids, keep)
                }
                SchedulerAction::MarkJunk { ids, .. } => format!("mark_junk({:?})", ids),
                SchedulerAction::Relabel {
                    id,
                    add_labels,
                    remove_labels,
                    ..
                } => format!("relabel({}+{:?}-{:?})", id, add_labels, remove_labels),
                SchedulerAction::Reflect { .. } => "reflect".to_string(),
                SchedulerAction::UseTool { tool, .. } => format!("use_tool({})", tool),
                SchedulerAction::ActOutward { intent, .. } => format!("act_outward({})", intent),
            };
            let action_pre_snap = self.build_snapshot();
            self.execute_action(action);
            // 突破3：learn_history 反馈闭环——记录决策的真实后果，而非固定 "executed"。
            // 用动作前后的快照对比判断是否产生了实际变化（能量消耗/簇数变化/记忆变化）。
            // LLM 下次 decide 时会看到 decision_history（cognitive.rs:1043 注入 prompt），
            // 从而从自己决策的效果中学习："上次 dream=effective, pulse=no_effect → 多做 dream 少做 pulse"。
            let post_action_snap = self.build_snapshot();
            // 用长度+能量判断是否产生效果（Vec<TetraMeta> 无 PartialEq，用 len 近似）
            let outcome = if post_action_snap.tetras.len() != action_pre_snap.tetras.len()
                || post_action_snap.clusters.len() != action_pre_snap.clusters.len()
                || (post_action_snap.energy - action_pre_snap.energy).abs() > 0.5
            {
                "effective"
            } else {
                "no_effect"
            };
            self.record_decision(
                tick,
                &action_name,
                &response.thoughts.chars().take(100).collect::<String>(),
                outcome,
            );
        }

        // Close the evolution loop: record outcome effectiveness for EVERY cognitive action type
        // (memory-driven evolution). Previously only Dream recorded outcomes, so adaptive params
        // + drive never learned from Fission/Fuse/Pulse/Link/Consolidate/MarkJunk.
        let post_snap = self.build_snapshot();
        for action in &limited_actions {
            let at = match action {
                SchedulerAction::Pulse { .. } => Some(ActionType::Pulse),
                SchedulerAction::Fission { .. } => Some(ActionType::Fission),
                SchedulerAction::Fuse { .. } => Some(ActionType::Merge),
                SchedulerAction::Dream => Some(ActionType::Dream),
                SchedulerAction::Link { .. } => Some(ActionType::Link),
                SchedulerAction::Consolidate { .. } => Some(ActionType::Merge),
                SchedulerAction::MarkJunk { .. } => Some(ActionType::Evict),
                _ => None,
            };
            if let Some(at) = at {
                self.record_outcome_snaps(at, &pre_snap, &post_snap, tick);
            }
        }
        let action_names: Vec<&str> = limited_actions
            .iter()
            .map(|a| match a {
                SchedulerAction::Pulse { .. } => "pulse",
                SchedulerAction::Fission { .. } => "fission",
                SchedulerAction::Fuse { .. } => "fuse",
                SchedulerAction::Dream => "dream",
                SchedulerAction::Link { .. } => "link",
                SchedulerAction::Consolidate { .. } => "consolidate",
                SchedulerAction::MarkJunk { .. } => "mark_junk",
                SchedulerAction::Relabel { .. } => "relabel",
                SchedulerAction::Reflect { .. } => "reflect",
                SchedulerAction::UseTool { .. } => "use_tool",
                SchedulerAction::ActOutward { .. } => "act_outward",
            })
            .collect();
        tracing::info!(
            "[LLM] tick {} executed {} actions: {:?}",
            tick,
            action_names.len(),
            action_names
        );

        if tick.is_multiple_of(10) {
            self.auto_save();
        }
    }

    pub(in crate::engine::scheduler) fn execute_task(&self, task: &ScheduledTask) {
        match task {
            ScheduledTask::CreateTetra { core, data, mass } => {
                let positions = Tetrahedron::compute_vertices(*core);
                let tetra = Tetrahedron {
                    id: 0,
                    vertex_ids: [0; 4],
                    core: *core,
                    data: data.clone(),
                    mass: *mass,
                };
                match self.space.add_tetrahedron(&tetra, &positions) {
                    Ok(id) => {
                        tracing::info!("created tetrahedron {}", id);
                        let _ = self.tx.send(EngineEvent::TetrahedronCreated(id));
                    }
                    Err(e) => tracing::warn!("failed to create: {}", e),
                }
            }
        }
    }

    /// 冷记忆复习(Deliberative 层,阻塞线程内调用):与其他调度器模型调用共用单飞门,忙则跳过。
    pub(in crate::engine::scheduler) fn cold_review_blocking(&self) {
        if !self.cognitive.enabled() {
            return;
        }
        let Some(_deliberative) = self.deliberative_gate.try_acquire() else {
            self.layers.record_deferred(RunLayer::Deliberative);
            return;
        };
        let _timer = self.layers.time(RunLayer::Deliberative);
        let _ = self.review_cold_memories();
    }

    /// 冷记忆复习(Deliberative 层,事件循环内调用):放进阻塞线程池,不占用 async 工作线程。
    pub(in crate::engine::scheduler) fn spawn_cold_review(self: &Arc<Self>) {
        if !self.cognitive.enabled() {
            return;
        }
        let Some(permit) = self.deliberative_gate.try_acquire() else {
            self.layers.record_deferred(RunLayer::Deliberative);
            return;
        };
        let me = self.clone();
        tokio::task::spawn_blocking(move || {
            let _deliberative = permit;
            let _timer = me.layers.time(RunLayer::Deliberative);
            let _ = me.review_cold_memories();
        });
    }

    /// 等待重新取得 cycle 门(阻塞线程内调用)。超时返回 None。
    pub(in crate::engine::scheduler) fn reacquire_cycle(
        &self,
        timeout: Duration,
    ) -> Option<SingleFlightPermit> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if let Some(p) = self.cycle_gate.try_acquire() {
                return Some(p);
            }
            if std::time::Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Commit 周期,分层执行:
    /// 1. Routine(持 cycle 门):tick + 生成待思考状态
    /// 2. Deliberative(只持 deliberative 门,**释放** cycle 门):模型 decide —— 网络往返期间
    ///    attend/commit 的 routine tick 照常运行,不再整段被推迟
    /// 3. 重新取得 cycle 门后应用决策,写空间仍与 routine 串行
    ///
    /// 上一轮模型调用尚未返回时,本轮的 thought 直接跳过(背压),不会堆积并发模型调用。
    pub(in crate::engine::scheduler) fn run_commit_cycle(
        self: &Arc<Self>,
        cycle_permit: SingleFlightPermit,
    ) {
        let thought = {
            let _timer = self.layers.time(RunLayer::Routine);
            self.tick_and_maybe_think()
        };
        let Some(ct) = thought else {
            let count = self.tick_count.load(Ordering::SeqCst);
            if count.is_multiple_of(3) {
                self.sweep_action_leases();
                self.run_action_planner_pass_if_due();
                self.auto_save();
            } // P0: flush every 3 ticks
            if count.is_multiple_of(5) {
                self.flush_access_counts();
                drop(cycle_permit);
                self.cold_review_blocking();
            }
            return;
        };
        let Some(deliberative) = self.deliberative_gate.try_acquire() else {
            self.layers.record_deferred(RunLayer::Deliberative);
            tracing::debug!(
                "[Layers] tick {} thought skipped; previous model call still running",
                ct.tick
            );
            return;
        };
        // 批次C：注入自适应参数+行动效果到认知引擎（断裂点5+6）
        {
            use crate::engine::adaptive::Param;
            let ad = self.adaptive.lock();
            let snap = format!(
                "fission_entropy: {:.3} (default 0.3)\nfission_min_size: {:.0} (default 6)\nmerge_distance: {:.1} (default 5.0)\ndream_interval: {:.0} (default 50)\nevict_mass: {:.3} (default 0.3)\npulse_budget: {:.0} (default 3)",
                ad.get(Param::FissionEntropyThreshold),
                ad.get(Param::FissionMinClusterSize),
                ad.get(Param::MergeDistance),
                ad.get(Param::DreamInterval),
                ad.get(Param::EvictionMassThreshold),
                ad.get(Param::PulseBudget),
            );
            self.cognitive.set_adaptive_snapshot(&snap);
        }
        {
            let tracker = self.outcome.lock();
            let summary = tracker.effectiveness_summary();
            if !summary.is_empty() {
                let text: Vec<String> = summary
                    .iter()
                    .map(|(action, eff)| format!("{:?}: {:.0}%", action, eff * 100.0))
                    .collect();
                self.cognitive.set_effectiveness_summary(&text.join("\n"));
            }
        }
        // 慢层不占快层的门:模型往返期间释放 cycle 门
        drop(cycle_permit);
        let decision = {
            let _timer = self.layers.time(RunLayer::Deliberative);
            self.cognitive.decide(&ct.state)
        };
        drop(deliberative);
        match decision {
            Ok(response) => {
                // 应用决策会写空间:重新与 routine 串行
                let Some(_cycle_permit) = self.reacquire_cycle(COMMIT_APPLY_WAIT) else {
                    self.layers.record_apply_dropped();
                    tracing::warn!(
                        "[Layers] tick {} decision dropped: cycle gate busy for {:?}",
                        ct.tick,
                        COMMIT_APPLY_WAIT
                    );
                    return;
                };
                self.apply_thought(ct.tick, response);
            }
            Err(e) => tracing::warn!("[LLM] cognitive error: {}", e),
        }
    }

    pub async fn run_with_rx(self: Arc<Self>, rx: broadcast::Receiver<EngineEvent>) {
        self.run_unified(rx, true).await;
    }

    pub async fn run_quiet(self: Arc<Self>, rx: broadcast::Receiver<EngineEvent>) {
        self.run_unified(rx, false).await;
    }

    async fn run_unified(
        self: Arc<Self>,
        mut rx: broadcast::Receiver<EngineEvent>,
        cognitive: bool,
    ) {
        let Some(_loop_permit) = self.loop_gate.try_acquire() else {
            tracing::warn!("[Scheduler] duplicate loop start ignored");
            return;
        };

        loop {
            // 先克隆 interval 值再 drop 读锁，避免 select! 分支持锁跨整个 sleep 周期
            let plan = self.horizon.lock().plan(cognitive);
            tracing::debug!(
                "[Horizon] phase={} sleep_ms={} pressure={:.2} debt={}",
                plan.phase.as_str(),
                plan.sleep_ms,
                plan.pressure,
                plan.debt
            );
            let tick_interval = Duration::from_millis(plan.sleep_ms);
            let commit = cognitive && plan.phase == crate::engine::horizon::HorizonPhase::Commit;
            tokio::select! {
                _ = tokio::time::sleep(tick_interval) => {
                    if commit {
                        let Some(cycle_permit) = self.cycle_gate.try_acquire() else {
                            self.layers.record_deferred(RunLayer::Routine);
                            let missed = {
                                let mut h = self.horizon.lock();
                                h.note_defer();
                                h.missed_commits()
                            };
                            tracing::info!(
                                "[Horizon] commit deferred (missed_commits={}); previous cycle still running",
                                missed
                            );
                            continue;
                        };
                        let me = self.clone();
                        let handle = tokio::task::spawn_blocking(move || {
                            me.run_commit_cycle(cycle_permit);
                        });
                        // tick panic 恢复：监控线程存活，panic 后下一 tick 自动重试
                        tokio::spawn(async move {
                            if let Err(e) = handle.await {
                                tracing::error!("[Scheduler] tick thread panic: {}. Next tick will retry automatically.", e);
                            }
                        });
                    } else {
                        let Some(_cycle_permit) = self.cycle_gate.try_acquire() else {
                            self.layers.record_deferred(RunLayer::Routine);
                            let missed = {
                                let mut h = self.horizon.lock();
                                h.note_defer();
                                h.missed_commits()
                            };
                            tracing::debug!(
                                "[Horizon] attend deferred (missed_commits={}); previous cycle still running",
                                missed
                            );
                            continue;
                        };
                        let _timer = self.layers.time(RunLayer::Routine);
                        let count = self.tick_count.fetch_add(1, Ordering::SeqCst);
                        self.energy.replenish(12.0);
                        let tasks: Vec<ScheduledTask> = self.queue.lock().drain(..).collect();
                        for task in &tasks { self.execute_task(task); }
                        if count.is_multiple_of(5) {
                            let snap = self.build_snapshot();
                            self.auto_fission(&snap);
                            self.auto_save();
                            self.flush_access_counts();
                            self.spawn_cold_review();
                        }
                        if count.is_multiple_of(50) && count > 0 && self.energy.available() >= 50.0 {
                            self.auto_dream();
                        }
                        if count.is_multiple_of(200) && count > 0 {
                            let snap = self.build_snapshot();
                            self.evict_low_quality(&snap);
                        }
                    }
                }
                event = rx.recv() => {
                    let _timer = self.layers.time(RunLayer::Reflex);
                    match event {
                        Ok(EngineEvent::Shutdown) => {
                            tracing::info!("[Scheduler] shutdown, persisting state");
                            self.auto_save();
                            break;
                        }
                        Ok(EngineEvent::TetrahedronCreated(id)) => {
                            self.horizon.lock().note_stimulus(0.4);
                            self.log_event(format!("created({})", id));
                            if self.tick_count.load(Ordering::SeqCst).is_multiple_of(3) { self.auto_save(); }
                        }
                        Ok(EngineEvent::TetrahedronRemoved(id)) => {
                            self.log_event(format!("purged({})", id));
                        }
                        Ok(EngineEvent::TetrahedronMoved(id, _)) => {
                            self.log_event(format!("moved({})", id));
                        }
                        Ok(EngineEvent::EnergyLow { remaining }) => {
                            tracing::warn!("[Scheduler] energy low: {:.1}", remaining);
                        }
                        Ok(EngineEvent::ActionOutcomeLanded {
                            action_id,
                            outcome_memory_id,
                            state,
                        }) => {
                            // Wake path: outcome write-back is a stimulus for the next
                            // deliberative/dream cadence (horizon + event log).
                            self.horizon.lock().note_stimulus(0.55);
                            // PR-L: buffer; the Routine cadence coalesces bursts into one pass.
                            self.planner_pass.note_trigger(action_id, &state);
                            self.log_event(format!(
                                "action_outcome(id={}, mem={}, state={})",
                                action_id, outcome_memory_id, state
                            ));
                            tracing::info!(
                                "[action] outcome landed action={} memory={} state={}",
                                action_id,
                                outcome_memory_id,
                                state
                            );
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            tracing::info!("[Scheduler] event bus closed, persisting state");
                            self.auto_save();
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    pub(in crate::engine::scheduler) fn cluster_ids_centroid(&self, ids: &[u64]) -> Point3 {
        let mut sum_x = 0.0f64;
        let mut sum_y = 0.0f64;
        let mut sum_z = 0.0f64;
        let mut count = 0usize;
        for &id in ids {
            if let Some(t) = self.space.get_tetrahedron(id) {
                sum_x += t.core.x;
                sum_y += t.core.y;
                sum_z += t.core.z;
                count += 1;
            }
        }
        if count > 0 {
            Point3::new(
                sum_x / count as f64,
                sum_y / count as f64,
                sum_z / count as f64,
            )
        } else {
            Point3::zero()
        }
    }

    pub(in crate::engine::scheduler) fn cluster_core_centroid(
        &self,
        cluster: &crate::domain::space::Cluster,
    ) -> Point3 {
        self.cluster_ids_centroid(&cluster.tetra_ids)
    }
}
