use super::super::*;

impl McpHandler {
    pub(in crate::engine::mcp) fn tool_drive_inbox(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(50)
            .max(1) as usize;
        let e2e_public_key = self.engine.scheduler().e2e_pubkey();
        let signals: Vec<serde_json::Value> = self
            .engine
            .scheduler()
            .current_drive_inbox(limit)
            .iter()
            .map(|signal| signal.inbox_value_with_e2e(e2e_public_key.as_deref()))
            .collect();
        let stats = self.engine.scheduler().drive_queue().stats();
        let empty_reason = crate::engine::drive::inbox_empty_reason(signals.len(), &stats);
        self.smrp_ok(
            "drive_inbox",
            serde_json::json!({
                "signals": signals,
                "stats": stats,
                "empty_reason": empty_reason,
            }),
        )
    }

    pub(in crate::engine::mcp) fn tool_drive_ack(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let drive_id = match args.get("drive_id").and_then(|v| v.as_u64()) {
            Some(id) => id,
            None => return self.smrp_err("drive_ack", 400, "drive_id is required"),
        };
        let executed = args
            .get("executed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let outcome = args
            .get("outcome")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let reflection = args
            .get("reflection")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let signal_info = self.engine.scheduler().drive_queue().get_signal(drive_id);

        let feedback = crate::engine::drive::DriveFeedback {
            responded_at: chrono::Utc::now().timestamp(),
            executed,
            outcome: outcome.clone(),
            reflection,
        };
        let (success, first_ack) = self
            .engine
            .scheduler()
            .drive_queue()
            .acknowledge(drive_id, feedback);
        if success {
            self.engine.scheduler().save_drive_queue();
        }

        // Attribute outcome value only to the drive associated with this signal.
        if success && first_ack {
            if let Some((drive, multiplier)) = signal_info
                .as_ref()
                .and_then(|signal| signal.intent_type.feedback_target())
            {
                let sentiment =
                    crate::engine::drive::DriveFeedback::sentiment_for_outcome(&outcome);
                let reward = sentiment.reward() * multiplier;
                self.engine
                    .scheduler()
                    .drive_engine_lock()
                    .reward(drive, reward);
                tracing::info!(
                    "[L0] drive_ack reward: #{} intent={:?} executed={} reward={:+.3} sentiment={:?}",
                    drive_id,
                    signal_info.as_ref().map(|signal| &signal.intent_type),
                    executed,
                    reward,
                    sentiment
                );
            }
        }

        tracing::info!(
            "[L0] drive_ack via MCP: drive #{} executed={}",
            drive_id,
            executed
        );
        self.smrp_ok(
            "drive_ack",
            serde_json::json!({
                "drive_id": drive_id,
                "acknowledged": success,
                "first_ack": first_ack,
                "learned": success,
            }),
        )
    }

    /// Read-only: never runs a pass (no drive side effects). `run_now` is ignored
    /// for compatibility; use `action_planner_run` (write permission) instead.
    pub(in crate::engine::mcp) fn tool_action_planner_status(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let mut status = self.engine.scheduler.planner_pass().status();
        if args.get("run_now").and_then(|v| v.as_bool()) == Some(true) {
            status["note"] =
                serde_json::json!("run_now moved to action_planner_run (write permission)");
        }
        self.smrp_ok("action_planner_status", status)
    }

    pub(in crate::engine::mcp) fn tool_action_control(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        if let Some(e) =
            self.reject_unknown_args("action_control", args, &["action_id", "op", "note"])
        {
            return e;
        }
        let id = match args.get("action_id").and_then(|v| v.as_u64()) {
            Some(i) => i,
            None => return self.smrp_err("action_control", 400, "action_id is required"),
        };
        let op = args.get("op").and_then(|v| v.as_str()).unwrap_or("");
        let note = args.get("note").and_then(|v| v.as_str()).unwrap_or("");
        let frontier = self.engine.scheduler.action_frontier();
        let res = match op {
            "steer" => frontier.steer(id, note),
            "pause" => frontier.set_paused(id, true),
            "resume" => frontier.set_paused(id, false),
            "stop" => frontier.stop(id, note).inspect(|_| {
                // Dependents of stopped work cannot proceed.
                frontier.cascade_dep_failures();
            }),
            _ => return self.smrp_err("action_control", 400, "op must be steer|pause|resume|stop"),
        };
        match res {
            Ok(a) => {
                self.engine.scheduler.save_action_frontier();
                self.smrp_ok(
                    "action_control",
                    serde_json::json!({"op": op, "action": frontier.card(&a)}),
                )
            }
            Err(e) => {
                let code = if e.contains("not found") { 404 } else { 409 };
                self.smrp_err_or_conflict("action_control", code, &e)
            }
        }
    }

    pub(in crate::engine::mcp) fn tool_action_goal_create(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        if let Some(e) =
            self.reject_unknown_args("action_goal_create", args, &["title", "done_when"])
        {
            return e;
        }
        let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("");
        // A bare string is one criterion (was silently swallowed into []).
        let dw = match self.parse_str_list("action_goal_create", args, "done_when", true) {
            Ok(v) => v,
            Err(e) => return e,
        };
        match self
            .engine
            .scheduler
            .action_frontier()
            .create_goal(title, dw)
        {
            Ok(g) => {
                self.engine.scheduler.save_action_frontier();
                self.smrp_ok(
                    "action_goal_create",
                    serde_json::json!({"goal": g.to_json()}),
                )
            }
            Err(e) => self.smrp_err("action_goal_create", 400, &e),
        }
    }

    pub(in crate::engine::mcp) fn tool_action_goal_prove(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        if let Some(e) =
            self.reject_unknown_args("action_goal_prove", args, &["goal_id", "evidence"])
        {
            return e;
        }
        let gid = match args.get("goal_id").and_then(|v| v.as_u64()) {
            Some(g) => g,
            None => return self.smrp_err("action_goal_prove", 400, "goal_id is required"),
        };
        let ev: Vec<String> = args
            .get("evidence")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let frontier = self.engine.scheduler.action_frontier();
        match frontier.prove_goal(gid, ev) {
            Ok((g, cancelled)) => {
                // Dependents outside the goal that relied on cancelled work.
                let cascaded = frontier.cascade_dep_failures();
                self.engine.scheduler.save_action_frontier();
                self.smrp_ok(
                    "action_goal_prove",
                    serde_json::json!({"goal": g.to_json(), "cancelled": cancelled, "dep_failed_cascade": cascaded}),
                )
            }
            Err(e) => {
                let code = if e.contains("not found") { 404 } else { 409 };
                self.smrp_err_or_conflict("action_goal_prove", code, &e)
            }
        }
    }

    pub(in crate::engine::mcp) fn tool_action_goal_list(
        &self,
        _args: &serde_json::Value,
    ) -> serde_json::Value {
        let frontier = self.engine.scheduler.action_frontier();
        let actions = frontier.snapshot();
        let goals: Vec<serde_json::Value> = frontier
            .goals()
            .into_iter()
            .map(|g| {
                let mut counts = serde_json::Map::new();
                for a in actions.iter().filter(|a| a.goal_id == Some(g.id)) {
                    let k = a.state.as_str().to_string();
                    let n = counts.get(&k).and_then(|v| v.as_u64()).unwrap_or(0) + 1;
                    counts.insert(k, serde_json::json!(n));
                }
                let mut j = g.to_json();
                j["action_counts"] = serde_json::Value::Object(counts);
                j
            })
            .collect();
        self.smrp_ok("action_goal_list", serde_json::json!({"goals": goals}))
    }

    pub(in crate::engine::mcp) fn tool_action_planner_run(
        &self,
        _args: &serde_json::Value,
    ) -> serde_json::Value {
        let report = self.engine.scheduler.run_action_planner_pass_now();
        let mut status = self.engine.scheduler.planner_pass().status();
        status["report"] = serde_json::to_value(report).unwrap_or_default();
        self.smrp_ok("action_planner_run", status)
    }

    pub(in crate::engine::mcp) fn tool_action_list(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        if let Some(e) = self.reject_unknown_args("action_list", args, &["state", "limit"]) {
            return e;
        }
        let state = args
            .get("state")
            .and_then(|v| v.as_str())
            .and_then(crate::engine::agent_action::ActionState::parse);
        if let Some(_raw) = args.get("state").and_then(|v| v.as_str()) {
            if state.is_none() {
                return self.smrp_err(
                    "action_list",
                    400,
                    "state must be open|claimed|done|blocked|exhausted",
                );
            }
        }
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(20)
            .clamp(1, 100) as usize;
        let frontier = self.engine.scheduler.action_frontier();
        let actions: Vec<serde_json::Value> = frontier
            .list(state, limit)
            .into_iter()
            .map(|a| frontier.card(&a))
            .collect();
        self.smrp_ok(
            "action_list",
            serde_json::json!({
                "actions": actions,
                "open_count": self.engine.scheduler.action_frontier().open_count(),
                "stats": self.engine.scheduler.action_frontier().stats(),
            }),
        )
    }

    pub(in crate::engine::mcp) fn tool_action_heartbeat(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        if let Some(e) = self.reject_unknown_args(
            "action_heartbeat",
            args,
            &["action_id", "claimed_by", "worker_id"],
        ) {
            return e;
        }
        let action_id = match args.get("action_id").and_then(|v| v.as_u64()) {
            Some(id) => id,
            None => return self.smrp_err("action_heartbeat", 400, "action_id is required"),
        };
        let actor = match self.resolve_worker_label("action_heartbeat", args) {
            Ok(a) => a,
            Err(e) => return e,
        };
        let frontier = self.engine.scheduler.action_frontier();
        match frontier.heartbeat(action_id, &actor) {
            Ok((a, directives)) => {
                self.engine.scheduler.save_action_frontier();
                let mut js = a.to_json();
                js["lease_secs"] = serde_json::json!(frontier.lease_secs());
                js["directives"] = directives;
                self.smrp_ok("action_heartbeat", js)
            }
            Err(e) => {
                let code = if e.contains("ownership") { 403 } else { 409 };
                self.smrp_err_or_conflict("action_heartbeat", code, &e)
            }
        }
    }

    pub(in crate::engine::mcp) fn tool_action_claim(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        if let Some(e) = self.reject_unknown_args(
            "action_claim",
            args,
            &["action_id", "claimed_by", "worker_id"],
        ) {
            return e;
        }
        let action_id = match args.get("action_id").and_then(|v| v.as_u64()) {
            Some(id) => id,
            None => return self.smrp_err("action_claim", 400, "action_id is required"),
        };
        let claimed_by = match self.resolve_worker_label("action_claim", args) {
            Ok(a) => a,
            Err(e) => return e,
        };
        match self
            .engine
            .scheduler
            .action_frontier()
            .claim(action_id, &claimed_by)
        {
            Ok(a) => {
                self.engine.scheduler.save_action_frontier();
                self.smrp_ok("action_claim", a.to_json())
            }
            Err(e) => {
                let code = if e.contains("ownership") || e.contains("assigned to") {
                    403
                } else {
                    409
                };
                self.smrp_err_or_conflict("action_claim", code, &e)
            }
        }
    }

    pub(in crate::engine::mcp) fn tool_drive_to_actions(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let drive_id = match args.get("drive_id").and_then(|v| v.as_u64()) {
            Some(id) => id,
            None => return self.smrp_err("drive_to_actions", 400, "drive_id is required"),
        };
        let claim_now = args.get("claim").and_then(|v| v.as_bool()).unwrap_or(false);

        let signal = match self.engine.scheduler().drive_queue().get_signal(drive_id) {
            Some(s) => s,
            None => {
                return self.smrp_err(
                    "drive_to_actions",
                    404,
                    &format!("drive signal {} not found", drive_id),
                )
            }
        };

        let mut evidence_ids: Vec<u64> = signal.evidence.clone();
        if evidence_ids.is_empty() {
            if let Some(g) = &signal.grounding {
                evidence_ids = g.evidence.iter().map(|e| e.id).collect();
            }
        }
        evidence_ids.sort_unstable();
        evidence_ids.dedup();
        evidence_ids.retain(|id| self.engine.space.get_tetrahedron(*id).is_some());
        if evidence_ids.is_empty() {
            return self.smrp_err(
                "drive_to_actions",
                400,
                "drive has no grounded persistent-memory evidence — cannot materialize action",
            );
        }

        let frontier = self.engine.scheduler.action_frontier();
        if frontier.has_live_drive(drive_id) {
            let existing = frontier.find_by_drive(drive_id);
            return self.smrp_conflict("drive_to_actions", &format!(
                    "drive {} already has a live action {} — complete or block it first (no double-spend)",
                    drive_id,
                    existing.first().map(|a| a.id).unwrap_or(0)
                ),
            );
        }

        let skills: Vec<(String, f64)> = self
            .engine
            .skills
            .match_skills_scored(&signal.description, &self.engine.user_id, 5)
            .into_iter()
            .map(|(sk, score)| (sk.name, score))
            .collect();

        let draft = match crate::engine::agent_action::build_drive_draft(
            drive_id,
            &signal.description,
            &evidence_ids,
            &skills,
        ) {
            Some(d) => d,
            None => return self.smrp_err("drive_to_actions", 400, "failed to build drive draft"),
        };

        let mut action =
            match frontier.enqueue(draft.into_drive_action(drive_id, &signal.description)) {
                Ok(a) => a,
                Err(e) => return self.smrp_conflict("drive_to_actions", &e),
            };

        if claim_now {
            match frontier.claim(action.id, &self.engine.user_id) {
                Ok(a) => action = a,
                Err(e) => {
                    return self.smrp_conflict(
                        "drive_to_actions",
                        &format!("enqueued but claim failed: {e}"),
                    )
                }
            }
        }

        self.engine.scheduler.save_action_frontier();
        self.smrp_ok(
            "drive_to_actions",
            serde_json::json!({
                "action": action.to_json(),
                "drive_id": drive_id,
                "evidence_memory_ids": evidence_ids,
                "claimed": claim_now,
                "loop": ["drive_inbox", "drive_to_actions", "action_claim", "execute", "action_complete→drive_ack"],
            }),
        )
    }

    pub(in crate::engine::mcp) fn is_admin_actor(&self) -> bool {
        // Only explicit Admin role may override ownership. Ungated primary accounts
        // still act as their user_id — they must be the assignee.
        matches!(
            self.role_gate,
            Some(crate::engine::user_manager::UserRole::Admin)
        )
    }

    pub(in crate::engine::mcp) fn tool_worker_register(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        if let Some(e) = self.reject_unknown_args(
            "worker_register",
            args,
            &["worker_id", "capabilities", "max_load"],
        ) {
            return e;
        }
        let worker_id = match args.get("worker_id").and_then(|v| v.as_str()) {
            Some(s) if !s.trim().is_empty() => s.trim(),
            _ => return self.smrp_err("worker_register", 400, "worker_id is required"),
        };
        let caps: Vec<String> = args
            .get("capabilities")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(|s| s.trim().to_string()))
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let max_load = args
            .get("max_load")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32);
        match self
            .engine
            .scheduler
            .worker_registry()
            .register(worker_id, caps, max_load)
        {
            Ok(rec) => {
                self.engine.scheduler.save_worker_registry();
                self.smrp_ok(
                    "worker_register",
                    serde_json::json!({
                        "worker": {
                            "worker_id": rec.worker_id,
                            "capabilities": rec.capabilities,
                            "max_load": rec.max_load,
                            "blacklisted_until": rec.blacklisted_until,
                            "registered_at": rec.registered_at,
                            "updated_at": rec.updated_at,
                        }
                    }),
                )
            }
            Err(e) => self.smrp_err("worker_register", 400, &e),
        }
    }

    pub(in crate::engine::mcp) fn tool_worker_unregister(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        const T: &str = "worker_unregister";
        if let Some(e) = self.reject_unknown_args(T, args, &["worker_id", "force"]) {
            return e;
        }
        let worker_id = match args.get("worker_id").and_then(|v| v.as_str()) {
            Some(s) if !s.trim().is_empty() => s.trim().to_string(),
            _ => return self.smrp_err(T, 400, "worker_id is required"),
        };
        // Owner (ungated primary account) or explicit Admin only.
        let allowed = self.role_gate.is_none() || self.is_admin_actor();
        if !allowed {
            return self.smrp_err(
                T,
                403,
                "worker_unregister requires the account owner or Admin",
            );
        }
        let force = args.get("force").and_then(|v| v.as_bool()).unwrap_or(false);
        let frontier = self.engine.scheduler.action_frontier();
        let load = crate::engine::dispatch::WorkerRegistry::load_of(frontier, &worker_id);
        if load > 0 && !force {
            return self.smrp_conflict(
                T,
                &format!(
                    "worker {worker_id} holds {load} live claim(s); finish them or pass force=true"
                ),
            );
        }
        match self
            .engine
            .scheduler
            .worker_registry()
            .unregister(&worker_id)
        {
            Some(_) => {
                self.engine.scheduler.save_worker_registry();
                self.smrp_ok(T, serde_json::json!({"worker_id": worker_id, "removed": true, "live_claims": load}))
            }
            None => self.smrp_err(T, 404, &format!("worker {worker_id} not found")),
        }
    }

    pub(in crate::engine::mcp) fn tool_worker_list(
        &self,
        _args: &serde_json::Value,
    ) -> serde_json::Value {
        let frontier = self.engine.scheduler.action_frontier();
        let workers: Vec<serde_json::Value> = self
            .engine
            .scheduler
            .worker_registry()
            .list()
            .into_iter()
            .map(|w| {
                let load = crate::engine::dispatch::WorkerRegistry::load_of(frontier, &w.worker_id);
                serde_json::json!({
                    "worker_id": w.worker_id,
                    "capabilities": w.capabilities,
                    "max_load": w.max_load,
                    "current_load": load,
                    "blacklisted_until": w.blacklisted_until,
                    "registered_at": w.registered_at,
                    "updated_at": w.updated_at,
                })
            })
            .collect();
        self.smrp_ok(
            "worker_list",
            serde_json::json!({ "workers": workers, "count": workers.len() }),
        )
    }

    pub(in crate::engine::mcp) fn tool_action_assign(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        if let Some(e) =
            self.reject_unknown_args("action_assign", args, &["action_id", "worker_id"])
        {
            return e;
        }
        let action_id = match args.get("action_id").and_then(|v| v.as_u64()) {
            Some(id) => id,
            None => return self.smrp_err("action_assign", 400, "action_id is required"),
        };
        let frontier = self.engine.scheduler.action_frontier();
        let registry = self.engine.scheduler.worker_registry();
        let result = match args.get("worker_id").and_then(|v| v.as_str()) {
            Some(wid) if !wid.trim().is_empty() => frontier
                .assign(action_id, wid.trim())
                .map(|a| (a, wid.trim().to_string())),
            _ => crate::engine::dispatch::auto_assign(registry, frontier, action_id)
                .map(|(a, w)| (a, w.worker_id)),
        };
        match result {
            Ok((action, worker_id)) => {
                self.engine.scheduler.save_action_frontier();
                self.engine.scheduler.save_worker_registry();
                self.smrp_ok(
                    "action_assign",
                    serde_json::json!({
                        "action": action.to_json(),
                        "worker_id": worker_id,
                        "assignee": action.assignee,
                        "assigned_at": action.assigned_at,
                    }),
                )
            }
            Err(e) => {
                let code = if e.contains("ownership") || e.contains("assigned to") {
                    403
                } else if e.contains("not found") {
                    404
                } else {
                    409
                };
                self.smrp_err_or_conflict("action_assign", code, &e)
            }
        }
    }

    pub(in crate::engine::mcp) fn tool_action_dispatch(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        if let Some(e) = self.reject_unknown_args("action_dispatch", args, &["limit"]) {
            return e;
        }
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(5)
            .clamp(1, 20) as usize;
        let frontier = self.engine.scheduler.action_frontier();
        let registry = self.engine.scheduler.worker_registry();
        let results = crate::engine::dispatch::dispatch_batch(registry, frontier, limit);
        let assigned = results
            .iter()
            .filter(|r| r.get("ok") == Some(&serde_json::json!(true)))
            .count();
        self.engine.scheduler.save_action_frontier();
        self.engine.scheduler.save_worker_registry();
        self.smrp_ok(
            "action_dispatch",
            serde_json::json!({
                "results": results,
                "assigned": assigned,
                "limit": limit,
            }),
        )
    }

    pub(in crate::engine::mcp) fn tool_action_complete(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        if let Some(e) = self.reject_unknown_args(
            "action_complete",
            args,
            &[
                "action_id",
                "outcome",
                "done_when_evidence",
                "claimed_by",
                "worker_id",
                "failure_kind",
                "success",
                "labels",
                "state",
            ],
        ) {
            return e;
        }
        let action_id = match args.get("action_id").and_then(|v| v.as_u64()) {
            Some(id) => id,
            None => return self.smrp_err("action_complete", 400, "action_id is required"),
        };
        let outcome = args
            .get("outcome")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if outcome.is_empty() {
            return self.smrp_err(
                "action_complete",
                400,
                "outcome is required — structured write-back cannot be empty",
            );
        }
        // Schema errors are 400 and never touch state / requeue budget.
        let dw_evidence =
            match self.parse_str_list("action_complete", args, "done_when_evidence", false) {
                Ok(v) => v,
                Err(e) => return e,
            };
        if let Err(e) = self.parse_str_list("action_complete", args, "labels", false) {
            return e;
        }
        if let Some(v) = args.get("success") {
            if !v.is_null() && !v.is_boolean() {
                return self.smrp_err(
                    "action_complete",
                    400,
                    "invalid params: success must be a boolean",
                );
            }
        }
        for k in ["state", "failure_kind"] {
            if let Some(v) = args.get(k) {
                if !v.is_null() && !v.is_string() {
                    return self.smrp_err(
                        "action_complete",
                        400,
                        &format!("invalid params: {k} must be a string"),
                    );
                }
            }
        }
        let actor = match self.resolve_worker_label("action_complete", args) {
            Ok(a) => a,
            Err(e) => return e,
        };
        let frontier = self.engine.scheduler.action_frontier();
        let action = match frontier.get(action_id) {
            Some(a) => a,
            None => {
                return self.smrp_err(
                    "action_complete",
                    404,
                    &format!("action {} not found", action_id),
                )
            }
        };
        if action.state.is_terminal() {
            return self.smrp_conflict(
                "action_complete",
                &format!(
                    "action {} already terminal ({})",
                    action_id,
                    action.state.as_str()
                ),
            );
        }
        if action.paused {
            return self.smrp_conflict(
                "action_complete",
                &format!("action {} is paused — resume before completing", action_id),
            );
        }

        let success = args
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let terminal = match args.get("state").and_then(|v| v.as_str()) {
            Some(s) => match crate::engine::agent_action::ActionState::parse(s) {
                Some(st) if st.is_terminal() => st,
                _ => {
                    return self.smrp_err(
                        "action_complete",
                        400,
                        "state must be done|blocked|exhausted",
                    )
                }
            },
            None => {
                if success {
                    crate::engine::agent_action::ActionState::Done
                } else {
                    crate::engine::agent_action::ActionState::Blocked
                }
            }
        };

        // PR-J: optional failure classification for non-done outcomes.
        let failure_kind = match args.get("failure_kind").and_then(|v| v.as_str()) {
            Some(k) => match crate::engine::agent_action::FailureKind::parse(k) {
                Some(fk) => Some(fk),
                None => {
                    return self.smrp_err(
                        "action_complete",
                        400,
                        "failure_kind must be transient|exhausted|blocked",
                    )
                }
            },
            None => None,
        };
        let terminal = match failure_kind {
            Some(crate::engine::agent_action::FailureKind::Transient)
            | Some(crate::engine::agent_action::FailureKind::Blocked)
            | Some(crate::engine::agent_action::FailureKind::LeaseExpired) => {
                crate::engine::agent_action::ActionState::Blocked
            }
            Some(crate::engine::agent_action::FailureKind::Exhausted) => {
                crate::engine::agent_action::ActionState::Exhausted
            }
            None => terminal,
        };
        let goal_debts =
            crate::engine::agent_action::prove_done_when(&action.done_when, &dw_evidence);
        let mut terminal = terminal;
        // Soft-fail: requesting done without full done_when proof → blocked.
        if !goal_debts.is_empty()
            && matches!(terminal, crate::engine::agent_action::ActionState::Done)
        {
            terminal = crate::engine::agent_action::ActionState::Blocked;
        }

        // Ownership gate BEFORE writing outcome memory (no leak on foreign complete).
        {
            let owner = action.assignee.as_deref().or(action.claimed_by.as_deref());
            if let Some(owner) = owner {
                if owner != actor && !self.is_admin_actor() {
                    return self.smrp_err(
                        "action_complete",
                        403,
                        &format!(
                            "action {} owned by assignee '{}'; '{}' cannot complete (ownership)",
                            action_id, owner, actor
                        ),
                    );
                }
            }
        }

        let mut labels = vec![
            "action-outcome".to_string(),
            format!("action:{}", action_id),
        ];
        if let Some(a) = action.assignee.as_ref().or(action.claimed_by.as_ref()) {
            labels.push(format!("assignee:{}", a));
            labels.push(format!("source_agent:{}", a));
        }
        if let Some(extra) = args.get("labels").and_then(|v| v.as_array()) {
            for v in extra {
                if let Some(s) = v.as_str() {
                    let t = s.trim();
                    if !t.is_empty() && !labels.iter().any(|x| x == t) {
                        labels.push(t.to_string());
                    }
                }
            }
        }

        let content = format!(
            "[action-outcome #{}] {}\n\nEvidence: {:?}\nSource query: {}",
            action_id,
            outcome,
            action.evidence_memory_ids,
            action.source_query.as_deref().unwrap_or("")
        );

        let report = match self
            .engine
            .scheduler
            .api_create_memory_full(&content, labels)
        {
            Ok(r) => r,
            Err(e) => return self.smrp_err("action_complete", 500, &e),
        };
        let outcome_id = report.id;

        let kg = self.engine.scheduler.kg_handle();
        let mut linked = 0usize;
        for &eid in &action.evidence_memory_ids {
            if self.engine.space.get_tetrahedron(eid).is_some() {
                kg.add_relation(
                    eid,
                    outcome_id,
                    crate::engine::knowledge::RelationType::Precedes,
                    0.85,
                );
                linked += 1;
            }
        }

        let is_admin = self.is_admin_actor();
        #[cfg(test)]
        run_before_complete_transition_hook();
        match frontier.complete_as(action_id, outcome_id, terminal, &actor, is_admin) {
            Ok(done) => {
                let mut done = done;
                let mut accountability = serde_json::Value::Null;
                let mut goal_closed = false;
                // Log Blocked at the moment complete_as lands it — before
                // account_failure_kind may requeue back to Open.
                if done.state == crate::engine::agent_action::ActionState::Blocked
                    || matches!(terminal, crate::engine::agent_action::ActionState::Blocked)
                {
                    let task_for_action = done.bound_task_id.clone();
                    let recall_ids = task_for_action
                        .as_deref()
                        .map(|t| self.engine.storage.recent_recall_ids(t, 5))
                        .unwrap_or_default();
                    self.log_outcome_event(
                        "action_blocked",
                        task_for_action.as_deref(),
                        &recall_ids,
                        &[],
                        &[],
                        &serde_json::json!({
                            "action_id": action_id,
                            "failure_kind": failure_kind.map(|k| k.as_str()),
                            "requeue_count": done.requeue_count,
                            "debts": goal_debts,
                            "state": "blocked",
                        }),
                    );
                }
                if done.state == crate::engine::agent_action::ActionState::Done {
                    if let Some(g) = done.goal_id {
                        goal_closed = frontier.maybe_close_goal(g);
                    }
                }
                if matches!(
                    done.state,
                    crate::engine::agent_action::ActionState::Blocked
                        | crate::engine::agent_action::ActionState::Exhausted
                ) && !done.escalated
                {
                    let reason = done
                        .last_failure_reason
                        .clone()
                        .unwrap_or_else(|| format!("{:?}", done.state).to_lowercase());
                    let kind = failure_kind.unwrap_or(match done.state {
                        crate::engine::agent_action::ActionState::Exhausted => {
                            crate::engine::agent_action::FailureKind::Exhausted
                        }
                        _ => crate::engine::agent_action::FailureKind::Blocked,
                    });
                    match crate::engine::dispatch::account_failure_kind(
                        self.engine.scheduler.worker_registry(),
                        frontier,
                        action_id,
                        kind,
                        &reason,
                    ) {
                        Ok((a, disposition)) => {
                            done = a;
                            if disposition == "escalated" {
                                self.engine.scheduler.notify_action_escalated(
                                    action_id,
                                    done.last_failure_reason.as_deref().unwrap_or(""),
                                );
                            }
                            accountability = serde_json::json!({
                                "disposition": disposition,
                                "failure_kind": done.last_failure_kind.map(|k| k.as_str()),
                                "transient_retries": done.transient_retries,
                                "not_before": done.not_before,
                                "requeue_count": done.requeue_count,
                                "escalated": done.escalated,
                                "last_responsible_assignee": done.last_responsible_assignee,
                            });
                        }
                        Err(e) => {
                            accountability = serde_json::json!({ "error": e });
                        }
                    }
                }
                self.engine.scheduler.save_action_frontier();
                self.engine
                    .bus
                    .publish(crate::engine::bus::EngineEvent::ActionOutcomeLanded {
                        action_id,
                        outcome_memory_id: outcome_id,
                        state: done.state.as_str().to_string(),
                    });
                // Drive→Action: first successful complete acks the source drive (no double-spend of learning).
                let mut drive_ack = serde_json::Value::Null;
                // Only a landed Done acks the drive. A soft-failed complete is requeued
                // (#255) and stays live; acking it would consume the drive while the
                // action is still being reworked.
                let landed_done = done.state == crate::engine::agent_action::ActionState::Done;
                if let (Some(did), true) = (done.source_drive_id, landed_done) {
                    let feedback = crate::engine::drive::DriveFeedback {
                        responded_at: chrono::Utc::now().timestamp(),
                        executed: matches!(
                            terminal,
                            crate::engine::agent_action::ActionState::Done
                        ),
                        outcome: outcome.to_string(),
                        reflection: Some(format!("via action_complete #{}", action_id)),
                    };
                    let (acked, first) = self
                        .engine
                        .scheduler()
                        .drive_queue()
                        .acknowledge(did, feedback);
                    if acked {
                        self.engine.scheduler().save_drive_queue();
                    }
                    drive_ack = serde_json::json!({
                        "drive_id": did,
                        "acknowledged": acked,
                        "first_ack": first,
                    });
                }
                self.smrp_ok(
                    "action_complete",
                    serde_json::json!({
                        "action": done.to_json(),
                        "outcome_memory_id": outcome_id,
                        "relations_created": linked,
                        "drive_ack": drive_ack,
                        "goal_closed": goal_closed,
                        "done_when_debts": goal_debts.clone(),
                        "done_when_proven": goal_debts.is_empty(),
                        "accountability": accountability,
                    }),
                )
            }
            Err(e) => {
                // The outcome memory was written before the frontier transition;
                // if the transition lost (cancelled/terminal in between), remove it
                // so a late/raced outcome never pollutes memory.
                // Hard purge (not soft forget): a forgotten tombstone is still a memory.
                let _ = self.engine.scheduler.api_purge_memory(outcome_id);
                let code = if e.contains("ownership") { 403 } else { 409 };
                self.smrp_err_or_conflict("action_complete", code, &e)
            }
        }
    }
}
