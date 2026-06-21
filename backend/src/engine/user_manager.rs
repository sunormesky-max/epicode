use std::collections::HashMap;
use std::sync::Arc;

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, SaltString};
use argon2::{Algorithm, Argon2, Params, PasswordHasher, PasswordVerifier, Version};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use super::crypto::constant_time_eq;
use super::vector::VectorLayer;
use super::Engine;

fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    let params =
        Params::new(65536, 3, 4, Some(32)).map_err(|e| format!("invalid argon2 params: {e}"))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    argon2
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| format!("argon2 hash failed: {e}"))
}

fn verify_password(password: &str, stored: &str) -> bool {
    if stored.is_empty() {
        return false;
    }
    let Ok(parsed_hash) = PasswordHash::new(stored) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
}

const MAX_USERS: usize = 1000;
const IDLE_TIMEOUT_SECS: u64 = 3600;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub user_id: String,
    pub api_key: String,
    #[serde(default)]
    pub password_hash: String,
    pub plan: UserPlan,
    pub max_memories: usize,
    pub memories_used: usize,
    pub created_at: i64,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub sub_accounts: Vec<String>,
    #[serde(default)]
    pub is_admin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UserPlan {
    Free,
    Pro,
    Enterprise,
}

impl UserPlan {
    pub fn max_memories(&self) -> usize {
        match self {
            UserPlan::Free => 1000,
            UserPlan::Pro => 10000,
            UserPlan::Enterprise => 100000,
        }
    }

    pub fn max_embedding_dims(&self) -> usize {
        768
    }
}

pub struct UserSlot {
    pub engine: Arc<Engine>,
    pub last_access: std::time::Instant,
}

pub struct UserManager {
    slots: RwLock<HashMap<String, UserSlot>>,
    users_db: RwLock<HashMap<String, UserInfo>>,
    base_data_dir: std::path::PathBuf,
    shared_vector: Option<Arc<VectorLayer>>,
    invite_code: RwLock<String>,
    used_codes: RwLock<Vec<String>>,
    pending_codes: RwLock<Vec<String>>,
    last_backup: RwLock<std::time::Instant>,
    meta_crypto: Option<super::crypto::CryptoEngine>,
    pub_skills: RwLock<Option<Arc<super::skills::SkillEngine>>>,
}

impl UserManager {
    pub fn new(base_data_dir: &std::path::Path) -> Self {
        let (invite, used) = Self::load_invite_state(base_data_dir);
        let meta_crypto = super::crypto::CryptoEngine::from_env().ok();
        Self {
            slots: RwLock::new(HashMap::new()),
            users_db: RwLock::new(Self::load_users_db(base_data_dir)),
            base_data_dir: base_data_dir.to_path_buf(),
            shared_vector: None,
            invite_code: RwLock::new(invite),
            used_codes: RwLock::new(used),
            pending_codes: RwLock::new(Self::load_pending_codes(base_data_dir)),
            last_backup: RwLock::new(std::time::Instant::now()),
            meta_crypto,
            pub_skills: RwLock::new(None),
        }
    }

    pub fn with_shared_vector(base_data_dir: &std::path::Path, vector: Arc<VectorLayer>) -> Self {
        let (invite, used) = Self::load_invite_state(base_data_dir);
        let meta_crypto = super::crypto::CryptoEngine::from_env().ok();
        Self {
            slots: RwLock::new(HashMap::new()),
            users_db: RwLock::new(Self::load_users_db(base_data_dir)),
            base_data_dir: base_data_dir.to_path_buf(),
            shared_vector: Some(vector),
            invite_code: RwLock::new(invite),
            used_codes: RwLock::new(used),
            pending_codes: RwLock::new(Self::load_pending_codes(base_data_dir)),
            last_backup: RwLock::new(std::time::Instant::now()),
            meta_crypto,
            pub_skills: RwLock::new(None),
        }
    }

    fn generate_invite_code() -> String {
        use rand::RngExt;
        let chars: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789";
        let mut rng = rand::rng();
        (0..32)
            .map(|_| chars[rng.random_range(0..chars.len())] as char)
            .collect()
    }

