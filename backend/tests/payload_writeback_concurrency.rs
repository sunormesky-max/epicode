//! 治理/维护/API 路径 vs 并发用户写入:丢失更新 / 意图正确性 / 延迟。
//! 仅使用公开 API,可在 main 与本分支上原样运行对比(各自独立 CARGO_TARGET_DIR)。
//! 基准: cargo test --release --test payload_writeback_concurrency -- --ignored --nocapture
use epicode::domain::ops::MemoryOp;
use epicode::engine::Engine;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

fn engine() -> (Arc<Engine>, std::path::PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("epicode-pwb-{}-{}", std::process::id(), rand_id()));
    std::fs::create_dir_all(&dir).unwrap();
    (Arc::new(Engine::with_data_dir(dir.clone())), dir)
}

fn rand_id() -> u64 {
    use std::sync::atomic::AtomicU64;
    static C: AtomicU64 = AtomicU64::new(0);
    C.fetch_add(1, Ordering::Relaxed)
}

fn seed(e: &Engine, n: usize) -> Vec<u64> {
    (0..n)
        .map(|i| {
            e.gateway()
                .create_memory(
                    &format!("seed memory {i} about topic{i} with details{i} kappa{i}"),
                    if i % 3 == 0 {
                        vec![format!("t{i}"), "quarantine".to_string()]
                    } else {
                        vec![format!("t{i}")]
                    },
                )
                .unwrap()
                .id
        })
        .collect()
}

const PATHS: &[&str] = &[
    "api_set_memory_class",
    "api_forget_memory",
    "heal_memory",
    "api_scavenge",
];

/// 对每个 id 运行一次被测路径(sweep 类路径整体运行一次)
fn run_path(e: &Engine, path: &str, ids: &[u64], pass: usize) {
    let s = e.scheduler();
    match path {
        "api_set_memory_class" => ids.iter().for_each(|&id| {
            let _ = s.api_set_memory_class(
                id,
                if pass.is_multiple_of(2) {
                    "working"
                } else {
                    "episodic"
                },
            );
        }),
        "api_forget_memory" => ids.iter().for_each(|&id| {
            let _ = s.api_forget_memory(id);
        }),
        "heal_memory" => {
            let _ = s.heal_memory(0.3);
        }
        "api_scavenge" => {
            let _ = s.api_scavenge();
        }
        _ => unreachable!(),
    }
}

fn intent_ok(e: &Engine, path: &str, ids: &[u64]) -> usize {
    let mut miss = 0;
    for &id in ids {
        let t = e.space().get_tetrahedron(id).unwrap();
        let ok = match path {
            "api_set_memory_class" => t.data.memory_class.as_deref() == Some("episodic"),

            "api_forget_memory" => t.data.valid_to.is_some(),
            "heal_memory" => !t.data.labels.iter().any(|l| l == "quarantine"),
            // scavenge: 唤醒沉睡记忆 access_count 0→1
            "api_scavenge" => t.data.access_count >= 1,
            _ => unreachable!(),
        };
        if !ok {
            miss += 1;
        }
        // 原始标签必须仍在
        if !t.data.labels.iter().any(|l| l.starts_with('t')) {
            miss += 1;
        }
    }
    miss
}

struct Out {
    written: usize,
    lost: usize,
    miss: usize,
    ms: f64,
}

const PASSES: usize = 20;

fn round(e: &Arc<Engine>, path: &str, n: usize, writers: usize) -> Out {
    let ids = Arc::new(seed(e, n));
    let stop = Arc::new(AtomicBool::new(false));
    let hs: Vec<_> = (0..writers)
        .map(|w| {
            let (e, ids, stop) = (e.clone(), ids.clone(), stop.clone());
            std::thread::spawn(move || {
                let mut mine = Vec::new();
                let mut x = w * 7919;
                // 每写线程最多 5000 次,避免标签列表膨胀拖慢被测路径
                while !stop.load(Ordering::Relaxed) && mine.len() < 5000 {
                    let id = ids[x % ids.len()];
                    let l = format!("u{w}-{x}");
                    let _ = e.space().apply_ops(id, &[MemoryOp::AddLabel(l.clone())]);
                    mine.push((id, l));
                    x += 31;
                    std::thread::yield_now();
                }
                mine
            })
        })
        .collect();
    std::thread::sleep(std::time::Duration::from_millis(2));
    let t0 = Instant::now();
    // 重复 PASSES 遍(偶数遍结尾 = episodic),放大并发窗口;延迟按单遍计
    for pass in 0..PASSES {
        run_path(e, path, &ids, pass);
    }
    let ms = t0.elapsed().as_secs_f64() * 1e3 / PASSES as f64;
    stop.store(true, Ordering::Relaxed);
    let (mut written, mut lost) = (0, 0);
    for h in hs {
        for (id, l) in h.join().unwrap() {
            written += 1;
            if !e
                .space()
                .get_tetrahedron(id)
                .unwrap()
                .data
                .labels
                .contains(&l)
            {
                lost += 1;
            }
        }
    }
    let miss = intent_ok(e, path, &ids);
    Out {
        written,
        lost,
        miss,
        ms,
    }
}

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() - 1) as f64 * p).round() as usize]
}

#[test]
#[ignore]
fn payload_writeback_bench() {
    let rounds: usize = std::env::var("PWB_ROUNDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);
    let n: usize = std::env::var("PWB_N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(300);
    for path in PATHS {
        for &w in &[0usize, 4] {
            let mut lat = Vec::new();
            let (mut wr, mut lo, mut mi, mut rl) = (0, 0, 0, 0);
            let (e, dir) = engine();
            for _ in 0..rounds {
                let o = round(&e, path, n, w);
                lat.push(o.ms);
                wr += o.written;
                lo += o.lost;
                mi += o.miss;
                if o.lost > 0 {
                    rl += 1;
                }
            }
            drop(e);
            let _ = std::fs::remove_dir_all(dir);
            let (p50, p95) = (pct(&mut lat, 0.5), pct(&mut lat, 0.95));
            println!("RESULT path={path} n={n} writers={w} rounds={rounds} written={wr} lost={lo} rounds_with_loss={rl} intent_miss={mi} p50_ms={p50:.2} p95_ms={p95:.2}");
        }
    }
}

/// 回归:这些路径不得覆盖并发写入的用户标签(main 上失败)
#[test]
fn writeback_paths_keep_concurrent_labels() {
    let mut report = Vec::new();
    for path in PATHS {
        let (mut lost, mut miss) = (0, 0);
        let (e, dir) = engine();
        for _ in 0..3 {
            let o = round(&e, path, 120, 4);
            lost += o.lost;
            miss += o.miss;
        }
        drop(e);
        let _ = std::fs::remove_dir_all(dir);
        report.push((path, lost, miss));
    }
    for (p, lost, miss) in &report {
        assert_eq!(*miss, 0, "{p}: intent not applied");
        assert_eq!(*lost, 0, "{p}: overwrote concurrent labels ({report:?})");
    }
}
