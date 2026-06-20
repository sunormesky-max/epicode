use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 用户角色定义
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UserRole {
    Owner,
    Editor,
    Viewer,
}

impl UserRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            UserRole::Owner => "owner",
            UserRole::Editor => "editor",
            UserRole::Viewer => "viewer",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "owner" => Some(UserRole::Owner),
            "editor" => Some(UserRole::Editor),
            "viewer" => Some(UserRole::Viewer),
            _ => None,
        }
    }
}

/// 资源类型定义
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceType {
    Memory,
    Space,
    Skill,
    Team,
    Workspace,
}

impl ResourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResourceType::Memory => "memory",
            ResourceType::Space => "space",
            ResourceType::Skill => "skill",
            ResourceType::Team => "team",
            ResourceType::Workspace => "workspace",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "memory" => Some(ResourceType::Memory),
            "space" => Some(ResourceType::Space),
            "skill" => Some(ResourceType::Skill),
            "team" => Some(ResourceType::Team),
            "workspace" => Some(ResourceType::Workspace),
            _ => None,
        }
    }
}

/// 操作定义
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    Create,
    Read,
    Update,
    Delete,
    Export,
    Share,
    ManageUsers,
}

impl Action {
    pub fn as_str(&self) -> &'static str {
        match self {
            Action::Create => "create",
            Action::Read => "read",
            Action::Update => "update",
            Action::Delete => "delete",
            Action::Export => "export",
            Action::Share => "share",
            Action::ManageUsers => "manage_users",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "create" => Some(Action::Create),
            "read" => Some(Action::Read),
            "update" => Some(Action::Update),
            "delete" => Some(Action::Delete),
            "export" => Some(Action::Export),
            "share" => Some(Action::Share),
            "manage_users" => Some(Action::ManageUsers),
            _ => None,
        }
    }
}

/// 权限记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permission {
    pub id: String,
    pub user_id: String,
    pub resource_id: String,
    pub resource_type: ResourceType,
    pub role: UserRole,
    pub granted_at: DateTime<Utc>,
    pub granted_by: String,
    pub tenant_id: String,
    pub revoked_at: Option<DateTime<Utc>>,
}

/// 权限矩阵：定义每个角色可以执行的操作
pub struct PermissionMatrix;

impl PermissionMatrix {
    /// 权限矩阵: role -> allowed_actions
    pub fn get_allowed_actions(role: UserRole) -> Vec<Action> {
        match role {
            UserRole::Owner => vec![
                Action::Create,
                Action::Read,
                Action::Update,
                Action::Delete,
                Action::Export,
                Action::Share,
                Action::ManageUsers,
            ],
            UserRole::Editor => vec![
                Action::Create,
                Action::Read,
                Action::Update,
                Action::Export,
                Action::Share,
            ],
            UserRole::Viewer => vec![Action::Read, Action::Export],
        }
    }

    /// 检查角色是否可以执行操作
    pub fn can_perform(role: UserRole, action: Action) -> bool {
        Self::get_allowed_actions(role).contains(&action)
    }

    /// 获取权限矩阵的详细表示
    pub fn get_matrix() -> HashMap<&'static str, Vec<&'static str>> {
        let mut matrix = HashMap::new();
        matrix.insert(
            "owner",
            vec![
                "create",
                "read",
                "update",
                "delete",
                "export",
                "share",
                "manage_users",
            ],
        );
        matrix.insert(
            "editor",
            vec!["create", "read", "update", "export", "share"],
        );
        matrix.insert("viewer", vec!["read", "export"]);
        matrix
    }
}

/// 授权错误
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthzError {
    PermissionDenied {
        user_id: String,
        resource_id: String,
        action: String,
    },
    ResourceNotFound {
        resource_id: String,
    },
    UserNotFound {
        user_id: String,
    },
    InvalidRole {
        role: String,
    },
    InternalError {
        message: String,
    },
}

impl std::fmt::Display for AuthzError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthzError::PermissionDenied {
                user_id,
                resource_id,
                action,
            } => {
                write!(
                    f,
                    "Permission denied for user {} to {} on resource {}",
                    user_id, action, resource_id
                )
            }
            AuthzError::ResourceNotFound { resource_id } => {
                write!(f, "Resource not found: {}", resource_id)
            }
            AuthzError::UserNotFound { user_id } => {
                write!(f, "User not found: {}", user_id)
            }
            AuthzError::InvalidRole { role } => {
                write!(f, "Invalid role: {}", role)
            }
            AuthzError::InternalError { message } => {
                write!(f, "Internal error: {}", message)
            }
        }
    }
}

