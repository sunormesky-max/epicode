use std::sync::Arc;

use epicode::domain::space::Space;
use epicode::engine::bus::EventBus;
use epicode::engine::cognitive::CognitiveEngine;
use epicode::engine::energy::EnergyCenter;
use epicode::engine::gateway::GatewayCenter;
use epicode::engine::knowledge::KnowledgeGraph;
use epicode::engine::scheduler::SchedulerCenter;

fn main() {
    println!("=== Epicode LLM中央调度器测试 ===\n");

    let api_key = std::env::var("DEEPSEEK_API_KEY").unwrap_or_default();
    if api_key.is_empty() {
        println!("ERROR: 请设置 DEEPSEEK_API_KEY 环境变量");
        println!("  $env:DEEPSEEK_API_KEY = \"sk-xxx\"; cargo run --bin brain_test");
        return;
    }

    let bus = Arc::new(EventBus::new(256));
    let space = Arc::new(Space::new());
    let energy = Arc::new(EnergyCenter::new(
        10000.0,
        1.0,
        bus.sender(),
        bus.subscribe(),
    ));
    let knowledge = Arc::new(KnowledgeGraph::new());
    let cognitive = Arc::new(CognitiveEngine::new(&api_key, "deepseek-chat"));
    let classifier = Arc::new(epicode::engine::classifier::CategoryClassifier::new(
        &api_key,
        "deepseek-chat",
    ));
    let embedding = Arc::new(epicode::engine::embedding::EmbeddingService::from_env());
    let vector = epicode::engine::vector::VectorLayer::load(std::path::Path::new("models"))
        .ok()
        .map(Arc::new);
    let gateway = Arc::new(GatewayCenter::new(
        space.clone(),
        energy.clone(),
        cognitive.clone(),
        classifier,
        bus.sender(),
        bus.subscribe(),
        knowledge.clone(),
        embedding,
        vector,
    ));

    let scheduler = Arc::new(SchedulerCenter::new(
        space.clone(),
        energy.clone(),
        knowledge.clone(),
        cognitive.clone(),
        gateway.clone(),
        bus.sender(),
        bus.subscribe(),
        1000,
        10000.0,
    ));

    println!("--- Phase 1: 放入混合记忆 ---");
    let topics = vec![
        (
            "quantum",
            vec![
                "quantum entanglement between particles",
                "quantum superposition principle",
                "Heisenberg uncertainty principle",
                "quantum tunneling in semiconductors",
                "quantum computing with qubits",
                "quantum error correction codes",
                "quantum teleportation protocol",
                "quantum key distribution security",
            ],
        ),
        (
            "rust",
            vec![
                "Rust ownership and borrowing rules",
                "Rust lifetime annotations explained",
                "Rust async await tokio runtime",
                "Rust trait objects dynamic dispatch",
                "Rust memory safety without GC",
                "Rust concurrency data races prevention",
                "Rust smart pointers Box Rc Arc",
                "Rust iterator adapters combinators",
            ],
        ),
        (
            "cooking",
            vec![
                "cooking steak reverse sear method",
                "cooking pasta al dente technique",
                "cooking sourdough bread starter",
                "cooking French onion soup recipe",
                "cooking tempura batter light crispy",
                "cooking risotto creamy consistency",
                "cooking braised short ribs slow",
                "cooking chocolate tempering temperature",
            ],
        ),
    ];

    let mut count = 0;
    for (topic, texts) in &topics {
        for text in texts {
            match gateway.create_memory(text, vec![topic.to_string()]) {
                Ok(id) => {
                    count += 1;
                    println!("  [{}] {} → id={}", topic, &text[..30.min(text.len())], id);
                }
                Err(e) => println!("  FAIL: {} → {}", text, e),
            }
        }
    }
    println!("  共 {} 条记忆, 能量: {:.0}", count, energy.available());

    let clusters = space.find_clusters();
    println!("\n  初始簇数: {}", clusters.len());

    println!("\n--- Phase 2: LLM认知决策 ---");
    println!("  调用 DeepSeek V4 Flash 分析系统状态...\n");

    let state = scheduler.collect_state_internal();
    println!("  状态摘要:");
    println!(
        "    tick: {}, energy: {:.0}/{:.0}",
        state.tick, state.energy, state.max_energy
    );
    println!(
        "    tetras: {}, vertices: {}, clusters: {}",
        state.total_tetras,
        state.total_vertices,
        state.clusters.len()
    );
    for c in &state.clusters {
        let dominant = c
            .label_distribution
            .iter()
            .max_by_key(|(_, &v)| v)
            .map(|(k, v)| format!("{}:{}", k, v))
            .unwrap_or("none".into());
        println!(
            "    簇{}: {}个 [{}] entropy={:.3} centroid=({:.1},{:.1},{:.1})",
            c.index, c.size, dominant, c.entropy, c.centroid[0], c.centroid[1], c.centroid[2]
        );
    }

    match cognitive.decide(&state) {
        Ok(response) => {
            println!("\n  [LLM思考] {}", response.thoughts);
            println!("\n  [LLM决策] {} 个操作:", response.actions.len());
            for (i, action) in response.actions.iter().enumerate() {
                match action {
                    epicode::engine::cognitive::SchedulerAction::Pulse {
                        origin,
                        pulse_type,
                        ttl,
                    } => {
                        println!(
                            "    {}. pulse(origin={}, type={}, ttl={})",
                            i + 1,
                            origin,
                            pulse_type,
                            ttl
                        );
                    }
                    epicode::engine::cognitive::SchedulerAction::Fission { cluster_index } => {
                        println!("    {}. fission(cluster={})", i + 1, cluster_index);
                    }
                    epicode::engine::cognitive::SchedulerAction::Fuse {
                        cluster_a,
                        cluster_b,
                    } => {
                        println!("    {}. fuse(clusters={}, {})", i + 1, cluster_a, cluster_b);
                    }
                    epicode::engine::cognitive::SchedulerAction::Dream => {
                        println!("    {}. dream", i + 1);
                    }
                    epicode::engine::cognitive::SchedulerAction::Link { a, b, reason } => {
                        println!("    {}. link({} <-> {}): {}", i + 1, a, b, reason);
                    }
                    epicode::engine::cognitive::SchedulerAction::UseTool { tool, args } => {
                        println!("    {}. use_tool({}): {}", i + 1, tool, args);
                    }
                    epicode::engine::cognitive::SchedulerAction::Consolidate {
                        ids,
                        keep,
                        summary,
                    } => {
                        println!(
                            "    {}. consolidate({:?} -> keep #{}): {}",
                            i + 1,
                            ids,
                            keep,
                            summary
                        );
                    }
                    epicode::engine::cognitive::SchedulerAction::MarkJunk { ids, reason } => {
                        println!("    {}. mark_junk({:?}): {}", i + 1, ids, reason);
                    }
                    epicode::engine::cognitive::SchedulerAction::Relabel {
                        id,
                        add_labels,
                        remove_labels,
                        reason,
                    } => {
                        println!(
                            "    {}. relabel(#{} +{:?} -{:?}): {}",
                            i + 1,
                            id,
                            add_labels,
                            remove_labels,
                            reason
                        );
                    }
                    epicode::engine::cognitive::SchedulerAction::Reflect {
                        observation,
                        insight,
                    } => {
                        println!("    {}. reflect: {} | {}", i + 1, observation, insight);
                    }
                }
            }

            println!("\n--- Phase 3: 执行LLM决策 ---");
            for action in &response.actions {
                scheduler.execute_action_internal(action);
            }

            let clusters_after = space.find_clusters();
            println!(
                "\n  执行后: {} 簇 → {} 簇",
                clusters.len(),
                clusters_after.len()
            );
            println!("  能量剩余: {:.0}", energy.available());
        }
        Err(e) => {
            println!("\n  [ERROR] LLM调用失败: {}", e);
        }
    }

    println!("\n=== 测试完成 ===");
}
