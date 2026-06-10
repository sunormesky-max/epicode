use std::time::Instant;

#[tokio::main]
async fn main() {
    let mut engine = epicode::engine::Engine::with_data_dir(
        std::path::PathBuf::from("data/bench_tmp")
    );
    engine.start();

    println!("=== Epicode v1.0.0 Performance Benchmark ===\n");

    let total_memories = 100;
    let warmup = 5;

    // Phase 1: Write throughput
    println!("--- Phase 1: Write (Remember) Throughput ---");
    let mut write_times = Vec::new();
    for i in 0..(total_memories + warmup) {
        let content = format!("Benchmark memory #{i}: Rust memory system with ONNX vector search and multi-layer scheduling, testing performance at scale with various content lengths and semantic diversity in Chinese and English混合内容");
        let start = Instant::now();
        let result = engine.scheduler.api_remember(&content);
        let elapsed = start.elapsed();
        if i >= warmup {
            write_times.push(elapsed.as_micros() as f64 / 1000.0);
        }
        if result.is_err() && i < 5 {
            eprintln!("ERROR at {}: {:?}", i, result);
        }
    }
    print_stats("Remember", &write_times, total_memories);

    // Phase 2: Search latency
    println!("\n--- Phase 2: Search Latency ---");
    let queries = vec![
        "Rust向量搜索",
        "performance optimization",
        "记忆系统架构设计",
        "ONNX模型推理速度",
        "multi-tenant data isolation",
        "四面体物理哲学",
        "加密安全防护措施",
        "Epicode cloud deployment",
    ];
    let mut search_times = Vec::new();
    for (round, query) in queries.iter().cycle().take(50).enumerate() {
        let start = Instant::now();
        let _ = engine.scheduler.api_search(query, 10);
        let elapsed = start.elapsed();
        if round >= 5 {
            search_times.push(elapsed.as_micros() as f64 / 1000.0);
        }
    }
    print_stats("Search", &search_times, 45);

    // Phase 3: Same query cache hit
    println!("\n--- Phase 3: Search Cache Hit ---");
    let mut cache_times = Vec::new();
    for i in 0..20 {
        let start = Instant::now();
        let _ = engine.scheduler.api_search("Rust向量搜索", 10);
        let elapsed = start.elapsed();
        if i >= 2 {
            cache_times.push(elapsed.as_micros() as f64 / 1000.0);
        }
    }
    print_stats("Search (cached)", &cache_times, 18);

    // Phase 4: Recall latency
    println!("\n--- Phase 4: Recall Latency ---");
    let mut recall_times = Vec::new();
    let recall_queries = vec!["架构设计", "security", "性能", "cloud", "记忆"];
    for (round, q) in recall_queries.iter().cycle().take(20).enumerate() {
        let start = Instant::now();
        let _ = engine.scheduler.api_recall(q, 3);
        let elapsed = start.elapsed();
        if round >= 3 {
            recall_times.push(elapsed.as_micros() as f64 / 1000.0);
        }
    }
    print_stats("Recall", &recall_times, 17);

    // Phase 5: Get single node
    println!("\n--- Phase 5: Get Node (by ID) ---");
    let mut get_times = Vec::new();
    for id in 1..=100 {
        let start = Instant::now();
        let _ = engine.scheduler.api_get_node(id);
        let elapsed = start.elapsed();
        get_times.push(elapsed.as_micros() as f64 / 1000.0);
    }
    print_stats("Get Node", &get_times, 100);

    // Phase 6: Stats
    println!("\n--- Phase 6: Stats ---");
    let mut stats_times = Vec::new();
    for _ in 0..50 {
        let start = Instant::now();
        let _ = engine.scheduler.api_stats();
        let elapsed = start.elapsed();
        stats_times.push(elapsed.as_micros() as f64 / 1000.0);
    }
    print_stats("Stats", &stats_times, 50);

    // Phase 7: Bulk write with varying content lengths
    println!("\n--- Phase 7: Write by Content Length ---");
    for &len in [50, 200, 500, 1000, 2000].iter() {
        let content = "x".repeat(len);
        let mut times = Vec::new();
        for i in 0..20 {
            let c = format!("{}{}", content, i);
            let start = Instant::now();
            let _ = engine.scheduler.api_remember(&c);
            let elapsed = start.elapsed();
            times.push(elapsed.as_micros() as f64 / 1000.0);
        }
        print_stats(&format!("Write {}chars", len), &times, 20);
    }

    // Phase 8: Search with varying result limits
    println!("\n--- Phase 8: Search by Result Limit ---");
    for &limit in [5, 10, 20, 50, 100].iter() {
        let mut times = Vec::new();
        for i in 0..10 {
            let q = format!("benchmark query {}", i % 5);
            let start = Instant::now();
            let _ = engine.scheduler.api_search(&q, limit);
            let elapsed = start.elapsed();
            times.push(elapsed.as_micros() as f64 / 1000.0);
        }
        print_stats(&format!("Search limit={}", limit), &times, 10);
    }

    let stats = engine.scheduler.api_stats();
    println!("\n=== Final Stats ===");
    println!("Total memories: {}", stats.tetra_count);
    println!("Clusters: {}", stats.clusters);
    println!("Energy: {}", stats.energy);

    engine.final_save();
    std::fs::remove_dir_all("data/bench_tmp").ok();

    println!("\n=== Benchmark Complete ===");
}

fn print_stats(name: &str, times: &[f64], count: usize) {
    let mut sorted = times.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let avg: f64 = sorted.iter().sum::<f64>() / sorted.len() as f64;
    let p50 = sorted[sorted.len() / 2];
    let p95 = sorted[(sorted.len() as f64 * 0.95) as usize];
    let p99 = sorted[(sorted.len() as f64 * 0.99) as usize];
    let min = sorted[0];
    let max = sorted[sorted.len() - 1];
    println!("{} (n={}) avg={:.1}ms p50={:.1}ms p95={:.1}ms p99={:.1}ms min={:.1}ms max={:.1}ms",
        name, count, avg, p50, p95, p99, min, max);
}
