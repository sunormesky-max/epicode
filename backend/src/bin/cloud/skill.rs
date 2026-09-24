//! Skills API HTTP handlers：list/create/get/update/delete/publish/search/pending/public/explore/pull/link。

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use epicode::engine::user_manager::UserInfo;

use super::helpers::{first_engine, get_engine, AuthedEngine};
use super::state::CloudState;

// ===== Skills API =====

pub async fn list_skills(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    // S2: 我的库视图 = 个人技能 + 系统技能(只读卡, 前端SYS徽章本就为此设计)
    let mut skills = engine.skills.list(Some(&engine.user_id));
    skills.extend(engine.skills.list_system());
    (
        StatusCode::OK,
        Json(epicode::engine::smrp::envelope_ok(
            &engine,
            "skill_list",
            serde_json::json!({"skills": skills}),
        )),
    )
}

#[derive(Deserialize)]
pub struct CreateSkillRequest {
    pub name: Option<String>,
    pub skill_md: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub triggers: Option<Vec<String>>,
}

pub async fn create_skill(
    AuthedEngine(engine): AuthedEngine,
    Json(req): Json<CreateSkillRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let name_raw = req
        .name
        .unwrap_or_else(|| format!("skill-{}", chrono::Utc::now().timestamp()));
    let md = req
        .skill_md
        .unwrap_or_else(|| "# New Skill\n\nDescribe your skill here.".to_string());
    // 输入校验：防止无界写入
    let name = name_raw.trim().to_string();
    if name.is_empty() || name.len() > 128 {
        return (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "skill_create",
                400,
                "skill name must be 1-128 characters",
            )),
        );
    }
    if md.len() > 256 * 1024 {
        return (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "skill_create",
                400,
                "skill content exceeds 256KB limit",
            )),
        );
    }
    let skill = engine.skills.create(name, md, engine.user_id.clone());
    // S2: 触发描述/场景词(创建即可带, 编辑亦可改 — 自动触发精度的用户侧杠杆)
    if let Some(d) = req.description.as_ref() {
        let _ = engine
            .skills
            .set_description(skill.id, d.chars().take(240).collect());
    }
    if let Some(t) = req.triggers.as_ref() {
        let _ = engine
            .skills
            .set_triggers(skill.id, t.iter().take(8).cloned().collect());
    }
    let skill = engine.skills.get(skill.id).unwrap_or(skill);
    (
        StatusCode::OK,
        Json(epicode::engine::smrp::envelope_ok(
            &engine,
            "skill_create",
            serde_json::json!({"skill": skill}),
        )),
    )
}

pub async fn get_skill(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Path(id): Path<u64>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    match engine.skills.get(id) {
        Some(skill) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &engine,
                "skill_get",
                serde_json::json!({"skill": skill}),
            )),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "skill_get",
                404,
                "skill not found",
            )),
        ),
    }
}

#[derive(Deserialize)]
pub struct UpdateSkillRequest {
    pub skill_md: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub triggers: Option<Vec<String>>,
}

pub async fn update_skill(
    AuthedEngine(engine): AuthedEngine,
    Path(id): Path<u64>,
    Json(req): Json<UpdateSkillRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    match engine.skills.update(id, req.skill_md, req.version) {
        Ok(skill) => {
            if let Some(d) = req.description.as_ref() {
                let _ = engine
                    .skills
                    .set_description(id, d.chars().take(240).collect());
            }
            if let Some(t) = req.triggers.as_ref() {
                let _ = engine
                    .skills
                    .set_triggers(id, t.iter().take(8).cloned().collect());
            }
            let fresh = engine.skills.get(id).unwrap_or(skill);
            (
                StatusCode::OK,
                Json(epicode::engine::smrp::envelope_ok(
                    &engine,
                    "skill_update",
                    serde_json::json!({"skill": fresh}),
                )),
            )
        }
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "skill_update",
                404,
                &e,
            )),
        ),
    }
}

pub async fn delete_skill(
    AuthedEngine(engine): AuthedEngine,
    Path(id): Path<u64>,
) -> (StatusCode, Json<serde_json::Value>) {
    match engine.skills.delete(id) {
        Ok(()) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &engine,
                "skill_delete",
                serde_json::json!({"status": "deleted"}),
            )),
        ),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "skill_delete",
                404,
                &e,
            )),
        ),
    }
}

