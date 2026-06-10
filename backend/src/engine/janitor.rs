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
        if let Err(e) = ctx.storage.save_kg_only(ctx.knowledge) {
            tracing::warn!("[Janitor] auto-save kg failed: {}", e);
        } else {
            ctx.knowledge.clear_dirty();
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
    ctx.gateway.mark_dirty(id);
}
