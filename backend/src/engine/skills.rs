use crate::domain::tetra::TetraId;
use crate::engine::hnsw::HnswIndex;
use crate::engine::storage::StorageManager;
use crate::engine::vector::{VectorLayer, EMBEDDING_DIM};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::Mutex;

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
    #[serde(default)]
    pub category: Option<String>,
    /// S2: 触发描述 — 第一层渐进披露常驻上下文(写"何时用"而非"是什么", ≤120字)
    #[serde(default)]
    pub description: Option<String>,
    /// S2: 触发场景词(语义触发索引组成部分, ≤8个)
    #[serde(default)]
    pub triggers: Vec<String>,
    /// S2: 曝光计数(被自动注入面推荐但未被skill_get取用 — 描述优化实证数据)
    #[serde(default)]
    pub surface_impressions: u64,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub produces: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

pub struct SkillEngine {
    skills: Mutex<HashMap<u64, Skill>>,
    next_id: Mutex<u64>,
    storage: Arc<StorageManager>,
    vector: Mutex<Option<Arc<VectorLayer>>>,
    index: Mutex<HnswIndex>,
    embeddings: Mutex<HashMap<u64, Vec<f64>>>,
}

impl SkillEngine {
    pub fn new(storage: Arc<StorageManager>) -> Self {
        let engine = Self {
            skills: Mutex::new(HashMap::new()),
            next_id: Mutex::new(1),
            storage,
            vector: Mutex::new(None),
            index: Mutex::new(HnswIndex::new(EMBEDDING_DIM, 16, 200)),
            embeddings: Mutex::new(HashMap::new()),
        };
        engine.load_from_storage();
        super::system_skills::ensure_system_skills(&engine);
        engine
    }

    fn load_from_storage(&self) {
        if let Some(data) = self.storage.get_meta("skills_data") {
            if let Ok(loaded) = serde_json::from_str::<Vec<Skill>>(&data) {
                let mut skills = self.skills.lock();
                let mut next_id = self.next_id.lock();
                let mut migrated = 0usize;
                for mut s in loaded {
                    if s.id >= *next_id {
                        *next_id = s.id + 1;
                    }
                    if s.is_public && s.review_status == ReviewStatus::Draft {
                        s.review_status = ReviewStatus::Approved;
                    }
                    // S2迁移: 旧技能补触发描述(系统技能的精写值由ensure_system_skills覆盖)
                    if s.description.is_none() {
                        let first_para: String = s.skill_md.lines()
                            .map(|l| l.trim())
                            .find(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("---"))
                            .unwrap_or("").chars().take(100).collect();
                        s.description = Some(if first_para.is_empty() {
                            s.name.clone()
                        } else {
                            format!("{} — {}", s.name, first_para)
                        });
                        migrated += 1;
                    }
                    skills.insert(s.id, s);
                }
                tracing::info!("[SkillEngine] loaded {} skills from storage (S2 migrated {} descriptions)", skills.len(), migrated);
            }
        }
    }

    fn persist(&self) {
        let skills = self.skills.lock();
        if let Ok(data) = serde_json::to_string(&skills.values().collect::<Vec<_>>()) {
            let _ = self.storage.set_meta("skills_data", &data);
        }
    }

    fn alloc_id(&self) -> u64 {
        let mut next_id = self.next_id.lock();
        let id = *next_id;
        *next_id += 1;
        id
    }

    pub fn set_vector(&self, vector: Arc<VectorLayer>) {
        {
            let mut v = self.vector.lock();
            *v = Some(vector);
        }
        self.rebuild_index();
        tracing::info!("[SkillEngine] vector layer injected, index rebuilt");
    }

    pub fn has_vector(&self) -> bool {
        self.vector.lock().is_some()
    }

    fn rebuild_index(&self) {
        let vector = match self.vector.lock().clone() {
            Some(v) => v,
            None => return,
        };
        // 阶段4修复:clone skills 后立即释放锁,HTTP embedding 调用在无锁状态做
        let skills_snapshot: Vec<Skill> = {
            let skills = self.skills.lock();
            skills.values().cloned().collect()
        };
        // 在无锁状态下做 embedding（可能涉及数百次 HTTP 调用）
        let mut new_index = HnswIndex::new(EMBEDDING_DIM, 16, 200);
        let mut new_embeddings: HashMap<u64, Vec<f64>> = HashMap::new();
        for skill in &skills_snapshot {
            let text = skill_text_for_embed(skill);
            if let Ok(emb) = vector.embed_passage(&text) {
                new_index.insert(skill.id, emb.clone());
                new_embeddings.insert(skill.id, emb);
            }
        }
        // 最后一次性拿锁写入结果
        let count = new_embeddings.len();
        *self.index.lock() = new_index;
        *self.embeddings.lock() = new_embeddings;
        tracing::info!("[SkillEngine] index rebuilt: {} skills indexed", count);
    }

