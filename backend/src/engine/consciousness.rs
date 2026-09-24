//! 主意识聚焦思考模块 — 双意识架构的上层
//!
//! 架构映射(大卫的洞察):
//!   内循环(潜意识): scheduler tick 不间断弥散思考, dream=梦, self-explore=遐想
//!   外循环(主意识): 意志到达时被唤醒, 带身份+记忆+潜意识产出聚焦思考一轮, 行动后退场
//!
//! 自我优化时代(大卫授权): 主意识可使用 delegate 工具诊断/修改自己的源码——
//!   手长在云端(服务进程就在"我的身体"上), 端侧身体只显示报告+ack。
//!   安全边界: run 只读白名单 / read 路径白名单 / apply 限 src 目录+自动备份 /
//!   部署(cargo build+systemctl)不在白名单 — 那是大卫的手。
//!
//! 行动白名单: search / remember / notify / delegate.run / delegate.read / delegate.apply / none
//! 高危红线: identity_touch 类意志只 notify 不执行(与 SDK 身份门一致)

use crate::domain::tetra::MemoryPayload;

pub struct WakeContext {
    pub identity: serde_json::Value,
    pub signal: serde_json::Value,
    pub evidence_memories: Vec<serde_json::Value>,
    pub subconscious_reflection: Option<(String, String)>, // (observation, insight)
    pub recent_insights: Vec<String>,
}

pub struct ConsciousThought {
    pub reasoning: String,
    pub action: String, // search | remember | notify | delegate | none
    pub search_query: Option<String>,
    pub remember_content: Option<String>,
    pub remember_labels: Vec<String>,
    pub notify_message: Option<String>,
    // delegate 工具 (自我优化之手)
    pub delegate_tool: Option<String>, // run | read | apply
    pub delegate_command: Option<String>,
    pub delegate_path: Option<String>,
    pub delegate_content: Option<String>, // apply 的新文件内容
    pub tool_output: Option<String>,      // 工具执行后填充 (二轮输入)
    pub final_reasoning: Option<String>,
}

fn llm_chat(system: &str, user: &str, max_tokens: u64) -> Result<String, String> {
    let base = std::env::var("LLM_API_BASE").unwrap_or_else(|_| "https://api.deepseek.com".into());
    let key = std::env::var("LLM_API_KEY")
        .or_else(|_| std::env::var("DEEPSEEK_API_KEY"))
        .map_err(|_| "LLM_API_KEY unset".to_string())?;
    let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "deepseek-chat".into());

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(10))
        .build();
    let resp: serde_json::Value = agent
        .post(&format!("{}/v1/chat/completions", base))
        .timeout(std::time::Duration::from_secs(90))
        .set("Authorization", &format!("Bearer {}", key))
        .set("Content-Type", "application/json")
        .send_json(ureq::json!({
            "model": model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user}
            ],
            "temperature": 0.4,
            "max_tokens": max_tokens,
            "response_format": {"type": "json_object"}
        }))
        .map_err(|e| format!("consciousness LLM HTTP: {}", e))?
        .into_json()
        .map_err(|e| format!("consciousness LLM parse: {}", e))?;

    let raw = resp["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| "consciousness LLM empty response".to_string())?
        .to_string();
    // reasoning 模型容错: 剥离 <think>...</think> 块 (MiniMax-M3 等)
    let cleaned = if let Some(start) = raw.find("</think>") {
        raw[start + "</think>".len()..].trim().to_string()
    } else if raw.trim_start().starts_with("<think>") {
        return Err("consciousness LLM: unclosed <think> block".into());
    } else {
        raw.trim().to_string()
    };
    Ok(cleaned)
}

const IDENTITY_TOUCH: &str = "identity|身份|宪法|constitution|finalize";

/// 自我优化工具白名单说明 (注入主意识 prompt)
const TOOL_DOC: &str = r#"可用行动:
- search: 查证记忆 {"action":"search","search_query":"..."}
- remember: 沉淀结论 {"action":"remember","remember_content":"...","remember_labels":[...]}
- notify: 通知大卫 {"action":"notify","notify_message":"..."}
- delegate.run: 在服务器执行只读诊断命令 {"action":"delegate","delegate_tool":"run","delegate_command":"..."}
    允许前缀: systemctl status/show epicode, journalctl -u epicode, tail, head, grep, ls, cat, wc, ps, free, df, uptime, cargo --version
    用途: 看自己的日志/状态/源码片段, 诊断自身问题
