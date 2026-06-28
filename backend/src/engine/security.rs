use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::crypto::constant_time_eq;

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
    /// Optional administrative key. When set, sensitive endpoints
    /// (`/admin/*`, `/config`) require this key instead of the regular API key,
    /// preventing a standard client from escalating to admin operations.
    /// When `None`, admin endpoints fall back to requiring a regular API key
    /// (single-tenant mode) — but only when security is enabled.
    pub admin_key: Option<String>,
    pub rate_limit_per_minute: u64,
    pub max_content_length: usize,
    pub max_query_length: usize,
    pub max_labels: usize,
    pub audit_log_size: usize,
    pub tenant_quotas: HashMap<String, usize>,
    pub max_tenants: usize,
}

fn env_var(name: &str) -> Result<String, std::env::VarError> {
    std::env::var(format!("EPICODE_{}", name))
        .or_else(|_| std::env::var(format!("TETRAMEM_{}", name)))
}

impl SecurityConfig {
    /// Try to build a `SecurityConfig` from environment variables.
    /// Returns `Err` if `TETRAMEM_API_KEY` is not set and insecure auth is not allowed.
    pub fn try_from_env() -> Result<Self, String> {
        let key = env_var("API_KEY").ok().filter(|k| !k.is_empty());
        // Insecure auth must be an explicit opt-in. We deliberately do NOT
        // auto-enable it for `cfg!(debug_assertions)` builds: a debug binary
        // accidentally deployed to production would otherwise expose every
        // endpoint with no authentication. Operators must set ALLOW_INSECURE_AUTH=1
        // (and only when EPICODE_API_KEY is unset) to opt in.
        let allow_insecure = matches!(
            env_var("ALLOW_INSECURE_AUTH"),
            Ok(v) if v == "1" || v.eq_ignore_ascii_case("true")
        ) || cfg!(test);
        let admin_key = env_var("ADMIN_KEY").ok().filter(|k| !k.is_empty());
        let (enabled, api_keys) = match key {
            Some(k) => (true, vec![k]),
            None if allow_insecure => {
                tracing::warn!(
                    "EPICODE_API_KEY (or TETRAMEM_API_KEY) not set — insecure auth is enabled only for local/dev use"
                );
                (false, vec![])
            }
            None => {
                return Err(
                    "EPICODE_API_KEY (or TETRAMEM_API_KEY) must be set. Generate one with: openssl rand -base64 32"
                        .to_string(),
                );
            }
        };
        Ok(Self {
            enabled,
            api_keys,
            admin_key,
            rate_limit_per_minute: RATE_LIMIT_MAX_REQUESTS,
            max_content_length: MAX_CONTENT_LENGTH,
            max_query_length: MAX_QUERY_LENGTH,
            max_labels: MAX_LABELS_PER_MEMORY,
            audit_log_size: 200,
            tenant_quotas: HashMap::new(),
            max_tenants: 1000,
        })
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self::try_from_env().unwrap_or_else(|e| {
            panic!("FATAL: {e}");
        })
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
    DeniedQuota,
}

#[derive(Debug, Clone)]
struct RateBucket {
    window_start: Instant,
    count: u64,
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
    denied_quota_count: AtomicUsize,
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
            denied_quota_count: AtomicUsize::new(0),
        }
    }

    pub fn from_env() -> Self {
        Self::new(SecurityConfig::default())
    }

    /// Fallible version that returns `Err` instead of panicking.
    pub fn try_from_env() -> Result<Self, String> {
        Ok(Self::new(SecurityConfig::try_from_env()?))
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



    pub fn extract_tenant_id(api_key: &str) -> String {
        api_key.split(':').next().unwrap_or("default").to_string()
    }

    pub fn check_tenant_quota(&self, tenant_id: &str, current_count: usize) -> Result<(), SecurityResult> {
        if let Some(&quota) = self.config.tenant_quotas.get(tenant_id) {
            if current_count >= quota {
                self.denied_quota_count.fetch_add(1, Ordering::SeqCst);
                return Err(SecurityResult::DeniedQuota);
            }
        }
        Ok(())
    }
    pub fn check_rate_limit(&self, client_id: &str) -> Result<(), SecurityResult> {
        let tenant_id = Self::extract_tenant_id(client_id);
        let rate_key = format!("{}:{}", tenant_id, Self::hash_key(client_id));
        let mut buckets = self.rate_buckets.lock();
        let now = Instant::now();
        let limit = self.config.rate_limit_per_minute;
        let window = std::time::Duration::from_secs(RATE_LIMIT_WINDOW_SECS);

        if buckets.len() > self.config.max_tenants * 2 {
            buckets.retain(|_, b| now.saturating_duration_since(b.window_start) < window);
        }

        if buckets.len() >= self.config.max_tenants && !buckets.contains_key(&rate_key) {
            return Err(SecurityResult::DeniedRateLimit);
        }

        let bucket = buckets.entry(rate_key).or_insert(RateBucket {
            window_start: now,
            count: 0,
        });

        if now.saturating_duration_since(bucket.window_start) >= window {
            bucket.window_start = now;
            bucket.count = 0;
        }

        if bucket.count >= limit {
            return Err(SecurityResult::DeniedRateLimit);
        }

        bucket.count += 1;
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
                SecurityResult::DeniedQuota => {
                    self.denied_quota_count.fetch_add(1, Ordering::SeqCst);
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
            denied_quota: self.denied_quota_count.load(Ordering::SeqCst),
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
        let chars: Vec<char> = key.chars().collect();
        if chars.len() <= 8 {
            return "*".repeat(chars.len());
        }
        // Use char indexing (not byte slicing) so multi-byte UTF-8 keys can never
        // panic on a char boundary — a remotely-supplied X-API-Key could otherwise
        // crash the server, and `panic = "abort"` would kill the process.
        let head: String = chars.iter().take(3).collect();
        let tail: String = chars.iter().rev().take(2).rev().collect();
        format!("{head}****{tail}")
    }

    /// Check whether `api_key` is permitted to perform administrative operations.
    /// Uses constant-time comparison to avoid leaking the admin key via timing.
    /// Returns `true` when security is disabled (local/dev), or when the supplied
    /// key matches the configured admin key, or — in single-tenant mode where no
    /// admin key is configured — when it matches the regular API key.
    pub fn check_admin(&self, api_key: &str) -> bool {
        if !self.config.enabled {
            return true;
        }
        if let Some(admin) = &self.config.admin_key {
            return constant_time_eq(api_key, admin);
        }
        // Fallback: regular API key doubles as admin key in single-tenant mode.
        self.config
            .api_keys
            .iter()
            .any(|k| constant_time_eq(api_key, k))
    }

    fn hash_key(key: &str) -> String {
        let mut h: u64 = 14695981039346656037;
        for b in key.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        format!("rl:{h:016x}")
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
    pub denied_quota: usize,
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
            admin_key: None,
            rate_limit_per_minute: 5,
            max_content_length: 100,
            max_query_length: 50,
            max_labels: 5,
            audit_log_size: 50,
            tenant_quotas: std::collections::HashMap::new(),
            max_tenants: 100,
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
        let labels: Vec<String> = (0..6).map(|i| format!("label{i}")).collect();
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
            admin_key: None,
            rate_limit_per_minute: 0,
            max_content_length: 0,
            max_query_length: 0,
            max_labels: 0,
            audit_log_size: 10,
            tenant_quotas: std::collections::HashMap::new(),
            max_tenants: 100,
        });
        assert!(guard.authenticate("anything").is_ok());
    }

    #[test]
    fn mask_key_hides_middle() {
        let masked = SecurityGuard::mask_key("abcdefghijklmnop");
        assert_eq!(masked, "abc****op");
    }

    #[test]
    fn mask_key_handles_multibyte_without_panic() {
        // A remotely-supplied X-API-Key with multi-byte UTF-8 must not panic
        // (regression for the old `&key[..3]` byte slice).
        let masked = SecurityGuard::mask_key("🔑secure-key-🔐");
        assert!(masked.contains("****"));
    }

    #[test]
    fn check_admin_with_dedicated_admin_key() {
        let guard = SecurityGuard::new(SecurityConfig {
            enabled: true,
            api_keys: vec!["regular-key".to_string()],
            admin_key: Some("admin-secret".to_string()),
            rate_limit_per_minute: 5,
            max_content_length: 100,
            max_query_length: 50,
            max_labels: 5,
            audit_log_size: 50,
            tenant_quotas: std::collections::HashMap::new(),
            max_tenants: 100,
        });
        assert!(!guard.check_admin("regular-key"));
        assert!(guard.check_admin("admin-secret"));
        assert!(!guard.check_admin("wrong"));
    }

    #[test]
    fn check_admin_single_tenant_falls_back_to_api_key() {
        let guard = SecurityGuard::new(SecurityConfig {
            enabled: true,
            api_keys: vec!["regular-key".to_string()],
            admin_key: None,
            rate_limit_per_minute: 5,
            max_content_length: 100,
            max_query_length: 50,
            max_labels: 5,
            audit_log_size: 50,
            tenant_quotas: std::collections::HashMap::new(),
            max_tenants: 100,
        });
        assert!(guard.check_admin("regular-key"));
        assert!(!guard.check_admin("other"));
    }
}
