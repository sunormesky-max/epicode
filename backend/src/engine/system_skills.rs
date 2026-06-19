use super::skills::{Skill, SkillEngine};

fn system_skill(name: &str, skill_md: &str, id: u64) -> Skill {
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
        created_at: now,
        updated_at: now,
    }
}

const SYSTEM_SKILLS: &[(&str, &str)] = &[
    (
        "记忆智能存取",
        include_str!("../../system_skills/01_memory_io.md"),
    ),
    (
        "自动进化循环",
        include_str!("../../system_skills/02_auto_evolve.md"),
    ),
    (
        "技能发现引擎",
        include_str!("../../system_skills/03_skill_discovery.md"),
    ),
    (
        "知识图谱导航",
        include_str!("../../system_skills/04_knowledge_nav.md"),
    ),
    (
        "上下文管理",
        include_str!("../../system_skills/05_context_mgmt.md"),
    ),
    (
        "质量自控",
        include_str!("../../system_skills/06_quality_control.md"),
    ),
    (
        "系统全览",
        include_str!("../../system_skills/07_system_overview.md"),
    ),
    (
        "对话智能",
        include_str!("../../system_skills/08_conversation.md"),
    ),
];

pub fn ensure_system_skills(engine: &SkillEngine) {
    let existing = engine.list_system();
    let existing_names: std::collections::HashSet<String> =
        existing.iter().map(|s| s.name.clone()).collect();

    for (idx, (name, md)) in SYSTEM_SKILLS.iter().enumerate() {
        if existing_names.contains(*name) {
            continue;
        }
        let id = 900_000 + idx as u64;
        let skill = system_skill(name, md, id);
        engine.insert_skill(skill);
        tracing::info!("[SystemSkills] installed '{}' (id={})", name, id);
    }
}
