use super::*;

impl SchedulerCenter {
    pub fn save_worker_registry(&self) {
        let snap = self.worker_registry.snapshot();
        match serde_json::to_string(&snap) {
            Ok(js) => {
                if let Err(e) = self.storage.save_drive_kv("worker_registry", &js) {
                    tracing::warn!("[dispatch] worker registry save failed: {}", e);
                }
            }
            Err(e) => tracing::warn!("[dispatch] worker registry serialize failed: {}", e),
        }
    }

    pub fn restore_worker_registry(&self) {
        if let Some(js) = self.storage.load_drive_kv("worker_registry") {
            match serde_json::from_str::<Vec<crate::engine::dispatch::WorkerRecord>>(&js) {
                Ok(workers) => {
                    self.worker_registry.restore(workers);
                    tracing::info!(
                        "[dispatch] restored {} workers",
                        self.worker_registry.list().len()
                    );
                }
                Err(e) => tracing::warn!("[dispatch] worker registry restore failed: {}", e),
            }
        }
    }

    /// PR-I: return lapsed claims to the frontier (or escalate). Called on
    /// restart (after registry restore) and from the Routine save cadence.
    pub fn sweep_action_leases(&self) -> usize {
        let swept = crate::engine::dispatch::sweep_expired_leases(
            &self.worker_registry,
            &self.action_frontier,
        );
        // PR-K: also settle dependents of failed/pruned upstreams (e.g. after restart).
        let cascaded = self.action_frontier.cascade_dep_failures();
        if !cascaded.is_empty() {
            tracing::info!("[action] dep_failed cascade blocked {:?}", cascaded);
        }
        if !swept.is_empty() || !cascaded.is_empty() {
            tracing::info!("[action] lease sweep reclaimed {:?}", swept);
            self.save_action_frontier();
        }
        swept.len()
    }

    /// PR-L: run a pass if the debounce/heartbeat says so; notify new signals.
    pub fn run_action_planner_pass_if_due(&self) {
        if let Some(r) = self
            .planner_pass
            .maybe_run(&self.action_frontier, &self.worker_registry)
        {
            self.notify_planner_signals(&r.new_signals);
        }
    }

    /// Force a pass now (MCP/tests).
    pub fn run_action_planner_pass_now(&self) -> crate::engine::planner_pass::PassReport {
        let r = self.planner_pass.run(
            crate::engine::planner_pass::PassReason::Manual,
            &self.action_frontier,
            &self.worker_registry,
        );
        self.notify_planner_signals(&r.new_signals);
        r
    }

    /// Immediate hook for escalation (does not wait for the next pass). Deduped
    /// with the pass, so the human is told exactly once per escalated action.
    pub fn notify_action_escalated(&self, action_id: u64, reason: &str) {
        let sig = crate::engine::planner_pass::StallSignal::Escalated {
            action_id,
            reason: reason.to_string(),
        };
        if self.planner_pass.mark_notified(&sig) {
            self.enqueue_planner_drive(&sig);
        }
    }

    pub(in crate::engine::scheduler) fn notify_planner_signals(
        &self,
        sigs: &[crate::engine::planner_pass::StallSignal],
    ) {
        for s in sigs {
            tracing::warn!("[planner_pass] {}", s.describe());
            self.enqueue_planner_drive(s);
        }
    }

    pub(in crate::engine::scheduler) fn enqueue_planner_drive(
        &self,
        sig: &crate::engine::planner_pass::StallSignal,
    ) {
        use crate::engine::drive::{DriveIntent, DriveSignal, DriveUrgency};
        let urgency = match sig {
            crate::engine::planner_pass::StallSignal::Escalated { .. } => DriveUrgency::High,
            _ => DriveUrgency::Medium,
        };
        let signal = DriveSignal {
            id: 0,
            timestamp: chrono::Utc::now().timestamp(),
            intent_type: DriveIntent::Warn,
            description: sig.describe(),
            evidence: vec![],
            urgency: urgency.clone(),
            target_capability: Some("conversation".into()),
            emotion: None,
            origin_tick: self.tick_count.load(Ordering::SeqCst),
            status: crate::engine::drive::default_status(),
            feedback: None,
            retry_count: 0,
            expires_at: crate::engine::drive::default_expires_at(&urgency),
            enqueued_at_ms: 0,
            time_budget_ms: None,
            grounding: None,
            terminal_reason: None,
        };
        match self.drive_queue.try_enqueue(signal) {
            Ok(_) => self.save_drive_queue(),
            Err(e) => tracing::warn!("[planner_pass] drive enqueue failed: {:?}", e),
        }
    }

    pub fn save_action_frontier(&self) {
        self.save_worker_registry();
        // Keep frontier bounded across restarts: drop terminal actions older than 7 days.
        let pruned = self.action_frontier.prune_terminal_older_than(7 * 86400);
        if pruned > 0 {
            tracing::info!("[action] pruned {} stale terminal actions", pruned);
        }
        if let Ok(js) = serde_json::to_string(&self.action_frontier.goals_snapshot()) {
            if let Err(e) = self.storage.save_drive_kv("agent_action_goals", &js) {
                tracing::warn!("[action] goals save failed: {}", e);
            }
        }
        let snap = self.action_frontier.snapshot();
        match serde_json::to_string(&snap) {
            Ok(js) => {
                if let Err(e) = self.storage.save_drive_kv("agent_action_frontier", &js) {
                    tracing::warn!("[action] frontier save failed: {}", e);
                }
            }
            Err(e) => tracing::warn!("[action] frontier serialize failed: {}", e),
        }
    }

