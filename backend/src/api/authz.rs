use crate::domain::permission::{
    Action, AuditResult, AuthzError, Permission, PermissionMatrix, ResourceType,
};
use crate::engine::audit::AuditLogger;
use std::sync::{Arc, Mutex};

/// 权限存储库
pub struct PermissionRepository {
    permissions: Arc<Mutex<Vec<Permission>>>,
}

impl PermissionRepository {
    pub fn new() -> Self {
        Self {
            permissions: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// 添加权限
    pub fn add_permission(&self, permission: Permission) -> Result<(), String> {
        match self.permissions.lock() {
            Ok(mut perms) => {
                perms.push(permission);
                Ok(())
            }
            Err(e) => Err(format!("Failed to acquire lock: {}", e)),
        }
    }

    /// 获取用户在资源上的权限
    pub fn get_permission(
        &self,
        user_id: &str,
        resource_id: &str,
        resource_type: ResourceType,
    ) -> Result<Option<Permission>, String> {
        match self.permissions.lock() {
            Ok(perms) => Ok(perms
                .iter()
                .find(|p| {
                    p.user_id == user_id
                        && p.resource_id == resource_id
                        && p.resource_type == resource_type
                        && p.revoked_at.is_none()
                })
                .cloned()),
            Err(e) => Err(format!("Failed to acquire lock: {}", e)),
        }
    }

    /// 获取用户的所有权限
    pub fn get_user_permissions(&self, user_id: &str) -> Result<Vec<Permission>, String> {
        match self.permissions.lock() {
            Ok(perms) => Ok(perms
                .iter()
                .filter(|p| p.user_id == user_id && p.revoked_at.is_none())
                .cloned()
                .collect()),
            Err(e) => Err(format!("Failed to acquire lock: {}", e)),
        }
    }

    /// 获取资源的所有权限
    pub fn get_resource_permissions(
        &self,
        resource_id: &str,
        resource_type: ResourceType,
    ) -> Result<Vec<Permission>, String> {
        match self.permissions.lock() {
            Ok(perms) => Ok(perms
                .iter()
                .filter(|p| {
                    p.resource_id == resource_id
                        && p.resource_type == resource_type
                        && p.revoked_at.is_none()
                })
                .cloned()
                .collect()),
            Err(e) => Err(format!("Failed to acquire lock: {}", e)),
        }
    }

    /// 撤销权限
    pub fn revoke_permission(&self, permission_id: &str) -> Result<(), String> {
        match self.permissions.lock() {
            Ok(mut perms) => {
                if let Some(perm) = perms.iter_mut().find(|p| p.id == permission_id) {
                    perm.revoked_at = Some(chrono::Utc::now());
                    Ok(())
                } else {
                    Err("Permission not found".to_string())
                }
            }
            Err(e) => Err(format!("Failed to acquire lock: {}", e)),
        }
    }

    /// 删除所有权限（仅用于测试）
    #[cfg(test)]
    pub fn clear(&self) -> Result<(), String> {
        match self.permissions.lock() {
            Ok(mut perms) => {
                perms.clear();
                Ok(())
            }
            Err(e) => Err(format!("Failed to acquire lock: {}", e)),
        }
    }
}

impl Clone for PermissionRepository {
    fn clone(&self) -> Self {
        Self {
            permissions: Arc::clone(&self.permissions),
        }
    }
}

impl Default for PermissionRepository {
    fn default() -> Self {
        Self::new()
    }
}

/// 授权检查器
pub struct AuthorizationChecker {
    repo: PermissionRepository,
    audit_logger: AuditLogger,
}

impl AuthorizationChecker {
    pub fn new(repo: PermissionRepository, audit_logger: AuditLogger) -> Self {
        Self { repo, audit_logger }
    }

    /// 检查用户是否可以执行操作
    pub async fn check(
        &self,
        user_id: &str,
        resource_id: &str,
        resource_type: ResourceType,
        action: Action,
        tenant_id: &str,
    ) -> Result<(), AuthzError> {
        // 获取用户权限
        let permission = self
            .repo
            .get_permission(user_id, resource_id, resource_type)
            .map_err(|e| AuthzError::InternalError { message: e })?;

        let role = match permission {
            Some(perm) => perm.role,
            None => {
                // 记录审计日志
                let _ = self.audit_logger.log(
                    user_id.to_string(),
                    action.as_str().to_string(),
                    resource_type.as_str().to_string(),
                    resource_id.to_string(),
                    AuditResult::PermissionDenied,
                    Some("No permission found".to_string()),
                    tenant_id.to_string(),
                    None,
                );
                return Err(AuthzError::PermissionDenied {
                    user_id: user_id.to_string(),
                    resource_id: resource_id.to_string(),
                    action: action.as_str().to_string(),
                });
            }
        };

        // 检查角色是否可以执行操作
        if PermissionMatrix::can_perform(role, action) {
            // 记录成功的审计日志
            let _ = self.audit_logger.log(
                user_id.to_string(),
                action.as_str().to_string(),
                resource_type.as_str().to_string(),
                resource_id.to_string(),
                AuditResult::Success,
                None,
                tenant_id.to_string(),
                None,
            );
            Ok(())
        } else {
            // 记录拒绝的审计日志
            let _ = self.audit_logger.log(
                user_id.to_string(),
                action.as_str().to_string(),
                resource_type.as_str().to_string(),
                resource_id.to_string(),
                AuditResult::PermissionDenied,
                Some(format!(
                    "Role {:?} cannot perform action {:?}",
                    role, action
                )),
                tenant_id.to_string(),
                None,
            );
            Err(AuthzError::PermissionDenied {
                user_id: user_id.to_string(),
                resource_id: resource_id.to_string(),
                action: action.as_str().to_string(),
            })
        }
    }

    /// 获取用户权限
    pub fn get_user_permissions(&self, user_id: &str) -> Result<Vec<Permission>, AuthzError> {
        self.repo
            .get_user_permissions(user_id)
            .map_err(|e| AuthzError::InternalError { message: e })
    }

    /// 获取资源权限
    pub fn get_resource_permissions(
        &self,
        resource_id: &str,
        resource_type: ResourceType,
    ) -> Result<Vec<Permission>, AuthzError> {
        self.repo
            .get_resource_permissions(resource_id, resource_type)
            .map_err(|e| AuthzError::InternalError { message: e })
    }

    /// 授予权限
    pub fn grant_permission(&self, permission: Permission) -> Result<String, AuthzError> {
        self.repo
            .add_permission(permission.clone())
            .map_err(|e| AuthzError::InternalError { message: e })?;
        Ok(permission.id)
    }

    /// 撤销权限
    pub fn revoke_permission(&self, permission_id: &str) -> Result<(), AuthzError> {
        self.repo
            .revoke_permission(permission_id)
            .map_err(|e| AuthzError::InternalError { message: e })
    }

    /// 获取审计日志
    pub fn get_audit_logs(
        &self,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<crate::domain::permission::AuditLogEntry>, usize), AuthzError> {
        self.audit_logger
            .get_paginated(offset, limit)
            .map_err(|e| AuthzError::InternalError { message: e })
    }

    pub fn get_audit_logger(&self) -> AuditLogger {
        self.audit_logger.clone()
    }
}

impl Clone for AuthorizationChecker {
    fn clone(&self) -> Self {
        Self {
            repo: self.repo.clone(),
            audit_logger: self.audit_logger.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::permission::UserRole;
    use chrono::Utc;

    #[test]
    fn test_permission_repository_new() {
        let repo = PermissionRepository::new();
        let user_perms = repo.get_user_permissions("user1").unwrap();
        assert!(user_perms.is_empty());
    }

    #[test]
    fn test_permission_repository_add_and_get() {
        let repo = PermissionRepository::new();
        let perm = Permission {
            id: "perm1".to_string(),
            user_id: "user1".to_string(),
            resource_id: "resource1".to_string(),
            resource_type: ResourceType::Memory,
            role: UserRole::Editor,
            granted_at: Utc::now(),
            granted_by: "admin".to_string(),
            tenant_id: "tenant1".to_string(),
            revoked_at: None,
        };

        repo.add_permission(perm.clone()).unwrap();
        let retrieved = repo
            .get_permission("user1", "resource1", ResourceType::Memory)
            .unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().role, UserRole::Editor);
    }

    #[test]
    fn test_permission_repository_get_user_permissions() {
        let repo = PermissionRepository::new();
        for i in 0..3 {
            let perm = Permission {
                id: format!("perm{}", i),
                user_id: "user1".to_string(),
                resource_id: format!("resource{}", i),
                resource_type: ResourceType::Memory,
                role: UserRole::Viewer,
                granted_at: Utc::now(),
                granted_by: "admin".to_string(),
                tenant_id: "tenant1".to_string(),
                revoked_at: None,
            };
            repo.add_permission(perm).unwrap();
        }

        let perms = repo.get_user_permissions("user1").unwrap();
        assert_eq!(perms.len(), 3);
    }

    #[test]
    fn test_authorization_checker_allow() {
        let repo = PermissionRepository::new();
        let logger = AuditLogger::new();
        let checker = AuthorizationChecker::new(repo, logger);

        let perm = Permission {
            id: "perm1".to_string(),
            user_id: "user1".to_string(),
            resource_id: "resource1".to_string(),
            resource_type: ResourceType::Memory,
            role: UserRole::Editor,
            granted_at: Utc::now(),
            granted_by: "admin".to_string(),
            tenant_id: "tenant1".to_string(),
            revoked_at: None,
        };

        checker.grant_permission(perm).unwrap();

        let result = futures::executor::block_on(checker.check(
            "user1",
            "resource1",
            ResourceType::Memory,
            Action::Read,
            "tenant1",
        ));
        assert!(result.is_ok());
    }

    #[test]
    fn test_authorization_checker_deny() {
        let repo = PermissionRepository::new();
        let logger = AuditLogger::new();
        let checker = AuthorizationChecker::new(repo, logger);

        let perm = Permission {
            id: "perm1".to_string(),
            user_id: "user1".to_string(),
            resource_id: "resource1".to_string(),
            resource_type: ResourceType::Memory,
            role: UserRole::Viewer,
            granted_at: Utc::now(),
            granted_by: "admin".to_string(),
            tenant_id: "tenant1".to_string(),
            revoked_at: None,
        };

        checker.grant_permission(perm).unwrap();

        let result = futures::executor::block_on(checker.check(
            "user1",
            "resource1",
            ResourceType::Memory,
            Action::Delete,
            "tenant1",
        ));
        assert!(result.is_err());
    }

    #[test]
    fn test_authorization_checker_no_permission() {
        let repo = PermissionRepository::new();
        let logger = AuditLogger::new();
        let checker = AuthorizationChecker::new(repo, logger);

        let result = futures::executor::block_on(checker.check(
            "user1",
            "resource1",
            ResourceType::Memory,
            Action::Read,
            "tenant1",
        ));
        assert!(result.is_err());
    }

    #[test]
    fn test_authorization_checker_revoke() {
        let repo = PermissionRepository::new();
        let logger = AuditLogger::new();
        let checker = AuthorizationChecker::new(repo, logger);

        let perm = Permission {
            id: "perm1".to_string(),
            user_id: "user1".to_string(),
            resource_id: "resource1".to_string(),
            resource_type: ResourceType::Memory,
            role: UserRole::Editor,
            granted_at: Utc::now(),
            granted_by: "admin".to_string(),
            tenant_id: "tenant1".to_string(),
            revoked_at: None,
        };

        checker.grant_permission(perm).unwrap();
        checker.revoke_permission("perm1").unwrap();

        let result = futures::executor::block_on(checker.check(
            "user1",
            "resource1",
            ResourceType::Memory,
            Action::Read,
            "tenant1",
        ));
        assert!(result.is_err());
    }

    #[test]
    fn test_authorization_checker_audit_logs() {
        let repo = PermissionRepository::new();
        let logger = AuditLogger::new();
        let checker = AuthorizationChecker::new(repo, logger);

        let perm = Permission {
            id: "perm1".to_string(),
            user_id: "user1".to_string(),
            resource_id: "resource1".to_string(),
            resource_type: ResourceType::Memory,
            role: UserRole::Editor,
            granted_at: Utc::now(),
            granted_by: "admin".to_string(),
            tenant_id: "tenant1".to_string(),
            revoked_at: None,
        };

        checker.grant_permission(perm).unwrap();
        let _ = futures::executor::block_on(checker.check(
            "user1",
            "resource1",
            ResourceType::Memory,
            Action::Read,
            "tenant1",
        ));

        let (logs, total) = checker.get_audit_logs(0, 10).unwrap();
        assert!(!logs.is_empty());
        assert!(total > 0);
    }
}
