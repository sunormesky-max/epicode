use super::*;

impl SchedulerCenter {
    /// P5: 突触修剪 — 平均度数88.5过高致PPR同质化, 每节点保留strength top-K关系
    pub(in crate::engine::scheduler) fn synaptic_pruning(&self) -> usize {
        const MAX_DEGREE: usize = 50;
        let mut pruned = 0;
        // 获取每节点的关系, 按strength排序, 弱于top-K的删除
        let all = self.knowledge.relation_count();
        if all == 0 {
            return 0;
        }

        // 采样检查度数过高的节点
        let tetras = self.space.all_tetras_meta();
        for t in tetras.iter().step_by(50).take(20) {
            // 每50条检查1条, 共20个采样
            let rels = self.knowledge.query_relations(t.id);
            if rels.len() > MAX_DEGREE {
                // 按strength排序, 弱的标记删除
                let mut sorted: Vec<_> = rels.iter().map(|(tid, _rt, s)| (*tid, s)).collect();
                sorted.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
                for (tid, _) in sorted.iter().skip(MAX_DEGREE) {
                    self.knowledge.remove_relation(
                        t.id,
                        *tid,
                        crate::engine::knowledge::RelationType::SimilarTo,
                    );
                    pruned += 1;
                }
            }
        }
        if pruned > 0 {
            tracing::info!(
                "[P5] pruned {} weak synapses (degree cap {})",
                pruned,
                MAX_DEGREE
            );
        }
        pruned
    }

