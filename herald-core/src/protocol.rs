use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::ErrorCode;
use crate::event::Sequence;
use crate::member::Role;
use crate::presence::PresenceStatus;

// ---------------------------------------------------------------------------
// Raw frame envelope (JSON wire format)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawFrame {
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub ref_: Option<String>,
    #[serde(default)]
    pub payload: Value,
}

// ---------------------------------------------------------------------------
// Client → Server
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum ClientMessage {
    Auth {
        ref_: Option<String>,
        token: String,
        last_seen_at: Option<i64>,
    },
    AuthRefresh {
        ref_: Option<String>,
        token: String,
    },
    Subscribe {
        ref_: Option<String>,
        streams: Vec<String>,
    },
    Unsubscribe {
        ref_: Option<String>,
        streams: Vec<String>,
    },
    EventPublish {
        ref_: Option<String>,
        stream: String,
        body: String,
        meta: Option<Value>,
        parent_id: Option<String>,
    },
    EventEdit {
        ref_: Option<String>,
        stream: String,
        id: String,
        body: String,
    },
    CursorUpdate {
        stream: String,
        seq: Sequence,
    },
    PresenceSet {
        ref_: Option<String>,
        status: PresenceStatus,
    },
    TypingStart {
        stream: String,
    },
    TypingStop {
        stream: String,
    },
    EventsFetch {
        ref_: Option<String>,
        stream: String,
        before: Option<Sequence>,
        after: Option<Sequence>,
        limit: Option<u32>,
    },
    EventDelete {
        ref_: Option<String>,
        stream: String,
        id: String,
    },
    Ping {
        ref_: Option<String>,
    },
    EventTrigger {
        ref_: Option<String>,
        stream: String,
        event: String,
        data: Option<Value>,
    },
    ReactionAdd {
        ref_: Option<String>,
        stream: String,
        event_id: String,
        emoji: String,
    },
    ReactionRemove {
        ref_: Option<String>,
        stream: String,
        event_id: String,
        emoji: String,
    },
}

