use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::ErrorCode;
use crate::member::Role;
use crate::message::Sequence;
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
        rooms: Vec<String>,
    },
    Unsubscribe {
        ref_: Option<String>,
        rooms: Vec<String>,
    },
    MessageSend {
        ref_: Option<String>,
        room: String,
        body: String,
        meta: Option<Value>,
    },
    CursorUpdate {
        room: String,
        seq: Sequence,
    },
    PresenceSet {
        ref_: Option<String>,
        status: PresenceStatus,
    },
    TypingStart {
        room: String,
    },
    TypingStop {
        room: String,
    },
    MessagesFetch {
        ref_: Option<String>,
        room: String,
        before: Option<Sequence>,
        limit: Option<u32>,
    },
    MessageDelete {
        ref_: Option<String>,
        room: String,
        id: String,
    },
    Ping {
        ref_: Option<String>,
    },
    EventTrigger {
        ref_: Option<String>,
        room: String,
        event: String,
        data: Option<Value>,
    },
}

impl ClientMessage {
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
                rooms: str_array_field(p, "rooms")?,
            }),
            "unsubscribe" => Ok(Self::Unsubscribe {
                ref_: raw.ref_,
                rooms: str_array_field(p, "rooms")?,
            }),
            "message.send" => Ok(Self::MessageSend {
                ref_: raw.ref_,
                room: str_field(p, "room")?,
                body: str_field(p, "body")?,
                meta: p.get("meta").cloned(),
            }),
            "cursor.update" => Ok(Self::CursorUpdate {
                room: str_field(p, "room")?,
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
                room: str_field(p, "room")?,
            }),
            "typing.stop" => Ok(Self::TypingStop {
                room: str_field(p, "room")?,
            }),
            "messages.fetch" => Ok(Self::MessagesFetch {
                ref_: raw.ref_,
                room: str_field(p, "room")?,
                before: p.get("before").and_then(|v| v.as_u64()),
                limit: p
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .and_then(|v| u32::try_from(v).ok()),
            }),
            "message.delete" => Ok(Self::MessageDelete {
                ref_: raw.ref_,
                room: str_field(p, "room")?,
                id: str_field(p, "id")?,
            }),
            "ping" => Ok(Self::Ping { ref_: raw.ref_ }),
            "event.trigger" => Ok(Self::EventTrigger {
                ref_: raw.ref_,
                room: str_field(p, "room")?,
                event: str_field(p, "event")?,
                data: p.get("data").cloned(),
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
    #[serde(rename = "message.new")]
    MessageNew { payload: MessageNewPayload },
    #[serde(rename = "message.ack")]
    MessageAck {
        #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
        ref_: Option<String>,
        payload: MessageAckPayload,
    },
    #[serde(rename = "messages.batch")]
    MessagesBatch {
        #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
        ref_: Option<String>,
        payload: MessagesBatchPayload,
    },
    #[serde(rename = "message.deleted")]
    MessageDeleted { payload: MessageDeletedPayload },
    #[serde(rename = "presence.changed")]
    PresenceChanged { payload: PresenceChangedPayload },
    #[serde(rename = "cursor.moved")]
    CursorMoved { payload: CursorMovedPayload },
    #[serde(rename = "member.joined")]
    MemberJoined { payload: MemberPayload },
    #[serde(rename = "member.left")]
    MemberLeft { payload: MemberPayload },
    #[serde(rename = "room.updated")]
    RoomUpdated { payload: RoomEventPayload },
    #[serde(rename = "room.deleted")]
    RoomDeleted { payload: RoomEventPayload },
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
    #[serde(rename = "room.subscriber_count")]
    RoomSubscriberCount { payload: RoomSubscriberCountPayload },
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
    pub room: String,
    pub members: Vec<MemberPresence>,
    pub cursor: Sequence,
    pub latest_seq: Sequence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageNewPayload {
    pub room: String,
    pub id: String,
    pub seq: Sequence,
    pub sender: String,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
    pub sent_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageAckPayload {
    pub id: String,
    pub seq: Sequence,
    pub sent_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagesBatchPayload {
    pub room: String,
    pub messages: Vec<MessageNewPayload>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDeletedPayload {
    pub room: String,
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
    pub room: String,
    pub user_id: String,
    pub seq: Sequence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberPayload {
    pub room: String,
    pub user_id: String,
    pub role: Role,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomEventPayload {
    pub room: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenExpiringPayload {
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypingPayload {
    pub room: String,
    pub user_id: String,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventReceivedPayload {
    pub room: String,
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
pub struct RoomSubscriberCountPayload {
    pub room: String,
    pub count: usize,
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