    fn load_invite_state(base_data_dir: &std::path::Path) -> (String, Vec<String>) {
        let path = base_data_dir.join("invite_state.json");
        if path.exists() {
            if let Ok(raw) = std::fs::read_to_string(&path) {
                let meta_crypto = super::crypto::CryptoEngine::from_env().ok();
                let data = if let Some(ref crypto) = meta_crypto {
                    match serde_json::from_str::<serde_json::Value>(&raw) {
                        Ok(v) if v.get("__enc").is_some() => {
                            match crypto
                                .decrypt_content(v["__enc"].as_str().unwrap_or(""), "__invite__")
                            {
                                Ok(dec) => dec,
                                Err(_) => raw,
                            }
                        }
                        _ => raw,
                    }
                } else {
                    raw
                };
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
                    let code = v["current"].as_str().unwrap_or("").to_string();
                    let used = v["used"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| x.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    if !code.is_empty() {
                        return (code, used);
                    }
                }
            }
        }
        let code = Self::generate_invite_code();
        tracing::info!("[UserManager] generated initial invite code");
        (code, vec![])
    }

    fn load_pending_codes(base_data_dir: &std::path::Path) -> Vec<String> {
        let path = base_data_dir.join("invite_state.json");
        if path.exists() {
            if let Ok(raw) = std::fs::read_to_string(&path) {
                let meta_crypto = super::crypto::CryptoEngine::from_env().ok();
                let data = if let Some(ref crypto) = meta_crypto {
                    match serde_json::from_str::<serde_json::Value>(&raw) {
                        Ok(v) if v.get("__enc").is_some() => {
                            match crypto
                                .decrypt_content(v["__enc"].as_str().unwrap_or(""), "__invite__")
                            {
                                Ok(dec) => dec,
                                Err(_) => raw,
                            }
                        }
                        _ => raw,
                    }
                } else {
                    raw
                };
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
                    return v["pending"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| x.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                }
            }
        }
        vec![]
    }

