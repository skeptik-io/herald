use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RoomId(pub String);

impl RoomId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for RoomId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl std::fmt::Display for RoomId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum EncryptionMode {
    #[default]
    Plaintext,
    ServerEncrypted,
    E2e,
}

impl EncryptionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Plaintext => "plaintext",
            Self::ServerEncrypted => "server_encrypted",
            Self::E2e => "e2e",
        }
    }

    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s {
            "plaintext" => Some(Self::Plaintext),
            "server_encrypted" | "server-encrypted" => Some(Self::ServerEncrypted),
            "e2e" => Some(Self::E2e),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Room {
    pub id: RoomId,
    pub name: String,
    #[serde(default)]
    pub encryption_mode: EncryptionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
    pub created_at: i64,
}
