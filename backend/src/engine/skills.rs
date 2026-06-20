use crate::domain::tetra::TetraId;
use crate::engine::storage::StorageManager;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[derive(Default)]
pub enum ReviewStatus {
    #[default]
    Draft,
    PendingReview,
    Approved,
    Rejected,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub id: u64,
    pub name: String,
    pub skill_md: String,
    pub version: String,
    pub owner: String,
    #[serde(default)]
    pub is_public: bool,
    #[serde(default)]
    pub review_status: ReviewStatus,
    #[serde(default)]
    pub review_note: Option<String>,
    #[serde(default)]
    pub usage_count: u64,
    #[serde(default)]
    pub success_rate: f64,
    #[serde(default)]
    pub memory_ids: Vec<u64>,
    #[serde(default)]
    pub evolved_from: Option<u64>,
    #[serde(default)]
    pub is_system: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

pub struct SkillEngine {
    skills: Mutex<HashMap<u64, Skill>>,
    next_id: Mutex<u64>,
    storage: std::sync::Arc<StorageManager>,
}

impl SkillEngine {
    pub fn new(storage: std::sync::Arc<StorageManager>) -> Self {
        let engine = Self {
            skills: Mutex::new(HashMap::new()),
            next_id: Mutex::new(1),
            storage,
        };
        engine.load_from_storage();
        super::system_skills::ensure_system_skills(&engine);
        engine
    }

    fn load_from_storage(&self) {
        if let Some(data) = self.storage.get_meta("skills_data") {
            if let Ok(loaded) = serde_json::from_str::<Vec<Skill>>(&data) {
                let mut skills = self.skills.lock().expect("skills mutex poisoned during load");
                let mut next_id = self.next_id.lock().expect("next_id mutex poisoned during load");
                for mut s in loaded {
                    if s.id >= *next_id {
                        *next_id = s.id + 1;
                    }
                    if s.is_public && s.review_status == ReviewStatus::Draft {
                        s.review_status = ReviewStatus::Approved;
                    }
                    skills.insert(s.id, s);
                }
                tracing::info!("[SkillEngine] loaded {} skills from storage", skills.len());
            }
        }
    }

    fn persist(&self) {
        let skills = self.skills.lock().expect("skills mutex poisoned during persist");
        if let Ok(data) = serde_json::to_string(&skills.values().collect::<Vec<_>>()) {
            let _ = self.storage.set_meta("skills_data", &data);
        }
    }

    fn alloc_id(&self) -> u64 {
        let mut next_id = self.next_id.lock().expect("next_id mutex poisoned during alloc_id");
        let id = *next_id;
        *next_id += 1;
        id
    }

    pub fn create(&self, name: String, skill_md: String, owner: String) -> Skill {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let id = self.alloc_id();
        let skill = Skill {
            id,
            name,
            skill_md,
            version: "0.1.0".to_string(),
            owner,
            is_public: false,
            review_status: ReviewStatus::Draft,
            review_note: None,
            usage_count: 0,
            success_rate: 0.0,
            memory_ids: Vec::new(),
            evolved_from: None,
            is_system: false,
            created_at: now,
            updated_at: now,
        };
        let mut skills = self.skills.lock().expect("skills mutex poisoned in create");
        skills.insert(id, skill.clone());
        drop(skills);
        self.persist();
        tracing::info!("[SkillEngine] created skill '{}' (id={})", skill.name, id);
        skill
    }

    pub fn get(&self, id: u64) -> Option<Skill> {
        self.skills.lock().expect("skills mutex poisoned in get").get(&id).cloned()
    }

    pub fn list(&self, owner: Option<&str>) -> Vec<Skill> {
        let skills = self.skills.lock().expect("skills mutex poisoned in list");
        skills
            .values()
            .filter(|s| owner.is_none_or(|o| s.owner == o))
            .cloned()
            .collect()
    }

    pub fn list_public(&self) -> Vec<Skill> {
        self.skills
            .lock()
            .expect("skills mutex poisoned in list_public")
            .values()
            .filter(|s| s.is_public)
            .cloned()
            .collect()
    }

    pub fn list_system(&self) -> Vec<Skill> {
        self.skills
            .lock()
            .expect("skills mutex poisoned in list_system")
            .values()
            .filter(|s| s.is_system)
            .cloned()
            .collect()
    }