    fn save_invite_state(&self) {
        let path = self.base_data_dir.join("invite_state.json");
        let code = self.invite_code.read().clone();
        let used = self.used_codes.read().clone();
        let pending = self.pending_codes.read().clone();
        let payload = serde_json::json!({"current": code, "used": used, "pending": pending});
        let output = if let Some(ref crypto) = self.meta_crypto {
            match crypto.encrypt_content(
                &serde_json::to_string(&payload).unwrap_or_default(),
                "__invite__",
            ) {
                Ok(enc) => serde_json::json!({"__enc": enc}).to_string(),
                Err(_) => serde_json::to_string_pretty(&payload).unwrap_or_default(),
            }
        } else {
            serde_json::to_string_pretty(&payload).unwrap_or_default()
        };
        if let Err(e) = std::fs::write(&path, &output) {
            tracing::warn!("[UserManager] failed to save invite state: {}", e);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
    }

    pub fn use_invite_code(&self, code: &str) -> Result<(), String> {
        {
            let mut pending = self.pending_codes.write();
            if let Some(pos) = pending.iter().position(|c| constant_time_eq(code, c)) {
                let used_code = pending.remove(pos);
                self.used_codes.write().push(used_code);
                drop(pending);
                self.save_invite_state();
                tracing::info!(
                    "[UserManager] pending invite code used, {} remaining",
                    self.pending_codes.read().len()
                );
                return Ok(());
            }
        }

        let current = self.invite_code.read();
        if !constant_time_eq(code, &current) {
            return Err("invalid or expired invite code".into());
        }
        drop(current);

        let old = self.invite_code.read().clone();
        self.used_codes.write().push(old);
        let new_code = Self::generate_invite_code();
        *self.invite_code.write() = new_code.clone();
        self.save_invite_state();
        tracing::info!("[UserManager] invite code used, rotated new code");
        Ok(())
    }

    pub fn generate_batch_codes(&self, count: usize) -> Vec<String> {
        let mut codes = Vec::with_capacity(count);
        for _ in 0..count {
            codes.push(Self::generate_invite_code());
        }
        self.pending_codes.write().extend(codes.clone());
        self.save_invite_state();
        tracing::info!("[UserManager] generated {} batch invite codes", count);
        codes
    }

    pub fn all_invite_codes(&self) -> Vec<String> {
        let current = self.invite_code.read().clone();
        let pending = self.pending_codes.read().clone();
        let mut all = vec![current];
        all.extend(pending);
        all
    }

    pub fn current_invite_code(&self) -> String {
        let pending = self.pending_codes.read();
        if let Some(code) = pending.first() {
            return code.clone();
        }
        drop(pending);
        let mut new_codes = Vec::new();
        let code = Self::generate_invite_code();
        new_codes.push(code.clone());
        *self.pending_codes.write() = new_codes;
        self.save_invite_state();
        code
    }

    pub fn register(
        &self,
        user_id: &str,
        api_key: &str,
        plan: UserPlan,
        password: &str,
    ) -> Result<UserInfo, String> {
        if password.len() < 6 {
            return Err("password must be at least 8 characters".into());
        }
        if password.len() > 128 {
            return Err("password must be under 128 characters".into());
        }
        let mut db = self.users_db.write();
        if db.contains_key(user_id) {
            return Err("user already exists".into());
        }
        if db.values().any(|u| constant_time_eq(&u.api_key, api_key)) {
            return Err("api key already in use".into());
        }
        if db.len() >= MAX_USERS {
            return Err("user limit reached".into());
        }
        let max_mem = plan.max_memories();
        let password_hash =
            hash_password(password).map_err(|e| format!("failed to hash password: {e}"))?;
        let info = UserInfo {
            user_id: user_id.to_string(),
            api_key: api_key.to_string(),
            password_hash,
            plan,
            max_memories: max_mem,
            memories_used: 0,
            created_at: chrono::Utc::now().timestamp(),
            parent: None,
            sub_accounts: Vec::new(),
            is_admin: false,
        };
        db.insert(user_id.to_string(), info.clone());
        let snapshot = db.clone();
        drop(db);
        if let Err(e) = self.save_users_db(&snapshot) {
            let mut db = self.users_db.write();
            db.remove(user_id);
            return Err(format!("failed to persist registration: {e}"));
        }
        tracing::info!(
            "[UserManager] registered user {} plan={:?} max_memories={}",
            user_id,
            info.plan,
            max_mem
        );
        Ok(info)
    }

    pub fn authenticate(&self, api_key: &str) -> Option<UserInfo> {
        let db = self.users_db.read();
        let found = db
            .values()
            .find(|u| constant_time_eq(&u.api_key, api_key))
            .cloned();
        if found.is_none() {
            tracing::debug!(
                "[UserManager] auth failed for key prefix {}",
                &api_key.get(..2.min(api_key.len())).unwrap_or("")
            );
        }
        found
    }

    pub fn login(&self, user_id: &str, password: &str) -> Result<UserInfo, String> {
        let db = self.users_db.read();
        let info = db.get(user_id).ok_or("user not found")?.clone();
        drop(db);
        if info.password_hash.is_empty() {
            return Err("password not set for this account, please contact admin".into());
        }
        if !verify_password(password, &info.password_hash) {
            return Err("invalid password".into());
        }
        tracing::info!("[UserManager] user {} logged in via password", user_id);
        Ok(info)
    }

    pub fn set_plan(&self, user_id: &str, plan: UserPlan) -> Result<(), String> {
        let mut db = self.users_db.write();
        let info = db.get_mut(user_id).ok_or("user not found")?;
        info.plan = plan.clone();
        info.max_memories = plan.max_memories();
        let snapshot = db.clone();
        drop(db);
        self.save_users_db(&snapshot)
            .map_err(|e| format!("failed to persist plan: {e}"))?;
        tracing::info!(
            "[UserManager] plan set for user {} -> {:?} (max_memories={})",
            user_id,
            plan,
            plan.max_memories()
        );
        Ok(())
    }

    pub fn set_password(&self, user_id: &str, password: &str) -> Result<(), String> {
        if password.len() < 8 {
            return Err("password must be at least 8 characters".into());
        }
        if password.len() > 128 {
            return Err("password must be under 128 characters".into());
        }
        let mut db = self.users_db.write();
        let info = db.get_mut(user_id).ok_or("user not found")?;
        info.password_hash =
            hash_password(password).map_err(|e| format!("failed to hash password: {e}"))?;
        let snapshot = db.clone();
        drop(db);
        self.save_users_db(&snapshot)
            .map_err(|e| format!("failed to persist password: {e}"))?;
        tracing::info!("[UserManager] password set for user {}", user_id);
        Ok(())
    }

    pub fn create_subaccount(
        &self,
        parent_id: &str,
        sub_user_id: &str,
        password: &str,
    ) -> Result<UserInfo, String> {
        if password.len() < 6 {
            return Err("password must be at least 8 characters".into());
        }
        if password.len() > 128 {
            return Err("password must be under 128 characters".into());
        }
        if sub_user_id.is_empty() || sub_user_id.len() > 64 {
            return Err("user_id must be 1-64 characters".into());
        }
        if !sub_user_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err("user_id: only a-z A-Z 0-9 - _ allowed".into());
        }
        let mut db = self.users_db.write();
        if db.contains_key(sub_user_id) {
            return Err("user already exists".into());
        }
        let parent_info = db.get(parent_id).ok_or("parent user not found")?.clone();
        if parent_info.parent.is_some() {
            return Err("sub-accounts cannot create their own sub-accounts".into());
        }
        if parent_info.sub_accounts.len() >= 10 {
            return Err("maximum 10 sub-accounts per main account".into());
        }
        let api_key = format!("tm-{}", uuid::Uuid::new_v4().to_string().replace("-", ""));
        let password_hash =
            hash_password(password).map_err(|e| format!("failed to hash password: {e}"))?;
        let sub_info = UserInfo {
            user_id: sub_user_id.to_string(),
            api_key: api_key.clone(),
            password_hash,
            plan: UserPlan::Free,
            max_memories: 0,
            memories_used: 0,
            created_at: chrono::Utc::now().timestamp(),
            parent: Some(parent_id.to_string()),
            sub_accounts: Vec::new(),
            is_admin: false,
        };
        db.insert(sub_user_id.to_string(), sub_info.clone());
        if let Some(p) = db.get_mut(parent_id) {
            p.sub_accounts.push(sub_user_id.to_string());
        }
        let snapshot = db.clone();
        drop(db);
        if let Err(e) = self.save_users_db(&snapshot) {
            let mut db = self.users_db.write();
            db.remove(sub_user_id);
            if let Some(p) = db.get_mut(parent_id) {
                p.sub_accounts.retain(|s| s != sub_user_id);
            }
            return Err(format!("failed to persist sub-account: {e}"));
        }
        tracing::info!(
            "[UserManager] created sub-account {} under parent {}",
            sub_user_id,
            parent_id
        );
        Ok(sub_info)
    }