impl std::error::Error for AuthzError {}

/// 审计日志条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogEntry {
    pub id: String,
    pub user_id: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    pub timestamp: DateTime<Utc>,
    pub result: AuditResult,
    pub error_message: Option<String>,
    pub tenant_id: String,
    pub details: Option<String>,
}

/// 审计结果
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditResult {
    Success,
    PermissionDenied,
    Error,
}

impl AuditResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuditResult::Success => "success",
            AuditResult::PermissionDenied => "permission_denied",
            AuditResult::Error => "error",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_role_conversions() {
        assert_eq!(UserRole::Owner.as_str(), "owner");
        assert_eq!(UserRole::Editor.as_str(), "editor");
        assert_eq!(UserRole::Viewer.as_str(), "viewer");

        assert_eq!(UserRole::from_str("owner"), Some(UserRole::Owner));
        assert_eq!(UserRole::from_str("editor"), Some(UserRole::Editor));
        assert_eq!(UserRole::from_str("viewer"), Some(UserRole::Viewer));
        assert_eq!(UserRole::from_str("invalid"), None);
    }

    #[test]
    fn test_resource_type_conversions() {
        assert_eq!(ResourceType::Memory.as_str(), "memory");
        assert_eq!(ResourceType::Space.as_str(), "space");

        assert_eq!(ResourceType::from_str("memory"), Some(ResourceType::Memory));
        assert_eq!(ResourceType::from_str("invalid"), None);
    }

    #[test]
    fn test_action_conversions() {
        assert_eq!(Action::Create.as_str(), "create");
        assert_eq!(Action::Read.as_str(), "read");
        assert_eq!(Action::from_str("create"), Some(Action::Create));
        assert_eq!(Action::from_str("invalid"), None);
    }

    #[test]
    fn test_permission_matrix_owner() {
        let actions = PermissionMatrix::get_allowed_actions(UserRole::Owner);
        assert_eq!(actions.len(), 7);
        assert!(actions.contains(&Action::Create));
        assert!(actions.contains(&Action::Delete));
        assert!(actions.contains(&Action::ManageUsers));
    }

    #[test]
    fn test_permission_matrix_editor() {
        let actions = PermissionMatrix::get_allowed_actions(UserRole::Editor);
        assert_eq!(actions.len(), 5);
        assert!(actions.contains(&Action::Create));
        assert!(actions.contains(&Action::Update));
        assert!(!actions.contains(&Action::Delete));
        assert!(!actions.contains(&Action::ManageUsers));
    }

    #[test]
    fn test_permission_matrix_viewer() {
        let actions = PermissionMatrix::get_allowed_actions(UserRole::Viewer);
        assert_eq!(actions.len(), 2);
        assert!(actions.contains(&Action::Read));
        assert!(actions.contains(&Action::Export));
        assert!(!actions.contains(&Action::Create));
        assert!(!actions.contains(&Action::Delete));
    }

    #[test]
    fn test_can_perform() {
        assert!(PermissionMatrix::can_perform(
            UserRole::Owner,
            Action::Delete
        ));
        assert!(PermissionMatrix::can_perform(
            UserRole::Editor,
            Action::Update
        ));
        assert!(!PermissionMatrix::can_perform(
            UserRole::Editor,
            Action::Delete
        ));
        assert!(PermissionMatrix::can_perform(
            UserRole::Viewer,
            Action::Read
        ));
        assert!(!PermissionMatrix::can_perform(
            UserRole::Viewer,
            Action::Create
        ));
    }

    #[test]
    fn test_permission_matrix_all_combinations() {
        let roles = [UserRole::Owner, UserRole::Editor, UserRole::Viewer];
        let actions = [
            Action::Create,
            Action::Read,
            Action::Update,
            Action::Delete,
            Action::Export,
            Action::Share,
            Action::ManageUsers,
        ];

        // 确保矩阵对所有组合都给出答案
        for role in &roles {
            for action in &actions {
                let _ = PermissionMatrix::can_perform(*role, *action);
            }
        }
    }

    #[test]
    fn test_audit_result_conversions() {
        assert_eq!(AuditResult::Success.as_str(), "success");
        assert_eq!(AuditResult::PermissionDenied.as_str(), "permission_denied");
        assert_eq!(AuditResult::Error.as_str(), "error");
    }
}
