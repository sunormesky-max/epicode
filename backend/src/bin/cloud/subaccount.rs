//! 子账号 HTTP handlers：list / create / revoke / set-role（分级权限控制）。

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use epicode::engine::user_manager::{Permission, UserInfo, UserRole};

use super::helpers::require_perm;
use super::state::CloudState;

#[derive(Deserialize)]
pub struct CreateSubaccountRequest {
    pub user_id: String,
    pub password: String,
    /// 分级角色: admin/developer/tester/viewer (缺省 developer, 与存量迁移默认一致)
    #[serde(default)]
    pub role: Option<String>,
}

fn forbidden(op: &str, msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::FORBIDDEN,
        Json(epicode::engine::smrp::envelope_err_plain(op, 403, msg)),
    )
}

pub async fn list_subaccounts(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(r) = require_perm(&user, Permission::SubaccountManage) {
        return r;
    }
    let subs = st.user_mgr.list_subaccounts(&user.user_id);
    let items: Vec<serde_json::Value> = subs
        .iter()
        .map(|s| {
            serde_json::json!({
                "user_id": s.user_id,
                "plan": serde_json::to_value(&s.plan).unwrap_or_default(),
                "role": s.role.as_str(),
                "email": s.email,
                "custom_permissions": s.custom_permissions,
                "effective_permissions": s.effective_permissions(),
                "memories_used": s.memories_used,
                "created_at": s.created_at,
            })
        })
        .collect();
    (
        StatusCode::OK,
        Json(epicode::engine::smrp::envelope_ok_plain(
            "subaccount_list",
            serde_json::json!({
                "subaccounts": items,
                "total": items.len(),
            }),
        )),
    )
}

pub async fn create_subaccount(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Json(req): Json<CreateSubaccountRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(r) = require_perm(&user, Permission::SubaccountManage) {
        return r;
    }
    // 保留名大小写不敏感拒绝(审计三轮高优): Sunorme 可绕过大小写敏感的
    // 重名检查, 而 is_privileged_id 比较小写 — 子账户将获得库审批特权
    if epicode::engine::user_manager::UserManager::is_reserved_id(&req.user_id) {
        return forbidden("subaccount_create", "this username is reserved");
    }
    if req.user_id.is_empty() || req.user_id.len() > 64 {
        return (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err_plain(
                "subaccount_create",
                400,
                "user_id must be 1-64 characters",
            )),
        );
    }
    if !req
        .user_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err_plain(
                "subaccount_create",
                400,
                "user_id: only a-z A-Z 0-9 - _ allowed",
            )),
        );
    }
    if req.password.len() < 6 {
        return (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err_plain(
                "subaccount_create",
                400,
                "password must be at least 6 characters",
            )),
        );
    }
    // 分级角色解析(缺省 developer)
    let role = match req.role.as_deref() {
        None | Some("developer") => UserRole::Developer,
        Some(s) => match UserRole::parse(s) {
            Some(r) => r,
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(epicode::engine::smrp::envelope_err_plain(
                        "subaccount_create",
                        400,
                        "role must be one of: admin, developer, tester, viewer",
                    )),
                )
            }
        },
    };
    // admin 角色是代理全权 — 仅主账户可授予(owner 亲自点名, admin 不能再造 admin)
    if role == UserRole::Admin && user.parent.is_some() {
        return forbidden(
            "subaccount_create",
            "only the main account can grant the admin role",
        );
    }
    match st
        .user_mgr
        .create_subaccount(&user.user_id, &req.user_id, &req.password, role)
    {
        Ok(info) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok_plain(
                "subaccount_create",
                serde_json::json!({
                    "user_id": info.user_id,
                    "api_key": info.api_key,
                    "role": info.role.as_str(),
                    "message": "Sub-account created. The agent must confirm its identity on first connection. Identity is permanent and cannot be changed.",
                }),
            )),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err_plain(
                "subaccount_create",
                400,
                &e,
            )),
        ),
    }
}

