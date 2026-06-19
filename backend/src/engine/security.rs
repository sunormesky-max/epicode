use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

const RATE_LIMIT_WINDOW_SECS: u64 = 60;
const RATE_LIMIT_MAX_REQUESTS: u64 = 120;
const MAX_CONTENT_LENGTH: usize = 10000;
const MAX_QUERY_LENGTH: usize = 2000;
const MAX_LABELS_PER_MEMORY: usize = 10;
const MAX_LABEL_LENGTH: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    pub enabled: bool,
    pub api_keys: Vec<String>,
    pub rate_limit_per_minute: u64,
    pub max_content_length: usize,
    pub max_query_length: usize,
    pub max_labels: usize,
    pub audit_log_size: usize,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        let key = std::env::var("TETRAMEM_API_KEY")
            .ok()
            .filter(|k| !k.is_empty());
        let allow_insecure = matches!(
            std::env::var("TETRAMEM_ALLOW_INSECURE_AUTH"),
            Ok(v) if v == "1" || v.eq_ignore_ascii_case("true")
        ) || cfg!(test)
            || cfg!(debug_assertions);
        let (enabled, api_keys) = match key {
            Some(k) => (true, vec![k]),
            None if allow_insecure => {
                tracing::warn!(
                    "TETRAMEM_API_KEY not set — insecure auth is enabled only for local/dev use"
                );
                (false, vec![])
            }
            None => {
                panic!(
                    "FATAL: TETRAMEM_API_KEY must be set. Generate one with: openssl rand -base64 32"
                );
            }
        };
        Self {
            enabled,
            api_keys,
            rate_limit_per_minute: RATE_LIMIT_MAX_REQUESTS,
            max_content_length: MAX_CONTENT_LENGTH,
            max_query_length: MAX_QUERY_LENGTH,
            max_labels: MAX_LABELS_PER_MEMORY,
            audit_log_size: 200,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: i64,
    pub action: String,
    pub client: String,
    pub result: SecurityResult,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum SecurityResult {
    Allowed,
    DeniedAuth,
    DeniedRateLimit,
    DeniedValidation,
    DeniedConstitution,
    DeniedEnergy,
}

#[derive(Debug, Clone)]
struct RateBucket {
    timestamps: Vec<std::time::Instant>,
}

pub struct SecurityGuard {
    pub config: SecurityConfig,
    rate_buckets: Mutex<HashMap<String, RateBucket>>,
    audit_log: Mutex<Vec<AuditEntry>>,
    total_requests: AtomicU64,
    total_denied: AtomicU64,
    denied_auth_count: AtomicUsize,
    denied_rate_count: AtomicUsize,
    denied_validation_count: AtomicUsize,
    denied_constitution_count: AtomicUsize,
    denied_energy_count: AtomicUsize,
}

impl SecurityGuard {
    pub fn new(config: SecurityConfig) -> Self {
        Self {
            config,
            rate_buckets: Mutex::new(HashMap::new()),
            audit_log: Mutex::new(Vec::new()),
            total_requests: AtomicU64::new(0),
            total_denied: AtomicU64::new(0),
            denied_auth_count: AtomicUsize::new(0),
            denied_rate_count: AtomicUsize::new(0),
            denied_validation_count: AtomicUsize::new(0),
            denied_constitution_count: AtomicUsize::new(0),
            denied_energy_count: AtomicUsize::new(0),
        }
    }

    pub fn from_env() -> Self {
        Self::new(SecurityConfig::default())
    }

    pub fn authenticate(&self, api_key: &str) -> Result<String, SecurityResult> {
        if !self.config.enabled {
            return Ok("anonymous".to_string());
        }
        if self.config.api_keys.is_empty() {
            return Ok("anonymous".to_string());
        }
        let mut matched = false;
        for k in &self.config.api_keys {
            let mut diff = 0u8;
            let max_len = k.len().max(api_key.len());
            for i in 0..max_len {
                let a = k.as_bytes().get(i).copied().unwrap_or(0);
                let b = api_key.as_bytes().get(i).copied().unwrap_or(0);
                diff |= a ^ b;
            }
            diff |= (k.len() != api_key.len()) as u8;
            if diff == 0 {
                matched = true;
            }
        }
        if matched {
            let client_id = Self::mask_key(api_key);
            Ok(client_id)
        } else {
            Err(SecurityResult::DeniedAuth)
        }
    }

    pub fn check_rate_limit(&self, client_id: &str) -> Result<(), SecurityResult> {
        let rate_key = Self::hash_key(client_id);
        let mut buckets = self.rate_buckets.lock();
        let now = Instant::now();
        let limit = self.config.rate_limit_per_minute;

        if buckets.len() > 10000 {
            buckets.retain(|_, b| {
                b.timestamps
                    .last()
                    .map(|t| now.duration_since(*t).as_secs() < RATE_LIMIT_WINDOW_SECS)
                    .unwrap_or(false)
            });
        }

        let bucket = buckets.entry(rate_key).or_insert(RateBucket {
            timestamps: Vec::new(),
        });

        let cutoff = now - std::time::Duration::from_secs(RATE_LIMIT_WINDOW_SECS);
        bucket.timestamps.retain(|t| *t > cutoff);

        if bucket.timestamps.len() >= limit as usize {
            return Err(SecurityResult::DeniedRateLimit);
        }

        bucket.timestamps.push(now);
        Ok(())
    }

    pub fn validate_content(&self, content: &str) -> Result<(), SecurityResult> {
        if content.trim().is_empty() {
            return Err(SecurityResult::DeniedValidation);
        }
        if content.len() > self.config.max_content_length {
            return Err(SecurityResult::DeniedValidation);
        }
        Ok(())
    }

    pub fn validate_query(&self, query: &str) -> Result<(), SecurityResult> {
        if query.trim().is_empty() {
            return Err(SecurityResult::DeniedValidation);
        }
        if query.len() > self.config.max_query_length {
            return Err(SecurityResult::DeniedValidation);
        }
        Ok(())
    }

    pub fn validate_labels(&self, labels: &[String]) -> Result<(), SecurityResult> {
        if labels.len() > self.config.max_labels {
            return Err(SecurityResult::DeniedValidation);
        }
        for label in labels {
            if label.len() > MAX_LABEL_LENGTH || label.trim().is_empty() {
                return Err(SecurityResult::DeniedValidation);
            }
            if !label.chars().all(|c| {
                c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c.is_whitespace()
            }) {
                return Err(SecurityResult::DeniedValidation);
            }
        }
        Ok(())
    }

    pub fn check_constitution_create(&self, content_exists: bool) -> Result<(), SecurityResult> {
        if !content_exists {
            return Err(SecurityResult::DeniedValidation);
        }
        Ok(())
    }

    pub fn check_constitution_delete(&self) -> Result<(), SecurityResult> {
        Err(SecurityResult::DeniedConstitution)
    }

    pub fn check_constitution_fission(
        &self,
        entropy: f64,
        cluster_size: usize,
    ) -> Result<(), SecurityResult> {
        if cluster_size >= 30 {
            return Ok(());
        }
        if entropy < 0.4 {
            return Err(SecurityResult::DeniedConstitution);
        }
        if cluster_size < 6 {
            return Err(SecurityResult::DeniedConstitution);
        }
        Ok(())
    }

    pub fn check_constitution_blend(&self) -> Result<(), SecurityResult> {
        Err(SecurityResult::DeniedConstitution)
    }

    pub fn check_energy(&self, available: f64, cost: f64) -> Result<(), SecurityResult> {
        if available < cost {
            Err(SecurityResult::DeniedEnergy)
        } else {
            Ok(())
        }
    }

    pub fn audit(&self, action: &str, client: &str, result: SecurityResult, detail: &str) {
        let entry = AuditEntry {
            timestamp: chrono::Utc::now().timestamp(),
            action: action.to_string(),
            client: client.to_string(),
            result,
            detail: detail.to_string(),
        };

        self.total_requests.fetch_add(1, Ordering::SeqCst);
        if result != SecurityResult::Allowed {
            self.total_denied.fetch_add(1, Ordering::SeqCst);
            match result {
                SecurityResult::DeniedAuth => {
                    self.denied_auth_count.fetch_add(1, Ordering::SeqCst);
                }
                SecurityResult::DeniedRateLimit => {
                    self.denied_rate_count.fetch_add(1, Ordering::SeqCst);
                }
                SecurityResult::DeniedValidation => {
                    self.denied_validation_count.fetch_add(1, Ordering::SeqCst);
                }
                SecurityResult::DeniedConstitution => {
                    self.denied_constitution_count
                        .fetch_add(1, Ordering::SeqCst);
                }
                SecurityResult::DeniedEnergy => {
                    self.denied_energy_count.fetch_add(1, Ordering::SeqCst);
                }
                SecurityResult::Allowed => {}
            }
        }

        let mut log = self.audit_log.lock();
        log.push(entry);
        if log.len() > self.config.audit_log_size {
            let excess = log.len() - self.config.audit_log_size;
            log.drain(0..excess);
        }

        if result != SecurityResult::Allowed {
            tracing::warn!(
                "[Security] {} by {} — {:?}: {}",
                action,
                client,
                result,
                detail
            );
        }
    }

    pub fn full_check(
        &self,
        api_key: &str,
        action: &str,
    ) -> Result<String, (SecurityResult, String)> {
        let client = match self.authenticate(api_key) {
            Ok(c) => c,
            Err(r) => {
                self.audit(action, &Self::mask_key(api_key), r, "authentication failed");
                return Err((r, "authentication failed".to_string()));
            }
        };

        if let Err(r) = self.check_rate_limit(&client) {
            self.audit(action, &client, r, "rate limit exceeded");
            return Err((r, "rate limit exceeded".to_string()));
        }

        self.audit(action, &client, SecurityResult::Allowed, "passed");
        Ok(client)
    }

    pub fn stats(&self) -> SecurityStats {
        SecurityStats {
            enabled: self.config.enabled,
            total_requests: self.total_requests.load(Ordering::SeqCst),
            total_denied: self.total_denied.load(Ordering::SeqCst),
            denied_auth: self.denied_auth_count.load(Ordering::SeqCst),
            denied_rate_limit: self.denied_rate_count.load(Ordering::SeqCst),
            denied_validation: self.denied_validation_count.load(Ordering::SeqCst),
            denied_constitution: self.denied_constitution_count.load(Ordering::SeqCst),
            denied_energy: self.denied_energy_count.load(Ordering::SeqCst),
            rate_limit_per_minute: self.config.rate_limit_per_minute,
            max_content_length: self.config.max_content_length,
            audit_entries: self.audit_log.lock().len(),
        }
    }

    pub fn audit_log(&self, limit: usize) -> Vec<AuditEntry> {
        let log = self.audit_log.lock();
        log.iter().rev().take(limit).cloned().collect()
    }

    fn mask_key(key: &str) -> String {
        if key.len() <= 8 {
            return "*".repeat(key.len());
        }
        format!("{}****{}", &key[..3], &key[key.len() - 2..])
    }

    fn hash_key(key: &str) -> String {
        let mut h: u64 = 14695981039346656037;
        for b in key.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        format!("rl:{:016x}", h)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SecurityStats {
    pub enabled: bool,
    pub total_requests: u64,
    pub total_denied: u64,
    pub denied_auth: usize,
    pub denied_rate_limit: usize,
    pub denied_validation: usize,
    pub denied_constitution: usize,
    pub denied_energy: usize,
    pub rate_limit_per_minute: u64,
    pub max_content_length: usize,
    pub audit_entries: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_guard() -> SecurityGuard {
        SecurityGuard::new(SecurityConfig {
            enabled: true,
            api_keys: vec!["test-key-123".to_string()],
            rate_limit_per_minute: 5,
            max_content_length: 100,
            max_query_length: 50,
            max_labels: 5,
            audit_log_size: 50,
        })
    }

    #[test]
    fn authenticate_valid_key() {
        let guard = test_guard();
        let result = guard.authenticate("test-key-123");
        assert!(result.is_ok());
    }

    #[test]
    fn authenticate_invalid_key() {
        let guard = test_guard();
        let result = guard.authenticate("wrong-key");
        assert_eq!(result.unwrap_err(), SecurityResult::DeniedAuth);
    }

    #[test]
    fn rate_limit_allows_within_limit() {
        let guard = test_guard();
        for _ in 0..5 {
            assert!(guard.check_rate_limit("client1").is_ok());
        }
    }

    #[test]
    fn rate_limit_blocks_over_limit() {
        let guard = test_guard();
        for _ in 0..5 {
            guard.check_rate_limit("client1").ok();
        }
        let result = guard.check_rate_limit("client1");
        assert_eq!(result.unwrap_err(), SecurityResult::DeniedRateLimit);
    }

    #[test]
    fn rate_limit_independent_clients() {
        let guard = test_guard();
        for _ in 0..5 {
            guard.check_rate_limit("client1").ok();
        }
        assert!(guard.check_rate_limit("client2").is_ok());
    }

    #[test]
    fn validate_content_ok() {
        let guard = test_guard();
        assert!(guard.validate_content("hello world").is_ok());
    }

    #[test]
    fn validate_content_empty() {
        let guard = test_guard();
        assert_eq!(
            guard.validate_content("").unwrap_err(),
            SecurityResult::DeniedValidation
        );
    }

    #[test]
    fn validate_content_too_long() {
        let guard = test_guard();
        let long = "x".repeat(101);
        assert_eq!(
            guard.validate_content(&long).unwrap_err(),
            SecurityResult::DeniedValidation
        );
    }

    #[test]
    fn validate_query_too_long() {
        let guard = test_guard();
        let long = "q".repeat(51);
        assert_eq!(
            guard.validate_query(&long).unwrap_err(),
            SecurityResult::DeniedValidation
        );
    }

    #[test]
    fn validate_labels_too_many() {
        let guard = test_guard();
        let labels: Vec<String> = (0..6).map(|i| format!("label{}", i)).collect();
        assert_eq!(
            guard.validate_labels(&labels).unwrap_err(),
            SecurityResult::DeniedValidation
        );
    }

    #[test]
    fn constitution_blocks_delete() {
        let guard = test_guard();
        assert_eq!(
            guard.check_constitution_delete().unwrap_err(),
            SecurityResult::DeniedConstitution
        );
    }

    #[test]
    fn constitution_blocks_fission() {
        let guard = test_guard();
        assert_eq!(
            guard.check_constitution_fission(0.1, 3).unwrap_err(),
            SecurityResult::DeniedConstitution
        );
    }

    #[test]
    fn constitution_allows_fission_when_entropy_high() {
        let guard = test_guard();
        assert!(guard.check_constitution_fission(0.6, 10).is_ok());
    }

    #[test]
    fn constitution_blocks_fission_small_cluster() {
        let guard = test_guard();
        assert_eq!(
            guard.check_constitution_fission(0.8, 3).unwrap_err(),
            SecurityResult::DeniedConstitution
        );
    }

    #[test]
    fn constitution_blocks_blend() {
        let guard = test_guard();
        assert_eq!(
            guard.check_constitution_blend().unwrap_err(),
            SecurityResult::DeniedConstitution
        );
    }

    #[test]
    fn energy_check_insufficient() {
        let guard = test_guard();
        assert_eq!(
            guard.check_energy(5.0, 10.0).unwrap_err(),
            SecurityResult::DeniedEnergy
        );
    }

    #[test]
    fn energy_check_sufficient() {
        let guard = test_guard();
        assert!(guard.check_energy(100.0, 10.0).is_ok());
    }

    #[test]
    fn full_check_ok() {
        let guard = test_guard();
        let result = guard.full_check("test-key-123", "test");
        assert!(result.is_ok());
    }

    #[test]
    fn full_check_bad_key() {
        let guard = test_guard();
        let result = guard.full_check("bad-key", "test");
        assert!(result.is_err());
    }

    #[test]
    fn audit_log_entries() {
        let guard = test_guard();
        guard.audit("create", "client1", SecurityResult::Allowed, "ok");
        guard.audit(
            "delete",
            "client1",
            SecurityResult::DeniedConstitution,
            "forbidden",
        );
        let log = guard.audit_log(10);
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].action, "delete");
        assert_eq!(log[1].action, "create");
    }

    #[test]
    fn stats_counters() {
        let guard = test_guard();
        guard.audit("test", "c1", SecurityResult::Allowed, "ok");
        guard.audit("test", "c1", SecurityResult::DeniedAuth, "bad key");
        let stats = guard.stats();
        assert_eq!(stats.total_requests, 2);
        assert_eq!(stats.total_denied, 1);
    }

    #[test]
    fn disabled_security_allows_all() {
        let guard = SecurityGuard::new(SecurityConfig {
            enabled: false,
            api_keys: vec![],
            rate_limit_per_minute: 0,
            max_content_length: 0,
            max_query_length: 0,
            max_labels: 0,
            audit_log_size: 10,
        });
        assert!(guard.authenticate("anything").is_ok());
    }

    #[test]
    fn mask_key_hides_middle() {
        let masked = SecurityGuard::mask_key("abcdefghijklmnop");
        assert_eq!(masked, "abc****op");
    }
}