    pub fn restore_action_frontier(&self) {
        if let Some(js) = self.storage.load_drive_kv("agent_action_goals") {
            match serde_json::from_str::<Vec<crate::engine::agent_action::ActionGoal>>(&js) {
                Ok(g) => self.action_frontier.restore_goals(g),
                Err(e) => tracing::warn!("[action] goals restore failed: {}", e),
            }
        }
        if let Some(js) = self.storage.load_drive_kv("agent_action_frontier") {
            match serde_json::from_str::<Vec<crate::engine::agent_action::AgentAction>>(&js) {
                Ok(actions) => {
                    self.action_frontier.restore(actions);
                    tracing::info!(
                        "[action] restored {} actions from storage",
                        self.action_frontier.snapshot().len()
                    );
                }
                Err(e) => tracing::warn!("[action] frontier restore failed: {}", e),
            }
        }
    }
    /// L0 Active Inference: detect prediction errors with INTENT ROUTING.
    ///
    /// Analyzes memory content to determine the correct intent type:
    /// - identity/security/boundary/enforced → WARN (needs human attention)
    /// - architecture/decision/bridge/protocol → SUGGEST (needs agent evaluation)
    /// - knowledge-gap/curiosity → EXPLORE (self-driving can handle)
    ///
    /// Also deduplicates: won't re-enqueue if evidence already has pending signal.
    /// L0: Restore drive queue from SQLite after engine load.
    /// ingested 持久化(债: 曾为内存HashSet, 重启清零致agent端重复拉取)
    pub fn save_ingested(&self) {
        let mut ids = self.drive_queue.ingested_ids();
        let overflow = ids.len().saturating_sub(1000);
        if overflow > 0 {
            ids.drain(..overflow);
            self.drive_queue.restore_ingested(ids.clone());
        }
        if let Ok(v) = serde_json::to_string(&ids) {
            let _ = self.storage.save_drive_kv("ingested_ids", &v);
        }
    }
    pub fn restore_ingested(&self) {
        if let Some(v) = self.storage.load_drive_kv("ingested_ids") {
            if let Ok(ids) = serde_json::from_str::<Vec<u64>>(&v) {
                let n = ids.len();
                self.drive_queue.restore_ingested(ids);
                tracing::info!("[Drive] ingested restored: {} ids", n);
            }
        }
    }

    /// 稳态恢复: 演化状态 (weights/history) — 曾重启即失忆
    pub fn restore_drive_engine(&self) {
        if let Some(data) = self.storage.load_drive_engine_state() {
            match serde_json::from_str::<serde_json::Value>(&data) {
                Ok(v) => {
                    let mut de = self.drive.lock();
                    let before = de.evolution_snapshot();
                    de.restore_from(&v);
                    tracing::info!(
                        "[Drive] engine state restored: {:?} -> {:?}",
                        before.get("weights"),
                        de.evolution_snapshot().get("weights")
                    );
                }
                Err(e) => tracing::warn!("[Drive] engine state parse failed: {}", e),
            }
        }
    }
    pub fn save_drive_engine(&self) {
        let snap = self.drive.lock().persist_snapshot();
        if let Ok(data) = serde_json::to_string(&snap) {
            if let Err(e) = self.storage.save_drive_engine_state(&data) {
                tracing::warn!("[Drive] engine state save failed: {}", e);
            }
        }
    }

    pub fn save_tick_state(&self) {
        let tick = self.tick_count.load(Ordering::SeqCst);
        let dream = self.last_dream_tick.load(Ordering::SeqCst);
        let fission = self.last_fission_tick.load(Ordering::SeqCst);
        let body_missing = self.last_body_missing_tick.load(Ordering::SeqCst);
        let body = format!("{},{},{},{}", tick, dream, fission, body_missing);
        if let Err(e) = self.storage.save_drive_kv("scheduler_tick", &body) {
            tracing::warn!("[Scheduler] save tick failed: {}", e);
        }
        let adaptive = self.adaptive.lock().to_json().to_string();
        if let Err(e) = self.storage.save_drive_kv("adaptive_params", &adaptive) {
            tracing::warn!("[Scheduler] save adaptive params failed: {}", e);
        }
    }

    pub fn restore_tick_state(&self) {
        if let Some(raw) = self.storage.load_drive_kv("adaptive_params") {
            match serde_json::from_str::<serde_json::Value>(&raw) {
                Ok(v) => {
                    let n = self.adaptive.lock().restore_json(&v);
                    tracing::info!("[Scheduler] restored {} adaptive params", n);
                }
                Err(e) => tracing::warn!("[Scheduler] adaptive params snapshot unreadable: {}", e),
            }
        }
        if let Some(s) = self.storage.load_drive_kv("scheduler_tick") {
            let parts: Vec<&str> = s.split(',').collect();
            if parts.len() >= 2 {
                if let (Ok(tick), Ok(dream)) = (parts[0].parse::<u64>(), parts[1].parse::<u64>()) {
                    self.tick_count.store(tick, Ordering::SeqCst);
                    self.last_dream_tick.store(dream, Ordering::SeqCst);
                    if parts.len() >= 3 {
                        if let Ok(f) = parts[2].parse::<u64>() {
                            self.last_fission_tick.store(f, Ordering::SeqCst);
                        }
                    }
                    if parts.len() >= 4 {
                        if let Ok(b) = parts[3].parse::<u64>() {
                            self.last_body_missing_tick.store(b, Ordering::SeqCst);
                        }
                    }
                    tracing::info!(
                        "[Scheduler] restored tick={} dream={} fission={} body_missing={}",
                        tick,
                        dream,
                        parts.get(2).unwrap_or(&"?"),
                        parts.get(3).unwrap_or(&"0")
                    );
                }
            }
        }
    }

