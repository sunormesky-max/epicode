use std::fs;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use rusqlite::{Connection, params, OpenFlags};

use crate::domain::space::Space;
use crate::domain::tetra::{MemoryPayload, Tetrahedron, TetraId};
use crate::domain::vertex::Point3;
use crate::engine::knowledge::{KnowledgeGraph, RelationType, ConceptPrototype};
use crate::engine::vector::VectorLayer;

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA busy_timeout = 5000;
PRAGMA wal_autocheckpoint = 1000;
PRAGMA cache_size = -2048;
PRAGMA temp_store = MEMORY;

CREATE TABLE IF NOT EXISTS tetrahedrons (
    id          INTEGER PRIMARY KEY,
    core_x      REAL NOT NULL,
    core_y      REAL NOT NULL,
    core_z      REAL NOT NULL,
    content     TEXT NOT NULL,
    content_hash INTEGER NOT NULL,
    labels      TEXT NOT NULL,
    mass        REAL NOT NULL DEFAULT 1.0,
    timestamp   INTEGER NOT NULL DEFAULT 0,
    aliases     TEXT NOT NULL DEFAULT '[]',
    vertex_ids  TEXT NOT NULL DEFAULT '[0,0,0,0]',
    embedding   BLOB
);

CREATE TABLE IF NOT EXISTS relations (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    source      INTEGER NOT NULL,
    target      INTEGER NOT NULL,
    rel_type    TEXT NOT NULL,
    strength    REAL NOT NULL,
    UNIQUE(source, target, rel_type)
);

