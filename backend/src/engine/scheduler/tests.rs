use super::*;
use crate::domain::tetra::EDGE_LENGTH;
use crate::domain::tetra::{MemoryPayload, Tetrahedron};
use crate::domain::vertex::Point3;
use crate::engine::CategoryClassifier;
use crate::engine::EmbeddingService;
use crate::engine::GatewayCenter;
use crate::engine::StorageManager;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::Arc;

struct MemoryImproveMockServer {
    base_url: String,
    requests: Arc<std::sync::Mutex<Vec<(String, serde_json::Value)>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl MemoryImproveMockServer {
    fn start(rewrite: String) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let server_stop = Arc::clone(&stop);
        let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
        let server_requests = Arc::clone(&requests);
        let thread = std::thread::spawn(move || {
            while !server_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                            .unwrap();
                        let (path, request) = read_memory_improve_request(&mut stream);
                        server_requests
                            .lock()
                            .unwrap()
                            .push((path.clone(), request));

                        let response = match path.as_str() {
                            "/v1/chat/completions" => serde_json::json!({
                                "choices": [{"message": {"content": rewrite}}]
                            }),
                            "/api/embed" => {
                                let mut embedding = vec![0.0; super::super::vector::EMBEDDING_DIM];
                                embedding[1] = 1.0;
                                serde_json::json!({"embeddings": [embedding]})
                            }
                            _ => panic!("unexpected memory-improve fixture path: {}", path),
                        };
                        let body = serde_json::to_vec(&response).unwrap();
                        write!(
                                stream,
                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                body.len()
                            )
                            .unwrap();
                        stream.write_all(&body).unwrap();
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(error) => panic!("memory-improve fixture failed: {error}"),
                }
            }
        });

        Self {
            base_url,
            requests,
            stop,
            thread: Some(thread),
        }
    }

    fn requests(&self) -> Vec<(String, serde_json::Value)> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for MemoryImproveMockServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let result = thread.join();
            if !std::thread::panicking() {
                result.expect("memory-improve fixture should stop cleanly");
            }
        }
    }
}

fn read_memory_improve_request(stream: &mut std::net::TcpStream) -> (String, serde_json::Value) {
    let mut request = Vec::new();
    let mut expected_length = None;
    let mut buffer = [0u8; 4096];
    loop {
        let read = stream
            .read(&mut buffer)
            .expect("fixture request read failed");
        assert!(
            read > 0,
            "fixture request ended before the body was complete"
        );
        request.extend_from_slice(&buffer[..read]);

        if expected_length.is_none() {
            if let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .expect("fixture request must include Content-Length");
                expected_length = Some(header_end + 4 + content_length);
            }
        }

        if expected_length.is_some_and(|length| request.len() >= length) {
            break;
        }
    }

    let header_end = request
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .expect("fixture request must include headers");
    let headers = String::from_utf8_lossy(&request[..header_end]);
    let path = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .expect("fixture request must include a path")
        .to_string();
    let body_end = expected_length.expect("fixture request body length must be known");
    let body = &request[header_end + 4..body_end];
    let body = serde_json::from_slice(body).expect("fixture request body must be JSON");
    (path, body)
}

struct MemoryImproveTestDir(std::path::PathBuf);

impl MemoryImproveTestDir {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("epicode-memory-improve-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for MemoryImproveTestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn periodic_save_never_acks_unexecuted_signals() {
    // An enabled cognitive engine used to turn the text "do not execute" into
    // an executed acknowledgement. Keep the model available so this test
    // exercises the production configuration without making an external call.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let stop = Arc::new(AtomicBool::new(false));
    let stop_server = Arc::clone(&stop);
    let model_requests = Arc::new(AtomicUsize::new(0));
    let request_counter = Arc::clone(&model_requests);
    let server = std::thread::spawn(move || {
        while !stop_server.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    request_counter.fetch_add(1, Ordering::Relaxed);
                    stream
                        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                        .unwrap();
                    let mut request = [0u8; 4096];
                    let _ = stream.read(&mut request);
                    let body = r#"{"choices":[{"message":{"content":"do not execute"}}]}"#;
                    let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                    let _ = stream.write_all(response.as_bytes());
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(error) => panic!("local model fixture failed: {error}"),
            }
        }
    });

    let cognitive = Arc::new(CognitiveEngine::with_base(
        "test-key",
        "test-model",
        &base_url,
    ));
    let (scheduler, _, _) = build_scheduler_with_cognitive(cognitive);
    let queue = scheduler.drive_queue();
    let make_signal = |description: &str| super::super::drive::DriveSignal {
        id: 0,
        timestamp: chrono::Utc::now().timestamp(),
        intent_type: super::super::drive::DriveIntent::Suggest,
        description: description.into(),
        evidence: vec![],
        urgency: super::super::drive::DriveUrgency::Medium,
        target_capability: Some("conversation".into()),
        emotion: None,
        origin_tick: 0,
        status: super::super::drive::DriveStatus::Pending,
        feedback: None,
        retry_count: 0,
        expires_at: None,
        enqueued_at_ms: 0,
        time_budget_ms: None,
        grounding: None,
        terminal_reason: None,
    };
    let delivered = queue.enqueue(make_signal("Review this decision with the agent"));
    let pending = queue.enqueue(make_signal("Present this suggestion to the user"));
    assert_eq!(queue.poll(1)[0].id, delivered);

    scheduler.auto_save();
    scheduler.set_runtime_primary(true);
    scheduler.auto_save();

    stop.store(true, Ordering::Relaxed);
    server.join().unwrap();
    assert_eq!(model_requests.load(Ordering::Relaxed), 0);

    let delivered_signal = queue.get_signal(delivered).unwrap();
    assert!(matches!(
        delivered_signal.status,
        super::super::drive::DriveStatus::Delivered
    ));
    let pending_signal = queue.get_signal(pending).unwrap();
    assert!(matches!(
        pending_signal.status,
        super::super::drive::DriveStatus::Pending
    ));
    for signal in [&delivered_signal, &pending_signal] {
        assert!(signal.feedback.is_none());
        assert_eq!(signal.retry_count, 0);
    }
    assert_eq!(queue.stats()["executed"], 0);
    assert_eq!(queue.stats()["rejected"], 0);
    assert_eq!(queue.peek_unacked(10).len(), 2);
}

#[test]
fn grounded_signal_expires_when_its_persistent_source_changes() {
    let (scheduler, space, _) = build_scheduler();
    let id = add_tetra_to_space(
        &space,
        Point3 {
            x: 50.0,
            y: 50.0,
            z: 50.0,
        },
        "a current decision memory",
        vec!["decision".into()],
    );
    let source = space.get_tetrahedron(id).unwrap();
    let revision = super::super::drive::memory_revision(&source.data);
    let signal = super::super::drive::DriveSignal {
        id: 0,
        timestamp: chrono::Utc::now().timestamp(),
        intent_type: super::super::drive::DriveIntent::Suggest,
        description: "Review a memory-backed decision".into(),
        evidence: vec![id],
        urgency: super::super::drive::DriveUrgency::Medium,
        target_capability: Some("conversation".into()),
        emotion: None,
        origin_tick: 0,
        status: super::super::drive::DriveStatus::Pending,
        feedback: None,
        retry_count: 0,
        expires_at: None,
        enqueued_at_ms: 0,
        time_budget_ms: None,
        grounding: Some(super::super::drive::DriveGrounding {
            reason: "recent decision memory".into(),
            evidence: vec![super::super::drive::DriveMemoryEvidence {
                id,
                revision: format!("{:016x}", revision),
                recorded_at: source.data.timestamp,
                last_reviewed_at: source.data.last_reviewed_ts,
                importance: source.data.importance,
            }],
            uncertainty: vec![super::super::drive::DriveGroundingUncertainty::SingleMemorySource],
            fresh_until: None,
            complete: true,
        }),
        terminal_reason: None,
    };
    let now = chrono::Utc::now().timestamp();
    assert!(scheduler.signal_evidence_is_current(&signal, now));
    let drive_id = scheduler
        .drive_queue()
        .try_enqueue(signal)
        .expect("grounded signal should be admitted");

    let mut reviewed_memory = space.get_tetrahedron(id).unwrap().data;
    reviewed_memory.last_reviewed_ts = Some(now);
    space.update_payload(id, reviewed_memory).unwrap();
    let mut projection = scheduler.drive_queue().get_signal(drive_id).unwrap();
    scheduler.refresh_signal_grounding(&mut projection, now);
    assert!(!projection
        .grounding
        .as_ref()
        .unwrap()
        .uncertainty
        .contains(&super::super::drive::DriveGroundingUncertainty::NotReviewed));
    assert!(scheduler.signal_evidence_is_current(&projection, now));

    let mut updated = space.get_tetrahedron(id).unwrap().data;
    updated.content = "the decision has changed".into();
    updated.content_hash += 1;
    space.update_payload(id, updated).unwrap();

    let signal = scheduler.drive_queue().get_signal(drive_id).unwrap();
    assert!(!scheduler.signal_evidence_is_current(&signal, now));
    assert_eq!(
        scheduler.drive_queue().expire_stale_evidence(&[drive_id]),
        1
    );
    assert_eq!(scheduler.drive_queue().stats()["stale_evidence_expired"], 1);
}