    fn reindex_skill(&self, id: u64) {
        let vector = match self.vector.lock().clone() {
            Some(v) => v,
            None => return,
        };
        let skills = self.skills.lock();
        let skill = match skills.get(&id) {
            Some(s) => s.clone(),
            None => return,
        };
        drop(skills);
        let text = skill_text_for_embed(&skill);
        if let Ok(emb) = vector.embed_passage(&text) {
            let mut index = self.index.lock();
            let mut embeddings = self.embeddings.lock();
            if !index.is_empty() {
                index.remove(id);
            }
            index.insert(id, emb.clone());
            embeddings.insert(id, emb);
        }
    }

    fn remove_from_index(&self, id: u64) {
        let mut index = self.index.lock();
        let mut embeddings = self.embeddings.lock();
        index.remove(id);
        embeddings.remove(&id);
    }

    /// S2: 曝光计数(注入面批量计数; task_start低频调用, persist开销可接受)
    pub fn increment_impressions(&self, ids: &[u64]) {
        if ids.is_empty() { return; }
        {
            let mut skills = self.skills.lock();
            for &id in ids {
                if let Some(s) = skills.get_mut(&id) {
                    s.surface_impressions += 1;
                }
            }
        }
        self.persist();
    }

    /// S2: 系统技能描述批量对齐 — 单次persist+只reindex变化项(引擎加载时vector未注入, reindex为no-op,
    /// 全量索引由set_vector→rebuild_index用新嵌入文本重建; 不受SKIP_SKILL_SYNC门控, 描述是短文本无风暴风险)
    pub fn backfill_descriptions(&self, pairs: &[(u64, String)]) -> usize {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
        let mut changed: Vec<u64> = Vec::new();
        {
            let mut skills = self.skills.lock();
            for (id, desc) in pairs {
                if let Some(s) = skills.get_mut(id) {
                    if s.description.as_deref() != Some(desc.as_str()) {
                        s.description = Some(desc.clone());
                        s.updated_at = now;
                        changed.push(*id);
                    }
                }
            }
        }
        if changed.is_empty() { return 0; }
        self.persist();
        for id in &changed { self.reindex_skill(*id); }
        changed.len()
    }

    /// S2: 设置触发场景词
    pub fn set_triggers(&self, id: u64, triggers: Vec<String>) -> Result<(), String> {
        {
            let mut skills = self.skills.lock();
            match skills.get_mut(&id) {
                Some(s) => s.triggers = triggers,
                None => return Err("skill not found".into()),
            }
        }
        self.persist();
        self.reindex_skill(id);
        Ok(())
    }

    /// S2: 全库内容版本戳(实时更新感知 — 握手/开工比对, 变更即提示重sync)
    pub fn content_version(&self) -> String {
        use std::hash::{Hash, Hasher};
        let skills = self.skills.lock();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let mut ids: Vec<_> = skills.keys().copied().collect();
        ids.sort();
        for id in ids {
            if let Some(s) = skills.get(&id) {
                (id, &s.name, &s.version, s.updated_at, &s.skill_md).hash(&mut h);
            }
        }
        drop(skills);
        format!("{:016x}", h.finish())
    }

    /// S2: 设置触发描述(系统技能精写值落地 + 用户编辑器)
    pub fn set_description(&self, id: u64, description: String) -> Result<(), String> {
        {
            let mut skills = self.skills.lock();
            match skills.get_mut(&id) {
                Some(s) => {
                    s.description = Some(description);
                    s.updated_at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
                }
                None => return Err("skill not found".into()),
            }
        }
        self.persist();
        self.reindex_skill(id);
        Ok(())
    }

