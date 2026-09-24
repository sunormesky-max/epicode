use crate::domain::space::Space;
use crate::domain::tetra::TetraId;
use crate::engine::gateway::GatewayCenter;
use crate::engine::knowledge::KnowledgeGraph;
use crate::engine::storage::StorageManager;

pub struct JanitorCtx<'a> {
    pub space: &'a Space,
    pub storage: &'a StorageManager,
    pub knowledge: &'a KnowledgeGraph,
    pub gateway: &'a GatewayCenter,
}

pub fn auto_save(ctx: &JanitorCtx) {
    if ctx.knowledge.is_dirty() {
        // F4: KG增量优先 — 曾每次dirty整表DELETE+重插13.6万行(日均千万行写)
        let (ups, dels) = ctx.knowledge.drain_pending_relations();
        let delta_total = ups.len() + dels.len();
        if delta_total > 0 && delta_total <= 50_000 {
            match ctx.storage.save_relations_delta(&ups, &dels) {
                Ok((u, d)) => {
                    tracing::debug!("[Janitor] kg delta saved: +{} -{}", u, d);
                    ctx.knowledge.clear_dirty();
                }
                Err(e) => tracing::warn!("[Janitor] kg delta failed (will full-save): {}", e),
            }
        }
        if ctx.knowledge.is_dirty() {
            // 回退/兜底: 无增量信息或超量 → 全量
            if let Err(e) = ctx.storage.save_kg_only(ctx.knowledge) {
                tracing::warn!("[Janitor] auto-save kg failed: {}", e);
            } else {
                ctx.knowledge.clear_dirty();
            }
        }
    }
    let dirty = ctx.gateway.drain_dirty();
    if !dirty.is_empty() {
        match ctx.storage.batch_upsert(ctx.space, &dirty) {
            Ok(n) => tracing::debug!("[Janitor] auto-save {} dirty tetras", n),
            Err(e) => tracing::warn!("[Janitor] auto-save batch failed: {}", e),
        }
    }
}

pub fn mark_dirty_persist(ctx: &JanitorCtx, id: TetraId) {
    if let Some(tetra) = ctx.space.get_tetrahedron(id) {
        if let Err(e) = ctx.storage.upsert_tetra(&tetra) {
            tracing::warn!("[Janitor] persist_tetra {} failed: {}", id, e);
        }
    }
    if ctx.knowledge.is_dirty() {
        if let Err(e) = ctx.storage.save_kg_only(ctx.knowledge) {
            tracing::warn!("[Janitor] persist kg failed: {}", e);
        } else {
            ctx.knowledge.clear_dirty();
        }
    }
    ctx.gateway.mark_dirty(id);
}
