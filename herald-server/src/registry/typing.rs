use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

const TYPING_TTL_SECS: u64 = 10;

/// Key: (tenant_id, stream_id)
type StreamKey = (String, String);

struct TypingEntry {
    started_at: Instant,
}

#[derive(Default)]
pub struct TypingTracker {
    /// Maps (tenant, stream) -> { user_id -> entry }
    inner: Mutex<HashMap<StreamKey, HashMap<String, TypingEntry>>>,
}

impl TypingTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_typing(&self, tenant_id: &str, stream_id: &str, user_id: &str, active: bool) {
        let Ok(mut map) = self.inner.lock() else {
            return;
        };
        let key = (tenant_id.to_string(), stream_id.to_string());
        if active {
            map.entry(key).or_default().insert(
                user_id.to_string(),
                TypingEntry {
                    started_at: Instant::now(),
                },
            );
        } else if let Some(stream) = map.get_mut(&key) {
            stream.remove(user_id);
            if stream.is_empty() {
                map.remove(&key);
            }
        }
    }

    /// Remove all typing state for a user across all streams.
    /// Returns list of stream_ids where the user was typing.
    pub fn remove_user(&self, tenant_id: &str, user_id: &str) -> Vec<String> {
        let Ok(mut map) = self.inner.lock() else {
            return vec![];
        };
        let mut streams = Vec::new();
        let tenant = tenant_id.to_string();
        let mut empty_keys = Vec::new();
        for (key, users) in map.iter_mut() {
            if key.0 == tenant {
                if users.remove(user_id).is_some() {
                    streams.push(key.1.clone());
                }
                if users.is_empty() {
                    empty_keys.push(key.clone());
                }
            }
        }
        for key in empty_keys {
            map.remove(&key);
        }
        streams
    }

    /// Remove expired typing entries. Returns list of (tenant_id, stream_id, user_id) that expired.
    pub fn expire(&self) -> Vec<(String, String, String)> {
        let Ok(mut map) = self.inner.lock() else {
            return vec![];
        };
        let mut expired = Vec::new();
        let mut empty_keys = Vec::new();
        for (key, users) in map.iter_mut() {
            let mut expired_users = Vec::new();
            for (user_id, entry) in users.iter() {
                if entry.started_at.elapsed().as_secs() >= TYPING_TTL_SECS {
                    expired_users.push(user_id.clone());
                }
            }
            for user_id in expired_users {
                users.remove(&user_id);
                expired.push((key.0.clone(), key.1.clone(), user_id));
            }
            if users.is_empty() {
                empty_keys.push(key.clone());
            }
        }
        for key in empty_keys {
            map.remove(&key);
        }
        expired
    }
}
