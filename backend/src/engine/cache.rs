use moka::future::Cache;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::domain::tetra::{MemoryPayload, TetraId};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CacheValue {
    pub results: Vec<(TetraId, f64, f64, MemoryPayload)>,
    pub timestamp: i64,
}

#[derive(Debug)]
pub struct CacheStats {
    pub l1_hits: AtomicU64,
    pub l1_misses: AtomicU64,
    pub l2_hits: AtomicU64,
    pub l2_misses: AtomicU64,
    pub evictions: AtomicU64,
}

impl CacheStats {
    pub fn new() -> Arc<Self> {
        Arc::new(CacheStats {
            l1_hits: AtomicU64::new(0),
            l1_misses: AtomicU64::new(0),
            l2_hits: AtomicU64::new(0),
            l2_misses: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
        })
    }

    pub fn hit_ratio(&self) -> f64 {
        let total_hits =
            self.l1_hits.load(Ordering::Relaxed) + self.l2_hits.load(Ordering::Relaxed);
        let total_misses =
            self.l1_misses.load(Ordering::Relaxed) + self.l2_misses.load(Ordering::Relaxed);
        let total = total_hits + total_misses;
        if total == 0 {
            0.0
        } else {
            (total_hits as f64 / total as f64) * 100.0
        }
    }

