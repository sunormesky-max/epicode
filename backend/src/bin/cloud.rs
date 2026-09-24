//! Epicode Cloud 二进制入口。
//!
//! 本文件只负责：模块声明、全局初始化（main）、路由装配、后台任务与优雅关闭。
//! 所有业务逻辑分散在 `cloud/` 子模块中：
//!
//! - [`state`]        共享状态 / 限流桶 / 常量
//! - [`helpers`]      跨 handler 复用的辅助函数
//! - [`auth`]         鉴权 + 限流中间件
//! - [`tcp`]          TCP MCP 服务器
//! - [`health`]       健康检查 / 公共统计 / 注册 / 登录 / agent guide
//! - [`memory`]       记忆 CRUD / search / recall / ask / graph / timeline / docs
//! - [`identity`]     身份仪式相关端点
//! - [`archive`]      档案库 API
//! - [`skill`]        Skills API
//! - [`subaccount`]   子账号 API
//! - [`apikey`]      用户自助密钥旅程(P20/P21重建: masked/reveal/reset)
//! - [`library`]     L1图书馆(全局知识资产: collections/acl/ingest/search)
//! - [`admin`]        管理端 + 静态入口（panel / swagger / openapi / smrp-spec）
//! - [`mcp_endpoint`] HTTP /mcp JSON-RPC 端点

#[path = "cloud/admin.rs"]
mod admin;
#[path = "cloud/apikey.rs"]
mod apikey;
#[path = "cloud/archive.rs"]
mod archive;
#[path = "cloud/auth.rs"]
mod auth;
#[path = "cloud/consciousness.rs"]
mod consciousness;
#[path = "cloud/health.rs"]
mod health;
#[path = "cloud/helpers.rs"]
mod helpers;
#[path = "cloud/identity.rs"]
mod identity;
#[path = "cloud/library.rs"]
mod library;
#[path = "cloud/mcp_endpoint.rs"]
mod mcp_endpoint;
#[path = "cloud/memory.rs"]
mod memory;
#[path = "cloud/runtime.rs"]
mod runtime;
#[path = "cloud/skill.rs"]
mod skill;
#[path = "cloud/state.rs"]
mod state;
#[path = "cloud/subaccount.rs"]
mod subaccount;
#[path = "cloud/tcp.rs"]
mod tcp;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::routing::{delete, get, post};
use axum::{middleware, Router};
use parking_lot::Mutex;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use epicode::engine::skills::SkillEngine;
use epicode::engine::storage::StorageManager;
use epicode::engine::user_manager::UserManager;
use epicode::engine::Engine;

