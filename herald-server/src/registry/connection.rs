use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

use dashmap::DashMap;
use herald_core::protocol::ServerMessage;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnId(pub u64);

impl std::fmt::Display for ConnId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "conn_{}", self.0)
    }
}

static NEXT_CONN_ID: AtomicU64 = AtomicU64::new(1);

impl ConnId {
    pub fn next() -> Self {
        Self(NEXT_CONN_ID.fetch_add(1, Ordering::Relaxed))
    }
}

pub struct ConnectionHandle {
    #[allow(dead_code)]
    pub conn_id: ConnId,
    pub tenant_id: String,
    pub user_id: String,
    pub tx: mpsc::Sender<ServerMessage>,
    pub rooms: HashSet<String>,
}

/// Key for per-tenant user tracking: (tenant_id, user_id)
type UserKey = (String, String);

#[derive(Default)]
pub struct ConnectionRegistry {
    by_conn: DashMap<ConnId, ConnectionHandle>,
    by_user: DashMap<UserKey, HashSet<ConnId>>,
    /// Generation counter per user — incremented on every connect.
    /// Used by presence linger to detect reconnects during the linger window.
    by_generation: DashMap<UserKey, u64>,
}

impl ConnectionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &self,
        conn_id: ConnId,
        tenant_id: String,
        user_id: String,
        tx: mpsc::Sender<ServerMessage>,
    ) {
        self.by_conn.insert(
            conn_id,
            ConnectionHandle {
                conn_id,
                tenant_id: tenant_id.clone(),
                user_id: user_id.clone(),
                tx,
                rooms: HashSet::new(),
            },
        );
        self.by_user
            .entry((tenant_id, user_id))
            .or_default()
            .insert(conn_id);
    }

    pub fn unregister(&self, conn_id: ConnId) -> Option<(String, String)> {
        if let Some((_, handle)) = self.by_conn.remove(&conn_id) {
            let tenant_id = handle.tenant_id.clone();
            let user_id = handle.user_id.clone();
            let key = (tenant_id.clone(), user_id.clone());
            if let Some(mut conns) = self.by_user.get_mut(&key) {
                conns.remove(&conn_id);
                if conns.is_empty() {
                    drop(conns);
                    self.by_user.remove(&key);
                }
            }
            Some((tenant_id, user_id))
        } else {
            None
        }
    }

    pub fn add_room_subscription(&self, conn_id: ConnId, room_id: &str) {
        if let Some(mut handle) = self.by_conn.get_mut(&conn_id) {
            handle.rooms.insert(room_id.to_string());
        }
    }

    pub fn remove_room_subscription(&self, conn_id: ConnId, room_id: &str) {
        if let Some(mut handle) = self.by_conn.get_mut(&conn_id) {
            handle.rooms.remove(room_id);
        }
    }

    /// Returns true if sent, false if dropped (channel full or conn not found).
    pub fn send_to_conn(&self, conn_id: ConnId, msg: &ServerMessage) -> bool {
        if let Some(handle) = self.by_conn.get(&conn_id) {
            handle.tx.try_send(msg.clone()).is_ok()
        } else {
            false
        }
    }

    pub fn user_connection_count(&self, tenant_id: &str, user_id: &str) -> usize {
        let key = (tenant_id.to_string(), user_id.to_string());
        self.by_user.get(&key).map(|c| c.len()).unwrap_or(0)
    }

    pub fn tenant_connection_count(&self, tenant_id: &str) -> usize {
        self.by_user
            .iter()
            .filter(|entry| entry.key().0 == tenant_id)
            .map(|entry| entry.value().len())
            .sum()
    }

    pub fn total_connections(&self) -> usize {
        self.by_conn.len()
    }

    /// Increment and return the generation counter for a user.
    pub fn increment_generation(&self, tenant_id: &str, user_id: &str) -> u64 {
        let key = (tenant_id.to_string(), user_id.to_string());
        let mut entry = self.by_generation.entry(key).or_insert(0);
        *entry += 1;
        *entry
    }

    /// Send a message to all connected clients (used for shutdown notification).
    pub fn broadcast_all(&self, msg: &ServerMessage) {
        for entry in self.by_conn.iter() {
            let _ = entry.value().tx.try_send(msg.clone());
        }
    }

    /// Get the current generation for a user.
    pub fn current_generation(&self, tenant_id: &str, user_id: &str) -> u64 {
        let key = (tenant_id.to_string(), user_id.to_string());
        self.by_generation.get(&key).map(|v| *v).unwrap_or(0)
    }
}
