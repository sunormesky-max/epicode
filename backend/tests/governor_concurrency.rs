//! governor 周期 vs 并发用户写入:丢失更新 / 意图正确性 / 延迟。
//! 仅使用公开 API,可在 main 与原型上原样运行对比。
//! 运行: cargo test --release --test governor_concurrency -- --ignored --nocapture
use epicode::domain::ops::MemoryOp;
use epicode::domain::space::Space;
use epicode::domain::tetra::{MemoryPayload, Tetrahedron};
use epicode::domain::vertex::Point3;
use epicode::engine::governor::{LifecycleGovernor, FORGET_IMPORTANCE, IMPORTANCE_FLOOR};
use epicode::engine::knowledge::KnowledgeGraph;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

const DAY: i64 = 86400;

#[derive(Default)]
struct Kinds {
    tomb: Vec<u64>,
    decay: Vec<u64>,
    recurrent: Vec<u64>,
    dup: Vec<u64>,
    all: Vec<u64>,
}

fn seed(space: &Space, n: usize) -> Kinds {
    let now = chrono::Utc::now().timestamp();
    let mut k = Kinds::default();
    for i in 0..n {
        let core = Point3::new(i as f64 * 3.0, 0.0, 0.0);
        let pos = Tetrahedron::compute_vertices(core);
        let mut p = MemoryPayload {
            content: format!("note{i} alpha{i} beta{i} gamma{i} delta{i}"),
            labels: vec![format!("t{i}")],
            importance: 1.0,
            timestamp: now - 2 * DAY,
            ..Default::default()
        };
        let kind = i % 6;
        match kind {
            0 => {
                p.memory_class = Some("episodic".into());
                p.importance = 0.1;
                p.timestamp = now - 90 * DAY;
            }
            1 => {
                p.memory_class = Some("session".into());
                p.timestamp = now - 10 * DAY;
            }
            2 => {
                p.memory_class = Some("bridge".into());
                p.timestamp = now - 3 * DAY;
            }
            3 => {
                p.memory_class = Some("episodic".into());
                p.timestamp = now - 40 * DAY;
            }
            4 => p.access_count = 5,
            _ => {
                p.content = format!(
                    "duplicate marker dupkey{k} alpha{k} beta{k} gamma{k}",
                    k = i / 12
                );
            }
        }
        let t = Tetrahedron {
            id: 0,
            vertex_ids: [0; 4],
            core,
            data: p,
            mass: 1.0,
        };
        let id = space.add_tetrahedron(&t, &pos).unwrap();
        k.all.push(id);
        match kind {
            0..=2 => k.tomb.push(id),
            3 => k.decay.push(id),
            4 => k.recurrent.push(id),
            _ => k.dup.push(id),
        }
    }
    k
}

/// 与调度器 tick 中 governor 段的调用顺序一致
fn governor_cycle(space: &Space, kg: &KnowledgeGraph) {
    let gov = LifecycleGovernor::evaluate_with_hot_labels(space, kg, &[]);
    LifecycleGovernor::reset_access_counts(space, &gov.recurrent_ids);
    let c = LifecycleGovernor::find_merge_candidates(space);
    LifecycleGovernor::execute_merges(space, &c);
}

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() - 1) as f64 * p).round() as usize]
}

struct Out {
    written: usize,
    lost: usize,
    intent_miss: usize,
    absorbed_mass_max: f64,
    ms: f64,
}

