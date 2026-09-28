use std::collections::HashMap;
use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::crypto::{constant_time_eq, constant_time_eq_bytes};
use super::vector::VectorLayer;
use super::Engine;

fn hash_password(password: &str) -> String {
    use argon2::password_hash::SaltString;
    use argon2::PasswordHasher;
    let salt = SaltString::generate(&mut rand::rngs::OsRng);
    let argon2 = argon2::Argon2::default();
    match argon2.hash_password(password.as_bytes(), &salt) {
        Ok(hash) => hash.to_string(),
        Err(_) => {
            tracing::error!("[UserManager] argon2 hash failed, falling back to legacy");
            hash_password_legacy(password)
        }
    }
}

fn hash_password_legacy(password: &str) -> String {
    let salt: [u8; 32] = rand::random();
    let mut hasher = Sha256::new();
    hasher.update(salt);
    hasher.update(password.as_bytes());
    let hash = hasher.finalize();
    format!("{}:{}", B64.encode(salt), B64.encode(hash))
}

fn verify_password(password: &str, stored: &str) -> bool {
    if stored.is_empty() {
        return false;
    }
    if stored.starts_with("$argon2") {
        use argon2::PasswordHash;
        use argon2::PasswordVerifier;
        let hash = match PasswordHash::new(stored) {
            Ok(h) => h,
            Err(_) => return false,
        };
        argon2::Argon2::default()
            .verify_password(password.as_bytes(), &hash)
            .is_ok()
    } else {
        verify_password_legacy(password, stored)
    }
}

fn verify_password_legacy(password: &str, stored: &str) -> bool {
    let parts: Vec<&str> = stored.splitn(2, ':').collect();
    if parts.len() != 2 {
        return false;
    }
    let salt = match B64.decode(parts[0]) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let expected = match B64.decode(parts[1]) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let mut hasher = Sha256::new();
    hasher.update(&salt);
    hasher.update(password.as_bytes());
    let hash = hasher.finalize();
    constant_time_eq_bytes(&hash, &expected)
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
        crate::engine::vector::EMBEDDING_DIM
    }
}

/// Phase 3 P0: 人格加载状态（Tester-Q契约 #1658）
/// unknown → warming_up → ready | degraded
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum PersonaState {
    #[default]
    Unknown,
    WarmingUp,
    Ready,
    Degraded,
}