#[test]
fn l0_label_selection_filters_stale_sources_before_applying_the_limit() {
    let (scheduler, space, _) = build_scheduler();
    let now = chrono::Utc::now().timestamp();
    let labels = vec!["decision".to_string()];
    let stale_id = add_tetra_to_space(
        &space,
        Point3 {
            x: 60.0,
            y: 60.0,
            z: 60.0,
        },
        "newest but expired decision",
        labels.clone(),
    );
    let current_id = add_tetra_to_space(
        &space,
        Point3 {
            x: 70.0,
            y: 70.0,
            z: 70.0,
        },
        "older current decision",
        labels.clone(),
    );
    let mut stale = space.get_tetrahedron(stale_id).unwrap().data;
    stale.timestamp = now - 10;
    stale.valid_from = now - 10;
    stale.expired_at = Some(now - 1);
    space.update_payload(stale_id, stale).unwrap();
    scheduler.gateway.update_label_index(stale_id, &[], &labels);

    let mut current = space.get_tetrahedron(current_id).unwrap().data;
    current.timestamp = now - 100;
    current.valid_from = now - 100;
    space.update_payload(current_id, current).unwrap();
    scheduler
        .gateway
        .update_label_index(current_id, &[], &labels);

    assert_eq!(
        scheduler.gateway.list_by_labels(&["decision"], 1)[0].0,
        stale_id
    );
    let selected = scheduler
        .gateway
        .list_by_labels_matching(&["decision"], 1, |_, memory| {
            super::super::drive::memory_is_recent(memory, now, 1_800)
        });
    assert_eq!(selected[0].0, current_id);
}

#[test]
fn new_contradiction_withholds_a_queued_action_suggestion() {
    let (scheduler, space, knowledge) = build_scheduler();
    let source_id = add_tetra_to_space(
        &space,
        Point3 {
            x: 80.0,
            y: 80.0,
            z: 80.0,
        },
        "current decision source",
        vec!["decision".into()],
    );
    let conflicting_id = add_tetra_to_space(
        &space,
        Point3 {
            x: 90.0,
            y: 90.0,
            z: 90.0,
        },
        "current conflicting source",
        vec!["decision".into()],
    );
    let source = space.get_tetrahedron(source_id).unwrap();
    let signal = super::super::drive::DriveSignal {
        id: 0,
        timestamp: chrono::Utc::now().timestamp(),
        intent_type: super::super::drive::DriveIntent::Suggest,
        description: "Review a decision follow-up".into(),
        evidence: vec![source_id],
        urgency: super::super::drive::DriveUrgency::Medium,
        target_capability: Some("conversation".into()),
        emotion: None,
        origin_tick: 0,
        status: super::super::drive::DriveStatus::Pending,
        feedback: None,
        retry_count: 0,
        expires_at: None,
        enqueued_at_ms: 0,
        time_budget_ms: None,
        grounding: Some(super::super::drive::DriveGrounding {
            reason: "recent decision memory".into(),
            evidence: vec![super::super::drive::DriveMemoryEvidence {
                id: source_id,
                revision: format!(
                    "{:016x}",
                    super::super::drive::memory_revision(&source.data)
                ),
                recorded_at: source.data.timestamp,
                last_reviewed_at: source.data.last_reviewed_ts,
                importance: source.data.importance,
            }],
            uncertainty: vec![super::super::drive::DriveGroundingUncertainty::SingleMemorySource],
            fresh_until: None,
            complete: true,
        }),
        terminal_reason: None,
    };
    let now = chrono::Utc::now().timestamp();
    assert!(scheduler.signal_evidence_is_current(&signal, now));
    let drive_id = scheduler
        .drive_queue()
        .try_enqueue(signal)
        .expect("suggestion should be admitted before contradiction appears");

    knowledge.add_relation(
        source_id,
        conflicting_id,
        super::super::knowledge::RelationType::Contradicts,
        0.5,
    );

    let signal = scheduler.drive_queue().get_signal(drive_id).unwrap();
    assert!(!scheduler.signal_evidence_is_current(&signal, now));
    assert_eq!(
        scheduler.drive_queue().expire_stale_evidence(&[drive_id]),
        1
    );
    assert_eq!(scheduler.drive_queue().stats()["stale_evidence_expired"], 1);
}

#[test]
fn l0_recall_fixture_preserves_real_sources_and_orders_by_retrieval_signal() {
    let recall = serde_json::json!({
        "results": {
            "decision": [
                {"id": 12, "content": "Highest duplicate", "timestamp": 100, "relevance": [0.95, 0.1]},
                {"id": 12, "content": "Lower duplicate", "timestamp": 100, "relevance": [0.1, 0.1]},
                {"id": 13, "content": "Second relevance", "timestamp": 101, "relevance": [0.9, 0.2]},
                {"id": 14, "timestamp": 102, "relevance": [1.0, 0.0]}
            ]
        }
    });

    let evidence = recalled_drive_evidence(&recall);

    assert_eq!(evidence.len(), 2);
    assert_eq!(evidence[0].id, 12);
    assert_eq!(evidence[0].content, "Highest duplicate");
    assert_eq!(evidence[1].id, 13);
    let summary = local_exploration_summary("question", &evidence, &[]).unwrap();
    assert!(summary.contains("[#13]"));
    assert!(summary.contains("[#12]"));
    assert!(summary.contains("not independent verification"));
}

#[test]
fn sparse_or_conflicting_recall_never_becomes_an_answer_memory() {
    assert_eq!(
        local_exploration_summary("question", &[], &[]),
        Err(ExploreBlockReason::NoCurrentEvidence)
    );
    let one_source = vec![RecalledDriveEvidence {
        id: 42,
        content: "One related source".into(),
        timestamp: 100,
        relevance: 0.1,
    }];
    let summary = local_exploration_summary("question", &one_source, &[]).unwrap();
    assert!(summary.contains("One related memory only"));
    assert!(summary.contains("[#42]"));
    assert_eq!(
        local_exploration_summary("question", &one_source, &[42, 43]),
        Err(ExploreBlockReason::ConflictingEvidence)
    );
    assert!(recalled_drive_evidence(&serde_json::json!({"results": null})).is_empty());
}

#[test]
fn single_flight_gate_skips_overlapping_work_and_reopens_after_completion() {
    const WORKERS: usize = 12;

    let gate = Arc::new(SingleFlightGate::default());
    let barrier = Arc::new(std::sync::Barrier::new(WORKERS + 1));
    let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let peak_active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let workers: Vec<_> = (0..WORKERS)
        .map(|_| {
            let gate = Arc::clone(&gate);
            let barrier = Arc::clone(&barrier);
            let active = Arc::clone(&active);
            let peak_active = Arc::clone(&peak_active);
            std::thread::spawn(move || {
                barrier.wait();
                let permit = gate.try_acquire();
                if permit.is_some() {
                    let current = active.fetch_add(1, Ordering::Relaxed) + 1;
                    peak_active.fetch_max(current, Ordering::Relaxed);
                }
                barrier.wait();
                if permit.is_some() {
                    active.fetch_sub(1, Ordering::Relaxed);
                }
                drop(permit);
            })
        })
        .collect();

    barrier.wait();
    barrier.wait();
    for worker in workers {
        worker.join().expect("worker should finish");
    }

    assert_eq!(peak_active.load(Ordering::Relaxed), 1);
    assert_eq!(active.load(Ordering::Relaxed), 0);
    assert!(gate.try_acquire().is_some());
}

