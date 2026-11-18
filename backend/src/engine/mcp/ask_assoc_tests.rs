use crate::engine::knowledge::RelationType;

fn isolated_engine() -> crate::engine::Engine {
    crate::engine::Engine::with_data_dir(
        std::env::temp_dir().join(format!("mcp-ask-assoc-{}", uuid::Uuid::new_v4())),
    )
}

/// memory_ask 必须像 memory_recall 一样带出 KG 联想扩展的邻居：
/// assoc = 父节点分 × 边强度（#267 语义），且不超过父节点分。
#[tokio::test(flavor = "multi_thread")]
async fn ask_surfaces_kg_neighbor_with_parent_bounded_assoc() {
    let mut engine = isolated_engine();
    engine.start();
    let s = &engine.scheduler;
    // 填充 50 条与问题部分同词的记忆，把与问题无关的邻居挤出直接命中种子。
    for i in 0..50 {
        let _ = s.api_create_memory(
            &format!("联想 冷启动 回滚 演练 笔记 第{i}条：记录第{i}次演练的细节"),
            vec![format!("topic{}", i % 5)],
        );
    }
    let q = "联想种子记忆 唯一标记 ZK3 冷启动 回滚 演练";
    let (seed, _) = s.api_create_memory(q, vec!["seed-topic".into()]).unwrap();
    let (nb, _) = s
        .api_create_memory(
            "完全无关的邻居：周五团队聚餐订在河边餐厅，记得带伞",
            vec!["neighbor-topic".into()],
        )
        .unwrap();
    let strength = 0.9;
    s.kg_handle()
        .add_relation(seed, nb, RelationType::Related, strength);

    // 前置：邻居不是直接命中（recall 30 个种子都不含它），只能靠联想带出。
    let r = s.api_recall(q, 2).unwrap();
    let mut recall_nb = None;
    let mut recall_seed = None;
    for (_, arr) in r["results"].as_object().unwrap() {
        for f in arr.as_array().unwrap() {
            let rel = f["relevance"].as_array().unwrap();
            let (ds, asim) = (rel[0].as_f64().unwrap(), rel[1].as_f64().unwrap());
            if f["id"].as_u64() == Some(nb) {
                recall_nb = Some((ds, asim));
            }
            if f["id"].as_u64() == Some(seed) {
                recall_seed = Some(ds);
            }
        }
    }
    let (nb_ds, nb_assoc) = recall_nb.expect("recall must surface the KG neighbor");
    let seed_ds = recall_seed.expect("recall must contain the seed");
    assert_eq!(nb_ds, 0.0, "neighbor must not be a direct hit");
    assert!(nb_assoc >= seed_ds * strength - 1e-9 && nb_assoc <= seed_ds + 1e-9);

    let a = s.api_ask(q, 2).unwrap();
    let mems = a["memories"].as_array().unwrap();
    let rel_of = |id: u64| {
        mems.iter()
            .find(|m| m["id"].as_u64() == Some(id))
            .and_then(|m| m["relevance"].as_f64())
    };
    let ask_seed = rel_of(seed).expect("ask must contain the seed");
    let ask_nb = rel_of(nb).expect("ask must list the KG neighbor");
    eprintln!("seed={ask_seed} nb={ask_nb} strength={strength}");
    assert!(
        ask_nb >= ask_seed * strength - 1e-9,
        "neighbor assoc {ask_nb} < parent {ask_seed} × {strength}"
    );
    assert!(ask_nb <= ask_seed + 1e-9, "assoc must be bounded by parent");
    let answer = a["answer"].as_str().unwrap();
    assert!(
        answer.contains(&format!("[#{nb}]")),
        "ask context must include the associative neighbor"
    );
}