pub struct UserSlot {
    pub engine: Arc<Engine>,
    pub last_access: std::time::Instant,
    pub persona_state: PersonaState,
    pub loop_started: std::sync::atomic::AtomicBool,
    // α0.2: D-series runtime binding flag (set by cloud runtime register/unregister)
    pub has_primary: std::sync::atomic::AtomicBool,
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
    /// Phase 3 P0: 正在加载的用户集合（singleflight 去重）
    loading_users: parking_lot::Mutex<std::collections::HashSet<String>>,
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
            loading_users: parking_lot::Mutex::new(std::collections::HashSet::new()),
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
            loading_users: parking_lot::Mutex::new(std::collections::HashSet::new()),
        }
    }

    fn generate_invite_code() -> String {
        use rand::Rng;
        let chars: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789";
        let mut rng = rand::thread_rng();
        (0..32)
            .map(|_| chars[rng.gen_range(0..chars.len())] as char)
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

        // 原子检查+轮换（kimi2.7 #5：消除 TOCTOU，write lock 持有期间完成检查+轮换）
        let mut current = self.invite_code.write();
        if !constant_time_eq(code, &current) {
            return Err("invalid or expired invite code".into());
        }
        let old = current.clone();
        self.used_codes.write().push(old);
        *current = Self::generate_invite_code();
        drop(current);
        self.save_invite_state();
        tracing::info!("[UserManager] invite code used, rotated new code");
        Ok(())
    }

    /// 注册失败回补邀请码: 邀请码在注册前被消耗, 若账户创建失败(重名/超限)
    /// 应恢复, 否则持码者白白损失一个名额 (审计 2026-09 低优 #22)
    pub fn refund_invite_code(&self, code: &str) {
        {
            let mut used = self.used_codes.write();
            if let Some(pos) = used.iter().position(|c| constant_time_eq(code, c)) {
                let refunded = used.remove(pos);
                self.pending_codes.write().push(refunded);
            }
        }
        self.save_invite_state();
        tracing::info!(
            "[UserManager] invite code refunded (registration failed), {} pending",
            self.pending_codes.read().len()
        );
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

    /// 特权账号名单(库审批等 owner 级判定): 可经 EPICODE_OWNER_IDS/TETRAMEM_OWNER_IDS
    /// 配置(逗号分隔), 默认 "sunorme". 替代散落的硬编码字符串比较
    /// (审计 2026-09 中优 #9: 特权绑定用户名, 且该名可被抢注).
    pub fn is_privileged_id(user_id: &str) -> bool {
        use std::sync::OnceLock;
        static OWNERS: OnceLock<Vec<String>> = OnceLock::new();
        let owners = OWNERS.get_or_init(|| {
            // cloud 二进制启动时会把 EPICODE_* 别名进 TETRAMEM_*;
            // 库场景优先读 TETRAMEM_(别名后生效值), 其它二进制回退 EPICODE_
            std::env::var("TETRAMEM_OWNER_IDS")
                .or_else(|_| std::env::var("EPICODE_OWNER_IDS"))
                .unwrap_or_else(|_| "sunorme".into())
                .split(',')
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        });
        owners.iter().any(|o| o.eq(&user_id.to_lowercase()))
    }

    /// 保留用户名: 特权名与 admin 在自助注册(邀请码路径)中不可用 —
    /// 防止 owner 账号创建前被持码者抢注获得库审批权 (审计 2026-09 中优 #9)
    pub fn is_reserved_id(user_id: &str) -> bool {
        Self::is_privileged_id(user_id)
            || user_id.eq_ignore_ascii_case("admin")
            || user_id.eq_ignore_ascii_case("root")
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
        // pending 空时才生成新码（不覆盖已有 pending，只追加）
        let code = Self::generate_invite_code();
        self.pending_codes.write().push(code.clone());
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
            return Err("password must be at least 6 characters".into());
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
        let info = UserInfo {
            user_id: user_id.to_string(),
            api_key: api_key.to_string(),
            password_hash: hash_password(password),
            plan,
            max_memories: max_mem,
            memories_used: 0,
            created_at: chrono::Utc::now().timestamp(),
            parent: None,
            sub_accounts: Vec::new(),
        };
        db.insert(user_id.to_string(), info.clone());
        let snapshot = db.clone();
        drop(db);
        if let Err(e) = self.save_users_db(&snapshot) {
            let mut db = self.users_db.write();
            db.remove(user_id);
            return Err(format!("failed to persist registration: {}", e));
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
            .map_err(|e| format!("failed to persist plan: {}", e))?;
        tracing::info!(
            "[UserManager] plan set for user {} -> {:?} (max_memories={})",
            user_id,
            plan,
            plan.max_memories()
        );
        Ok(())
    }

    pub fn set_password(&self, user_id: &str, password: &str) -> Result<(), String> {
        if password.len() < 6 {
            return Err("password must be at least 6 characters".into());
        }
        if password.len() > 128 {
            return Err("password must be under 128 characters".into());
        }
        let mut db = self.users_db.write();
        let info = db.get_mut(user_id).ok_or("user not found")?;
        info.password_hash = hash_password(password);
        let snapshot = db.clone();
        drop(db);
        self.save_users_db(&snapshot)
            .map_err(|e| format!("failed to persist password: {}", e))?;
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
            return Err("password must be at least 6 characters".into());
        }
        if password.len() > 128 {
            return Err("password must be under 128 characters".into());
        }
        if sub_user_id.is_empty() || sub_user_id.len() > 64 {
            return Err("user_id must be 1-64 characters".into());
        }
        // 引擎层同样拒绝保留名(审计三轮): handler 层校验之外的纵深防御
        if Self::is_reserved_id(sub_user_id) {
            return Err("this username is reserved".into());
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
        if parent_info.sub_accounts.iter().any(|s| s == sub_user_id) {
            return Err("sub-account already linked".into()); // 防重复（kimi 子账户 bug 根因）
        }
        let api_key = format!("tm-{}", uuid::Uuid::new_v4().to_string().replace("-", ""));
        let sub_info = UserInfo {
            user_id: sub_user_id.to_string(),
            api_key: api_key.clone(),
            password_hash: hash_password(password),
            plan: UserPlan::Free,
            max_memories: 0,
            memories_used: 0,
            created_at: chrono::Utc::now().timestamp(),
            parent: Some(parent_id.to_string()),
            sub_accounts: Vec::new(),
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
            return Err(format!("failed to persist sub-account: {}", e));
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
            Some(info) => {
                let mut seen = std::collections::HashSet::new();
                info.sub_accounts
                    .iter()
                    .filter(|sid| seen.insert((*sid).clone())) // 去重（修复历史重复）
                    .filter_map(|sid| db.get(sid).cloned())
                    .collect()
            }
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
            .map_err(|e| format!("failed to persist: {}", e))?;
        tracing::info!(
            "[UserManager] revoked sub-account {} from parent {}",
            sub_user_id,
            parent_id
        );
        Ok(())
    }

    pub fn get_engine(&self, user_id: &str) -> Result<Arc<Engine>, String> {
        // H6 修复：防御性兜底——防止 ../ 路径遍历逃逸到任意目录
        if user_id.is_empty()
            || user_id.len() > 64
            || !user_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err("invalid user_id".into());
        }

        // Phase 3 P0: persona_state gate (Tester-Q契约 #1658)
        {
            let slots = self.slots.read();
            if let Some(slot) = slots.get(user_id) {
                match slot.persona_state {
                    PersonaState::Ready => return Ok(slot.engine.clone()),
                    PersonaState::WarmingUp => return Err("PERSONA_WARMING_UP".into()),
                    PersonaState::Degraded => return Err("PERSONA_DEGRADED".into()),
                    PersonaState::Unknown => {}
                }
            }
        }

        // P0c/L1: 单飞只拒绝外部并发者; 加载权持有者(helpers spawn_blocking先try_mark_loading)
        // 调 get_engine_inner —— K1曾在此拒绝自己的调用者, 引擎永远加载不出(19:38失败循环+冻结根因)
        {
            let mut loading = self.loading_users.lock();
            if loading.contains(user_id) {
                return Err("PERSONA_WARMING_UP".into());
            }
            loading.insert(user_id.to_string());
        }
        // 双重检查: 拿到loading标记后复查slot(可能在等锁期间别人已完成)
        {
            let slots = self.slots.read();
            if let Some(slot) = slots.get(user_id) {
                if matches!(slot.persona_state, PersonaState::Ready) {
                    self.loading_users.lock().remove(user_id);
                    return Ok(slot.engine.clone());
                }
            }
        }

        // panic加固: 加载中途panic不得永久卡死loading标记(重启才能解) — 接住并转Degraded语义
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.get_engine_inner(user_id)
        }))
        .unwrap_or_else(|p| {
            let msg = p
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".into());
            tracing::error!("[UserManager] persona load PANIC for {}: {}", user_id, msg);
            Err("PERSONA_LOAD_PANIC".into())
        });
        self.loading_users.lock().remove(user_id);
        result
    }

    /// L1: 加载权持有者入口(helpers spawn_blocking 用)
    pub fn get_engine_for_loader(&self, user_id: &str) -> Result<Arc<Engine>, String> {
        self.get_engine_inner(user_id)
    }

    /// L1: 真正的加载体 — 仅由持有加载权的调用方使用(get_engine公共层 / helpers spawn_blocking)
    fn get_engine_inner(&self, user_id: &str) -> Result<Arc<Engine>, String> {
        self.evict_idle();

        let user_data_dir = self.base_data_dir.join("users").join(user_id);
        let engine = if let Some(sv) = &self.shared_vector {
            Engine::with_shared_vector(user_data_dir, sv.clone(), user_id)
        } else {
            Engine::with_data_dir(user_data_dir)
        };

        // P0-1 修复(Tester-Q契约 #1658): 删除 handle.enter() + start_with_interval
        // restore 路径只做数据恢复，不启动 cognitive loop
        // cognitive loop 由 ensure_cognitive_loop_started() 在 async 线程启动
        // 防止 spawn_blocking 线程里的 cognitive loop 用 ureq 同步 HTTP 饿死 executor
        tracing::info!(
            "[UserManager] user '{}' — engine data restored (cognitive loop deferred)",
            user_id
        );

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
                    persona_state: PersonaState::Ready,
                    loop_started: std::sync::atomic::AtomicBool::new(false),
                    has_primary: std::sync::atomic::AtomicBool::new(false),
                },
            );
        }
        self.loading_users.lock().remove(user_id);

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

    pub fn evict_idle(&self) {
        self.evict_idle_with(IDLE_TIMEOUT_SECS, false);
    }

    /// 压力感知驱逐 (2026-09-23): 内存吃紧时用短门槛提前回收半闲置引擎,
    /// 避免"空闲40-60分钟(<1h线)+内存已紧"窗口卡住 laya/图书馆员等服务
    pub fn evict_idle_with(&self, min_idle_secs: u64, pressure: bool) {
        // H3 修复：锁内只收集要驱逐的 engine Arc，drop 锁后再做 final_save（避免 SQLite I/O 阻塞所有用户）
        let engines_to_save: Vec<(String, Arc<Engine>)> = {
            let mut slots = self.slots.write();
            let now = std::time::Instant::now();

            let idle_ids: Vec<String> = slots
                .iter()
                .filter(|(_, slot)| now.duration_since(slot.last_access).as_secs() > min_idle_secs)
                .map(|(id, _)| id.clone())
                .collect();

            let mut to_save = Vec::new();
            for id in &idle_ids {
                if let Some(slot) = slots.remove(id) {
                    slot.engine.request_shutdown();
                    to_save.push((id.clone(), slot.engine.clone()));
                    if pressure {
                        tracing::warn!("[UserManager] pressure-evicted engine for user {} (idle {}s > {}s, mem tight)", id, now.duration_since(slot.last_access).as_secs(), min_idle_secs);
                    } else {
                        tracing::info!(
                            "[UserManager] evicted idle engine for user {} (idle {}s)",
                            id,
                            now.duration_since(slot.last_access).as_secs()
                        );
                    }
                }
            }

            if idle_ids.is_empty() && slots.len() >= MAX_USERS {
                if let Some((id, _slot)) = slots.iter().min_by_key(|(_, s)| s.last_access) {
                    let id = id.clone();
                    if let Some(slot) = slots.remove(&id) {
                        slot.engine.request_shutdown();
                        to_save.push((id.clone(), slot.engine.clone()));
                        tracing::warn!("[UserManager] evicted oldest user {} to make room", id);
                    }
                }
            }
            to_save
        }; // 锁已 drop

        // 锁外异步保存（不阻塞 authenticate/touch/get_engine）
        for (id, engine) in engines_to_save {
            engine.final_save();
            tracing::info!("[UserManager] saved evicted engine for user {}", id);
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
            return Err(format!("failed to persist memory count: {}", e));
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
        let dst = self
            .base_data_dir
            .join(format!("users_meta_{}.json.bak", ts));
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

    /// Phase 3 P0: 获取引擎或触发异步加载（singleflight）
    /// Ok(engine) = Ready 可用
    /// Err(WarmingUp) = 首次触发，正在后台加载
    /// Err(Degraded) = 用户不存在或加载失败
    ///
    /// 关键：首次请求标记 loading → spawn_blocking 加载 → 立即返回 WarmingUp
    /// 后续请求看到 loading 直接返回 WarmingUp，不重复加载
    pub fn get_engine_or_trigger(&self, user_id: &str) -> Result<Arc<Engine>, PersonaState> {
        // 验证 user_id
        if user_id.is_empty()
            || user_id.len() > 64
            || !user_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(PersonaState::Degraded);
        }

        // 快速路径 1：slot 已 Ready
        {
            let slots = self.slots.read();
            if let Some(slot) = slots.get(user_id) {
                match slot.persona_state {
                    PersonaState::Ready => return Ok(slot.engine.clone()),
                    PersonaState::WarmingUp => return Err(PersonaState::WarmingUp),
                    PersonaState::Degraded => return Err(PersonaState::Degraded),
                    PersonaState::Unknown => {}
                }
            }
        }

        // 快速路径 2：正在加载（singleflight）
        {
            let loading = self.loading_users.lock();
            if loading.contains(user_id) {
                return Err(PersonaState::WarmingUp);
            }
        }

        // 检查用户是否存在
        let user_exists = {
            let db = self.users_db.read();
            db.contains_key(user_id)
        };
        if !user_exists {
            return Err(PersonaState::Degraded);
        }

        // P0c: 单飞已下沉到 get_engine 内部(K1), 此处不再重复标记
        // (双重标记会在清理时互相清除, 破坏互斥)

        // 同步加载（当前保持阻塞——真正的 spawn_blocking 需要 Arc<Self>）
        // 折衷：如果 tokio runtime 存在，在 blocking pool 里加载
        // 否则直接同步加载
        if let Ok(_handle) = tokio::runtime::Handle::try_current() {
            // 在 tokio runtime 里——但当前请求可能已经 hold 了 runtime，
            // 所以用 spawn_blocking 让它不阻塞 async executor
            // 但 spawn_blocking 是异步的，我们需要同步等待结果
            // 这在 axum extractor 里不太理想——因为 extractor 是 async 的
            // 最佳方案：让 AuthedEngine extractor 直接调 async 版本
            //
            // 当前折衷：直接同步调 get_engine（保持原有行为）
            // 但加上 loading 标记和 timeout 保护
            tracing::info!(
                "[UserManager] persona load triggered for '{}' (sync fallback)",
                user_id
            );
        }

        // 同步加载（实际加载逻辑在 get_engine 里, 单飞/清理都在其中）
        let result = self.get_engine(user_id);

        match result {
            Ok(engine) => {
                tracing::info!("[UserManager] persona READY for '{}'", user_id);
                Ok(engine)
            }
            Err(e) => {
                tracing::error!("[UserManager] persona load FAILED for '{}': {}", user_id, e);
                Err(PersonaState::Degraded)
            }
        }
    }

    /// Phase 3 P0: 预热用户引擎（启动时调用，非阻塞）
    /// 为每个已知用户 spawn 后台加载线程，标记 loading
    /// 加载完成后 slot 自动标记 Ready
    pub fn prewarm_all(&self) {
        tracing::info!("[UserManager] prewarm_all: only primary sunorme (no tenant engines)");
        self.prewarm_primary();
    }

    pub fn prewarm_primary(&self) {
        const PRIMARY: &str = "sunorme";
        tracing::info!("[UserManager] prewarm_primary {}", PRIMARY);
        match self.get_engine(PRIMARY) {
            Ok(_) => tracing::info!("[UserManager] prewarm_primary complete"),
            Err(e) => tracing::warn!("[UserManager] prewarm_primary failed: {}", e),
        }
    }

    /// Phase 3 P0: 检查用户是否正在加载
    pub fn is_loading(&self, user_id: &str) -> bool {
        self.loading_users.lock().contains(user_id)
    }

    /// S1根治: 原子 check-and-set — 未在加载则标记并返回 true(调用方获得加载权);
    /// 已在加载返回 false。消除 check-then-act 竞态窗口(双实例根因)。
    pub fn try_mark_loading(&self, user_id: &str) -> bool {
        let mut loading = self.loading_users.lock();
        loading.insert(user_id.to_string()) // HashSet::insert 返回是否新插入
    }

    /// Phase 3 P0: 标记用户为正在加载（singleflight）— 已由 try_mark_loading 取代, 保留兼容
    pub fn mark_loading(&self, user_id: &str) {
        self.loading_users.lock().insert(user_id.to_string());
    }

    /// Phase 3 P0: 清除加载标记
    pub fn clear_loading(&self, user_id: &str) {
        self.loading_users.lock().remove(user_id);
    }

    /// Phase 3 P0-3: 通过 API key 查找 user_id
    pub fn find_user_by_api_key(&self, api_key: &str) -> String {
        if api_key.is_empty() {
            return String::new();
        }
        let db = self.users_db.read();
        for (uid, info) in db.iter() {
            if info.api_key == api_key {
                return uid.clone();
            }
        }
        String::new()
    }

    /// Phase 3 P0-3: 只从缓存读 engine（不触发加载）
    /// 用于 persona_ready 等只读状态端点
    pub fn vector_ready(&self) -> bool {
        self.shared_vector.is_some()
    }

    pub fn try_get_engine_slot(&self, user_id: &str) -> Option<Arc<Engine>> {
        let slots = self.slots.read();
        slots
            .get(user_id)
            .filter(|s| s.persona_state == PersonaState::Ready)
            .map(|s| s.engine.clone())
    }

    /// 审计终极修复: strict 版 — 加载进行中拒绝, 供直调方 (runtime/mcp_endpoint) 使用,
    /// 防止在 singleflight 加载未完成时构造第二个 Engine (双 DriveQueue → id 冲突)。
    /// AuthedEngine 的 singleflight 内部继续用 get_engine (自举需要)。
    pub fn get_engine_strict(&self, user_id: &str) -> Result<Arc<Engine>, String> {
        if user_id.is_empty()
            || user_id.len() > 64
            || !user_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err("invalid user_id".into());
        }
        if self.is_loading(user_id) {
            return Err("PERSONA_WARMING_UP".into());
        }
        self.get_engine(user_id)
    }

    /// α0.2: runtime primary_executor 绑定标志 (由 cloud 层 register/unregister 更新)
    pub fn has_primary_executor(&self, user_id: &str) -> bool {
        let slots = self.slots.read();
        slots
            .get(user_id)
            .map(|s| s.has_primary.load(std::sync::atomic::Ordering::Relaxed))
            .unwrap_or(false)
    }

    /// α0.2: 设置 primary 绑定标志
    pub fn set_has_primary_executor(&self, user_id: &str, val: bool) {
        let mut slots = self.slots.write();
        if let Some(s) = slots.get_mut(user_id) {
            s.has_primary
                .store(val, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Phase 3 P0-2b: 检查 cognitive loop 是否已启动
    pub fn is_loop_started(&self, user_id: &str) -> bool {
        let slots = self.slots.read();
        slots
            .get(user_id)
            .map(|s| s.loop_started.load(std::sync::atomic::Ordering::Relaxed))
            .unwrap_or(false)
    }

    /// Phase 3 P0-2b: 标记 cognitive loop 已启动（once 去重）
    pub fn mark_loop_started(&self, user_id: &str) {
        let mut slots = self.slots.write();
        if let Some(s) = slots.get_mut(user_id) {
            s.loop_started
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Phase 3 P0: 获取用户人格状态（Tester-Q契约 #1658）
    pub fn get_persona_state(&self, user_id: &str) -> PersonaState {
        let slots = self.slots.read();
        match slots.get(user_id) {
            Some(slot) => slot.persona_state,
            None => PersonaState::Unknown,
        }
    }

    /// Phase 3 P0: 标记用户人格为 Ready（加载完成后调用）
    pub fn mark_persona_ready(&self, user_id: &str) {
        let mut slots = self.slots.write();
        if let Some(slot) = slots.get_mut(user_id) {
            slot.persona_state = PersonaState::Ready;
            tracing::info!("[UserManager] persona ready for user '{}'", user_id);
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
                return Err(format!("failed to persist API key reset: {}", e));
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
        if user_id == "sunorme" {
            return Err("cannot delete admin".into());
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
            std::fs::remove_dir_all(&user_dir).map_err(|e| format!("rm dir: {}", e))?;
        }
        tracing::info!("[UserManager] deleted user {}", user_id);
        Ok(())
    }

    fn save_users_db(&self, db: &HashMap<String, UserInfo>) -> Result<(), String> {
        let db_path = self.base_data_dir.join("users_meta.json");
        let tmp_path = self.base_data_dir.join("users_meta.json.tmp");
        let json = serde_json::to_string_pretty(db).map_err(|e| format!("serialize: {}", e))?;
        let output = if let Some(ref crypto) = self.meta_crypto {
            let enc = crypto
                .encrypt_content(&json, "__meta_db__")
                .map_err(|e| format!("encrypt users_meta: {}", e))?;
            serde_json::json!({"__enc": enc}).to_string()
        } else {
            json
        };
        std::fs::write(&tmp_path, &output).map_err(|e| format!("write tmp: {}", e))?;
        std::fs::rename(&tmp_path, &db_path).map_err(|e| format!("rename: {}", e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }
}