fn add_tetra_to_space(space: &Space, core: Point3, content: &str, labels: Vec<String>) -> TetraId {
    let positions = Tetrahedron::compute_vertices(core);
    let data = MemoryPayload {
        content: content.to_string(),
        content_hash: content.len() as u64,
        labels,
        timestamp: 0,
        aliases: vec![],
        embedding: vec![],
        importance: 1.0,
        enforced: false,
        rationale: None,
        access_count: 0,
        memory_type: None,
        identity_stamp: None,
        source_agent: None,
        valid_from: 0,
        valid_to: None,
        last_reviewed_ts: None,
        expired_at: None,
        invalidated_at: None,
        memory_class: None,
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

/// Build a scheduler with all real subsystems, cognitive disabled (no API key).
fn build_scheduler() -> (Arc<SchedulerCenter>, Arc<Space>, Arc<KnowledgeGraph>) {
    build_scheduler_with_cognitive(Arc::new(CognitiveEngine::new("", "")))
}

fn build_scheduler_with_cognitive(
    cognitive: Arc<CognitiveEngine>,
) -> (Arc<SchedulerCenter>, Arc<Space>, Arc<KnowledgeGraph>) {
    let space = Arc::new(Space::new());
    let embedding = Arc::new(EmbeddingService::from_env());
    let storage =
        Arc::new(StorageManager::new(std::path::Path::new("test_data_scheduler")).unwrap());
    build_scheduler_with_services(space, cognitive, embedding, storage)
}

fn build_scheduler_with_services(
    space: Arc<Space>,
    cognitive: Arc<CognitiveEngine>,
    embedding: Arc<EmbeddingService>,
    storage: Arc<StorageManager>,
) -> (Arc<SchedulerCenter>, Arc<Space>, Arc<KnowledgeGraph>) {
    let bus = super::super::bus::EventBus::new(64);
    let tx = bus.sender();
    let rx = bus.subscribe();
    let energy = Arc::new(EnergyCenter::new(10000.0, 8.0, tx.clone(), bus.subscribe()));
    let knowledge = Arc::new(KnowledgeGraph::new());
    let classifier = Arc::new(CategoryClassifier::new("", ""));
    let gateway = Arc::new(GatewayCenter::new(
        space.clone(),
        energy.clone(),
        cognitive.clone(),
        classifier.clone(),
        tx.clone(),
        bus.subscribe(),
        knowledge.clone(),
        embedding.clone(),
        None,
    ));
    let security = Arc::new(SecurityGuard::from_env());
    let scheduler = Arc::new(SchedulerCenter::with_security(
        space.clone(),
        energy.clone(),
        knowledge.clone(),
        cognitive.clone(),
        gateway.clone(),
        tx,
        rx,
        1000,
        10000.0,
        security,
        storage,
    ));
    (scheduler, space, knowledge)
}

#[test]
fn memory_improve_preserves_content_refreshes_indexes_and_persists() {
    let test_dir = MemoryImproveTestDir::new();
    let server = MemoryImproveMockServer::start(format!(
        "Improved context: {} tail: 重要🧠",
        "y".repeat(180)
    ));
    let original = format!(
        "{} and the original tail is important: 重要🧠",
        "x".repeat(120)
    );
    let generated = format!("Improved context: {} tail: 重要🧠", "y".repeat(180));
    let expected_content = format!(
        "{}\n[improved from: {}]",
        generated,
        original.chars().take(60).collect::<String>()
    );
    let expected_hash = super::super::search_engine::hash_content(&expected_content);
    let mut expected_embedding = vec![0.0; super::super::vector::EMBEDDING_DIM];
    expected_embedding[1] = 1.0;

    let space = Arc::new(Space::new());
    let memory_id = add_tetra_to_space(&space, Point3::new(10.0, 0.0, 0.0), &original, vec![]);
    let mut original_payload = space.get_tetrahedron(memory_id).unwrap().data;
    original_payload.content_hash = super::super::search_engine::hash_content(&original);
    original_payload.embedding = {
        let mut embedding = vec![0.0; super::super::vector::EMBEDDING_DIM];
        embedding[0] = 1.0;
        embedding
    };
    space.update_payload(memory_id, original_payload).unwrap();

    let protected_content = "short enforced memory";
    let protected_id = add_tetra_to_space(
        &space,
        Point3::new(20.0, 0.0, 0.0),
        protected_content,
        vec![],
    );
    let mut protected_payload = space.get_tetrahedron(protected_id).unwrap().data;
    protected_payload.enforced = true;
    space
        .update_payload(protected_id, protected_payload)
        .unwrap();

    let storage = Arc::new(StorageManager::new(&test_dir.0).unwrap());
    storage.save_space_only(&space).unwrap();
    let cognitive = Arc::new(CognitiveEngine::with_base(
        "test-key",
        "test-model",
        &server.base_url,
    ));
    let embedding = Arc::new(EmbeddingService::with_api_url_for_test(&format!(
        "{}/api/embed",
        server.base_url
    )));
    let (scheduler, _, knowledge) =
        build_scheduler_with_services(space.clone(), cognitive, embedding, storage.clone());

    let result = scheduler.api_improve_memory(10);
    let requests = server.requests();
    let prompt_contains_full_source = requests
        .iter()
        .find(|(path, _)| path == "/v1/chat/completions")
        .and_then(|(_, request)| request["messages"][1]["content"].as_str())
        .is_some_and(|prompt| prompt.contains(&original));
    let improved_once =
        result["candidates"].as_u64() == Some(1) && result["improved"].as_u64() == Some(1);

    let updated = space.get_tetrahedron(memory_id).unwrap();
    let full_content_written = updated.data.content == expected_content;
    let hash_refreshed = updated.data.content_hash == expected_hash;
    let embedding_refreshed = updated.data.embedding == expected_embedding;
    let protected = space.get_tetrahedron(protected_id).unwrap();
    let enforcement_preserved = protected.data.enforced
        && protected.data.content == protected_content
        && protected.data.embedding.is_empty();
    let search_results = scheduler
        .gateway_handle()
        .search_vector_only(&expected_content, 1)
        .unwrap();
    let hnsw_refreshed = search_results
        .first()
        .is_some_and(|(id, score, _, _)| *id == memory_id && (*score - 1.0).abs() < 1e-9);

    drop(scheduler);
    drop(space);
    drop(knowledge);
    drop(storage);

    let reloaded_storage = StorageManager::new(&test_dir.0).unwrap();
    let reloaded_space = Space::new();
    let reloaded_knowledge = KnowledgeGraph::new();
    let load_report = reloaded_storage.load_all(&reloaded_space, &reloaded_knowledge);
    let persisted = reloaded_space.get_tetrahedron(memory_id);
    let persistence_survives_restart = load_report.space_ok
        && persisted.as_ref().is_some_and(|memory| {
            memory.data.content == expected_content
                && memory.data.content_hash == expected_hash
                && memory.data.embedding == expected_embedding
        });
    let protected_survives_restart = reloaded_space
        .get_tetrahedron(protected_id)
        .is_some_and(|memory| memory.data.enforced && memory.data.content == protected_content);

    assert!(
            improved_once
                && prompt_contains_full_source
                && full_content_written
                && hash_refreshed
                && embedding_refreshed
                && hnsw_refreshed
                && persistence_survives_restart
                && enforcement_preserved
                && protected_survives_restart,
            "memory_improve regression: improved={improved_once}, full_source={prompt_contains_full_source}, full_content={full_content_written}, hash={hash_refreshed}, embedding={embedding_refreshed}, HNSW={hnsw_refreshed}, restart={persistence_survives_restart}, enforced={enforcement_preserved}, enforced_after_restart={protected_survives_restart}"
        );
}

#[test]
fn memory_improve_rejects_rewrites_at_the_existing_size_limit() {
    let test_dir = MemoryImproveTestDir::new();
    let server = MemoryImproveMockServer::start("x".repeat(300));
    let space = Arc::new(Space::new());
    let memory_id = add_tetra_to_space(&space, Point3::new(10.0, 0.0, 0.0), "short memory", vec![]);
    let before = space.get_tetrahedron(memory_id).unwrap();
    let storage = Arc::new(StorageManager::new(&test_dir.0).unwrap());
    let cognitive = Arc::new(CognitiveEngine::with_base(
        "test-key",
        "test-model",
        &server.base_url,
    ));
    let embedding = Arc::new(EmbeddingService::with_api_url_for_test(&format!(
        "{}/api/embed",
        server.base_url
    )));
    let (scheduler, _, _) =
        build_scheduler_with_services(space.clone(), cognitive, embedding, storage);

    let result = scheduler.api_improve_memory(1);
    let after = space.get_tetrahedron(memory_id).unwrap();

    assert_eq!(result["candidates"].as_u64(), Some(1));
    assert_eq!(result["improved"].as_u64(), Some(0));
    assert_eq!(after.data.content, before.data.content);
    assert_eq!(after.data.content_hash, before.data.content_hash);
    assert_eq!(after.data.embedding, before.data.embedding);
}

/// Scenario: Seed a space with N tetrahedrons forming multiple clusters.
/// Spacing must be exactly EDGE_LENGTH (1.0) for vertices to merge and form clusters.
fn seed_reality(space: &Space) -> Vec<TetraId> {
    let mut ids = Vec::new();
    // Cluster A: physics-related, placed in a chain at EDGE_LENGTH spacing
    let physics_topics = [
        (
            "Quantum mechanics wave function",
            vec!["physics".into(), "quantum".into()],
        ),
        (
            "General relativity spacetime",
            vec!["physics".into(), "relativity".into()],
        ),
        (
            "Thermodynamics entropy",
            vec!["physics".into(), "thermo".into()],
        ),
        (
            "Electromagnetic field equations",
            vec!["physics".into(), "em".into()],
        ),
        (
            "Particle physics standard model",
            vec!["physics".into(), "quantum".into()],
        ),
        (
            "String theory extra dimensions",
            vec!["physics".into(), "quantum".into()],
        ),
        (
            "Statistical mechanics ensemble",
            vec!["physics".into(), "thermo".into()],
        ),
    ];
    for (i, (text, labels)) in physics_topics.iter().enumerate() {
        let core = Point3::new(i as f64 * EDGE_LENGTH, 0.0, 0.0);
        ids.push(add_tetra_to_space(space, core, text, labels.clone()));
    }

    // Cluster B: programming, placed in a separate region
    let prog_topics = [
        (
            "Rust ownership and borrowing",
            vec!["rust".into(), "programming".into()],
        ),
        (
            "Python async await patterns",
            vec!["python".into(), "programming".into()],
        ),
        (
            "C++ template metaprogramming",
            vec!["cpp".into(), "programming".into()],
        ),
        (
            "Go goroutines and channels",
            vec!["go".into(), "programming".into()],
        ),
        (
            "JavaScript event loop",
            vec!["js".into(), "programming".into()],
        ),
        (
            "Haskell monad transformers",
            vec!["haskell".into(), "programming".into()],
        ),
        (
            "TypeScript type inference",
            vec!["ts".into(), "programming".into()],
        ),
    ];
    for (i, (text, labels)) in prog_topics.iter().enumerate() {
        let core = Point3::new(20.0 + i as f64 * EDGE_LENGTH, 0.0, 0.0);
        ids.push(add_tetra_to_space(space, core, text, labels.clone()));
    }

    // Cluster C: mixed topics — high entropy, designed to trigger fission
    let mixed = [
        (
            "Neural network backpropagation",
            vec!["ai".into(), "ml".into()],
        ),
        ("Shakespeare sonnet analysis", vec!["literature".into()]),
        (
            "Climate change carbon cycle",
            vec!["science".into(), "climate".into()],
        ),
        ("Bach fugue counterpoint", vec!["music".into()]),
        ("Roman empire military tactics", vec!["history".into()]),
        ("Recipe for chocolate cake", vec!["cooking".into()]),
        ("Proof of Fermat last theorem", vec!["math".into()]),
    ];
    for (i, (text, labels)) in mixed.iter().enumerate() {
        let core = Point3::new(40.0 + i as f64 * EDGE_LENGTH, 0.0, 0.0);
        ids.push(add_tetra_to_space(space, core, text, labels.clone()));
    }

    ids
}

#[test]
fn api_search_scored_routes_auto_and_fusion_through_mode_specific_search() {
    let (scheduler, space, _knowledge) = build_scheduler();
    seed_reality(&space);

    for mode in [
        super::super::search_engine::SearchMode::Auto,
        super::super::search_engine::SearchMode::Fusion,
    ] {
        let filters = super::super::search_engine::SearchFilters {
            mode,
            ..Default::default()
        };
        let (results, notes) = scheduler
            .api_search_scored(
                "what happened before quantum wave function",
                5,
                Some(&filters),
            )
            .expect("mode-specific search should succeed");
        assert!(
            !results.is_empty(),
            "{mode:?} should return fixture matches"
        );
        assert!(
            !notes.matched_by_map.is_empty(),
            "{mode:?} should preserve the sources reported by search_with_mode"
        );
    }
}

// ---- Test: TickSnapshot correctly captures space state ----

#[test]
fn snapshot_matches_space_state() {
    let (sched, space, _kg) = build_scheduler();
    seed_reality(&space);

    let snap = sched.build_snapshot();

    assert_eq!(snap.tetras.len(), 21, "21 tetras seeded");
    assert_eq!(snap.clusters.len(), 3, "3 clusters expected");

    // labels_map must cover every tetra
    for t in &snap.tetras {
        assert!(
            snap.labels_map.contains_key(&t.id),
            "labels_map missing tetra {}",
            t.id
        );
        assert!(
            snap.core_map.contains_key(&t.id),
            "core_map missing tetra {}",
            t.id
        );
    }

    // Verify cluster membership covers all tetras
    let clustered: HashSet<u64> = snap
        .clusters
        .iter()
        .flat_map(|c| c.tetra_ids.iter().copied())
        .collect();
    assert_eq!(clustered.len(), 21, "all 21 tetras should be in a cluster");
}

// ---- Test: collect_state_from_snap produces correct metrics ----

#[test]
fn collect_state_accurate_metrics() {
    let (sched, space, _kg) = build_scheduler();
    seed_reality(&space);

    let snap = sched.build_snapshot();
    let state = sched.collect_state_from_snap(&snap);

    assert_eq!(state.total_tetras, 21);
    assert_eq!(state.total_clusters, 3);

    // Cluster 0 (physics, 7 tetras) should have lower entropy than mixed cluster
    assert!(
        state.clusters[0].entropy < 0.7,
        "physics cluster should be moderately cohesive, entropy={}",
        state.clusters[0].entropy
    );

    // Cluster 2 (mixed, 7 tetras) should have the highest entropy (all different labels)
    assert!(
        state.clusters[2].entropy >= state.clusters[0].entropy,
        "mixed cluster entropy ({}) >= physics cluster entropy ({})",
        state.clusters[2].entropy,
        state.clusters[0].entropy
    );

    // Memory info should have correct cluster assignments
    let physics_memories: Vec<_> = state
        .memories
        .iter()
        .filter(|m| m.labels.contains(&"physics".to_string()))
        .collect();
    assert_eq!(physics_memories.len(), 7);
    assert!(physics_memories
        .iter()
        .all(|m| m.cluster_index == 0 || m.cluster_index < 3));

    // Energy should match
    assert!(
        state.energy > 9000.0,
        "energy should be near max after replenish"
    );
}

// ---- Test: auto_pulse reads from snapshot without crashing ----

#[test]
fn auto_pulse_uses_snapshot() {
    let (sched, space, _kg) = build_scheduler();
    seed_reality(&space);

    // Manually tick to set tick_count > 0
    sched.tick_count.fetch_add(1, Ordering::SeqCst);
    sched.energy.replenish(100.0);

    let snap = sched.build_snapshot();
    sched.auto_pulse(&snap);

    // Verify pulse actually ran — should not panic, clusters should still be valid
    let clusters = space.find_clusters();
    assert!(
        clusters.len() >= 3,
        "clusters should remain intact after pulse"
    );
}

// ---- Test: auto_fission skips low-entropy clusters ----

#[test]
fn auto_fission_skips_cohesive_clusters() {
    let (sched, space, _kg) = build_scheduler();
    // Only seed a cohesive cluster — all same label
    for i in 0..7 {
        let core = Point3::new(i as f64 * EDGE_LENGTH, 0.0, 0.0);
        add_tetra_to_space(
            &space,
            core,
            &format!("physics topic {}", i),
            vec!["physics".into()],
        );
    }

    sched.tick_count.fetch_add(20, Ordering::SeqCst);
    let snap = sched.build_snapshot();
    sched.auto_fission(&snap);

    let clusters = space.find_clusters();
    assert_eq!(clusters.len(), 1, "cohesive cluster should NOT be split");
}

// ---- Test: auto_fission splits high-entropy clusters ----

#[test]
fn auto_fission_splits_diverse_cluster() {
    let (sched, space, _kg) = build_scheduler();
    // Seed a cluster with completely different labels, placed at EDGE_LENGTH spacing
    let topics = [
        ("Topic A", vec!["alpha".into()]),
        ("Topic B", vec!["beta".into()]),
        ("Topic C", vec!["gamma".into()]),
        ("Topic D", vec!["delta".into()]),
        ("Topic E", vec!["epsilon".into()]),
        ("Topic F", vec!["zeta".into()]),
        ("Topic G", vec!["eta".into()]),
        ("Topic H", vec!["theta".into()]),
    ];
    for (i, (text, labels)) in topics.iter().enumerate() {
        let core = Point3::new(i as f64 * EDGE_LENGTH, 0.0, 0.0);
        add_tetra_to_space(&space, core, text, labels.clone());
    }

    // Must have enough ticks to pass cooldown
    sched.tick_count.fetch_add(20, Ordering::SeqCst);
    let snap = sched.build_snapshot();

    assert_eq!(snap.clusters.len(), 1, "should start as one cluster");
    let entropy =
        dynamics::compute_entropy_from_labels(&snap.clusters[0].tetra_ids, &snap.labels_map);
    assert!(
        entropy > 0.5,
        "diverse labels should have high entropy, got {}",
        entropy
    );

    sched.auto_fission(&snap);

    // After fission, tetras should have been relocated — at least some positions changed
    let after_tetras = space.all_tetrahedrons();
    let unique_x: HashSet<i64> = after_tetras
        .iter()
        .map(|t| (t.core.x * 10.0) as i64)
        .collect();
    // With 8 completely different topics, some should have been pushed apart
    assert!(
        unique_x.len() > 1,
        "fission should relocate minority tetras to new positions, got {} unique x positions",
        unique_x.len()
    );
}

// ---- Test: perform_fission_from_snap uses snapshot data ----

#[test]
fn fission_from_snap_no_extra_locks() {
    let (sched, space, _kg) = build_scheduler();
    // Create a diverse cluster at EDGE_LENGTH spacing
    for i in 0..8 {
        let label = format!("label-{}", i);
        let core = Point3::new(i as f64 * EDGE_LENGTH, 0.0, 0.0);
        add_tetra_to_space(&space, core, &format!("content {}", i), vec![label]);
    }

    sched.tick_count.fetch_add(20, Ordering::SeqCst);
    let snap = sched.build_snapshot();

    let result = sched.perform_fission_from_snap(0, 0, 8.0, "TestFission", &snap);
    assert!(result, "fission should succeed on diverse cluster");

    // Should have split — at minimum tetras are now in different spatial positions
    let after_tetras = space.all_tetrahedrons();
    assert!(after_tetras.len() == 8, "no tetras lost");
}

// ---- Test: Full tick cycle with snapshot (tick_and_maybe_think) ----

#[test]
fn full_tick_cycle_consistent() {
    let (sched, space, _kg) = build_scheduler();
    seed_reality(&space);

    // Simulate 25 ticks (covers tick%5 and tick%10 paths)
    for _ in 0..25 {
        let _ = sched.tick_and_maybe_think();
    }

    let final_tetras = space.all_tetrahedrons();
    assert_eq!(final_tetras.len(), 21, "no tetras lost during 25 ticks");

    let clusters = space.find_clusters();
    assert!(!clusters.is_empty(), "clusters should still exist");

    // Verify mass has been updated (auto_pulse adds mass) or at minimum no data loss
    let total_mass: f64 = final_tetras.iter().map(|t| t.mass).sum();
    assert!(
        total_mass >= 21.0,
        "total mass should be at least 21.0 (initial), got {}",
        total_mass
    );

    // Verify decision history was recorded (tick%5 triggers cognitive path)
    let state = sched.collect_state_internal();
    assert_eq!(state.tick, 25);
    assert!(state.total_tetras >= 21);
}

// ---- Test: Memory creation through API path ----

#[test]
fn api_create_memory_integration() {
    let (sched, space, _kg) = build_scheduler();

    let (id1, _) = sched
        .api_create_memory("Rust ownership model", vec!["rust".into()])
        .unwrap();
    let (id2, _) = sched
        .api_create_memory("Python list comprehension", vec!["python".into()])
        .unwrap();
    let (id3, _) = sched
        .api_create_memory("Rust trait objects", vec!["rust".into()])
        .unwrap();

    assert!(id1 != id2 && id2 != id3, "IDs should be unique");

    let tetras = space.all_tetrahedrons();
    assert_eq!(tetras.len(), 3);

    // Rust memories should be close to each other (same label → nearby placement)
    let rust_tetras: Vec<&Tetrahedron> = tetras
        .iter()
        .filter(|t| t.data.labels.contains(&"rust".to_string()))
        .collect();
    assert_eq!(rust_tetras.len(), 2);
    let dx = (rust_tetras[0].core.x - rust_tetras[1].core.x).abs();
    assert!(
        dx < 5.0,
        "same-label memories should be placed nearby, dx={}",
        dx
    );
}

// ---- Test: Multiple fission rounds don't corrupt state ----

#[test]
fn repeated_fission_stable() {
    let (sched, space, _kg) = build_scheduler();

    // Seed a large diverse cluster
    for i in 0..20 {
        let label = format!("label-{}", i % 5);
        let core = Point3::new(i as f64 * EDGE_LENGTH, 0.0, 0.0);
        add_tetra_to_space(&space, core, &format!("content {}", i), vec![label]);
    }

    // Run 50 ticks to trigger multiple fission cycles
    for _ in 0..50 {
        let _ = sched.tick_and_maybe_think();
    }

    let final_tetras = space.all_tetrahedrons();
    assert_eq!(
        final_tetras.len(),
        20,
        "no tetras lost after 50 ticks with fission"
    );

    // Verify all tetras have valid positions (no NaN, no extreme values)
    for t in &final_tetras {
        assert!(
            t.core.x.is_finite(),
            "x should be finite for tetra {}",
            t.id
        );
        assert!(
            t.core.y.is_finite(),
            "y should be finite for tetra {}",
            t.id
        );
        assert!(
            t.core.z.is_finite(),
            "z should be finite for tetra {}",
            t.id
        );
        assert!(t.mass > 0.0, "mass should be positive for tetra {}", t.id);
    }
}

// ---- Test: Snapshot data consistency under concurrent reads ----

#[test]
fn snapshot_is_consistent_view() {
    let (sched, space, _kg) = build_scheduler();
    seed_reality(&space);

    let snap = sched.build_snapshot();

    // Verify snapshot internal consistency: labels_map matches tetras
    for t in &snap.tetras {
        let snap_labels = snap.labels_map.get(&t.id).unwrap();
        assert_eq!(
            snap_labels, &t.labels,
            "labels_map mismatch for tetra {}",
            t.id
        );

        let snap_core = snap.core_map.get(&t.id).unwrap();
        assert!(
            (snap_core.x - t.core.x).abs() < 1e-10,
            "core_map x mismatch for tetra {}",
            t.id
        );
        assert!(
            (snap_core.y - t.core.y).abs() < 1e-10,
            "core_map y mismatch for tetra {}",
            t.id
        );
        assert!(
            (snap_core.z - t.core.z).abs() < 1e-10,
            "core_map z mismatch for tetra {}",
            t.id
        );
    }

    // Verify cluster membership: all cluster tetra IDs exist in tetras
    let all_ids: HashSet<u64> = snap.tetras.iter().map(|t| t.id).collect();
    for cluster in &snap.clusters {
        for &id in &cluster.tetra_ids {
            assert!(
                all_ids.contains(&id),
                "cluster references non-existent tetra {}",
                id
            );
        }
    }
}

// ---- Test: generate_aliases doesn't crash with snapshot ----

/// Reproduction: the same first 10 cold memories were re-sampled on every
/// review, so one KEEP verdict was applied again and again (importance
/// ratcheted to the 3.0 cap) and the remaining cold memories were never seen.
#[test]
fn cold_review_applies_each_verdict_once_and_covers_all_cold_memories() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let stop = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let server = {
        let (stop, calls) = (Arc::clone(&stop), Arc::clone(&calls));
        std::thread::spawn(move || {
            let verdicts: String = (0..64).map(|i| format!("#{i}:KEEP\\n")).collect();
            let body = format!(r#"{{"choices":[{{"message":{{"content":"{verdicts}"}}}}]}}"#);
            while !stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        calls.fetch_add(1, Ordering::Relaxed);
                        let _ = stream.set_nonblocking(false);
                        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
                        let mut request = [0u8; 65536];
                        let _ = stream.read(&mut request);
                        let response = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                body.len(),
                                body
                            );
                        let _ = stream.write_all(response.as_bytes());
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(2));
                    }
                    Err(e) => panic!("model fixture failed: {e}"),
                }
            }
        })
    };
    let cognitive = Arc::new(CognitiveEngine::with_base(
        "test-key",
        "test-model",
        &base_url,
    ));
    let (scheduler, space, _) = build_scheduler_with_cognitive(cognitive);
    let ids: Vec<TetraId> = (0..30)
        .map(|i| {
            add_tetra_to_space(
                &space,
                Point3 {
                    x: 20.0 * i as f64,
                    y: 7.0,
                    z: 3.0,
                },
                &format!("cold memory {i}"),
                vec!["note".into()],
            )
        })
        .collect();
    let rounds = 10;
    let reviewed: usize = (0..rounds).map(|_| scheduler.review_cold_memories()).sum();
    stop.store(true, Ordering::Relaxed);
    server.join().unwrap();

    let importances: Vec<f64> = ids
        .iter()
        .map(|id| space.get_tetrahedron(*id).unwrap().data.importance)
        .collect();
    let max = importances.iter().cloned().fold(0.0, f64::max);
    let covered = importances.iter().filter(|v| **v > 1.0).count();
    eprintln!(
            "R11COLD rounds={rounds} model_calls={} reviewed={reviewed} covered={covered}/30 max_importance={max:.2}",
            calls.load(Ordering::Relaxed)
        );
    assert_eq!(covered, 30, "every cold memory reviewed once");
    assert!(
        (max - 1.3).abs() < 1e-9,
        "one KEEP verdict = one +0.3 boost"
    );
    assert_eq!(reviewed, 30);
    assert_eq!(
        calls.load(Ordering::Relaxed),
        3,
        "no model call once nothing is due"
    );
}