    pub fn restore_drive_queue(&self) {
        match self.storage.max_drive_signal_id() {
            Ok(max_id) => self.drive_queue.reserve_next_id(max_id.saturating_add(1)),
            Err(e) => tracing::warn!("[L0] failed to read archived drive signal IDs: {}", e),
        }
        match self.storage.load_drive_signals() {
            Ok(signals) => {
                if !signals.is_empty() {
                    tracing::info!("[L0] restoring {} drive signals from SQLite", signals.len());
                    self.drive_queue.restore(signals);
                }
            }
            Err(e) => tracing::warn!("[L0] failed to load drive signals: {}", e),
        }
        if let Some(js) = self.storage.load_drive_kv("ingested_ids") {
            if let Ok(ids) = serde_json::from_str::<Vec<u64>>(&js) {
                self.drive_queue.restore_ingested(ids);
            }
        }
        if let Some(js) = self.storage.load_drive_kv("signal_policy") {
            if let Ok(p) = serde_json::from_str(&js) {
                self.drive_queue.restore_policy(p);
            }
        }
        if let Some(vs) = self.storage.load_drive_kv("signal_policy_version") {
            if let Ok(v) = vs.parse::<u64>() {
                self.drive_queue.set_policy_version(v);
            }
        }
    }

    pub(in crate::engine::scheduler) fn grounding_for_evidence(
        &self,
        evidence_ids: &[u64],
        reason: &str,
        conflict_ids: &[u64],
        freshness_window_secs: Option<i64>,
    ) -> Option<crate::engine::drive::DriveGrounding> {
        if evidence_ids.is_empty() {
            return None;
        }

        let now = chrono::Utc::now().timestamp();
        let mut evidence = Vec::new();
        let mut unresolved = false;
        for &id in evidence_ids {
            match self.api_get_node(id) {
                Some(memory) if crate::engine::drive::memory_is_current(&memory, now) => {
                    evidence.push(crate::engine::drive::DriveMemoryEvidence {
                        id,
                        revision: format!(
                            "{:016x}",
                            crate::engine::drive::memory_revision(&memory)
                        ),
                        recorded_at: memory.timestamp,
                        last_reviewed_at: memory.last_reviewed_ts,
                        importance: memory.importance,
                    });
                }
                _ => unresolved = true,
            }
        }
        if evidence.is_empty() {
            return None;
        }

        let mut uncertainty = Vec::new();
        if evidence.len() == 1 {
            uncertainty.push(crate::engine::drive::DriveGroundingUncertainty::SingleMemorySource);
        }
        if evidence.iter().any(|item| item.last_reviewed_at.is_none()) {
            uncertainty.push(crate::engine::drive::DriveGroundingUncertainty::NotReviewed);
        }
        if !conflict_ids.is_empty() {
            uncertainty.push(crate::engine::drive::DriveGroundingUncertainty::KnownConflict);
        }
        if unresolved {
            uncertainty.push(crate::engine::drive::DriveGroundingUncertainty::UnresolvedEvidence);
        }

        Some(crate::engine::drive::DriveGrounding {
            reason: reason.to_string(),
            complete: !unresolved && evidence.len() == evidence_ids.len(),
            fresh_until: freshness_window_secs.map(|window| {
                evidence
                    .iter()
                    .map(|source| source.recorded_at.saturating_add(window))
                    .max()
                    .unwrap_or_default()
            }),
            evidence,
            uncertainty,
        })
    }

    pub(in crate::engine::scheduler) fn active_contradiction_ids(
        &self,
        id: u64,
        now: i64,
    ) -> Vec<u64> {
        let mut contradictions: Vec<u64> = self
            .api_get_relations(id)
            .into_iter()
            .filter(|(_, relation, _)| relation.eq_ignore_ascii_case("contradicts"))
            .filter_map(|(other_id, _, _)| {
                self.api_get_node(other_id)
                    .filter(|memory| crate::engine::drive::memory_is_current(memory, now))
                    .map(|_| other_id)
            })
            .collect();
        contradictions.sort_unstable();
        contradictions.dedup();
        contradictions
    }

    pub(in crate::engine::scheduler) fn signal_evidence_is_current(
        &self,
        signal: &crate::engine::drive::DriveSignal,
        now: i64,
    ) -> bool {
        let Some(grounding) = signal.grounding.as_ref() else {
            return true;
        };
        if !grounding.is_fresh_at(now) {
            return false;
        }
        let sources_current = grounding.evidence.iter().all(|source| {
            self.api_get_node(source.id).is_some_and(|memory| {
                crate::engine::drive::memory_is_current(&memory, now)
                    && format!("{:016x}", crate::engine::drive::memory_revision(&memory))
                        == source.revision
            })
        });
        if !sources_current {
            return false;
        }
        if matches!(
            &signal.intent_type,
            crate::engine::drive::DriveIntent::Suggest
        ) {
            if grounding
                .uncertainty
                .contains(&crate::engine::drive::DriveGroundingUncertainty::KnownConflict)
            {
                return false;
            }
            return grounding
                .evidence
                .iter()
                .all(|source| self.active_contradiction_ids(source.id, now).is_empty());
        }
        true
    }