pub async fn publish_skill(
    State(st): State<CloudState>,
    AuthedEngine(engine): AuthedEngine,
    Path(id): Path<u64>,
) -> (StatusCode, Json<serde_json::Value>) {
    let source = match engine.skills.get(id) {
        Some(s) => s,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "skill_publish",
                    404,
                    "skill not found",
                )),
            )
        }
    };
    if source.is_system {
        return (
            StatusCode::FORBIDDEN,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "skill_publish",
                403,
                "system skills cannot be published",
            )),
        );
    }
    // 安全闸门：提交到公共库时标记为 PendingReview，不直接对外可见
    // 公共技能库创建后状态为 Draft → submit_for_review 改为 PendingReview
    let pub_skill = st.pub_skills.create(
        source.name.clone(),
        source.skill_md.clone(),
        source.owner.clone(),
    );
    // 将公共库中的技能标记为待审核
    st.pub_skills.submit_for_review(pub_skill.id).ok();
    engine.skills.submit_for_review(id).ok();
    (
        StatusCode::OK,
        Json(epicode::engine::smrp::envelope_ok(
            &engine,
            "skill_publish",
            serde_json::json!({
                "skill": pub_skill,
                "status": "pending_review",
                "message": "submitted for review. Will be visible after admin approval."
            }),
        )),
    )
}

pub async fn list_pending_skills(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    let pending = engine.skills.review_pending();
    (
        StatusCode::OK,
        Json(epicode::engine::smrp::envelope_ok(
            &engine,
            "skill_pending",
            serde_json::json!({"skills": pending}),
        )),
    )
}

#[derive(Deserialize)]
pub struct LinkMemoryRequest {
    pub memory_id: u64,
}

pub async fn link_skill_memory(
    AuthedEngine(engine): AuthedEngine,
    Path(id): Path<u64>,
    Json(req): Json<LinkMemoryRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    match engine.skills.link_memory(id, req.memory_id) {
        Ok(()) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &engine,
                "skill_link",
                serde_json::json!({"status": "linked"}),
            )),
        ),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "skill_link",
                404,
                &e,
            )),
        ),
    }
}

#[derive(Deserialize)]
pub struct SearchSkillsRequest {
    pub query: String,
    pub limit: Option<usize>,
}

pub async fn search_skills(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Json(req): Json<SearchSkillsRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    let limit = req.limit.unwrap_or(10);
    let skills = engine
        .skills
        .match_skills(&req.query, &engine.user_id, limit);
    (
        StatusCode::OK,
        Json(epicode::engine::smrp::envelope_ok(
            &engine,
            "skill_search",
            serde_json::json!({"skills": skills}),
        )),
    )
}

pub async fn list_public_skills(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    let skills = st.pub_skills.list(None);
    (
        StatusCode::OK,
        Json(epicode::engine::smrp::envelope_ok(
            &engine,
            "skill_public_list",
            serde_json::json!({"skills": skills, "total": skills.len()}),
        )),
    )
}

pub async fn explore_public_skills(
    State(st): State<CloudState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let skills = st.pub_skills.list_public();
    let all_public: Vec<serde_json::Value> = skills
        .iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id,
                "name": s.name,
                "skill_md": s.skill_md,
                "version": s.version,
                "owner": s.owner,
                "category": s.category,
                "usage_count": s.usage_count,
                "success_rate": s.success_rate,
                "memory_ids_count": s.memory_ids.len(),
                "is_system": s.is_system,
                // R2e: 社区页同步 — 触发描述/场景词/体积(只增)
                "description": s.description,
                "triggers": s.triggers,
                "byte_size": s.skill_md.len(),
                "created_at": s.created_at,
                "updated_at": s.updated_at,
            })
        })
        .collect();
    match first_engine(&st) {
        Some(engine) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &engine,
                "skill_explore",
                serde_json::json!({"skills": all_public, "total": all_public.len()}),
            )),
        ),
        None => (
            StatusCode::OK,
            Json(serde_json::json!({"skills": all_public, "total": all_public.len()})),
        ),
    }
}

pub async fn pull_public_skill(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Path(id): Path<u64>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    let source = match st.pub_skills.get(id) {
        Some(s) => s,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "skill_pull",
                    404,
                    "public skill not found",
                )),
            )
        }
    };
    let skill = engine.skills.fork(&source, engine.user_id.clone());
    (
        StatusCode::OK,
        Json(epicode::engine::smrp::envelope_ok(
            &engine,
            "skill_pull",
            serde_json::json!({"skill": skill}),
        )),
    )
}
