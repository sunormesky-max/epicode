//! Proving harness: main-sim dream clamp(0.1) vs constitutional live floor 0.3.

use crate::domain::space::Space;
use crate::domain::tetra::{MemoryPayload, TetraId, Tetrahedron};
use crate::domain::vertex::Point3;
use crate::engine::bus::EventBus;
use crate::engine::cognitive::CognitiveEngine;
use crate::engine::dream::DreamEngine;
use crate::engine::embedding::EmbeddingService;
use crate::engine::energy::EnergyCenter;
use crate::engine::gateway::GatewayCenter;
use crate::engine::governor::{clamp_live_importance, FORGET_IMPORTANCE, IMPORTANCE_FLOOR};
use crate::engine::knowledge::KnowledgeGraph;
use crate::engine::scheduler::SchedulerCenter;
use crate::engine::security::SecurityGuard;
use crate::engine::storage::StorageManager;
use crate::engine::CategoryClassifier;
use parking_lot::Mutex;
use std::sync::Arc;

static LOCK: Mutex<()> = Mutex::new(());

#[derive(Default, Debug)]
struct FloorStats {
    n_live: usize,
    min_live: f64,
    below_floor_count: usize,
    below_floor_frac: f64,
    tombstone_at_forget: usize,
    live_illegally_near_tombstone: usize,
}

#[allow(clippy::too_many_arguments)]
fn add_mem(
    space: &Space,
    core: Point3,
    content: &str,
    labels: Vec<String>,
    importance: f64,
    access_count: u32,
    timestamp: i64,
    valid_to: Option<i64>,
) -> TetraId {
    let positions = Tetrahedron::compute_vertices(core);
    let data = MemoryPayload {
        content: content.to_string(),
        content_hash: content.len() as u64,
        labels,
        timestamp,
        importance,
        access_count,
        valid_to,
        ..Default::default()
    };
    let tetra = Tetrahedron {
        id: 0,
        vertex_ids: [0; 4],
        core,
        data,
        mass: 1.0,
    };
    space.add_tetrahedron(&tetra, &positions).unwrap()
}

fn seed_floor_corpus(space: &Space) {
    let now = chrono::Utc::now().timestamp();
    for i in 0..40 {
        let start_imp = 0.32 + (i as f64 % 5.0) * 0.02;
        add_mem(
            space,
            Point3::new(i as f64, 0.0, 0.0),
            &format!("old unaccessed note floor-probe-{i} miscellaneous remark"),
            vec!["floor-probe".into()],
            start_imp,
            0,
            now - 86400 * 45,
            None,
        );
    }
    for i in 0..10 {
        add_mem(
            space,
            Point3::new(100.0 + i as f64, 1.0, 0.0),
            &format!("hot frequently retrieved floor-hot-{i} recurring topic"),
            vec!["floor-hot".into()],
            1.0,
            10,
            now - 86400 * 10,
            None,
        );
    }
    for i in 0..5 {
        add_mem(
            space,
            Point3::new(200.0 + i as f64, 2.0, 0.0),
            &format!("forgotten tombstone floor-tomb-{i}"),
            vec!["floor-tomb".into()],
            FORGET_IMPORTANCE,
            0,
            now - 86400 * 90,
            Some(now - 86400),
        );
    }
}

fn collect_live_floor_stats(space: &Space) -> FloorStats {
    let mut stats = FloorStats::default();
    let mut min_live = f64::MAX;
    for t in space.all_tetrahedrons() {
        if t.data.valid_to.is_some() {
            if t.data.importance <= FORGET_IMPORTANCE + 1e-6 {
                stats.tombstone_at_forget += 1;
            }
            continue;
        }
        stats.n_live += 1;
        min_live = min_live.min(t.data.importance);
        if t.data.importance + 1e-9 < IMPORTANCE_FLOOR {
            stats.below_floor_count += 1;
        }
        if t.data.importance < 0.15 {
            stats.live_illegally_near_tombstone += 1;
        }
    }
    stats.min_live = if stats.n_live == 0 { 0.0 } else { min_live };
    stats.below_floor_frac = if stats.n_live == 0 {
        0.0
    } else {
        stats.below_floor_count as f64 / stats.n_live as f64
    };
    stats
}