    /// S2: 带分数的语义匹配(自动触发注入面: 分数=阈值过滤与曝光排序依据)
    pub fn match_skills_scored(&self, query: &str, owner: &str, limit: usize) -> Vec<(Skill, f64)> {
        let vector = match self.vector.lock().clone() { Some(v) => v, None => return Vec::new() };
        let q_emb = match vector.embed(query) {
            Ok(e) if e.len() == super::vector::EMBEDDING_DIM => e,
            _ => return Vec::new(),
        };
        let candidates = self.index.lock().search_knn(&q_emb, limit * 3, 50);
        let skills = self.skills.lock();
        let mut scored: Vec<(f64, &Skill)> = Vec::new();
        for (id, cosine) in candidates {
            if let Some(skill) = skills.get(&id) {
                // R2a: 废止技能不进自动触发面(仍可skill_get显式取)
                if skill.description.as_deref().map_or(false, |d| d.starts_with("已废止")) { continue; }
                if skill.owner == owner || skill.owner == "__system__" || skill.is_public {
                    let usage_bonus = (skill.usage_count as f64).ln_1p() * 0.1;
                    let final_score = cosine * 0.85 + usage_bonus * 0.15;
                    scored.push((final_score, skill));
                }
            }
        }
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter().take(limit)
            .map(|(s, sk)| (sk.clone(), (s * 1000.0).round() / 1000.0))
            .collect()
    }

    pub fn increment_usage(&self, id: u64) {
        let mut skills = self.skills.lock();
        if let Some(skill) = skills.get_mut(&id) {
            skill.usage_count += 1;
        }
        drop(skills);
        // 阶段4修复:热路径不立即persist(O(N)全量序列化254技能)。
        // usage_count是统计指标,非关键数据。下次create/update/purge时会被持久化。
    }

    pub fn record_feedback(&self, id: u64, helpful: bool) -> Result<(), String> {
        let mut skills = self.skills.lock();
        let skill = skills.get_mut(&id).ok_or("skill not found")?;
        let score = if helpful { 1.0 } else { 0.0 };
        let total = skill.usage_count.max(1) as f64;
        skill.success_rate = (skill.success_rate * total + score) / (total + 1.0);
        drop(skills);
        self.persist();
        Ok(())
    }

    pub fn set_system_review_note(&self, id: u64, note: String) {
        let mut skills = self.skills.lock();
        if let Some(skill) = skills.get_mut(&id) {
            skill.review_note = Some(note);
        }
        drop(skills);
        self.persist();
    }