    pub fn update(
        &self,
        id: u64,
        skill_md: Option<String>,
        version: Option<String>,
    ) -> Result<Skill, String> {
        let mut skills = self.skills.lock().expect("skills mutex poisoned in update");
        let skill = skills.get_mut(&id).ok_or("skill not found")?;
        if let Some(md) = skill_md {
            skill.skill_md = md;
        }
        if let Some(v) = version {
            skill.version = v;
        }
        skill.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let updated = skill.clone();
        drop(skills);
        self.persist();
        Ok(updated)
    }

    pub fn append_description(&self, id: u64, description: &str) -> Result<(), String> {
        let mut skills = self.skills.lock().expect("skills mutex poisoned in append_description");
        let skill = skills.get_mut(&id).ok_or("skill not found")?;
        let desc_section = format!("\n\n## 中文描述\n\n{}", description);
        skill.skill_md.push_str(&desc_section);
        skill.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        drop(skills);
        self.persist();
        Ok(())
    }

    pub fn fork(&self, source: &Skill, new_owner: String) -> Skill {
        {
            let skills = self.skills.lock().expect("skills mutex poisoned in fork");
            if let Some(existing) = skills
                .values()
                .find(|s| s.evolved_from == Some(source.id) && s.owner == new_owner)
            {
                let dup = existing.clone();
                drop(skills);
                tracing::info!(
                    "[SkillEngine] fork dedup: '{}' already forked as id={}",
                    dup.name,
                    dup.id
                );
                return dup;
            }
        }
        let id = self.alloc_id();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let forked = Skill {
            id,
            name: source.name.clone(),
            skill_md: source.skill_md.clone(),
            version: source.version.clone(),
            owner: new_owner,
            is_public: false,
            review_status: ReviewStatus::Draft,
            review_note: None,
            usage_count: 0,
            success_rate: 0.0,
            memory_ids: Vec::new(),
            evolved_from: Some(source.id),
            is_system: false,
            created_at: now,
            updated_at: now,
        };
        let mut skills = self.skills.lock().expect("skills mutex poisoned in fork (insert)");
        skills.insert(id, forked.clone());
        drop(skills);
        self.persist();
        tracing::info!(
            "[SkillEngine] forked skill '{}' (id={}) from id={}",
            forked.name,
            id,
            source.id
        );
        forked
    }

    pub fn insert_skill(&self, skill: Skill) -> Skill {
        let id = if skill.id >= *self.next_id.lock().expect("next_id mutex poisoned in insert_skill (check)") {
            let mut next_id = self.next_id.lock().expect("next_id mutex poisoned in insert_skill (update)");
            *next_id = skill.id + 1;
            skill.id
        } else {
            self.alloc_id()
        };
        let s = Skill { id, ..skill };
        let mut skills = self.skills.lock().expect("skills mutex poisoned in insert_skill");
        skills.insert(id, s.clone());
        drop(skills);
        self.persist();
        tracing::info!("[SkillEngine] inserted skill '{}' (id={})", s.name, id);
        s
    }

    pub fn take(&self, id: u64) -> Option<Skill> {
        let mut skills = self.skills.lock().expect("skills mutex poisoned in take");
        let s = skills.remove(&id);
        drop(skills);
        if s.is_some() {
            self.persist();
            tracing::info!("[SkillEngine] took skill id={}", id);
        }
        s
    }

    pub fn delete(&self, id: u64) -> Result<(), String> {
        let mut skills = self.skills.lock().expect("skills mutex poisoned in delete");
        let skill = skills.get(&id).ok_or("skill not found")?;
        if skill.is_system {
            return Err("system skills cannot be deleted".to_string());
        }
        skills.remove(&id);
        drop(skills);
        self.persist();
        Ok(())
    }

    pub fn submit_for_review(&self, id: u64) -> Result<Skill, String> {
        let mut skills = self.skills.lock().expect("skills mutex poisoned in submit_for_review");
        let skill = skills.get_mut(&id).ok_or("skill not found")?;
        if skill.is_public || skill.review_status == ReviewStatus::PendingReview {
            return Err("skill already published or pending review".to_string());
        }
        if skill.skill_md.trim().is_empty() {
            return Err("skill content cannot be empty".to_string());
        }
        skill.review_status = ReviewStatus::PendingReview;
        skill.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let submitted = skill.clone();
        drop(skills);
        self.persist();
        tracing::info!(
            "[SkillEngine] skill '{}' (id={}) submitted for review",
            submitted.name,
            id
        );
        Ok(submitted)
    }

