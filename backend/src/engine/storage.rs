use std::fs;
use std::path::{Path, PathBuf};

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, OpenFlags};

use crate::domain::space::Space;
use crate::domain::tetra::{MemoryPayload, TetraId, Tetrahedron};
use crate::domain::vertex::Point3;
use crate::engine::knowledge::{ConceptPrototype, KnowledgeGraph, RelationType};
use crate::engine::vector::VectorLayer;

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA busy_timeout = 5000;
PRAGMA wal_autocheckpoint = 1000;
PRAGMA cache_size = -8192;
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
";

const MIGRATION_ADD_EMBEDDING: &str = "ALTER TABLE tetrahedrons ADD COLUMN embedding BLOB";
const MIGRATION_ADD_IMPORTANCE: &str =
    "ALTER TABLE tetrahedrons ADD COLUMN importance REAL NOT NULL DEFAULT 1.0";
const MIGRATION_ADD_ENFORCED: &str =
    "ALTER TABLE tetrahedrons ADD COLUMN enforced INTEGER NOT NULL DEFAULT 0";
const MIGRATION_ADD_RATIONALE: &str =
    "ALTER TABLE tetrahedrons ADD COLUMN rationale TEXT DEFAULT NULL";
const MIGRATION_ADD_ACCESS_COUNT: &str =
    "ALTER TABLE tetrahedrons ADD COLUMN access_count INTEGER NOT NULL DEFAULT 0";
const MIGRATION_ADD_MEMORY_TYPE: &str =
    "ALTER TABLE tetrahedrons ADD COLUMN memory_type TEXT DEFAULT NULL";
const MIGRATION_ADD_HEALTH_SNAPSHOTS: &str = "CREATE TABLE IF NOT EXISTS health_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp INTEGER NOT NULL,
    total_memories INTEGER NOT NULL,
    clusters INTEGER NOT NULL,
    feedback_records INTEGER NOT NULL DEFAULT 0,
    avg_importance REAL NOT NULL DEFAULT 0,
    enforced_count INTEGER NOT NULL DEFAULT 0
)";

const MIGRATION_ADD_KEY_METADATA: &str = "CREATE TABLE IF NOT EXISTS key_metadata (
    key_id TEXT PRIMARY KEY,
    created_at INTEGER NOT NULL,
    rotated_at INTEGER,
    revoked_at INTEGER,
    status TEXT NOT NULL DEFAULT 'active',
    version INTEGER NOT NULL,
    created_timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
)";

const MIGRATION_ADD_KEY_EVENTS: &str = "CREATE TABLE IF NOT EXISTS key_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    event_type TEXT NOT NULL,
    key_id TEXT NOT NULL,
    old_key_id TEXT,
    new_key_id TEXT,
    reason TEXT,
    event_timestamp INTEGER NOT NULL,
    created_timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_key_id (key_id),
    INDEX idx_event_type (event_type),
    INDEX idx_event_timestamp (event_timestamp)
)";

const MIGRATION_ADD_DELETED_MEMORIES: &str = "CREATE TABLE IF NOT EXISTS deleted_memories (
   id INTEGER PRIMARY KEY,
   tetra_json TEXT NOT NULL,
   deleted_at INTEGER NOT NULL,
   expires_at INTEGER NOT NULL
)";

#[derive(Debug, Clone, serde::Serialize)]
pub struct DeletedMemoryInfo {
    pub id: TetraId,
    pub content: String,
    pub labels: Vec<String>,
    pub timestamp: i64,
    pub deleted_at: i64,
    pub expires_at: i64,
}

pub struct StorageManager {
    pool: Pool<SqliteConnectionManager>,
    data_dir: PathBuf,
    backup_dir: PathBuf,
    crypto: Option<super::crypto::CryptoEngine>,
    crypto_user: String,
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

        let manager = SqliteConnectionManager::file(&db_path)
            .with_flags(OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE);

        let pool = r2d2::Pool::builder()
            .max_size(16)
            .connection_timeout(std::time::Duration::from_secs(5))
            .idle_timeout(Some(std::time::Duration::from_secs(60)))
            .build(manager)
            .map_err(|e| format!("failed to build connection pool: {}", e))?;