/// Main @ d4c4f77 dream recompute used clamp(0.1, 3.0).
fn main_sim_recompute(space: &Space, rounds: usize) -> FloorStats {
    for _ in 0..rounds {
        let tetras = space.all_tetrahedrons();
        let now_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as f64;
        for t in &tetras {
            if t.data.enforced || t.data.valid_to.is_some() {
                continue;
            }
            let access = t.data.access_count as f64;
            let age_days = (now_ts - t.data.timestamp as f64) / 86400.0;
            let mut new_importance = t.data.importance;
            if access > 5.0 {
                new_importance += 0.1;
            }
            if age_days > 7.0 && access < 1.0 {
                new_importance -= 0.05;
            }
            if age_days > 30.0 && access < 1.0 {
                new_importance -= 0.1;
            }
            new_importance = new_importance.clamp(0.1, 3.0);
            if (new_importance - t.data.importance).abs() > 0.01 {
                if let Some(mut tetra) = space.get_tetrahedron(t.id) {
                    tetra.data.importance = new_importance;
                    let _ = space.update_payload(t.id, tetra.data);
                }
            }
        }
    }
    collect_live_floor_stats(space)
}

fn spike_recompute(space: &Space, rounds: usize) -> FloorStats {
    let counts = std::collections::HashMap::new();
    for _ in 0..rounds {
        let _ = DreamEngine::recompute_importance(space, &counts);
    }
    collect_live_floor_stats(space)
}

#[test]
fn floor_main_sim_vs_unified_dream_distribution() {
    let space_main = Space::new();
    seed_floor_corpus(&space_main);
    let main = main_sim_recompute(&space_main, 5);

    let space_fix = Space::new();
    seed_floor_corpus(&space_fix);
    let fixed = spike_recompute(&space_fix, 5);

    eprintln!(
        "[importance-floor MAIN-sim] n_live={} min={:.3} below={}/{} ({:.1}%) near_tomb={}",
        main.n_live,
        main.min_live,
        main.below_floor_count,
        main.n_live,
        main.below_floor_frac * 100.0,
        main.live_illegally_near_tombstone
    );
    eprintln!(
        "[importance-floor FIXED]    n_live={} min={:.3} below={}/{} ({:.1}%) near_tomb={}",
        fixed.n_live,
        fixed.min_live,
        fixed.below_floor_count,
        fixed.n_live,
        fixed.below_floor_frac * 100.0,
        fixed.live_illegally_near_tombstone
    );

    assert!(main.below_floor_count > 0, "main-sim must dip below 0.3");
    assert_eq!(fixed.below_floor_count, 0);
    assert!(fixed.min_live + 1e-9 >= IMPORTANCE_FLOOR);
    assert!(fixed.min_live > main.min_live + 0.05);
    assert_eq!(fixed.tombstone_at_forget, 5);
}

#[test]
fn floor_constants_policy() {
    assert!((IMPORTANCE_FLOOR - 0.3).abs() < 1e-9);
    assert!((FORGET_IMPORTANCE - 0.01).abs() < 1e-9);
    assert!((clamp_live_importance(0.05) - 0.3).abs() < 1e-9);
}

fn build_scheduler() -> (Arc<SchedulerCenter>, Arc<Space>) {
    let space = Arc::new(Space::new());
    let bus = EventBus::new(64);
    let tx = bus.sender();
    let rx = bus.subscribe();
    let energy = Arc::new(EnergyCenter::new(10000.0, 8.0, tx.clone(), bus.subscribe()));
    let knowledge = Arc::new(KnowledgeGraph::new());
    let cognitive = Arc::new(CognitiveEngine::new("", ""));
    let classifier = Arc::new(CategoryClassifier::new("", ""));
    std::env::set_var("EMBEDDING_API_URL", "http://example.invalid/v1/embeddings");
    std::env::set_var("EMBEDDING_API_KEY", "");
    let embedding = Arc::new(EmbeddingService::from_env());
    let gateway = Arc::new(GatewayCenter::new(
        space.clone(),
        energy.clone(),
        cognitive.clone(),
        classifier,
        tx.clone(),
        bus.subscribe(),
        knowledge.clone(),
        embedding,
        None,
    ));
    let security = Arc::new(SecurityGuard::from_env());
    let dir = std::env::temp_dir().join(format!(
        "epicode-floor-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&dir);
    let storage = Arc::new(StorageManager::new(&dir).unwrap());
    let scheduler = Arc::new(SchedulerCenter::with_security(
        space.clone(),
        energy,
        knowledge,
        cognitive,
        gateway,
        tx,
        rx,
        1000,
        10000.0,
        security,
        storage,
    ));
    (scheduler, space)
}

#[test]
fn forget_tombstone_uses_forget_importance() {
    let _g = LOCK.lock();
    let (scheduler, space) = build_scheduler();
    let id = add_mem(
        &space,
        Point3::zero(),
        "explicit forget target memory",
        vec!["forget-me".into()],
        2.0,
        0,
        chrono::Utc::now().timestamp(),
        None,
    );
    scheduler.api_forget_memory(id).expect("forget");
    let after = space.get_tetrahedron(id).unwrap();
    assert!(after.data.valid_to.is_some());
    assert!((after.data.importance - FORGET_IMPORTANCE).abs() < 1e-9);
}