use helpers::security_headers_middleware;
use state::{CloudState, RateBucket, RATE_LIMIT_WINDOW_SECS};
use tcp::run_tcp_server;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()))
        .init();

    tracing::info!("Epicode Cloud v1.0.0 — starting...");

    let admin_key = std::env::var("TETRAMEM_ADMIN_KEY")
        .expect("FATAL: TETRAMEM_ADMIN_KEY environment variable must be set");

    let listen_addr =
        std::env::var("TETRAMEM_LISTEN_ADDR").unwrap_or_else(|_| "127.0.0.1:9111".into());

    let cors_origin = std::env::var("TETRAMEM_CORS_ORIGIN")
        .unwrap_or_else(|e| { tracing::warn!("[CORS] TETRAMEM_CORS_ORIGIN not set or invalid ({}), falling back to https://epicode.cn", e); "https://epicode.cn".into() });

    let data_dir = std::path::PathBuf::from(
        std::env::var("TETRAMEM_DATA_DIR").unwrap_or_else(|_| "data".into()),
    );
    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        tracing::error!("FATAL: cannot create data dir {:?}: {}", data_dir, e);
        std::process::exit(1);
    }

    let shared_vector = Engine::load_shared_vector();
    // L1 图书馆: 全局库(独立SQLite, 复用共享VectorLayer — 零额外模型内存)
    let library_state = {
        let lib_db = std::path::PathBuf::from(
            std::env::var("TETRAMEM_DATA_DIR").unwrap_or_else(|_| "/var/lib/tetramem".to_string()),
        )
        .join("library.db");
        match epicode::engine::library::LibraryStore::open(&lib_db, shared_vector.clone()) {
            Ok(ls) => {
                tracing::info!(
                    "[Library] store ready ({} chunks): {}",
                    ls.chunk_count(),
                    lib_db.display()
                );
                std::sync::Arc::new(ls)
            }
            Err(e) => {
                tracing::error!("[Library] open failed: {} — degraded memory mode", e);
                std::sync::Arc::new(
                    epicode::engine::library::LibraryStore::open(
                        std::path::Path::new(":memory:"),
                        shared_vector.clone(),
                    )
                    .expect("memory library"),
                )
            }
        }
    };
    let user_mgr = if let Some(ref sv) = shared_vector {
        tracing::info!("Shared VectorLayer loaded for cloud API");
        Arc::new(UserManager::with_shared_vector(&data_dir, sv.clone()))
    } else {
        Arc::new(UserManager::new(&data_dir))
    };
    tracing::info!("UserManager initialized, data_dir={:?}", data_dir);

    let pub_skills_dir = data_dir.join("pub_skills");
    let pub_skills = {
        let pub_storage =
            Arc::new(StorageManager::new(&pub_skills_dir).expect("pub_skills storage init failed"));
        Arc::new(SkillEngine::new(pub_storage))
    };
    // 紧急修复：启动时 set_vector 触发 254 技能全量 reindex，在主线程同步执行死锁。
    // 改为延迟设置：启动完成后在 tokio task 里异步设置（不阻塞 bind）。
    // skill_execute 在 vector 未设置时会降级到关键词匹配，功能不受影响。
    if let Some(ref sv) = shared_vector {
        let sv_clone = sv.clone();
        let ps_clone = pub_skills.clone();
        // 延迟到 runtime 启动后异步执行（main 末尾 tokio::spawn）
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(5));
            tracing::info!("[Startup] async pub_skills.set_vector (delayed)");
            ps_clone.set_vector(sv_clone);
            tracing::info!("[Startup] pub_skills vector index ready");
        });
    }
    tracing::info!("Public SkillEngine initialized (vector deferred)");

    // 内存治理：每 10 分钟调用 malloc_trim 把 glibc 释放但滞留在堆 bins 的内存归还 OS。
    // AutoDream 大整理（如 830 簇聚类）后 RSS 常驻高位不回落，是内存谷底逐周期加深的机制。
    #[cfg(target_os = "linux")]
    {
        extern "C" {
            fn malloc_trim(pad: usize) -> i32;
        }
        std::thread::spawn(|| loop {
            std::thread::sleep(std::time::Duration::from_secs(600));
            let freed = unsafe { malloc_trim(0) };
            if freed == 1 {
                tracing::info!("[MemGov] malloc_trim: heap memory returned to OS");
            }
        });
    }

    user_mgr.set_pub_skills(pub_skills.clone());

    let rate_limits: Arc<Mutex<HashMap<String, RateBucket>>> = Arc::new(Mutex::new(HashMap::new()));
    let api_call_counts: Arc<Mutex<HashMap<String, u64>>> = Arc::new(Mutex::new(HashMap::new()));
    let api_calls_daily: Arc<Mutex<HashMap<String, HashMap<String, u64>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    // 智能化突破：创建认知洞察广播通道
    let (insight_tx, _insight_rx) =
        tokio::sync::broadcast::channel::<epicode::engine::insight::InsightEvent>(256);
    // 把 insight_tx 注入所有用户的引擎，让认知引擎能 emit 洞察事件
    {
        let _tx_clone = insight_tx.clone();
        // 为当前和未来的用户引擎设置 insight channel
        // UserManager 在 get_engine 时创建引擎，我们需要在引擎创建后注入
        // 最简方案：存到 CloudState，SSE 直接订阅
    }

    let state = CloudState {
        user_mgr: user_mgr.clone(),
        admin_key,
        rate_limits: rate_limits.clone(),
        active_tasks: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        pub_skills: pub_skills.clone(),
        api_call_counts: api_call_counts.clone(),
        api_calls_daily: api_calls_daily.clone(),
        insight_tx: Arc::new(insight_tx),
        startup_phase: Arc::new(std::sync::atomic::AtomicU8::new(0)),
        // α1fix: 启动时恢复持久化的 primary 绑定 (关系保留; 激活仍靠 sidecar heartbeat 复活)
        primary_executors: Arc::new(parking_lot::RwLock::new(runtime::load_bindings())),
        stream_tickets: Arc::new(parking_lot::Mutex::new(HashMap::new())),
        library: library_state,
    };

    let allowed_headers: Vec<axum::http::HeaderName> = vec![
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderName::from_static("x-api-key"),
        axum::http::HeaderName::from_static("x-admin-key"),
        axum::http::HeaderName::from_static("x-invite-code"),
    ];
    let cors = CorsLayer::new()
        .allow_origin([
            cors_origin
                .parse::<axum::http::HeaderValue>()
                .unwrap_or_else(|_| "https://epicode.cn".parse().unwrap()),
            "http://localhost:3000".parse().unwrap(), // 本地 dev（kimi #11）
            "http://127.0.0.1:3000".parse().unwrap(),
        ])
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PUT,
            axum::http::Method::DELETE,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers(allowed_headers);

    let app = Router::new()
        .route("/health", get(health::health))
        .route("/v1/health", get(health::health))
        .route("/ready", get(health::ready))
        .route("/v1/stream", get(health::sse_stream))
        .route("/v1/stream/ticket", post(health::mint_stream_ticket))
        .route("/v1/agent-guide", get(health::agent_guide))
        .route("/v1/smrp", get(admin::smrp_spec))
        .route("/stats/public", get(health::public_stats))
        .route("/docs", get(admin::swagger_ui))
        .route("/openapi.yaml", get(admin::openapi_spec))
        .route("/register", post(health::register_user))
        .route("/v1/login", post(health::login_user))
        .route("/v1/api-key", get(apikey::api_key_masked))
        .route("/v1/api-key/reveal", post(apikey::api_key_reveal))
        .route("/v1/api-key/reset", post(apikey::api_key_reset))
        .route("/v1/logout", post(health::logout_user))
        .route("/v1/digest", post(memory::digest_content))
        .route("/v1/remember", post(memory::remember))
        .route("/v1/ingest/batch", post(memory::ingest_batch))
        .route("/v1/library/collections", post(library::create_collection))
        .route("/v1/library/acl", post(library::set_acl))
        .route("/v1/library/ingest", post(library::ingest))
        .route("/v1/library/search", post(library::search))
        .route(
            "/v1/library/requests",
            post(library::submit_request).get(library::list_requests),
        )
        .route("/v1/library/requests/handle", post(library::handle_request))
        .route("/v1/library/visibility", post(library::set_visibility))
        .route("/v1/search", post(memory::search))
        .route("/v1/recall", post(memory::recall))
        .route("/v1/ask", post(memory::ask))
        .route("/v1/nodes", post(memory::create_node))
        .route("/v1/nodes/:id", get(memory::get_node))
        .route("/v1/knowledge", post(memory::knowledge))
        .route("/v1/graph/analysis", get(memory::graph_analysis))
        .route("/v1/graph/export", get(memory::graph_export))
        .route("/v1/stats", get(memory::user_stats))
        .route(
            "/v1/identity",
            get(identity::user_identity).put(identity::update_identity_http),
        )
        .route("/v1/personality/export", get(memory::export_personality))
        .route("/v1/personality/import", post(memory::import_personality))
        .route("/v1/knowledge/cards", get(memory::knowledge_cards))
        .route("/v1/identity/confirm", post(identity::confirm_identity))
        .route("/v1/identity/step", post(identity::identity_step_http))
        .route(
            "/v1/identity/finalize",
            post(identity::identity_finalize_http),
        )
        .route("/v1/timeline", get(memory::timeline))
        .route(
            "/v1/memories/:id",
            get(memory::get_memory)
                .delete(memory::delete_memory)
                .put(memory::update_memory_content),
        )
        .route("/v1/memories/:id/forget", post(memory::forget_memory))
        .route(
            "/v1/memories/batch-delete",
            post(memory::batch_delete_memories),
        )
        .route(
            "/v1/memories/bulk-quarantine",
            post(memory::bulk_quarantine),
        )
        .route("/v1/memories/bulk-restore", post(memory::bulk_restore))
        .route("/v1/memories/noise-stats", get(memory::noise_stats))
        .route(
            "/v1/memories/noise-candidates",
            get(memory::noise_candidates),
        )
        .route("/v1/kg/quality", get(memory::kg_quality))
        .route("/v1/operations/dry-run", post(memory::operations_dry_run))
        .route("/v1/operations/confirm", post(memory::operations_confirm))
        .route(
            "/v1/operations/audit-log",
            get(memory::operations_audit_log),
        )
        // P4-2: Contradiction Queue (矛盾队列)
        .route(
            "/v1/memories/contradictions",
            post(memory::list_contradictions),
        )
        .route(
            "/v1/memories/contradictions/resolve",
            post(memory::resolve_contradiction),
        )
        .route(
            "/v1/memories/contradictions/archive",
            post(memory::archive_contradiction),
        )
        // P4-3: Project Switch (项目隔离)
        .route("/v1/projects", get(memory::list_projects))
        .route("/v1/projects/switch", post(memory::switch_project))
        .route("/v1/projects/current", get(memory::current_project))
        // P4-4: Enforced Rules Lifecycle (硬约束生命周期)
        .route("/v1/rules/learn", post(memory::learn_rule))
        .route("/v1/rules/list", get(memory::list_rules))
        .route("/v1/rules/audit", get(memory::audit_rules))
        .route("/v1/rules/:id", delete(memory::revoke_rule))
        .route("/v1/docs/import", post(memory::import_doc))
        .route("/v1/drive/inbox", get(memory::drive_inbox))
        .route("/v1/drive/ingested", post(memory::drive_ingested))
        .route("/v1/drive/policy", get(memory::drive_policy))
        .route("/v1/persona/ready", get(health::persona_ready))
        .route("/v1/drive/ack", post(memory::drive_ack))
        .route("/v1/drive/evolution", get(memory::drive_evolution))
        .route("/v1/consciousness/think", post(consciousness::think))
        .route("/v1/runtime/register", post(runtime::register))
        .route("/v1/runtime/unregister", post(runtime::unregister))
        .route("/v1/runtime/status", get(runtime::status))
        .route("/v1/runtime/heartbeat", post(runtime::heartbeat))
        .route("/v1/runtime/manifest", get(runtime::manifest))
        .route("/v1/docs", get(memory::list_docs))
        .route("/v1/archive/tree", get(archive::archive_tree))
        .route("/v1/archive/node", post(archive::archive_create_node))
        .route(
            "/v1/archive/node/:id",
            get(archive::archive_get_node)
                .put(archive::archive_edit_node)
                .delete(archive::archive_delete_node),
        )
        .route("/v1/archive/merge", post(archive::archive_merge))
        .route("/v1/archive/move", post(archive::archive_move))
        .route("/v1/archive/import", post(archive::archive_import))
        .route("/admin/panel", get(admin::admin_panel))
        .route("/admin/users", get(admin::admin_list_users))
        .route("/admin/stats", get(admin::admin_stats))
        .route("/admin/users/list", get(admin::admin_users_list))
        .route("/admin/users/:user_id", get(admin::admin_user_detail))
        .route(
            "/admin/users/:user_id/reset-key",
            post(admin::admin_reset_key),
        )
        .route(
            "/admin/users/:user_id/set-password",
            post(admin::admin_set_password),
        )
        .route(
            "/admin/users/:user_id/set-plan",
            post(admin::admin_set_plan),
        )
        .route(
            "/admin/users/:user_id/delete",
            post(admin::admin_delete_user),
        )
        .route(
            "/admin/users/:user_id/memories/:id/purge",
            post(admin::admin_purge_memory),
        )
        .route(
            "/admin/invites/generate",
            post(admin::admin_generate_invites),
        )
        .route("/admin/invites/list", get(admin::admin_list_invites))
        .route("/admin/backup", post(admin::admin_backup_all))
        .route("/admin/backup/:user_id", post(admin::admin_backup_user))
        .route(
            "/admin/backups/:user_id",
            get(admin::admin_list_user_backups),
        )
        .route(
            "/admin/purge-pub-skills",
            post(admin::admin_purge_pub_skills),
        )
        .route("/admin/reindex", post(admin::admin_reindex))
        .route("/admin/scavenge", post(admin::admin_scavenge))
        .route("/admin/skills/pending", get(admin::admin_pending_skills))
        .route(
            "/admin/skills/resync-system",
            post(admin::admin_resync_system_skills),
        )
        .route(
            "/admin/skills/optimize-descriptions",
            post(admin::admin_optimize_descriptions),
        )
        .route(
            "/admin/skills/:id/approve",
            post(admin::admin_approve_skill),
        )
        .route("/admin/skills/:id/reject", post(admin::admin_reject_skill))
        .route("/mcp", post(mcp_endpoint::mcp_endpoint))
        .route("/v1/subaccounts", get(subaccount::list_subaccounts))
        .route(
            "/v1/subaccounts/create",
            post(subaccount::create_subaccount),
        )
        .route(
            "/v1/subaccounts/:user_id/revoke",
            post(subaccount::revoke_subaccount),
        )
        .route(
            "/v1/skills",
            get(skill::list_skills).post(skill::create_skill),
        )
        .route("/v1/skills/pending", get(skill::list_pending_skills))
        .route("/v1/skills/search", post(skill::search_skills))
        .route("/v1/skills/public", get(skill::list_public_skills))
        .route("/v1/skills/explore", get(skill::explore_public_skills))
        .route("/v1/skills/public/:id/pull", post(skill::pull_public_skill))
        .route(
            "/v1/skills/:id",
            get(skill::get_skill)
                .put(skill::update_skill)
                .delete(skill::delete_skill),
        )
        .route("/v1/skills/:id/publish", post(skill::publish_skill))
        .route("/v1/skills/:id/link", post(skill::link_skill_memory))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ))
        .layer(middleware::from_fn(security_headers_middleware))
        .layer(middleware::from_fn(helpers::request_id_middleware))
        .layer(tower_http::limit::RequestBodyLimitLayer::new(
            2 * 1024 * 1024,
        ))
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    {
        // L1 cache-cap: prewarm可关(EPICODE_PREWARM_PRIMARY=0) — sunorme引擎3-4G驻留是0G主因
        // 关闭后引擎按需懒加载(首次访问时), 避免开机即占满内存
        if std::env::var("EPICODE_PREWARM_PRIMARY")
            .map(|v| v != "0")
            .unwrap_or(true)
        {
            let mgr = user_mgr.clone();
            tokio::spawn(async move {
                let _ = tokio::task::spawn_blocking(move || mgr.prewarm_primary()).await;
            });
        } else {
            tracing::info!("[UserManager] prewarm_primary SKIPPED (EPICODE_PREWARM_PRIMARY=0) — engine will lazy-load on first access");
        }
    }

    // Phase 3: 标记 Ready（必须在 with_state move 之前 clone）
    {
        let phase_clone = state.startup_phase.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            phase_clone.store(1u8, std::sync::atomic::Ordering::Relaxed);
            tracing::info!("[Startup] service marked READY");
        });
    }

    let active_tasks_counter = state.active_tasks.clone();
    let app = app.with_state(state);

    let addr: SocketAddr = match listen_addr.parse() {
        Ok(a) => a,
        Err(e) => {
            tracing::error!("FATAL: invalid listen address '{}': {}", listen_addr, e);
            std::process::exit(1);
        }
    };
    // 零停机: systemd socket 激活 — LISTEN_FDS=1 时继承 fd3, 端口由 systemd 持有
    // 重启期间连接在 socket backlog 排队而非被拒(曾每次部署必 502)
    let listener = {
        let activated = std::env::var("LISTEN_FDS").ok().as_deref() == Some("1")
            && std::env::var("LISTEN_PID")
                .ok()
                .and_then(|p| p.parse::<u32>().ok())
                .map(|pid| pid == std::process::id())
                .unwrap_or(false);
        if activated {
            use std::os::unix::io::FromRawFd;
            tracing::info!(
                "Epicode Cloud socket-activated: inheriting fd 3 (systemd holds {})",
                addr
            );
            let std_l = unsafe { std::net::TcpListener::from_raw_fd(3) };
            std_l.set_nonblocking(true).ok();
            match tokio::net::TcpListener::from_std(std_l) {
                Ok(l) => l,
                Err(e) => {
                    tracing::error!("FATAL: fd3 -> tokio: {}", e);
                    std::process::exit(1);
                }
            }
        } else {
            match tokio::net::TcpListener::bind(addr).await {
                Ok(l) => l,
                Err(e) => {
                    tracing::error!("FATAL: bind {}: {}", addr, e);
                    std::process::exit(1);
                }
            }
        }
    };

    tracing::info!("Epicode Cloud listening on {}", addr);

    let tcp_port: Option<u16> = std::env::var("TETRAMEM_TCP_PORT")
        .ok()
        .and_then(|s| s.parse().ok());
    let tcp_bind = std::env::var("TETRAMEM_TCP_BIND").unwrap_or_else(|_| "127.0.0.1".into());

    let shutdown_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));

    if let Some(port) = tcp_port {
        let tcp_addr = format!("{}:{}", tcp_bind, port);
        let mgr = user_mgr.clone();
        let sf = shutdown_flag.clone();
        let rt_handle = tokio::runtime::Handle::current();
        let _tcp_thread = std::thread::spawn(move || {
            let _guard = rt_handle.enter();
            run_tcp_server(&tcp_addr, &mgr, &sf);
        });
    }

    {
        let mgr = user_mgr.clone();
        let sf = shutdown_flag.clone();
        tokio::spawn(async move {
            // glibc arena 滞留: 引擎驱逐后堆已释放但内存不还OS(RSS不降), 须主动trim (2026-09-23实测: 驱逐后3min RSS 5.3G纹丝不动)
            extern "C" {
                fn malloc_trim(pad: usize) -> std::os::raw::c_int;
            }
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(600)).await;
                if sf.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                // 压力感知驱逐 (2026-09-23): 可用内存吃紧时用10分钟短门槛提前回收半闲置引擎
                let avail_mb = std::fs::read_to_string("/proc/meminfo")
                    .ok()
                    .and_then(|s| {
                        s.lines()
                            .find(|l| l.starts_with("MemAvailable:"))
                            .and_then(|l| {
                                l.split_whitespace()
                                    .nth(1)
                                    .and_then(|v| v.parse::<u64>().ok())
                            })
                    })
                    .map(|kb| kb / 1024)
                    .unwrap_or(u64::MAX);
                if avail_mb < 2800 {
                    tracing::warn!(
                        "[evict-sweep] mem avail {}MB < 2800MB, pressure sweep (idle>600s)",
                        avail_mb
                    );
                    mgr.evict_idle_with(600, true);
                } else {
                    mgr.evict_idle();
                }
                unsafe {
                    malloc_trim(0);
                } // 每轮清扫后归还自由堆给OS
            }
        });
    }

    // 周期性淘汰 rate_limits / api_call_counts / api_calls_daily 中的过期 key（防止内存无限增长）
    {
        let rl = rate_limits.clone();
        let cc = api_call_counts.clone();
        let dc = api_calls_daily.clone();
        let mgr = user_mgr.clone();
        let sf2 = shutdown_flag.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(300)).await;
                if sf2.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                // rate_limits: 清除窗口过期且计数为0的 bucket
                {
                    let mut m = rl.lock();
                    let cutoff = std::time::Instant::now()
                        - std::time::Duration::from_secs(RATE_LIMIT_WINDOW_SECS * 2);
                    m.retain(|_, bucket| bucket.window_start > cutoff || bucket.count > 0);
                }
                // api_call_counts: 保留最近活跃的 client_id（>0 且总量 ≤5000）
                {
                    let mut m = cc.lock();
                    if m.len() > 5000 {
                        let mut sorted: Vec<_> = m.iter().map(|(k, v)| (k.clone(), *v)).collect();
                        sorted.sort_by_key(|a| std::cmp::Reverse(a.1));
                        sorted.truncate(5000);
                        m.clear();
                        for (k, v) in sorted {
                            m.insert(k, v);
                        }
                    }
                }
                // api_calls_daily: flush 到各用户 db + 只保留最近 90 天
                {
                    let mut m = dc.lock();
                    // flush: 对每个 api_key，把当日计数写入对应用户的 db
                    let entries: Vec<(String, Vec<(String, u64)>)> = m
                        .iter()
                        .map(|(api_key, daily)| {
                            (
                                api_key.clone(),
                                daily.iter().map(|(d, c)| (d.clone(), *c)).collect(),
                            )
                        })
                        .collect();
                    // 清空内存（已 flush，下次从 db 读）
                    m.clear();
                    drop(m);
                    // 异步 flush 到用户 db（spawn_blocking 避免 blocking I/O 在 async 里）
                    let mgr2 = mgr.clone();
                    tokio::task::spawn_blocking(move || {
                        for (api_key, daily) in &entries {
                            if let Some(info) = mgr2.authenticate(api_key) {
                                if let Ok(engine) = mgr2.get_engine(&info.user_id) {
                                    for (date, count) in daily {
                                        let _ = engine.storage.api_stats_add(date, *count as i64);
                                    }
                                }
                            }
                        }
                    });
                }
            }
        });
    }

    {
        let mgr = user_mgr.clone();
        let sf = shutdown_flag.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                if sf.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                mgr.maybe_auto_backup(21600);
                mgr.backup_meta();
            }
        });
    }

    if let Err(e) = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        #[cfg(unix)]
        {
            let mut sigterm =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("failed to install SIGTERM handler");
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    tracing::info!("Received SIGINT, shutting down...");
                }
                _ = sigterm.recv() => {
                    tracing::info!("Received SIGTERM, shutting down...");
                }
            }
        }
        #[cfg(not(unix))]
        {
            tokio::signal::ctrl_c().await.ok();
            tracing::info!("Shutting down...");
        }
    })
    .await
    {
        tracing::error!("Server error: {}", e);
    }

    tracing::info!("Saving all user engines...");
    shutdown_flag.store(true, std::sync::atomic::Ordering::Relaxed);
    for _ in 0..30 {
        if active_tasks_counter.load(std::sync::atomic::Ordering::Relaxed) == 0 {
            break;
        }
        tracing::info!(
            "Waiting for {} active tasks...",
            active_tasks_counter.load(std::sync::atomic::Ordering::Relaxed)
        );
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    user_mgr.final_save_all();
    tracing::info!("Epicode Cloud stopped.");
}