        // Initialize schema using one connection from the pool
        {
            let conn = pool.get().map_err(|e| {
                format!("failed to get connection from pool for schema init: {}", e)
            })?;

            conn.execute_batch(SCHEMA)
                .map_err(|e| format!("failed to initialize schema: {}", e))?;
            if let Err(e) = conn.execute_batch(MIGRATION_ADD_EMBEDDING) {
                tracing::debug!(
                    "[Storage] embedding migration skipped (likely already applied): {}",
                    e
                );
            }
            if let Err(e) = conn.execute_batch(MIGRATION_ADD_IMPORTANCE) {
                tracing::debug!(
                    "[Storage] importance migration skipped (likely already applied): {}",
                    e
                );
            }
            if let Err(e) = conn.execute_batch(MIGRATION_ADD_ENFORCED) {
                tracing::debug!(
                    "[Storage] enforced migration skipped (likely already applied): {}",
                    e
                );
            }
            if let Err(e) = conn.execute_batch(MIGRATION_ADD_RATIONALE) {
                tracing::debug!("[Storage] rationale migration skipped: {}", e);
            }
            if let Err(e) = conn.execute_batch(MIGRATION_ADD_ACCESS_COUNT) {
                tracing::debug!("[Storage] access_count migration skipped: {}", e);
            }
            if let Err(e) = conn.execute_batch(MIGRATION_ADD_MEMORY_TYPE) {
                tracing::debug!("[Storage] memory_type migration skipped: {}", e);
            }
            if let Err(e) = conn.execute_batch(MIGRATION_ADD_HEALTH_SNAPSHOTS) {
                tracing::debug!("[Storage] health_snapshots migration skipped: {}", e);
            }
            if let Err(e) = conn.execute_batch(MIGRATION_ADD_KEY_METADATA) {
                tracing::debug!("[Storage] key_metadata migration skipped: {}", e);
            }
            if let Err(e) = conn.execute_batch(MIGRATION_ADD_KEY_EVENTS) {
                tracing::debug!("[Storage] key_events migration skipped: {}", e);
            }
            if let Err(e) = conn.execute_batch(MIGRATION_ADD_DELETED_MEMORIES) {
                tracing::debug!("[Storage] deleted_memories migration skipped: {}", e);
            }
        }

        tracing::info!(
            "SQLite database opened with connection pool: {}",
            db_path.display()
        );

        Ok(Self {
            pool,
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
            crypto
                .encrypt_content(content, &self.crypto_user)
                .map_err(|e| {
                    tracing::error!(
                        "[Storage] FATAL: encrypt failed for user {}: {}. Data NOT stored.",
                        self.crypto_user,
                        e
                    );
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
                    tracing::error!(
                        "[Storage] decrypt failed for user {}: {}. Returning raw.",
                        self.crypto_user,
                        e
                    );
                    content.to_string()
                }
            }
        } else {
            content.to_string()
        }
    }

    pub fn load_all(&self, space: &Space, kg: &KnowledgeGraph) -> LoadReport {
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

        space.restore_counters();
        report
    }

    pub fn save_all(&self, space: &Space, kg: &KnowledgeGraph) -> Result<(), String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;

        self.save_tetrahedrons_tx(&tx, space)?;
        self.save_relations_tx(&tx, kg)?;
        self.save_concepts_tx(&tx, kg)?;

        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn save_space_only(&self, space: &Space) -> Result<(), String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        self.save_tetrahedrons_tx(&tx, space)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn save_kg_only(&self, kg: &KnowledgeGraph) -> Result<(), String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        self.save_relations_tx(&tx, kg)?;
        self.save_concepts_tx(&tx, kg)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn upsert_tetra(&self, tetra: &Tetrahedron) -> Result<(), String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        let labels_json = self.encrypt_field(
            &serde_json::to_string(&tetra.data.labels).unwrap_or_else(|_| "[]".into()),
        )?;
        let aliases_json = self.encrypt_field(
            &serde_json::to_string(&tetra.data.aliases).unwrap_or_else(|_| "[]".into()),
        )?;
        let vertex_json =
            serde_json::to_string(&tetra.vertex_ids).unwrap_or_else(|_| "[0,0,0,0]".into());
        let emb_blob = if tetra.data.embedding.is_empty() {
            None
        } else {
            Some(VectorLayer::embedding_to_blob(&tetra.data.embedding))
        };
        let content_hash = tetra.data.content_hash as i64;
        let encrypted_content = self.encrypt_field(&tetra.data.content)?;

