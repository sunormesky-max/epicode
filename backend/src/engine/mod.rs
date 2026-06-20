pub mod adaptive;
pub mod assembler;
pub mod audit;
pub mod auto_pipeline;
pub mod bus;
pub mod cache;
pub mod classifier;
pub mod cognitive;
pub mod cognitive_hooks;
pub mod constitution;
pub mod crypto;
pub mod digestion;
pub mod dream;
pub mod drive;
pub mod dynamics;
pub mod embedding;
pub mod emotion;
pub mod energy;
pub mod gateway;
pub mod governor;
pub mod hnsw;
pub mod index_manager;
pub mod intake;
pub mod janitor;
pub mod key_rotation;
pub mod knowledge;
pub mod mcp;
pub mod outcome;
pub mod pulse;
pub mod reasoning;
pub mod retrieval;
pub mod scheduler;
pub mod search_engine;
pub mod security;
pub mod skills;
pub mod storage;
pub mod system_skills;
pub mod tools;
pub mod user_manager;
pub mod vector;

use std::sync::Arc;
use std::sync::Mutex;
use tokio::task::JoinHandle;

use crate::domain::space::Space;

use self::audit::AuditLogger;
use self::bus::EventBus;
use self::classifier::CategoryClassifier;
use self::cognitive::CognitiveEngine;
use self::embedding::EmbeddingService;
use self::energy::EnergyCenter;
use self::gateway::GatewayCenter;
use self::knowledge::KnowledgeGraph;
use self::scheduler::SchedulerCenter;
use self::security::SecurityGuard;
use self::skills::SkillEngine;
use self::storage::StorageManager;
use self::vector::VectorLayer;
use crate::api::authz::{AuthorizationChecker, PermissionRepository};

