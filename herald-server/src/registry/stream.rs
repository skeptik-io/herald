use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use dashmap::DashMap;
use parking_lot::Mutex;

use crate::registry::connection::ConnId;

pub struct StreamState {
    pub members: HashSet<String>,
    pub subscribers: HashMap<String, HashSet<ConnId>>,
    pub sequence: AtomicU64,
    pub archived: AtomicBool,
    pub public: AtomicBool,
    /// Cache channel: last `ServerMessage::EventNew` for this stream.
    pub last_event: Mutex<Option<herald_core::protocol::ServerMessage>>,
}

/// Composite key for tenant-scoped streams.
type StreamKey = (String, String); // (tenant_id, stream_id)

#[derive(Default)]
pub struct StreamRegistry {
    streams: DashMap<StreamKey, StreamState>,
}

impl StreamRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create_stream(&self, tenant_id: &str, stream_id: &str, initial_seq: u64, public: bool) {
        let key = (tenant_id.to_string(), stream_id.to_string());
        self.streams.entry(key).or_insert_with(|| StreamState {
            members: HashSet::new(),
            subscribers: HashMap::new(),
            sequence: AtomicU64::new(initial_seq),
            archived: AtomicBool::new(false),
            public: AtomicBool::new(public),
            last_event: Mutex::new(None),
        });
    }

    pub fn remove_stream(&self, tenant_id: &str, stream_id: &str) {
        self.streams
            .remove(&(tenant_id.to_string(), stream_id.to_string()));
    }

    pub fn add_member(&self, tenant_id: &str, stream_id: &str, user_id: &str) {
        let key = (tenant_id.to_string(), stream_id.to_string());
        if let Some(mut state) = self.streams.get_mut(&key) {
            state.members.insert(user_id.to_string());
        }
    }

    pub fn remove_member(&self, tenant_id: &str, stream_id: &str, user_id: &str) {
        let key = (tenant_id.to_string(), stream_id.to_string());
        if let Some(mut state) = self.streams.get_mut(&key) {
            state.members.remove(user_id);
            state.subscribers.remove(user_id);
        }
    }

    pub fn is_member(&self, tenant_id: &str, stream_id: &str, user_id: &str) -> bool {
        let key = (tenant_id.to_string(), stream_id.to_string());
        self.streams
            .get(&key)
            .map(|s| s.members.contains(user_id))
            .unwrap_or(false)
    }

    pub fn subscribe(&self, tenant_id: &str, stream_id: &str, user_id: &str, conn_id: ConnId) {
        let key = (tenant_id.to_string(), stream_id.to_string());
        if let Some(mut state) = self.streams.get_mut(&key) {
            state
                .subscribers
                .entry(user_id.to_string())
                .or_default()
                .insert(conn_id);
        }
    }

    pub fn unsubscribe(&self, tenant_id: &str, stream_id: &str, user_id: &str, conn_id: ConnId) {
        let key = (tenant_id.to_string(), stream_id.to_string());
        if let Some(mut state) = self.streams.get_mut(&key) {
            if let Some(conns) = state.subscribers.get_mut(user_id) {
                conns.remove(&conn_id);
                if conns.is_empty() {
                    state.subscribers.remove(user_id);
                }
            }
        }
    }

    /// Unsubscribe a connection from all streams. Returns the list of affected stream IDs.
    pub fn unsubscribe_conn_from_all(
        &self,
        conn_id: ConnId,
        tenant_id: &str,
        user_id: &str,
    ) -> Vec<String> {
        let mut affected = Vec::new();
        for mut entry in self.streams.iter_mut() {
            let (tid, sid) = entry.key();
            if tid != tenant_id {
                continue;
            }
            let sid = sid.clone();
            if let Some(conns) = entry.subscribers.get_mut(user_id) {
                if conns.remove(&conn_id) {
                    affected.push(sid);
                }
                if conns.is_empty() {
                    entry.subscribers.remove(user_id);
                }
            }
        }
        affected
    }

    /// Get the total number of subscriber connections for a stream.
    pub fn subscriber_count(&self, tenant_id: &str, stream_id: &str) -> usize {
        let key = (tenant_id.to_string(), stream_id.to_string());
        self.streams
            .get(&key)
            .map(|s| s.subscribers.values().map(|c| c.len()).sum())
            .unwrap_or(0)
    }

    pub fn next_seq(&self, tenant_id: &str, stream_id: &str) -> u64 {
        let key = (tenant_id.to_string(), stream_id.to_string());
        self.streams
            .get(&key)
            .map(|s| s.sequence.fetch_add(1, Ordering::SeqCst) + 1)
            .unwrap_or(1)
    }

    pub fn current_seq(&self, tenant_id: &str, stream_id: &str) -> u64 {
        let key = (tenant_id.to_string(), stream_id.to_string());
        self.streams
            .get(&key)
            .map(|s| s.sequence.load(Ordering::SeqCst))
            .unwrap_or(0)
    }

    pub fn get_members(&self, tenant_id: &str, stream_id: &str) -> Vec<String> {
        let key = (tenant_id.to_string(), stream_id.to_string());
        self.streams
            .get(&key)
            .map(|s| s.members.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn get_subscriber_conns(
        &self,
        tenant_id: &str,
        stream_id: &str,
    ) -> Vec<(String, Vec<ConnId>)> {
        let key = (tenant_id.to_string(), stream_id.to_string());
        self.streams
            .get(&key)
            .map(|s| {
                s.subscribers
                    .iter()
                    .map(|(uid, conns)| (uid.clone(), conns.iter().copied().collect()))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn stream_exists(&self, tenant_id: &str, stream_id: &str) -> bool {
        let key = (tenant_id.to_string(), stream_id.to_string());
        self.streams.contains_key(&key)
    }

    pub fn stream_count(&self) -> usize {
        self.streams.len()
    }

    pub fn tenant_stream_count(&self, tenant_id: &str) -> usize {
        self.streams
            .iter()
            .filter(|entry| entry.key().0 == tenant_id)
            .count()
    }

    pub fn set_archived(&self, tenant_id: &str, stream_id: &str, archived: bool) {
        let key = (tenant_id.to_string(), stream_id.to_string());
        if let Some(state) = self.streams.get(&key) {
            state.archived.store(archived, Ordering::SeqCst);
        }
    }

    pub fn is_archived(&self, tenant_id: &str, stream_id: &str) -> bool {
        let key = (tenant_id.to_string(), stream_id.to_string());
        self.streams
            .get(&key)
            .map(|s| s.archived.load(Ordering::SeqCst))
            .unwrap_or(false)
    }

    pub fn is_public(&self, tenant_id: &str, stream_id: &str) -> bool {
        let key = (tenant_id.to_string(), stream_id.to_string());
        self.streams
            .get(&key)
            .map(|s| s.public.load(Ordering::SeqCst))
            .unwrap_or(false)
    }

    pub fn get_member_streams(&self, tenant_id: &str, user_id: &str) -> Vec<String> {
        self.streams
            .iter()
            .filter(|entry| {
                let (tid, _) = entry.key();
                tid == tenant_id && entry.members.contains(user_id)
            })
            .map(|entry| entry.key().1.clone())
            .collect()
    }

    /// Cache the last event for a stream (cache channel behavior).
    pub fn set_last_event(
        &self,
        tenant_id: &str,
        stream_id: &str,
        msg: herald_core::protocol::ServerMessage,
    ) {
        let key = (tenant_id.to_string(), stream_id.to_string());
        if let Some(state) = self.streams.get(&key) {
            *state.last_event.lock() = Some(msg);
        }
    }

    /// Get the cached last event for a stream.
    pub fn get_last_event(
        &self,
        tenant_id: &str,
        stream_id: &str,
    ) -> Option<herald_core::protocol::ServerMessage> {
        let key = (tenant_id.to_string(), stream_id.to_string());
        self.streams
            .get(&key)
            .and_then(|s| s.last_event.lock().clone())
    }
}