    pub fn l1_hit_ratio(&self) -> f64 {
        let hits = self.l1_hits.load(Ordering::Relaxed);
        let misses = self.l1_misses.load(Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 {
            0.0
        } else {
            (hits as f64 / total as f64) * 100.0
        }
    }

    pub fn l2_hit_ratio(&self) -> f64 {
        let hits = self.l2_hits.load(Ordering::Relaxed);
        let misses = self.l2_misses.load(Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 {
            0.0
        } else {
            (hits as f64 / total as f64) * 100.0
        }
    }
}

pub struct CacheLayer {
    l1_cache: Cache<String, CacheValue>,
    l2_client: Option<redis::Client>,
    stats: Arc<CacheStats>,
}

impl CacheLayer {
    pub fn generate_query_key(
        query: &str,
        filters: Option<&super::search_engine::SearchFilters>,
    ) -> String {
        let mut hasher = Sha256::new();
        hasher.update(query.as_bytes());

        if let Some(filters) = filters {
            if let Some(ref labels) = filters.labels {
                for label in labels {
                    hasher.update(label.as_bytes());
                }
            }
            if let Some(min_imp) = filters.min_importance {
                hasher.update(min_imp.to_le_bytes());
            }
            if let Some(max_imp) = filters.max_importance {
                hasher.update(max_imp.to_le_bytes());
            }
            if let Some(since) = filters.since_ts {
                hasher.update(since.to_le_bytes());
            }
            if let Some(until) = filters.until_ts {
                hasher.update(until.to_le_bytes());
            }
            if let Some(ref project) = filters.project {
                hasher.update(project.as_bytes());
            }
        }

        format!("search:{}", hex::encode(hasher.finalize()))
    }

    pub fn new(stats: Arc<CacheStats>) -> Self {
        let l1_cache = Cache::builder()
            .max_capacity(10_000)
            .time_to_live(Duration::from_secs(5 * 60))
            .build();

        let l2_client = std::env::var("REDIS_URL").ok().and_then(|url| {
            match redis::Client::open(url.as_str()) {
                Ok(client) => match client.get_connection() {
                    Ok(_conn) => {
                        tracing::info!("Redis L2 cache enabled");
                        Some(client)
                    }
                    Err(e) => {
                        tracing::warn!("Failed to connect to Redis: {}, using L1 only", e);
                        None
                    }
                },
                Err(e) => {
                    tracing::warn!("Invalid Redis URL: {}, using L1 only", e);
                    None
                }
            }
        });

        CacheLayer {
            l1_cache,
            l2_client,
            stats,
        }
    }

    pub fn generate_cache_key(
        query_vector: &[f64],
        filters: &super::search_engine::SearchFilters,
    ) -> String {
        let mut hasher = Sha256::new();

        for &v in query_vector {
            hasher.update(v.to_le_bytes());
        }

        if let Some(ref labels) = filters.labels {
            for label in labels {
                hasher.update(label.as_bytes());
            }
        }
        if let Some(min_imp) = filters.min_importance {
            hasher.update(min_imp.to_le_bytes());
        }
        if let Some(max_imp) = filters.max_importance {
            hasher.update(max_imp.to_le_bytes());
        }
        if let Some(since) = filters.since_ts {
            hasher.update(since.to_le_bytes());
        }
        if let Some(until) = filters.until_ts {
            hasher.update(until.to_le_bytes());
        }
        if let Some(ref project) = filters.project {
            hasher.update(project.as_bytes());
        }

        format!("search:{}", hex::encode(hasher.finalize()))
    }

    pub async fn get(&self, key: &str) -> Option<CacheValue> {
        if let Some(value) = self.l1_cache.get(key).await {
            self.stats.l1_hits.fetch_add(1, Ordering::Relaxed);
            tracing::debug!("L1 cache hit for key: {}", key);
            return Some(value);
        }

        self.stats.l1_misses.fetch_add(1, Ordering::Relaxed);

        if let Some(client) = self.l2_client.as_ref() {
            if let Ok(mut conn) = client.get_connection() {
                if let Ok(Some(data)) = redis::cmd("GET")
                    .arg(key)
                    .query::<Option<Vec<u8>>>(&mut conn)
                {
                    if let Ok(value) = bincode::deserialize::<CacheValue>(&data) {
                        self.stats.l2_hits.fetch_add(1, Ordering::Relaxed);
                        let _ = self.l1_cache.insert(key.to_string(), value.clone()).await;
                        tracing::debug!("L2 cache hit for key: {}", key);
                        return Some(value);
                    }
                }
            }
        }

        self.stats.l2_misses.fetch_add(1, Ordering::Relaxed);
        tracing::debug!("Cache miss for key: {}", key);
        None
    }

    pub async fn set(&self, key: String, value: CacheValue) {
        let _ = self.l1_cache.insert(key.clone(), value.clone()).await;

        if let Some(client) = self.l2_client.as_ref() {
            if let Ok(serialized) = bincode::serialize(&value) {
                if let Ok(mut conn) = client.get_connection() {
                    let ttl = 30 * 60;
                    let _ = redis::cmd("SETEX")
                        .arg(&key)
                        .arg(ttl)
                        .arg(&serialized)
                        .query::<()>(&mut conn);
                }
            }
        }

        tracing::debug!("Cache set for key: {}", key);
    }

    pub async fn invalidate(&self, pattern: &str) {
        self.l1_cache.invalidate_all();

        if let Some(client) = self.l2_client.as_ref() {
            if let Ok(mut conn) = client.get_connection() {
                let keys: Vec<String> = redis::cmd("KEYS")
                    .arg(pattern)
                    .query(&mut conn)
                    .unwrap_or_default();

                for key in keys {
                    let _ = redis::cmd("DEL").arg(&key).query::<()>(&mut conn);
                }
            }
        }

        tracing::debug!("Cache invalidated for pattern: {}", pattern);
    }

    pub async fn invalidate_by_key(&self, key: &str) {
        self.l1_cache.invalidate(key).await;

        if let Some(client) = self.l2_client.as_ref() {
            if let Ok(mut conn) = client.get_connection() {
                let _ = redis::cmd("DEL").arg(key).query::<()>(&mut conn);
            }
        }

        tracing::debug!("Cache invalidated for key: {}", key);
    }

    pub fn stats(&self) -> Arc<CacheStats> {
        self.stats.clone()
    }

    pub async fn clear(&self) {
        self.l1_cache.invalidate_all();
        if let Some(client) = self.l2_client.as_ref() {
            if let Ok(mut conn) = client.get_connection() {
                let _ = redis::cmd("FLUSHDB").query::<()>(&mut conn);
            }
        }
        tracing::info!("Cache cleared");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_key_generation() {
        let vector = vec![0.1, 0.2, 0.3];
        let filters = super::super::search_engine::SearchFilters::default();
        let key = CacheLayer::generate_cache_key(&vector, &filters);
        assert!(key.starts_with("search:"));
        assert!(!key.is_empty());
    }

    #[test]
    fn test_query_cache_key_generation() {
        let filters = super::super::search_engine::SearchFilters::default();
        let key = CacheLayer::generate_query_key("hello world", Some(&filters));
        assert!(key.starts_with("search:"));
        assert!(!key.is_empty());
    }

    #[test]
    fn test_cache_stats() {
        let stats = CacheStats::new();
        assert_eq!(stats.hit_ratio(), 0.0);

        stats.l1_hits.fetch_add(1, Ordering::Relaxed);
        stats.l1_misses.fetch_add(1, Ordering::Relaxed);
        assert_eq!(stats.hit_ratio(), 50.0);
    }
}
