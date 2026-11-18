use std::io::{self, BufRead};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::domain::tetra::MemoryPayload;
use crate::engine::Engine;

pub const MAX_MCP_REQUEST_BYTES: usize = 1024 * 1024;
pub const MAX_MCP_RESPONSE_BYTES: usize = 1024 * 1024;

pub fn read_mcp_line<R: BufRead>(reader: &mut R, line: &mut Vec<u8>) -> io::Result<usize> {
    line.clear();
    loop {
        let (consumed, has_newline) = {
            let available = reader.fill_buf()?;
            if available.is_empty() {
                return Ok(line.len());
            }
            let newline = available.iter().position(|byte| *byte == b'\n');
            let content_len = newline.unwrap_or(available.len());
            if line.len().saturating_add(content_len) > MAX_MCP_REQUEST_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "MCP request exceeds the 1 MiB limit",
                ));
            }
            let consumed = newline.map(|index| index + 1).unwrap_or(available.len());
            line.extend_from_slice(&available[..consumed]);
            (consumed, newline.is_some())
        };
        reader.consume(consumed);
        if has_newline {
            return Ok(line.len());
        }
    }
}

fn truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// 查询哈希(REST 埋点与 MCP 共用, recall_log 隐私约定: 默认只存 hash)。
pub fn sha256_query_hex(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct McpRequest {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct McpResponse {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpError>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct McpError {
    pub code: i64,
    pub message: String,
}

/// 配额上下文:写入工具调用前检查内存配额(与 REST 对等)。
pub struct QuotaContext {
    pub user_mgr: Arc<super::user_manager::UserManager>,
    pub user_id: String,
}

pub struct McpHandler {
    engine: Arc<Engine>,
    pub_skills: Option<Arc<super::skills::SkillEngine>>,
    /// 配额上下文:None 时跳过配额检查(本地 MCP / mcp_server 二进制)。
    quota: Option<QuotaContext>,
    /// A01(审计#134): 子账户角色门 — None=主账户/本地全权; Some(role)时
    /// tools_call 按工具→权限映射拒无权调用。HTTP/TCP入口注入。
    role_gate: Option<crate::engine::user_manager::UserRole>,
    /// Explicit grant set. When set, it overrides role.can for tool checks.
    custom_permissions: Option<Vec<String>>,
}

mod annotations;
mod catalog;
mod dispatch;
mod handlers;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod input_validation_tests;

mod recall_self_tests;

/// Catalog consistency contract: what Epicode ADVERTISES (tools/list, discovery
/// manifest, public tool count, marketing copy) must equal what it can actually
/// DO (dispatch arms, permission gate, annotations, handler params).
#[cfg(test)]
mod catalog_consistency;

#[cfg(test)]
mod recall_outcome_log_mcp_tests;

#[cfg(test)]
mod ask_assoc_tests;
/// Manual-rot guard: executor-facing manuals may only reference tools, tool
/// parameters and HTTP endpoints that exist today, the canonical manual must
/// cover the whole catalog, carry the build version, and be what the handshake
/// serves. Historical documents (with the "Historical document" banner) are
/// exempt; everything an executor is pointed at is not.
#[cfg(test)]
mod manual_rot;

impl McpHandler {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self {
            engine,
            pub_skills: None,
            quota: None,
            role_gate: None,
            custom_permissions: None,
        }
    }

    /// 写入工具配额检查(与 REST check_and_increment_memory 对等)。
    /// 成功返回 Ok,超配额返回错误信封。
    fn check_quota(&self, tool: &str) -> Result<(), serde_json::Value> {
        if let Some(q) = &self.quota {
            if let Err(e) = q.user_mgr.check_and_increment_memory(&q.user_id) {
                return Err(self.smrp_err(tool, 403, &e));
            }
        }
        Ok(())
    }

    /// dedup 命中时回滚配额(与 REST decrement_memory_count 对等)。
    fn rollback_quota(&self, is_new: bool) {
        if !is_new {
            if let Some(q) = &self.quota {
                q.user_mgr.decrement_memory_count(&q.user_id, 1);
            }
        }
    }

    /// 成功信封。
    fn smrp_ok(&self, tool: &str, data: serde_json::Value) -> serde_json::Value {
        super::smrp::envelope_ok(&self.engine, tool, data)
    }

    /// 错误信封（SMRP §4.1：失败也走信封，ok=false）。
    fn smrp_err(&self, tool: &str, code: i64, msg: &str) -> serde_json::Value {
        super::smrp::envelope_err(&self.engine, tool, code, msg)
    }

    /// 409 Conflict helper that also appends an `http_409` outcome_event (design-draft §b).
    fn smrp_conflict(&self, tool: &str, msg: &str) -> serde_json::Value {
        self.log_outcome_event(
            "http_409",
            None,
            &[],
            &[],
            &[],
            &serde_json::json!({"tool": tool, "msg": msg}),
        );
        self.smrp_err(tool, 409, msg)
    }

    /// Route 409 through `smrp_conflict` (outcome log); other codes stay as plain errors.
    fn smrp_err_or_conflict(&self, tool: &str, code: i64, msg: &str) -> serde_json::Value {
        if code == 409 {
            self.smrp_conflict(tool, msg)
        } else {
            self.smrp_err(tool, code, msg)
        }
    }

    fn sha256_hex(text: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(text.as_bytes());
        format!("{:x}", hasher.finalize())
    }
    /// Collect returned memory ids + per-item sources from recall/search/ask data.
    fn extract_returned_and_sources(data: &serde_json::Value) -> (Vec<u64>, Vec<Vec<String>>) {
        let mut items: Vec<&serde_json::Value> = Vec::new();
        if let Some(arr) = data.get("items").and_then(|v| v.as_array()) {
            items.extend(arr.iter());
        } else if let Some(arr) = data.get("results").and_then(|v| v.as_array()) {
            items.extend(arr.iter());
        } else if let Some(arr) = data.get("memories").and_then(|v| v.as_array()) {
            items.extend(arr.iter());
        } else if let Some(tiers) = data.get("tiers").and_then(|v| v.as_object()) {
            for key in ["primary", "hub", "experiential", "contextual"] {
                if let Some(arr) = tiers.get(key).and_then(|v| v.as_array()) {
                    items.extend(arr.iter());
                }
            }
        } else if let Some(sections) = data.get("sections").and_then(|v| v.as_object()) {
            for arr in sections.values() {
                if let Some(frags) = arr.as_array() {
                    items.extend(frags.iter());
                }
            }
        }
        let mut returned = Vec::new();
        let mut sources = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for item in items {
            let Some(id) = item.get("id").and_then(|v| v.as_u64()) else {
                continue;
            };
            if !seen.insert(id) {
                continue;
            }
            returned.push(id);
            let src: Vec<String> = match item.get("source") {
                Some(serde_json::Value::Array(a)) => a
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect(),
                Some(serde_json::Value::String(s)) => vec![s.clone()],
                _ => Vec::new(),
            };
            sources.push(src);
        }
        (returned, sources)
    }

    /// Append-only recall log + inject `recall_id` into `data` when the flag is on.
    fn attach_recall_log(
        &self,
        tool: &str,
        query: &str,
        data: &mut serde_json::Value,
        attempt: i64,
        stage: Option<&str>,
    ) {
        if !super::spike_flags::recall_outcome_log() {
            return;
        }
        let (returned, sources) = Self::extract_returned_and_sources(data);
        let task_id = self
            .engine
            .storage
            .get_active_task_for_user(&self.engine.user_id);
        // S1 基准埋点(#369): 明文查询(锚词解析需要)仅在 env 显式开启时落库
        let query_text = if super::spike_flags::env_flag("EPICODE_QUERY_TEXT_LOG") {
            Some(query)
        } else {
            None
        };
        let rec = super::storage::RecallLogRecord {
            user_id: &self.engine.user_id,
            task_id: task_id.as_deref(),
            tool,
            query_hash: &Self::sha256_hex(query),
            query_text,
            attempt,
            stage,
            returned: &returned,
            sources: &sources,
            reserved: &[],
        };
        match self.engine.storage.log_recall(&rec) {
            Ok(rid) => {
                if let Some(obj) = data.as_object_mut() {
                    obj.insert("recall_id".into(), serde_json::json!(rid));
                }
            }
            Err(e) => tracing::warn!("[recall_log] log_recall failed: {}", e),
        }
    }

    fn log_outcome_event(
        &self,
        kind: &str,
        task_id: Option<&str>,
        recall_ids: &[String],
        surfaced: &[u64],
        adopted: &[u64],
        payload: &serde_json::Value,
    ) {
        if !super::spike_flags::recall_outcome_log() {
            return;
        }
        if let Err(e) = self.engine.storage.log_outcome(
            &self.engine.user_id,
            kind,
            task_id,
            recall_ids,
            surfaced,
            adopted,
            payload,
        ) {
            tracing::warn!("[outcome_event] log_outcome({}) failed: {}", kind, e);
        }
    }

    /// Strict param check for action/worker tools: unknown top-level keys are a
    /// 400 (schema error) so typos like `evidence` / `worker` never silently
    /// change semantics or consume requeue budget.
    fn reject_unknown_args(
        &self,
        tool: &str,
        args: &serde_json::Value,
        allowed: &[&str],
    ) -> Option<serde_json::Value> {
        let obj = match args {
            serde_json::Value::Null => return None,
            serde_json::Value::Object(o) => o,
            _ => {
                return Some(self.smrp_err(
                    tool,
                    400,
                    "invalid params: arguments must be an object",
                ))
            }
        };
        let mut unknown: Vec<&str> = obj
            .keys()
            .map(|k| k.as_str())
            .filter(|k| !allowed.contains(k))
            .collect();
        if unknown.is_empty() {
            return None;
        }
        unknown.sort_unstable();
        Some(self.smrp_err(
            tool,
            400,
            &format!(
                "invalid params: unknown field(s) {} (allowed: {})",
                unknown.join(", "),
                allowed.join(", ")
            ),
        ))
    }

    /// Resolve the worker label: `claimed_by`, with `worker_id` accepted as an
    /// alias. Both present and different → 400. Neither → engine user.
    fn resolve_worker_label(
        &self,
        tool: &str,
        args: &serde_json::Value,
    ) -> Result<String, serde_json::Value> {
        let pick = |k: &str| -> Result<Option<String>, serde_json::Value> {
            match args.get(k) {
                None | Some(serde_json::Value::Null) => Ok(None),
                Some(serde_json::Value::String(s)) => {
                    let t = s.trim();
                    Ok(if t.is_empty() {
                        None
                    } else {
                        Some(t.to_string())
                    })
                }
                Some(_) => {
                    Err(self.smrp_err(tool, 400, &format!("invalid params: {k} must be a string")))
                }
            }
        };
        let cb = pick("claimed_by")?;
        let wid = pick("worker_id")?;
        match (cb, wid) {
            (Some(a), Some(b)) if a != b => Err(self.smrp_err(
                tool,
                400,
                "invalid params: claimed_by and worker_id disagree (worker_id is an alias of claimed_by)",
            )),
            (Some(a), _) | (None, Some(a)) => Ok(a),
            (None, None) => Ok(self.engine.user_id.clone()),
        }
    }

    /// Parse an optional array-of-strings param. Non-array / non-string items → 400.
    /// `allow_scalar`: a bare string is accepted as a single-item list.
    fn parse_str_list(
        &self,
        tool: &str,
        args: &serde_json::Value,
        key: &str,
        allow_scalar: bool,
    ) -> Result<Vec<String>, serde_json::Value> {
        match args.get(key) {
            None | Some(serde_json::Value::Null) => Ok(Vec::new()),
            Some(serde_json::Value::String(s)) if allow_scalar => {
                let t = s.trim();
                Ok(if t.is_empty() {
                    Vec::new()
                } else {
                    vec![t.to_string()]
                })
            }
            Some(serde_json::Value::Array(a)) => a
                .iter()
                .map(|x| {
                    x.as_str().map(|s| s.to_string()).ok_or_else(|| {
                        self.smrp_err(
                            tool,
                            400,
                            &format!("invalid params: {key} items must be strings"),
                        )
                    })
                })
                .collect(),
            Some(_) => Err(self.smrp_err(
                tool,
                400,
                &format!("invalid params: {key} must be an array of strings"),
            )),
        }
    }

    /// 定位一条记忆所属簇。委托共享层。
    fn cluster_of(&self, id: crate::domain::tetra::TetraId) -> Option<(usize, usize)> {
        super::smrp::cluster_index(&self.engine).get(&id).copied()
    }

    /// 构造 MemoryItem。委托共享层。
    #[allow(clippy::too_many_arguments)]
    fn memory_item(
        &self,
        id: crate::domain::tetra::TetraId,
        content: &str,
        labels: &[String],
        timestamp: i64,
        tier: &str,
        source: Vec<&str>,
        similarity: f64,
        topology: Option<serde_json::Value>,
    ) -> serde_json::Value {
        super::smrp::memory_item(
            &self.engine,
            id,
            content,
            labels,
            timestamp,
            tier,
            source,
            similarity,
            topology,
        )
    }

    /// SMRP §7.2 — 结构化录入工具的统一回声：create + 安置摘要 + 建链数。
    /// echo 由调用方预填类别/回声字段；本方法补 id/status/placement/relations_formed。
    /// 配额检查在此统一完成(与 REST 对等,B1修复)。
    fn create_echo(
        &self,
        tool: &str,
        content: &str,
        labels: Vec<String>,
        mut echo: serde_json::Value,
    ) -> serde_json::Value {
        // 配额检查(与 REST check_and_increment_memory 对等)
        if let Err(err_resp) = self.check_quota(tool) {
            return err_resp;
        }
        // P1记忆分层：根据 tool 名和 labels 自动设置 memory_class
        let auto_class = if tool == "session_summary"
            || tool == "ctx_save"
            || tool == "context_observe"
            || labels.iter().any(|l| {
                l == "session-summary" || l == "ctx-session-summary" || l == "system-observation"
            }) {
            Some("session".to_string())
        } else {
            None
        };

        match self
            .engine
            .scheduler
            .api_create_memory_full(content, labels)
        {
            Ok(r) => {
                self.rollback_quota(r.is_new); // dedup 回滚配额
                                               // P1：创建后更新 memory_class
                if let Some(ref class) = auto_class {
                    if let Err(e) = self.engine.scheduler.api_set_memory_class(r.id, class) {
                        tracing::warn!("[MCP] memory_class persist failed for {}: {}", r.id, e);
                    }
                }
                let status = if r.dedup_matched.is_some() {
                    "deduped"
                } else if r.is_new {
                    "created"
                } else {
                    "exists"
                };
                echo["id"] = serde_json::json!(r.id);
                echo["status"] = serde_json::json!(status);
                if let Some(ref class) = auto_class {
                    echo["memory_class"] = serde_json::json!(class);
                }
                echo["placement"] = match &r.placement {
                    Some(p) => serde_json::json!({
                        "layer": p.layer,
                        "joined_cluster_size": self.cluster_of(r.id).map(|(_, s)| s),
                        "vertices_shared": p.vertices_shared,
                        "is_seed": p.is_seed,
                    }),
                    None => serde_json::Value::Null,
                };
                echo["relations_formed"] = serde_json::json!(r.relations_formed);
                self.smrp_ok(tool, echo)
            }
            Err(e) => {
                self.rollback_quota(false); // 创建失败回滚配额
                self.smrp_err(tool, 500, &e)
            }
        }
    }

    fn build_search_filters(
        &self,
        args: &serde_json::Value,
    ) -> Option<super::search_engine::SearchFilters> {
        let has_labels = args["labels"].is_array();
        let has_min_imp = args["min_importance"].is_number();
        let has_project = args["project"].is_string();
        let has_since = args["since_days"].is_number();
        let has_mode = args["mode"].is_string();
        let has_strict = args["strict_filter"].is_boolean();
        let has_as_of = args["as_of"].is_number();
        // Phase 1: mode/strict_filter 也算"有过滤参数", 不能返回 None 否则 mode 丢失
        if !has_labels
            && !has_min_imp
            && !has_project
            && !has_since
            && !has_mode
            && !has_strict
            && !has_as_of
        {
            return None;
        }
        let mut f = super::search_engine::SearchFilters::default();
        if has_labels {
            f.labels = args["labels"].as_array().map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            });
        }
        if has_min_imp {
            f.min_importance = args["min_importance"].as_f64();
        }
        if has_project {
            f.project = args["project"].as_str().map(String::from);
        }
        if let Some(days) = args["since_days"].as_u64() {
            let now_ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            f.since_ts = Some(now_ts - (days as i64 * 86400));
        }
        // Phase 1: 解析 mode 参数
        if let Some(mode_str) = args["mode"].as_str() {
            f.mode = super::search_engine::SearchMode::from_str_lossy(mode_str);
        }
        // Phase 1: 解析 strict_filter
        if let Some(strict) = args["strict_filter"].as_bool() {
            f.strict_filter = strict;
        }
        // P1-6(路线图#170): 时点查询暴露给 agent — 只返回 as_of 时刻有效的记忆
        // (valid_from <= as_of 且 (valid_to为空 或 valid_to > as_of)), 让"上个月他住哪"这类
        // 历史问题不被当前事实覆盖。REST /v1/search 已有同参数, 此处补齐 MCP 面。
        if has_as_of {
            f.as_of = args["as_of"].as_i64().filter(|t| *t > 0);
        }
        Some(f)
    }

    fn build_action_items(
        &self,
        sched: &super::scheduler::SchedulerCenter,
    ) -> Vec<serde_json::Value> {
        let mut items: Vec<serde_json::Value> = Vec::new();
        let now_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let sessions = sched.api_list_by_labels(&["session-summary"], 10);
        for (_, p) in sessions.iter().take(3) {
            if let Some(pos) = p.content.find("next_steps") {
                let start = pos + 11;
                if let Some(slice) = p.content.get(start..) {
                    let end = slice.find('\n').unwrap_or(slice.len());
                    let next = truncate_str(&slice[..end], 150).trim();
                    if !next.is_empty() && next.len() > 5 {
                        let age_days = (now_ts - p.timestamp) / 86400;
                        items.push(serde_json::json!({
                            "type": "incomplete_task",
                            "detail": truncate_str(next, 120),
                            "age_days": age_days,
                            "source_memory_id": p.content_hash,
                        }));
                    }
                }
            }
        }

        let all = sched.api_load_context(50);
        let mut potential_dupes: Vec<(String, String, u64, u64)> = Vec::new();
        for i in 0..all.len() {
            for j in (i + 1)..all.len().min(i + 10) {
                let (_, s1, c1, l1) = &all[i];
                let (_, s2, c2, l2) = &all[j];
                if *s1 > 1.5 && *s2 > 1.5 {
                    let overlap_labels: Vec<_> = l1.iter().filter(|l| l2.contains(l)).collect();
                    if overlap_labels.len() >= 2 && (c1.len() > 30 || c2.len() > 30) {
                        let sim = super::intake::MemoryIntake::text_similarity(c1, c2);
                        if sim > 0.55 {
                            potential_dupes.push((
                                c1.chars().take(60).collect(),
                                c2.chars().take(60).collect(),
                                0,
                                0,
                            ));
                            if potential_dupes.len() >= 3 {
                                break;
                            }
                        }
                    }
                }
            }
            if potential_dupes.len() >= 3 {
                break;
            }
        }
        for (a, b, _, _) in &potential_dupes {
            items.push(serde_json::json!({
                "type": "potential_duplicate",
                "detail": format!("Two memories may overlap: '{}' vs '{}'", a, b),
            }));
        }

        let low_imp_stale: Vec<_> = all
            .iter()
            .filter(|(_, s, c, _)| *s < 0.5 && c.len() > 20)
            .take(2)
            .collect();
        for (_, _, content, _) in low_imp_stale {
            items.push(serde_json::json!({
                "type": "low_relevance",
                "detail": format!("Low-relevance memory may need cleanup: '{}'", truncate_str(content, 80)),
            }));
        }

        let outdated = sched.api_list_by_labels(&["outdated"], 5);
        if !outdated.is_empty() {
            items.push(serde_json::json!({
                "type": "outdated_memories",
                "detail": format!("{} memories marked outdated, consider cleanup or restore", outdated.len()),
                "count": outdated.len(),
            }));
        }

        items.truncate(5);
        items
    }

    pub fn with_pub_skills(
        engine: Arc<Engine>,
        pub_skills: Arc<super::skills::SkillEngine>,
    ) -> Self {
        Self {
            engine,
            pub_skills: Some(pub_skills),
            quota: None,
            role_gate: None,
            custom_permissions: None,
        }
    }

    /// A01: 注入子账户角色门(主账户/本地不调用=全权)
    pub fn with_role_gate(mut self, role: crate::engine::user_manager::UserRole) -> Self {
        self.role_gate = Some(role);
        self
    }

    pub fn with_custom_permissions(mut self, permissions: Option<Vec<String>>) -> Self {
        self.custom_permissions = permissions;
        self
    }

    pub fn with_quota(mut self, quota: QuotaContext) -> Self {
        self.quota = Some(quota);
        self
    }

    pub fn engine(&self) -> Arc<Engine> {
        Arc::clone(&self.engine)
    }

    pub fn handle(&self, req: McpRequest) -> McpResponse {
        match req.method.as_str() {
            "initialize" => self.initialize(req.id),
            "server/discover" => self.server_discover(req.id),
            "ping" => self.pong(req.id),
            "tools/list" => self.tools_list(req.id),
            "tools/call" => self.tools_call(req.id, req.params),
            "resources/list" => self.resources_list(req.id),
            "resources/read" => self.resources_read(req.id, req.params),
            "logging/setLevel" => self.logging_set_level(req.id, req.params),
            "subscriptions/listen" => self.subscriptions_listen(req.id, req.params),
            "notifications/initialized" => McpResponse {
                jsonrpc: "2.0".into(),
                id: req.id,
                result: Some(serde_json::json!({})),
                error: None,
            },
            _ => McpResponse {
                jsonrpc: "2.0".into(),
                id: req.id,
                result: None,
                error: Some(McpError {
                    code: -32601,
                    message: format!("unknown method: {}", req.method),
                }),
            },
        }
    }

    /// zod严格客户端友好: 递归移除值为null的对象字段(缺席比null类型更兼容)
    fn strip_null_fields(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(m) => {
                m.retain(|_, val| !val.is_null());
                for val in m.values_mut() {
                    Self::strip_null_fields(val);
                }
            }
            serde_json::Value::Array(a) => {
                for val in a.iter_mut() {
                    Self::strip_null_fields(val);
                }
            }
            _ => {}
        }
    }

    fn smrp_ok_nn(&self, tool: &str, mut data: serde_json::Value) -> serde_json::Value {
        Self::strip_null_fields(&mut data);
        self.smrp_ok(tool, data)
    }

    /// P22 对时协议: 现实时间权威源 — 服务端物理时区时间, 每次交互下发
    fn server_now_json() -> serde_json::Value {
        let now = chrono::Utc::now();
        let beijing = now + chrono::Duration::hours(8);
        serde_json::json!({
            "epoch_ms": now.timestamp_millis(),
            "iso": beijing.format("%Y-%m-%dT%H:%M:%S%.3f+08:00").to_string(),
            "human": beijing.format("%H:%M:%S").to_string() + " CST(UTC+8, 服务器权威)",
        })
    }
}