    pub fn approve_skill(&self, id: u64) -> Result<Skill, String> {
        let mut skills = self.skills.lock().expect("skills mutex poisoned in approve_skill");
        let skill = skills.get_mut(&id).ok_or("skill not found")?;
        if skill.review_status != ReviewStatus::PendingReview {
            return Err("skill is not pending review".to_string());
        }
        skill.review_status = ReviewStatus::Approved;
        skill.is_public = true;
        skill.review_note = None;
        skill.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let approved = skill.clone();
        drop(skills);
        self.persist();
        tracing::info!(
            "[SkillEngine] skill '{}' (id={}) approved and published",
            approved.name,
            id
        );
        Ok(approved)
    }

    pub fn reject_skill(&self, id: u64, reason: &str) -> Result<Skill, String> {
        let mut skills = self.skills.lock().expect("skills mutex poisoned in reject_skill");
        let skill = skills.get_mut(&id).ok_or("skill not found")?;
        if skill.review_status != ReviewStatus::PendingReview {
            return Err("skill is not pending review".to_string());
        }
        skill.review_status = ReviewStatus::Rejected;
        skill.review_note = Some(reason.to_string());
        skill.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let rejected = skill.clone();
        drop(skills);
        self.persist();
        tracing::info!(
            "[SkillEngine] skill '{}' (id={}) rejected: {}",
            rejected.name,
            id,
            reason
        );
        Ok(rejected)
    }

    pub fn security_check(skill: &Skill) -> Result<(), String> {
        let md = &skill.skill_md;
        let dangerous_patterns = [
            ("rm -rf", "dangerous shell command"),
            ("DROP TABLE", "SQL drop statement"),
            ("DELETE FROM", "SQL delete statement"),
            ("eval(", "code eval usage"),
            ("exec(", "code exec usage"),
            ("system(", "system command execution"),
            ("__import__", "Python import exploit"),
            ("Process(", "process spawn"),
            (".execute(", "command execution"),
        ];
        for (pattern, reason) in dangerous_patterns {
            if md.contains(pattern) {
                return Err(format!("security check failed: {}", reason));
            }
        }
        if md.len() > 10000 {
            return Err("skill content exceeds 10000 characters".to_string());
        }
        if skill.name.len() > 100 {
            return Err("skill name exceeds 100 characters".to_string());
        }
        if skill.name.trim().is_empty() {
            return Err("skill name cannot be empty".to_string());
        }
        Ok(())
    }

    pub fn review_pending(&self) -> Vec<Skill> {
        let skills = self.skills.lock().expect("skills mutex poisoned in review_pending");
        skills
            .values()
            .filter(|s| s.review_status == ReviewStatus::PendingReview)
            .cloned()
            .collect()
    }

    pub fn link_memory(&self, skill_id: u64, memory_id: TetraId) -> Result<(), String> {
        let mut skills = self.skills.lock().expect("skills mutex poisoned in link_memory");
        let skill = skills.get_mut(&skill_id).ok_or("skill not found")?;
        if !skill.memory_ids.contains(&memory_id) {
            skill.memory_ids.push(memory_id);
        }
        drop(skills);
        self.persist();
        Ok(())
    }

    pub fn purge_non_system(&self) -> usize {
        let mut skills = self.skills.lock().expect("skills mutex poisoned in purge_non_system");
        let before = skills.len();
        skills.retain(|_, s| s.is_system);
        let removed = before - skills.len();
        drop(skills);
        if removed > 0 {
            self.persist();
            tracing::info!("[SkillEngine] purged {} non-system skills", removed);
        }
        removed
    }

    pub fn match_skills(&self, query: &str, owner: &str, limit: usize) -> Vec<Skill> {
        let skills = self.skills.lock().expect("skills mutex poisoned in match_skills");
        let query_lower = query.to_lowercase();
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();

        let mut scored: Vec<(f64, &Skill)> = skills
            .values()
            .filter(|s| s.owner == owner || s.is_public)
            .map(|s| {
                let name_lower = s.name.to_lowercase();
                let md_lower = s.skill_md.to_lowercase();

                let name_match = query_words
                    .iter()
                    .filter(|w| name_lower.contains(*w))
                    .count() as f64;
                let md_match = query_words.iter().filter(|w| md_lower.contains(*w)).count() as f64;
                let usage_bonus = (s.usage_count as f64).ln_1p() * 0.1;
                let success_bonus = s.success_rate * 0.2;
                let score = name_match * 2.0 + md_match * 1.0 + usage_bonus + success_bonus;
                (score, s)
            })
            .filter(|(score, _)| *score > 0.0)
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored
            .into_iter()
            .take(limit)
            .map(|(_, s)| s.clone())
            .collect()
    }
}
