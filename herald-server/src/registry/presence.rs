use std::collections::HashMap;
use std::sync::Mutex;

use herald_core::presence::PresenceStatus;

use crate::registry::connection::ConnectionRegistry;

struct ManualOverride {
    status: PresenceStatus,
    set_at: std::time::Instant,
}

/// Key: (tenant_id, user_id)
type PresenceKey = (String, String);

#[derive(Default)]
pub struct PresenceTracker {
    overrides: Mutex<HashMap<PresenceKey, ManualOverride>>,
}

impl PresenceTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_manual(&self, tenant_id: &str, user_id: &str, status: PresenceStatus) {
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
                    set_at: std::time::Instant::now(),
                },
            );
        }
    }

    pub fn resolve(
        &self,
        tenant_id: &str,
        user_id: &str,
        connections: &ConnectionRegistry,
        ttl_secs: u64,
    ) -> PresenceStatus {
        let key = (tenant_id.to_string(), user_id.to_string());
        if let Ok(overrides) = self.overrides.lock() {
            if let Some(ov) = overrides.get(&key) {
                if ov.set_at.elapsed().as_secs() < ttl_secs {
                    return ov.status;
                }
            }
        }

        if connections.user_connection_count(tenant_id, user_id) > 0 {
            PresenceStatus::Online
        } else {
            PresenceStatus::Offline
        }
    }
}
