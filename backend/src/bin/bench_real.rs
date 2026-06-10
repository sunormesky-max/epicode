use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use epicode::engine::Engine;
use epicode::engine::mcp::McpHandler;

#[tokio::main]
async fn main() {
    std::env::set_var("EMBEDDING_API_URL", "disabled://none");

    let data_dir = std::env::var("TETRAMEM_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data_bench"));
    if data_dir.exists() {
        std::fs::remove_dir_all(&data_dir).ok();
    }
    std::fs::create_dir_all(&data_dir).ok();

    let mut engine = Engine::with_data_dir(data_dir);
    engine.start_quiet_with_interval(300000);
    let handler = Arc::new(McpHandler::new(Arc::new(engine)));

    let mem_count: usize = std::env::var("MEM_COUNT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);

    eprintln!("=== Epicode Benchmark: {} memories ===", mem_count);

    // Phase 1: Create memories
    let categories = vec![
        ("architecture", "System uses microservices with event-driven communication pattern"),
        ("bug", "Memory leak in connection pool caused by unclosed handles in async tasks"),
        ("decision", "Chose PostgreSQL over MongoDB for ACID compliance requirements"),
        ("pattern", "Always use circuit breaker pattern for external service calls"),
        ("preference", "User prefers dark theme with monospace fonts for code review"),
        ("session", "Implemented OAuth2 flow and fixed token refresh edge cases"),
        ("finding", "Query performance degrades linearly with JOIN count above 5 tables"),
        ("convention", "All API endpoints return consistent JSON error format with code field"),
    ];

    let t0 = Instant::now();
    let mut create_times = Vec::new();
    let mut ids = Vec::new();

    for i in 0..mem_count {
        let (cat, template) = &categories[i % categories.len()];
        let content = format!("[{}] {} — instance #{} with unique context about {} operations", 
            cat, template, i, cat);
        let labels = vec![cat.to_string(), format!("bench-{}", i % 10)];

        let raw = format!(r#"{{"jsonrpc":"2.0","id":{},"method":"tools/call","params":{{"name":"memory_create","arguments":{{"content":"{}","labels":{}}}}}}}"#,
            i, content.replace('"', "\\\""), serde_json::to_string(&labels).unwrap());

        let t = Instant::now();
        let resp = handler.process_json(&raw);
        create_times.push(t.elapsed().as_millis() as u64);

        if let Ok(p) = serde_json::from_str::<serde_json::Value>(&resp) {
            if let Some(inner) = p["result"]["content"][0]["text"].as_str() {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(inner) {
                    if let Some(id) = v["id"].as_u64() {
                        ids.push(id);
                    }
                }
            }
        }

        if (i + 1) % 50 == 0 {
            eprintln!("  Created {}/{}", i + 1, mem_count);
        }
    }
    let create_total = t0.elapsed();
    eprintln!("Create: {} memories in {}ms (avg {}ms, p95 {}ms, max {}ms)",
        mem_count, create_total.as_millis(),
        create_times.iter().sum::<u64>() as f64 / create_times.len() as f64,
        percentile(&create_times, 95),
        create_times.iter().max().unwrap_or(&0));

    // Phase 2: Search
    let queries = vec![
        "memory leak connection pool",
        "microservices architecture pattern",
        "database selection decision",
        "circuit breaker external service",
        "dark theme user interface preference",
        "OAuth2 authentication token refresh",
        "query performance JOIN optimization",
        "API error format convention",
        "event driven communication",
        "async task handle cleanup",
    ];

    let mut search_times = Vec::new();
    let mut search_sims = Vec::new();
    let t1 = Instant::now();

    for (qi, query) in queries.iter().cycle().take(mem_count / 2).enumerate() {
        let raw = format!(r#"{{"jsonrpc":"2.0","id":{},"method":"tools/call","params":{{"name":"memory_search","arguments":{{"query":"{}","limit":5}}}}}}"#,
            1000 + qi, query);

        let t = Instant::now();
        let resp = handler.process_json(&raw);
        search_times.push(t.elapsed().as_millis() as u64);

        if let Ok(p) = serde_json::from_str::<serde_json::Value>(&resp) {
            if let Some(inner) = p["result"]["content"][0]["text"].as_str() {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(inner) {
                    if let Some(results) = v["results"].as_array() {
                        for r in results {
                            if let Some(sim) = r["similarity"].as_f64() {
                                search_sims.push(sim);
                            }
                        }
                    }
                }
            }
        }
    }
    let search_total = t1.elapsed();
    eprintln!("Search: {} queries in {}ms (avg {}ms, p95 {}ms, max {}ms)",
        mem_count / 2, search_total.as_millis(),
        search_times.iter().sum::<u64>() as f64 / search_times.len() as f64,
        percentile(&search_times, 95),
        search_times.iter().max().unwrap_or(&0));

    if !search_sims.is_empty() {
        let avg_sim = search_sims.iter().sum::<f64>() / search_sims.len() as f64;
        let max_sim = search_sims.iter().fold(0.0f64, |a, b| a.max(*b));
        let above_05 = search_sims.iter().filter(|s| **s > 0.5).count();
        eprintln!("Search quality: avg_sim={:.3}, max_sim={:.3}, >0.5: {}/{} ({:.0}%)",
            avg_sim, max_sim, above_05, search_sims.len(), above_05 as f64 / search_sims.len() as f64 * 100.0);
    }

    // Phase 3: Recall
    let t2 = Instant::now();
    let recall_raw = r#"{"jsonrpc":"2.0","id":2000,"method":"tools/call","params":{"name":"memory_recall","arguments":{"query":"system architecture decisions","depth":2}}}"#;
    let _resp = handler.process_json(&recall_raw);
    let recall_time = t2.elapsed();
    eprintln!("Recall: {}ms", recall_time.as_millis());

    // Phase 4: Stats
    let stats_raw = r#"{"jsonrpc":"2.0","id":3000,"method":"tools/call","params":{"name":"space_stats","arguments":{}}}"#;
    let resp = handler.process_json(stats_raw);
    if let Ok(p) = serde_json::from_str::<serde_json::Value>(&resp) {
        if let Some(inner) = p["result"]["content"][0]["text"].as_str() {
            eprintln!("Stats: {}", inner);
        }
    }

    // Phase 5: Dream
    let t3 = Instant::now();
    let dream_raw = r#"{"jsonrpc":"2.0","id":4000,"method":"tools/call","params":{"name":"dream_cycle","arguments":{}}}"#;
    let resp = handler.process_json(dream_raw);
    let dream_time = t3.elapsed();
    if let Ok(p) = serde_json::from_str::<serde_json::Value>(&resp) {
        if let Some(inner) = p["result"]["content"][0]["text"].as_str() {
            eprintln!("Dream: {}ms — {}", dream_time.as_millis(), inner);
        }
    }

    // Phase 6: context_observe
    let observe_raw = r#"{"jsonrpc":"2.0","id":5000,"method":"tools/call","params":{"name":"context_observe","arguments":{"context":"We decided to use Redis for caching because it provides sub-millisecond latency. The pattern is to always check cache before hitting the database. We fixed a bug where cache keys were not being invalidated on updates, causing stale data. Root cause was missing cache eviction logic in the update handler.","project":"Epicode","role":"coding"}}}"#;
    let resp = handler.process_json(observe_raw);
    if let Ok(p) = serde_json::from_str::<serde_json::Value>(&resp) {
        if let Some(inner) = p["result"]["content"][0]["text"].as_str() {
            eprintln!("context_observe: {}", inner);
        }
    }

    // Phase 7: ctx_load (simulating new session)
    let t4 = Instant::now();
    let ctx_raw = r#"{"jsonrpc":"2.0","id":6000,"method":"tools/call","params":{"name":"ctx_load","arguments":{"project":"bench-0"}}}"#;
    let _resp = handler.process_json(ctx_raw);
    let ctx_time = t4.elapsed();
    eprintln!("ctx_load: {}ms", ctx_time.as_millis());

    // Memory usage
    if let Ok(mem) = std::fs::metadata(format!("data_bench/epicode.db")) {
        eprintln!("DB size: {}KB", mem.len() / 1024);
    }
}

fn percentile(data: &[u64], p: u64) -> u64 {
    if data.is_empty() { return 0; }
    let mut sorted = data.to_vec();
    sorted.sort();
    let idx = ((p as f64 / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}
