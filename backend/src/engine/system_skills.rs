use super::skills::{Skill, SkillEngine};

fn system_skill(name: &str, description: &str, skill_md: &str, id: u64) -> Skill {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    Skill {
        id,
        name: name.to_string(),
        skill_md: skill_md.to_string(),
        version: "1.0.0".to_string(),
        owner: "__system__".to_string(),
        is_public: true,
        review_status: super::skills::ReviewStatus::Approved,
        review_note: None,
        usage_count: 0,
        success_rate: 0.0,
        memory_ids: Vec::new(),
        evolved_from: None,
        is_system: true,
        category: None,
        description: Some(description.to_string()),
        triggers: Vec::new(),
        surface_impressions: 0,
        requires: Vec::new(),
        produces: Vec::new(),
        capabilities: Vec::new(),
        created_at: now,
        updated_at: now,
    }
}

// S2: 触发描述按"何时用"而非"是什么"书写(描述=触发条件 — 自动触发精度的决定因素)
const SYSTEM_SKILLS: &[(&str, &str, &str)] = &[
    ("记忆智能存取", "何时用: 任何要存/取/搜记忆的时刻 — memory_search精确检索、memory_create沉淀经验、上下文丢失后恢复锚定。任务开工与收尾必用。",
     include_str!("../../system_skills/01_memory_io.md")),
    ("自动进化循环", "何时用: 完成一段实质性工作之后 — ctx_save/session_summary沉淀收尾、pattern_learn把重复做法升为模式、检查自我进化闭环。",
     include_str!("../../system_skills/02_auto_evolve.md")),
    ("技能发现引擎", "何时用: 进入陌生领域或发现重复踩坑 — 检索社区技能库、fork适配为自己的、上报技能缺口。找'这类事怎么做'的方法论时用。",
     include_str!("../../system_skills/03_skill_discovery.md")),
    ("知识图谱导航", "何时用: 需要关联推理与多跳查询 — concepts看主题簇、knowledge_relations查邻接记忆、跨记忆溯源。查'X和Y是什么关系'时用。",
     include_str!("../../system_skills/04_knowledge_nav.md")),
    ("上下文管理", "何时用: 长会话或多任务切换 — doc_import把大文档入档、context_observe自动沉淀对话要点、防上下文溢出丢信息。",
     include_str!("../../system_skills/05_context_mgmt.md")),
    ("质量自控", "何时用: 任何'完成'声明之前 — self_rating四维自评、交付前验证清单、code-review式自查。交付质量把关时用。",
     include_str!("../../system_skills/06_quality_control.md")),
    ("系统全览", "何时用: 新agent首次接入或对系统能力迷路时 — space_stats看空间统计、身份确认流程、系统能力地图总览。",
     include_str!("../../system_skills/07_system_overview.md")),
    ("对话智能", "何时用: 与人类用户对话时 — 诚实原则、时间数据引用规范、记忆引用格式、拒绝过度承诺。",
     include_str!("../../system_skills/08_conversation.md")),
    ("执行器装配手册", "已废止(2026-08-29战略转向) — MCP握手是现行接入方式; 仅保留'未经用户明示同意不装后台组件'安全纪律。",
     include_str!("../../system_skills/09_executor_playbook.md")),
    ("Epicode 详细参考", "何时用: 遇到具体操作问题 — '怎么建子任务'/'怎么装Pulse'/'时间效性怎么用'/相位机/时间树/校准链全操作细节都在这里。",
     include_str!("../../system_skills/10_epicode_reference.md")),
];