    pub fn list_subaccounts(&self, parent_id: &str) -> Vec<UserInfo> {
        let db = self.users_db.read();
        match db.get(parent_id) {
            Some(info) => info
                .sub_accounts
                .iter()
                .filter_map(|sid| db.get(sid).cloned())
                .collect(),
            None => Vec::new(),
        }
    }

    pub fn revoke_subaccount(&self, parent_id: &str, sub_user_id: &str) -> Result<(), String> {
        let mut db = self.users_db.write();
        let sub = db.get(sub_user_id).ok_or("sub-account not found")?.clone();
        if sub.parent.as_deref() != Some(parent_id) {
            return Err("not your sub-account".into());
        }
        if let Some(p) = db.get_mut(parent_id) {
            p.sub_accounts.retain(|s| s != sub_user_id);
        }
        db.remove(sub_user_id);
        let snapshot = db.clone();
        drop(db);
        self.save_users_db(&snapshot)
            .map_err(|e| format!("failed to persist: {e}"))?;
        tracing::info!(
            "[UserManager] revoked sub-account {} from parent {}",
            sub_user_id,
            parent_id
        );
        Ok(())
    }

    pub fn get_engine(&self, user_id: &str) -> Result<Arc<Engine>, String> {
        {
            let slots = self.slots.read();
            if let Some(slot) = slots.get(user_id) {
                return Ok(slot.engine.clone());
            }
        }

        self.evict_idle();

        let user_data_dir = self.base_data_dir.join("users").join(user_id);
        let mut engine = if let Some(sv) = &self.shared_vector {
            Engine::with_shared_vector(user_data_dir, sv.clone(), user_id)
        } else {
            Engine::with_data_dir(user_data_dir)
        };

        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                let _guard = handle.enter();
                engine.start_with_interval(120000);
            }
            Err(_) => {
                tracing::warn!("[UserManager] no tokio runtime, engine scheduler will not run");
            }
        }

        let engine_arc = Arc::new(engine);

        let actual_count = engine_arc.storage.tetra_count();
        {
            let mut db = self.users_db.write();
            if let Some(info) = db.get_mut(user_id) {
                if info.memories_used != actual_count {
                    tracing::info!(
                        "[UserManager] syncing {} memories_used: {} -> {}",
                        user_id,
                        info.memories_used,
                        actual_count
                    );
                    info.memories_used = actual_count;
                    let snapshot = db.clone();
                    drop(db);
                    if let Err(e) = self.save_users_db(&snapshot) {
                        tracing::warn!("[UserManager] failed to persist sync: {}", e);
                    }
                }
            }
        }

        {
            let mut slots = self.slots.write();
            slots.insert(
                user_id.to_string(),
                UserSlot {
                    engine: engine_arc.clone(),
                    last_access: std::time::Instant::now(),
                },
            );
        }

        tracing::info!(
            "[UserManager] loaded engine for user {} (shared_vector={})",
            user_id,
            self.shared_vector.is_some()
        );
        if let Some(ref pub_sk) = *self.pub_skills.read() {
            engine_arc.scheduler.set_pub_skills(pub_sk.clone());
        }

        Ok(engine_arc)
    }

    pub fn set_pub_skills(&self, pub_skills: Arc<super::skills::SkillEngine>) {
        *self.pub_skills.write() = Some(pub_skills.clone());
        let slots = self.slots.read();
        for slot in slots.values() {
            slot.engine.scheduler.set_pub_skills(pub_skills.clone());
        }
    }

    pub fn touch(&self, user_id: &str) {
        let mut slots = self.slots.write();
        if let Some(slot) = slots.get_mut(user_id) {
            slot.last_access = std::time::Instant::now();
        }
    }

    fn evict_idle(&self) {
        let mut slots = self.slots.write();
        if slots.len() < MAX_USERS {
            return;
        }
        let now = std::time::Instant::now();
        let idle_ids: Vec<String> = slots
            .iter()
            .filter(|(_, slot)| now.duration_since(slot.last_access).as_secs() > IDLE_TIMEOUT_SECS)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &idle_ids {
            if let Some(slot) = slots.remove(id) {
                slot.engine.request_shutdown();
                slot.engine.final_save();
                tracing::info!("[UserManager] evicted idle engine for user {}", id);
            }
        }
        if idle_ids.is_empty() && slots.len() >= MAX_USERS {
            if let Some((id, _slot)) = slots.iter().min_by_key(|(_, s)| s.last_access) {
                let id = id.clone();
                if let Some(slot) = slots.remove(&id) {
                    slot.engine.request_shutdown();
                    slot.engine.final_save();
                    tracing::warn!("[UserManager] evicted oldest user {} to make room", id);
                }
            }
        }
    }

    pub fn user_stats(&self, user_id: &str) -> Option<UserInfo> {
        let db = self.users_db.read();
        db.get(user_id).cloned()
    }

    pub fn check_memory_limit(&self, user_id: &str) -> Result<(), String> {
        let db = self.users_db.read();
        if let Some(info) = db.get(user_id) {
            let owner_id = info.parent.as_deref().unwrap_or(user_id);
            let owner = db.get(owner_id).ok_or("owner not found")?;
            let total_used: usize = db
                .values()
                .filter(|u| u.user_id == owner_id || u.parent.as_deref() == Some(owner_id))
                .map(|u| u.memories_used)
                .sum();
            if total_used >= owner.max_memories {
                return Err(format!(
                    "memory limit reached ({}/{}, shared across account)",
                    total_used, owner.max_memories
                ));
            }
        }
        Ok(())
    }

    pub fn check_and_increment_memory(&self, user_id: &str) -> Result<(), String> {
        let mut db = self.users_db.write();
        let info = db.get(user_id).ok_or("user not found")?.clone();
        let owner_id = info.parent.as_deref().unwrap_or(user_id);
        let owner = db.get(owner_id).ok_or("owner not found")?.clone();
        let total_used: usize = db
            .values()
            .filter(|u| u.user_id == owner_id || u.parent.as_deref() == Some(owner_id))
            .map(|u| u.memories_used)
            .sum();
        if total_used >= owner.max_memories {
            return Err(format!(
                "memory limit reached ({}/{}, shared across account)",
                total_used, owner.max_memories
            ));
        }
        if let Some(info) = db.get_mut(user_id) {
            info.memories_used += 1;
        }
        let snapshot = db.clone();
        drop(db);
        if let Err(e) = self.save_users_db(&snapshot) {
            let mut db = self.users_db.write();
            if let Some(info) = db.get_mut(user_id) {
                info.memories_used -= 1;
            }
            return Err(format!("failed to persist memory count: {e}"));
        }
        Ok(())
    }

    pub fn increment_memory_count(&self, user_id: &str) {
        let mut db = self.users_db.write();
        if let Some(info) = db.get_mut(user_id) {
            info.memories_used += 1;
            let snapshot = db.clone();
            drop(db);
            if let Err(e) = self.save_users_db(&snapshot) {
                let mut db = self.users_db.write();
                if let Some(info) = db.get_mut(user_id) {
                    info.memories_used -= 1;
                }
                tracing::error!(
                    "[UserManager] failed to persist memory count for {}: {}",
                    user_id,
                    e
                );
            }
        }
    }

    pub fn decrement_memory_count(&self, user_id: &str, count: usize) {
        let mut db = self.users_db.write();
        if let Some(info) = db.get_mut(user_id) {
            info.memories_used = info.memories_used.saturating_sub(count);
            let snapshot = db.clone();
            drop(db);
            if let Err(e) = self.save_users_db(&snapshot) {
                tracing::error!(
                    "[UserManager] failed to persist memory count decrement for {}: {}",
                    user_id,
                    e
                );
            }
        }
    }

    pub fn active_users(&self) -> usize {
        self.slots.read().len()
    }

    pub fn total_users(&self) -> usize {
        self.users_db.read().len()
    }

    pub fn slots_read(&self) -> parking_lot::RwLockReadGuard<'_, HashMap<String, UserSlot>> {
        self.slots.read()
    }

    pub fn final_save_all(&self) {
        let slots = self.slots.read();
        for (uid, slot) in slots.iter() {
            slot.engine.final_save();
            tracing::info!("[UserManager] saved engine for user {}", uid);
        }
    }

    pub fn maybe_auto_backup(&self, interval_secs: u64) {
        {
            let last = self.last_backup.read();
            if last.elapsed().as_secs() < interval_secs {
                return;
            }
        }
        *self.last_backup.write() = std::time::Instant::now();
        let slots = self.slots.read();
        let mut ok_count = 0u32;
        let mut err_count = 0u32;
        for (uid, slot) in slots.iter() {
            match slot.engine.backup() {
                Ok(_) => ok_count += 1,
                Err(e) => {
                    err_count += 1;
                    tracing::error!("[AutoBackup] failed for user {}: {}", uid, e);
                }
            }
        }
        if ok_count + err_count > 0 {
            tracing::info!(
                "[AutoBackup] completed: {} ok, {} errors",
                ok_count,
                err_count
            );
        }
    }

    pub fn backup_meta(&self) {
        let src = self.base_data_dir.join("users_meta.json");
        if !src.exists() {
            return;
        }
        let ts = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
        let dst = self.base_data_dir.join(format!("users_meta_{ts}.json.bak"));
        if let Err(e) = std::fs::copy(&src, &dst) {
            tracing::error!("[BackupMeta] failed: {}", e);
            return;
        }
        if let Ok(mut entries) = std::fs::read_dir(&self.base_data_dir) {
            let mut backups: Vec<_> = entries
                .by_ref()
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_name().to_string_lossy().starts_with("users_meta_")
                        && e.file_name().to_string_lossy().ends_with(".bak")
                })
                .collect();
            backups.sort_by_key(|e| e.file_name());
            while backups.len() > 5 {
                if let Some(old) = backups.first() {
                    let _ = std::fs::remove_file(old.path());
                }
                backups.remove(0);
            }
        }
    }

    pub fn list_users(&self) -> Vec<UserInfo> {
        let db = self.users_db.read();
        db.values().cloned().collect()
    }

    pub fn reset_api_key(&self, user_id: &str) -> Result<String, String> {
        let mut db = self.users_db.write();
        if !db.contains_key(user_id) {
            return Err("user not found".into());
        }
        let mut new_key = format!("tm-{}", uuid::Uuid::new_v4().to_string().replace("-", ""));
        for _ in 0..10 {
            if !db.values().any(|u| constant_time_eq(&u.api_key, &new_key)) {
                break;
            }
            new_key = format!("tm-{}", uuid::Uuid::new_v4().to_string().replace("-", ""));
        }
        if let Some(info) = db.get_mut(user_id) {
            let old_key = info.api_key.clone();
            info.api_key = new_key.clone();
            let snapshot = db.clone();
            drop(db);
            if let Err(e) = self.save_users_db(&snapshot) {
                let mut db = self.users_db.write();
                if let Some(info) = db.get_mut(user_id) {
                    info.api_key = old_key;
                }
                return Err(format!("failed to persist API key reset: {e}"));
            }
            tracing::info!("[UserManager] reset API key for user {}", user_id);
            Ok(new_key)
        } else {
            Err("user not found".into())
        }
    }

    fn load_users_db(base_dir: &std::path::Path) -> HashMap<String, UserInfo> {
        let db_path = base_dir.join("users_meta.json");
        if !db_path.exists() {
            return HashMap::new();
        }
        let raw = match std::fs::read_to_string(&db_path) {
            Ok(d) => d,
            Err(e) => {
                tracing::error!("[UserManager] failed to read users_meta.json: {}", e);
                return HashMap::new();
            }
        };
        let meta_crypto = super::crypto::CryptoEngine::from_env().ok();
        let data = if let Some(ref crypto) = meta_crypto {
            match serde_json::from_str::<serde_json::Value>(&raw) {
                Ok(v) if v.is_object() && v.get("__enc").is_some() => {
                    let enc_payload = v["__enc"].as_str().unwrap_or("");
                    match crypto.decrypt_content(enc_payload, "__meta_db__") {
                        Ok(dec) => dec,
                        Err(e) => {
                            tracing::warn!(
                                "[UserManager] decrypt users_meta failed, trying plaintext: {}",
                                e
                            );
                            raw
                        }
                    }
                }
                _ => raw,
            }
        } else {
            raw
        };
        match serde_json::from_str::<HashMap<String, UserInfo>>(&data) {
            Ok(db) => {
                tracing::info!("[UserManager] loaded {} users from disk", db.len());
                db
            }
            Err(e) => {
                tracing::error!("[UserManager] failed to parse users_meta.json: {}", e);
                let corrupted = base_dir.join("users_meta.json.corrupted");
                let _ = std::fs::rename(&db_path, &corrupted);
                tracing::error!(
                    "[UserManager] corrupted file backed up to users_meta.json.corrupted"
                );
                HashMap::new()
            }
        }
    }

    pub fn delete_user(&self, user_id: &str) -> Result<(), String> {
        {
            let db = self.users_db.read();
            if let Some(u) = db.get(user_id) {
                if u.is_admin {
                    return Err("cannot delete admin".into());
                }
            }
        }
        {
            let mut db = self.users_db.write();
            if db.remove(user_id).is_none() {
                return Err("user not found".into());
            }
            self.save_users_db(&db)?;
        }
        {
            let mut slots = self.slots.write();
            slots.remove(user_id);
        }
        let user_dir = self.base_data_dir.join("users").join(user_id);
        if user_dir.exists() {
            std::fs::remove_dir_all(&user_dir).map_err(|e| format!("rm dir: {e}"))?;
        }
        tracing::info!("[UserManager] deleted user {}", user_id);
        Ok(())
    }

    fn save_users_db(&self, db: &HashMap<String, UserInfo>) -> Result<(), String> {
        let db_path = self.base_data_dir.join("users_meta.json");
        let tmp_path = self.base_data_dir.join("users_meta.json.tmp");
        let json = serde_json::to_string_pretty(db).map_err(|e| format!("serialize: {e}"))?;
        let output = if let Some(ref crypto) = self.meta_crypto {
            let enc = crypto
                .encrypt_content(&json, "__meta_db__")
                .map_err(|e| format!("encrypt users_meta: {e}"))?;
            serde_json::json!({"__enc": enc}).to_string()
        } else {
            json
        };
        std::fs::write(&tmp_path, &output).map_err(|e| format!("write tmp: {e}"))?;
        std::fs::rename(&tmp_path, &db_path).map_err(|e| format!("rename: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }
}