    pub fn create(&self, name: String, skill_md: String, owner: String) -> Skill {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let id = self.alloc_id();
        let (category, requires, produces, capabilities) = parse_frontmatter_fields(&skill_md);
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
            category,
            description: None,
            triggers: Vec::new(),
            surface_impressions: 0,
            requires,
            produces,
            capabilities,
            created_at: now,
            updated_at: now,
        };
        let mut skills = self.skills.lock();
        skills.insert(id, skill.clone());
        drop(skills);
        self.persist();
        self.reindex_skill(id);
        tracing::info!("[SkillEngine] created skill '{}' (id={})", skill.name, id);
        skill
    }

    pub fn get(&self, id: u64) -> Option<Skill> {
        self.skills.lock().get(&id).cloned()
    }

    pub fn list(&self, owner: Option<&str>) -> Vec<Skill> {
        let skills = self.skills.lock();
        skills
            .values()
            .filter(|s| owner.is_none_or(|o| s.owner == o))
            .cloned()
            .collect()
    }

    pub fn list_public(&self) -> Vec<Skill> {
        self.skills
            .lock()
            .values()
            .filter(|s| s.is_public && (s.review_status == ReviewStatus::Approved || s.is_system))
            .cloned()
            .collect()
    }

    pub fn list_system(&self) -> Vec<Skill> {
        self.skills
            .lock()
            .values()
            .filter(|s| s.is_system)
            .cloned()
            .collect()
    }

    pub fn update(&self, id: u64, skill_md: Option<String>, version: Option<String>) -> Result<Skill, String> {
        let mut skills = self.skills.lock();
        let skill = skills.get_mut(&id).ok_or("skill not found")?;
        if let Some(md) = skill_md {
            let (category, requires, produces, capabilities) = parse_frontmatter_fields(&md);
            skill.skill_md = md;
            skill.category = category;
            skill.requires = requires;
            skill.produces = produces;
            skill.capabilities = capabilities;
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
        self.reindex_skill(id);
        Ok(updated)
    }

    pub fn append_description(&self, id: u64, description: &str) -> Result<(), String> {
        let mut skills = self.skills.lock();
        let skill = skills.get_mut(&id).ok_or("skill not found")?;
        let desc_section = format!("\n\n## 中文描述\n\n{}", description);
        skill.skill_md.push_str(&desc_section);
        skill.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        drop(skills);
        self.persist();
        self.reindex_skill(id);
        Ok(())
    }

    pub fn fork(&self, source: &Skill, new_owner: String) -> Skill {
        {
            let skills = self.skills.lock();
            if let Some(existing) = skills.values().find(|s| s.evolved_from == Some(source.id) && s.owner == new_owner) {
                let dup = existing.clone();
                drop(skills);
                tracing::info!("[SkillEngine] fork dedup: '{}' already forked as id={}", dup.name, dup.id);
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
            category: source.category.clone(),
            description: source.description.clone(),
            triggers: source.triggers.clone(),
            surface_impressions: 0,
            requires: source.requires.clone(),
            produces: source.produces.clone(),
            capabilities: source.capabilities.clone(),
            created_at: now,
            updated_at: now,
        };
        let mut skills = self.skills.lock();
        skills.insert(id, forked.clone());
        drop(skills);
        self.persist();
        self.reindex_skill(id);
        tracing::info!("[SkillEngine] forked skill '{}' (id={}) from id={}", forked.name, id, source.id);
        forked
    }

    pub fn insert_skill(&self, skill: Skill) -> Skill {
        let id = if skill.id >= *self.next_id.lock() {
            let mut next_id = self.next_id.lock();
            *next_id = skill.id + 1;
            skill.id
        } else {
            self.alloc_id()
        };
        let s = Skill { id, ..skill };
        let mut skills = self.skills.lock();
        skills.insert(id, s.clone());
        drop(skills);
        self.persist();
        self.reindex_skill(id);
        tracing::info!("[SkillEngine] inserted skill '{}' (id={})", s.name, id);
        s
    }

    pub fn take(&self, id: u64) -> Option<Skill> {
        let mut skills = self.skills.lock();
        let s = skills.remove(&id);
        drop(skills);
        if s.is_some() {
            self.remove_from_index(id);
            self.persist();
            tracing::info!("[SkillEngine] took skill id={}", id);
        }
        s
    }

    pub fn delete(&self, id: u64) -> Result<(), String> {
        let mut skills = self.skills.lock();
        let skill = skills.get(&id).ok_or("skill not found")?;
        if skill.is_system {
            return Err("system skills cannot be deleted".to_string());
        }
        skills.remove(&id);
        drop(skills);
        self.remove_from_index(id);
        self.persist();
        Ok(())
    }

    pub fn submit_for_review(&self, id: u64) -> Result<Skill, String> {
        let mut skills = self.skills.lock();
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
        tracing::info!("[SkillEngine] skill '{}' (id={}) submitted for review", submitted.name, id);
        Ok(submitted)
    }

    pub fn approve_skill(&self, id: u64) -> Result<Skill, String> {
        let mut skills = self.skills.lock();
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
        tracing::info!("[SkillEngine] skill '{}' (id={}) approved and published", approved.name, id);
        Ok(approved)
    }

    pub fn reject_skill(&self, id: u64, reason: &str) -> Result<Skill, String> {
        let mut skills = self.skills.lock();
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
        tracing::info!("[SkillEngine] skill '{}' (id={}) rejected: {}", rejected.name, id, reason);
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
        let skills = self.skills.lock();
        skills.values()
            .filter(|s| s.review_status == ReviewStatus::PendingReview)
            .cloned()
            .collect()
    }

    pub fn link_memory(&self, skill_id: u64, memory_id: TetraId) -> Result<(), String> {
        let mut skills = self.skills.lock();
        let skill = skills.get_mut(&skill_id).ok_or("skill not found")?;
        if !skill.memory_ids.contains(&memory_id) {
            skill.memory_ids.push(memory_id);
        }
        drop(skills);
        // 阶段4:record_feedback保留persist(影响排序),但可考虑后续改为debounce
        self.persist();
        Ok(())
    }

    pub fn purge_non_system(&self) -> usize {
        let mut skills = self.skills.lock();
        let to_remove: Vec<u64> = skills.iter()
            .filter(|(_, s)| !s.is_system)
            .map(|(id, _)| *id)
            .collect();
        for id in &to_remove {
            skills.remove(id);
        }
        drop(skills);
        if !to_remove.is_empty() {
            let mut index = self.index.lock();
            let mut embeddings = self.embeddings.lock();
            for id in &to_remove {
                index.remove(*id);
                embeddings.remove(id);
            }
            self.persist();
            tracing::info!("[SkillEngine] purged {} non-system skills (index cleaned)", to_remove.len());
        }
        to_remove.len()
    }

    pub fn match_skills(&self, query: &str, owner: &str, limit: usize) -> Vec<Skill> {
        if let Some(result) = self.match_skills_semantic(query, owner, limit) {
            return result;
        }
        self.match_skills_keyword(query, owner, limit)
    }

    fn match_skills_semantic(&self, query: &str, owner: &str, limit: usize) -> Option<Vec<Skill>> {
        let vector = self.vector.lock().clone()?;
        let q_emb = vector.embed(query).ok()?;
        let candidates = self.index.lock().search_knn(&q_emb, limit * 3, 50);
        let skills = self.skills.lock();
        let mut scored: Vec<(f64, &Skill)> = Vec::new();
        for (id, cosine) in candidates {
            if let Some(skill) = skills.get(&id) {
                // R2a: 废止技能不进语义匹配面(REST search/模拟器/技能发现共用此路)
                if skill.description.as_deref().map_or(false, |d| d.starts_with("已废止")) { continue; }
                if skill.owner == owner || skill.is_public {
                    let usage_bonus = (skill.usage_count as f64).ln_1p() * 0.1;
                    let feedback_score = skill.success_rate * 0.5 + usage_bonus;
                    let final_score = cosine * 0.7 + feedback_score * 0.3;
                    scored.push((final_score, skill));
                }
            }
        }
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        Some(scored.into_iter().take(limit).map(|(_, s)| s.clone()).collect())
    }

    fn match_skills_keyword(&self, query: &str, owner: &str, limit: usize) -> Vec<Skill> {
        let skills = self.skills.lock();
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
                let md_match = query_words
                    .iter()
                    .filter(|w| md_lower.contains(*w))
                    .count() as f64;
                let usage_bonus = (s.usage_count as f64).ln_1p() * 0.1;
                let success_bonus = s.success_rate * 0.2;
                let score = name_match * 2.0 + md_match * 1.0 + usage_bonus + success_bonus;
                (score, s)
            })
            .filter(|(score, _)| *score > 0.0)
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter().take(limit).map(|(_, s)| s.clone()).collect()
    }

}

