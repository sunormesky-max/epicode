pub mod adaptive;
pub mod assembler;
pub mod audit;
pub mod auto_pipeline;
pub mod bus;
pub mod cluster;
pub mod cache;
pub mod classifier;
pub mod cognitive;
pub mod decision_center;
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
pub mod plugin;
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

use parking_lot::Mutex;
use std::sync::Arc;
use tokio::task::JoinHandle;

use crate::domain::space::Space;

use self::audit::AuditLogger;
use self::bus::EventBus;
use self::cluster::{ClusterConfig, ClusterHandle, DistributedBus};
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
    pub cluster: Option<Arc<ClusterHandle>>,
    #[allow(dead_code)]
    dist_bus: Option<DistributedBus>,
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
    pub plugin_registry: Arc<plugin::PluginRegistry>,
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

        let cluster_config = ClusterConfig::from_env();
        let (cluster, dist_bus) = if cluster_config.enabled {
            let node_id = uuid::Uuid::new_v4();
            let gossip = Arc::new(self::cluster::GossipState::new(
                node_id,
                cluster_config.listen_addr.clone(),
                cluster_config.heartbeat_interval_ms,
                cluster_config.heartbeat_timeout_ms,
            ));
            let ring = parking_lot::RwLock::new(self::cluster::HashRing::new(cluster_config.vnode_count));
            let handle = Arc::new(ClusterHandle {
                ring,
                gossip,
                config: cluster_config.clone(),
            });
            let dbus = DistributedBus::new(bus.sender(), Some(handle.clone()));
            tracing::info!("[{}] cluster mode enabled: node_id={}, listen={}", uid, node_id, cluster_config.listen_addr);
            (Some(handle), Some(dbus))
        } else {
            tracing::debug!("[{}] cluster mode disabled", uid);
            (None, None)
        };

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
                panic!(
                    "FATAL: failed to initialize storage at {}: {}",
                    data_path.display(),
                    e
                );
            });
            match self::crypto::CryptoEngine::from_env() {
                Ok(crypto) => {
                    tracing::info!("Encryption enabled (AES-256-GCM) for user '{}'", uid);
                    sm.with_encryption(crypto, uid)
                }
                Err(_) => {
                    tracing::debug!(
                        "Encryption disabled (no EPICODE_MASTER_KEY or TETRAMEM_MASTER_KEY)"
                    );
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

        let decision_center = Arc::new(crate::engine::decision_center::DecisionCenter::new(cognitive.clone()));
        let scheduler = Arc::new(SchedulerCenter::with_security(
            space.clone(),
            energy.clone(),
            knowledge,
            cognitive.clone(),
            decision_center,
            gateway.clone(),
            bus.sender(),
            bus.subscribe(),
            1000,
            energy::DEFAULT_MAX_ENERGY,
            security.clone(),
            storage.clone(),
        ));

        let plugin_registry = Arc::new(self::plugin::PluginRegistry::new());
        let builtin_plugin = Arc::new(self::plugin::BuiltinToolPlugin);
        if let Err(e) = plugin_registry.register(builtin_plugin) {
            tracing::warn!("Failed to register builtin plugin: {}", e);
        }

        let tool_ctx = Arc::new(self::tools::ToolContext::new(
            space.clone(),
            energy.clone(),
            scheduler.kg_handle(),
            security.clone(),
            energy::DEFAULT_MAX_ENERGY,
        ));
        let registry = Arc::new(self::tools::ToolRegistry::new_with_plugin_registry(
            tool_ctx,
            Some(plugin_registry.clone()),
        ));
        cognitive.set_tools(registry);

        let skills = Arc::new(SkillEngine::new(storage.clone()));
        scheduler.set_skills(skills.clone());

        let key_rotation = Arc::new(Mutex::new(key_rotation::KeyRotation::new(90, 30, 5)));

        let audit_logger = AuditLogger::new();
        let permission_repo = PermissionRepository::new();
        let authz = Arc::new(AuthorizationChecker::new(permission_repo, audit_logger));

        Self {
            space,
            bus,
            cluster,
            dist_bus,
            gateway,
            energy,
            scheduler,
            cognitive,
            guard: security,
            storage,
            skills,
            key_rotation,
            authz,
            plugin_registry,
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
        let json = serde_json::to_string_pretty(&data).map_err(|e| format!("serialize: {e}"))?;
        std::fs::write(&identity_path, &json).map_err(|e| format!("write: {e}"))?;
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
        let json = serde_json::to_string_pretty(&data).map_err(|e| format!("serialize: {e}"))?;
        std::fs::write(&identity_path, &json).map_err(|e| format!("write: {e}"))?;
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
        .map_err(|e| format!("serialize: {e}"))?;
        std::fs::write(&identity_path, &json).map_err(|e| format!("write: {e}"))?;
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
        let json = serde_json::to_string_pretty(&data).map_err(|e| format!("serialize: {e}"))?;
        std::fs::write(&identity_path, &json).map_err(|e| format!("write: {e}"))?;
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

    // High-level API facade methods -----------------------------------------------------------
    // The methods below are thin delegations to scheduler/security subsystems. They exist to
    // discourage API handlers from reaching directly into `engine.scheduler.*` and to make the
    // public surface of Engine explicit. Over time, remaining direct field access should be
    // migrated behind similar methods.

    pub fn validate_content(&self, content: &str) -> Result<(), self::security::SecurityResult> {
        self.guard.validate_content(content)
    }

    pub fn validate_query(&self, query: &str) -> Result<(), self::security::SecurityResult> {
        self.guard.validate_query(query)
    }

    pub fn validate_labels(&self, labels: &[String]) -> Result<(), self::security::SecurityResult> {
        self.guard.validate_labels(labels)
    }

    pub fn remember(
        &self,
        content: &str,
    ) -> Result<(crate::domain::tetra::TetraId, Vec<String>), String> {
        self.scheduler.api_remember(content)
    }

    pub fn ask(&self, question: &str, depth: usize) -> Result<serde_json::Value, String> {
        self.scheduler.api_ask(question, depth)
    }

    pub fn recall(&self, query: &str, depth: usize) -> Result<serde_json::Value, String> {
        self.scheduler.api_recall(query, depth)
    }

    pub fn search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<
        Vec<(
            crate::domain::tetra::TetraId,
            f64,
            f64,
            crate::domain::tetra::MemoryPayload,
        )>,
        String,
    > {
        self.scheduler.api_search(query, limit)
    }

    pub fn search_filtered(
        &self,
        query: &str,
        limit: usize,
        filters: Option<&self::search_engine::SearchFilters>,
    ) -> Result<
        Vec<(
            crate::domain::tetra::TetraId,
            f64,
            f64,
            crate::domain::tetra::MemoryPayload,
        )>,
        String,
    > {
        self.scheduler.api_search_filtered(query, limit, filters)
    }

    pub fn create_memory_with_time(
        &self,
        content: &str,
        labels: Vec<String>,
        timestamp: i64,
    ) -> Result<crate::domain::tetra::TetraId, String> {
        self.scheduler
            .api_create_memory_with_time(content, labels, timestamp)
    }

    pub fn list_nodes(
        &self,
        limit: usize,
    ) -> Vec<(
        crate::domain::tetra::TetraId,
        crate::domain::tetra::MemoryPayload,
    )> {
        self.scheduler.api_list_nodes_limit(limit)
    }

    pub fn get_node(
        &self,
        id: crate::domain::tetra::TetraId,
    ) -> Option<crate::domain::tetra::MemoryPayload> {
        self.scheduler.api_get_node(id)
    }

    pub fn delete_memory(
        &self,
        id: crate::domain::tetra::TetraId,
    ) -> Result<crate::domain::tetra::TetraId, String> {
        self.scheduler.api_delete_memory(id)
    }

    pub fn restore_memory(
        &self,
        id: crate::domain::tetra::TetraId,
    ) -> Result<crate::domain::tetra::TetraId, String> {
        self.scheduler.api_restore_memory(id)
    }

    pub fn list_deleted_memories(&self) -> Result<Vec<self::storage::DeletedMemoryInfo>, String> {
        self.scheduler.api_list_deleted_memories()
    }

    pub fn pulse(
        &self,
        origin: crate::domain::tetra::TetraId,
        ttl: u32,
    ) -> Result<crate::domain::pulse::PulseResult, String> {
        self.scheduler.api_pulse(origin, ttl)
    }

    pub fn dream(&self) -> Result<String, String> {
        self.scheduler.api_dream()
    }

    pub fn stats(&self) -> self::gateway::SpaceStats {
        self.scheduler.api_stats()
    }

    pub fn get_relations(
        &self,
        id: crate::domain::tetra::TetraId,
    ) -> Vec<(crate::domain::tetra::TetraId, String, f64)> {
        self.scheduler.api_get_relations(id)
    }

    pub fn get_concepts(&self) -> Vec<(String, usize)> {
        self.scheduler.api_get_concepts()
    }

    pub fn reason_analogies(&self, min_confidence: f64) -> Vec<serde_json::Value> {
        self.scheduler.api_reason_analogies(min_confidence)
    }

    pub fn reason_patterns(&self) -> Vec<String> {
        self.scheduler.api_reason_patterns()
    }

    // Space / cylinder / identity accessors --------------------------------------------------

    pub fn cylinder_health(&self) -> crate::domain::cylinder::HealthReport {
        self.space.cylinder_health()
    }

    pub fn cylinder_radius(&self) -> f64 {
        self.space.cylinder_radius()
    }

    pub fn cylinder_height(&self) -> f64 {
        self.space.cylinder_height()
    }

    pub fn cylinder_port_count(&self) -> usize {
        self.space.cylinder_port_count()
    }

    pub fn is_identity_confirmed(&self) -> bool {
        self.space.is_identity_confirmed()
    }

    pub fn identity_info(&self) -> Option<crate::domain::cylinder::IdentityInfo> {
        self.space.identity_info()
    }

    pub fn find_clusters(&self) -> Vec<crate::domain::space::Cluster> {
        self.space.find_clusters()
    }

    pub fn get_tetrahedron(
        &self,
        id: crate::domain::tetra::TetraId,
    ) -> Option<crate::domain::tetra::Tetrahedron> {
        self.space.get_tetrahedron(id)
    }

    // Security / guard accessors ---------------------------------------------------------------

    pub fn guard_stats(&self) -> self::security::SecurityStats {
        self.guard.stats()
    }

    pub fn guard_audit_log(&self, limit: usize) -> Vec<self::security::AuditEntry> {
        self.guard.audit_log(limit)
    }

    pub fn guard_config(&self) -> &self::security::SecurityConfig {
        &self.guard.config
    }

    // Gateway / cache accessors ----------------------------------------------------------------

    pub fn cache_stats_snapshot(&self) -> (u64, u64, u64, u64, u64, f64, f64, f64) {
        self.gateway.cache_stats_snapshot()
    }

    pub fn clear_query_cache(&self) {
        self.gateway.clear_query_cache()
    }

    // Storage accessors ------------------------------------------------------------------------

    pub fn storage_list_backups(&self) -> Vec<self::storage::BackupInfo> {
        self.storage.list_backups()
    }

    pub fn storage_tetra_count(&self) -> usize {
        self.storage.tetra_count()
    }

    pub fn storage_relation_count(&self) -> usize {
        self.storage.relation_count()
    }

    pub fn storage_get_meta(&self, key: &str) -> Option<String> {
        self.storage.get_meta(key)
    }

    pub fn storage_set_meta_batch(&self, refs: &[(&str, &str)]) -> Result<(), String> {
        self.storage.set_meta_batch(refs)
    }

    // Cognitive accessors ----------------------------------------------------------------------

    pub fn cognitive_enabled(&self) -> bool {
        self.cognitive.enabled()
    }

    // Async wrappers for potentially blocking operations ---------------------------------------
    // These offload the underlying synchronous Engine work to tokio's blocking thread pool so
    // that HTTP handlers in routes.rs do not block tokio worker threads during SQLite I/O,
    // ONNX inference, or synchronous HTTP embedding fallback calls.

    pub async fn remember_async(
        self: Arc<Self>,
        content: &str,
    ) -> Result<(crate::domain::tetra::TetraId, Vec<String>), String> {
        let content = content.to_string();
        crate::api::server::blocking(move || self.remember(&content)).await?
    }

    pub async fn ask_async(
        self: Arc<Self>,
        question: &str,
        depth: usize,
    ) -> Result<serde_json::Value, String> {
        let question = question.to_string();
        crate::api::server::blocking(move || self.ask(&question, depth)).await?
    }

    pub async fn recall_async(
        self: Arc<Self>,
        query: &str,
        depth: usize,
    ) -> Result<serde_json::Value, String> {
        let query = query.to_string();
        crate::api::server::blocking(move || self.recall(&query, depth)).await?
    }

    pub async fn search_async(
        self: Arc<Self>,
        query: &str,
        limit: usize,
    ) -> Result<
        Vec<(
            crate::domain::tetra::TetraId,
            f64,
            f64,
            crate::domain::tetra::MemoryPayload,
        )>,
        String,
    > {
        let query = query.to_string();
        crate::api::server::blocking(move || self.search(&query, limit)).await?
    }

    pub async fn search_filtered_async(
        self: Arc<Self>,
        query: String,
        limit: usize,
        filters: Option<self::search_engine::SearchFilters>,
    ) -> Result<
        Vec<(
            crate::domain::tetra::TetraId,
            f64,
            f64,
            crate::domain::tetra::MemoryPayload,
        )>,
        String,
    > {
        crate::api::server::blocking(move || self.search_filtered(&query, limit, filters.as_ref()))
            .await?
    }

    pub async fn create_memory_with_time_async(
        self: Arc<Self>,
        content: String,
        labels: Vec<String>,
        timestamp: i64,
    ) -> Result<crate::domain::tetra::TetraId, String> {
        crate::api::server::blocking(move || {
            self.create_memory_with_time(&content, labels, timestamp)
        })
        .await?
    }

    pub async fn delete_memory_async(
        self: Arc<Self>,
        id: crate::domain::tetra::TetraId,
    ) -> Result<crate::domain::tetra::TetraId, String> {
        crate::api::server::blocking(move || self.delete_memory(id)).await?
    }

    pub async fn restore_memory_async(
        self: Arc<Self>,
        id: crate::domain::tetra::TetraId,
    ) -> Result<crate::domain::tetra::TetraId, String> {
        crate::api::server::blocking(move || self.restore_memory(id)).await?
    }

    pub async fn pulse_async(
        self: Arc<Self>,
        origin: crate::domain::tetra::TetraId,
        ttl: u32,
    ) -> Result<crate::domain::pulse::PulseResult, String> {
        crate::api::server::blocking(move || self.pulse(origin, ttl)).await?
    }

    pub async fn dream_async(self: Arc<Self>) -> Result<String, String> {
        crate::api::server::blocking(move || self.dream()).await?
    }

    pub async fn reason_analogies_async(
        self: Arc<Self>,
        min_confidence: f64,
    ) -> Result<Vec<serde_json::Value>, String> {
        crate::api::server::blocking(move || self.reason_analogies(min_confidence)).await
    }

    pub async fn reason_patterns_async(self: Arc<Self>) -> Result<Vec<String>, String> {
        crate::api::server::blocking(move || self.reason_patterns()).await
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