    pub(in crate::engine::scheduler) fn refresh_signal_grounding(
        &self,
        signal: &mut crate::engine::drive::DriveSignal,
        now: i64,
    ) {
        let Some(grounding) = signal.grounding.as_mut() else {
            return;
        };
        for source in &mut grounding.evidence {
            if let Some(memory) = self.api_get_node(source.id) {
                source.last_reviewed_at = memory.last_reviewed_ts;
            }
        }
        if grounding
            .evidence
            .iter()
            .any(|source| source.last_reviewed_at.is_none())
        {
            if !grounding
                .uncertainty
                .contains(&crate::engine::drive::DriveGroundingUncertainty::NotReviewed)
            {
                grounding
                    .uncertainty
                    .push(crate::engine::drive::DriveGroundingUncertainty::NotReviewed);
            }
        } else {
            grounding.uncertainty.retain(|value| {
                *value != crate::engine::drive::DriveGroundingUncertainty::NotReviewed
            });
        }

        if !matches!(
            &signal.intent_type,
            crate::engine::drive::DriveIntent::Suggest
        ) {
            let has_known_conflict = grounding
                .evidence
                .iter()
                .any(|source| !self.active_contradiction_ids(source.id, now).is_empty());
            if has_known_conflict {
                if !grounding
                    .uncertainty
                    .contains(&crate::engine::drive::DriveGroundingUncertainty::KnownConflict)
                {
                    grounding
                        .uncertainty
                        .push(crate::engine::drive::DriveGroundingUncertainty::KnownConflict);
                }
            } else {
                grounding.uncertainty.retain(|value| {
                    *value != crate::engine::drive::DriveGroundingUncertainty::KnownConflict
                });
            }
        }
    }

    /// Return only unacknowledged signals whose captured memory sources still match.
    /// Changed, expired, forgotten, or superseded sources terminalize the proposal.
    pub fn current_drive_inbox(&self, limit: usize) -> Vec<crate::engine::drive::DriveSignal> {
        let now = chrono::Utc::now().timestamp();
        let candidates = self.drive_queue.peek_unacked(usize::MAX);
        let mut current = Vec::new();
        let mut stale_ids = Vec::new();

        for signal in candidates {
            if current.len() >= limit {
                break;
            }
            if !self.signal_evidence_is_current(&signal, now) {
                stale_ids.push(signal.id);
            } else {
                let mut signal = signal;
                self.refresh_signal_grounding(&mut signal, now);
                current.push(signal);
            }
        }

        if self.drive_queue.expire_stale_evidence(&stale_ids) > 0 {
            self.save_drive_queue();
        }
        current
    }

    pub fn current_drive_signal(&self, drive_id: u64) -> Option<crate::engine::drive::DriveSignal> {
        let signal = self
            .drive_queue
            .get_signal(drive_id)
            .filter(crate::engine::drive::DriveSignal::is_retryable)?;
        let now = chrono::Utc::now().timestamp();
        if self.signal_evidence_is_current(&signal, now) {
            let mut signal = signal;
            self.refresh_signal_grounding(&mut signal, now);
            Some(signal)
        } else if self.drive_queue.expire_stale_evidence(&[drive_id]) > 0 {
            self.save_drive_queue();
            None
        } else {
            None
        }
    }

