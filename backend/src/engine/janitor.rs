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

fn persist_knowledge_graph(storage: &StorageManager, knowledge: &KnowledgeGraph) {
    let Some((revision, full_save_required)) = knowledge.persistence_snapshot() else {
        return;
    };

    if full_save_required {
        knowledge.drain_pending_relations();
        match storage.save_kg_only(knowledge) {
            Ok(()) => {
                knowledge.mark_saved_if_unchanged(revision);
            }
            Err(e) => tracing::warn!("[Janitor] full kg save failed: {}", e),
        }
        return;
    }

    let (ups, dels) = knowledge.drain_pending_relations();
    let delta_total = ups.len() + dels.len();
    if delta_total > 0 && delta_total <= 50_000 {
        match storage.save_relations_delta(&ups, &dels, knowledge) {
            Ok((u, d)) => {
                tracing::debug!("[Janitor] kg delta saved: +{} -{}", u, d);
                if knowledge.mark_saved_if_unchanged(revision) {
                    return;
                }
            }
            Err(e) => tracing::warn!("[Janitor] kg delta failed (will full-save): {}", e),
        }
    }

    if let Some((fallback_revision, _)) = knowledge.persistence_snapshot() {
        knowledge.drain_pending_relations();
        match storage.save_kg_only(knowledge) {
            Ok(()) => {
                knowledge.mark_saved_if_unchanged(fallback_revision);
            }
            Err(e) => tracing::warn!("[Janitor] auto-save kg failed: {}", e),
        }
    }
}

pub fn auto_save(ctx: &JanitorCtx) {
    persist_knowledge_graph(ctx.storage, ctx.knowledge);

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
    persist_knowledge_graph(ctx.storage, ctx.knowledge);
    ctx.gateway.mark_dirty(id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::knowledge::RelationType;
    use rusqlite::Connection;
    use std::path::{Path, PathBuf};

    fn temp_dir(name: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "epicode_janitor_{}_{}_{}",
            name,
            std::process::id(),
            nonce
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn relation_strength(data_dir: &Path, source: i64, target: i64) -> f64 {
        Connection::open(data_dir.join("tetramem.db"))
            .unwrap()
            .query_row(
                "SELECT strength FROM relations WHERE source=?1 AND target=?2",
                rusqlite::params![source, target],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn delta_save_persists_reinforcement_and_concepts() {
        let data_dir = temp_dir("kg_delta");
        let storage = StorageManager::new(&data_dir).unwrap();
        let knowledge = KnowledgeGraph::new();
        knowledge.add_relation(1, 2, RelationType::SimilarTo, 0.5);
        persist_knowledge_graph(&storage, &knowledge);

        knowledge.add_relation(3, 4, RelationType::Related, 0.4);
        knowledge.reinforce_edges(&[(1, 2)]);
        knowledge.update_concepts(&[(1, vec!["rust".to_string()]), (2, vec!["rust".to_string()])]);
        persist_knowledge_graph(&storage, &knowledge);

        let connection = Connection::open(data_dir.join("tetramem.db")).unwrap();
        let strength: f64 = connection
            .query_row(
                "SELECT strength FROM relations WHERE source=1 AND target=2",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let member_count: i64 = connection
            .query_row(
                "SELECT member_count FROM concepts WHERE label='rust'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(strength > 0.5);
        assert_eq!(member_count, 2);

        drop(connection);
        drop(storage);
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[test]
    fn decay_forces_persistence_when_no_relation_is_removed() {
        let data_dir = temp_dir("kg_decay");
        let storage = StorageManager::new(&data_dir).unwrap();
        let knowledge = KnowledgeGraph::new();
        knowledge.add_relation(10, 11, RelationType::SimilarTo, 0.8);
        persist_knowledge_graph(&storage, &knowledge);
        let before = relation_strength(&data_dir, 10, 11);

        assert_eq!(knowledge.decay_relations(), 0);
        assert!(knowledge
            .persistence_snapshot()
            .is_some_and(|(_, full_save)| full_save));
        persist_knowledge_graph(&storage, &knowledge);

        let after = relation_strength(&data_dir, 10, 11);
        assert!((after - before * 0.9995).abs() < 1e-12);
        assert!(!knowledge.is_dirty());

        drop(storage);
        std::fs::remove_dir_all(data_dir).unwrap();
    }
}