pub struct Engine {
    pub space: Arc<Space>,
    pub bus: Arc<EventBus>,
    #[allow(dead_code)] // kept alive for SchedulerCenter's Arc lifetime
    gateway: Arc<GatewayCenter>,
    pub energy: Arc<EnergyCenter>,
    pub scheduler: Arc<SchedulerCenter>,
    pub cognitive: Arc<CognitiveEngine>,
    pub guard: Arc<SecurityGuard>,
    pub storage: Arc<StorageManager>,
    pub skills: Arc<SkillEngine>,
    pub key_rotation: Arc<Mutex<key_rotation::KeyRotation>>,
    pub authz: Arc<AuthorizationChecker>,
    handles: Vec<JoinHandle<()>>,
    pub user_id: String,
    data_path: std::path::PathBuf,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        Self::with_data_dir(std::path::PathBuf::from("data"))
    }

    pub fn with_data_dir(data_path: std::path::PathBuf) -> Self {
        Self::build(data_path, None, None)
    }

    pub fn with_shared_vector(
        data_path: std::path::PathBuf,
        shared_vector: Arc<VectorLayer>,
        user_id: &str,
    ) -> Self {
        Self::build(data_path, Some(shared_vector), Some(user_id))
    }

    pub fn load_shared_vector() -> Option<Arc<VectorLayer>> {
        let model_dir = {
            let exe_dir = std::env::current_exe()
                .ok()
                .and_then(|e| e.parent().map(|p| p.to_path_buf()));
            let candidates: Vec<std::path::PathBuf> = vec![
                std::path::PathBuf::from("models"),
                exe_dir
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                    .join("models"),
            ];
            candidates
                .into_iter()
                .find(|d| d.join("model.onnx").exists())
                .unwrap_or_else(|| std::path::PathBuf::from("models"))
        };
        match VectorLayer::load(&model_dir) {
            Ok(v) => {
                tracing::info!(
                    "Shared VectorLayer initialized (ONNX, {} dims, 1 copy for all users)",
                    self::vector::EMBEDDING_DIM
                );
                Some(Arc::new(v))
            }
            Err(e) => {
                tracing::warn!(
                    "VectorLayer unavailable: {} — falling back to HTTP embedding",
                    e
                );
                None
            }
        }
    }

    fn build(
        data_path: std::path::PathBuf,
        shared_vector: Option<Arc<VectorLayer>>,
        user_id: Option<&str>,
    ) -> Self {
        let uid = user_id.unwrap_or("mcp-default");
        let api_key = std::env::var("DEEPSEEK_API_KEY").unwrap_or_default();
        if api_key.is_empty() {
            tracing::warn!("DEEPSEEK_API_KEY not set — cognitive engine disabled.");
        }

        let space = Arc::new(Space::new());
        let bus = Arc::new(EventBus::new(256));

        let energy = Arc::new(EnergyCenter::new(
            energy::DEFAULT_MAX_ENERGY,
            energy::RECHARGE_RATE,
            bus.sender(),
            bus.subscribe(),
        ));

        let knowledge = Arc::new(KnowledgeGraph::new());
        let security = Arc::new(SecurityGuard::from_env());

        let cognitive = Arc::new(CognitiveEngine::new(&api_key, "deepseek-chat"));

        let vector = if let Some(sv) = shared_vector {
            tracing::info!("Using shared VectorLayer for user '{}'", uid);
            Some(sv)
        } else {
            let model_dir = {
                let exe_dir = std::env::current_exe()
                    .ok()
                    .and_then(|e| e.parent().map(|p| p.to_path_buf()));
                let candidates: Vec<std::path::PathBuf> = vec![
                    std::path::PathBuf::from("models"),
                    exe_dir
                        .unwrap_or_else(|| std::path::PathBuf::from("."))
                        .join("models"),
                    data_path.join("models"),
                ];
                candidates
                    .into_iter()
                    .find(|d| d.join("model.onnx").exists())
                    .unwrap_or_else(|| std::path::PathBuf::from("models"))
            };
            match VectorLayer::load(&model_dir) {
                Ok(v) => {
                    tracing::info!(
                        "VectorLayer initialized (in-process ONNX, {} dims)",
                        self::vector::EMBEDDING_DIM
                    );
                    Some(Arc::new(v))
                }
                Err(e) => {
                    tracing::warn!(
                        "VectorLayer unavailable: {} — falling back to HTTP embedding",
                        e
                    );
                    None
                }
            }
        };

        let embedding = Arc::new(EmbeddingService::from_env());
        if embedding.enabled() {
            tracing::info!("Embedding HTTP fallback enabled");
        }

        let storage = Arc::new({
            let sm = StorageManager::new(&data_path).unwrap_or_else(|e| {
                tracing::error!("FATAL: {}", e);
                std::process::exit(1);
            });
            match self::crypto::CryptoEngine::from_env() {
                Ok(crypto) => {
                    tracing::info!("Encryption enabled (AES-256-GCM) for user '{}'", uid);
                    sm.with_encryption(crypto, uid)
                }
                Err(_) => {
                    tracing::debug!("Encryption disabled (no TETRAMEM_MASTER_KEY)");
                    sm
                }
            }
        });

        let report = storage.load_all(&space, &knowledge);
        if report.tetras_loaded > 0 {
            tracing::info!(
                "[{}] loaded {} tetras, {} relations, {} concepts",
                uid,
                report.tetras_loaded,
                report.relations_loaded,
                report.concepts_loaded
            );
        }

        {
            let identity_path = data_path.join("identity.json");
            if identity_path.exists() {
                if let Ok(data) = std::fs::read_to_string(&identity_path) {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
                        let name = v["name"].as_str().unwrap_or("").to_string();
                        let mission = v["mission"].as_str().unwrap_or("").to_string();
                        let author = v["author"].as_str().unwrap_or("").to_string();
                        let extra: std::collections::HashMap<String, String> = v
                            .get("extra")
                            .and_then(|e| serde_json::from_value(e.clone()).ok())
                            .unwrap_or_default();
                        if !name.is_empty() {
                            let name_clone = name.clone();
                            space.confirm_identity(name, mission, author, extra);
                            tracing::info!("[{}] identity loaded: {}", uid, name_clone);
                        }
                    }
                }
            } else {
                let pending_path = data_path.join("identity_pending.json");
                if pending_path.exists() {
                    if let Ok(data) = std::fs::read_to_string(&pending_path) {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
                            let pending = space.pending_identity();
                            let mut p = pending;
                            if let Some(n) = v["name"].as_str() {
                                if !n.is_empty() {
                                    p.name = Some(n.to_string());
                                }
                            }
                            if let Some(m) = v["mission"].as_str() {
                                if !m.is_empty() {
                                    p.mission = Some(m.to_string());
                                }
                            }
                            if let Some(a) = v["author"].as_str() {
                                if !a.is_empty() {
                                    p.author = Some(a.to_string());
                                }
                            }
                            if let Some(pe) = v["personality"].as_str() {
                                p.personality = Some(pe.to_string());
                            }
                            if let Some(l) = v["language"].as_str() {
                                p.language = Some(l.to_string());
                            }
                            for (step, val) in [
                                (1, &p.name),
                                (2, &p.mission),
                                (3, &p.author),
                                (4, &p.personality),
                                (5, &p.language),
                            ] {
                                if let Some(v) = val {
                                    space.set_identity_step(step, v.clone());
                                }
                            }
                            tracing::info!(
                                "[{}] pending identity restored: step {}/5",
                                uid,
                                space.pending_identity().current_step()
                            );
                        }
                    }
                } else {
                    tracing::info!(
                        "[{}] no identity confirmed yet, awaiting first connection",
                        uid
                    );
                }
            }
        }

        let gateway = Arc::new(GatewayCenter::new(
            space.clone(),
            energy.clone(),
            cognitive.clone(),
            Arc::new(CategoryClassifier::new(&api_key, "deepseek-v4-flash")),
            bus.sender(),
            bus.subscribe(),
            knowledge.clone(),
            embedding.clone(),
            vector,
        ));

        let scheduler = Arc::new(SchedulerCenter::with_security(
            space.clone(),
            energy.clone(),
            knowledge,
            cognitive.clone(),
            gateway.clone(),
            bus.sender(),
            bus.subscribe(),
            1000,
            energy::DEFAULT_MAX_ENERGY,
            security.clone(),
            storage.clone(),
        ));

        let tool_ctx = Arc::new(self::tools::ToolContext::new(
            space.clone(),
            energy.clone(),
            scheduler.kg_handle(),
            security.clone(),
            energy::DEFAULT_MAX_ENERGY,
        ));
        let registry = Arc::new(self::tools::ToolRegistry::new(tool_ctx));
        cognitive.set_tools(registry);

        let skills = Arc::new(SkillEngine::new(storage.clone()));
        scheduler.set_skills(skills.clone());

        let key_rotation = Arc::new(Mutex::new(
            key_rotation::KeyRotation::new(90, 30, 5).unwrap_or_else(|e| {
                tracing::error!("Failed to initialize key rotation: {}", e);
                key_rotation::KeyRotation::new(90, 30, 5).unwrap()
            }),
        ));

        let audit_logger = AuditLogger::new();
        let permission_repo = PermissionRepository::new();
        let authz = Arc::new(AuthorizationChecker::new(permission_repo, audit_logger));

        Self {
            space,
            bus,
            gateway,
            energy,
            scheduler,
            cognitive,
            guard: security,
            storage,
            skills,
            key_rotation,
            authz,
            handles: Vec::new(),
            user_id: uid.to_string(),
            data_path: data_path.clone(),
        }
    }

    pub fn space(&self) -> &Space {
        &self.space
    }

    pub fn gateway(&self) -> &GatewayCenter {
        &self.gateway
    }

    pub fn scheduler(&self) -> &SchedulerCenter {
        &self.scheduler
    }

    pub fn confirm_identity(
        &self,
        name: String,
        mission: String,
        author: String,
        extra: std::collections::HashMap<String, String>,
    ) -> Result<(), String> {
        if self.space.identity_info().is_some() {
            return Err("identity already confirmed and cannot be changed".into());
        }
        self.space
            .confirm_identity(name.clone(), mission.clone(), author.clone(), extra.clone());
        let identity_path = self.data_path.join("identity.json");
        let data = serde_json::json!({
            "name": name,
            "mission": mission,
            "author": author,
            "extra": extra,
            "confirmed_at": chrono::Utc::now().timestamp(),
            "immutable": true,
        });
        let json = serde_json::to_string_pretty(&data).map_err(|e| format!("serialize: {}", e))?;
        std::fs::write(&identity_path, &json).map_err(|e| format!("write: {}", e))?;
        tracing::info!(
            "[{}] identity confirmed and persisted: {}",
            self.user_id,
            name
        );
        Ok(())
    }

    pub fn update_identity(
        &self,
        name: Option<String>,
        mission: Option<String>,
        author: Option<String>,
        extra: Option<std::collections::HashMap<String, String>>,
    ) -> Result<(), String> {
        if self.space.identity_info().is_none() {
            return Err("identity not yet confirmed".into());
        }
        self.space
            .update_identity(name.clone(), mission.clone(), author.clone(), extra.clone());
        let info = self.space.identity_info().unwrap();
        let identity_path = self.data_path.join("identity.json");
        let data = serde_json::json!({
            "name": info.system_name,
            "mission": info.mission,
            "author": info.author,
            "extra": info.extra,
            "confirmed_at": chrono::Utc::now().timestamp(),
        });
        let json = serde_json::to_string_pretty(&data).map_err(|e| format!("serialize: {}", e))?;
        std::fs::write(&identity_path, &json).map_err(|e| format!("write: {}", e))?;
        tracing::info!("[{}] identity updated", self.user_id);
        Ok(())
    }

    pub fn identity_step(
        &self,
        step: usize,
        value: String,
    ) -> Result<super::domain::cylinder::PendingIdentity, String> {
        if self.space.identity_info().is_some() {
            return Err("identity already confirmed".into());
        }
        if !(1..=5).contains(&step) {
            return Err("step must be 1-5".into());
        }
        if value.trim().is_empty() && step <= 3 {
            return Err("value is required for this step".into());
        }
        self.space.set_identity_step(step, value);
        let pending = self.space.pending_identity();
        let pending_clone = pending.clone();
        let identity_path = self.data_path.join("identity_pending.json");
        let json = serde_json::to_string_pretty(&serde_json::json!({
            "name": pending.name,
            "mission": pending.mission,
            "author": pending.author,
            "personality": pending.personality,
            "language": pending.language,
        }))
        .map_err(|e| format!("serialize: {}", e))?;
        std::fs::write(&identity_path, &json).map_err(|e| format!("write: {}", e))?;
        Ok(pending_clone)
    }

    pub fn confirm_ritual(&self) -> Result<super::domain::cylinder::IdentityInfo, String> {
        if self.space.identity_info().is_some() {
            return Err("identity already confirmed".into());
        }
        let pending = self.space.pending_identity();
        if !pending.is_complete() {
            return Err(format!(
                "ritual incomplete: step {} not done. Required: name(1), mission(2), author(3)",
                pending.current_step()
            ));
        }
        if !self.space.confirm_pending_identity() {
            return Err("confirmation failed".into());
        }
        let info = self
            .space
            .identity_info()
            .ok_or("identity not set after confirmation")?;
        let identity_path = self.data_path.join("identity.json");
        let data = serde_json::json!({
            "name": info.system_name,
            "mission": info.mission,
            "author": info.author,
            "extra": info.extra,
            "confirmed_at": chrono::Utc::now().timestamp(),
            "immutable": true,
        });
        let json = serde_json::to_string_pretty(&data).map_err(|e| format!("serialize: {}", e))?;
        std::fs::write(&identity_path, &json).map_err(|e| format!("write: {}", e))?;
        let _ = std::fs::remove_file(self.data_path.join("identity_pending.json"));
        tracing::info!(
            "[{}] identity ritual complete: {}",
            self.user_id,
            info.system_name
        );
        Ok(info)
    }

    pub fn start(&mut self) {
        let energy = self.energy.clone();
        let rx_e = self.bus.subscribe();
        self.handles.push(tokio::spawn(async move {
            EnergyCenter::run_detached(energy, rx_e).await;
        }));

        let scheduler = self.scheduler.clone();
        let rx_s = self.bus.subscribe();
        self.handles.push(tokio::spawn(async move {
            scheduler.run_with_rx(rx_s).await;
        }));

        tracing::info!("Engine ignited: scheduler-only mode");
    }

    pub fn start_quiet(&mut self) {
        let energy = self.energy.clone();
        let rx_e = self.bus.subscribe();
        self.handles.push(tokio::spawn(async move {
            EnergyCenter::run_detached(energy, rx_e).await;
        }));

        let scheduler = self.scheduler.clone();
        let rx_s = self.bus.subscribe();
        self.handles.push(tokio::spawn(async move {
            scheduler.run_quiet(rx_s).await;
        }));

        tracing::info!("Engine ignited: quiet mode (auto-save only, no cognitive ticks)");
    }

    pub fn start_with_interval(&mut self, tick_ms: u64) {
        let energy = self.energy.clone();
        let rx_e = self.bus.subscribe();
        self.handles.push(tokio::spawn(async move {
            EnergyCenter::run_detached(energy, rx_e).await;
        }));

        let scheduler = self.scheduler.clone();
        scheduler.set_tick_interval(tick_ms);
        let rx_s = self.bus.subscribe();
        self.handles.push(tokio::spawn(async move {
            scheduler.run_with_rx(rx_s).await;
        }));

        tracing::info!(
            "Engine ignited: full mode (tick={}ms, cognitive+LLM enabled)",
            tick_ms
        );
    }

    pub fn start_quiet_with_interval(&mut self, tick_ms: u64) {
        let energy = self.energy.clone();
        let rx_e = self.bus.subscribe();
        self.handles.push(tokio::spawn(async move {
            EnergyCenter::run_detached(energy, rx_e).await;
        }));

        let scheduler = self.scheduler.clone();
        scheduler.set_tick_interval(tick_ms);
        let rx_s = self.bus.subscribe();
        self.handles.push(tokio::spawn(async move {
            scheduler.run_quiet(rx_s).await;
        }));

        tracing::info!(
            "Engine ignited: quiet mode (tick={}ms, auto-save only)",
            tick_ms
        );
    }

    pub fn save_all(&self) -> Result<(), String> {
        let kg = self.scheduler.kg_handle();
        self.storage.save_all(&self.space, &kg)
    }

    pub fn backup(&self) -> Result<String, String> {
        self.storage.backup()
    }

    pub fn list_backups(&self) -> Vec<self::storage::BackupInfo> {
        self.storage.list_backups()
    }

    pub fn final_save(&self) {
        let kg = self.scheduler.kg_handle();
        match self.storage.save_all(&self.space, &kg) {
            Ok(()) => tracing::info!("Final save complete — all data persisted."),
            Err(e) => tracing::error!("Final save FAILED: {} — data may be lost!", e),
        }
        if let Err(e) = self.storage.checkpoint() {
            tracing::warn!("Final WAL checkpoint failed: {}", e);
        }
    }

    pub fn request_shutdown(&self) {
        let _ = self.bus.sender().send(bus::EngineEvent::Shutdown);
    }

    // ── 权限管理方法 ──

    pub fn grant_permission(
        &self,
        permission: crate::domain::permission::Permission,
    ) -> Result<String, crate::domain::permission::AuthzError> {
        self.authz.grant_permission(permission)
    }

    pub fn revoke_permission(
        &self,
        permission_id: &str,
    ) -> Result<(), crate::domain::permission::AuthzError> {
        self.authz.revoke_permission(permission_id)
    }

    pub fn get_user_permissions(
        &self,
        user_id: &str,
    ) -> Result<Vec<crate::domain::permission::Permission>, crate::domain::permission::AuthzError>
    {
        self.authz.get_user_permissions(user_id)
    }

    pub fn get_resource_permissions(
        &self,
        resource_id: &str,
        resource_type: crate::domain::permission::ResourceType,
    ) -> Result<Vec<crate::domain::permission::Permission>, crate::domain::permission::AuthzError>
    {
        self.authz
            .get_resource_permissions(resource_id, resource_type)
    }

    pub async fn check_authorization(
        &self,
        user_id: &str,
        resource_id: &str,
        resource_type: crate::domain::permission::ResourceType,
        action: crate::domain::permission::Action,
        tenant_id: &str,
    ) -> Result<(), crate::domain::permission::AuthzError> {
        self.authz
            .check(user_id, resource_id, resource_type, action, tenant_id)
            .await
    }

    pub fn get_audit_logs(
        &self,
        offset: usize,
        limit: usize,
    ) -> Result<
        (Vec<crate::domain::permission::AuditLogEntry>, usize),
        crate::domain::permission::AuthzError,
    > {
        self.authz.get_audit_logs(offset, limit)
    }

    pub async fn shutdown(mut self) {
        tracing::info!("Engine shutting down — saving all data...");
        let kg = self.scheduler.kg_handle();
        match self.storage.save_all(&self.space, &kg) {
            Ok(()) => tracing::info!("Final save complete."),
            Err(e) => tracing::error!("Final save failed: {}", e),
        }
        if let Err(e) = self.storage.checkpoint() {
            tracing::warn!("Final WAL checkpoint failed: {}", e);
        }
        let _ = self.bus.sender().send(bus::EngineEvent::Shutdown);
        for handle in self.handles.drain(..) {
            let _ = handle.await;
        }
    }
}