fn content_hash(content: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub fn ensure_system_skills(engine: &SkillEngine) {
    // 紧急修复：启动时系统技能更新触发 persist()×8 全量重写 + reindex ONNX 死锁。
    // 用 EPICODE_SKIP_SKILL_SYNC=1 在启动时跳过更新（仅首次安装时安装缺失的）。
    // 技能内容更新改为运行时按需触发（管理员 API），不在启动路径做。
    let skip_update = std::env::var("EPICODE_SKIP_SKILL_SYNC").ok().as_deref() == Some("1");

    let existing = engine.list_system();
    let existing_by_name: std::collections::HashMap<String, &Skill> =
        existing.iter().map(|s| (s.name.clone(), s)).collect();
    let existing_by_id: std::collections::HashMap<u64, &Skill> =
        existing.iter().map(|s| (s.id, s)).collect();

    for (idx, (name, desc, md)) in SYSTEM_SKILLS.iter().enumerate() {
        let target_id = 900_000 + idx as u64;
        let seed_hash = content_hash(&format!("{}|{}", md, desc));

        if let Some(&existing_skill) = existing_by_name.get(*name) {
            let stored_hash = existing_skill.review_note.as_deref().unwrap_or("");
            if stored_hash != seed_hash {
                if skip_update {
                    tracing::info!(
                        "[SystemSkills] skip update '{}' (EPICODE_SKIP_SKILL_SYNC=1)",
                        name
                    );
                } else {
                    match engine.update(
                        existing_skill.id,
                        Some(md.to_string()),
                        Some("1.0.0".to_string()),
                    ) {
                        Ok(_) => {
                            let _ = engine.set_description(existing_skill.id, desc.to_string());
                            engine.set_system_review_note(
                                existing_skill.id,
                                format!("seed:{}", seed_hash),
                            );
                            tracing::info!(
                                "[SystemSkills] updated '{}' (id={}) — content/desc changed",
                                name,
                                existing_skill.id
                            );
                        }
                        Err(e) => {
                            tracing::warn!("[SystemSkills] failed to update '{}': {}", name, e)
                        }
                    }
                }
            } else {
                // hash未变但描述缺失(旧库) — 补描述不改hash语义
                if existing_skill.description.is_none() {
                    let _ = engine.set_description(existing_skill.id, desc.to_string());
                }
            }
            continue;
        }

        if existing_by_id.contains_key(&target_id) {
            tracing::warn!(
                "[SystemSkills] id {} occupied by different skill, skipping '{}'",
                target_id,
                name
            );
            continue;
        }

        let mut skill = system_skill(name, desc, md, target_id);
        skill.review_note = Some(format!("seed:{}", seed_hash));
        engine.insert_skill(skill);
        tracing::info!("[SystemSkills] installed '{}' (id={})", name, target_id);
    }

    // S2: 描述批量对齐(SKIP门控之外 — 精写触发描述对全部已装系统技能生效, 单persist防风暴)
    let desc_pairs: Vec<(u64, String)> = SYSTEM_SKILLS
        .iter()
        .filter_map(|(name, desc, _)| {
            existing_by_name
                .get(*name)
                .map(|s| (s.id, desc.to_string()))
        })
        .collect();
    let n = engine.backfill_descriptions(&desc_pairs);
    if n > 0 {
        tracing::info!("[SystemSkills] S2 backfilled {} trigger descriptions", n);
    }
}

/// α0.5fix: 运行时强制同步系统技能 (管理员 API 调用, 绕过 EPICODE_SKIP_SKILL_SYNC)
/// 用于源文件更新后把新内容推入 SkillEngine DB (如 playbook A2.5 序修正)
pub fn force_sync_system_skills(engine: &SkillEngine) {
    let existing = engine.list_system();
    let existing_by_name: std::collections::HashMap<String, &Skill> =
        existing.iter().map(|s| (s.name.clone(), s)).collect();
    for (idx, (name, desc, md)) in SYSTEM_SKILLS.iter().enumerate() {
        let target_id = 900_000 + idx as u64;
        let seed_hash = content_hash(&format!("{}|{}", md, desc));
        if let Some(&existing_skill) = existing_by_name.get(*name) {
            let stored_hash = existing_skill.review_note.as_deref().unwrap_or("");
            if stored_hash != seed_hash {
                match engine.update(
                    existing_skill.id,
                    Some(md.to_string()),
                    Some("1.0.0".to_string()),
                ) {
                    Ok(_) => {
                        let _ = engine.set_description(existing_skill.id, desc.to_string());
                        let _ = engine.set_system_review_note(
                            existing_skill.id,
                            format!("seed:{}", seed_hash),
                        );
                        tracing::info!(
                            "[SystemSkills] FORCE updated '{}' (id={})",
                            name,
                            existing_skill.id
                        );
                    }
                    Err(e) => {
                        tracing::warn!("[SystemSkills] force update '{}' failed: {}", name, e)
                    }
                }
            }
        } else {
            let _ = target_id;
            tracing::warn!("[SystemSkills] force sync: '{}' not found in engine, skip (install path not forced)", name);
        }
    }
}