    /// P2: dream复习相 — 78%记忆从未被检索命中, 沉默老化未经价值验证
    /// 采样cold记忆→LLM判断→有值标记reviewed+升importance / 无值降权
    ///
    /// 每条记忆在冷却期内只复习一次:以前同一批(list_nodes 顺序的前 10 条)每次都被重新抽中,
    /// KEEP 每轮 +0.3 直到封顶、FADE 每轮减半直到跌出候选 —— 一次判断被重复施加多次,
    /// 其余冷记忆永远轮不到。
    pub(in crate::engine::scheduler) fn review_cold_memories(&self) -> usize {
        /// 同一条冷记忆两次复习之间的最短间隔(7 天)。
        const COLD_REVIEW_COOLDOWN_SECS: i64 = 7 * 86_400;
        let now = chrono::Utc::now().timestamp();
        let recently_reviewed = {
            let mut map = self.cold_reviewed_at.lock();
            map.retain(|_, at| now - *at < COLD_REVIEW_COOLDOWN_SECS);
            map.keys().copied().collect::<HashSet<TetraId>>()
        };
        let cold: Vec<(u64, String)> = self
            .gateway
            .list_nodes()
            .into_iter()
            .filter(|(id, p)| {
                p.valid_to.is_none()
                    && p.importance > 0.1
                    && p.access_count == 0
                    && !recently_reviewed.contains(id)
                    && p.last_reviewed_ts
                        .is_none_or(|ts| now - ts >= COLD_REVIEW_COOLDOWN_SECS)
            })
            .take(10)
            .map(|(id, p)| (id, p.content.chars().take(200).collect()))
            .collect();
        if cold.is_empty() {
            return 0;
        }

        let prompt = format!(
            "For each memory below, answer in one word: KEEP (still valuable) or FADE (no longer relevant). Format: id:KEEP or id:FADE per line.

{}",
            cold.iter().map(|(id, c)| format!("#{}: {}", id, c)).collect::<Vec<_>>().join("
")
        );
        let Ok(response) = self.cognitive.generate_free_text(&prompt, 300) else {
            return 0;
        };

        let mut reviewed = 0;
        {
            // 已送审即进入冷却(含模型未给出判断的条目),避免下一轮又抽到同一批
            let mut map = self.cold_reviewed_at.lock();
            for (id, _) in &cold {
                map.insert(*id, now);
            }
        }
        for (id, _) in &cold {
            let keep = response.contains(&format!("#{}:KEEP", id))
                || response.contains(&format!("{}: KEEP", id));
            let fade = response.contains(&format!("#{}:FADE", id))
                || response.contains(&format!("{}: FADE", id));
            if keep {
                // 有价值: 标记已复习+提升importance
                if let Some(t) = self.space.get_tetrahedron(*id) {
                    let mut d = t.data.clone();
                    d.last_reviewed_ts = Some(now);
                    d.importance = (d.importance + 0.3).min(3.0);
                    let _ = self.space.patch_payload(*id, &t.data, d);
                    if let Some(t) = self.space.get_tetrahedron(*id) {
                        let _ = self.storage.upsert_tetra(&t);
                    }
                    reviewed += 1;
                }
            } else if fade {
                // 无价值: 降权(×0.7, 温和) — 但尊重宪法地板 IMPORTANCE_FLOOR。
                // 旧版 ×0.5 且 .max(0.05): 单次 LLM 判决(仅看前 200 字符预览)即可
                // 打穿 governor 维护的活性地板, 属自动流程越权; 低于地板只允许
                // 显式遗忘通道(FORGET_IMPORTANCE 例外, 见 governor 模块文档)。
                if let Some(t) = self.space.get_tetrahedron(*id) {
                    let mut d = t.data.clone();
                    d.importance =
                        (d.importance * 0.7).max(crate::engine::governor::IMPORTANCE_FLOOR);
                    let _ = self.space.patch_payload(*id, &t.data, d);
                    if let Some(t) = self.space.get_tetrahedron(*id) {
                        let _ = self.storage.upsert_tetra(&t);
                    }
                    reviewed += 1;
                }
            }
        }
        if reviewed > 0 {
            tracing::info!(
                "[P2] reviewed {} cold memories ({} of {} sampled)",
                reviewed,
                reviewed,
                cold.len()
            );
        }
        reviewed
    }

    pub(in crate::engine::scheduler) fn persist_tetra(&self, id: TetraId) {
        let ctx = crate::engine::janitor::JanitorCtx {
            space: &self.space,
            storage: &self.storage,
            knowledge: &self.knowledge,
            gateway: &self.gateway,
        };
        crate::engine::janitor::mark_dirty_persist(&ctx, id);
    }

    /// Deprioritize a memory WITHOUT deleting it (constitution §4.5 — memories are sacred).
    /// Adds "quarantine" label, sets valid_to, floors importance/mass, and persists.
    /// Replaces the former purge_tetra path used by evict/governor.
    pub(in crate::engine::scheduler) fn supersede_tetra(&self, id: TetraId) {
        if let Some(tetra) = self.space.get_tetrahedron(id) {
            let old_labels = tetra.data.labels.clone();
            let mut data = tetra.data.clone();
            if !data.labels.iter().any(|l| l == "quarantine") {
                data.labels.push("quarantine".to_string());
            }
            let now = chrono::Utc::now().timestamp();
            data.valid_to = Some(now);
            data.importance = data
                .importance
                .min(crate::engine::governor::IMPORTANCE_FLOOR); // O-A
            let new_labels = data.labels.clone();
            if let Err(e) = self.space.patch_payload(id, &tetra.data, data) {
                tracing::warn!("[Scheduler] update_payload {} failed: {}", id, e);
            }
            if let Err(e) = self.space.update_mass(id, 0.05) {
                tracing::warn!("[Scheduler] update_mass {} failed: {}", id, e);
            }
            let _ = self.space.update_validity(id, Some(now));
            self.gateway
                .update_label_index(id, &old_labels, &new_labels);
        }
        self.persist_tetra(id);
        tracing::info!(
            "[Supersede] tetra {} deprioritized (quarantine label, NO deletion)",
            id
        );
    }

    pub(in crate::engine::scheduler) fn purge_tetra(&self, id: TetraId) {
        let labels = self
            .space
            .get_tetrahedron(id)
            .map(|t| t.data.labels.clone());
        if let Err(e) = self.space.remove_tetrahedron(id) {
            tracing::debug!("purge_tetra {}: space already removed: {}", id, e);
        }
        if let Err(e) = self.storage.delete_tetra(id) {
            tracing::warn!("purge_tetra {}: storage delete failed: {}", id, e);
        }
        self.knowledge.remove_relations_for(id);
        if let Some(ref lbls) = labels {
            self.gateway.on_tetra_removed(id, lbls);
        } else {
            self.gateway.remove_from_hnsw(id);
            self.gateway.remove_from_content_hash(id);
        }
        let _ = self
            .tx
            .send(crate::engine::bus::EngineEvent::TetrahedronRemoved(id));
        tracing::info!(
            "[Purge] tetra {} fully cleaned (space+storage+KG+HNSW+index)",
            id
        );
    }

    pub(in crate::engine::scheduler) fn record_outcome(
        &self,
        action: ActionType,
        pre_snap: &TickSnapshot,
        tick: u64,
    ) {
        let post_snap = self.build_snapshot();
        self.record_outcome_snaps(action, pre_snap, &post_snap, tick);
    }

    /// Core outcome recording that accepts a pre-built post-snapshot (avoids rebuilding
    /// the O(N) snapshot once per action when recording several outcomes from one decision).
    pub(in crate::engine::scheduler) fn record_outcome_snaps(
        &self,
        action: ActionType,
        pre_snap: &TickSnapshot,
        post_snap: &TickSnapshot,
        tick: u64,
    ) {
        let pre_entropy = if !pre_snap.clusters.is_empty() {
            pre_snap
                .clusters
                .iter()
                .map(|c| dynamics::compute_entropy_from_labels(&c.tetra_ids, &pre_snap.labels_map))
                .sum::<f64>()
                / pre_snap.clusters.len() as f64
        } else {
            0.0
        };
        let post_entropy = if !post_snap.clusters.is_empty() {
            post_snap
                .clusters
                .iter()
                .map(|c| dynamics::compute_entropy_from_labels(&c.tetra_ids, &post_snap.labels_map))
                .sum::<f64>()
                / post_snap.clusters.len() as f64
        } else {
            0.0
        };
        let mut outcome = ActionOutcome {
            action,
            pre_entropy,
            post_entropy,
            pre_cluster_count: pre_snap.clusters.len(),
            post_cluster_count: post_snap.clusters.len(),
            pre_tetra_count: pre_snap.tetras.len(),
            post_tetra_count: post_snap.tetras.len(),
            pre_energy: pre_snap.energy,
            post_energy: post_snap.energy,
            effectiveness: 0.0,
            tick,
        };
        outcome.compute_effectiveness();

        let effectiveness = outcome.effectiveness;
        let mut outcome_tracker = self.outcome.lock();
        outcome_tracker.record(outcome);
        drop(outcome_tracker);

        let mut adaptive = self.adaptive.lock();
        adaptive.adapt_from_outcome(action, effectiveness);
        drop(adaptive);

        if let Some(drive_type) = match action {
            ActionType::Pulse | ActionType::Link => Some(crate::engine::drive::Drive::Curiosity),
            ActionType::Fission | ActionType::Merge => Some(crate::engine::drive::Drive::Coherence),
            ActionType::Dream => Some(crate::engine::drive::Drive::Efficiency),
            ActionType::Evict => Some(crate::engine::drive::Drive::Efficiency),
        } {
            self.drive.lock().reward(drive_type, effectiveness);
        }
    }

    pub(in crate::engine::scheduler) fn auto_save(&self) {
        // P2/P5/P6: 空间维护(dream不做, tick做轻量版)
        // 冷记忆复习(P2)是模型调用,已移到 Deliberative 层(cold_review_*):
        // auto_save 在事件循环线程里被频繁内联调用,不能在这里做网络往返。
        let _ = self.synaptic_pruning();
        let _ = self.storage.archive_old_drive_signals();
        // 2026-09-22审计接线: 过期记忆归档(此前archive_stale_superseded从未被调用, 最后归档停在8/16)
        if let Ok(moved) = self.storage.archive_stale_superseded(30) {
            if moved > 0 {
                tracing::info!(
                    "[scheduler] archive_stale_superseded moved {} tetras",
                    moved
                );
            }
        }
        self.save_tick_state();
        self.save_drive_engine(); // 稳态: 演化状态随周期保存
        let ctx = crate::engine::janitor::JanitorCtx {
            space: &self.space,
            storage: &self.storage,
            knowledge: &self.knowledge,
            gateway: &self.gateway,
        };
        crate::engine::janitor::auto_save(&ctx);
    }

    /// Periodic flush of buffered access_count totals to Space + storage.
    /// Replaces the former per-search-result write (which held the Space write-lock per result).
    pub(in crate::engine::scheduler) fn flush_access_counts(&self) {
        let snapshot = self.gateway.access_counts_snapshot();
        // 先更新内存中的 payload，收集需要持久化的 (id, count) 对
        let mut dirty_updates: Vec<(TetraId, u32)> = Vec::new();
        for (id, session_delta) in &snapshot {
            if let Some(tetra) = self.space.get_tetrahedron(*id) {
                // session_delta 是本次进程的增量；真正的累计值 = DB历史值 + 增量
                // 修复倒退bug：原直接写 session_delta 覆盖了重启前的历史值
                let cumulative = tetra.data.access_count.saturating_add(*session_delta);
                if tetra.data.access_count != cumulative {
                    // M2修复:用单字段更新,避免 clone 整个 payload(含 8KB embedding)
                    if let Err(e) = self.space.update_access_count(*id, cumulative) {
                        tracing::warn!("[Scheduler] update_access_count {} failed: {}", id, e);
                    }
                    // 突破3: 遗忘曲线 — 被搜索命中的记忆更新复习时间,重置衰减节拍
                    let now = chrono::Utc::now().timestamp();
                    let _ = self.space.update_last_reviewed(*id, now);
                    // 探索回路闭合(滋养侧): 命中喂质量, 对抗梦周期 ×0.98 衰减——
                    // 检索到的记忆保重, 检索不到的缓慢失重落底被隔离。
                    let _ = self.space.update_mass(*id, 0.02);
                    dirty_updates.push((*id, cumulative));
                }
            }
        }
        // 批量写入 DB — 单事务一次锁
        if !dirty_updates.is_empty() {
            if let Err(e) = self.storage.batch_update_access_counts(&dirty_updates) {
                tracing::warn!(
                    "[Scheduler] batch_update_access_counts failed ({} ids): {}",
                    dirty_updates.len(),
                    e
                );
            }
        }
        // 清空 session 计数器（已合并到 DB 值）
        self.gateway.reset_session_access_counts();
    }

    pub(in crate::engine::scheduler) fn log_event(&self, event: String) {
        let mut events = self.recent_events.lock();
        events.push(event);
        if events.len() > 20 {
            let excess = events.len() - 20;
            events.drain(0..excess);
        }
    }

    pub(in crate::engine::scheduler) fn evict_low_quality(&self, snap: &TickSnapshot) {
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
        let purge = |id: TetraId| {
            self.supersede_tetra(id);
        };
        crate::engine::auto_pipeline::evict_low_quality(&ctx, &snap.tetras, &purge);
    }

    pub(in crate::engine::scheduler) fn record_decision(
        &self,
        tick: u64,
        action: &str,
        detail: &str,
        result: &str,
    ) {
        let mut hist = self.decision_history.lock();
        hist.push(crate::engine::cognitive::DecisionRecord {
            tick,
            action: action.to_string(),
            detail: detail.to_string(),
            result: result.to_string(),
        });
        if hist.len() > 30 {
            let excess = hist.len() - 30;
            hist.drain(0..excess);
        }
    }

    pub(in crate::engine::scheduler) fn generate_aliases(&self, round: usize, snap: &TickSnapshot) {
        let ctx = crate::engine::cognitive_hooks::CognitiveHooksCtx {
            space: &self.space,
            storage: &self.storage,
            gateway: &self.gateway,
            cognitive: &self.cognitive,
        };
        crate::engine::cognitive_hooks::generate_aliases(&ctx, round, &snap.tetras);
    }
}
