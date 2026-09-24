//! L1 图书馆 — 全局共享知识资产存储(独立于个人tetra空间)
//! 设计: docs/2026-09-05-L1-library-permissions-design.md
//! 特性: 独立SQLite(library.db) / 共享一份HNSW / 批量嵌入复用VectorLayer /
//!       content_hash全局幂等 / 集合ACL + 可见性(私|entitled|public)
//! 权限v1: 写=集合owner; 读=owner或ACL授权或public(entitled的套餐门控由handler层注入plan判断)

use parking_lot::{Mutex, RwLock};
use rusqlite::{params, Connection};
use std::collections::HashSet;
use std::sync::Arc;

use super::hnsw::HnswIndex;
use super::vector::{VectorLayer, EMBEDDING_DIM};

/// L1全局访问器: cloud.rs main()注入一次, engine层MCP工具经此检索
static GLOBAL_LIBRARY: std::sync::OnceLock<std::sync::Arc<LibraryStore>> =
    std::sync::OnceLock::new();

pub fn set_global_library(lib: std::sync::Arc<LibraryStore>) {
    let _ = GLOBAL_LIBRARY.set(lib);
}

pub fn global_library() -> Option<&'static std::sync::Arc<LibraryStore>> {
    GLOBAL_LIBRARY.get()
}

/// MCP层检索结果(简化, 不含ACL细节)
#[derive(Clone, Debug)]
pub struct LibraryHitPublic {
    pub content: String,
    pub title: String,
    pub chunk_no: i64,
    pub client_ref: String,
    pub score: f64,
}

pub struct LibraryStore {
    conn: Mutex<Connection>,
    hnsw: RwLock<HnswIndex>,
    vector: Option<Arc<VectorLayer>>,
}

#[derive(serde::Deserialize)]
pub struct LibraryChunkIn {
    pub chunk_no: i64,
    pub content: String,
}

#[derive(serde::Deserialize)]
pub struct LibraryItemIn {
    pub client_ref: Option<String>,
    pub title: String,
    #[serde(default)]
    pub source_meta: Option<String>,
    pub chunks: Vec<LibraryChunkIn>,
}

#[derive(serde::Serialize)]
pub struct LibraryHit {
    pub chunk_id: i64,
    pub chunk_no: i64,
    pub content: String,
    pub score: f64,
    pub item_id: i64,
    pub title: String,
    pub client_ref: Option<String>,
    pub source_meta: Option<String>,
    pub collection_id: i64,
}

impl LibraryStore {
    pub fn open(
        db_path: &std::path::Path,
        vector: Option<Arc<VectorLayer>>,
    ) -> Result<Self, String> {
        let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA busy_timeout=5000;
             CREATE TABLE IF NOT EXISTS library_collections (
                id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, owner TEXT NOT NULL,
                visibility TEXT NOT NULL DEFAULT 'private', plan_gate TEXT,
                created_at INTEGER);
             CREATE TABLE IF NOT EXISTS library_items (
                id INTEGER PRIMARY KEY, client_ref TEXT UNIQUE,
                collection_id INTEGER NOT NULL REFERENCES library_collections(id),
                title TEXT NOT NULL, source_meta TEXT,
                added_by TEXT NOT NULL, created_at INTEGER);
             CREATE TABLE IF NOT EXISTS library_chunks (
                id INTEGER PRIMARY KEY,
                item_id INTEGER NOT NULL REFERENCES library_items(id),
                chunk_no INTEGER NOT NULL, content TEXT NOT NULL,
                content_hash TEXT NOT NULL, embedding BLOB,
                UNIQUE(item_id, chunk_no));
             CREATE INDEX IF NOT EXISTS idx_lchunks_hash ON library_chunks(content_hash);
             CREATE INDEX IF NOT EXISTS idx_litems_coll ON library_items(collection_id);
             CREATE TABLE IF NOT EXISTS library_acls (
                collection_id INTEGER NOT NULL, principal TEXT NOT NULL, level TEXT NOT NULL,
                PRIMARY KEY(collection_id, principal));
             CREATE TABLE IF NOT EXISTS library_requests (
                id INTEGER PRIMARY KEY,
                user_id TEXT NOT NULL,
                title TEXT NOT NULL,
                url TEXT,
                note TEXT,
                status TEXT NOT NULL DEFAULT 'pending',
                created_at INTEGER,
                handled_at INTEGER,
                handler_note TEXT);",
        )
        .map_err(|e| e.to_string())?;