    pub fn detect_prediction_errors(&self) -> Vec<crate::engine::drive::DriveSignal> {
        let mut signals = Vec::new();
        let now = chrono::Utc::now().timestamp();

        // Get existing unacked evidence to dedup.
        // peek_unacked 覆盖 Pending+Delivered: 信号被daemon取走(Delivered)但执行端未ack前,
        // 同一evidence不得重发 — 曾致 #5982 在30分钟窗内重复产 #105187/#105188
        let pending = self
            .drive_queue
            .peek_unacked(crate::engine::drive::MAX_QUEUE_SIGNALS);
        let pending_evidence: std::collections::HashSet<u64> = pending
            .iter()
            .flat_map(|s| s.evidence.iter().copied())
            .collect();

        // ── Category 1: WARN — only for RECENTLY written identity/security memories ──
        // Don't scan the full library every tick — that creates noise.
        // Only warn on memories written in the last 10 minutes.
        let now_ts = now;
        let ten_min_ago = now_ts - 600;
        let warn_candidates = self.gateway.list_by_labels_matching(
            &["identity", "security", "enforced", "boundary"],
            50,
            |id, memory| {
                memory.timestamp > ten_min_ago
                    && memory.timestamp <= now_ts
                    && memory.importance >= 2.5
                    && !pending_evidence.contains(&id)
                    && crate::engine::drive::memory_is_current(memory, now_ts)
                    && !memory
                        .labels
                        .iter()
                        .any(|label| label == "l0-exempt" || label == "quarantine")
                    && !crate::engine::drive::will_content_closed(&memory.content)
            },
        );

        for (id, p) in warn_candidates.iter().take(2) {
            let conflicts = self.active_contradiction_ids(*id, now_ts);
            let mut evidence = vec![*id];
            evidence.extend(conflicts.iter().copied());
            evidence.sort_unstable();
            evidence.dedup();
            let content_lower = p.content.to_lowercase();
            let mut desc = if content_lower.contains("identity") || content_lower.contains("身份")
            {
                format!("Identity-related memory #{} has high importance ({:.1}). Consider reviewing identity boundaries.", id, p.importance)
            } else if content_lower.contains("security") || content_lower.contains("安全") {
                format!(
                    "Security memory #{} (importance {:.1}) may need attention: {}",
                    id,
                    p.importance,
                    p.content.chars().take(80).collect::<String>()
                )
            } else if content_lower.contains("enforced") || content_lower.contains("rule") {
                format!(
                    "Enforced rule #{} (importance {:.1}): {}",
                    id,
                    p.importance,
                    p.content.chars().take(80).collect::<String>()
                )
            } else {
                format!(
                    "High-importance memory #{} ({:.1}) in warn category: {}",
                    id,
                    p.importance,
                    p.content.chars().take(80).collect::<String>()
                )
            };
            if !conflicts.is_empty() {
                desc.push_str(&format!(
                    " Known contradictory memory evidence: {:?}; review before acting.",
                    conflicts
                ));
            }

            signals.push(crate::engine::drive::DriveSignal {
                id: 0,
                timestamp: now,
                intent_type: crate::engine::drive::DriveIntent::Warn,
                description: desc,
                evidence: evidence.clone(),
                urgency: crate::engine::drive::DriveUrgency::High,
                target_capability: Some("conversation".into()),
                emotion: None,
                origin_tick: 0,
                status: crate::engine::drive::default_status(),
                feedback: None,
                retry_count: 0,
                expires_at: crate::engine::drive::default_expires_at(&crate::engine::drive::DriveUrgency::High),
                enqueued_at_ms: 0,
                time_budget_ms: None,
                grounding: self.grounding_for_evidence(
                    &evidence,
                    "Recent high-importance identity, security, enforced, or boundary memory within the existing 10-minute window.",
                    &conflicts,
                    Some(600),
                ),
                terminal_reason: None,
            });
        }

        // ── Category 2: SUGGEST — architecture/decision/bridge gaps ──
        let suggest_window = now_ts - 1800;
        let suggest_mems = self.gateway.list_by_labels_matching(
            &[
                "decision",
                "architecture",
                "bridge",
                "protocol",
                "will-seed",
                "will-expression",
                "charter",
                "core-directive",
            ],
            10,
            |id, memory| {
                memory.timestamp > suggest_window
                    && memory.timestamp <= now_ts
                    && (memory.labels.iter().any(|label| {
                        label == "will-seed"
                            || label == "will-expression"
                            || label == "charter"
                            || label == "core-directive"
                    }) || memory.importance >= 2.5)
                    && !pending_evidence.contains(&id)
                    && crate::engine::drive::memory_is_current(memory, now_ts)
                    && !memory.labels.iter().any(|label| {
                        label == "identity"
                            || label == "security"
                            || label == "l0-exempt"
                            || label == "quarantine"
                    })
                    && !crate::engine::drive::will_content_closed(&memory.content)
            },
        );
        // 防重播: 只建议最近30分钟内写入/修改的记忆。
        // 老记忆(如 charter/will-seed 永久记忆)会在信号被消费后脱离pending去重,
        // 曾导致 Architecture memory #2857 每40秒重发一次的无限循环。
        for (id, p) in suggest_mems.iter().take(3) {
            let conflicts = self.active_contradiction_ids(*id, now_ts);
            let mut evidence = vec![*id];
            evidence.extend(conflicts.iter().copied());
            evidence.sort_unstable();
            evidence.dedup();
            let content_lower = p.content.to_lowercase();
            let mut desc = if content_lower.contains("bridge") || content_lower.contains("桥") {
                format!(
                    "Bridge/integration memory #{} suggests an action item: {}",
                    id,
                    p.content.chars().take(80).collect::<String>()
                )
            } else if content_lower.contains("decision") || content_lower.contains("决策") {
                format!(
                    "Decision memory #{} may need follow-through: {}",
                    id,
                    p.content.chars().take(80).collect::<String>()
                )
            } else if content_lower.contains("protocol") || content_lower.contains("协议") {
                format!(
                    "Protocol-related memory #{} needs implementation: {}",
                    id,
                    p.content.chars().take(80).collect::<String>()
                )
            } else {
                format!(
                    "Architecture memory #{} (importance {:.1}) has actionable potential: {}",
                    id,
                    p.importance,
                    p.content.chars().take(80).collect::<String>()
                )
            };

            if !conflicts.is_empty() {
                desc = format!(
                    "Potential contradiction between current memories {:?}; withholding the action-oriented follow-up until the evidence is reviewed.",
                    evidence
                );
            }

            signals.push(crate::engine::drive::DriveSignal {
                id: 0,
                timestamp: now,
                intent_type: if conflicts.is_empty() {
                    crate::engine::drive::DriveIntent::Suggest
                } else {
                    crate::engine::drive::DriveIntent::Warn
                },
                description: desc,
                evidence: evidence.clone(),
                urgency: crate::engine::drive::DriveUrgency::Medium,
                target_capability: Some(if conflicts.is_empty() {
                    "code_review".into()
                } else {
                    "conversation".into()
                }),
                emotion: None,
                origin_tick: 0,
                status: crate::engine::drive::default_status(),
                feedback: None,
                retry_count: 0,
                expires_at: crate::engine::drive::default_expires_at(&crate::engine::drive::DriveUrgency::Medium),
                enqueued_at_ms: 0,
                time_budget_ms: None,
                grounding: self.grounding_for_evidence(
                    &evidence,
                    if conflicts.is_empty() {
                        "Recent decision, architecture, bridge, protocol, or directive memory matched the existing 30-minute follow-up gate."
                    } else {
                        "A current contradiction was found; the action-oriented follow-up was withheld."
                    },
                    &conflicts,
                    Some(1800),
                ),
                terminal_reason: None,
            });
        }

        // ── Category 3: EXPLORE — knowledge gaps (self-driving consumes these) ──
        let explore_window = now_ts - 3600;
        let explore_mems =
            self.gateway
                .list_by_labels_matching(&["knowledge-gap"], 5, |id, memory| {
                    memory.timestamp > explore_window
                        && memory.timestamp <= now_ts
                        && !pending_evidence.contains(&id)
                        && crate::engine::drive::memory_is_current(memory, now_ts)
                        && !crate::engine::drive::will_content_closed(&memory.content)
                        && !memory.labels.iter().any(|label| {
                            label == "identity"
                                || label == "security"
                                || label == "quarantine"
                                || label == "l0-exempt"
                        })
                });
        // 防重播: 只探索最近60分钟内产生的gap, 老gap已被探索多轮仍存说明非易解, 重复发信号只产生噪音
        let explore_candidates = explore_mems.iter().take(1).collect::<Vec<_>>();

        if !explore_candidates.is_empty() {
            let evidence: Vec<u64> = explore_candidates
                .iter()
                .take(3)
                .map(|(id, _)| *id)
                .collect();
            // 自我优化修复: 从 evidence 记忆中提取实际缺失的查询内容
            // 之前: "Knowledge gaps detected from N miss queries" (空壳,执行端不知道缺什么)
            // 现在: "知识缺口: 我不知道 '{query}' 相关的知识" (有实质内容)
            let gap_queries: Vec<String> = explore_candidates
                .iter()
                .take(2)
                .map(|(_, p)| {
                    p.content
                        .strip_prefix("[knowledge-gap] 待学习：")
                        .unwrap_or(&p.content)
                        .chars()
                        .take(80)
                        .collect::<String>()
                })
                .collect();
            let gap_source = explore_candidates[0];
            let gap_desc = format!(
                "Knowledge gap from memory #{} recorded at {}: search miss for '{}'. A miss means recall did not find support, not that no answer exists.",
                gap_source.0,
                gap_source.1.timestamp,
                gap_queries.join("' and '")
            );
            signals.push(crate::engine::drive::DriveSignal {
                id: 0,
                timestamp: now,
                intent_type: crate::engine::drive::DriveIntent::Explore,
                description: gap_desc,
                evidence: evidence.clone(),
                urgency: crate::engine::drive::DriveUrgency::Low,
                target_capability: Some("search".into()),
                emotion: None,
                origin_tick: 0,
                status: crate::engine::drive::default_status(),
                feedback: None,
                retry_count: 0,
                expires_at: crate::engine::drive::default_expires_at(&crate::engine::drive::DriveUrgency::Low),
                enqueued_at_ms: 0,
                time_budget_ms: None,
                grounding: self.grounding_for_evidence(
                    &evidence,
                    "Recent knowledge-gap memory created by an unanswered memory search within the existing 60-minute window.",
                    &[],
                    Some(3600),
                ),
                terminal_reason: None,
            });
        }

        // ── Category 4: BODY_MISSING — α0.2 出生流程入口 ──
        // 有身份 + 无 primary 执行体 → 温和提醒装配 (非安装包, 只递合同指引)
        // 冷却: 每 10 tick 最多 1 条; Low urgency; quiet discipline 由 TTL 承担
        {
            let has_primary = *self.runtime_has_primary.lock();
            let tick = self.tick_count.load(std::sync::atomic::Ordering::SeqCst);
            let last_bm = self
                .last_body_missing_tick
                .load(std::sync::atomic::Ordering::SeqCst);
            // 冷却: 距上次 body_missing >= 10 tick; last_bm==0 表示从未发过(首个 tick 即发)
            let cooldown_ok = last_bm == 0 || tick.saturating_sub(last_bm) >= 10;
            if !has_primary && cooldown_ok {
                self.last_body_missing_tick
                    .store(tick, std::sync::atomic::Ordering::SeqCst);
                let has_identity = self.space.identity_info().is_some();
                if has_identity {
                    signals.push(crate::engine::drive::DriveSignal {
                        id: 0, timestamp: now,
                        intent_type: crate::engine::drive::DriveIntent::Suggest,
                        description: "[body_missing] 检测到尚无 primary_executor 执行体, 意志无法落地。请向用户出示 GET /v1/runtime/manifest 返回的合同与 8 条风险卡, 等待用户明确说「允许装配」后, 按 executor-integration-playbook A0-A8 执行。禁止未经用户确认自行装配。".to_string(),
                        evidence: vec![],
                        urgency: crate::engine::drive::DriveUrgency::Low,
                        target_capability: Some("conversation".into()),
                        emotion: None, origin_tick: tick,
                        status: crate::engine::drive::default_status(), feedback: None,
                        retry_count: 0,
                        expires_at: crate::engine::drive::default_expires_at(&crate::engine::drive::DriveUrgency::Low),
                        enqueued_at_ms: 0, time_budget_ms: None,
                        grounding: None,
                        terminal_reason: None,
                    });
                }
            }
        }

        // Recheck the evidence after candidate selection to close the concurrent-update window.
        signals.retain(|s| {
            s.evidence.is_empty()
                || s.grounding.as_ref().is_some_and(|grounding| {
                    grounding.complete
                        && grounding.is_fresh_at(now)
                        && self.signal_evidence_is_current(s, now)
                })
        });

        signals
    }