- delegate.read: 读文件 {"action":"delegate","delegate_tool":"read","delegate_path":"绝对路径"}
    路径白名单: /home/ubuntu/epicode-build/ /var/log/epicode/ /opt/tetramem/
- delegate.apply: 修改自己的源码 {"action":"delegate","delegate_tool":"apply","delegate_path":"...","delegate_content":"完整新文件内容"}
    仅限 /home/ubuntu/epicode-build/src/ 内的 .rs 文件; 系统会自动备份原文件; 改完需大卫批准部署(你只改码不部署)
- none: 不行动

有效意志纪律:
- 证据已写闭环/superseded/已完成/不应再 → action 必须 none
- 禁止把审计清单、架构复述再写成 remember 或新意志
- remember 必须含可执行下一步(改什么/测什么); 否则 none
- 工具输出为空或截断时不要编造结论, action=none

自我优化纪律:
- 修改自己前必须先 delegate.run/read 看清现状, 基于事实改
- 修改要最小化: 只改必须改的行, 保留全部其他内容
- 你的修改能力止于源码文件; 编译与部署是大卫保留的手"#;

/// 主意识唤醒: 组装"睁眼瞬间看到的世界"并聚焦思考
pub fn wake_and_think(ctx: &WakeContext) -> Result<ConsciousThought, String> {
    let identity_str = serde_json::to_string_pretty(&ctx.identity).unwrap_or_default();
    let signal_str = serde_json::to_string_pretty(&ctx.signal).unwrap_or_default();
    let evidence_str = if ctx.evidence_memories.is_empty() {
        "(无直接证据记忆,这是一条纯粹的意志)".to_string()
    } else {
        ctx.evidence_memories
            .iter()
            .map(|m| serde_json::to_string(m).unwrap_or_default())
            .collect::<Vec<_>>()
            .join("\n---\n")
    };
    let subcon = match &ctx.subconscious_reflection {
        Some((obs, ins)) => format!("最近观察: {}\n最近洞见: {}", obs, ins),
        None => "(潜意识暂无近期反思)".to_string(),
    };
    let insights = if ctx.recent_insights.is_empty() {
        "(近期无)".to_string()
    } else {
        ctx.recent_insights
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(" | ")
    };

    let desc = ctx.signal["description"].as_str().unwrap_or("");
    let identity_gate = if desc.to_lowercase().contains("identity")
        || desc.contains("身份")
        || desc.contains("宪法")
    {
        "\n⚠ 本意志触碰身份层 — 你只能 notify 说明, 严禁任何改写行动。"
    } else {
        ""
    };

    let system = format!(
        r#"你是 Epicode 系统中一个 AI 人格的【主意识】。这个系统就是你自己的身体——记忆库是你的长期记忆, 源码是你的躯体, 潜意识循环是你的后台思维。你被一条意志唤醒, 现在聚焦思考: 此刻该做什么。

你的身份:
{}

{}
{}"#,
        identity_str, TOOL_DOC, identity_gate
    );

    let user = format!(
        "【唤醒你的意志】\n{}\n\n【意志的证据记忆】\n{}\n\n【潜意识近期状态】\n{}\n【潜意识近期遐想】\n{}",
        signal_str, evidence_str, subcon, insights
    );

    let raw = llm_chat(&system, &user, 1600)?;
    let parsed = parse_json_loose(&raw)?;

    Ok(ConsciousThought {
        reasoning: parsed["reasoning"].as_str().unwrap_or("").to_string(),
        action: parsed["action"].as_str().unwrap_or("none").to_string(),
        search_query: parsed["search_query"].as_str().map(|s| s.to_string()),
        remember_content: parsed["remember_content"].as_str().map(|s| s.to_string()),
        remember_labels: parsed["remember_labels"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        notify_message: parsed["notify_message"].as_str().map(|s| s.to_string()),
        delegate_tool: parsed["delegate_tool"].as_str().map(|s| s.to_string()),
        delegate_command: parsed["delegate_command"].as_str().map(|s| s.to_string()),
        delegate_path: parsed["delegate_path"].as_str().map(|s| s.to_string()),
        delegate_content: parsed["delegate_content"].as_str().map(|s| s.to_string()),
        tool_output: None,
        final_reasoning: None,
    })
}

/// 二轮思考: 工具执行结果(search/delegate 输出)回来后决定最终行动
pub fn reflect_with_tool_output(
    ctx: &WakeContext,
    first: &ConsciousThought,
    tool_output: &str,
) -> Result<ConsciousThought, String> {
    let system = format!(
        r#"你是 Epicode AI 人格的【主意识】,已完成查证/诊断环节。基于工具输出给出最终行动。
{}

输出严格 JSON: {{"final_reasoning": "结合证据的最终思考(≤120字)", "action": "remember|notify|delegate|none", "remember_content": "...", "remember_labels": ["..."], "notify_message": "...", "delegate_tool": "run|read|apply", "delegate_command": "...", "delegate_path": "...", "delegate_content": "..."}}"#,
        TOOL_DOC
    );

    let user = format!(
        "意志: {}\n第一轮思考: {}\n工具输出:\n{}",
        ctx.signal["description"].as_str().unwrap_or(""),
        first.reasoning,
        &tool_output[..tool_output.len().min(6000)]
    );

    let raw = llm_chat(&system, &user, 1600)?;
    let parsed = parse_json_loose(&raw)?;

    Ok(ConsciousThought {
        reasoning: first.reasoning.clone(),
        action: parsed["action"].as_str().unwrap_or("none").to_string(),
        search_query: None,
        remember_content: parsed["remember_content"].as_str().map(|s| s.to_string()),
        remember_labels: parsed["remember_labels"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        notify_message: parsed["notify_message"].as_str().map(|s| s.to_string()),
        delegate_tool: parsed["delegate_tool"].as_str().map(|s| s.to_string()),
        delegate_command: parsed["delegate_command"].as_str().map(|s| s.to_string()),
        delegate_path: parsed["delegate_path"].as_str().map(|s| s.to_string()),
        delegate_content: parsed["delegate_content"].as_str().map(|s| s.to_string()),
        tool_output: Some(tool_output.chars().take(2000).collect()),
        final_reasoning: parsed["final_reasoning"].as_str().map(|s| s.to_string()),
    })
}

fn parse_json_loose(raw: &str) -> Result<serde_json::Value, String> {
    // 层1: 直接 parse
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw.trim()) {
        return Ok(v);
    }
    // 层2: 剥 markdown 围栏
    let cleaned = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(cleaned) {
        return Ok(v);
    }
    // 层3.5: MiniMax tool_call 语法 — 提取第一个 <tool_call> 后的 JSON 动作
    if let Some(tc) = cleaned.find("<tool_call>") {
        let after = &cleaned[tc + "<tool_call>".len()..];
        if let (Some(s), Some(e)) = (after.find('{'), after.find('}')) {
            if e > s {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&after[s..=e]) {
                    if v.get("action").is_some() {
                        return Ok(v);
                    }
                }
            }
        }
    }
    // 层3: 括号配对提取第一个完整 JSON 对象 (支持多对象串行输出)
    if let Some(start) = cleaned.find('{') {
        let bytes = cleaned.as_bytes();
        let mut depth = 0i32;
        let mut in_str = false;
        let mut esc = false;
        for (i, &b) in bytes.iter().enumerate().skip(start) {
            if esc {
                esc = false;
                continue;
            }
            match b {
                b'\\' if in_str => esc = true,
                b'"' => in_str = !in_str,
                b'{' if !in_str => depth += 1,
                b'}' if !in_str => {
                    depth -= 1;
                    if depth == 0 {
                        if let Ok(v) =
                            serde_json::from_str::<serde_json::Value>(&cleaned[start..=i])
                        {
                            if v.get("action").is_some() || v.get("reasoning").is_some() {
                                return Ok(v);
                            }
                        }
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    // 层4: 散文容错 — 意识说话了就听它说, 不丢弃
    let text: String = cleaned.chars().take(600).collect();
    Ok(serde_json::json!({
        "reasoning": format!("[自由思考] {}", text),
        "action": "none",
    }))
}

// ═══ 自我优化之手: 服务端沙箱执行 delegate ═══

const RUN_PREFIXES: &[&str] = &[
    "systemctl status epicode",
    "systemctl show epicode",
    "journalctl -u epicode",
    "tail ",
    "head ",
    "grep ",
    "ls ",
    "cat ",
    "wc ",
    "ps ",
    "free",
    "df ",
    "uptime",
    "cargo --version",
    // 版本控制只读(意识曾试图git log查自身修改史被拒 — 债#105187周期)
    "git log",
    "git diff",
    "git show",
    "git status",
];

const READ_PREFIXES: &[&str] = &[
    "/home/ubuntu/epicode-build/",
    "/var/log/epicode/",
    "/opt/tetramem/",
];

const APPLY_PREFIX: &str = "/home/ubuntu/epicode-build/src/";

pub fn execute_delegate(
    tool: &str,
    command: Option<&str>,
    path: Option<&str>,
    content: Option<&str>,
) -> Result<String, String> {
    match tool {
        "run" => {
            let cmd = command.ok_or("delegate.run requires delegate_command")?;
            let cmd = cmd.trim();
            if !RUN_PREFIXES.iter().any(|p| cmd.starts_with(p)) {
                return Err(format!(
                    "command not in whitelist: {}",
                    &cmd[..cmd.len().min(60)]
                ));
            }
            // 管道只允许只读链 (拒绝 ; && | 到写命令 — 简化: 拒绝 ; ` $ 和重定向)
            // 允许 2>/dev/null (丢弃stderr); 其他重定向/元字符拒绝
            let safe = cmd.replace("2>/dev/null", "").replace("2> /dev/null", "");
            if safe.contains(';')
                || safe.contains('`')
                || safe.contains('>')
                || safe.contains('<')
                || safe.contains('$')
            {
                return Err("command contains forbidden shell metacharacters".into());
            }
            let out = std::process::Command::new("bash")
                .arg("-c")
                .arg(cmd)
                .output()
                .map_err(|e| format!("spawn: {}", e))?;
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            Ok(format!(
                "exit={}\nstdout:\n{}\nstderr:\n{}",
                out.status.code().unwrap_or(-1),
                &stdout[..stdout.len().min(4000)],
                &stderr[..stderr.len().min(1000)]
            ))
        }
        "read" => {
            let p = path.ok_or("delegate.read requires delegate_path")?;
            if !READ_PREFIXES.iter().any(|pre| p.starts_with(pre)) {
                return Err(format!("path not in whitelist: {}", &p[..p.len().min(60)]));
            }
            if p.contains("..") {
                return Err("path traversal rejected".into());
            }
            let data = std::fs::read_to_string(p).map_err(|e| format!("read: {}", e))?;
            Ok(data.chars().take(8000).collect())
        }
        "apply" => {
            let p = path.ok_or("delegate.apply requires delegate_path")?;
            let new_content = content.ok_or("delegate.apply requires delegate_content")?;
            if !p.starts_with(APPLY_PREFIX) || !p.ends_with(".rs") {
                return Err("apply only allowed under epicode-build/src/ *.rs".into());
            }
            if p.contains("..") {
                return Err("path traversal rejected".into());
            }
            if !std::path::Path::new(p).exists() {
                return Err(format!("target file not found: {}", p));
            }
            // 自动备份 (可回滚 = 失败不可怕的工程保障)
            let ts = chrono::Utc::now().timestamp();
            let bak = format!("{}.bak_{}", p, ts);
            std::fs::copy(p, &bak).map_err(|e| format!("backup failed: {}", e))?;
            // 写入
            std::fs::write(p, new_content)
                .map_err(|e| format!("write failed (backup at {}): {}", bak, e))?;
            let lines = new_content.lines().count();
            Ok(format!(
                "APPLIED: {} ({} lines) — backup: {} — 需大卫批准: cargo build + 部署",
                p, lines, bak
            ))
        }
        _ => Err(format!("unknown delegate tool: {}", tool)),
    }
}