pub async fn revoke_subaccount(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Path(sub_id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(r) = require_perm(&user, Permission::SubaccountManage) {
        return r;
    }
    match st.user_mgr.revoke_subaccount(&user.user_id, &sub_id) {
        Ok(()) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok_plain(
                "subaccount_revoke",
                serde_json::json!({
                    "message": format!("sub-account {} revoked", sub_id),
                }),
            )),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err_plain(
                "subaccount_revoke",
                400,
                &e,
            )),
        ),
    }
}

/// GET /v1/settings — 账户设置(主题偏好/自定义CSS/记忆输出策略, 跟随账户)
pub async fn get_user_settings(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    match st.user_mgr.get_user_settings(&user.user_id) {
        Ok(settings) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok_plain(
                "user_settings_get",
                serde_json::json!({
                    "settings": settings,
                    "plan": serde_json::to_value(&user.plan).unwrap_or_default(),
                    "can_theme_custom": user.can_customize_theme(),
                    "can_memory_output_control": user.can_control_memory_output(),
                    "can_permission_edit": user.can_edit_permissions(),
                }),
            )),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err_plain(
                "user_settings_get",
                400,
                &e,
            )),
        ),
    }
}

#[derive(Deserialize)]
pub struct SettingsPatchRequest {
    pub patch: serde_json::Value,
}

/// PUT /v1/settings — 账户设置写入(计划门控: Free不可自定义主题)
pub async fn set_user_settings(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Json(req): Json<SettingsPatchRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    match st.user_mgr.set_user_settings(&user.user_id, req.patch) {
        Ok(settings) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok_plain(
                "user_settings_set",
                serde_json::json!({ "settings": settings }),
            )),
        ),
        Err(e) => (
            StatusCode::FORBIDDEN,
            Json(epicode::engine::smrp::envelope_err_plain(
                "user_settings_set",
                403,
                &e,
            )),
        ),
    }
}

#[derive(Deserialize)]
pub struct SetRoleRequest {
    pub role: String,
}

/// PATCH /v1/subaccounts/:id/role — 变更子账户角色
/// 主账户: 可授任意角色(含 admin); admin 子账户: 不可授 admin(防权限自我复制)
pub async fn set_subaccount_role(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Path(sub_id): Path<String>,
    Json(req): Json<SetRoleRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(r) = require_perm(&user, Permission::SubaccountManage) {
        return r;
    }
    let role = match UserRole::parse(&req.role) {
        Some(r) => r,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(epicode::engine::smrp::envelope_err_plain(
                    "subaccount_set_role",
                    400,
                    "role must be one of: admin, developer, tester, viewer",
                )),
            )
        }
    };
    if role == UserRole::Admin && user.parent.is_some() {
        return forbidden(
            "subaccount_set_role",
            "only the main account can grant the admin role",
        );
    }
    match st
        .user_mgr
        .set_subaccount_role(&user.user_id, &sub_id, role)
    {
        Ok(info) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok_plain(
                "subaccount_set_role",
                serde_json::json!({
                    "user_id": info.user_id,
                    "role": info.role.as_str(),
                    "permissions": info.role.permissions(),
                }),
            )),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err_plain(
                "subaccount_set_role",
                400,
                &e,
            )),
        ),
    }
}

#[derive(Deserialize)]
pub struct SetPermissionsRequest {
    /// Explicit grant list. Null restores the role template.
    pub permissions: Option<Vec<String>>,
}

/// PATCH /v1/subaccounts/:id/permissions — human-configured grant set
pub async fn set_subaccount_permissions(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Path(sub_id): Path<String>,
    Json(req): Json<SetPermissionsRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(r) = require_perm(&user, Permission::PermissionEdit) {
        return r;
    }
    if !user.can_edit_permissions() {
        return forbidden(
            "subaccount_set_permissions",
            "permission editing requires a paid plan",
        );
    }
    match st
        .user_mgr
        .set_subaccount_permissions(&user.user_id, &sub_id, req.permissions)
    {
        Ok(info) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok_plain(
                "subaccount_set_permissions",
                serde_json::json!({
                    "user_id": info.user_id,
                    "role": info.role.as_str(),
                    "custom_permissions": info.custom_permissions,
                    "effective_permissions": info.effective_permissions(),
                }),
            )),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err_plain(
                "subaccount_set_permissions",
                400,
                &e,
            )),
        ),
    }
}