    pub fn run_prediction_error_check(&self) {
        let signals = self.detect_prediction_errors();

        // Evidence-level throttle: don't emit if an unacked signal with the same
        // evidence fingerprint already exists. This prevents repeat alarms where
        // the same identity evidence (#763) gets a new drive_id every tick.
        //
        // Fingerprint = intent_type + sorted evidence IDs.
        // A signal is "unacked" if it's in Pending or Delivered status
        // (not yet Executed or Rejected).
        let mut enqueued = 0;
        let mut throttled = 0;
        let mut backpressured = 0;
        for signal in signals {
            match self.drive_queue.should_birth(
                &signal.intent_type,
                &signal.evidence,
                &signal.description,
            ) {
                Ok(()) => match self.drive_queue.try_enqueue(signal) {
                    Ok(_) => {
                        self.save_drive_queue();
                        enqueued += 1;
                    }
                    Err(error) => {
                        backpressured += 1;
                        tracing::warn!("[L0] signal admission backpressured: {:?}", error);
                    }
                },
                Err(why) => {
                    throttled += 1;
                    tracing::info!(
                        "[L0] birth valve: {} ev={:?} desc={:.60}",
                        why,
                        signal.evidence,
                        signal.description
                    );
                }
            }
        }

        if enqueued > 0 || throttled > 0 || backpressured > 0 {
            // P1-6: enqueue 后立刻持久化, 防止重启丢 signal
            tracing::info!(
                "[L0] prediction errors [user={}]: {} enqueued, {} throttled, {} backpressured",
                self.owner_user.lock().clone(),
                enqueued,
                throttled,
                backpressured
            );
        }
    }

