use super::*;
use axum::extract::Extension;
use epicode::engine::{user_manager::UserRole, Engine};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc};

struct TestDirectory(PathBuf);

impl Drop for TestDirectory {
    fn drop(&mut self) {
        // Only remove the unique directory allocated by this test fixture.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture() -> (TestDirectory, Arc<Engine>) {
    let dir = std::env::temp_dir().join(format!("epicode-archive-test-{}", uuid::Uuid::new_v4()));
    let engine = Arc::new(Engine::with_data_dir(dir.clone()));
    (TestDirectory(dir), engine)
}

fn user(role: UserRole, owner: bool) -> Extension<UserInfo> {
    Extension(
        serde_json::from_value(json!({
            "user_id": "archive-test", "api_key": "fixture-only", "plan": "Pro",
            "max_memories": 10000, "memories_used": 0, "created_at": 0,
            "parent": if owner { None } else { Some("fixture-owner") }, "role": role
        }))
        .unwrap(),
    )
}

fn create(engine: &Engine, title: &str) -> u64 {
    engine
        .scheduler
        .api_archive_create_node(0, "doc", title, "正文\n\n第二段\r\n末行", None)
        .unwrap()
}

fn snapshot(engine: &Engine) -> Value {
    let mut memories: Vec<_> = engine
        .space
        .all_tetrahedrons()
        .iter()
        .map(|t| (t.id, t.data.content.clone(), t.data.labels.clone()))
        .collect();
    memories.sort_by_key(|t| t.0);
    json!({"memories": memories, "archive": engine.scheduler.api_archive_tree()})
}

fn denied(response: (StatusCode, Json<Value>)) {
    assert_eq!(response.0, StatusCode::FORBIDDEN);
    assert_eq!(response.1 .0["code"], "FORBIDDEN_ROLE");
}

#[tokio::test]
async fn viewer_cannot_mutate_any_archive_handler() {
    let (_dir, engine) = fixture();
    let first = create(&engine, "Viewer first document");
    let second = create(&engine, "Viewer second document");
    let before = snapshot(&engine);
    denied(
        archive_create_node(
            AuthedEngine(engine.clone()),
            user(UserRole::Viewer, false),
            Json(ArchiveNodeRequest {
                parent_id: 0,
                node_type: "doc".into(),
                title: "Forbidden".into(),
                content: "Forbidden body".into(),
                category: None,
            }),
        )
        .await,
    );
    denied(
        archive_edit_node(
            AuthedEngine(engine.clone()),
            user(UserRole::Viewer, false),
            Path(first),
            Json(ArchiveEditRequest {
                title: Some("Forbidden rename".into()),
                content: Some("Forbidden replacement".into()),
                category: Some("changed".into()),
            }),
        )
        .await,
    );
    denied(
        archive_delete_node(
            AuthedEngine(engine.clone()),
            user(UserRole::Viewer, false),
            Path(first),
        )
        .await,
    );
    denied(
        archive_merge(
            AuthedEngine(engine.clone()),
            user(UserRole::Viewer, false),
            Json(ArchiveMergeRequest {
                source_ids: vec![first, second],
                title: "Forbidden merge".into(),
                category: None,
            }),
        )
        .await,
    );
    denied(
        archive_move(
            AuthedEngine(engine.clone()),
            user(UserRole::Viewer, false),
            Json(ArchiveMoveRequest {
                node_id: second,
                new_parent_id: first,
            }),
        )
        .await,
    );
    denied(
        archive_import(
            AuthedEngine(engine.clone()),
            user(UserRole::Viewer, false),
            Json(ArchiveImportRequest {
                project_name: "Forbidden project".into(),
                documents: vec![ArchiveImportDoc {
                    title: "Forbidden import".into(),
                    content: "Imported body".into(),
                    category: "test".into(),
                }],
            }),
        )
        .await,
    );
    assert_eq!(
        snapshot(&engine),
        before,
        "denied requests must not change content, labels or the archive tree"
    );
    assert_eq!(
        archive_tree(AuthedEngine(engine.clone())).await.0,
        StatusCode::OK
    );
    assert_eq!(
        archive_get_node(AuthedEngine(engine.clone()), Path(first))
            .await
            .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn tester_can_write_but_cannot_delete_or_merge() {
    let (_dir, engine) = fixture();
    let first = create(&engine, "Tester original");
    let second = create(&engine, "Tester second");
    let before = snapshot(&engine);
    denied(
        archive_delete_node(
            AuthedEngine(engine.clone()),
            user(UserRole::Tester, false),
            Path(first),
        )
        .await,
    );
    denied(
        archive_merge(
            AuthedEngine(engine.clone()),
            user(UserRole::Tester, false),
            Json(ArchiveMergeRequest {
                source_ids: vec![first, second],
                title: "Forbidden merge".into(),
                category: None,
            }),
        )
        .await,
    );
    assert_eq!(snapshot(&engine), before);

    assert_eq!(
        archive_create_node(
            AuthedEngine(engine.clone()),
            user(UserRole::Tester, false),
            Json(ArchiveNodeRequest {
                parent_id: 0,
                node_type: "doc".into(),
                title: "Tester new".into(),
                content: "Created by tester".into(),
                category: None,
            })
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        archive_edit_node(
            AuthedEngine(engine.clone()),
            user(UserRole::Tester, false),
            Path(first),
            Json(ArchiveEditRequest {
                title: Some("Renamed".into()),
                content: None,
                category: None,
            })
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        engine.scheduler.api_get_node(first).unwrap().content,
        "# Renamed\n\n正文\n\n第二段\r\n末行"
    );
    assert_eq!(
        archive_move(
            AuthedEngine(engine.clone()),
            user(UserRole::Tester, false),
            Json(ArchiveMoveRequest {
                node_id: second,
                new_parent_id: first,
            })
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(engine
        .scheduler
        .api_get_node(second)
        .unwrap()
        .labels
        .contains(&format!("parent:{}", first)));
    assert_eq!(
        archive_import(
            AuthedEngine(engine.clone()),
            user(UserRole::Tester, false),
            Json(ArchiveImportRequest {
                project_name: "Tester import".into(),
                documents: vec![ArchiveImportDoc {
                    title: "Imported document".into(),
                    content: "Imported by tester".into(),
                    category: "test".into(),
                }],
            })
        )
        .await
        .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn owner_admin_and_developer_keep_destructive_operations() {
    let (_dir, engine) = fixture();
    // Owner bypass must work even when its ignored role field says viewer.
    for (role, owner) in [
        (UserRole::Viewer, true),
        (UserRole::Admin, false),
        (UserRole::Developer, false),
    ] {
        let first = create(&engine, &format!("{role:?} merge source one"));
        let second = create(&engine, &format!("{role:?} merge source two"));
        let response = archive_merge(
            AuthedEngine(engine.clone()),
            user(role, owner),
            Json(ArchiveMergeRequest {
                source_ids: vec![first, second],
                title: format!("{role:?} merged"),
                category: None,
            }),
        )
        .await;
        assert_eq!(response.0, StatusCode::OK);
        assert!(engine
            .scheduler
            .api_get_node(first)
            .unwrap()
            .labels
            .contains(&"merged".into()));
        let merged = response.1 .0["data"]["id"].as_u64().unwrap();
        assert_eq!(
            archive_delete_node(
                AuthedEngine(engine.clone()),
                user(role, owner),
                Path(merged)
            )
            .await
            .0,
            StatusCode::OK
        );
        assert!(engine
            .scheduler
            .api_get_node(merged)
            .unwrap()
            .labels
            .contains(&"archived".into()));
    }
}

#[tokio::test]
async fn archive_edit_distinguishes_missing_and_empty_body() {
    let (_dir, engine) = fixture();
    let id = create(&engine, "Edit original");
    for (title, body, expected) in [
        (Some("Renamed"), None, "# Renamed\n\n正文\n\n第二段\r\n末行"),
        (None, Some("Replacement"), "# Renamed\n\nReplacement"),
        (Some("Cleared"), Some(""), "# Cleared\n\n"),
        (Some("Still empty"), None, "# Still empty\n\n"),
    ] {
        let response = archive_edit_node(
            AuthedEngine(engine.clone()),
            user(UserRole::Developer, false),
            Path(id),
            Json(ArchiveEditRequest {
                title: title.map(str::to_owned),
                content: body.map(str::to_owned),
                category: None,
            }),
        )
        .await;
        assert_eq!(response.0, StatusCode::OK);
        assert_eq!(engine.scheduler.api_get_node(id).unwrap().content, expected);
    }
    let title_only = engine
        .scheduler
        .api_archive_create_node(0, "project", "Empty project", "", None)
        .unwrap();
    engine
        .scheduler
        .api_archive_edit_node(title_only, Some("Renamed project"), None, None)
        .unwrap();
    assert_eq!(
        engine.scheduler.api_get_node(title_only).unwrap().content,
        "# Renamed project"
    );
}
