use std::collections::HashMap;
use std::sync::Mutex;

use herald_core::presence::PresenceStatus;

use crate::registry::connection::ConnectionRegistry;

/// A manual presence override with optional wall-clock expiry.
struct ManualOverride {
    status: PresenceStatus,
    /// Unix timestamp (milliseconds) when this override expires.
    /// `None` means indefinite.
    expires_at_ms: Option<i64>,
    /// Original ISO 8601 string for the expiry (passed through to clients).
    until_iso: Option<String>,
}

/// Key: (tenant_id, user_id)
type PresenceKey = (String, String);

#[derive(Default)]
pub struct PresenceTracker {
    overrides: Mutex<HashMap<PresenceKey, ManualOverride>>,
    /// Unix millis of each user's last disconnect (after linger).
    last_seen: Mutex<HashMap<PresenceKey, i64>>,
}

impl PresenceTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a manual presence override for a user.
    ///
    /// - `Online` clears any existing override (reverts to connection-derived).
    /// - `Away`, `Dnd`, `Offline` set the override with an optional expiry.
    pub fn set_manual(
        &self,
        tenant_id: &str,
        user_id: &str,
        status: PresenceStatus,
        expires_at_ms: Option<i64>,
        until_iso: Option<String>,
    ) {
        let Ok(mut overrides) = self.overrides.lock() else {
            return;
        };
        let key = (tenant_id.to_string(), user_id.to_string());
        if status == PresenceStatus::Online {
            overrides.remove(&key);
        } else {
            overrides.insert(
                key,
                ManualOverride {
                    status,
                    expires_at_ms,
                    until_iso,
                },
            );
        }
    }

    /// Resolve effective presence for a user.
    ///
    /// Priority: unexpired manual override > connection-derived.
    pub fn resolve(
        &self,
        tenant_id: &str,
        user_id: &str,
        connections: &ConnectionRegistry,
        _max_ttl_secs: u64,
    ) -> ResolvedPresence {
        let key = (tenant_id.to_string(), user_id.to_string());
        let now_ms = crate::ws::connection::now_millis();

        if let Ok(overrides) = self.overrides.lock() {
            if let Some(ov) = overrides.get(&key) {
                let expired = match ov.expires_at_ms {
                    Some(exp) => now_ms >= exp,
                    None => false,
                };

                if !expired {
                    return ResolvedPresence {
                        status: ov.status,
                        until: ov.until_iso.clone(),
                        last_seen_at: None,
                    };
                }
            }
        }

        // No active override — derive from connection state
        if connections.user_connection_count(tenant_id, user_id) > 0 {
            ResolvedPresence {
                status: PresenceStatus::Online,
                until: None,
                last_seen_at: None,
            }
        } else {
            let last_seen_at = self
                .last_seen
                .lock()
                .ok()
                .and_then(|map| map.get(&key).copied());
            ResolvedPresence {
                status: PresenceStatus::Offline,
                until: None,
                last_seen_at,
            }
        }
    }

    /// Convenience: resolve and return just the status.
    pub fn resolve_status(
        &self,
        tenant_id: &str,
        user_id: &str,
        connections: &ConnectionRegistry,
        max_ttl_secs: u64,
    ) -> PresenceStatus {
        self.resolve(tenant_id, user_id, connections, max_ttl_secs)
            .status
    }

    /// Sweep expired overrides. Returns the `(tenant_id, user_id)` pairs whose
    /// overrides were removed (their effective presence may have changed).
    pub fn sweep_expired(&self) -> Vec<(String, String)> {
        let now_ms = crate::ws::connection::now_millis();
        let Ok(mut overrides) = self.overrides.lock() else {
            return vec![];
        };
        let mut changed = Vec::new();
        overrides.retain(|key, ov| {
            if let Some(exp) = ov.expires_at_ms {
                if now_ms >= exp {
                    changed.push(key.clone());
                    return false;
                }
            }
            true
        });
        changed
    }

    /// Load overrides from persisted state (called on startup).
    pub fn load_overrides(&self, overrides_list: Vec<PersistedOverride>) {
        let now_ms = crate::ws::connection::now_millis();
        let Ok(mut map) = self.overrides.lock() else {
            return;
        };
        for po in overrides_list {
            // Skip already-expired overrides
            if let Some(exp) = po.expires_at_ms {
                if now_ms >= exp {
                    continue;
                }
            }
            let key = (po.tenant_id, po.user_id);
            map.insert(
                key,
                ManualOverride {
                    status: po.status,
                    expires_at_ms: po.expires_at_ms,
                    until_iso: po.until_iso,
                },
            );
        }
    }

    /// Dump current overrides for persistence.
    pub fn dump_overrides(&self) -> Vec<PersistedOverride> {
        let Ok(overrides) = self.overrides.lock() else {
            return vec![];
        };
        overrides
            .iter()
            .map(|((tenant_id, user_id), ov)| PersistedOverride {
                tenant_id: tenant_id.clone(),
                user_id: user_id.clone(),
                status: ov.status,
                expires_at_ms: ov.expires_at_ms,
                until_iso: ov.until_iso.clone(),
            })
            .collect()
    }

    // -----------------------------------------------------------------------
    // last_seen tracking
    // -----------------------------------------------------------------------

    /// Record the timestamp of a user's last disconnect.
    pub fn stamp_last_seen(&self, tenant_id: &str, user_id: &str, ts_ms: i64) {
        if let Ok(mut map) = self.last_seen.lock() {
            map.insert((tenant_id.to_string(), user_id.to_string()), ts_ms);
        }
    }

    /// Clear the in-memory last-seen entry (called on reconnect).
    pub fn clear_last_seen(&self, tenant_id: &str, user_id: &str) {
        if let Ok(mut map) = self.last_seen.lock() {
            map.remove(&(tenant_id.to_string(), user_id.to_string()));
        }
    }

    /// Get the last-seen timestamp for a user, if any.
    pub fn get_last_seen(&self, tenant_id: &str, user_id: &str) -> Option<i64> {
        self.last_seen.lock().ok().and_then(|map| {
            map.get(&(tenant_id.to_string(), user_id.to_string()))
                .copied()
        })
    }

    /// Bulk-load last-seen entries from WAL on startup.
    pub fn load_last_seen(&self, entries: Vec<PersistedLastSeen>) {
        if let Ok(mut map) = self.last_seen.lock() {
            for entry in entries {
                map.insert((entry.tenant_id, entry.user_id), entry.last_seen_at_ms);
            }
        }
    }

    /// Dump last-seen entries for diagnostics.
    pub fn dump_last_seen(&self) -> Vec<PersistedLastSeen> {
        let Ok(map) = self.last_seen.lock() else {
            return vec![];
        };
        map.iter()
            .map(|((tenant_id, user_id), &ts)| PersistedLastSeen {
                tenant_id: tenant_id.clone(),
                user_id: user_id.clone(),
                last_seen_at_ms: ts,
            })
            .collect()
    }
}

/// Resolved presence with optional expiry info for the client.
pub struct ResolvedPresence {
    pub status: PresenceStatus,
    /// ISO 8601 string of when the override expires, if applicable.
    pub until: Option<String>,
    /// Unix millis of the user's last disconnect. Present only when offline.
    pub last_seen_at: Option<i64>,
}

/// Serializable override for WAL persistence.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PersistedOverride {
    pub tenant_id: String,
    pub user_id: String,
    pub status: PresenceStatus,
    pub expires_at_ms: Option<i64>,
    pub until_iso: Option<String>,
}

/// Serializable last-seen entry for WAL persistence.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PersistedLastSeen {
    pub tenant_id: String,
    pub user_id: String,
    pub last_seen_at_ms: i64,
}