        let hnsw = HnswIndex::new(EMBEDDING_DIM, 16, 100);
        // 冷启动索引恢复: 空库为空; 大库后续由相4(索引持久化)接管
        let rows: Vec<(i64, Option<Vec<u8>>)> = {
            match conn
                .prepare("SELECT id, embedding FROM library_chunks WHERE embedding IS NOT NULL")
            {
                Ok(mut s) => s
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                    .map(|it| it.filter_map(|x| x.ok()).collect())
                    .map_err(|e| format!("library restore read: {}", e))?,
                Err(e) => return Err(format!("library restore prepare: {}", e)),
            }
        };
        let restored = {
            let mut n = 0usize;
            let mut idx = hnsw;
            for (id, blob) in rows {
                if let Some(b) = blob {
                    let emb = VectorLayer::blob_to_embedding(&b);
                    if emb.len() == EMBEDDING_DIM {
                        idx.insert(id as u64, emb);
                        n += 1;
                    }
                }
            }
            if n > 0 {
                tracing::info!("[Library] HNSW restored from {} chunks", n);
            }
            idx
        };
        Ok(Self {
            conn: Mutex::new(conn),
            hnsw: RwLock::new(restored),
            vector,
        })
    }

    pub fn create_collection(
        &self,
        owner: &str,
        name: &str,
        visibility: &str,
        plan_gate: Option<&str>,
    ) -> Result<i64, String> {
        if !matches!(visibility, "private" | "entitled" | "public") {
            return Err("visibility must be private|entitled|public".into());
        }
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO library_collections(name, owner, visibility, plan_gate, created_at) VALUES(?,?,?,?,?)",
            params![name, owner, visibility, plan_gate, chrono::Utc::now().timestamp()],
        )
        .map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn set_acl(&self, collection_id: i64, principal: &str, level: &str) -> Result<(), String> {
        if !matches!(level, "viewer" | "curator") {
            return Err("level must be viewer|curator".into());
        }
        let conn = self.conn.lock();
        conn.execute(
            "INSERT OR REPLACE INTO library_acls(collection_id, principal, level) VALUES(?,?,?)",
            params![collection_id, principal, level],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn collection_owner(&self, collection_id: i64) -> Option<String> {
        self.conn
            .lock()
            .query_row(
                "SELECT owner FROM library_collections WHERE id=?1",
                params![collection_id],
                |r| r.get(0),
            )
            .ok()
    }

    /// 批量灌装: owner-only; client_ref幂等(item级) + content_hash全局幂等(chunk级);
    /// 嵌入走 embed_batch 预热 + embed_passage 取向量(缓存命中) — 复用单条路径全部基建。
    pub fn ingest(
        &self,
        caller: &str,
        collection_id: i64,
        items: &[LibraryItemIn],
    ) -> Result<(usize, usize, usize), String> {
        let owner = self
            .collection_owner(collection_id)
            .ok_or("collection not found")?;
        if owner != caller {
            return Err("only collection owner can ingest".into());
        }
        let vl = self
            .vector
            .as_ref()
            .ok_or("embedding layer unavailable")?
            .clone();

        // 计划阶段: 确定item映射与待嵌chunk
        struct PlanChunk {
            item_id: i64,
            chunk_no: i64,
            content: String,
            hash: i64,
        }
        let mut plan: Vec<PlanChunk> = Vec::new();
        let mut items_new = 0usize;
        let mut skipped_plan = 0usize;
        {
            let conn = self.conn.lock();
            for it in items {
                let item_id = if let Some(r) = &it.client_ref {
                    match conn.query_row(
                        "SELECT id FROM library_items WHERE client_ref=?1",
                        params![r],
                        |x| x.get::<_, i64>(0),
                    ) {
                        Ok(id) => id,
                        Err(_) => {
                            conn.execute(
                                "INSERT INTO library_items(client_ref, collection_id, title, source_meta, added_by, created_at) VALUES(?,?,?,?,?,?)",
                                params![r, collection_id, it.title, it.source_meta, caller, chrono::Utc::now().timestamp()],
                            )
                            .map_err(|e| e.to_string())?;
                            items_new += 1;
                            conn.last_insert_rowid()
                        }
                    }
                } else {
                    conn.execute(
                        "INSERT INTO library_items(client_ref, collection_id, title, source_meta, added_by, created_at) VALUES(?,?,?,?,?,?)",
                        params![rusqlite::types::Null, collection_id, it.title, it.source_meta, caller, chrono::Utc::now().timestamp()],
                    )
                    .map_err(|e| e.to_string())?;
                    items_new += 1;
                    conn.last_insert_rowid()
                };
                for ch in &it.chunks {
                    if ch.content.trim().is_empty() {
                        continue;
                    }
                    let h = super::search_engine::hash_content(&ch.content) as i64;
                    let exists: bool = conn
                        .query_row(
                            "SELECT 1 FROM library_chunks WHERE content_hash=?1",
                            params![h],
                            |_| Ok(true),
                        )
                        .unwrap_or(false);
                    let exists_at_slot: bool = conn
                        .query_row(
                            "SELECT 1 FROM library_chunks WHERE item_id=?1 AND chunk_no=?2",
                            params![item_id, ch.chunk_no],
                            |_| Ok(true),
                        )
                        .unwrap_or(false);
                    if exists || exists_at_slot {
                        skipped_plan += 1;
                    } else {
                        plan.push(PlanChunk {
                            item_id,
                            chunk_no: ch.chunk_no,
                            content: ch.content.clone(),
                            hash: h,
                        });
                    }
                }
            }
        }
        if plan.is_empty() {
            return Ok((items_new, 0, skipped_plan));
        }

        // L1fix: 批量嵌入直取向量 — 不再走"预热+逐条缓存取回"
        let texts: Vec<String> = plan.iter().map(|p| p.content.clone()).collect();
        let vecs = vl.embed_batch_vecs(&texts)?;
        let mut embedded: Vec<(usize, Vec<f64>)> = Vec::with_capacity(plan.len());
        for (i, e) in vecs.into_iter().enumerate() {
            if e.len() == EMBEDDING_DIM {
                embedded.push((i, e));
            }
        }
        // 9/12修复: 批量路径的空向量(长文本批超60s/内存门延迟)退回单条embed_passage(30s内,
        // 慢但完整) — 此前"一空拒整批"在长文本夜=重试死锁零进展; 单条也空才是真死, 走下方契约
        if embedded.len() < plan.len() {
            let mut got = vec![false; plan.len()];
            for (i, _) in &embedded {
                got[*i] = true;
            }
            for i in 0..plan.len() {
                if !got[i] {
                    let e = vl.embed_passage(&texts[i]).unwrap_or_default();
                    if e.len() == EMBEDDING_DIM {
                        embedded.push((i, e));
                    }
                }
            }
            embedded.sort_by_key(|(i, _)| *i);
        }
        // L1迁移教训(00:14事故): ONNX熔断窗口会产生空向量 — 空嵌入=批次整体拒绝,
        // 不写入不记账(此前空向量被误计为deduped, 1231篇假完成)。幂等设计让整批重试零成本。
        if embedded.len() < plan.len() {
            return Err(format!(
                "embedding degraded: {}/{} chunks got empty vectors (ONNX disabled/busy) — nothing written, retry the batch later",
                plan.len() - embedded.len(), plan.len()
            ));
        }

        // 单事务写入 + HNSW
        let inserted = {
            let mut conn = self.conn.lock();
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            let mut n = 0usize;
            for (i, emb) in &embedded {
                let p = &plan[*i];
                tx.execute(
                    "INSERT OR REPLACE INTO library_chunks(item_id, chunk_no, content, content_hash, embedding) VALUES(?,?,?,?,?)",
                    params![p.item_id, p.chunk_no, p.content, p.hash, VectorLayer::embedding_to_blob(emb)],
                )
                .map_err(|e| e.to_string())?;
                n += 1;
            }
            tx.commit().map_err(|e| e.to_string())?;
            n
        };
        {
            let mut idx = self.hnsw.write();
            for (i, emb) in &embedded {
                let p = &plan[*i];
                let cid: Option<i64> = self
                    .conn
                    .lock()
                    .query_row(
                        "SELECT id FROM library_chunks WHERE item_id=?1 AND chunk_no=?2",
                        params![p.item_id, p.chunk_no],
                        |r| r.get(0),
                    )
                    .ok();
                if let Some(id) = cid {
                    idx.insert(id as u64, emb.clone());
                }
            }
        }
        // deduped=计划阶段已存在被跳过的数(真实去重); inserted必然==plan.len()(空嵌入已整批拒绝)
        Ok((items_new, inserted, skipped_plan))
    }

    /// 检索: 向量knn(多取)→权限过滤→topK, 带provenance
    pub fn search(
        &self,
        user_id: &str,
        plan_level: &str,
        query: &str,
        k: usize,
    ) -> Result<Vec<LibraryHit>, String> {
        let vl = self.vector.as_ref().ok_or("embedding layer unavailable")?;
        let q = vl.embed_passage(query).unwrap_or_default();
        if q.len() != EMBEDDING_DIM {
            return self.search_keyword(user_id, plan_level, query, k);
        }
        let knn = { self.hnsw.read().search_knn(&q, (k * 3).max(20), 64) };

        let conn = self.conn.lock();
        // 权限集合: owner / ACL / public / entitled且套餐满足
        let mut allowed: HashSet<i64> = HashSet::new();
        let mut stmt = conn
            .prepare("SELECT id, visibility, plan_gate FROM library_collections")
            .map_err(|e| e.to_string())?;
        let cols: Vec<(i64, String, Option<String>)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map(|it| it.filter_map(|x| x.ok()).collect())
            .unwrap_or_default();
        drop(stmt);
        let mut acl_stmt = conn
            .prepare("SELECT collection_id FROM library_acls WHERE principal=?1")
            .map_err(|e| e.to_string())?;
        let acl_cols: Vec<i64> = acl_stmt
            .query_map(params![user_id], |r| r.get(0))
            .map(|it| it.filter_map(|x| x.ok()).collect())
            .unwrap_or_default();
        drop(acl_stmt);
        for c in &acl_cols {
            allowed.insert(*c);
        }
        for (id, vis, gate) in &cols {
            match vis.as_str() {
                "public" => {
                    allowed.insert(*id);
                }
                "entitled" => {
                    let g = gate.as_deref().unwrap_or("");
                    if g.is_empty() || plan_level == "enterprise" || plan_level == g {
                        allowed.insert(*id);
                    }
                }
                _ => {}
            }
        }
        let owner_cols: Vec<i64> = {
            let mut s = conn
                .prepare("SELECT id FROM library_collections WHERE owner=?1")
                .map_err(|e| e.to_string())?;
            s.query_map(params![user_id], |r| r.get(0))
                .map(|it| it.filter_map(|x| x.ok()).collect())
                .unwrap_or_default()
        };
        for c in &owner_cols {
            allowed.insert(*c);
        }

        let mut out: Vec<LibraryHit> = Vec::new();
        for (cid_u, score) in knn {
            if out.len() >= k {
                break;
            }
            let cid = cid_u as i64;
            let hit = conn.query_row(
                "SELECT c.id, c.chunk_no, c.content, c.item_id, i.title, i.client_ref, i.source_meta, i.collection_id
                 FROM library_chunks c JOIN library_items i ON c.item_id=i.id WHERE c.id=?1",
                params![cid],
                |r| {
                    Ok(LibraryHit {
                        chunk_id: r.get(0)?, chunk_no: r.get(1)?, content: r.get(2)?,
                        item_id: r.get(3)?, title: r.get(4)?, client_ref: r.get(5)?,
                        source_meta: r.get(6)?, collection_id: r.get(7)?,
                        score: (score * 10000.0).round() / 10000.0,
                    })
                },
            );
            if let Ok(h) = hit {
                if allowed.contains(&h.collection_id) {
                    out.push(h);
                }
            }
        }
        Ok(out)
    }

    /// L1权限v2: 收集请求 — 全体用户可提, owner处理(accept/reject)
    pub fn create_request(
        &self,
        user_id: &str,
        title: &str,
        url: Option<&str>,
        note: Option<&str>,
    ) -> Result<i64, String> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO library_requests(user_id, title, url, note, status, created_at) VALUES(?,?,?,?,'pending',?)",
            params![user_id, title, url, note, chrono::Utc::now().timestamp()],
        ).map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_requests(&self, status: Option<&str>) -> Vec<serde_json::Value> {
        let conn = self.conn.lock();
        let sql = match status {
            Some(_s) => "SELECT id, user_id, title, url, note, status, created_at, handled_at, handler_note FROM library_requests WHERE status=?1 ORDER BY id DESC LIMIT 100",
            None => "SELECT id, user_id, title, url, note, status, created_at, handled_at, handler_note FROM library_requests ORDER BY id DESC LIMIT 100",
        };
        let mut stmt = match conn.prepare(sql) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        let map_row = |r: &rusqlite::Row| -> rusqlite::Result<serde_json::Value> {
            Ok(serde_json::json!({
                "id": r.get::<_, i64>(0)?, "user_id": r.get::<_, String>(1)?,
                "title": r.get::<_, String>(2)?, "url": r.get::<_, Option<String>>(3)?,
                "note": r.get::<_, Option<String>>(4)?, "status": r.get::<_, String>(5)?,
                "created_at": r.get::<_, i64>(6)?, "handled_at": r.get::<_, Option<i64>>(7)?,
                "handler_note": r.get::<_, Option<String>>(8)?,
            }))
        };
        match &status {
            Some(s) => stmt
                .query_map(params![s], map_row)
                .map(|it| it.filter_map(|x| x.ok()).collect())
                .unwrap_or_default(),
            None => stmt
                .query_map([], map_row)
                .map(|it| it.filter_map(|x| x.ok()).collect())
                .unwrap_or_default(),
        }
    }

    /// 处理请求: only_owner校验在handler层; accept时可选直接灌装一条(item入库)
    pub fn handle_request(
        &self,
        request_id: i64,
        action: &str,
        handler_note: Option<&str>,
    ) -> Result<(), String> {
        if !matches!(action, "accepted" | "rejected") {
            return Err("action must be accepted|rejected".into());
        }
        let conn = self.conn.lock();
        let n = conn.execute(
            "UPDATE library_requests SET status=?1, handled_at=?2, handler_note=?3 WHERE id=?4 AND status='pending'",
            params![action, chrono::Utc::now().timestamp(), handler_note, request_id],
        ).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("request not found or already handled".into());
        }
        Ok(())
    }

    pub fn pending_request_count(&self) -> i64 {
        self.conn
            .lock()
            .query_row(
                "SELECT COUNT(*) FROM library_requests WHERE status='pending'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0)
    }

    pub fn set_collection_visibility(
        &self,
        collection_id: i64,
        visibility: &str,
    ) -> Result<(), String> {
        if !matches!(visibility, "private" | "entitled" | "public") {
            return Err("visibility must be private|entitled|public".into());
        }
        let conn = self.conn.lock();
        let n = conn
            .execute(
                "UPDATE library_collections SET visibility=?1 WHERE id=?2",
                params![visibility, collection_id],
            )
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("collection not found".into());
        }
        Ok(())
    }

    /// L1fix: 词面回退检索 — ONNX降级时不瘫
    fn search_keyword(
        &self,
        user_id: &str,
        plan_level: &str,
        query: &str,
        k: usize,
    ) -> Result<Vec<LibraryHit>, String> {
        let conn = self.conn.lock();
        let allowed = Self::allowed_collections(&conn, user_id, plan_level);
        let words: Vec<&str> = query
            .split_whitespace()
            .filter(|w| w.chars().count() >= 2)
            .collect();
        if words.is_empty() {
            return Ok(Vec::new());
        }
        let like = format!("%{}%", words[0]);
        let mut stmt = conn.prepare(
            "SELECT c.id, c.chunk_no, c.content, c.item_id, i.title, i.client_ref, i.source_meta, i.collection_id
             FROM library_chunks c JOIN library_items i ON c.item_id=i.id
             WHERE c.content LIKE ?1 LIMIT ?2",
        ).map_err(|e| e.to_string())?;
        let rows: Vec<LibraryHit> = stmt
            .query_map(params![like, k * 3], |r| {
                Ok(LibraryHit {
                    chunk_id: r.get(0)?,
                    chunk_no: r.get(1)?,
                    content: r.get(2)?,
                    item_id: r.get(3)?,
                    title: r.get(4)?,
                    client_ref: r.get(5)?,
                    source_meta: r.get(6)?,
                    collection_id: r.get(7)?,
                    score: 0.0,
                })
            })
            .map(|it| it.filter_map(|x| x.ok()).collect())
            .unwrap_or_default();
        drop(stmt);
        Ok(rows
            .into_iter()
            .filter(|h| allowed.contains(&h.collection_id))
            .take(k)
            .collect())
    }

    fn allowed_collections(
        conn: &Connection,
        user_id: &str,
        plan_level: &str,
    ) -> std::collections::HashSet<i64> {
        use std::collections::HashSet;
        let mut allowed: HashSet<i64> = HashSet::new();
        let cols: Vec<(i64, String, Option<String>)> = conn
            .prepare("SELECT id, visibility, plan_gate FROM library_collections")
            .and_then(|mut s| {
                s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                    .map(|it| it.filter_map(|x| x.ok()).collect())
            })
            .unwrap_or_default();
        for (id, vis, gate) in &cols {
            match vis.as_str() {
                "public" => {
                    allowed.insert(*id);
                }
                "entitled" => {
                    let g = gate.as_deref().unwrap_or("");
                    if g.is_empty() || plan_level == "enterprise" || plan_level == g {
                        allowed.insert(*id);
                    }
                }
                _ => {}
            }
        }
        if let Ok(mut s) = conn.prepare("SELECT collection_id FROM library_acls WHERE principal=?1")
        {
            if let Ok(it) = s.query_map(params![user_id], |r| r.get::<_, i64>(0)) {
                for id in it.filter_map(|x| x.ok()) {
                    allowed.insert(id);
                }
            }
        }
        if let Ok(mut s) = conn.prepare("SELECT id FROM library_collections WHERE owner=?1") {
            if let Ok(it) = s.query_map(params![user_id], |r| r.get::<_, i64>(0)) {
                for id in it.filter_map(|x| x.ok()) {
                    allowed.insert(id);
                }
            }
        }
        allowed
    }

    /// L1 MCP入口: 全局公共检索(仅public集合, 不过滤个人ACL — MCP层简化)
    pub fn search_public(&self, query: &str, k: usize) -> Result<Vec<LibraryHitPublic>, String> {
        let vl = self.vector.as_ref().ok_or("embedding layer unavailable")?;
        let q = vl.embed_passage(query)?;
        if q.len() != EMBEDDING_DIM {
            return Ok(Vec::new());
        }
        let knn = { self.hnsw.read().search_knn(&q, k, 64) };
        let conn = self.conn.lock();
        let mut out: Vec<LibraryHitPublic> = Vec::new();
        for (cid_u, score) in knn {
            if out.len() >= k {
                break;
            }
            let cid = cid_u as i64;
            // 只返回public集合的chunk
            let hit = conn.query_row(
                "SELECT c.content, c.chunk_no, i.title, COALESCE(i.client_ref,''), col.visibility
                 FROM library_chunks c
                 JOIN library_items i ON c.item_id=i.id
                 JOIN library_collections col ON i.collection_id=col.id
                 WHERE c.id=?1 AND col.visibility='public'",
                params![cid],
                |r| {
                    Ok(LibraryHitPublic {
                        content: r.get::<_, String>(0)?,
                        chunk_no: r.get(1)?,
                        title: r.get(2)?,
                        client_ref: r.get(3)?,
                        score: (score * 10000.0).round() / 10000.0,
                    })
                },
            );
            if let Ok(h) = hit {
                out.push(h);
            }
        }
        Ok(out)
    }

    pub fn chunk_count(&self) -> i64 {
        self.conn
            .lock()
            .query_row("SELECT COUNT(*) FROM library_chunks", [], |r| r.get(0))
            .unwrap_or(0)
    }
}
