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
            Err(e) => {
                // A failed transaction writes none of this batch. Reinsert through the
                // set so marks added after drain_dirty remain queued too.
                for id in dirty {
                    ctx.gateway.mark_dirty(id);
                }
                tracing::warn!("[Janitor] auto-save batch failed (will retry): {}", e);
            }
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
    use crate::domain::tetra::{MemoryPayload, Tetrahedron};
    use crate::domain::vertex::Point3;
    use crate::engine::adaptive::AdaptiveParams;
    use crate::engine::auto_pipeline::{self, AutoPipelineCtx};
    use crate::engine::bus::EventBus;
    use crate::engine::classifier::CategoryClassifier;
    use crate::engine::cognitive::CognitiveEngine;
    use crate::engine::embedding::EmbeddingService;
    use crate::engine::energy::EnergyCenter;
    use crate::engine::knowledge::RelationType;
    use rusqlite::Connection;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

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

    struct PulseFixture {
        data_dir: PathBuf,
        space: Arc<Space>,
        knowledge: Arc<KnowledgeGraph>,
        gateway: GatewayCenter,
        energy: Arc<EnergyCenter>,
        storage: StorageManager,
        ids: [TetraId; 2],
    }

    impl PulseFixture {
        fn janitor_ctx(&self) -> JanitorCtx<'_> {
            JanitorCtx {
                space: &self.space,
                storage: &self.storage,
                knowledge: &self.knowledge,
                gateway: &self.gateway,
            }
        }
    }

    fn pulse_fixture(name: &str) -> PulseFixture {
        let data_dir = temp_dir(name);
        let space = Arc::new(Space::new());
        let knowledge = Arc::new(KnowledgeGraph::new());
        let bus = EventBus::new(8);
        let tx = bus.sender();
        let energy = Arc::new(EnergyCenter::new(100.0, 0.0, tx.clone(), bus.subscribe()));
        let gateway = GatewayCenter::new(
            Arc::clone(&space),
            Arc::clone(&energy),
            Arc::new(CognitiveEngine::new("", "")),
            Arc::new(CategoryClassifier::new("", "")),
            tx,
            bus.subscribe(),
            Arc::clone(&knowledge),
            Arc::new(EmbeddingService::from_env()),
            None,
        );
        let storage = StorageManager::new(&data_dir).unwrap();
        let mut ids = Vec::new();
        for (i, x) in [0.0, 1.0].into_iter().enumerate() {
            let core = Point3::new(x, 0.0, 0.0);
            let tetra = Tetrahedron {
                id: 0,
                vertex_ids: [0; 4],
                core,
                data: MemoryPayload {
                    content: format!("pulse memory {i}"),
                    content_hash: i as u64 + 1,
                    labels: vec!["pulse-test".into()],
                    embedding: vec![0.5; 4],
                    ..Default::default()
                },
                mass: 1.0,
            };
            ids.push(
                space
                    .add_tetrahedron(&tetra, &Tetrahedron::compute_vertices(core))
                    .unwrap(),
            );
        }
        assert!(!space.find_shared_vertices(ids[0], ids[1]).is_empty());
        storage.batch_upsert(&space, &ids).unwrap();
        PulseFixture {
            data_dir,
            space,
            knowledge,
            gateway,
            energy,
            storage,
            ids: [ids[0], ids[1]],
        }
    }

    fn restart_masses(fixture: PulseFixture) -> [f64; 2] {
        let data_dir = fixture.data_dir.clone();
        let ids = fixture.ids;
        drop(fixture);
        let storage = StorageManager::new(&data_dir).unwrap();
        let space = Space::new();
        let knowledge = KnowledgeGraph::new();
        let report = storage.load_all(&space, &knowledge);
        assert!(
            report.space_ok,
            "restart load failed: {:?}",
            report.space_error
        );
        let masses = ids.map(|id| space.get_tetrahedron(id).unwrap().mass);
        drop(storage);
        std::fs::remove_dir_all(data_dir).unwrap();
        masses
    }

    #[test]
    fn manual_pulse_retries_failed_batch_and_survives_restart() {
        let fixture = pulse_fixture("manual_pulse_retry");
        let changed = fixture.gateway.pulse(fixture.ids[0], 2).unwrap();
        assert!(changed.dirty_ids.contains(&fixture.ids[1]));
        let pulsed_mass = fixture.space.get_tetrahedron(fixture.ids[1]).unwrap().mass;
        assert!(pulsed_mass > 1.0);

        let connection = Connection::open(fixture.data_dir.join("tetramem.db")).unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER fail_pulse_save BEFORE INSERT ON tetrahedrons \
                 BEGIN SELECT RAISE(ABORT, 'injected write failure'); END;",
            )
            .unwrap();
        auto_save(&fixture.janitor_ctx());
        let mass_before_retry: f64 = connection
            .query_row(
                "SELECT mass FROM tetrahedrons WHERE id=?1",
                [fixture.ids[1] as i64],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(mass_before_retry, 1.0);

        connection
            .execute_batch("DROP TRIGGER fail_pulse_save")
            .unwrap();
        // A new mark after the failed drain must coexist with the requeued ID.
        fixture.space.update_mass(fixture.ids[0], 0.03).unwrap();
        fixture.gateway.mark_dirty(fixture.ids[0]);
        auto_save(&fixture.janitor_ctx());
        drop(connection);

        let masses = restart_masses(fixture);
        assert!(masses[0] > 1.0);
        assert!((masses[1] - pulsed_mass).abs() < 1e-12);
    }

    #[test]
    fn automatic_pulse_mass_survives_restart() {
        let fixture = pulse_fixture("automatic_pulse");
        let adaptive = AdaptiveParams::new();
        let ctx = AutoPipelineCtx {
            tick: 0,
            space: &fixture.space,
            energy: &fixture.energy,
            knowledge: &fixture.knowledge,
            gateway: &fixture.gateway,
            storage: &fixture.storage,
            emotion_pleasure: 0.0,
            emotion_arousal: 0.0,
            adaptive: &adaptive,
        };
        let tetras = fixture.space.all_tetras_meta();
        let clusters = fixture.space.find_clusters();
        assert_eq!(clusters.len(), 1);
        assert_eq!(
            auto_pipeline::auto_pulse(&ctx, &tetras, &clusters, &Default::default()),
            1
        );
        let in_memory = fixture
            .ids
            .map(|id| fixture.space.get_tetrahedron(id).unwrap().mass);
        assert!(in_memory.iter().any(|mass| *mass > 1.0));

        auto_save(&fixture.janitor_ctx());
        let persisted = restart_masses(fixture);
        for (expected, actual) in in_memory.into_iter().zip(persisted) {
            assert!((expected - actual).abs() < 1e-12);
        }
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
