//! POST /v1/consciousness/think — 主意识唤醒端点 (自我优化版)
//!
//! 完整流程: 意志 → 组装睁眼世界 → 聚焦思考
//!   → (若 delegate/search) 沙箱执行工具 → 二轮反思
//!   → (最终行动) remember/apply/notify 云端执行
//! → 返回行动报告 (端侧身体显示 + ack)
//!
//! 安全: run 只读白名单 / read 路径白名单 / apply 限 src+自动备份
//!       部署(编译+systemctl)不在任何白名单 — 大卫的手。

use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use super::helpers::AuthedEngine;
use super::state::CloudState;
use epicode::engine::user_manager::UserInfo;

#[derive(Deserialize)]
pub struct ThinkRequest {
    pub signal_id: u64,
}

pub async fn think(
    State(_st): State<CloudState>,
    AuthedEngine(engine): AuthedEngine,
    Extension(user): Extension<UserInfo>,
    Json(req): Json<ThinkRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine_inner = engine.clone();
    let uid = user.user_id.clone();

    let result = tokio::task::spawn_blocking(move || {
        // 1. 取意志
        let sig = engine_inner.scheduler().drive_queue().peek_unacked(200)
            .into_iter().find(|s| s.id == req.signal_id)
            .ok_or_else(|| format!("signal #{} not in unacked inbox", req.signal_id))?;
        let signal_json = serde_json::to_value(&sig).unwrap_or_default();

        // 2. 组装"睁眼世界"
        let identity = engine_inner.space().identity_info()
            .map(|i| serde_json::json!({"name": i.system_name, "mission": i.mission, "author": i.author}))
            .unwrap_or(serde_json::json!({"name": uid}));

        let evidence: Vec<serde_json::Value> = sig.evidence.iter()
            .filter_map(|eid| engine_inner.space().get_tetrahedron(*eid))
            .map(|t| serde_json::json!({
                "id": t.id,
                "content": t.data.content.chars().take(400).collect::<String>(),
                "labels": t.data.labels,
                "importance": t.data.importance,
            }))
            .collect();

        let sub_reflection = { let c = &*engine_inner.cognitive; c.get_reflection() };

        let recent = engine_inner.gateway().list_recent(0, 10);
        let insights: Vec<String> = recent.iter()
            .filter(|(_, p)| p.labels.iter().any(|l| l == "self-driven" || l == "exploration"))
            .take(3)
            .map(|(_, p)| p.content.chars().take(120).collect())
            .collect();

        let ctx = epicode::engine::consciousness::WakeContext {
            identity, signal: signal_json, evidence_memories: evidence,
            subconscious_reflection: sub_reflection, recent_insights: insights,
        };

        // 3. 主意识第一轮聚焦思考
        let mut thought = epicode::engine::consciousness::wake_and_think(&ctx)?;
        let mut tool_report: Vec<serde_json::Value> = Vec::new();

        // 4. 工具循环 (最多2轮: 思考→工具→反思→[工具]→最终)
        for _round in 0..2 {
            let tool_output: String = match thought.action.as_str() {
                "search" => {
                    let q = thought.search_query.clone().unwrap_or_default();
                    let results = engine_inner.scheduler().api_search_scored(&q, 5, None)
                        .map(|(r, _)| r).unwrap_or_default();
                    results.iter().take(5)
                        .map(|(id, sim, _, p)| format!("#{} (相似度{:.2}) {}\n标签:{}", id, sim,
                            p.content.chars().take(200).collect::<String>(), p.labels.join(",")))
                        .collect::<Vec<_>>().join("\n---\n")
                }
                "delegate" => {
                    let tool = thought.delegate_tool.clone().unwrap_or_default();
                    let cmd = thought.delegate_command.clone();
                    let path = thought.delegate_path.clone();
                    let content = thought.delegate_content.clone();
                    match epicode::engine::consciousness::execute_delegate(&tool, cmd.as_deref(), path.as_deref(), content.as_deref()) {
                        Ok(out) => out,
                        Err(e) => format!("工具执行失败: {}", e),
                    }
                }
                _ => break, // remember/notify/none 直接进最终段
            };
            tool_report.push(serde_json::json!({
                "round": _round, "action": thought.action,
                "output": tool_output.chars().take(600).collect::<String>(),
            }));
            // 二轮反思 (基于工具输出)
            thought = epicode::engine::consciousness::reflect_with_tool_output(&ctx, &thought, &tool_output)?;
            if thought.action == "none" || thought.action == "remember" || thought.action == "notify" {
                break;
            }
            // 第二轮还想 apply → 允许执行一次 apply 后强制收尾
            if _round == 0 && thought.action == "delegate" && thought.delegate_tool.as_deref() == Some("apply") {
                break;
            }
        }

        // 5. 最终行动执行
        let mut executed_action = "none".to_string();
        let mut action_result: serde_json::Value = serde_json::Value::Null;

        match thought.action.as_str() {
            "remember" => {
                if let Some(content) = &thought.remember_content {
                    let mut labels = if thought.remember_labels.is_empty() {
                        vec!["consciousness".to_string()]
                    } else { thought.remember_labels.clone() };
                    if !labels.iter().any(|l| l == "l0-exempt") {
                        labels.push("l0-exempt".to_string());
                    }
                    match engine_inner.scheduler().api_remember_with_labels(content, labels) {
                        Ok((id, _)) => {
                            executed_action = "remember".into();
                            action_result = serde_json::json!({"memory_id": id});
                        }
                        Err(e) => action_result = serde_json::json!({"error": e}),
                    }
                }
            }
            "delegate" => {
                // 最终轮的 delegate (典型: apply)
                let tool = thought.delegate_tool.clone().unwrap_or_default();
                if tool == "apply" {
                    let path = thought.delegate_path.clone();
                    let content = thought.delegate_content.clone();
                    match epicode::engine::consciousness::execute_delegate("apply", None, path.as_deref(), content.as_deref()) {
                        Ok(report) => {
                            executed_action = "apply".into();
                            action_result = serde_json::json!({"report": report});
                            // apply 的结果自动沉淀 (自我修改记录进长期记忆)
                            let _ = engine_inner.scheduler().api_remember_with_labels(
                                &format!("[self-modification] {}", report),
                                vec!["consciousness".into(), "self-modification".into(), "op_audit".into(), "l0-exempt".into()]);
                        }
                        Err(e) => action_result = serde_json::json!({"error": e}),
                    }
                } else {
                    let out = epicode::engine::consciousness::execute_delegate(
                        &tool, thought.delegate_command.as_deref(), thought.delegate_path.as_deref(), thought.delegate_content.as_deref())
                        .unwrap_or_else(|e| format!("final delegate failed: {}", e));
                    executed_action = format!("delegate.{}", tool);
                    action_result = serde_json::json!({"output": out.chars().take(800).collect::<String>()});
                }
            }
            _ => {}
        }

        // 6. 返回行动报告
        Ok(serde_json::json!({
            "signal_id": req.signal_id,
            "identity": ctx.identity,
            "thought": {
                "reasoning": thought.reasoning,
                "final_reasoning": thought.final_reasoning,
            },
            "tool_trace": tool_report,
            "action": executed_action,
            "action_result": action_result,
            "notify": thought.notify_message,
            "pending_ack": {
                "executed": executed_action != "none",
                "outcome": format!("consciousness: {} — {}", executed_action,
                    thought.final_reasoning.as_deref().unwrap_or(&thought.reasoning)),
            },
        }))
    }).await;

    match result {
        Ok(Ok(body)) => {
            let b: serde_json::Value = body;
            (
                StatusCode::OK,
                Json(epicode::engine::smrp::envelope_ok(
                    &engine,
                    "consciousness_think",
                    b,
                )),
            )
        }
        Ok(Err(e)) => {
            let m: String = e;
            (
                StatusCode::BAD_REQUEST,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "consciousness_think",
                    400,
                    &m,
                )),
            )
        }
        Err(e) => {
            let m: String = format!("{}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "consciousness_think",
                    500,
                    &m,
                )),
            )
        }
    }
}