impl ClientMessage {
    /// Returns the protocol type name for this message (e.g. "subscribe", "event.publish").
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Auth { .. } => "auth",
            Self::AuthRefresh { .. } => "auth.refresh",
            Self::Subscribe { .. } => "subscribe",
            Self::Unsubscribe { .. } => "unsubscribe",
            Self::EventPublish { .. } => "event.publish",
            Self::EventEdit { .. } => "event.edit",
            Self::EventDelete { .. } => "event.delete",
            Self::EventsFetch { .. } => "events.fetch",
            Self::EventTrigger { .. } => "event.trigger",
            Self::CursorUpdate { .. } => "cursor.update",
            Self::PresenceSet { .. } => "presence.set",
            Self::TypingStart { .. } => "typing.start",
            Self::TypingStop { .. } => "typing.stop",
            Self::Ping { .. } => "ping",
            Self::ReactionAdd { .. } => "reaction.add",
            Self::ReactionRemove { .. } => "reaction.remove",
        }
    }

    pub fn from_raw(raw: RawFrame) -> Result<Self, String> {
        let p = &raw.payload;
        match raw.type_.as_str() {
            "auth" => Ok(Self::Auth {
                ref_: raw.ref_,
                token: str_field(p, "token")?,
                last_seen_at: p.get("last_seen_at").and_then(|v| v.as_i64()),
            }),
            "auth.refresh" => Ok(Self::AuthRefresh {
                ref_: raw.ref_,
                token: str_field(p, "token")?,
            }),
            "subscribe" => Ok(Self::Subscribe {
                ref_: raw.ref_,
                streams: str_array_field(p, "streams")?,
            }),
            "unsubscribe" => Ok(Self::Unsubscribe {
                ref_: raw.ref_,
                streams: str_array_field(p, "streams")?,
            }),
            "event.publish" => Ok(Self::EventPublish {
                ref_: raw.ref_,
                stream: str_field(p, "stream")?,
                body: str_field(p, "body")?,
                meta: p.get("meta").cloned(),
                parent_id: p
                    .get("parent_id")
                    .and_then(|v| v.as_str())
                    .map(String::from),
            }),
            "event.edit" => Ok(Self::EventEdit {
                ref_: raw.ref_,
                stream: str_field(p, "stream")?,
                id: str_field(p, "id")?,
                body: str_field(p, "body")?,
            }),
            "cursor.update" => Ok(Self::CursorUpdate {
                stream: str_field(p, "stream")?,
                seq: u64_field(p, "seq")?,
            }),
            "presence.set" => {
                let s = str_field(p, "status")?;
                let status = PresenceStatus::from_str_loose(&s)
                    .ok_or_else(|| format!("invalid presence status: {s}"))?;
                Ok(Self::PresenceSet {
                    ref_: raw.ref_,
                    status,
                })
            }
            "typing.start" => Ok(Self::TypingStart {
                stream: str_field(p, "stream")?,
            }),
            "typing.stop" => Ok(Self::TypingStop {
                stream: str_field(p, "stream")?,
            }),
            "events.fetch" => Ok(Self::EventsFetch {
                ref_: raw.ref_,
                stream: str_field(p, "stream")?,
                before: p.get("before").and_then(|v| v.as_u64()),
                after: p.get("after").and_then(|v| v.as_u64()),
                limit: p
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .and_then(|v| u32::try_from(v).ok()),
            }),
            "event.delete" => Ok(Self::EventDelete {
                ref_: raw.ref_,
                stream: str_field(p, "stream")?,
                id: str_field(p, "id")?,
            }),
            "ping" => Ok(Self::Ping { ref_: raw.ref_ }),
            "event.trigger" => Ok(Self::EventTrigger {
                ref_: raw.ref_,
                stream: str_field(p, "stream")?,
                event: str_field(p, "event")?,
                data: p.get("data").cloned(),
            }),
            "reaction.add" => Ok(Self::ReactionAdd {
                ref_: raw.ref_,
                stream: str_field(p, "stream")?,
                event_id: str_field(p, "event_id")?,
                emoji: str_field(p, "emoji")?,
            }),
            "reaction.remove" => Ok(Self::ReactionRemove {
                ref_: raw.ref_,
                stream: str_field(p, "stream")?,
                event_id: str_field(p, "event_id")?,
                emoji: str_field(p, "emoji")?,
            }),
            other => Err(format!("unknown message type: {other}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Server → Client
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ServerMessage {
    #[serde(rename = "auth_ok")]
    AuthOk {
        #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
        ref_: Option<String>,
        payload: AuthOkPayload,
    },
    #[serde(rename = "auth_error")]
    AuthError {
        #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
        ref_: Option<String>,
        payload: ErrorPayload,
    },
    #[serde(rename = "subscribed")]
    Subscribed {
        #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
        ref_: Option<String>,
        payload: SubscribedPayload,
    },
    #[serde(rename = "event.new")]
    EventNew { payload: EventNewPayload },
    #[serde(rename = "event.ack")]
    EventAck {
        #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
        ref_: Option<String>,
        payload: EventAckPayload,
    },
    #[serde(rename = "events.batch")]
    EventsBatch {
        #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
        ref_: Option<String>,
        payload: EventsBatchPayload,
    },
    #[serde(rename = "event.edited")]
    EventEdited { payload: EventEditedPayload },
    #[serde(rename = "event.deleted")]
    EventDeleted { payload: EventDeletedPayload },
    #[serde(rename = "presence.changed")]
    PresenceChanged { payload: PresenceChangedPayload },
    #[serde(rename = "cursor.moved")]
    CursorMoved { payload: CursorMovedPayload },
    #[serde(rename = "member.joined")]
    MemberJoined { payload: MemberPayload },
    #[serde(rename = "member.left")]
    MemberLeft { payload: MemberPayload },
    #[serde(rename = "stream.updated")]
    StreamUpdated { payload: StreamEventPayload },
    #[serde(rename = "stream.deleted")]
    StreamDeleted { payload: StreamEventPayload },
    #[serde(rename = "system.token_expiring")]
    TokenExpiring { payload: TokenExpiringPayload },
    #[serde(rename = "typing")]
    Typing { payload: TypingPayload },
    #[serde(rename = "event.received")]
    EventReceived { payload: EventReceivedPayload },
    #[serde(rename = "watchlist.online")]
    WatchlistOnline { payload: WatchlistPayload },
    #[serde(rename = "watchlist.offline")]
    WatchlistOffline { payload: WatchlistPayload },
    #[serde(rename = "stream.subscriber_count")]
    StreamSubscriberCount {
        payload: StreamSubscriberCountPayload,
    },
    #[serde(rename = "reaction.changed")]
    ReactionChanged { payload: ReactionChangedPayload },
    #[serde(rename = "error")]
    Error {
        #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
        ref_: Option<String>,
        payload: ErrorPayload,
    },
    #[serde(rename = "pong")]
    Pong {
        #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
        ref_: Option<String>,
    },
}

impl ServerMessage {
    pub fn error(ref_: Option<String>, code: ErrorCode, message: impl Into<String>) -> Self {
        Self::Error {
            ref_,
            payload: ErrorPayload {
                code,
                message: message.into(),
            },
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            r#"{"type":"error","payload":{"code":"INTERNAL","message":"serialization failed"}}"#
                .to_string()
        })
    }
}

// ---------------------------------------------------------------------------
// Payload types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthOkPayload {
    pub user_id: String,
    pub connection_id: u64,
    pub server_time: i64,
    pub heartbeat_interval: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorPayload {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberPresence {
    pub user_id: String,
    pub role: Role,
    pub presence: PresenceStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribedPayload {
    pub stream: String,
    pub members: Vec<MemberPresence>,
    pub cursor: Sequence,
    pub latest_seq: Sequence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventNewPayload {
    pub stream: String,
    pub id: String,
    pub seq: Sequence,
    pub sender: String,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub sent_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventAckPayload {
    pub id: String,
    pub seq: Sequence,
    pub sent_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventsBatchPayload {
    pub stream: String,
    pub events: Vec<EventNewPayload>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEditedPayload {
    pub stream: String,
    pub id: String,
    pub seq: Sequence,
    pub body: String,
    pub edited_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventDeletedPayload {
    pub stream: String,
    pub id: String,
    pub seq: Sequence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceChangedPayload {
    pub user_id: String,
    pub presence: PresenceStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorMovedPayload {
    pub stream: String,
    pub user_id: String,
    pub seq: Sequence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberPayload {
    pub stream: String,
    pub user_id: String,
    pub role: Role,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamEventPayload {
    pub stream: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenExpiringPayload {
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypingPayload {
    pub stream: String,
    pub user_id: String,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventReceivedPayload {
    pub stream: String,
    pub event: String,
    pub sender: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchlistPayload {
    pub user_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamSubscriberCountPayload {
    pub stream: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReactionChangedPayload {
    pub stream: String,
    pub event_id: String,
    pub emoji: String,
    pub user_id: String,
    pub action: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn str_field(v: &Value, key: &str) -> Result<String, String> {
    v.get(key)
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| format!("missing or invalid field: {key}"))
}

fn u64_field(v: &Value, key: &str) -> Result<u64, String> {
    v.get(key)
        .and_then(|v| v.as_u64())
        .ok_or_else(|| format!("missing or invalid field: {key}"))
}

fn str_array_field(v: &Value, key: &str) -> Result<Vec<String>, String> {
    v.get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .ok_or_else(|| format!("missing or invalid field: {key}"))
}
