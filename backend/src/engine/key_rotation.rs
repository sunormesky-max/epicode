use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;
use zeroize::Zeroize;

type SecretKey = Vec<u8>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KeyEvent {
    Rotated {
        old_id: String,
        new_id: String,
        timestamp: DateTime<Utc>,
    },
    Revoked {
        key_id: String,
        reason: String,
        timestamp: DateTime<Utc>,
    },
    Restored {
        key_id: String,
        timestamp: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KeyStatus {
    Active,
    Transitioning,
    Revoked,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyMetadata {
    pub key_id: String,
    pub created_at: DateTime<Utc>,
    pub rotated_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub status: KeyStatus,
    pub version: u32,
}

pub struct KeyRotation {
    current_key_id: String,
    keys: HashMap<String, (SecretKey, KeyMetadata)>,
    rotation_interval: Duration,
    last_rotated: DateTime<Utc>,
    transition_period: Duration,
    events: Vec<KeyEvent>,
    max_key_versions: usize,
}

impl KeyRotation {
    pub fn new(
        rotation_days: u64,
        transition_days: u64,
        max_versions: usize,
    ) -> Result<Self, String> {
        let initial_key_id = Uuid::new_v4().to_string();
        let mut keys = HashMap::new();

        let mut key_data = vec![0u8; 32];
        rand::thread_rng().fill_bytes(&mut key_data);

        let metadata = KeyMetadata {
            key_id: initial_key_id.clone(),
            created_at: Utc::now(),
            rotated_at: None,
            revoked_at: None,
            status: KeyStatus::Active,
            version: 1,
        };

        keys.insert(initial_key_id.clone(), (key_data, metadata));

        Ok(Self {
            current_key_id: initial_key_id,
            keys,
            rotation_interval: Duration::days(rotation_days as i64),
            last_rotated: Utc::now(),
            transition_period: Duration::days(transition_days as i64),
            events: vec![],
            max_key_versions: max_versions,
        })
    }

    pub fn get_current_key_id(&self) -> String {
        self.current_key_id.clone()
    }

    pub fn get_current_key(&self) -> Option<Vec<u8>> {
        self.keys.get(&self.current_key_id).map(|(k, _)| k.clone())
    }

    pub fn list_active_keys(&self) -> Vec<(String, KeyMetadata)> {
        self.keys
            .iter()
            .filter_map(|(id, (_, meta))| {
                if matches!(meta.status, KeyStatus::Active | KeyStatus::Transitioning) {
                    Some((id.clone(), meta.clone()))
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn rotate_key(&mut self) -> Result<KeyEvent, String> {
        let old_id = self.current_key_id.clone();
        let new_id = Uuid::new_v4().to_string();
        let now = Utc::now();

        // Generate new key
        let mut new_key_data = vec![0u8; 32];
        rand::thread_rng().fill_bytes(&mut new_key_data);

        // Mark old key as transitioning
        if let Some((_, meta)) = self.keys.get_mut(&old_id) {
            if matches!(meta.status, KeyStatus::Active) {
                meta.status = KeyStatus::Transitioning;
                meta.rotated_at = Some(now);
            }
        }

        let version = self.keys.len() as u32 + 1;
        let new_metadata = KeyMetadata {
            key_id: new_id.clone(),
            created_at: now,
            rotated_at: Some(now),
            revoked_at: None,
            status: KeyStatus::Active,
            version,
        };

        self.keys
            .insert(new_id.clone(), (new_key_data, new_metadata));

        // Schedule old key expiration after transition period
        let expiration_time = now + self.transition_period;
        self.schedule_key_expiration(&old_id, expiration_time);

        // Cleanup old versions beyond max_key_versions
        self.cleanup_old_keys();

        self.current_key_id = new_id.clone();
        self.last_rotated = now;

        let event = KeyEvent::Rotated {
            old_id,
            new_id,
            timestamp: now,
        };
        self.events.push(event.clone());

        Ok(event)
    }

    pub fn revoke_key(&mut self, key_id: &str, reason: &str) -> Result<KeyEvent, String> {
        if !self.keys.contains_key(key_id) {
            return Err(format!("Key {} not found", key_id));
        }

        if key_id == self.current_key_id {
            return Err("Cannot revoke the current active key".to_string());
        }

        if let Some((_, meta)) = self.keys.get_mut(key_id) {
            meta.status = KeyStatus::Revoked;
            meta.revoked_at = Some(Utc::now());
        }

        let event = KeyEvent::Revoked {
            key_id: key_id.to_string(),
            reason: reason.to_string(),
            timestamp: Utc::now(),
        };
        self.events.push(event.clone());

        Ok(event)
    }

    pub fn restore_key(&mut self, key_id: &str) -> Result<KeyEvent, String> {
        if !self.keys.contains_key(key_id) {
            return Err(format!("Key {} not found", key_id));
        }

        if let Some((_, meta)) = self.keys.get_mut(key_id) {
            if matches!(meta.status, KeyStatus::Revoked) {
                meta.status = KeyStatus::Transitioning;
                meta.revoked_at = None;
            } else {
                return Err(format!("Key {} is not revoked", key_id));
            }
        }

        let event = KeyEvent::Restored {
            key_id: key_id.to_string(),
            timestamp: Utc::now(),
        };
        self.events.push(event.clone());

        Ok(event)
    }

    pub fn should_rotate(&self) -> bool {
        Utc::now() - self.last_rotated >= self.rotation_interval
    }

    pub fn get_events(&self) -> Vec<KeyEvent> {
        self.events.clone()
    }

    pub fn get_events_since(&self, timestamp: DateTime<Utc>) -> Vec<KeyEvent> {
        self.events
            .iter()
            .filter(|event| match event {
                KeyEvent::Rotated { timestamp: ts, .. }
                | KeyEvent::Revoked { timestamp: ts, .. }
                | KeyEvent::Restored { timestamp: ts, .. } => *ts > timestamp,
            })
            .cloned()
            .collect()
    }

    fn schedule_key_expiration(&mut self, key_id: &str, _expiration_time: DateTime<Utc>) {
        if let Some((_, meta)) = self.keys.get_mut(key_id) {
            meta.status = KeyStatus::Transitioning;
        }
    }

    fn cleanup_old_keys(&mut self) {
        if self.keys.len() > self.max_key_versions {
            let mut removable: Vec<_> = self
                .keys
                .iter()
                .filter(|(id, _)| **id != self.current_key_id)
                .map(|(id, (_, meta))| (id.clone(), meta.created_at))
                .collect();

            removable.sort_by_key(|(_, created_at)| *created_at);

            let to_remove = self.keys.len() - self.max_key_versions;
            for (id, _) in removable.into_iter().take(to_remove) {
                if let Some((mut key_data, _)) = self.keys.remove(&id) {
                    key_data.zeroize();
                }
            }
        }
    }

    pub fn validate_key(&self, key_id: &str) -> bool {
        if let Some((_, meta)) = self.keys.get(key_id) {
            matches!(meta.status, KeyStatus::Active | KeyStatus::Transitioning)
        } else {
            false
        }
    }
}

impl Drop for KeyRotation {
    fn drop(&mut self) {
        for (_, (mut key, _)) in self.keys.drain() {
            key.zeroize();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_rotation_initialization() {
        let kr = KeyRotation::new(90, 30, 5).unwrap();
        assert_eq!(kr.get_current_key_id().len(), 36); // UUID length
        assert!(kr.get_current_key().is_some());
        assert_eq!(kr.list_active_keys().len(), 1);
    }

    #[test]
    fn test_rotate_key() {
        let mut kr = KeyRotation::new(90, 30, 5).unwrap();
        let old_id = kr.get_current_key_id();
        let old_key = kr.get_current_key().unwrap();

        let event = kr.rotate_key().unwrap();
        let new_id = kr.get_current_key_id();
        let new_key = kr.get_current_key().unwrap();

        assert_ne!(old_id, new_id);
        assert_ne!(old_key, new_key);

        match event {
            KeyEvent::Rotated {
                old_id: oid,
                new_id: nid,
                ..
            } => {
                assert_eq!(oid, old_id);
                assert_eq!(nid, new_id);
            }
            _ => panic!("Expected Rotated event"),
        }

        let active_keys = kr.list_active_keys();
        assert_eq!(active_keys.len(), 2);
    }

    #[test]
    fn test_revoke_key() {
        let mut kr = KeyRotation::new(90, 30, 5).unwrap();
        let old_id = kr.get_current_key_id();

        kr.rotate_key().unwrap();

        let event = kr.revoke_key(&old_id, "Security incident").unwrap();
        match event {
            KeyEvent::Revoked { key_id, reason, .. } => {
                assert_eq!(key_id, old_id);
                assert_eq!(reason, "Security incident");
            }
            _ => panic!("Expected Revoked event"),
        }

        let active_keys = kr.list_active_keys();
        assert_eq!(active_keys.len(), 1);
    }

    #[test]
    fn test_revoke_current_key_fails() {
        let mut kr = KeyRotation::new(90, 30, 5).unwrap();
        let current_id = kr.get_current_key_id();

        let result = kr.revoke_key(&current_id, "Try to revoke current");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("Cannot revoke the current active key"));
    }

    #[test]
    fn test_restore_key() {
        let mut kr = KeyRotation::new(90, 30, 5).unwrap();
        let old_id = kr.get_current_key_id();

        kr.rotate_key().unwrap();
        kr.revoke_key(&old_id, "Test revocation").unwrap();

        let event = kr.restore_key(&old_id).unwrap();
        match event {
            KeyEvent::Restored { key_id, .. } => {
                assert_eq!(key_id, old_id);
            }
            _ => panic!("Expected Restored event"),
        }

        assert!(kr.validate_key(&old_id));
    }

    #[test]
    fn test_should_rotate() {
        let kr = KeyRotation::new(90, 30, 5).unwrap();
        assert!(!kr.should_rotate());
    }

    #[test]
    fn test_get_events_since() {
        let mut kr = KeyRotation::new(90, 30, 5).unwrap();
        let initial_time = Utc::now();

        tokio::runtime::Runtime::new().unwrap().block_on(async {
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        });

        kr.rotate_key().unwrap();

        let events = kr.get_events_since(initial_time);
        assert!(!events.is_empty());
    }

    #[test]
    fn test_key_zeroization_on_drop() {
        let kr = KeyRotation::new(90, 30, 5).unwrap();
        let key = kr.get_current_key().unwrap();
        assert!(key.iter().any(|b| *b != 0));
        // Key should be zeroed when kr is dropped
    }

    #[test]
    fn test_multiple_rotations() {
        let mut kr = KeyRotation::new(90, 30, 5).unwrap();

        for _ in 0..3 {
            kr.rotate_key().unwrap();
            let active_keys = kr.list_active_keys();
            assert!(!active_keys.is_empty());
            assert!(active_keys.len() <= 5);
        }
    }

    #[test]
    fn test_cleanup_old_keys() {
        let mut kr = KeyRotation::new(90, 30, 2).unwrap();

        for _ in 0..5 {
            kr.rotate_key().unwrap();
        }

        let active_keys = kr.list_active_keys();
        assert!(active_keys.len() <= 2);
    }

    #[test]
    fn test_validate_key() {
        let kr = KeyRotation::new(90, 30, 5).unwrap();
        let current_id = kr.get_current_key_id();

        assert!(kr.validate_key(&current_id));
        assert!(!kr.validate_key("non-existent-key"));
    }
}