fn strip_html(s: &str) -> String {
    // 仅当 '<' 后紧跟字母 / '/' / '!' 且存在闭合 '>' 时才视为标签；
    // 孤立 '<'(如 "a < b")保持原样,避免吞掉后续内容。
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<'
            && chars
                .get(i + 1)
                .is_some_and(|c| c.is_ascii_alphabetic() || *c == '/' || *c == '!')
        {
            if let Some(off) = chars[i + 1..].iter().position(|c| *c == '>') {
                i += off + 2;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn sanitize_label(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn regex_captures(name: &str) -> String {
    if let Some(start) = name.find('(') {
        if let Some(end) = name.find(')') {
            if start < end {
                let eng = &name[start + 1..end];
                let slug: String = eng
                    .to_lowercase()
                    .replace(&[':', '/', '\\', ' '][..], "-")
                    .chars()
                    .map(|c| {
                        if c.is_alphanumeric() || c == '-' {
                            c
                        } else {
                            '-'
                        }
                    })
                    .collect();
                return slug
                    .split('-')
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join("-");
            }
        }
    }
    String::new()
}

fn serialize_mcp_response(response: McpResponse, request_id: Option<serde_json::Value>) -> String {
    match serde_json::to_string(&response) {
        Ok(serialized) if serialized.len() <= MAX_MCP_RESPONSE_BYTES => serialized,
        Ok(_) => serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "error": {
                "code": -32000,
                "message": format!(
                    "response too large; reduce the result limit or narrow the query (maximum {} bytes)",
                    MAX_MCP_RESPONSE_BYTES
                )
            }
        })
        .to_string(),
        Err(e) => {
            tracing::error!("[MCP] failed to serialize JSON-RPC response: {}", e);
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "error": {"code": -32603, "message": "internal error"}
            })
            .to_string()
        }
    }
}

#[cfg(test)]
type TransitionHook = Box<dyn Fn()>;

#[cfg(test)]
thread_local! {
    /// Test seam: runs between outcome-memory write and frontier transition in
    /// action_complete, to make the cancel/stop race deterministic.
    static BEFORE_COMPLETE_TRANSITION: std::cell::RefCell<Option<TransitionHook>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn set_before_complete_transition_hook(h: Option<TransitionHook>) {
    BEFORE_COMPLETE_TRANSITION.with(|c| *c.borrow_mut() = h);
}

#[cfg(test)]
fn run_before_complete_transition_hook() {
    BEFORE_COMPLETE_TRANSITION.with(|c| {
        if let Some(f) = c.borrow().as_ref() {
            f();
        }
    });
}
