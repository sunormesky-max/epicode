use crate::domain::permission::{AuditLogEntry, AuditResult};
use chrono::Utc;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// 审计日志管理器
pub struct AuditLogger {
    logs: Arc<Mutex<Vec<AuditLogEntry>>>,
}

impl AuditLogger {
    pub fn new() -> Self {
        Self {
            logs: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// 记录审计日志条目
    #[allow(clippy::too_many_arguments)]
    pub fn log(
        &self,
        user_id: String,
        action: String,
        resource_type: String,
        resource_id: String,
        result: AuditResult,
        error_message: Option<String>,
        tenant_id: String,
        details: Option<String>,
    ) -> Result<String, String> {
        let id = Uuid::new_v4().to_string();
        let entry = AuditLogEntry {
            id: id.clone(),
            user_id,
            action,
            resource_type,
            resource_id,
            timestamp: Utc::now(),
            result,
            error_message,
            tenant_id,
            details,
        };

        match self.logs.lock() {
            Ok(mut logs) => {
                logs.push(entry);
                Ok(id)
            }
            Err(e) => Err(format!("Failed to acquire lock: {e}")),
        }
    }

    /// 获取所有审计日志
    pub fn get_all(&self) -> Result<Vec<AuditLogEntry>, String> {
        match self.logs.lock() {
            Ok(logs) => Ok(logs.clone()),
            Err(e) => Err(format!("Failed to acquire lock: {e}")),
        }
    }

    /// 分页获取审计日志
    pub fn get_paginated(
        &self,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<AuditLogEntry>, usize), String> {
        match self.logs.lock() {
            Ok(logs) => {
                let total = logs.len();
                let items = logs.iter().skip(offset).take(limit).cloned().collect();
                Ok((items, total))
            }
            Err(e) => Err(format!("Failed to acquire lock: {e}")),
        }
    }

    /// 按时间范围获取审计日志
    pub fn get_by_time_range(
        &self,
        start_timestamp: i64,
        end_timestamp: i64,
    ) -> Result<Vec<AuditLogEntry>, String> {
        match self.logs.lock() {
            Ok(logs) => {
                let items = logs
                    .iter()
                    .filter(|log| {
                        let ts = log.timestamp.timestamp();
                        ts >= start_timestamp && ts <= end_timestamp
                    })
                    .cloned()
                    .collect();
                Ok(items)
            }
            Err(e) => Err(format!("Failed to acquire lock: {e}")),
        }
    }

    /// 按用户ID过滤审计日志
    pub fn get_by_user(&self, user_id: &str) -> Result<Vec<AuditLogEntry>, String> {
        match self.logs.lock() {
            Ok(logs) => {
                let items = logs
                    .iter()
                    .filter(|log| log.user_id == user_id)
                    .cloned()
                    .collect();
                Ok(items)
            }
            Err(e) => Err(format!("Failed to acquire lock: {e}")),
        }
    }

    /// 按资源ID过滤审计日志
    pub fn get_by_resource(
        &self,
        resource_type: &str,
        resource_id: &str,
    ) -> Result<Vec<AuditLogEntry>, String> {
        match self.logs.lock() {
            Ok(logs) => {
                let items = logs
                    .iter()
                    .filter(|log| {
                        log.resource_type == resource_type && log.resource_id == resource_id
                    })
                    .cloned()
                    .collect();
                Ok(items)
            }
            Err(e) => Err(format!("Failed to acquire lock: {e}")),
        }
    }

    /// 清除所有审计日志（仅用于测试）
    #[cfg(test)]
    pub fn clear(&self) -> Result<(), String> {
        match self.logs.lock() {
            Ok(mut logs) => {
                logs.clear();
                Ok(())
            }
            Err(e) => Err(format!("Failed to acquire lock: {e}")),
        }
    }
}

impl Default for AuditLogger {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for AuditLogger {
    fn clone(&self) -> Self {
        Self {
            logs: Arc::clone(&self.logs),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_logger_new() {
        let logger = AuditLogger::new();
        let logs = logger.get_all().unwrap();
        assert!(logs.is_empty());
    }

    #[test]
    fn test_audit_logger_log() {
        let logger = AuditLogger::new();
        let result = logger.log(
            "user1".to_string(),
            "read".to_string(),
            "memory".to_string(),
            "resource1".to_string(),
            AuditResult::Success,
            None,
            "tenant1".to_string(),
            None,
        );
        assert!(result.is_ok());

        let logs = logger.get_all().unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].user_id, "user1");
        assert_eq!(logs[0].action, "read");
        assert_eq!(logs[0].result, AuditResult::Success);
    }

    #[test]
    fn test_audit_logger_get_by_user() {
        let logger = AuditLogger::new();
        logger
            .log(
                "user1".to_string(),
                "read".to_string(),
                "memory".to_string(),
                "resource1".to_string(),
                AuditResult::Success,
                None,
                "tenant1".to_string(),
                None,
            )
            .unwrap();
        logger
            .log(
                "user2".to_string(),
                "create".to_string(),
                "space".to_string(),
                "resource2".to_string(),
                AuditResult::Success,
                None,
                "tenant1".to_string(),
                None,
            )
            .unwrap();

        let user1_logs = logger.get_by_user("user1").unwrap();
        assert_eq!(user1_logs.len(), 1);
        assert_eq!(user1_logs[0].user_id, "user1");

        let user2_logs = logger.get_by_user("user2").unwrap();
        assert_eq!(user2_logs.len(), 1);
        assert_eq!(user2_logs[0].user_id, "user2");
    }

    #[test]
    fn test_audit_logger_paginated() {
        let logger = AuditLogger::new();
        for i in 0..5 {
            logger
                .log(
                    format!("user{i}"),
                    "read".to_string(),
                    "memory".to_string(),
                    format!("resource{i}"),
                    AuditResult::Success,
                    None,
                    "tenant1".to_string(),
                    None,
                )
                .unwrap();
        }

        let (items, total) = logger.get_paginated(0, 2).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(total, 5);

        let (items, total) = logger.get_paginated(2, 2).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(total, 5);

        let (items, total) = logger.get_paginated(4, 2).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(total, 5);
    }

    #[test]
    fn test_audit_logger_get_by_resource() {
        let logger = AuditLogger::new();
        logger
            .log(
                "user1".to_string(),
                "read".to_string(),
                "memory".to_string(),
                "resource1".to_string(),
                AuditResult::Success,
                None,
                "tenant1".to_string(),
                None,
            )
            .unwrap();
        logger
            .log(
                "user2".to_string(),
                "update".to_string(),
                "memory".to_string(),
                "resource1".to_string(),
                AuditResult::Success,
                None,
                "tenant1".to_string(),
                None,
            )
            .unwrap();

        let logs = logger.get_by_resource("memory", "resource1").unwrap();
        assert_eq!(logs.len(), 2);
    }

    #[test]
    fn test_audit_logger_permission_denied() {
        let logger = AuditLogger::new();
        logger
            .log(
                "user1".to_string(),
                "delete".to_string(),
                "memory".to_string(),
                "resource1".to_string(),
                AuditResult::PermissionDenied,
                Some("User is not owner".to_string()),
                "tenant1".to_string(),
                None,
            )
            .unwrap();

        let logs = logger.get_all().unwrap();
        assert_eq!(logs[0].result, AuditResult::PermissionDenied);
        assert_eq!(logs[0].error_message, Some("User is not owner".to_string()));
    }

    #[test]
    fn test_audit_logger_clone() {
        let logger = AuditLogger::new();
        logger
            .log(
                "user1".to_string(),
                "read".to_string(),
                "memory".to_string(),
                "resource1".to_string(),
                AuditResult::Success,
                None,
                "tenant1".to_string(),
                None,
            )
            .unwrap();

        let logger2 = logger.clone();
        let logs = logger2.get_all().unwrap();
        assert_eq!(logs.len(), 1);
    }
}
