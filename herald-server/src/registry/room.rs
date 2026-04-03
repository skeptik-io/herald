use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};

use dashmap::DashMap;
use herald_core::room::EncryptionMode;

use crate::registry::connection::ConnId;

pub struct RoomState {
    pub members: HashSet<String>,
    pub subscribers: HashMap<String, HashSet<ConnId>>,
    pub sequence: AtomicU64,
    pub encryption_mode: EncryptionMode,
}

/// Composite key for tenant-scoped rooms.
type RoomKey = (String, String); // (tenant_id, room_id)

#[derive(Default)]
pub struct RoomRegistry {
    rooms: DashMap<RoomKey, RoomState>,
}

impl RoomRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create_room(
        &self,
        tenant_id: &str,
        room_id: &str,
        initial_seq: u64,
        encryption_mode: EncryptionMode,
    ) {
        let key = (tenant_id.to_string(), room_id.to_string());
        self.rooms.entry(key).or_insert_with(|| RoomState {
            members: HashSet::new(),
            subscribers: HashMap::new(),
            sequence: AtomicU64::new(initial_seq),
            encryption_mode,
        });
    }

    pub fn remove_room(&self, tenant_id: &str, room_id: &str) {
        self.rooms
            .remove(&(tenant_id.to_string(), room_id.to_string()));
    }

    pub fn add_member(&self, tenant_id: &str, room_id: &str, user_id: &str) {
        let key = (tenant_id.to_string(), room_id.to_string());
        if let Some(mut state) = self.rooms.get_mut(&key) {
            state.members.insert(user_id.to_string());
        }
    }

    pub fn remove_member(&self, tenant_id: &str, room_id: &str, user_id: &str) {
        let key = (tenant_id.to_string(), room_id.to_string());
        if let Some(mut state) = self.rooms.get_mut(&key) {
            state.members.remove(user_id);
            state.subscribers.remove(user_id);
        }
    }

    pub fn is_member(&self, tenant_id: &str, room_id: &str, user_id: &str) -> bool {
        let key = (tenant_id.to_string(), room_id.to_string());
        self.rooms
            .get(&key)
            .map(|s| s.members.contains(user_id))
            .unwrap_or(false)
    }

    pub fn encryption_mode(&self, tenant_id: &str, room_id: &str) -> EncryptionMode {
        let key = (tenant_id.to_string(), room_id.to_string());
        self.rooms
            .get(&key)
            .map(|s| s.encryption_mode)
            .unwrap_or_default()
    }

    pub fn subscribe(&self, tenant_id: &str, room_id: &str, user_id: &str, conn_id: ConnId) {
        let key = (tenant_id.to_string(), room_id.to_string());
        if let Some(mut state) = self.rooms.get_mut(&key) {
            state
                .subscribers
                .entry(user_id.to_string())
                .or_default()
                .insert(conn_id);
        }
    }

    pub fn unsubscribe(&self, tenant_id: &str, room_id: &str, user_id: &str, conn_id: ConnId) {
        let key = (tenant_id.to_string(), room_id.to_string());
        if let Some(mut state) = self.rooms.get_mut(&key) {
            if let Some(conns) = state.subscribers.get_mut(user_id) {
                conns.remove(&conn_id);
                if conns.is_empty() {
                    state.subscribers.remove(user_id);
                }
            }
        }
    }

    pub fn unsubscribe_conn_from_all(&self, conn_id: ConnId, tenant_id: &str, user_id: &str) {
        for mut entry in self.rooms.iter_mut() {
            let (tid, _) = entry.key();
            if tid != tenant_id {
                continue;
            }
            if let Some(conns) = entry.subscribers.get_mut(user_id) {
                conns.remove(&conn_id);
                if conns.is_empty() {
                    entry.subscribers.remove(user_id);
                }
            }
        }
    }

    pub fn next_seq(&self, tenant_id: &str, room_id: &str) -> u64 {
        let key = (tenant_id.to_string(), room_id.to_string());
        self.rooms
            .get(&key)
            .map(|s| s.sequence.fetch_add(1, Ordering::SeqCst) + 1)
            .unwrap_or(1)
    }

    pub fn current_seq(&self, tenant_id: &str, room_id: &str) -> u64 {
        let key = (tenant_id.to_string(), room_id.to_string());
        self.rooms
            .get(&key)
            .map(|s| s.sequence.load(Ordering::SeqCst))
            .unwrap_or(0)
    }

    pub fn get_members(&self, tenant_id: &str, room_id: &str) -> Vec<String> {
        let key = (tenant_id.to_string(), room_id.to_string());
        self.rooms
            .get(&key)
            .map(|s| s.members.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn get_subscriber_conns(
        &self,
        tenant_id: &str,
        room_id: &str,
    ) -> Vec<(String, Vec<ConnId>)> {
        let key = (tenant_id.to_string(), room_id.to_string());
        self.rooms
            .get(&key)
            .map(|s| {
                s.subscribers
                    .iter()
                    .map(|(uid, conns)| (uid.clone(), conns.iter().copied().collect()))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn room_exists(&self, tenant_id: &str, room_id: &str) -> bool {
        let key = (tenant_id.to_string(), room_id.to_string());
        self.rooms.contains_key(&key)
    }

    pub fn room_count(&self) -> usize {
        self.rooms.len()
    }

    pub fn tenant_room_count(&self, tenant_id: &str) -> usize {
        self.rooms
            .iter()
            .filter(|entry| entry.key().0 == tenant_id)
            .count()
    }

    pub fn get_member_rooms(&self, tenant_id: &str, user_id: &str) -> Vec<String> {
        self.rooms
            .iter()
            .filter(|entry| {
                let (tid, _) = entry.key();
                tid == tenant_id && entry.members.contains(user_id)
            })
            .map(|entry| entry.key().1.clone())
            .collect()
    }
}