/// Reproduction: personality export/import used to drop every temporal and
/// provenance field — imported memories were re-dated to "now", cut to a
/// 150-char preview, and lost their validity window, class and source agent.
#[test]
fn personality_round_trip_preserves_memory_time_and_provenance() {
    let base = std::env::temp_dir().join(format!("epicode-persona-{}", uuid::Uuid::new_v4()));
    let src = crate::engine::Engine::with_data_dir(base.join("src"));
    let body = format!(
        "Decision record: {}",
        "we keep sqlite for local durability because it survives power loss. ".repeat(5)
    );
    let ts = 1_735_689_600; // 2025-01-01T00:00:00Z
    let (id, _) = src
        .scheduler
        .api_create_memory_with_time(&body, vec!["decision".into()], ts)
        .unwrap();
    let mut d = src.space.get_tetrahedron(id).unwrap().data.clone();
    d.importance = 2.5;
    d.valid_from = ts;
    d.valid_to = Some(ts + 30 * 86_400);
    d.invalidated_at = Some(ts + 31 * 86_400);
    d.memory_class = Some("permanent".into());
    d.source_agent = Some("agent-alpha".into());
    src.space.update_payload(id, d).unwrap();
    let pkg = src.scheduler.api_export_personality().unwrap();

    let dst = crate::engine::Engine::with_data_dir(base.join("dst"));
    let result = dst.scheduler.api_import_personality(&pkg).unwrap();
    assert_eq!(result["core_memories_restored"], 1);
    let (_, got) = dst
        .space
        .all_tetras_meta()
        .into_iter()
        .filter_map(|m| dst.space.get_tetrahedron(m.id).map(|t| (m.id, t.data)))
        .find(|(_, p)| p.labels.iter().any(|l| l == "imported"))
        .unwrap();
    assert_eq!(got.timestamp, ts);
    assert_eq!(got.valid_from, ts);
    assert_eq!(got.valid_to, Some(ts + 30 * 86_400));
    assert_eq!(got.invalidated_at, Some(ts + 31 * 86_400));
    assert_eq!(got.memory_class.as_deref(), Some("permanent"));
    assert_eq!(got.source_agent.as_deref(), Some("agent-alpha"));
    assert_eq!(got.content, format!("[imported] {}", body.trim()));
    assert!(got.labels.iter().any(|l| l == "decision"));
    assert!(!got.enforced);

    // a second hop keeps one "[imported]" prefix and the original time
    let again = ImportedMemory::from_export(
        &dst.scheduler.api_export_personality().unwrap()["core_memories"][0],
        chrono::Utc::now().timestamp(),
    )
    .unwrap();
    assert_eq!(again.content, got.content);
    assert_eq!(again.timestamp, ts);
    drop((src, dst));
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn imported_memory_sanitizes_legacy_and_hostile_entries() {
    let now = 2_000_000_000;
    // legacy 1.0 entry: preview only, no time → dated at import time
    let legacy = ImportedMemory::from_export(
        &serde_json::json!({"preview": "[imported] [imported] old fact", "importance": 9.0,
                                "labels": ["quarantine", "enforced", "topic"]}),
        now,
    )
    .unwrap();
    assert_eq!(legacy.content, "[imported] old fact");
    assert_eq!(legacy.timestamp, now);
    assert_eq!(legacy.valid_from, now);
    assert_eq!(legacy.importance, Some(3.0));
    assert_eq!(legacy.labels, vec!["topic", "imported", "l0-exempt"]);
    // future / negative / inverted times are not trusted
    let hostile = ImportedMemory::from_export(
        &serde_json::json!({"content": "x fact", "timestamp": now + 86_400,
                                "valid_from": 1_000, "valid_to": 10, "memory_class": "god",
                                "importance": f64::NAN}),
        now,
    )
    .unwrap();
    assert_eq!(hostile.timestamp, 1_000);
    assert_eq!(hostile.valid_from, 1_000);
    assert_eq!(hostile.valid_to, None);
    assert_eq!(hostile.memory_class, None);
    assert_eq!(hostile.importance, None);
    assert!(
        ImportedMemory::from_export(&serde_json::json!({"content": "[imported] "}), now).is_none()
    );
    assert!(ImportedMemory::from_export(&serde_json::json!({"labels": []}), now).is_none());
}

#[test]
fn personality_import_reserves_quota_per_new_memory() {
    let dir = std::env::temp_dir().join(format!("epicode-persona-q-{}", uuid::Uuid::new_v4()));
    let engine = crate::engine::Engine::with_data_dir(dir.clone());
    let pkg = serde_json::json!({
        "format": "epicode-personality/1.0",
        "core_memories": [
            {"preview": "alpha fact about the deployment pipeline"},
            {"preview": "beta fact about the backup schedule"},
            {"preview": "alpha fact about the deployment pipeline"},
            {"preview": "gamma fact about the incident runbook"},
        ]
    });
    let budget = 2usize;
    let used = std::cell::Cell::new(0usize);
    let mut reserve = || {
        if used.get() >= budget {
            return Err("memory limit reached".to_string());
        }
        used.set(used.get() + 1);
        Ok(())
    };
    let mut release = || used.set(used.get() - 1);
    let r = engine
        .scheduler
        .api_import_personality_with_quota(&pkg, &mut reserve, &mut release)
        .unwrap();
    let restored = r["core_memories_restored"].as_u64().unwrap() as usize;
    let deduped = r["core_memories_deduped"].as_u64().unwrap() as usize;
    let over = r["core_memories_over_quota"].as_u64().unwrap() as usize;
    assert_eq!(
        restored,
        used.get(),
        "quota charged exactly for new memories"
    );
    assert!(restored <= budget);
    assert_eq!(restored + deduped + over, 4);
    assert!(over >= 1);
    drop(engine);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn generate_aliases_with_snapshot() {
    let (sched, space, _kg) = build_scheduler();
    seed_reality(&space);

    let snap = sched.build_snapshot();
    // generate_aliases with cognitive disabled should be a no-op
    sched.generate_aliases(0, &snap);

    let tetras = space.all_tetrahedrons();
    assert_eq!(tetras.len(), 21, "no tetras lost");
}

// ---- Test: reclassify_memories with snapshot ----

#[test]
fn reclassify_memories_with_snapshot() {
    let (sched, space, _kg) = build_scheduler();
    seed_reality(&space);

    let snap = sched.build_snapshot();
    // reclassify with cognitive disabled should be a no-op
    sched.reclassify_memories(0, &snap);

    let tetras = space.all_tetrahedrons();
    assert_eq!(tetras.len(), 21, "no tetras lost");
}

// ---- Test: Tick snapshot entropy matches per-cluster computation ----

#[test]
fn snapshot_entropy_accuracy() {
    let (sched, space, _kg) = build_scheduler();
    seed_reality(&space);

    let snap = sched.build_snapshot();

    for cluster in &snap.clusters {
        let snap_entropy =
            dynamics::compute_entropy_from_labels(&cluster.tetra_ids, &snap.labels_map);
        // Compute "ground truth" entropy by reading from space directly
        let ground_truth = dynamics::compute_entropy(&space, cluster);
        let diff = (snap_entropy - ground_truth).abs();
        assert!(
            diff < 1e-10,
            "snapshot entropy ({}) should match ground truth ({}) for cluster with {} tetras",
            snap_entropy,
            ground_truth,
            cluster.tetra_ids.len()
        );
    }
}

// ---- Test: tick observability (missed commits) ----

#[test]
fn horizon_observability_tracks_missed_commits() {
    let (sched, _space, _kg) = build_scheduler();
    let (phase, _p, debt, missed) = sched.horizon_observability();
    assert_eq!(missed, 0);
    assert_eq!(debt, 0);
    assert!(!phase.is_empty());
    sched.horizon.lock().note_defer();
    let (_phase2, _p2, debt2, missed2) = sched.horizon_observability();
    assert_eq!(missed2, 1);
    assert_eq!(debt2, 1);
}

// ---- Test: api_dream energy gate (R4-S04) ----

#[test]
fn api_dream_consumes_energy_and_dry_run_does_not() {
    let (sched, _space, _kg) = build_scheduler();
    let before = sched.energy.available();
    assert!(before >= 15.0);
    sched.api_dream(true).expect("dry_run dream");
    assert!(
        (sched.energy.available() - before).abs() < 0.01,
        "dry_run must not consume energy"
    );
    sched.api_dream(false).expect("dream with energy");
    let after = sched.energy.available();
    assert!(
        (before - after - 15.0).abs() < 0.01,
        "api_dream should consume 15.0 energy (before={before}, after={after})"
    );
}

#[test]
fn api_dream_rejects_when_energy_insufficient() {
    let (sched, _space, _kg) = build_scheduler();
    let available = sched.energy.available();
    if available > 14.0 {
        assert!(
            sched.energy.consume(available - 14.0),
            "should drain to just under dream cost"
        );
    }
    assert!(sched.energy.available() < 15.0);
    let err = sched.api_dream(false).expect_err("must reject");
    assert!(err.contains("insufficient energy"), "got: {err}");
    // Failed attempt must not further drain the remaining budget.
    assert!((sched.energy.available() - 14.0).abs() < 0.01);
}

#[test]
fn api_dream_is_single_flight_with_other_dream_paths() {
    let (sched, _space, _kg) = build_scheduler();
    let before = sched.energy.available();
    // 模拟 auto_dream / LLM Dream 正在进行
    let held = sched.dream_gate.try_acquire().expect("gate free");
    let err = sched
        .api_dream(false)
        .expect_err("must refuse concurrent dream");
    assert!(err.contains("already running"), "got: {err}");
    assert!(
        (sched.energy.available() - before).abs() < 0.01,
        "refused dream must not consume energy"
    );
    // dry_run 只读,不受闸门限制
    sched.api_dream(true).expect("dry_run allowed");
    drop(held);
    sched.tick_count.store(42, Ordering::SeqCst);
    sched.api_dream(false).expect("dream after gate released");
    assert_eq!(
        sched.last_dream_tick.load(Ordering::SeqCst),
        42,
        "manual dream must reset the auto-dream interval"
    );
    assert!(
        sched.dream_gate.try_acquire().is_some(),
        "gate released after api_dream"
    );
}

// ---- Test: Large-scale scenario (100 memories) ----

#[test]
fn large_scale_100_memories() {
    let (sched, space, _kg) = build_scheduler();

    let categories = [
        "physics",
        "chemistry",
        "biology",
        "math",
        "cs",
        "history",
        "art",
        "music",
    ];
    for i in 0..100 {
        let cat = categories[i % categories.len()];
        // Space in groups: each category gets its own chain
        let cat_idx = (i % categories.len()) as f64;
        let in_chain = (i / categories.len()) as f64;
        let core = Point3::new(cat_idx * 20.0 + in_chain * EDGE_LENGTH, 0.0, 0.0);
        add_tetra_to_space(
            &space,
            core,
            &format!("Memory #{} about {}", i, cat),
            vec![cat.to_string()],
        );
    }

    assert_eq!(space.all_tetrahedrons().len(), 100);

    // Run 10 ticks
    for _ in 0..10 {
        let _ = sched.tick_and_maybe_think();
    }

    let final_tetras = space.all_tetrahedrons();
    assert_eq!(
        final_tetras.len(),
        100,
        "no tetras lost in large-scale test"
    );

    let state = sched.collect_state_internal();
    assert!(
        state.total_clusters >= 1,
        "should have at least 1 cluster with 100 memories"
    );
    assert_eq!(state.total_tetras, 100);
}

/// Local slow model fixture: one thread per connection, sleeps `llm_ms`,
/// tracks total requests, current and peak concurrent requests.
struct SlowModel {
    base_url: String,
    stop: Arc<AtomicBool>,
    requests: Arc<AtomicUsize>,
    in_flight: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
    server: Option<std::thread::JoinHandle<()>>,
}

impl SlowModel {
    fn start(llm_ms: u64) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(AtomicUsize::new(0));
        let in_flight = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let server = {
            let (stop, requests, in_flight, peak) = (
                Arc::clone(&stop),
                Arc::clone(&requests),
                Arc::clone(&in_flight),
                Arc::clone(&peak),
            );
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let (requests, in_flight, peak) =
                                (requests.clone(), in_flight.clone(), peak.clone());
                            std::thread::spawn(move || {
                                requests.fetch_add(1, Ordering::Relaxed);
                                let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                                peak.fetch_max(now, Ordering::SeqCst);
                                let _ = stream.set_nonblocking(false);
                                let _ = stream
                                    .set_read_timeout(Some(std::time::Duration::from_secs(2)));
                                let mut request = [0u8; 65536];
                                let _ = stream.read(&mut request);
                                std::thread::sleep(std::time::Duration::from_millis(llm_ms));
                                let body =
                                    r#"{"choices":[{"message":{"content":"no action needed"}}]}"#;
                                let response = format!(
                                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                        body.len(),
                                        body
                                    );
                                let _ = stream.write_all(response.as_bytes());
                                in_flight.fetch_sub(1, Ordering::SeqCst);
                            });
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(std::time::Duration::from_millis(2));
                        }
                        Err(e) => panic!("model fixture failed: {e}"),
                    }
                }
            })
        };
        SlowModel {
            base_url,
            stop,
            requests,
            in_flight,
            peak,
            server: Some(server),
        }
    }

    fn finish(mut self) -> (usize, usize) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(s) = self.server.take() {
            s.join().unwrap();
        }
        (
            self.requests.load(Ordering::Relaxed),
            self.peak.load(Ordering::SeqCst),
        )
    }
}