    /// L0 Active Inference: Self-driving loop.
    ///
    /// When no external agent is connected to poll /v1/drive/inbox,
    /// Epicode uses its own cognitive engine as its "hand" to execute
    /// pending drive signals. This closes the evolution loop internally:
    ///
    ///   memory → prediction error → will → self-execute → feedback → evolution
    ///
    /// This is the personality acting on its own will — thinking to itself,
    /// "I'm curious about X. Let me reason about what I know and record my conclusion."
    pub(in crate::engine::scheduler) fn process_drive_signals(&self) {
        // Peek without changing status so non-Explore signals stay visible to external agents.
        let candidates = self.drive_queue.peek_pending_matching(3, |signal| {
            matches!(
                signal.intent_type,
                crate::engine::drive::DriveIntent::Explore
            )
        });
        let now = chrono::Utc::now().timestamp();
        let mut stale_ids = Vec::new();
        let signals: Vec<_> = candidates
            .into_iter()
            .filter(|signal| {
                if self.signal_evidence_is_current(signal, now) {
                    true
                } else {
                    stale_ids.push(signal.id);
                    false
                }
            })
            .collect();
        if self.drive_queue.expire_stale_evidence(&stale_ids) > 0 {
            self.save_drive_queue();
        }
        if signals.is_empty() {
            return;
        }

        tracing::info!(
            "[L0] processing {} drive signals (self-driving)",
            signals.len()
        );

        for signal in signals {
            match signal.intent_type {
                crate::engine::drive::DriveIntent::Explore => {
                    tracing::info!(
                        "[L0] self-explore: drive #{} | {}",
                        signal.id,
                        signal.description.chars().take(80).collect::<String>()
                    );

                    let question = signal
                        .evidence
                        .iter()
                        .find_map(|id| self.api_get_node(*id))
                        .map(|memory| {
                            memory
                                .content
                                .strip_prefix("[knowledge-gap] 待学习：")
                                .unwrap_or(&memory.content)
                                .trim()
                                .to_string()
                        })
                        .filter(|question| !question.is_empty())
                        .unwrap_or_else(|| signal.description.clone());
                    let excluded_ids: HashSet<u64> = signal.evidence.iter().copied().collect();
                    let recall = self.api_recall(&question, 2);
                    let mut evidence = match recall {
                        Ok(result) => recalled_drive_evidence(&result),
                        Err(error) => {
                            tracing::warn!("[L0] local explore recall failed: {}", error);
                            Vec::new()
                        }
                    };

                    evidence.retain_mut(|item| {
                        if excluded_ids.contains(&item.id) {
                            return false;
                        }
                        let Some(memory) = self.api_get_node(item.id) else {
                            return false;
                        };
                        if !crate::engine::drive::memory_is_current(&memory, now)
                            || memory.labels.iter().any(|label| {
                                label == "superseded"
                                    || label == "quarantine"
                                    || label == "l0-exempt"
                                    || label == "auto-generated"
                            })
                        {
                            return false;
                        }
                        item.content = memory.content;
                        item.timestamp = memory.timestamp;
                        true
                    });

                    let mut conflicts = HashSet::new();
                    for item in &evidence {
                        for (other_id, relation, _) in self.api_get_relations(item.id) {
                            if relation.eq_ignore_ascii_case("contradicts")
                                && self.api_get_node(other_id).is_some_and(|memory| {
                                    crate::engine::drive::memory_is_current(&memory, now)
                                })
                            {
                                conflicts.insert(item.id);
                                conflicts.insert(other_id);
                            }
                        }
                    }
                    let mut conflict_ids: Vec<u64> = conflicts.into_iter().collect();
                    conflict_ids.sort_unstable();
                    let explored = local_exploration_summary(&question, &evidence, &conflict_ids);

                    match explored {
                        Ok(conclusion) => {
                            let evidence_ids = evidence
                                .iter()
                                .map(|item| format!("#{}", item.id))
                                .collect::<Vec<_>>()
                                .join(", ");
                            let mem_content = format!(
                                "[self-driven exploration]\nQuestion: {}\nEvidence IDs: {}\n{}",
                                question.chars().take(200).collect::<String>(),
                                evidence_ids,
                                conclusion.chars().take(800).collect::<String>()
                            );
                            let labels = vec![
                                "self-driven".to_string(),
                                "exploration".to_string(),
                                "auto-generated".to_string(),
                                "l0-exempt".to_string(),
                            ];

                            match self.api_remember_with_labels(&mem_content, labels) {
                                Ok((id, _)) => {
                                    let _ = self.space.update_importance(id, 0.3);
                                    if let Some(t) = self.space.get_tetrahedron(id) {
                                        let _ = self.storage.upsert_tetra(&t);
                                    }
                                    tracing::info!(
                                        "[L0] self-explore complete: stored local source summary as memory #{} ({} chars)",
                                        id,
                                        conclusion.len()
                                    );
                                    let _ = self.drive_queue.mark_consumed(
                                        signal.id,
                                        crate::engine::drive::DriveFeedback {
                                            responded_at: chrono::Utc::now().timestamp(),
                                            executed: true,
                                            outcome: format!(
                                                "Stored local-memory summary as memory #{} with {} cited sources",
                                                id,
                                                evidence.len()
                                            ),
                                            reflection: Some(
                                                "Summary retained source IDs and explicitly states that it is not independent verification."
                                                    .to_string(),
                                            ),
                                        },
                                    );
                                    self.save_drive_queue();
                                }
                                Err(error) => {
                                    tracing::warn!("[L0] self-explore store failed: {}", error);
                                    let _ = self.drive_queue.mark_consumed(
                                        signal.id,
                                        crate::engine::drive::DriveFeedback {
                                            responded_at: chrono::Utc::now().timestamp(),
                                            executed: false,
                                            outcome: format!(
                                                "Failed to store local summary: {}",
                                                error
                                            ),
                                            reflection: None,
                                        },
                                    );
                                    self.save_drive_queue();
                                }
                            }
                        }
                        Err(reason) => {
                            let outcome = match reason {
                                ExploreBlockReason::NoCurrentEvidence => {
                                    "Insufficient current local-memory evidence; no answer was stored."
                                        .to_string()
                                }
                                ExploreBlockReason::ConflictingEvidence => format!(
                                    "Conflicting current memory evidence {:?}; no answer was stored.",
                                    conflict_ids
                                ),
                            };
                            tracing::info!("[L0] self-explore withheld: {}", outcome);
                            let _ = self.drive_queue.mark_consumed(
                                signal.id,
                                crate::engine::drive::DriveFeedback {
                                    responded_at: chrono::Utc::now().timestamp(),
                                    executed: false,
                                    outcome,
                                    reflection: Some(
                                        "Kept the source gap memory; the signal remains retryable only under the existing retry limit."
                                            .to_string(),
                                    ),
                                },
                            );
                            self.save_drive_queue();
                        }
                    }
                }
                crate::engine::drive::DriveIntent::Suggest => {
                    // The personality has a suggestion — log it for now.
                    // When an external agent connects, it will see suggestions in the inbox.
                    tracing::info!(
                        "[L0] suggestion pending (drive #{}): {}",
                        signal.id,
                        signal.description.chars().take(80).collect::<String>()
                    );
                }
                crate::engine::drive::DriveIntent::Warn => {
                    // Warning — log and store as enforced observation
                    tracing::info!(
                        "[L0] warning (drive #{}): {}",
                        signal.id,
                        signal.description.chars().take(80).collect::<String>()
                    );
                }
                _ => {
                    // Constrain, Request, Share — log for now
                    tracing::info!(
                        "[L0] drive #{} ({:?}): {}",
                        signal.id,
                        signal.intent_type,
                        signal.description.chars().take(60).collect::<String>()
                    );
                }
            }
        }
    }