fn round(n: usize, writers: usize) -> Out {
    let space = Arc::new(Space::new());
    let kg = KnowledgeGraph::new();
    let k = seed(&space, n);
    let ids = Arc::new(k.all.clone());
    let stop = Arc::new(AtomicBool::new(false));
    let handles: Vec<_> = (0..writers)
        .map(|w| {
            let (space, ids, stop) = (space.clone(), ids.clone(), stop.clone());
            std::thread::spawn(move || {
                let mut mine = Vec::new();
                let mut x = w * 7919;
                while !stop.load(Ordering::Relaxed) {
                    let id = ids[x % ids.len()];
                    let l = format!("u{w}-{x}");
                    // 与 api_add_labels 相同的锁内追加
                    let _ = space.apply_ops(id, &[MemoryOp::AddLabel(l.clone())]);
                    mine.push((id, l));
                    x += 31;
                }
                mine
            })
        })
        .collect();
    std::thread::sleep(std::time::Duration::from_millis(2));
    let t0 = Instant::now();
    governor_cycle(&space, &kg);
    let ms = t0.elapsed().as_secs_f64() * 1e3;
    stop.store(true, Ordering::Relaxed);
    let mut written = 0;
    let mut lost = 0;
    for h in handles {
        for (id, l) in h.join().unwrap() {
            written += 1;
            if !space.get_tetrahedron(id).unwrap().data.labels.contains(&l) {
                lost += 1;
            }
        }
    }
    // 意图正确性
    let mut miss = 0;
    let mut cat = [0usize; 6];
    let g = |id| space.get_tetrahedron(id).unwrap();
    for &id in &k.tomb {
        let t = g(id);
        if t.data.valid_to.is_none() || (t.data.importance - FORGET_IMPORTANCE).abs() > 1e-9 {
            miss += 1;
            cat[0] += 1;
        }
        // 原始标签必须仍在
        if !t.data.labels.iter().any(|l| l.starts_with('t')) {
            miss += 1;
            cat[1] += 1;
        }
    }
    for &id in &k.decay {
        let t = g(id);
        if !(t.data.importance < 0.99 && t.data.importance >= IMPORTANCE_FLOOR - 1e-9) {
            miss += 1;
            cat[2] += 1;
        }
    }
    for &id in &k.recurrent {
        if g(id).data.access_count != 0 {
            miss += 1;
            cat[3] += 1;
        }
    }
    let mut absorbed_mass_max: f64 = 0.0;
    for pair in k.dup.chunks(2) {
        if pair.len() < 2 {
            continue;
        }
        let (a, b) = (g(pair[0]), g(pair[1]));
        let (keep, rem) = if a.data.labels.iter().any(|l| l == "superseded") {
            (b, a)
        } else {
            (a, b)
        };
        if !rem.data.labels.iter().any(|l| l == "superseded") || rem.data.valid_to.is_none() {
            miss += 1;
            cat[4] += 1;
        }
        // 保留者需拥有被吸收者的原始标签
        let orig = rem.data.labels.iter().find(|l| l.starts_with('t')).cloned();
        if let Some(o) = orig {
            if !keep.data.labels.contains(&o) {
                miss += 1;
                cat[5] += 1;
            }
        }
        absorbed_mass_max = absorbed_mass_max.max(rem.mass);
    }
    if std::env::var("GOV_DEBUG").is_ok() {
        eprintln!("cat tombv,tomblabel,decay,recur,dupsup,duplab = {cat:?}");
    }
    Out {
        written,
        lost,
        intent_miss: miss,
        absorbed_mass_max,
        ms,
    }
}

#[test]
#[ignore]
fn governor_bench() {
    let rounds: usize = std::env::var("GOV_ROUNDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    for &n in &[300usize, 1200] {
        for &w in &[0usize, 4] {
            let mut lat = Vec::new();
            let (mut wr, mut lo, mut mi) = (0, 0, 0);
            let mut mm: f64 = 0.0;
            let mut rounds_with_loss = 0;
            for _ in 0..rounds {
                let o = round(n, w);
                lat.push(o.ms);
                wr += o.written;
                lo += o.lost;
                mi += o.intent_miss;
                mm = mm.max(o.absorbed_mass_max);
                if o.lost > 0 {
                    rounds_with_loss += 1;
                }
            }
            let p50 = pct(&mut lat, 0.5);
            let p95 = pct(&mut lat, 0.95);
            println!(
                "RESULT n={n} writers={w} rounds={rounds} written={wr} lost={lo} rounds_with_loss={rounds_with_loss} intent_miss={mi} absorbed_mass_max={mm:.2} p50_ms={p50:.2} p95_ms={p95:.2}"
            );
        }
    }
}

/// 快速回归:并发写入时 governor 不得丢失用户标签
#[test]
fn governor_does_not_drop_concurrent_labels() {
    let mut lost = 0;
    let mut miss = 0;
    for _ in 0..5 {
        let o = round(300, 4);
        lost += o.lost;
        miss += o.intent_miss;
    }
    assert_eq!(miss, 0, "governor intent not fully applied");
    assert_eq!(lost, 0, "governor overwrote concurrent user labels");
}