/// Layered-run harness: a slow model (LLM_MS per call) behind the real
/// `run_unified` loop. Measures how many scheduler ticks (routine layer)
/// complete in a fixed window, how many beats were deferred, and the peak
/// number of concurrent model calls (deliberative single-flight).
fn run_layer_harness(llm_ms: u64, window_ms: u64) -> (u64, u64, usize, usize) {
    let model = SlowModel::start(llm_ms);
    let cognitive = Arc::new(CognitiveEngine::with_base(
        "test-key",
        "test-model",
        &model.base_url,
    ));
    let (scheduler, space, _) = build_scheduler_with_cognitive(cognitive);
    // unlabeled memories keep the space "unhealthy" so every 5th tick wants the model
    for i in 0..12 {
        add_tetra_to_space(
            &space,
            Point3 {
                x: 20.0 * i as f64,
                y: 7.0,
                z: 3.0,
            },
            &format!("orphan memory {i}"),
            vec![],
        );
    }
    scheduler.set_tick_interval(50);
    scheduler.horizon.lock().note_stimulus(1.0);
    let rx = scheduler.tx.subscribe();
    let start_tick = scheduler.tick_count.load(Ordering::SeqCst);
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let ticks = rt.block_on(async {
        let handle = tokio::spawn(scheduler.clone().run_with_rx(rx));
        tokio::time::sleep(std::time::Duration::from_millis(window_ms)).await;
        let ticks = scheduler.tick_count.load(Ordering::SeqCst) - start_tick;
        let _ = scheduler.tx.send(EngineEvent::Shutdown);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(10), handle).await;
        ticks
    });
    rt.shutdown_timeout(std::time::Duration::from_millis(llm_ms * 3 + 2000));
    let missed = scheduler.horizon_observability().3;
    let (calls, peak) = model.finish();
    (ticks, missed, calls, peak)
}

