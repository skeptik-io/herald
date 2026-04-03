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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Room {
    pub id: RoomId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
    #[serde(default)]
    pub archived: bool,
    pub created_at: i64,
}