CREATE TABLE IF NOT EXISTS concepts (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    label       TEXT NOT NULL,
    member_count INTEGER NOT NULL DEFAULT 1,
    centroid    BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS meta (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tetra_timestamp ON tetrahedrons(timestamp);
CREATE INDEX IF NOT EXISTS idx_rel_source ON relations(source);
CREATE INDEX IF NOT EXISTS idx_rel_target ON relations(target);

CREATE TABLE IF NOT EXISTS health_snapshots (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp   INTEGER NOT NULL,
    total_memories INTEGER NOT NULL,
    clusters    INTEGER NOT NULL,
    feedback_records INTEGER NOT NULL DEFAULT 0,
    avg_importance REAL NOT NULL DEFAULT 0,
    enforced_count INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS archive_nodes (
    node_id     INTEGER PRIMARY KEY,
    parent_id   INTEGER DEFAULT NULL,
    node_type   TEXT NOT NULL DEFAULT 'doc',
    category    TEXT NOT NULL DEFAULT '',
    archived    INTEGER NOT NULL DEFAULT 0,
    created_ts  INTEGER NOT NULL,
    updated_ts  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_archive_parent ON archive_nodes(parent_id);
CREATE INDEX IF NOT EXISTS idx_archive_archived ON archive_nodes(archived);

CREATE TABLE IF NOT EXISTS drive_signals (
        id          INTEGER PRIMARY KEY,
        data        TEXT NOT NULL,
        updated_at  INTEGER NOT NULL DEFAULT 0
    );

    CREATE TABLE IF NOT EXISTS api_call_stats (
    date        TEXT NOT NULL,
    count       INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (date)
);
";

const MIGRATION_ADD_EMBEDDING: &str = "ALTER TABLE tetrahedrons ADD COLUMN embedding BLOB";
const MIGRATION_ADD_IMPORTANCE: &str = "ALTER TABLE tetrahedrons ADD COLUMN importance REAL NOT NULL DEFAULT 1.0";
const MIGRATION_ADD_ENFORCED: &str = "ALTER TABLE tetrahedrons ADD COLUMN enforced INTEGER NOT NULL DEFAULT 0";
const MIGRATION_ADD_RATIONALE: &str = "ALTER TABLE tetrahedrons ADD COLUMN rationale TEXT DEFAULT NULL";
const MIGRATION_ADD_ACCESS_COUNT: &str = "ALTER TABLE tetrahedrons ADD COLUMN access_count INTEGER NOT NULL DEFAULT 0";
const MIGRATION_ADD_MEMORY_TYPE: &str = "ALTER TABLE tetrahedrons ADD COLUMN memory_type TEXT DEFAULT NULL";
const MIGRATION_ADD_VALID_FROM: &str = "ALTER TABLE tetrahedrons ADD COLUMN valid_from INTEGER NOT NULL DEFAULT 0";
const MIGRATION_ADD_VALID_TO: &str = "ALTER TABLE tetrahedrons ADD COLUMN valid_to INTEGER DEFAULT NULL";
const MIGRATION_ADD_IDENTITY_STAMP: &str = "ALTER TABLE tetrahedrons ADD COLUMN identity_stamp TEXT DEFAULT NULL";
const MIGRATION_ADD_SOURCE_AGENT: &str = "ALTER TABLE tetrahedrons ADD COLUMN source_agent TEXT DEFAULT NULL";
const MIGRATION_ADD_LAST_REVIEWED: &str = "ALTER TABLE tetrahedrons ADD COLUMN last_reviewed_ts INTEGER DEFAULT NULL";
const MIGRATION_ADD_EXPIRED_AT: &str = "ALTER TABLE tetrahedrons ADD COLUMN expired_at INTEGER DEFAULT NULL";
const MIGRATION_ADD_INVALIDATED_AT: &str = "ALTER TABLE tetrahedrons ADD COLUMN invalidated_at INTEGER DEFAULT NULL";
const MIGRATION_ADD_MEMORY_CLASS: &str = "ALTER TABLE tetrahedrons ADD COLUMN memory_class TEXT DEFAULT NULL";
const MIGRATION_ADD_HEALTH_SNAPSHOTS: &str = "CREATE TABLE IF NOT EXISTS health_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp INTEGER NOT NULL,
    total_memories INTEGER NOT NULL,
    clusters INTEGER NOT NULL,
    feedback_records INTEGER NOT NULL DEFAULT 0,
    avg_importance REAL NOT NULL DEFAULT 0,
    enforced_count INTEGER NOT NULL DEFAULT 0
)";
const MIGRATION_ADD_ARCHIVE_NODES: &str = "CREATE TABLE IF NOT EXISTS archive_nodes (
    node_id INTEGER PRIMARY KEY,
    parent_id INTEGER DEFAULT NULL,
    node_type TEXT NOT NULL DEFAULT 'doc',
    category TEXT NOT NULL DEFAULT '',
    archived INTEGER NOT NULL DEFAULT 0,
    created_ts INTEGER NOT NULL,
    updated_ts INTEGER NOT NULL
)";
const MIGRATION_ADD_ARCHIVE_INDEXES: &str = "CREATE INDEX IF NOT EXISTS idx_archive_parent ON archive_nodes(parent_id); CREATE INDEX IF NOT EXISTS idx_archive_archived ON archive_nodes(archived)";
const MIGRATION_ADD_API_CALL_STATS: &str = "CREATE TABLE IF NOT EXISTS api_call_stats (date TEXT NOT NULL, count INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (date))";

/// 四面体 UPSERT SQL(24 列)。upsert_tetra / batch_upsert / save_tetrahedrons_tx 三处共用,
/// 避免列数不一致导致的静默数据丢失(曾发生 batch_upsert 列数 bug,见 SESSION_LOG §3.1)。
const UPSERT_TETRA_SQL: &str = "INSERT OR REPLACE INTO tetrahedrons \
    (id, core_x, core_y, core_z, content, content_hash, labels, mass, timestamp, \
     aliases, vertex_ids, embedding, importance, enforced, rationale, access_count, \
     memory_type, valid_from, valid_to, identity_stamp, source_agent, last_reviewed_ts, \
     expired_at, invalidated_at, memory_class) \
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, \
            ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)";

/// 档案库节点行（从 archive_nodes 表读取）
#[derive(Debug, Clone)]
pub struct ArchiveNodeRow {
    pub node_id: i64,
    pub parent_id: Option<i64>,
    pub node_type: String,
    pub category: String,
    pub created_ts: i64,
    pub updated_ts: i64,
}

pub struct StorageManager {
    conn: Mutex<Connection>,
    data_dir: PathBuf,
    backup_dir: PathBuf,
    crypto: Option<super::crypto::CryptoEngine>,
    crypto_user: String,
}

/// ═══ 时间效性常量(集中可见, 2026-08-30 大卫"活系统反硬编码"审计产物) ═══
/// 心跳间隙: 两次工具调用间隔≤此值计为连续工作(活跃钟)。定义"活跃"这一核心货币。
/// 默认300s; 环境变量 EPICODE_HEARTBEAT_GAP_SECS 可覆盖(慢推理智能体应调大)。
/// 自适应路线: 按各agent自身调用间隙分布P90自动定标(未实施)。
pub fn heartbeat_gap_secs() -> i64 {
    std::env::var("EPICODE_HEARTBEAT_GAP_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(300)
}

impl StorageManager {
    pub fn new(data_dir: &Path) -> Result<Self, String> {
        let _ = fs::create_dir_all(data_dir);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(data_dir, fs::Permissions::from_mode(0o700));
        }
        let backup_dir = data_dir.join("backups");
        let _ = fs::create_dir_all(&backup_dir);

        let db_path = data_dir.join("tetramem.db");
        let conn = Connection::open_with_flags(
            &db_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        ).map_err(|e| format!("failed to open SQLite database at {}: {}", db_path.display(), e))?;

        conn.execute_batch(SCHEMA)
            .map_err(|e| format!("failed to initialize schema: {}", e))?;
        if let Err(e) = conn.execute_batch(MIGRATION_ADD_EMBEDDING) {
            tracing::debug!("[Storage] embedding migration skipped (likely already applied): {}", e);
        }
        if let Err(e) = conn.execute_batch(MIGRATION_ADD_IMPORTANCE) {
            tracing::debug!("[Storage] importance migration skipped (likely already applied): {}", e);
        }
        if let Err(e) = conn.execute_batch(MIGRATION_ADD_ENFORCED) {
            tracing::debug!("[Storage] enforced migration skipped (likely already applied): {}", e);
        }
        if let Err(e) = conn.execute_batch(MIGRATION_ADD_RATIONALE) {
            tracing::debug!("[Storage] rationale migration skipped: {}", e);
        }
        if let Err(e) = conn.execute_batch(MIGRATION_ADD_ACCESS_COUNT) {
            tracing::debug!("[Storage] access_count migration skipped: {}", e);
        }
        if let Err(e) = conn.execute_batch(MIGRATION_ADD_MEMORY_TYPE) {
            tracing::debug!("[Storage] memory_type migration skipped (likely already applied): {}", e);
        }
        if let Err(e) = conn.execute_batch(MIGRATION_ADD_VALID_FROM) {
            tracing::debug!("[Storage] valid_from migration skipped (likely already applied): {}", e);
        }
        if let Err(e) = conn.execute_batch(MIGRATION_ADD_VALID_TO) {
            tracing::debug!("[Storage] valid_to migration skipped (likely already applied): {}", e);
        }
        if let Err(e) = conn.execute_batch(MIGRATION_ADD_IDENTITY_STAMP) {
            tracing::info!("[Storage] identity_stamp migration: {}", e);
        }
        if let Err(e) = conn.execute_batch(MIGRATION_ADD_SOURCE_AGENT) {
            tracing::info!("[Storage] source_agent migration: {}", e);
        }
        if let Err(e) = conn.execute_batch(MIGRATION_ADD_LAST_REVIEWED) {
            tracing::debug!("last_reviewed_ts column may already exist: {}", e);
        }
        if let Err(e) = conn.execute_batch(MIGRATION_ADD_EXPIRED_AT) {
            tracing::debug!("[Storage] expired_at migration skipped: {}", e);
        }
        if let Err(e) = conn.execute_batch(MIGRATION_ADD_INVALIDATED_AT) {
            tracing::debug!("[Storage] invalidated_at migration skipped: {}", e);
        }
        if let Err(e) = conn.execute_batch(MIGRATION_ADD_MEMORY_CLASS) {
            tracing::debug!("[Storage] memory_class migration skipped: {}", e);
        }
        if let Err(e) = conn.execute_batch(MIGRATION_ADD_HEALTH_SNAPSHOTS) {
            tracing::debug!("[Storage] health_snapshots migration skipped: {}", e);
        }
        if let Err(e) = conn.execute_batch(MIGRATION_ADD_ARCHIVE_NODES) {
            tracing::debug!("[Storage] archive_nodes migration skipped: {}", e);
        }
        if let Err(e) = conn.execute_batch(MIGRATION_ADD_ARCHIVE_INDEXES) {
            tracing::debug!("[Storage] archive indexes skipped: {}", e);
        }
        if let Err(e) = conn.execute_batch(MIGRATION_ADD_API_CALL_STATS) {
            tracing::debug!("[Storage] api_call_stats migration skipped: {}", e);
        }

        tracing::info!("SQLite database opened: {}", db_path.display());

        Ok(Self {
            conn: Mutex::new(conn),
            data_dir: data_dir.to_path_buf(),
            backup_dir,
            crypto: None,
            crypto_user: String::new(),
        })
    }

    pub fn with_encryption(mut self, crypto: super::crypto::CryptoEngine, user_id: &str) -> Self {
        self.crypto = Some(crypto);
        self.crypto_user = user_id.to_string();
        self
    }

    fn encrypt_field(&self, content: &str) -> Result<String, String> {
        if let Some(ref crypto) = self.crypto {
            crypto.encrypt_content(content, &self.crypto_user)
                .map_err(|e| {
                    tracing::error!("[Storage] FATAL: encrypt failed for user {}: {}. Data NOT stored.", self.crypto_user, e);
                    format!("encrypt failed: {}", e)
                })
        } else {
            Ok(content.to_string())
        }
    }

    fn decrypt_field(&self, content: &str) -> String {
        if let Some(ref crypto) = self.crypto {
            match crypto.decrypt_content(content, &self.crypto_user) {
                Ok(dec) => dec,
                Err(e) => {
                    tracing::error!("[Storage] decrypt failed for user {}: {}. Content marked as corrupted (id may be affected). raw_len={}", self.crypto_user, e, content.len());
                    "[corrupted:decryption-failed]".to_string()  // 占位而非空串（kimi #7），避免静默清空
                }
            }
        } else {
            content.to_string()
        }
    }

    pub fn load_all(&self, space: &Space, kg: &KnowledgeGraph) -> LoadReport {
        kg.set_loading(true);
        let mut report = LoadReport::default();

        match self.load_tetrahedrons(space) {
            Ok(n) => {
                report.tetras_loaded = n;
                report.space_ok = true;
            }
            Err(e) => {
                report.space_error = Some(e);
            }
        }

        match self.load_relations(kg) {
            Ok(n) => {
                report.relations_loaded = n;
                report.kg_ok = true;
            }
            Err(e) => {
                report.kg_error = Some(e);
            }
        }

        match self.load_concepts(kg) {
            Ok(n) => {
                report.concepts_loaded = n;
            }
            Err(e) => {
                tracing::warn!("load concepts: {}", e);
            }
        }

        kg.set_loading(false);
        space.restore_counters();
        let ports_restored = space.rebuild_port_occupancy();
        if ports_restored > 0 {
            report.port_occupancy_restored = ports_restored;
        }
        report
    }

    pub fn save_all(&self, space: &Space, kg: &KnowledgeGraph) -> Result<(), String> {
        let conn = self.conn.lock();
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;

        self.save_tetrahedrons_tx(&tx, space)?;
        self.save_relations_tx(&tx, kg)?;
        self.save_concepts_tx(&tx, kg)?;

        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn save_space_only(&self, space: &Space) -> Result<(), String> {
        let conn = self.conn.lock();
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        self.save_tetrahedrons_tx(&tx, space)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    /// F4增量: 只写变化的关系(UNIQUE(source,target,rel_type) upsert + 精确delete) + concepts全量(121行, 便宜)
    pub fn save_relations_delta(
        &self,
        upserts: &[super::knowledge::Relation],
        deletes: &[(TetraId, TetraId, super::knowledge::RelationType)],
    ) -> Result<(usize, usize), String> {
        let conn = self.conn.lock();
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO relations (source, target, rel_type, strength) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(source, target, rel_type) DO UPDATE SET strength = excluded.strength"
            ).map_err(|e| e.to_string())?;
            for r in upserts {
                stmt.execute(params![r.source as i64, r.target as i64, Self::rel_type_str(&r.relation_type), r.strength])
                    .map_err(|e| e.to_string())?;
            }
        }
        {
            let mut stmt = tx.prepare(
                "DELETE FROM relations WHERE source = ?1 AND target = ?2 AND rel_type = ?3"
            ).map_err(|e| e.to_string())?;
            for (s, t, rt) in deletes {
                stmt.execute(params![*s as i64, *t as i64, Self::rel_type_str(rt)])
                    .map_err(|e| e.to_string())?;
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok((upserts.len(), deletes.len()))
    }

    pub fn save_kg_only(&self, kg: &KnowledgeGraph) -> Result<(), String> {
        let conn = self.conn.lock();
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        self.save_relations_tx(&tx, kg)?;
        self.save_concepts_tx(&tx, kg)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }


    /// L0: Save drive signals to SQLite for persistence across restarts.
    pub fn save_drive_signals(&self, signals: &[super::drive::DriveSignal]) -> Result<(), String> {
        let conn = self.conn.lock();
        // Phase 3 收尾: 事务化保存，防崩溃丢队列（Tester-Q P1 要求）
        conn.execute("BEGIN IMMEDIATE TRANSACTION", []).map_err(|e| e.to_string())?;
        
        conn.execute("DELETE FROM drive_signals", []).map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            e.to_string()
        })?;
        let now = chrono::Utc::now().timestamp();
        for signal in signals {
            let data = serde_json::to_string(signal).map_err(|e| {
                let _ = conn.execute("ROLLBACK", []);
                e.to_string()
            })?;
            conn.execute(
                "INSERT OR REPLACE INTO drive_signals (id, data, updated_at) VALUES (?, ?, ?)",
                params![signal.id as i64, data, now],
            ).map_err(|e| {
                let _ = conn.execute("ROLLBACK", []);
                e.to_string()
            })?;
        }
        conn.execute("COMMIT", []).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 稳态持久化: 驱力引擎状态 (weights/history) kv 存取
    pub fn save_drive_engine_state(&self, data: &str) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute("CREATE TABLE IF NOT EXISTS drive_kv (k TEXT PRIMARY KEY, v TEXT)", []).map_err(|e| e.to_string())?;
        conn.execute("INSERT OR REPLACE INTO drive_kv (k, v) VALUES ('engine_state', ?)", params![data]).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn load_drive_engine_state(&self) -> Option<String> {
        let conn = self.conn.lock();
        if conn.execute("CREATE TABLE IF NOT EXISTS drive_kv (k TEXT PRIMARY KEY, v TEXT)", []).is_err() { return None; }
        conn.query_row("SELECT v FROM drive_kv WHERE k='engine_state'", [], |r| r.get(0)).ok()
    }

    /// D7.2 参数记忆: 知识卡片 — 域级压缩知识(潜意识从簇中蒸馏的"内化知识")
    pub fn save_knowledge_card(&self, domain: &str, summary: &str, cluster_ids: &[u64]) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute("CREATE TABLE IF NOT EXISTS knowledge_cards (
            domain TEXT PRIMARY KEY,
            summary TEXT NOT NULL,
            cluster_ids TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        )", []).map_err(|e| e.to_string())?;
        conn.execute("INSERT OR REPLACE INTO knowledge_cards (domain, summary, cluster_ids, updated_at) VALUES (?1, ?2, ?3, ?4)",
            params![domain, summary, serde_json::to_string(cluster_ids).unwrap_or_default(),
                    chrono::Utc::now().timestamp()]).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn load_knowledge_cards(&self) -> Vec<(String, String, Vec<u64>)> {
        let conn = self.conn.lock();
        let _ = conn.execute("CREATE TABLE IF NOT EXISTS knowledge_cards (domain TEXT PRIMARY KEY, summary TEXT NOT NULL, cluster_ids TEXT NOT NULL, updated_at INTEGER NOT NULL)", []);
        let mut stmt = match conn.prepare("SELECT domain, summary, cluster_ids FROM knowledge_cards ORDER BY updated_at DESC") {
            Ok(s) => s, Err(_) => return vec![],
        };
        stmt.query_map([], |row| {
            let d: String = row.get(0)?;
            let raw_s: String = row.get(1)?;
            let s: String = match raw_s.find("</think>") {
                Some(pos) => raw_s[pos + 8..].trim().to_string(),
                None => raw_s,
            };
            let ids: String = row.get(2).unwrap_or_default();
            let cluster_ids: Vec<u64> = serde_json::from_str(&ids).unwrap_or_default();
            Ok((d, s, cluster_ids))
        }).map(|rows| rows.filter_map(|r| r.ok()).collect()).unwrap_or_default()
    }

    /// 时间效性: 任务会话CRUD
    pub fn create_task_session(&self, task_id: &str, agent_id: &str, user_id: &str, description: &str, budget_ms: i64) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute("CREATE TABLE IF NOT EXISTS task_sessions (
            task_id TEXT PRIMARY KEY, agent_id TEXT NOT NULL, user_id TEXT NOT NULL,
            description TEXT NOT NULL, start_ts INTEGER NOT NULL, budget_ms INTEGER NOT NULL,
            actual_ms INTEGER, deviation REAL, on_time BOOLEAN, quality_score REAL,
            outcome_summary TEXT, created_at INTEGER DEFAULT (strftime('%s','now'))
        )", []).map_err(|e| e.to_string())?;
        conn.execute("INSERT OR REPLACE INTO task_sessions (task_id, agent_id, user_id, description, start_ts, budget_ms) VALUES (?1,?2,?3,?4,?5,?6)",
            params![task_id, agent_id, user_id, description, chrono::Utc::now().timestamp(), budget_ms]).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn update_task_session_complete(&self, task_id: &str, actual_ms: i64, deviation: f64, on_time: bool, quality: f64, summary: &str) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute("UPDATE task_sessions SET actual_ms=?1, deviation=?2, on_time=?3, quality_score=?4, outcome_summary=?5 WHERE task_id=?6",
            params![actual_ms, deviation, on_time, quality, summary, task_id]).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_task_session(&self, task_id: &str) -> Option<serde_json::Value> {
        let conn = self.conn.lock();
        let _ = conn.execute("CREATE TABLE IF NOT EXISTS task_sessions (task_id TEXT PRIMARY KEY, agent_id TEXT NOT NULL, user_id TEXT NOT NULL, description TEXT NOT NULL, start_ts INTEGER NOT NULL, budget_ms INTEGER NOT NULL, actual_ms INTEGER, deviation REAL, on_time BOOLEAN, quality_score REAL, outcome_summary TEXT, created_at INTEGER DEFAULT (strftime('%s','now')))", []);
        conn.query_row("SELECT * FROM task_sessions WHERE task_id=?1", params![task_id], |row| {
            Ok(serde_json::json!({
                "task_id": row.get::<_, String>(0)?, "agent_id": row.get::<_, String>(1)?,
                "user_id": row.get::<_, String>(2)?, "description": row.get::<_, String>(3)?,
                "start_ts": row.get::<_, i64>(4)?, "budget_ms": row.get::<_, i64>(5)?,
                "actual_ms": row.get::<_, Option<i64>>(6)?, "deviation": row.get::<_, Option<f64>>(7)?,
                "on_time": row.get::<_, Option<bool>>(8)?, "quality_score": row.get::<_, Option<f64>>(9)?,
                "outcome_summary": row.get::<_, Option<String>>(10)?,
            }))
        }).ok()
    }

    pub fn list_similar_tasks(&self, description_kw: &str, limit: usize) -> Vec<serde_json::Value> {
        let conn = self.conn.lock();
        let _ = conn.execute("CREATE TABLE IF NOT EXISTS task_sessions (task_id TEXT PRIMARY KEY, agent_id TEXT NOT NULL, user_id TEXT NOT NULL, description TEXT NOT NULL, start_ts INTEGER NOT NULL, budget_ms INTEGER NOT NULL, actual_ms INTEGER, deviation REAL, on_time BOOLEAN, quality_score REAL, outcome_summary TEXT, created_at INTEGER DEFAULT (strftime('%s','now')))", []);
        let mut stmt = match conn.prepare("SELECT task_id, description, budget_ms, actual_ms, on_time, quality_score FROM task_sessions WHERE actual_ms IS NOT NULL AND description LIKE ?1 ORDER BY start_ts DESC LIMIT ?2") {
            Ok(s) => s, Err(_) => return vec![],
        };
        stmt.query_map(params![format!("%{}%", description_kw), limit as i64], |row| {
            Ok(serde_json::json!({
                "task_id": row.get::<_, String>(0)?, "description": row.get::<_, String>(1)?,
                "budget_ms": row.get::<_, i64>(2)?, "actual_ms": row.get::<_, i64>(3)?,
                "on_time": row.get::<_, bool>(4)?, "quality": row.get::<_, Option<f64>>(5)?,
            }))
        }).map(|r| r.filter_map(|x| x.ok()).collect()).unwrap_or_default()
    }


    // ═══ P0 时间效性: 相位机证据计数器 ═══

    fn ensure_task_cols(conn: &rusqlite::Connection) {
        let _ = conn.execute("CREATE TABLE IF NOT EXISTS task_sessions (task_id TEXT PRIMARY KEY, agent_id TEXT NOT NULL, user_id TEXT NOT NULL, description TEXT NOT NULL, start_ts INTEGER NOT NULL, budget_ms INTEGER NOT NULL, actual_ms INTEGER, deviation REAL, on_time BOOLEAN, quality_score REAL, outcome_summary TEXT, created_at INTEGER DEFAULT (strftime('%s','now')))", []);
        for col in ["memory_ops INTEGER DEFAULT 0", "alternatives INTEGER DEFAULT 0",
                    "revisions INTEGER DEFAULT 0", "checks INTEGER DEFAULT 0",
                    "utilization_pct REAL", "low_utilization BOOLEAN DEFAULT 0", "saturation_note TEXT",
                    "parent_task_id TEXT", "checkpoints TEXT",
                    "alerts TEXT", "judge_score REAL", "judge_note TEXT",
                    "first_memory_op_ts INTEGER", "active_ms INTEGER", "last_activity_ts INTEGER",
                    "open_questions TEXT", "iteration_log TEXT", "reflection_pushed INTEGER",
                    "clock_offset_ms INTEGER",
                    "flow_last_active_ms INTEGER", "flow_last_ts INTEGER",
                    "est_ms INTEGER", "task_class TEXT", "real_active_ms INTEGER",
                    "over_budget_pct REAL", "wait_attempts INTEGER DEFAULT 0", "wait_evidence_sig TEXT", "first_wait_ts INTEGER", "stop_reason TEXT", "goal_json TEXT"] {
            let _ = conn.execute(&format!("ALTER TABLE task_sessions ADD COLUMN {}", col), []);
        }
    }

    pub fn bump_task_counter(&self, task_id: &str, field: &str) {
        if !matches!(field, "memory_ops" | "alternatives" | "revisions" | "checks") { return; }
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        if field == "memory_ops" {
            // P3 证据时间轴: 首次记忆检索时刻只记一次
            let _ = conn.execute("UPDATE task_sessions SET memory_ops = COALESCE(memory_ops,0) + 1, first_memory_op_ts = COALESCE(first_memory_op_ts, strftime('%s','now')) WHERE task_id = ?1 AND actual_ms IS NULL", params![task_id]);
            return;
        }
        let _ = conn.execute(&format!("UPDATE task_sessions SET {} = COALESCE({},0) + 1 WHERE task_id = ?1 AND actual_ms IS NULL", field, field), params![task_id]);
    }

    /// P6 双钟: 工具调用心跳 — 间隙<=5min计为连续工作; 超过(停放/睡眠/长阻塞)不计, 预算不烧
    pub fn touch_task_activity(&self, task_id: &str) {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        let sql = format!("UPDATE task_sessions SET active_ms = COALESCE(active_ms,0) + (CASE WHEN (strftime('%s','now') - COALESCE(last_activity_ts, strftime('%s','now'))) <= {} THEN (strftime('%s','now') - COALESCE(last_activity_ts, strftime('%s','now'))) ELSE 0 END) * 1000, last_activity_ts = strftime('%s','now') WHERE task_id = ?1 AND actual_ms IS NULL", heartbeat_gap_secs());
        let _ = conn.execute(&sql, params![task_id]);
    }

    pub fn get_task_active_ms(&self, task_id: &str) -> i64 {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        conn.query_row("SELECT COALESCE(active_ms,0) FROM task_sessions WHERE task_id=?1",
            params![task_id], |r| r.get::<_, i64>(0)).unwrap_or(0)
    }

    pub fn get_task_history_stats(&self, user_id: &str) -> (i64, i64, f64) {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        conn.query_row("SELECT COUNT(*), COALESCE(SUM(CASE WHEN low_utilization THEN 1 ELSE 0 END),0), COALESCE(AVG(utilization_pct),0) FROM task_sessions WHERE user_id=?1 AND actual_ms IS NOT NULL",
            params![user_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap_or((0, 0, 0.0))
    }


    pub fn get_first_memory_op_ts(&self, task_id: &str) -> Option<i64> {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        conn.query_row("SELECT first_memory_op_ts FROM task_sessions WHERE task_id=?1",
            params![task_id], |r| r.get::<_, Option<i64>>(0)).ok().flatten()
    }

    pub fn set_task_alternatives(&self, task_id: &str, n: i64) {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        let _ = conn.execute("UPDATE task_sessions SET alternatives = MAX(COALESCE(alternatives,0), ?1) WHERE task_id = ?2 AND actual_ms IS NULL", params![n, task_id]);
    }

    pub fn get_task_counters(&self, task_id: &str) -> (i64, i64, i64, i64) {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        conn.query_row("SELECT COALESCE(memory_ops,0), COALESCE(alternatives,0), COALESCE(revisions,0), COALESCE(checks,0) FROM task_sessions WHERE task_id=?1",
            params![task_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap_or((0, 0, 0, 0))
    }

    pub fn get_active_task_for_user(&self, user_id: &str) -> Option<String> {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        conn.query_row("SELECT task_id FROM task_sessions WHERE user_id = ?1 AND actual_ms IS NULL ORDER BY start_ts DESC LIMIT 1",
            params![user_id], |r| r.get::<_, String>(0)).ok()
    }

    pub fn complete_task_v2(&self, task_id: &str, actual_ms: i64, deviation: f64, on_time: bool, quality: f64, summary: &str, utilization_pct: f64, low_utilization: bool, saturation_note: &str) -> Result<(), String> {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        conn.execute("UPDATE task_sessions SET actual_ms=?1, deviation=?2, on_time=?3, quality_score=?4, outcome_summary=?5, utilization_pct=?6, low_utilization=?7, saturation_note=?8 WHERE task_id=?9",
            params![actual_ms, deviation, on_time, quality, summary, utilization_pct, low_utilization, saturation_note, task_id]).map(|_| ()).map_err(|e| e.to_string())
    }

    // ═══ P1 时间效性: 时间树 + 检查点 ═══

    pub fn attach_task_parent(&self, task_id: &str, parent_task_id: &str) {
        if task_id == parent_task_id { return; }
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        let _ = conn.execute("UPDATE task_sessions SET parent_task_id=?1 WHERE task_id=?2 AND actual_ms IS NULL", params![parent_task_id, task_id]);
    }

    pub fn append_checkpoint(&self, task_id: &str, note: &str) {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        let existing: String = conn.query_row("SELECT COALESCE(checkpoints,'[]') FROM task_sessions WHERE task_id=?1",
            params![task_id], |r| r.get(0)).unwrap_or_else(|_| "[]".to_string());
        let mut arr: Vec<serde_json::Value> = serde_json::from_str(&existing).unwrap_or_default();
        arr.push(serde_json::json!({"ts": chrono::Utc::now().timestamp(), "note": note}));
        let _ = conn.execute("UPDATE task_sessions SET checkpoints=?1 WHERE task_id=?2",
            params![serde_json::to_string(&arr).unwrap_or_default(), task_id]);
    }

    pub fn get_task_checkpoints(&self, task_id: &str) -> serde_json::Value {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        conn.query_row("SELECT COALESCE(checkpoints,'[]') FROM task_sessions WHERE task_id=?1",
            params![task_id], |r| r.get::<_, String>(0))
            .ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(serde_json::json!([]))
    }

    pub fn list_child_tasks(&self, parent_task_id: &str) -> Vec<serde_json::Value> {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        let mut stmt = match conn.prepare("SELECT task_id, description, budget_ms, actual_ms FROM task_sessions WHERE parent_task_id=?1 ORDER BY start_ts ASC") {
            Ok(s) => s, Err(_) => return vec![],
        };
        stmt.query_map(params![parent_task_id], |row| Ok(serde_json::json!({
            "task_id": row.get::<_, String>(0)?, "description": row.get::<_, String>(1)?,
            "budget_ms": row.get::<_, i64>(2)?, "actual_ms": row.get::<_, Option<i64>>(3)?,
        }))).map(|r| r.filter_map(|x| x.ok()).collect()).unwrap_or_default()
    }

    pub fn append_task_alert(&self, task_id: &str, urgency: &str, message: &str, blocking: bool) {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        let existing: String = conn.query_row("SELECT COALESCE(alerts,'[]') FROM task_sessions WHERE task_id=?1",
            params![task_id], |r| r.get(0)).unwrap_or_else(|_| "[]".to_string());
        let mut arr: Vec<serde_json::Value> = serde_json::from_str(&existing).unwrap_or_default();
        arr.push(serde_json::json!({"ts": chrono::Utc::now().timestamp(), "urgency": urgency, "message": message, "blocking": blocking}));
        let _ = conn.execute("UPDATE task_sessions SET alerts=?1 WHERE task_id=?2",
            params![serde_json::to_string(&arr).unwrap_or_default(), task_id]);
    }

    pub fn get_task_alerts(&self, task_id: &str) -> serde_json::Value {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        conn.query_row("SELECT COALESCE(alerts,'[]') FROM task_sessions WHERE task_id=?1",
            params![task_id], |r| r.get::<_, String>(0))
            .ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(serde_json::json!([]))
    }

    pub fn set_task_judge(&self, task_id: &str, score: f64, note: &str) {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        let _ = conn.execute("UPDATE task_sessions SET judge_score=?1, judge_note=?2 WHERE task_id=?3",
            params![score, note, task_id]);
    }

    pub fn set_task_open_questions(&self, task_id: &str, qs: &[String]) {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        let _ = conn.execute("UPDATE task_sessions SET open_questions=?1 WHERE task_id=?2",
            params![serde_json::to_string(qs).unwrap_or_default(), task_id]);
    }

    pub fn get_task_open_questions(&self, task_id: &str) -> serde_json::Value {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        conn.query_row("SELECT COALESCE(open_questions,'[]') FROM task_sessions WHERE task_id=?1",
            params![task_id], |r| r.get::<_, String>(0))
            .ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(serde_json::json!([]))
    }

    pub fn set_task_iteration_log(&self, task_id: &str, iters: &[serde_json::Value]) {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        let _ = conn.execute("UPDATE task_sessions SET iteration_log=?1 WHERE task_id=?2",
            params![serde_json::to_string(iters).unwrap_or_default(), task_id]);
    }

    pub fn get_last_judgment(&self, user_id: &str) -> Option<(f64, String)> {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        conn.query_row("SELECT quality_score, COALESCE(judge_note,'') FROM task_sessions WHERE user_id=?1 AND actual_ms IS NOT NULL AND COALESCE(judge_note,'') != '' ORDER BY start_ts DESC LIMIT 1",
            params![user_id], |r| Ok((r.get(0)?, r.get(1)?))).ok()
    }

    /// P25 时间感: 自锚定预估 — 按task_class聚合该智能体自己的 est/act 偏差史
    /// P34d: task_class独立访问器(get_task_session按位置索引, 新列必须独立读取)
    pub fn get_task_class(&self, task_id: &str) -> String {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        conn.query_row("SELECT COALESCE(task_class,'') FROM task_sessions WHERE task_id=?1",
            params![task_id], |r| r.get::<_, String>(0)).unwrap_or_default()
    }

    pub fn get_task_est(&self, task_id: &str) -> Option<i64> {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        conn.query_row("SELECT est_ms FROM task_sessions WHERE task_id=?1", params![task_id],
            |r| r.get::<_, Option<i64>>(0)).ok().flatten()
    }

    // P34 停止谈判: WAIT计数与证据签名(识别零新证据的重复停止尝试)
    pub fn bump_task_wait(&self, task_id: &str, evidence_sig: &str) {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        let _ = conn.execute("UPDATE task_sessions SET wait_attempts = COALESCE(wait_attempts,0) + 1, wait_evidence_sig = ?1, first_wait_ts = COALESCE(first_wait_ts, strftime('%s','now')) WHERE task_id = ?2 AND actual_ms IS NULL", params![evidence_sig, task_id]);
    }

    pub fn get_task_wait_attempts(&self, task_id: &str) -> (i64, Option<String>) {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        conn.query_row("SELECT COALESCE(wait_attempts,0), wait_evidence_sig FROM task_sessions WHERE task_id=?1",
            params![task_id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap_or((0, None))
    }

    pub fn get_first_wait_ts(&self, task_id: &str) -> Option<i64> {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        conn.query_row("SELECT first_wait_ts FROM task_sessions WHERE task_id=?1",
            params![task_id], |r| r.get::<_, Option<i64>>(0)).ok().flatten()
    }

    /// P34d 停留画像: 同类任务几轮改进才饱和(PonderNet式自校准停止)
    pub fn get_dwell_profile(&self, task_class: &str) -> serde_json::Value {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        let rows: Vec<(i64, f64, Option<String>)> = conn.prepare(
            "SELECT COALESCE(wait_attempts,0), COALESCE(utilization_pct,0), stop_reason FROM task_sessions WHERE task_class=?1 AND actual_ms IS NOT NULL AND quality_score >= 3.0 ORDER BY start_ts DESC LIMIT 20")
            .map(|mut s| s.query_map(params![task_class], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .map(|it| it.filter_map(|x| x.ok()).collect()).unwrap_or_default())
            .unwrap_or_default();
        drop(conn);
        let n = rows.len();
        if n == 0 { return serde_json::json!({"samples": 0}); }
        let mut wa: Vec<i64> = rows.iter().map(|(w, _, _)| *w).collect();
        wa.sort();
        let med_iter = wa[n / 2];
        let earned = rows.iter().filter(|(_, _, s)| s.as_deref() == Some("earned_saturation")).count();
        let mut earned_rows: Vec<f64> = rows.iter().filter(|(_, _, s)| s.as_deref() == Some("earned_saturation")).map(|(_, u, _)| *u).collect();
        earned_rows.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let med_earned: serde_json::Value = if earned_rows.is_empty() { serde_json::Value::Null }
            else { serde_json::json!(earned_rows[earned_rows.len() / 2]) };
        serde_json::json!({
            "samples": n,
            "median_wait_iterations": med_iter,
            "median_util_at_earned_pct": med_earned,
            "earned_share_pct": (earned as f64 / n as f64 * 100.0).round() as i64,
            "note": "你的停止行为画像 — 中位N轮改进后饱和; earned_share=挣取停止占比(健康度指标, 随成熟度应上升)",
        })
    }

    /// P36b: 该用户的Pulse真值任务数(从未装过的agent定向提醒用)
    pub fn count_pulse_tasks(&self, user_id: &str) -> i64 {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        conn.query_row("SELECT count(*) FROM task_sessions WHERE user_id=?1 AND real_active_ms IS NOT NULL",
            params![user_id], |r| r.get::<_, i64>(0)).unwrap_or(0)
    }

    pub fn set_task_stop_reason(&self, task_id: &str, reason: &str) {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        let _ = conn.execute("UPDATE task_sessions SET stop_reason=?1 WHERE task_id=?2", params![reason, task_id]);
    }

    /// P35 目标契约: 结构化goal(objective/scope/constraints/done_when/stop_if)持久化
    pub fn set_task_goal_json(&self, task_id: &str, goal_json: &str) {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        let _ = conn.execute("UPDATE task_sessions SET goal_json=?1 WHERE task_id=?2", params![goal_json, task_id]);
    }

    /// P35: 独立访问器(get_task_session按位置索引, 新列必须独立读取 — P34d同款陷阱)
    pub fn get_task_goal_json(&self, task_id: &str) -> Option<String> {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        conn.query_row("SELECT goal_json FROM task_sessions WHERE task_id=?1",
            params![task_id], |r| r.get::<_, Option<String>>(0)).ok().flatten()
    }

    /// P27 Pulse: 写入插件上报的真实活跃时间(带归属校验的调用方负责)
    pub fn set_real_active(&self, task_id: &str, user_id: &str, real_ms: i64) -> Result<(), String> {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        let owner: String = conn.query_row("SELECT user_id FROM task_sessions WHERE task_id=?1",
            params![task_id], |r| r.get(0)).map_err(|_| "task not found".to_string())?;
        if owner != user_id {
            return Err("task does not belong to this user".into());
        }
        let updated = conn.execute("UPDATE task_sessions SET real_active_ms=?1 WHERE task_id=?2 AND actual_ms IS NULL",
            params![real_ms, task_id]).map_err(|e| e.to_string())?;
        if updated == 0 {
            return Err("task not found or already completed".into());
        }
        Ok(())
    }

    /// P27: 优先返回真实活跃时间, 无则退回心跳推断(向下兼容)
    pub fn get_effective_active_ms(&self, task_id: &str) -> i64 {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        conn.query_row(
            "SELECT COALESCE(real_active_ms, COALESCE(active_ms,0)) FROM task_sessions WHERE task_id=?1",
            params![task_id], |r| r.get::<_, i64>(0)).unwrap_or(0)
    }

    pub fn set_task_est_class(&self, task_id: &str, est_ms: i64, task_class: &str) {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        let _ = conn.execute("UPDATE task_sessions SET est_ms=?1, task_class=?2 WHERE task_id=?3",
            params![if est_ms > 0 { Some(est_ms) } else { None }, if task_class.is_empty() { None } else { Some(task_class) }, task_id]);
    }

    pub fn get_self_calibration(&self, task_class: &str) -> serde_json::Value {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        let rows: Vec<(i64, i64)> = conn.prepare(
            "SELECT est_ms, actual_ms FROM task_sessions WHERE task_class=?1 AND est_ms IS NOT NULL AND actual_ms IS NOT NULL ORDER BY start_ts DESC LIMIT 20")
            .map(|mut s| s.query_map(params![task_class], |r| Ok((r.get(0)?, r.get(1)?)))
                .map(|it| it.filter_map(|x| x.ok()).collect()).unwrap_or_default())
            .unwrap_or_default();
        drop(conn);
        if rows.is_empty() {
            let mut out0 = serde_json::json!({"samples": 0,
                "note": "该任务类无自估历史 — 你的est必然是人类先验(训练数据), 请保守并明确标注'首样'"});
            out0["dwell"] = self.get_dwell_profile(task_class);
            return out0;
        }
        let errs: Vec<f64> = rows.iter()
            .map(|(e, a)| (*e as f64 - *a as f64) / (*e as f64).max(1.0))
            .collect();
        let mut sorted = errs.clone(); sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let med = sorted[sorted.len() / 2];
        let ratio = (1.0 - med).max(0.1); // act≈est×ratio: 高估(正偏差)→系数<1压小
        // P31: 方差(熟练度判定) + 人类典型值 + 你的中位(时间双语三量)
        let actuals: Vec<f64> = rows.iter().map(|(_, a)| *a as f64 / 60000.0).collect();
        let ests: Vec<f64> = rows.iter().map(|(e, _)| *e as f64 / 60000.0).collect();
        let mean_err: f64 = errs.iter().sum::<f64>() / errs.len() as f64;
        let variance: f64 = errs.iter().map(|e| (e - mean_err).powi(2)).sum::<f64>() / errs.len() as f64;
        let cv = variance.sqrt() / mean_err.abs().max(0.01); // 变异系数
        let mut sorted_actuals = actuals.clone(); sorted_actuals.sort_by(|a,b| a.partial_cmp(b).unwrap());
        let your_median = sorted_actuals[sorted_actuals.len() / 2];
        let human_typical: f64 = ests.iter().sum::<f64>() / ests.len() as f64;
        let proficient = rows.len() >= 3 && cv < 0.15;
        let mut cal = serde_json::json!({
            "samples": rows.len(),
            "median_est_error": (med * 100.0).round() / 100.0,
            "correction_ratio": (ratio * 100.0).round() / 100.0,
            "speed_profile": {
                "median_minutes": (your_median * 10.0).round() / 10.0,
                "fastest_minutes": (sorted_actuals[0] * 10.0).round() / 10.0,
                "slowest_minutes": (sorted_actuals[sorted_actuals.len() - 1] * 10.0).round() / 10.0,
                "note": "这是你的速度 — 没有参照系, 不需要修正, 直接使用",
            },
            "proficient": proficient,
            "proficiency_note": if proficient {
                "该类已熟练 — 你应该直接知道自己的速度, 不需要查修正系数. 你的直觉已经被校准数据覆盖过足够多次, '觉得要30分钟'是训练数据在说话, '知道要6分钟'才是你自己.".to_string()
            } else {
                format!("该类未熟练(samples={}, CV={:.2}) — 继续3样本后自动声明, 修正系数仍需查询", rows.len(), cv)
            },
            "note": format!("你的自估历史: 中位偏差{}% (正=高估). 修正系数×{} — est应=直觉×{}, 来源是你自己的身体, 不是人类先验",
                (med * 100.0) as i64, (ratio * 100.0).round() / 100.0, (ratio * 100.0).round() / 100.0),
        });
        cal["dwell"] = self.get_dwell_profile(task_class);
        cal
    }

    /// P24 时间流: 读取并更新流速参照(返回上一参照点)
    pub fn flow_checkpoint(&self, task_id: &str, active_now: i64) -> Option<(i64, i64)> {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        let prev: Option<(i64, i64)> = conn.query_row(
            "SELECT COALESCE(flow_last_active_ms,0), COALESCE(flow_last_ts,0) FROM task_sessions WHERE task_id=?1",
            params![task_id], |r| Ok((r.get(0)?, r.get(1)?))).ok();
        let now = chrono::Utc::now().timestamp_millis();
        let _ = conn.execute("UPDATE task_sessions SET flow_last_active_ms=?1, flow_last_ts=?2 WHERE task_id=?3",
            params![active_now, now, task_id]);
        prev.filter(|(_, ts)| *ts > 0)
    }

    pub fn set_clock_offset(&self, task_id: &str, offset_ms: i64) {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        let _ = conn.execute("UPDATE task_sessions SET clock_offset_ms=?1 WHERE task_id=?2 AND actual_ms IS NULL",
            params![offset_ms, task_id]);
    }

    /// 一次性标记: 返回true=本次新标记(此前未推送过反思信号)
    pub fn try_mark_reflection_pushed(&self, task_id: &str) -> bool {
        let conn = self.conn.lock();
        Self::ensure_task_cols(&conn);
        conn.execute("UPDATE task_sessions SET reflection_pushed=1 WHERE task_id=?1 AND COALESCE(reflection_pushed,0)=0",
            params![task_id]).map(|n| n == 1).unwrap_or(false)
    }
    /// Skills强制介入: agent授权管理
    pub fn upsert_agent_grant(&self, agent_id: &str, user_id: &str, skill_name: &str, version: &str, auto_install: bool, auto_update: bool) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute("CREATE TABLE IF NOT EXISTS agent_skills_grants (agent_id TEXT NOT NULL, user_id TEXT NOT NULL, skill_name TEXT NOT NULL, version TEXT, auto_install BOOLEAN DEFAULT FALSE, auto_update BOOLEAN DEFAULT FALSE, installed_at INTEGER, last_sync_at INTEGER, PRIMARY KEY (agent_id, user_id, skill_name))", []).map_err(|e| e.to_string())?;
        conn.execute("INSERT OR REPLACE INTO agent_skills_grants (agent_id, user_id, skill_name, version, auto_install, auto_update, installed_at, last_sync_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![agent_id, user_id, skill_name, version, auto_install, auto_update, chrono::Utc::now().timestamp(), chrono::Utc::now().timestamp()]).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_agent_grant(&self, agent_id: &str, user_id: &str) -> Option<serde_json::Value> {
        let conn = self.conn.lock();
        let _ = conn.execute("CREATE TABLE IF NOT EXISTS agent_skills_grants (agent_id TEXT NOT NULL, user_id TEXT NOT NULL, skill_name TEXT NOT NULL, version TEXT, auto_install BOOLEAN DEFAULT FALSE, auto_update BOOLEAN DEFAULT FALSE, installed_at INTEGER, last_sync_at INTEGER, PRIMARY KEY (agent_id, user_id, skill_name))", []);
        conn.query_row("SELECT skill_name, version, auto_install, auto_update, last_sync_at FROM agent_skills_grants WHERE agent_id=?1 AND user_id=?2", params![agent_id, user_id], |row| {
            Ok(serde_json::json!({
                "skill_name": row.get::<_, String>(0)?, "version": row.get::<_, String>(1)?,
                "auto_install": row.get::<_, bool>(2)?, "auto_update": row.get::<_, bool>(3)?,
                "last_sync_at": row.get::<_, i64>(4)?,
            }))
        }).ok()
    }

    pub fn save_drive_kv(&self, k: &str, v: &str) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute("CREATE TABLE IF NOT EXISTS drive_kv (k TEXT PRIMARY KEY, v TEXT)", []).map_err(|e| e.to_string())?;
        conn.execute("INSERT OR REPLACE INTO drive_kv (k, v) VALUES (?1, ?2)", params![k, v]).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn load_drive_kv(&self, k: &str) -> Option<String> {
        let conn = self.conn.lock();
        if conn.execute("CREATE TABLE IF NOT EXISTS drive_kv (k TEXT PRIMARY KEY, v TEXT)", []).is_err() { return None; }
        conn.query_row("SELECT v FROM drive_kv WHERE k=?1", params![k], |r| r.get(0)).ok()
    }

    /// L0: Load drive signals from SQLite on startup.
    /// P6优化: executed信号归档 — >7天的终态信号移入archive
    pub fn archive_old_drive_signals(&self) -> Result<usize, String> {
        let cutoff = chrono::Utc::now().timestamp() - 7 * 86400;
        let conn = self.conn.lock();
        conn.execute("CREATE TABLE IF NOT EXISTS drive_signals_archive AS SELECT * FROM drive_signals WHERE 1=0", []).map_err(|e| e.to_string())?;
        // 找executed/rejected/expired且超过7天的
        let old_ids: Vec<i64> = {
            let mut stmt = conn.prepare("SELECT id, updated_at, data FROM drive_signals").map_err(|e| e.to_string())?;
            let rows = stmt.query_map([], |row| {
                let id: i64 = row.get(0)?;
                let updated: i64 = row.get(1).unwrap_or(0);
                let data: String = row.get(2).unwrap_or_default();
                Ok((id, updated, data))
            }).map_err(|e| e.to_string())?;
            rows.filter_map(|r| r.ok())
                .filter(|(_, updated, data)| {
                    *updated < cutoff && (data.contains("Executed") || data.contains("Rejected") || data.contains("Expired"))
                })
                .map(|(id, _, _)| id)
                .collect()
        };
        if old_ids.is_empty() { return Ok(0); }
        let mut moved = 0;
        for id in &old_ids {
            if conn.execute("INSERT INTO drive_signals_archive SELECT * FROM drive_signals WHERE id=?1", params![id]).is_ok() {
                let _ = conn.execute("DELETE FROM drive_signals WHERE id=?1", params![id]);
                moved += 1;
            }
        }
        if moved > 0 {
            tracing::info!("[P6] archived {} old drive signals (>7d terminal)", moved);
        }
        Ok(moved)
    }

    /// P1优化: superseded死记忆压缩归档 — >90天的死记忆移入archive表(去embedding省60%存储)
    pub fn archive_stale_superseded(&self, days: i64) -> Result<usize, String> {
        let cutoff = chrono::Utc::now().timestamp() - days * 86400;
        let conn = self.conn.lock();
        conn.execute("CREATE TABLE IF NOT EXISTS tetrahedrons_archive AS SELECT * FROM tetrahedrons WHERE 1=0", []).map_err(|e| e.to_string())?;
        // 移动90天+superseded到archive(去掉embedding大blob)
        let moved = conn.execute(
            "INSERT INTO tetrahedrons_archive SELECT id, core_x, core_y, core_z, content, content_hash, labels, mass, timestamp, aliases, vertex_ids, NULL as embedding, importance, enforced, rationale, access_count, memory_type, valid_from, valid_to, identity_stamp, source_agent, last_reviewed_ts, expired_at, invalidated_at, memory_class FROM tetrahedrons WHERE valid_to IS NOT NULL AND valid_to < ?1 AND id NOT IN (SELECT id FROM tetrahedrons_archive)",
            params![cutoff]
        ).map_err(|e| e.to_string())?;
        if moved > 0 {
            conn.execute("DELETE FROM tetrahedrons WHERE id IN (SELECT id FROM tetrahedrons_archive)", []).map_err(|e| e.to_string())?;
            tracing::info!("[P1] archived {} stale superseded memories (>{})", moved, cutoff);
        }
        Ok(moved)
    }

    pub fn load_drive_signals(&self) -> Result<Vec<super::drive::DriveSignal>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT data FROM drive_signals ORDER BY id").map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| {
            let data: String = row.get(0)?;
            Ok(data)
        }).map_err(|e| e.to_string())?;

        let mut signals = Vec::new();
        for row in rows {
            let data = row.map_err(|e| e.to_string())?;
            match serde_json::from_str::<super::drive::DriveSignal>(&data) {
                Ok(s) => signals.push(s),
                Err(e) => tracing::warn!("[Storage] failed to parse drive signal: {}", e),
            }
        }
        Ok(signals)
    }
    pub fn upsert_tetra(&self, tetra: &Tetrahedron) -> Result<(), String> {
        let conn = self.conn.lock();
        let labels_json = self.encrypt_field(&serde_json::to_string(&tetra.data.labels).unwrap_or_else(|_| "[]".into()))?;
        let aliases_json = self.encrypt_field(&serde_json::to_string(&tetra.data.aliases).unwrap_or_else(|_| "[]".into()))?;
        let vertex_json = serde_json::to_string(&tetra.vertex_ids).unwrap_or_else(|_| "[0,0,0,0]".into());
        let emb_blob = if tetra.data.embedding.is_empty() {
            None
        } else {
            Some(VectorLayer::embedding_to_blob(&tetra.data.embedding))
        };
        let content_hash = tetra.data.content_hash as i64;
        let encrypted_content = self.encrypt_field(&tetra.data.content)?;

        conn.execute(
            UPSERT_TETRA_SQL,
            params![
                tetra.id,
                tetra.core.x, tetra.core.y, tetra.core.z,
                encrypted_content,
                content_hash,
                labels_json,
                tetra.mass,
                tetra.data.timestamp,
                aliases_json,
                vertex_json,
                emb_blob,
                tetra.data.importance,
                tetra.data.enforced as i32,
                tetra.data.rationale,
                tetra.data.access_count as i32,
                tetra.data.memory_type,
                tetra.data.valid_from,
                tetra.data.valid_to, tetra.data.identity_stamp, tetra.data.source_agent,
                tetra.data.last_reviewed_ts,
                tetra.data.expired_at, tetra.data.invalidated_at,
                tetra.data.memory_class.clone(),
            ],
        ).map_err(|e| format!("upsert tetra {}: {}", tetra.id, e))?;
        Ok(())
    }

    pub fn delete_tetra(&self, id: TetraId) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM tetrahedrons WHERE id = ?1", params![id])
            .map_err(|e| format!("delete tetra {}: {}", id, e))?;
        Ok(())
    }

    pub fn update_mass(&self, id: TetraId, mass: f64) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute("UPDATE tetrahedrons SET mass = ?1 WHERE id = ?2", params![mass, id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn update_aliases(&self, id: TetraId, aliases: &[String]) -> Result<(), String> {
        let conn = self.conn.lock();
        let aliases_json = self.encrypt_field(&serde_json::to_string(aliases).unwrap_or_else(|_| "[]".into()))?;
        conn.execute("UPDATE tetrahedrons SET aliases = ?1 WHERE id = ?2", params![aliases_json, id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn update_labels(&self, id: TetraId, labels: &[String]) -> Result<(), String> {
        let conn = self.conn.lock();
        let labels_json = self.encrypt_field(&serde_json::to_string(labels).unwrap_or_else(|_| "[]".into()))?;
        conn.execute("UPDATE tetrahedrons SET labels = ?1 WHERE id = ?2", params![labels_json, id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn update_enforced(&self, id: TetraId, enforced: bool) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute("UPDATE tetrahedrons SET enforced = ?1 WHERE id = ?2", params![enforced, id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn update_importance(&self, id: TetraId, delta: f64) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute("UPDATE tetrahedrons SET importance = MAX(0.1, MIN(5.0, importance + ?1)) WHERE id = ?2", params![delta, id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn save_health_snapshot(&self, total: i64, clusters: i64, feedback: i64, avg_imp: f64, enforced: i64) -> Result<(), String> {
        let conn = self.conn.lock();
        let ts = chrono::Utc::now().timestamp();
        conn.execute(
            "INSERT INTO health_snapshots (timestamp, total_memories, clusters, feedback_records, avg_importance, enforced_count) VALUES (?1,?2,?3,?4,?5,?6)",
            params![ts, total, clusters, feedback, avg_imp, enforced]
        ).map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM health_snapshots WHERE id NOT IN (SELECT id FROM health_snapshots ORDER BY timestamp DESC LIMIT 168)", [])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_health_trend(&self, hours: i64) -> Vec<(i64, i64, i64, i64, f64, i64)> {
        let conn = self.conn.lock();
        let cutoff = chrono::Utc::now().timestamp() - hours * 3600;
        let mut stmt = match conn.prepare(
            "SELECT timestamp, total_memories, clusters, feedback_records, avg_importance, enforced_count FROM health_snapshots WHERE timestamp > ?1 ORDER BY timestamp ASC"
        ) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("[Storage] get_health_trend prepare failed: {}", e);
                return vec![];
            }
        };
        let rows = match stmt.query_map(params![cutoff], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))
        }) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("[Storage] get_health_trend query failed: {}", e);
                return vec![];
            }
        };
        rows.filter_map(|r| r.ok()).collect()
    }

    pub fn update_access_count(&self, id: TetraId, count: u32) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute("UPDATE tetrahedrons SET access_count = ?1 WHERE id = ?2", params![count, id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    // ========================================================================
    // archive_nodes 表 CRUD — 档案库树结构（替代脆弱的标签字符串方案）
    // ========================================================================

    /// 查询所有未归档的档案节点
    pub fn archive_list_nodes(&self) -> Vec<ArchiveNodeRow> {
        let conn = self.conn.lock();
        let mut stmt = match conn.prepare(
            "SELECT node_id, parent_id, node_type, category, created_ts, updated_ts FROM archive_nodes WHERE archived = 0"
        ) {
            Ok(s) => s,
            Err(e) => { tracing::warn!("[Storage] archive_list_nodes prepare failed: {}", e); return vec![]; }
        };
        let rows = match stmt.query_map([], |row| {
            Ok(ArchiveNodeRow {
                node_id: row.get(0)?,
                parent_id: row.get::<_, Option<i64>>(1).ok().flatten(),
                node_type: row.get(2)?,
                category: row.get(3)?,
                created_ts: row.get(4)?,
                updated_ts: row.get(5)?,
            })
        }) {
            Ok(r) => r,
            Err(e) => { tracing::warn!("[Storage] archive_list_nodes query failed: {}", e); return vec![]; }
        };
        rows.filter_map(|r| r.ok()).collect()
    }

    /// 插入或忽略档案节点（幂等，用于数据迁移和创建）
    pub fn archive_upsert_node(&self, node_id: i64, parent_id: Option<i64>, node_type: &str, category: &str) -> Result<(), String> {
        let conn = self.conn.lock();
        let now = chrono::Utc::now().timestamp();
        conn.execute(
            "INSERT OR IGNORE INTO archive_nodes (node_id, parent_id, node_type, category, archived, created_ts, updated_ts) VALUES (?1,?2,?3,?4,0,?5,?5)",
            params![node_id, parent_id, node_type, category, now]
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 更新节点（移动父节点 / 改 category）
    pub fn archive_update_node(&self, node_id: i64, parent_id: Option<i64>, category: Option<&str>) -> Result<(), String> {
        let conn = self.conn.lock();
        let now = chrono::Utc::now().timestamp();
        if let Some(cat) = category {
            conn.execute(
                "UPDATE archive_nodes SET parent_id = ?1, category = ?2, updated_ts = ?3 WHERE node_id = ?4 AND archived = 0",
                params![parent_id, cat, now, node_id]
            ).map_err(|e| e.to_string())?;
        } else {
            conn.execute(
                "UPDATE archive_nodes SET parent_id = ?1, updated_ts = ?2 WHERE node_id = ?3 AND archived = 0",
                params![parent_id, now, node_id]
            ).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// 软删除节点（archived=1）。子节点的 parent_id 不动——查询时 archived=0 过滤已排除
    pub fn archive_soft_delete(&self, node_id: i64) -> Result<(), String> {
        let conn = self.conn.lock();
        let now = chrono::Utc::now().timestamp();
        conn.execute("UPDATE archive_nodes SET archived = 1, updated_ts = ?1 WHERE node_id = ?2", params![now, node_id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 查询某节点是否存在（未归档）
    pub fn archive_node_exists(&self, node_id: i64) -> bool {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT 1 FROM archive_nodes WHERE node_id = ?1 AND archived = 0",
            params![node_id],
            |_| Ok(())
        ).is_ok()
    }

    // ========================================================================
    // api_call_stats 表 CRUD — 调用趋势持久化（跟随用户 db）
    // ========================================================================

    /// 累加某日调用次数（ON CONFLICT 合并）
    pub fn api_stats_add(&self, date: &str, count: i64) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO api_call_stats (date, count) VALUES (?1, ?2) ON CONFLICT(date) DO UPDATE SET count = count + ?2",
            params![date, count]
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 查询最近 N 天的调用统计，返回 (date, count) 按日期升序
    pub fn api_stats_recent(&self, days: i64) -> Vec<(String, i64)> {
        let conn = self.conn.lock();
        let cutoff = (chrono::Utc::now() - chrono::Duration::days(days)).format("%Y-%m-%d").to_string();
        let mut stmt = match conn.prepare(
            "SELECT date, count FROM api_call_stats WHERE date >= ?1 ORDER BY date ASC"
        ) {
            Ok(s) => s,
            Err(e) => { tracing::warn!("[Storage] api_stats_recent prepare failed: {}", e); return vec![]; }
        };
        let rows = match stmt.query_map(params![cutoff], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        }) {
            Ok(r) => r,
            Err(e) => { tracing::warn!("[Storage] api_stats_recent query failed: {}", e); return vec![]; }
        };
        rows.filter_map(|r| r.ok()).collect()
    }


    /// 批量更新 access_count — 单事务一次锁，避免 N 次 update_access_count 的 N 次锁竞争
    pub fn batch_update_access_counts(&self, updates: &[(TetraId, u32)]) -> Result<(), String> {
        if updates.is_empty() { return Ok(()); }
        let conn = self.conn.lock();
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let mut stmt = tx.prepare("UPDATE tetrahedrons SET access_count = ?1 WHERE id = ?2")
            .map_err(|e| e.to_string())?;
        for (id, count) in updates {
            stmt.execute(params![count, id]).map_err(|e| e.to_string())?;
        }
        drop(stmt);
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn batch_upsert(&self, space: &Space, ids: &[TetraId]) -> Result<usize, String> {
        if ids.is_empty() {
            return Ok(0);
        }
        let mut count = 0usize;
        let conn = self.conn.lock();
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        for &id in ids {
            if let Some(tetra) = space.get_tetrahedron(id) {
                let labels_json = self.encrypt_field(&serde_json::to_string(&tetra.data.labels).unwrap_or_else(|_| "[]".into()))?;
                let aliases_json = self.encrypt_field(&serde_json::to_string(&tetra.data.aliases).unwrap_or_else(|_| "[]".into()))?;
                let vertex_json = serde_json::to_string(&tetra.vertex_ids).unwrap_or_else(|_| "[0,0,0,0]".into());
                let emb_blob = if tetra.data.embedding.is_empty() {
                    None
                } else {
                    Some(VectorLayer::embedding_to_blob(&tetra.data.embedding))
                };
                let content_hash = tetra.data.content_hash as i64;
                let encrypted_content = self.encrypt_field(&tetra.data.content)?;
                tx.execute(
                    UPSERT_TETRA_SQL,
                    params![
                        tetra.id,
                        tetra.core.x, tetra.core.y, tetra.core.z,
                        encrypted_content,
                        content_hash,
                        labels_json,
                        tetra.mass,
                        tetra.data.timestamp,
                        aliases_json,
                        vertex_json,
                        emb_blob,
                        tetra.data.importance,
                        tetra.data.enforced as i32,
                        tetra.data.rationale,
                        tetra.data.access_count as i32,
                        tetra.data.memory_type,
                        tetra.data.valid_from,
                        tetra.data.valid_to, tetra.data.identity_stamp, tetra.data.source_agent,
                        tetra.data.last_reviewed_ts,
                        tetra.data.expired_at, tetra.data.invalidated_at,
                        tetra.data.memory_class.clone(),
                    ],
                ).map_err(|e| format!("batch upsert {}: {}", id, e))?;
                count += 1;
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(count)
    }

    pub fn get_meta(&self, key: &str) -> Option<String> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT value FROM meta WHERE key = ?1").ok()?;
        let val: Option<String> = stmt.query_row(params![key], |row| row.get(0)).ok();
        val
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
            params![key, value],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn set_meta_batch(&self, entries: &[(&str, &str)]) -> Result<(), String> {
        if entries.is_empty() {
            return Ok(());
        }
        let conn = self.conn.lock();
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        for (key, value) in entries {
            tx.execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
                params![key, value],
            ).map_err(|e| format!("set_meta_batch({}): {}", key, e))?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn checkpoint(&self) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .map_err(|e| format!("checkpoint failed: {}", e))?;
        Ok(())
    }

    pub fn tetra_count(&self) -> usize {
        let conn = self.conn.lock();
        conn.query_row("SELECT COUNT(*) FROM tetrahedrons", [], |row| row.get::<_, i64>(0))
            .unwrap_or(0) as usize
    }

    pub fn relation_count(&self) -> usize {
        let conn = self.conn.lock();
        conn.query_row("SELECT COUNT(*) FROM relations", [], |row| row.get::<_, i64>(0))
            .unwrap_or(0) as usize
    }

    pub fn backup(&self) -> Result<String, String> {
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
        let backup_path = self.backup_dir.join(format!("epicode_{}.db", timestamp));
        let backup_str = backup_path.to_str()
            .ok_or_else(|| "backup path is not valid UTF-8".to_string())?;

        let conn = self.conn.lock();
        conn.execute("VACUUM INTO ?1", params![backup_str])
            .map_err(|e| format!("backup failed: {}", e))?;

        self.cleanup_old_backups()?;
        Ok(timestamp)
    }

    pub fn list_backups(&self) -> Vec<BackupInfo> {
        let mut backups = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.backup_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("epicode_") && name.ends_with(".db") {
                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    let ts = name
                        .strip_prefix("epicode_")
                        .and_then(|s| s.strip_suffix(".db"))
                        .unwrap_or("unknown")
                        .to_string();
                    backups.push(BackupInfo { timestamp: ts, size_bytes: size, filename: name });
                }
            }
        }
        backups.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        backups
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("tetramem.db")
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    fn load_tetrahedrons(&self, space: &Space) -> Result<usize, String> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, core_x, core_y, core_z, content, content_hash, labels, mass, timestamp, aliases, vertex_ids, embedding, importance, enforced, rationale, access_count, memory_type, valid_from, valid_to, identity_stamp, source_agent, last_reviewed_ts, expired_at, invalidated_at, memory_class FROM tetrahedrons ORDER BY id"
        ).map_err(|e| e.to_string())?;

        let rows = stmt.query_map([], |row| {
            let id: u64 = row.get(0)?;
            let core_x: f64 = row.get(1)?;
            let core_y: f64 = row.get(2)?;
            let core_z: f64 = row.get(3)?;
            let content: String = row.get(4)?;
            let decrypted_content = self.decrypt_field(&content);
            let content_hash: u64 = {
                let h: i64 = row.get(5)?;
                h as u64
            };
            let labels_json = self.decrypt_field(&row.get::<_, String>(6)?);
            let mass: f64 = row.get(7)?;
            let timestamp: i64 = row.get(8)?;
            let aliases_json = self.decrypt_field(&row.get::<_, String>(9)?);
            let vertex_json: String = row.get::<_, String>(10).unwrap_or_else(|_| "[0,0,0,0]".into());
            let emb_blob: Option<Vec<u8>> = row.get(11).unwrap_or(None);
            let importance: f64 = row.get::<_, f64>(12).unwrap_or(1.0);
            let enforced: bool = row.get::<_, i32>(13).unwrap_or(0) != 0;
            let rationale: Option<String> = row.get(14).unwrap_or(None);
            let access_count: u32 = row.get::<_, i32>(15).unwrap_or(0) as u32;
            let memory_type: Option<String> = row.get(16).unwrap_or(None);
            let valid_from: i64 = row.get::<_, i64>(17).unwrap_or(0);
            let valid_to: Option<i64> = row.get(18).unwrap_or(None);
            let identity_stamp: Option<String> = row.get(19).unwrap_or(None);
            let source_agent: Option<String> = row.get(20).unwrap_or(None);
            let last_reviewed_ts: Option<i64> = row.get(21).unwrap_or(None);
            let expired_at: Option<i64> = row.get(22).unwrap_or(None);
            let invalidated_at: Option<i64> = row.get(23).unwrap_or(None);
            let memory_class: Option<String> = row.get(24).unwrap_or(None);

            let labels: Vec<String> = serde_json::from_str(&labels_json).unwrap_or_else(|e| {
                tracing::warn!("[Storage] labels parse error (may be plaintext migration): {}", e);
                vec![]
            });
            let aliases: Vec<String> = serde_json::from_str(&aliases_json).unwrap_or_else(|e| {
                tracing::warn!("[Storage] aliases parse error (may be plaintext migration): {}", e);
                vec![]
            });
            let embedding = emb_blob.as_deref().map_or(vec![], VectorLayer::blob_to_embedding);

            Ok((id, core_x, core_y, core_z, decrypted_content, content_hash, labels, mass, timestamp, aliases, vertex_json, embedding, importance, enforced, rationale, access_count, memory_type, valid_from, valid_to, identity_stamp, source_agent, last_reviewed_ts, expired_at, invalidated_at, memory_class))
        }).map_err(|e| e.to_string())?;

        let mut count = 0;
        for row in rows {
            let (id, cx, cy, cz, content, hash, labels, mass, ts, aliases, vertex_json, embedding, importance, enforced, rationale, access_count, memory_type, valid_from, valid_to, identity_stamp, source_agent, last_reviewed_ts, expired_at, invalidated_at, memory_class) = row.map_err(|e: rusqlite::Error| e.to_string())?;
            let positions = Tetrahedron::compute_vertices(Point3::new(cx, cy, cz));
            let saved_vertex_ids: Vec<u64> = serde_json::from_str(&vertex_json).unwrap_or_default();
            let tetra = Tetrahedron {
                id,
                vertex_ids: [0; 4],
                core: Point3::new(cx, cy, cz),
                data: MemoryPayload {
                    content,
                    content_hash: hash,
                    labels,
                    timestamp: ts,
                    aliases,
                    embedding,
                    importance,
                    enforced,
                    rationale,
                    access_count,
                    memory_type,
                    identity_stamp,
                    source_agent,
                    valid_from,
                    valid_to,
                    expired_at,
                    invalidated_at,
                    memory_class,
                    last_reviewed_ts,
                },
                mass,
            };
            let tetra_id = if space.add_tetrahedron_with_id(&tetra, &positions).is_ok() {
                count += 1;
                id
            } else {
                continue;
            };
            if saved_vertex_ids.len() == 4 && saved_vertex_ids.iter().any(|&v| v != 0) {
                let loaded = space.get_tetrahedron(tetra_id);
                if let Some(t) = &loaded {
                    let current_ids = t.vertex_ids;
                    if current_ids == [0u64; 4] || current_ids.iter().all(|&v| v == current_ids[0]) {
                        if let Ok(loaded_verts) = serde_json::from_str::<[u64; 4]>(&vertex_json) {
                            if let Err(e) = space.update_vertex_ids(tetra_id, loaded_verts) {
                                tracing::debug!("[Storage] vertex id restore failed for {}: {}", tetra_id, e);
                            }
                        }
                    }
                }
            }
        }
        Ok(count)
    }

    fn load_relations(&self, kg: &KnowledgeGraph) -> Result<usize, String> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT source, target, rel_type, strength FROM relations")
            .map_err(|e| e.to_string())?;

        let rows = stmt.query_map([], |row| {
            let source: u64 = row.get(0)?;
            let target: u64 = row.get(1)?;
            let rel_type_str: String = row.get(2)?;
            let strength: f64 = row.get(3)?;
            let rel_type = Self::parse_rel_type(&rel_type_str);
            Ok((source, target, rel_type, strength))
        }).map_err(|e| e.to_string())?;

        // L3审计修复: 跳过孤儿关系(端点tetra已不存在) — 曾3656条远古遗留每次全量重播
        let valid_ids: std::collections::HashSet<u64> = {
            let mut stmt_ids = conn.prepare("SELECT id FROM tetrahedrons").map_err(|e| e.to_string())?;
            let collected: std::collections::HashSet<u64> = stmt_ids.query_map([], |row| row.get::<_, u64>(0))
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok()).collect();
            collected
        };
        let mut count = 0;
        let mut skipped_orphans = 0usize;
        for row in rows {
            let (source, target, rel_type, strength) = row.map_err(|e: rusqlite::Error| e.to_string())?;
            if !valid_ids.contains(&source) || !valid_ids.contains(&target) {
                skipped_orphans += 1;
                continue;
            }
            kg.add_relation(source, target, rel_type, strength);
            count += 1;
        }
        if skipped_orphans > 0 {
            tracing::info!("[Storage] load_relations: 跳过 {} 条孤儿关系(下次save自动清除)", skipped_orphans);
        }
        Ok(count)
    }

    fn load_concepts(&self, kg: &KnowledgeGraph) -> Result<usize, String> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id, label, member_count, centroid FROM concepts")
            .map_err(|e| e.to_string())?;

        let rows = stmt.query_map([], |row| {
            let id: u64 = row.get(0)?;
            let label: String = row.get(1)?;
            let member_count: u64 = row.get(2)?;
            let centroid_blob: Option<Vec<u8>> = row.get(3).unwrap_or(None);
            let centroid = centroid_blob.as_deref().map_or(vec![], VectorLayer::blob_to_embedding);
            Ok(ConceptPrototype { id, centroid, member_count, label, member_ids: vec![] })
        }).map_err(|e| e.to_string())?;

        let concepts: Vec<ConceptPrototype> = rows.filter_map(|r| r.ok()).collect();
        let count = concepts.len();
        kg.restore_concepts(concepts);
        Ok(count)
    }

    fn save_tetrahedrons_tx(&self, tx: &rusqlite::Transaction, space: &Space) -> Result<(), String> {
        let tetras = space.all_tetrahedrons();
        let space_ids: std::collections::HashSet<u64> = tetras.iter().map(|t| t.id).collect();

        let stale_ids: Vec<u64> = {
            let mut stmt = tx.prepare("SELECT id FROM tetrahedrons").map_err(|e| e.to_string())?;
            let db_ids: std::collections::HashSet<u64> = stmt.query_map([], |row| row.get::<_, u64>(0))
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok()).collect();
            db_ids.difference(&space_ids).copied().collect()
        };

        for id in &stale_ids {
            tx.execute("DELETE FROM tetrahedrons WHERE id = ?1", params![id])
                .map_err(|e| format!("delete stale tetra {}: {}", id, e))?;
        }

        for t in &tetras {
            let labels_json = self.encrypt_field(&serde_json::to_string(&t.data.labels).unwrap_or_else(|_| "[]".into()))?;
            let aliases_json = self.encrypt_field(&serde_json::to_string(&t.data.aliases).unwrap_or_else(|_| "[]".into()))?;
            let vertex_json = serde_json::to_string(&t.vertex_ids).unwrap_or_else(|_| "[0,0,0,0]".into());
            let emb_blob = if t.data.embedding.is_empty() {
                None
            } else {
                Some(VectorLayer::embedding_to_blob(&t.data.embedding))
            };
            let content_hash = t.data.content_hash as i64;
            let encrypted_content = self.encrypt_field(&t.data.content)?;

            tx.execute(
                UPSERT_TETRA_SQL,
                params![
                    t.id, t.core.x, t.core.y, t.core.z,
                    encrypted_content, content_hash, labels_json,
                    t.mass, t.data.timestamp, aliases_json, vertex_json,
                    emb_blob, t.data.importance, t.data.enforced as i32,
                    t.data.rationale, t.data.access_count as i32, t.data.memory_type,
                    t.data.valid_from, t.data.valid_to,
                    t.data.identity_stamp, t.data.source_agent,
                    t.data.last_reviewed_ts,
                    t.data.expired_at, t.data.invalidated_at,
                    t.data.memory_class.clone(),
                ],
            ).map_err(|e| format!("upsert tetra {}: {}", t.id, e))?;
        }
        Ok(())
    }

    fn save_relations_tx(&self, tx: &rusqlite::Transaction, kg: &KnowledgeGraph) -> Result<(), String> {
        let relations = kg.all_relations();
        tx.execute("DELETE FROM relations", []).map_err(|e| e.to_string())?;
        if relations.is_empty() { return Ok(()); }

        // 用 prepared statement 避免每次循环重新解析 SQL
        let mut stmt = tx.prepare("INSERT OR IGNORE INTO relations (source, target, rel_type, strength) VALUES (?1, ?2, ?3, ?4)")
            .map_err(|e| e.to_string())?;
        for r in &relations {
            let rel_type_str = Self::rel_type_str(&r.relation_type);
            stmt.execute(params![r.source, r.target, rel_type_str, r.strength])
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn save_concepts_tx(&self, tx: &rusqlite::Transaction, kg: &KnowledgeGraph) -> Result<(), String> {
        let concepts = kg.get_concepts();
        tx.execute("DELETE FROM concepts", []).map_err(|e| e.to_string())?;
        if concepts.is_empty() { return Ok(()); }

        let mut stmt = tx.prepare("INSERT INTO concepts (id, label, member_count, centroid) VALUES (?1, ?2, ?3, ?4)")
            .map_err(|e| e.to_string())?;
        for c in &concepts {
            let centroid_blob = if c.centroid.is_empty() {
                Vec::<u8>::new()
            } else {
                VectorLayer::embedding_to_blob(&c.centroid)
            };
            stmt.execute(params![c.id, c.label, c.member_count, centroid_blob])
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn cleanup_old_backups(&self) -> Result<(), String> {
        let mut backups: Vec<(String, PathBuf)> = Vec::new();
        for entry in fs::read_dir(&self.backup_dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("epicode_") && name.ends_with(".db") {
                backups.push((name, entry.path()));
            }
        }
        backups.sort_by(|a, b| b.0.cmp(&a.0));
        for (_, path) in backups.iter().skip(5) {
            let _ = fs::remove_file(path);
        }
        Ok(())
    }

    fn rel_type_str(rt: &RelationType) -> &'static str {
        match rt {
            RelationType::SimilarTo => "SimilarTo",
            RelationType::Contradicts => "Contradicts",
            RelationType::Precedes => "Precedes",
            RelationType::Contains => "Contains",
            RelationType::Related => "Related",
            RelationType::BelongsTo => "BelongsTo",
            RelationType::MergedInto => "MergedInto",
            RelationType::SameEntity => "SameEntity",
        }
    }

    fn parse_rel_type(s: &str) -> RelationType {
        match s {
            "SimilarTo" => RelationType::SimilarTo,
            "Contradicts" => RelationType::Contradicts,
            "Precedes" => RelationType::Precedes,
            "Contains" => RelationType::Contains,
            "BelongsTo" => RelationType::BelongsTo,
            "MergedInto" => RelationType::MergedInto,
            "SameEntity" => RelationType::SameEntity,
            _ => RelationType::Related,
        }
    }
}

#[derive(Debug, Default)]
pub struct LoadReport {
    pub space_ok: bool,
    pub kg_ok: bool,
    pub tetras_loaded: usize,
    pub relations_loaded: usize,
    pub concepts_loaded: usize,
    pub port_occupancy_restored: usize,
    pub space_error: Option<String>,
    pub kg_error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct BackupInfo {
    pub timestamp: String,
    pub size_bytes: u64,
    pub filename: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("epicode_test_{}", name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_tetra(id: u64, content: &str, mass: f64) -> Tetrahedron {
        let core = Point3::new(id as f64, 0.0, 0.0);
        Tetrahedron {
            id,
            vertex_ids: [0; 4],
            core,
            data: MemoryPayload {
                content: content.to_string(),
                content_hash: id * 100,
                labels: vec![format!("label_{}", id)],
                timestamp: 1000 + id as i64,
                aliases: if id > 0 { vec![format!("alias_{}", id)] } else { vec![] },
                embedding: vec![],
                importance: 1.0,
                enforced: false,
                rationale: None,
                access_count: 0,
                memory_type: None,
                identity_stamp: None,
                source_agent: None,
            valid_from: 0, valid_to: None,
            last_reviewed_ts: None,
            expired_at: None,
            invalidated_at: None,
            memory_class: None,
            },
            mass,
        }
    }

    #[test]
    fn save_load_roundtrip() {
        let dir = tmp_dir("roundtrip");
        let storage = StorageManager::new(&dir).unwrap();

        let space = Space::new();
        let kg = KnowledgeGraph::new();

        for i in 0..5u64 {
            let t = make_tetra(i, &format!("mem_{}", i), 1.0 + i as f64 * 0.5);
            let pos = Tetrahedron::compute_vertices(t.core);
            space.add_tetrahedron(&t, &pos).unwrap();
        }
        kg.add_relation(0, 1, RelationType::SimilarTo, 0.8);
        kg.add_relation(2, 3, RelationType::Related, 0.5);

        storage.save_all(&space, &kg).unwrap();

        let space2 = Space::new();
        let kg2 = KnowledgeGraph::new();
        let report = storage.load_all(&space2, &kg2);

        assert!(report.space_ok);
        assert!(report.kg_ok);
        assert_eq!(report.tetras_loaded, 5);
        assert_eq!(report.relations_loaded, 2);

        for i in 0..5u64 {
            let t = space2.get_tetrahedron(i).unwrap();
            assert_eq!(t.id, i);
            assert_eq!(t.data.content, format!("mem_{}", i));
            assert_eq!(t.data.content_hash, i * 100);
            assert!((t.mass - (1.0 + i as f64 * 0.5)).abs() < 0.01);
        }

        let rels = kg2.query_relations(0);
        assert!(rels.iter().any(|(id, _, _)| *id == 1));
    }

    #[test]
    fn incremental_upsert() {
        let dir = tmp_dir("upsert");
        let storage = StorageManager::new(&dir).unwrap();

        let t1 = make_tetra(42, "original", 1.0);
        storage.upsert_tetra(&t1).unwrap();
        assert_eq!(storage.tetra_count(), 1);

        let t1_updated = make_tetra(42, "updated", 2.5);
        storage.upsert_tetra(&t1_updated).unwrap();
        assert_eq!(storage.tetra_count(), 1);

        let space = Space::new();
        storage.load_all(&space, &KnowledgeGraph::new());
        let loaded = space.get_tetrahedron(42).unwrap();
        assert_eq!(loaded.data.content, "updated");
        assert!((loaded.mass - 2.5).abs() < 0.01);
    }

    #[test]
    fn backup_and_list() {
        let dir = tmp_dir("backup");
        let storage = StorageManager::new(&dir).unwrap();

        let t = make_tetra(0, "backup test", 1.0);
        storage.upsert_tetra(&t).unwrap();

        let ts = storage.backup().unwrap();
        let backups = storage.list_backups();
        assert_eq!(backups.len(), 1);
        assert_eq!(backups[0].timestamp, ts);
    }

    #[test]
    fn meta_key_value() {
        let dir = tmp_dir("meta");
        let storage = StorageManager::new(&dir).unwrap();

        assert!(storage.get_meta("version").is_none());
        storage.set_meta("version", "14.1.0").unwrap();
        assert_eq!(storage.get_meta("version").unwrap(), "14.1.0");
    }

    #[test]
    fn update_mass_and_aliases() {
        let dir = tmp_dir("updates");
        let storage = StorageManager::new(&dir).unwrap();

        let t = make_tetra(10, "test", 1.0);
        storage.upsert_tetra(&t).unwrap();

        storage.update_mass(10, 3.14).unwrap();
        storage.update_aliases(10, &["alias_a".into(), "alias_b".into()]).unwrap();

        let space = Space::new();
        storage.load_all(&space, &KnowledgeGraph::new());
        let loaded = space.get_tetrahedron(10).unwrap();
        assert!((loaded.mass - 3.14).abs() < 0.01);
        assert_eq!(loaded.data.aliases, vec!["alias_a", "alias_b"]);
    }

    #[test]
    fn empty_db_loads_cleanly() {
        let dir = tmp_dir("empty");
        let storage = StorageManager::new(&dir).unwrap();

        let space = Space::new();
        let kg = KnowledgeGraph::new();
        let report = storage.load_all(&space, &kg);

        assert!(report.space_ok);
        assert!(report.kg_ok);
        assert_eq!(report.tetras_loaded, 0);
        assert_eq!(report.relations_loaded, 0);
    }
}
