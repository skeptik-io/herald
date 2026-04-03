use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;
use serde::Serialize;
use tokio::sync::broadcast;

/// Maximum events kept in the ring buffer.
const MAX_EVENTS: usize = 1000;

/// Maximum error entries kept per category.
const MAX_ERRORS: usize = 500;

#[derive(Debug, Clone, Serialize)]
pub struct AdminEvent {
    pub id: u64,
    pub timestamp: i64,
    pub kind: EventKind,
    pub tenant_id: Option<String>,
    pub details: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Connection,
    Disconnection,
    Message,
    Subscribe,
    Unsubscribe,
    AuthFailure,
    ClientError,
    WebhookError,
    HttpError,
}

impl std::fmt::Display for EventKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventKind::Connection => write!(f, "connection"),
            EventKind::Disconnection => write!(f, "disconnection"),
            EventKind::Message => write!(f, "message"),
            EventKind::Subscribe => write!(f, "subscribe"),
            EventKind::Unsubscribe => write!(f, "unsubscribe"),
            EventKind::AuthFailure => write!(f, "auth_failure"),
            EventKind::ClientError => write!(f, "client_error"),
            EventKind::WebhookError => write!(f, "webhook_error"),
            EventKind::HttpError => write!(f, "http_error"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorEntry {
    pub timestamp: i64,
    pub category: ErrorCategory,
    pub message: String,
    pub details: serde_json::Value,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    Client,
    Webhook,
    Http,
}

/// Snapshot of stats at a point in time, recorded periodically.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct StatsSnapshot {
    pub timestamp: i64,
    pub connections: u64,
    pub peak_connections: u64,
    pub messages_sent: u64,
    pub messages_delta: u64,
    pub webhooks_sent: u64,
    pub webhooks_delta: u64,
    pub rooms: u64,
    pub auth_failures: u64,
}

pub struct EventBus {
    events: RwLock<VecDeque<AdminEvent>>,
    errors: RwLock<VecDeque<ErrorEntry>>,
    next_id: AtomicU64,
    broadcast_tx: broadcast::Sender<AdminEvent>,

    // Stats tracking
    stats_snapshots: RwLock<VecDeque<StatsSnapshot>>,
    peak_connections_today: AtomicU64,
    webhooks_sent: AtomicU64,
    last_messages_total: AtomicU64,
    last_webhooks_total: AtomicU64,
    day_start_messages: AtomicU64,
    day_start_webhooks: AtomicU64,
    current_day: RwLock<u32>,
}

impl EventBus {
    pub fn new() -> Arc<Self> {
        let (broadcast_tx, _) = broadcast::channel(256);
        Arc::new(Self {
            events: RwLock::new(VecDeque::with_capacity(MAX_EVENTS)),
            errors: RwLock::new(VecDeque::with_capacity(MAX_ERRORS)),
            next_id: AtomicU64::new(1),
            broadcast_tx,
            stats_snapshots: RwLock::new(VecDeque::with_capacity(8640)), // ~30 days at 5min intervals
            peak_connections_today: AtomicU64::new(0),
            webhooks_sent: AtomicU64::new(0),
            last_messages_total: AtomicU64::new(0),
            last_webhooks_total: AtomicU64::new(0),
            day_start_messages: AtomicU64::new(0),
            day_start_webhooks: AtomicU64::new(0),
            current_day: RwLock::new(0),
        })
    }

    pub fn push_event(
        &self,
        kind: EventKind,
        tenant_id: Option<String>,
        details: serde_json::Value,
    ) {
        let event = AdminEvent {
            id: self.next_id.fetch_add(1, Ordering::Relaxed),
            timestamp: now_millis(),
            kind,
            tenant_id,
            details,
        };

        {
            let mut events = self.events.write();
            if events.len() >= MAX_EVENTS {
                events.pop_front();
            }
            events.push_back(event.clone());
        }

        let _ = self.broadcast_tx.send(event);
    }

