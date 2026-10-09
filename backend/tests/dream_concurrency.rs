//! 并发压力 + 幂等 + 质量分布 + 吞吐基准:dream 周期 vs 并发用户写入。
//! 仅使用公开 API,可在 main 与 op-log 原型上原样运行对比。
use epicode::domain::space::Space;
use epicode::domain::tetra::{MemoryPayload, Tetrahedron};
use epicode::domain::vertex::Point3;
use epicode::engine::dream::DreamEngine;
use epicode::engine::knowledge::KnowledgeGraph;
use std::sync::Arc;
use std::time::Instant;

fn seed(space: &Space, n: usize) -> Vec<u64> {
    let mut ids = Vec::new();
    for i in 0..n {
        let core = Point3::new(i as f64 * 3.0, 0.0, 0.0);
        let pos = Tetrahedron::compute_vertices(core);
        let mut emb = vec![0.0; epicode::engine::vector::EMBEDDING_DIM];
        emb[i % 64] = 1.0; // i 与 i+64 互为重复 → 有合并对
        let junk = i % 3 == 0;
        let t = Tetrahedron {
            id: 0,
            vertex_ids: [0; 4],
            core,
            data: MemoryPayload {
                content: format!("memory {i}"),
                labels: if junk {
                    vec!["junk".into()]
                } else {
                    vec![format!("t{i}")]
                },
                importance: 1.0,
                timestamp: chrono::Utc::now().timestamp(),
                embedding: emb,
                ..Default::default()
            },
            mass: 1.0,
        };
        ids.push(space.add_tetrahedron(&t, &pos).unwrap());
    }
    ids
}

/// 用户侧"正确"的写法:单锁闭包内追加标签(与 api_add_labels 的锁内语义一致)
fn user_add_label(space: &Space, id: u64, label: String) {
    let _ = space.with_tetra_mut(id, |p| {
        p.labels.push(label);
        true
    });
}

#[test]
fn concurrent_user_edits_survive_dream_cycles() {
    let space = Arc::new(Space::new());
    let ids = seed(&space, 192);
    let kg = Arc::new(KnowledgeGraph::new());
    let cycles = 40;
    let dreamer = {
        let (space, kg) = (space.clone(), kg.clone());
        std::thread::spawn(move || {
            for _ in 0..cycles {
                DreamEngine::cycle(&space, &kg, 0.99, 5, false);
            }
        })
    };
    let mut written = 0usize;
    let start = Instant::now();
    while !dreamer.is_finished() || written < 2000 {
        let id = ids[written % ids.len()];
        user_add_label(&space, id, format!("user-{written}"));
        written += 1;
        if written > 200_000 {
            break;
        }
    }
    dreamer.join().unwrap();
    let all = space.all_tetrahedrons();
    let present: usize = all
        .iter()
        .map(|t| {
            t.data
                .labels
                .iter()
                .filter(|l| l.starts_with("user-"))
                .count()
        })
        .sum();
    let lost = written - present;
    eprintln!(
        "STRESS cycles={cycles} user_writes={written} lost={lost} ({:.2}%) elapsed={:?}",
        lost as f64 * 100.0 / written as f64,
        start.elapsed()
    );
    assert_eq!(lost, 0, "user label edits lost to dream writeback");
}

#[test]
fn dream_is_idempotent_over_many_cycles() {
    let space = Space::new();
    seed(&space, 192);
    let kg = KnowledgeGraph::new();
    let mut per_cycle = Vec::new();
    for _ in 0..40 {
        let r = DreamEngine::cycle(&space, &kg, 0.99, 5, false);
        per_cycle.push((r.junk_evicted, r.duplicates_merged));
    }
    let all = space.all_tetrahedrons();
    let q = all
        .iter()
        .filter(|t| t.data.labels.iter().any(|l| l == "quarantine"))
        .count();
    let mut masses: Vec<f64> = all.iter().map(|t| t.mass).collect();
    masses.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let sum_ev: usize = per_cycle.iter().map(|x| x.0).sum();
    let sum_mg: usize = per_cycle.iter().map(|x| x.1).sum();
    let quarantined_mass_max = all
        .iter()
        .filter(|t| t.data.labels.iter().any(|l| l == "quarantine"))
        .map(|t| t.mass)
        .fold(0.0f64, f64::max);
    eprintln!(
        "IDEM per_cycle(evicted,merged)={per_cycle:?} total_evicted={sum_ev} distinct_quarantined={q} total_merged={sum_mg} mass[min/med/max]={:.2}/{:.2}/{:.2} quarantined_mass_max={quarantined_mass_max:.2}",
        masses[0],
        masses[masses.len() / 2],
        masses[masses.len() - 1]
    );
    assert_eq!(
        sum_ev, q,
        "every quarantine report must correspond to a distinct newly quarantined memory"
    );
    assert!(
        quarantined_mass_max <= 0.1 + 1e-9,
        "quarantined mass must be demoted, got {quarantined_mass_max}"
    );
    let sup = all
        .iter()
        .filter(|t| t.data.labels.iter().any(|l| l == "superseded"))
        .count();
    eprintln!("IDEM distinct_superseded={sup}");
    assert_eq!(
        sum_mg, sup,
        "every merge report must correspond to a distinct newly superseded memory"
    );
    let last = per_cycle.last().unwrap();
    assert_eq!(*last, (0, 0), "steady state must be a no-op");
}

#[test]
fn bench_dream_cycle_throughput() {
    let space = Space::new();
    seed(&space, 1000);
    let kg = KnowledgeGraph::new();
    let t = Instant::now();
    let n = 10;
    for _ in 0..n {
        DreamEngine::cycle(&space, &kg, 0.99, 5, false);
    }
    eprintln!("BENCH dream_cycle N=1000 avg={:?}", t.elapsed() / n);
}
