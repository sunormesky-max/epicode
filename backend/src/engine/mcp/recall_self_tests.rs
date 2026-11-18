#[allow(dead_code)]
fn isolated_engine() -> crate::engine::Engine {
    crate::engine::Engine::with_data_dir(
        std::env::temp_dir().join(format!("mcp-recall-self-{}", uuid::Uuid::new_v4())),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn recall_with_own_content_returns_that_memory() {
    let mut engine = isolated_engine();
    engine.start();
    let s = &engine.scheduler;
    let topics = ["Rust 调度器", "前端主题", "记忆整合", "部署流程", "身份层"];
    for i in 0..80 {
        let t = topics[i % topics.len()];
        let _ = s.api_create_memory(
            &format!("{t} 相关笔记 第{i}条：这条记录讨论{t}的设计取舍与实现细节，编号{i}"),
            vec![format!("topic{}", i % 5)],
        );
    }
    let target = "冒烟测试目标记忆：生产环境 1.9.0 动作闭环验证 smoke-r20 唯一标记 QX7";
    let (tid, _) = s.api_create_memory(target, vec!["topic0".into()]).unwrap();
    for depth in [1usize, 2] {
        let r = s.api_recall(target, depth).unwrap();
        let mut ids = vec![];
        let mut first = None;
        for (_, arr) in r["results"].as_object().unwrap() {
            for f in arr.as_array().unwrap() {
                let id = f["id"].as_u64().unwrap();
                let rel = f["relevance"].as_array().unwrap();
                let sc = rel[0].as_f64().unwrap().max(rel[1].as_f64().unwrap());
                if first.map(|(_, b)| sc > b).unwrap_or(true) {
                    first = Some((id, sc));
                }
                ids.push(id);
            }
        }
        eprintln!(
            "depth={depth} n={} contains={} bytes={}",
            ids.len(),
            ids.contains(&tid),
            serde_json::to_vec(&r).unwrap().len()
        );
        assert!(ids.contains(&tid), "depth {depth}: target missing");
        assert_eq!(
            first.map(|x| x.0),
            Some(tid),
            "depth {depth}: target not top-ranked"
        );
        let full = crate::engine::smrp::recall_data(&engine, &r, target, depth);
        let full_before = serde_json::to_vec(&full).unwrap().len();
        let mut capped = full.clone();
        crate::engine::smrp::cap_full_recall(
            &mut capped,
            crate::engine::smrp::SLIM_RECALL_MAX_ITEMS,
        );
        let full_after = serde_json::to_vec(&capped).unwrap().len();
        eprintln!("full depth={depth} before={full_before} after={full_after}");
        assert!(capped.get("sections").is_none());
        assert!(
            full_after * 2 < full_before,
            "full cap should at least halve payload"
        );
        let mut slim = crate::engine::smrp::to_slim_envelope(
            &full,
            crate::engine::smrp::SLIM_MAX_CONTENT_CHARS,
        );
        let before = serde_json::to_vec(&slim).unwrap().len();
        crate::engine::smrp::cap_slim_items(&mut slim, crate::engine::smrp::SLIM_RECALL_MAX_ITEMS);
        let after = serde_json::to_vec(&slim).unwrap().len();
        eprintln!("slim depth={depth} before={before} after={after}");
        assert_eq!(slim["items"][0]["id"].as_u64(), Some(tid));
        assert!(slim["items"].as_array().unwrap().len() <= 20);
        assert!(after * 2 < before, "slim cap should at least halve payload");
    }
}