    pub fn push_error(&self, category: ErrorCategory, message: String, details: serde_json::Value) {
        let entry = ErrorEntry {
            timestamp: now_millis(),
            category,
            message,
            details,
        };
        let mut errors = self.errors.write();
        if errors.len() >= MAX_ERRORS {
            errors.pop_front();
        }
        errors.push_back(entry);
    }

    pub fn recent_events(&self, limit: usize, after_id: Option<u64>) -> Vec<AdminEvent> {
        let events = self.events.read();
        let filtered: Vec<&AdminEvent> = if let Some(after) = after_id {
            events.iter().filter(|e| e.id > after).collect()
        } else {
            events.iter().collect()
        };
        filtered.into_iter().rev().take(limit).cloned().collect()
    }

    pub fn recent_errors(&self, category: Option<ErrorCategory>, limit: usize) -> Vec<ErrorEntry> {
        let errors = self.errors.read();
        let iter = errors.iter();
        let filtered: Vec<&ErrorEntry> = if let Some(cat) = category {
            iter.filter(|e| e.category == cat).collect()
        } else {
            iter.collect()
        };
        filtered.into_iter().rev().take(limit).cloned().collect()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AdminEvent> {
        self.broadcast_tx.subscribe()
    }

    pub fn increment_webhooks(&self) {
        self.webhooks_sent.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a stats snapshot. Called periodically (every 5 minutes).
    pub fn record_snapshot(
        &self,
        connections: u64,
        messages_total: u64,
        rooms: u64,
        auth_failures: u64,
    ) {
        let now = now_millis();
        let today = (now / 86_400_000) as u32;

        // Reset daily peaks on day change
        {
            let mut current_day = self.current_day.write();
            if *current_day != today {
                *current_day = today;
                self.peak_connections_today.store(0, Ordering::Relaxed);
                self.day_start_messages
                    .store(messages_total, Ordering::Relaxed);
                self.day_start_webhooks.store(
                    self.webhooks_sent.load(Ordering::Relaxed),
                    Ordering::Relaxed,
                );
            }
        }

        // Update peak
        self.peak_connections_today
            .fetch_max(connections, Ordering::Relaxed);

        let prev_messages = self
            .last_messages_total
            .swap(messages_total, Ordering::Relaxed);
        let webhooks_total = self.webhooks_sent.load(Ordering::Relaxed);
        let prev_webhooks = self
            .last_webhooks_total
            .swap(webhooks_total, Ordering::Relaxed);

        let snapshot = StatsSnapshot {
            timestamp: now,
            connections,
            peak_connections: self.peak_connections_today.load(Ordering::Relaxed),
            messages_sent: messages_total,
            messages_delta: messages_total.saturating_sub(prev_messages),
            webhooks_sent: webhooks_total,
            webhooks_delta: webhooks_total.saturating_sub(prev_webhooks),
            rooms,
            auth_failures,
        };

        let mut snapshots = self.stats_snapshots.write();
        if snapshots.len() >= 8640 {
            snapshots.pop_front();
        }
        snapshots.push_back(snapshot);
    }

    /// Get stats snapshots within a time range.
    pub fn get_snapshots(&self, from: i64, to: i64) -> Vec<StatsSnapshot> {
        let snapshots = self.stats_snapshots.read();
        snapshots
            .iter()
            .filter(|s| s.timestamp >= from && s.timestamp <= to)
            .cloned()
            .collect()
    }

    /// Get today's summary stats.
    pub fn today_summary(&self, current_messages: u64) -> TodaySummary {
        let day_start_messages = self.day_start_messages.load(Ordering::Relaxed);
        let day_start_webhooks = self.day_start_webhooks.load(Ordering::Relaxed);
        let webhooks_total = self.webhooks_sent.load(Ordering::Relaxed);
        TodaySummary {
            peak_connections: self.peak_connections_today.load(Ordering::Relaxed),
            messages_today: current_messages.saturating_sub(day_start_messages),
            webhooks_today: webhooks_total.saturating_sub(day_start_webhooks),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TodaySummary {
    pub peak_connections: u64,
    pub messages_today: u64,
    pub webhooks_today: u64,
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