        conn.execute(
            "INSERT OR REPLACE INTO tetrahedrons (id, core_x, core_y, core_z, content, content_hash, labels, mass, timestamp, aliases, vertex_ids, embedding, importance, enforced)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
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
            ],
        ).map_err(|e| format!("upsert tetra {}: {}", tetra.id, e))?;
        Ok(())
    }

    pub fn delete_tetra(&self, id: TetraId) -> Result<(), String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM tetrahedrons WHERE id = ?1", params![id])
            .map_err(|e| format!("delete tetra {}: {}", id, e))?;
        Ok(())
    }

    pub fn soft_delete_tetra(
        &self,
        tetra: &Tetrahedron,
        retention_days: i64,
    ) -> Result<(i64, i64), String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let deleted_at = chrono::Utc::now().timestamp();
        let expires_at = deleted_at + retention_days * 86_400;
        let tetra_json =
            serde_json::to_string(tetra).map_err(|e| format!("serialize deleted tetra: {}", e))?;
        tx.execute(
            "INSERT OR REPLACE INTO deleted_memories (id, tetra_json, deleted_at, expires_at) VALUES (?1, ?2, ?3, ?4)",
            params![tetra.id, tetra_json, deleted_at, expires_at],
        )
        .map_err(|e| format!("archive deleted tetra {}: {}", tetra.id, e))?;
        tx.execute("DELETE FROM tetrahedrons WHERE id = ?1", params![tetra.id])
            .map_err(|e| format!("delete tetra {}: {}", tetra.id, e))?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok((deleted_at, expires_at))
    }

    pub fn restore_deleted_tetra(&self, id: TetraId) -> Result<Option<Tetrahedron>, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let tetra_json: Option<String> = tx
            .query_row(
                "SELECT tetra_json FROM deleted_memories WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .ok();

        let Some(tetra_json) = tetra_json else {
            return Ok(None);
        };

        let tetra: Tetrahedron =
            serde_json::from_str(&tetra_json).map_err(|e| format!("restore parse tetra: {}", e))?;
        let labels_json = self.encrypt_field(
            &serde_json::to_string(&tetra.data.labels).unwrap_or_else(|_| "[]".into()),
        )?;
        let aliases_json = self.encrypt_field(
            &serde_json::to_string(&tetra.data.aliases).unwrap_or_else(|_| "[]".into()),
        )?;
        let vertex_json =
            serde_json::to_string(&tetra.vertex_ids).unwrap_or_else(|_| "[0,0,0,0]".into());
        let emb_blob = if tetra.data.embedding.is_empty() {
            None
        } else {
            Some(VectorLayer::embedding_to_blob(&tetra.data.embedding))
        };
        let content_hash = tetra.data.content_hash as i64;
        let encrypted_content = self.encrypt_field(&tetra.data.content)?;
        tx.execute(
            "INSERT OR REPLACE INTO tetrahedrons (id, core_x, core_y, core_z, content, content_hash, labels, mass, timestamp, aliases, vertex_ids, embedding, importance, enforced, rationale, access_count, memory_type)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
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
                tetra.data.access_count,
                tetra.data.memory_type,
            ],
        )
        .map_err(|e| format!("restore tetra {}: {}", tetra.id, e))?;
        tx.execute("DELETE FROM deleted_memories WHERE id = ?1", params![id])
            .map_err(|e| format!("remove deleted archive {}: {}", id, e))?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(Some(tetra))
    }

    pub fn list_deleted_memories(&self) -> Result<Vec<DeletedMemoryInfo>, String> {
        self.purge_expired_deleted_memories()?;
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT id, tetra_json, deleted_at, expires_at FROM deleted_memories ORDER BY deleted_at DESC")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                let id: u64 = row.get(0)?;
                let tetra_json: String = row.get(1)?;
                let deleted_at: i64 = row.get(2)?;
                let expires_at: i64 = row.get(3)?;
                Ok((id, tetra_json, deleted_at, expires_at))
            })
            .map_err(|e| e.to_string())?;

        let mut items = Vec::new();
        for row in rows {
            let (id, tetra_json, deleted_at, expires_at) =
                row.map_err(|e: rusqlite::Error| e.to_string())?;
            let tetra: Tetrahedron = serde_json::from_str(&tetra_json)
                .map_err(|e| format!("list deleted parse tetra {}: {}", id, e))?;
            items.push(DeletedMemoryInfo {
                id,
                content: tetra.data.content,
                labels: tetra.data.labels,
                timestamp: tetra.data.timestamp,
                deleted_at,
                expires_at,
            });
        }
        Ok(items)
    }

    pub fn purge_expired_deleted_memories(&self) -> Result<usize, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().timestamp();
        let deleted = conn
            .execute(
                "DELETE FROM deleted_memories WHERE expires_at <= ?1",
                params![now],
            )
            .map_err(|e| format!("purge expired deleted memories: {}", e))?;
        Ok(deleted)
    }

    pub fn update_mass(&self, id: TetraId, mass: f64) -> Result<(), String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE tetrahedrons SET mass = ?1 WHERE id = ?2",
            params![mass, id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn update_aliases(&self, id: TetraId, aliases: &[String]) -> Result<(), String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        let aliases_json =
            self.encrypt_field(&serde_json::to_string(aliases).unwrap_or_else(|_| "[]".into()))?;
        conn.execute(
            "UPDATE tetrahedrons SET aliases = ?1 WHERE id = ?2",
            params![aliases_json, id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn update_labels(&self, id: TetraId, labels: &[String]) -> Result<(), String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        let labels_json =
            self.encrypt_field(&serde_json::to_string(labels).unwrap_or_else(|_| "[]".into()))?;
        conn.execute(
            "UPDATE tetrahedrons SET labels = ?1 WHERE id = ?2",
            params![labels_json, id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn update_enforced(&self, id: TetraId, enforced: bool) -> Result<(), String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE tetrahedrons SET importance = importance, enforced = ?1 WHERE id = ?2",
            params![enforced, id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn update_importance(&self, id: TetraId, delta: f64) -> Result<(), String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        conn.execute("UPDATE tetrahedrons SET importance = MAX(0.1, MIN(5.0, importance + ?1)) WHERE id = ?2", params![delta, id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn save_health_snapshot(
        &self,
        total: i64,
        clusters: i64,
        feedback: i64,
        avg_imp: f64,
        enforced: i64,
    ) -> Result<(), String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
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
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("[Storage] failed to get connection from pool: {}", e);
                return Vec::new();
            }
        };
        let cutoff = chrono::Utc::now().timestamp() - hours * 3600;
        let mut stmt = match conn.prepare(
            "SELECT timestamp, total_memories, clusters, feedback_records, avg_importance, enforced_count FROM health_snapshots WHERE timestamp > ?1 ORDER BY timestamp ASC"
        ) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("[Storage] failed to prepare statement: {}", e);
                return Vec::new();
            }
        };
        let rows = match stmt.query_map(params![cutoff], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        }) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("[Storage] failed to query health trend: {}", e);
                return Vec::new();
            }
        };
        rows.filter_map(|r| r.ok()).collect()
    }

    pub fn update_access_count(&self, id: TetraId, count: u32) -> Result<(), String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE tetrahedrons SET access_count = ?1 WHERE id = ?2",
            params![count, id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn batch_upsert(&self, space: &Space, ids: &[TetraId]) -> Result<usize, String> {
        if ids.is_empty() {
            return Ok(0);
        }
        let mut count = 0usize;
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        for &id in ids {
            if let Some(tetra) = space.get_tetrahedron(id) {
                let labels_json = self.encrypt_field(
                    &serde_json::to_string(&tetra.data.labels).unwrap_or_else(|_| "[]".into()),
                )?;
                let aliases_json = self.encrypt_field(
                    &serde_json::to_string(&tetra.data.aliases).unwrap_or_else(|_| "[]".into()),
                )?;
                let vertex_json =
                    serde_json::to_string(&tetra.vertex_ids).unwrap_or_else(|_| "[0,0,0,0]".into());
                let emb_blob = if tetra.data.embedding.is_empty() {
                    None
                } else {
                    Some(VectorLayer::embedding_to_blob(&tetra.data.embedding))
                };
                let content_hash = tetra.data.content_hash as i64;
                let encrypted_content = self.encrypt_field(&tetra.data.content)?;
                tx.execute(
                    "INSERT OR REPLACE INTO tetrahedrons (id, core_x, core_y, core_z, content, content_hash, labels, mass, timestamp, aliases, vertex_ids, embedding, importance, enforced)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
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
                    ],
                ).map_err(|e| format!("batch upsert {}: {}", id, e))?;
                count += 1;
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(count)
    }

    pub fn get_meta(&self, key: &str) -> Option<String> {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("[Storage] failed to get connection from pool: {}", e);
                return None;
            }
        };
        let mut stmt = conn.prepare("SELECT value FROM meta WHERE key = ?1").ok()?;
        let val: Option<String> = stmt.query_row(params![key], |row| row.get(0)).ok();
        val
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
            params![key, value],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn set_meta_batch(&self, entries: &[(&str, &str)]) -> Result<(), String> {
        if entries.is_empty() {
            return Ok(());
        }
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        for (key, value) in entries {
            tx.execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
                params![key, value],
            )
            .map_err(|e| format!("set_meta_batch({}): {}", key, e))?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn checkpoint(&self) -> Result<(), String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .map_err(|e| format!("checkpoint failed: {}", e))?;
        Ok(())
    }

    pub fn tetra_count(&self) -> usize {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("[Storage] failed to get connection from pool: {}", e);
                return 0;
            }
        };
        conn.query_row("SELECT COUNT(*) FROM tetrahedrons", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap_or(0) as usize
    }

    pub fn relation_count(&self) -> usize {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("[Storage] failed to get connection from pool: {}", e);
                return 0;
            }
        };
        conn.query_row("SELECT COUNT(*) FROM relations", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap_or(0) as usize
    }

    pub fn backup(&self) -> Result<String, String> {
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
        let backup_path = self.backup_dir.join(format!("epicode_{}.db", timestamp));
        let backup_str = backup_path
            .to_str()
            .ok_or_else(|| "backup path is not valid UTF-8".to_string())?;

        let conn = self.pool.get().map_err(|e| e.to_string())?;
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
                    backups.push(BackupInfo {
                        timestamp: ts,
                        size_bytes: size,
                        filename: name,
                    });
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
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        let mut stmt = conn.prepare(
            "SELECT id, core_x, core_y, core_z, content, content_hash, labels, mass, timestamp, aliases, vertex_ids, embedding, importance, enforced, rationale, access_count, memory_type FROM tetrahedrons ORDER BY id"
        ).map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
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
                let vertex_json: String = row
                    .get::<_, String>(10)
                    .unwrap_or_else(|_| "[0,0,0,0]".into());
                let emb_blob: Option<Vec<u8>> = row.get(11).unwrap_or(None);
                let importance: f64 = row.get::<_, f64>(12).unwrap_or(1.0);
                let enforced: bool = row.get::<_, i32>(13).unwrap_or(0) != 0;
                let rationale: Option<String> = row.get(14).unwrap_or(None);
                let access_count: u32 = row.get::<_, i32>(15).unwrap_or(0) as u32;
                let memory_type: Option<String> = row.get(16).unwrap_or(None);

                let labels: Vec<String> = serde_json::from_str(&labels_json).unwrap_or_else(|e| {
                    tracing::warn!(
                        "[Storage] labels parse error (may be plaintext migration): {}",
                        e
                    );
                    vec![]
                });
                let aliases: Vec<String> =
                    serde_json::from_str(&aliases_json).unwrap_or_else(|e| {
                        tracing::warn!(
                            "[Storage] aliases parse error (may be plaintext migration): {}",
                            e
                        );
                        vec![]
                    });
                let embedding = emb_blob
                    .as_deref()
                    .map_or(vec![], VectorLayer::blob_to_embedding);

                Ok((
                    id,
                    core_x,
                    core_y,
                    core_z,
                    decrypted_content,
                    content_hash,
                    labels,
                    mass,
                    timestamp,
                    aliases,
                    vertex_json,
                    embedding,
                    importance,
                    enforced,
                    rationale,
                    access_count,
                    memory_type,
                ))
            })
            .map_err(|e| e.to_string())?;

        let mut count = 0;
        for row in rows {
            let (
                id,
                cx,
                cy,
                cz,
                content,
                hash,
                labels,
                mass,
                ts,
                aliases,
                vertex_json,
                embedding,
                importance,
                enforced,
                rationale,
                access_count,
                memory_type,
            ) = row.map_err(|e: rusqlite::Error| e.to_string())?;
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
                    if current_ids == [0u64; 4] || current_ids.iter().all(|&v| v == current_ids[0])
                    {
                        if let Ok(loaded_verts) = serde_json::from_str::<[u64; 4]>(&vertex_json) {
                            if let Err(e) = space.update_vertex_ids(tetra_id, loaded_verts) {
                                tracing::debug!(
                                    "[Storage] vertex id restore failed for {}: {}",
                                    tetra_id,
                                    e
                                );
                            }
                        }
                    }
                }
            }
        }
        Ok(count)
    }

    fn load_relations(&self, kg: &KnowledgeGraph) -> Result<usize, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT source, target, rel_type, strength FROM relations")
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                let source: u64 = row.get(0)?;
                let target: u64 = row.get(1)?;
                let rel_type_str: String = row.get(2)?;
                let strength: f64 = row.get(3)?;
                let rel_type = Self::parse_rel_type(&rel_type_str);
                Ok((source, target, rel_type, strength))
            })
            .map_err(|e| e.to_string())?;

        let mut count = 0;
        for row in rows {
            let (source, target, rel_type, strength) =
                row.map_err(|e: rusqlite::Error| e.to_string())?;
            kg.add_relation(source, target, rel_type, strength);
            count += 1;
        }
        Ok(count)
    }

    fn load_concepts(&self, kg: &KnowledgeGraph) -> Result<usize, String> {
        let conn = self.pool.get().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT id, label, member_count FROM concepts")
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                let id: u64 = row.get(0)?;
                let label: String = row.get(1)?;
                let member_count: u64 = row.get(2)?;
                Ok(ConceptPrototype {
                    id,
                    centroid: vec![],
                    member_count,
                    label,
                    member_ids: vec![],
                })
            })
            .map_err(|e| e.to_string())?;

        let concepts: Vec<ConceptPrototype> = rows.filter_map(|r| r.ok()).collect();
        let count = concepts.len();
        kg.restore_concepts(concepts);
        Ok(count)
    }

    fn save_tetrahedrons_tx(
        &self,
        tx: &rusqlite::Transaction,
        space: &Space,
    ) -> Result<(), String> {
        let tetras = space.all_tetrahedrons();
        tx.execute("DELETE FROM tetrahedrons", [])
            .map_err(|e| e.to_string())?;

        for t in &tetras {
            let labels_json = self.encrypt_field(
                &serde_json::to_string(&t.data.labels).unwrap_or_else(|_| "[]".into()),
            )?;
            let aliases_json = self.encrypt_field(
                &serde_json::to_string(&t.data.aliases).unwrap_or_else(|_| "[]".into()),
            )?;
            let vertex_json =
                serde_json::to_string(&t.vertex_ids).unwrap_or_else(|_| "[0,0,0,0]".into());
            let emb_blob = if t.data.embedding.is_empty() {
                None
            } else {
                Some(VectorLayer::embedding_to_blob(&t.data.embedding))
            };
            let content_hash = t.data.content_hash as i64;
            let encrypted_content = self.encrypt_field(&t.data.content)?;

            tx.execute(
                "INSERT INTO tetrahedrons (id, core_x, core_y, core_z, content, content_hash, labels, mass, timestamp, aliases, vertex_ids, embedding, importance, enforced, rationale, access_count, memory_type)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
                params![
                    t.id, t.core.x, t.core.y, t.core.z,
                    encrypted_content, content_hash, labels_json,
                    t.mass, t.data.timestamp, aliases_json, vertex_json,
                    emb_blob, t.data.importance, t.data.enforced as i32,
                    t.data.rationale, t.data.access_count as i32, t.data.memory_type,
                ],
            ).map_err(|e| format!("insert tetra {}: {}", t.id, e))?;
        }
        Ok(())
    }

    fn save_relations_tx(
        &self,
        tx: &rusqlite::Transaction,
        kg: &KnowledgeGraph,
    ) -> Result<(), String> {
        let relations = kg.all_relations();
        tx.execute("DELETE FROM relations", [])
            .map_err(|e| e.to_string())?;

        for r in &relations {
            let rel_type_str = Self::rel_type_str(&r.relation_type);
            tx.execute(
                "INSERT OR IGNORE INTO relations (source, target, rel_type, strength) VALUES (?1, ?2, ?3, ?4)",
                params![r.source, r.target, rel_type_str, r.strength],
            ).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn save_concepts_tx(
        &self,
        tx: &rusqlite::Transaction,
        kg: &KnowledgeGraph,
    ) -> Result<(), String> {
        let concepts = kg.get_concepts();
        tx.execute("DELETE FROM concepts", [])
            .map_err(|e| e.to_string())?;

        for c in &concepts {
            let centroid_blob = if c.centroid.is_empty() {
                Vec::<u8>::new()
            } else {
                VectorLayer::embedding_to_blob(&c.centroid)
            };
            tx.execute(
                "INSERT INTO concepts (id, label, member_count, centroid) VALUES (?1, ?2, ?3, ?4)",
                params![c.id, c.label, c.member_count, centroid_blob],
            )
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
        }
    }

    fn parse_rel_type(s: &str) -> RelationType {
        match s {
            "SimilarTo" => RelationType::SimilarTo,
            "Contradicts" => RelationType::Contradicts,
            "Precedes" => RelationType::Precedes,
            "Contains" => RelationType::Contains,
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
                aliases: if id > 0 {
                    vec![format!("alias_{}", id)]
                } else {
                    vec![]
                },
                embedding: vec![],
                importance: 1.0,
                enforced: false,
                rationale: None,
                access_count: 0,
                memory_type: None,
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
        storage
            .update_aliases(10, &["alias_a".into(), "alias_b".into()])
            .unwrap();

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

    #[test]
    fn soft_delete_and_restore_roundtrip() {
        let dir = tmp_dir("soft_delete_restore");
        let storage = StorageManager::new(&dir).unwrap();
        let tetra = make_tetra(42, "trash me", 1.5);

        storage.upsert_tetra(&tetra).unwrap();
        let (deleted_at, expires_at) = storage.soft_delete_tetra(&tetra, 30).unwrap();
        assert!(deleted_at > 0);
        assert!(expires_at > deleted_at);
        assert_eq!(storage.tetra_count(), 0);

        let deleted = storage.list_deleted_memories().unwrap();
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].id, 42);
        assert_eq!(deleted[0].content, "trash me");

        let restored = storage.restore_deleted_tetra(42).unwrap().unwrap();
        assert_eq!(restored.id, 42);
        assert_eq!(restored.data.content, "trash me");
        assert_eq!(storage.tetra_count(), 1);
        assert!(storage.list_deleted_memories().unwrap().is_empty());
    }

    #[test]
    fn purge_expired_deleted_memories_clears_old_records() {
        let dir = tmp_dir("purge_deleted");
        let storage = StorageManager::new(&dir).unwrap();
        let tetra = make_tetra(55, "expired", 1.0);

        storage.upsert_tetra(&tetra).unwrap();
        let (_deleted_at, _expires_at) = storage.soft_delete_tetra(&tetra, 0).unwrap();
        let purged = storage.purge_expired_deleted_memories().unwrap();
        assert_eq!(purged, 1);
        assert!(storage.list_deleted_memories().unwrap().is_empty());
    }
}