    /// 智能化突破：认知状态快照——返回 emotion/drive/cognitive_status/latest_thought
    /// 供 SSE 推送和 /v1/cognitive/state API 使用
    /// L0: Get drive queue stats for SSE/snapshot
    pub fn drive_stats(&self) -> serde_json::Value {
        self.drive_queue.stats()
    }

    /// L0: Persist drive queue to SQLite (called with auto_save every 10 ticks).
    /// pub: ack 处理器在成功后立即调用 — ack 只改内存, 依赖周期保存时
    /// 重启会回滚 ack 状态(#105187 曾复活为 Pending)
    pub fn save_drive_queue(&self) {
        // Serialize snapshots and full-table writes so an older save cannot overwrite a newer one.
        let _save_guard = self.drive_save_gate.lock();
        // Keep terminal signals in memory and the active snapshot if archiving fails.
        if let Err(e) = self
            .drive_queue
            .archive_archivable(7 * 86400 * 1000, |signals| {
                self.storage.save_archived_signals(signals)
            })
        {
            tracing::warn!("[L0] drive signal archive failed: {}", e);
        }
        self.save_tick_state();
        let signals = self.drive_queue.snapshot();
        if let Err(e) = self.storage.save_drive_signals(&signals) {
            tracing::warn!("[L0] drive queue save failed: {}", e);
        }
        let ingested = self.drive_queue.ingested_ids();
        if let Ok(js) = serde_json::to_string(&ingested) {
            if let Err(e) = self.storage.save_drive_kv("ingested_ids", &js) {
                tracing::warn!("[L0] ingested save failed: {}", e);
            }
        }
        if let Ok(js) = serde_json::to_string(&self.drive_queue.policy_snapshot()) {
            if let Err(e) = self.storage.save_drive_kv("signal_policy", &js) {
                tracing::warn!("[L0] signal_policy save failed: {}", e);
            }
        }
        let _ = self.storage.save_drive_kv(
            "signal_policy_version",
            &self.drive_queue.policy_version().to_string(),
        );
    }
}