/// Timing harness (machine-dependent, so not part of the default suite):
/// `cargo test --lib -- --ignored --nocapture layered_loop quiet_loop`
#[test]
#[ignore]
fn layered_loop_keeps_model_calls_single_flight() {
    let (ticks, missed, calls, peak) = run_layer_harness(800, 4000);
    eprintln!("R11LAYER llm_ms=800 window_ms=4000 ticks={ticks} deferred_beats={missed} model_calls={calls} peak_concurrent_calls={peak}");
    assert!(calls >= 1, "the slow model was never called");
    assert_eq!(peak, 1, "scheduler model calls must stay single-flight");
}

/// Reproduction: the commit cycle used to hold `cycle_gate` for the whole
/// model round-trip, so every routine tick in that window was deferred.
#[test]
fn slow_model_call_releases_cycle_gate_and_back_pressures_new_thoughts() {
    let model = SlowModel::start(5_000);
    let cognitive = Arc::new(CognitiveEngine::with_base(
        "test-key",
        "test-model",
        &model.base_url,
    ));
    let (scheduler, space, _) = build_scheduler_with_cognitive(cognitive);
    for i in 0..12 {
        add_tetra_to_space(
            &space,
            Point3 {
                x: 20.0 * i as f64,
                y: 7.0,
                z: 3.0,
            },
            &format!("orphan memory {i}"),
            vec![],
        );
    }
    let permit = scheduler.cycle_gate.try_acquire().unwrap();
    let worker = {
        let s = scheduler.clone();
        std::thread::spawn(move || s.run_commit_cycle(permit))
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while model.in_flight.load(Ordering::SeqCst) == 0 {
        assert!(std::time::Instant::now() < deadline, "model never called");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    // Routine layer can run while the model call is in flight.
    let routine = scheduler.cycle_gate.try_acquire();
    assert!(routine.is_some(), "cycle gate still held during model call");
    assert!(scheduler
        .deliberative_gate
        .in_flight
        .load(Ordering::Acquire));
    // A second commit cycle that wants to think (tick 30: metacognition check +
    // cognitive hooks) is back-pressured, not stacked on top of the slow call.
    scheduler.tick_count.store(30, Ordering::SeqCst);
    scheduler.run_commit_cycle(routine.unwrap());
    assert_eq!(scheduler.layers.deferred(RunLayer::Deliberative), 2);
    assert_eq!(model.requests.load(Ordering::SeqCst), 1);
    worker.join().unwrap();
    assert_eq!(scheduler.layers.runs(RunLayer::Deliberative), 1);
    assert_eq!(scheduler.layers.runs(RunLayer::Routine), 2);
    assert!(!scheduler.cycle_gate.in_flight.load(Ordering::Acquire));
    assert!(!scheduler
        .deliberative_gate
        .in_flight
        .load(Ordering::Acquire));
    let snapshot = scheduler.run_layers_snapshot();
    assert_eq!(snapshot["layers"]["deliberative"]["deferred"], 2);
    assert!(
        snapshot["layers"]["deliberative"]["max_ms"]
            .as_f64()
            .unwrap()
            >= 4_000.0
    );
    let (calls, peak) = model.finish();
    assert_eq!((calls, peak), (1, 1));
}

/// Quiet-mode harness (cloud default without ENABLE_COGNITIVE): the attend
/// path runs inline on the async loop. Measures ticks completed and the worst
/// scheduling lag seen by a 10 ms probe task on a single-worker runtime.
fn run_quiet_harness(llm_ms: u64, window_ms: u64) -> (u64, u64, usize, usize) {
    let model = SlowModel::start(llm_ms);
    let cognitive = Arc::new(CognitiveEngine::with_base(
        "test-key",
        "test-model",
        &model.base_url,
    ));
    let (scheduler, space, _) = build_scheduler_with_cognitive(cognitive);
    for i in 0..12 {
        add_tetra_to_space(
            &space,
            Point3 {
                x: 20.0 * i as f64,
                y: 7.0,
                z: 3.0,
            },
            &format!("cold memory {i}"),
            vec!["note".into()],
        );
    }
    scheduler.set_tick_interval(50);
    scheduler.horizon.lock().note_stimulus(1.0);
    let rx = scheduler.tx.subscribe();
    let start_tick = scheduler.tick_count.load(Ordering::SeqCst);
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let (ticks, max_lag_ms) = rt.block_on(async {
        let handle = tokio::spawn(scheduler.clone().run_quiet(rx));
        let probe = tokio::spawn(async move {
            let mut worst = 0u64;
            let end = std::time::Instant::now() + std::time::Duration::from_millis(window_ms);
            while std::time::Instant::now() < end {
                let t = std::time::Instant::now();
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                let lag = t.elapsed().as_millis() as u64;
                worst = worst.max(lag.saturating_sub(10));
            }
            worst
        });
        let worst = probe.await.unwrap();
        let ticks = scheduler.tick_count.load(Ordering::SeqCst) - start_tick;
        let _ = scheduler.tx.send(EngineEvent::Shutdown);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(10), handle).await;
        (ticks, worst)
    });
    rt.shutdown_timeout(std::time::Duration::from_millis(llm_ms * 3 + 2000));
    let (calls, peak) = model.finish();
    (ticks, max_lag_ms, calls, peak)
}

/// Timing harness, see `layered_loop_keeps_model_calls_single_flight`.
#[test]
#[ignore]
fn quiet_loop_never_blocks_async_worker_on_model_calls() {
    let (ticks, lag, calls, peak) = run_quiet_harness(800, 4000);
    eprintln!("R11QUIET llm_ms=800 window_ms=4000 ticks={ticks} max_async_lag_ms={lag} model_calls={calls} peak_concurrent_calls={peak}");
    assert!(lag < 400, "async worker blocked for {lag} ms");
    assert!(peak <= 1);
}

/// Reproduction: `auto_save` (called inline from the event loop, on every
/// few ticks and on creation events) used to make a blocking model call to
/// review cold memories.
#[test]
fn auto_save_makes_no_model_calls() {
    let model = SlowModel::start(10);
    let cognitive = Arc::new(CognitiveEngine::with_base(
        "test-key",
        "test-model",
        &model.base_url,
    ));
    let (scheduler, space, _) = build_scheduler_with_cognitive(cognitive);
    for i in 0..5 {
        add_tetra_to_space(
            &space,
            Point3 {
                x: 20.0 * i as f64,
                y: 7.0,
                z: 3.0,
            },
            &format!("cold memory {i}"),
            vec!["note".into()],
        );
    }
    for _ in 0..3 {
        scheduler.auto_save();
    }
    assert_eq!(model.requests.load(Ordering::SeqCst), 0);
    // the review still runs, on the deliberative layer
    scheduler.cold_review_blocking();
    assert_eq!(model.requests.load(Ordering::SeqCst), 1);
    assert_eq!(scheduler.layers.runs(RunLayer::Deliberative), 1);
    let (calls, peak) = model.finish();
    assert_eq!((calls, peak), (1, 1));
}

/// Reproduction: learned thresholds used to reset to defaults on every restart.
#[test]
fn learned_adaptive_params_survive_engine_restart() {
    use crate::engine::adaptive::Param;
    let dir = std::env::temp_dir().join(format!("epicode-adaptive-{}", uuid::Uuid::new_v4()));
    let learned = {
        let engine = crate::engine::Engine::with_data_dir(dir.clone());
        {
            let mut ad = engine.scheduler.adaptive.lock();
            for _ in 0..30 {
                ad.adapt_from_outcome(ActionType::Dream, 1.0);
                ad.adapt_from_outcome(ActionType::Pulse, 0.0);
            }
        }
        engine.scheduler.save_tick_state();
        let ad = engine.scheduler.adaptive.lock();
        (ad.get(Param::DreamInterval), ad.get(Param::PulseBudget))
    };
    assert!(learned.0 > 50.0 && learned.1 < 3.0, "{learned:?}");
    let engine = crate::engine::Engine::with_data_dir(dir.clone());
    let ad = engine.scheduler.adaptive.lock();
    assert_eq!(
        (ad.get(Param::DreamInterval), ad.get(Param::PulseBudget)),
        learned
    );
    drop(ad);
    let described = engine.scheduler.adaptive_params_snapshot();
    assert_eq!(described["dream_interval"]["default"], 50.0);
    drop(engine);
    let _ = std::fs::remove_dir_all(&dir);
}