fn skill_text_for_embed(skill: &Skill) -> String {
    // S2: 触发语义索引 = 名称+触发描述+场景词(对齐"何时用"的匹配, 不再嵌正文全文)
    let mut text = skill.name.clone();
    if let Some(ref cat) = skill.category {
        text.push(' ');
        text.push_str(cat);
    }
    if let Some(ref d) = skill.description {
        text.push(' ');
        text.push_str(&d.chars().take(160).collect::<String>());
    }
    if !skill.triggers.is_empty() {
        text.push(' ');
        text.push_str(&skill.triggers.join(" "));
    }
    text
}

fn parse_frontmatter_fields(skill_md: &str) -> (Option<String>, Vec<String>, Vec<String>, Vec<String>) {
    let content = skill_md.trim();
    if !content.starts_with("---") {
        return (None, Vec::new(), Vec::new(), Vec::new());
    }
    let rest = &content[3..];
    let end = match rest.find("---") {
        Some(i) => i,
        None => return (None, Vec::new(), Vec::new(), Vec::new()),
    };
    let yaml = &rest[..end];
    let mut category = None;
    let mut requires = Vec::new();
    let mut produces = Vec::new();
    let mut capabilities = Vec::new();
    let mut current_list_target: u8 = 0;
    for line in yaml.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("category:") {
            category = Some(val.trim().trim_matches('"').trim_matches('\'').to_string());
            current_list_target = 0;
        } else if let Some(val) = line.strip_prefix("requires:") {
            let parsed = parse_yaml_list(val);
            if !parsed.is_empty() {
                requires = parsed;
                current_list_target = 0;
            } else {
                current_list_target = 1;
            }
        } else if let Some(val) = line.strip_prefix("produces:") {
            let parsed = parse_yaml_list(val);
            if !parsed.is_empty() {
                produces = parsed;
                current_list_target = 0;
            } else {
                current_list_target = 2;
            }
        } else if let Some(val) = line.strip_prefix("capabilities:") {
            let parsed = parse_yaml_list(val);
            if !parsed.is_empty() {
                capabilities = parsed;
                current_list_target = 0;
            } else {
                current_list_target = 3;
            }
        } else if line.starts_with("- ") {
            let val = line.trim_start_matches("- ").trim().trim_matches('"').trim_matches('\'').to_string();
            if !val.is_empty() {
                match current_list_target {
                    1 => requires.push(val),
                    2 => produces.push(val),
                    3 => capabilities.push(val),
                    _ => {}
                }
            }
        } else if !line.is_empty() {
            current_list_target = 0;
        }
    }
    (category, requires, produces, capabilities)
}

fn parse_yaml_list(val: &str) -> Vec<String> {
    let val = val.trim();
    if val.starts_with('[') && val.ends_with(']') {
        val[1..val.len()-1]
            .split(',')
            .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        Vec::new()
    }
}
