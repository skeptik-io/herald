use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum PresenceStatus {
    Online,
    Away,
    Dnd,
    Offline,
}

impl PresenceStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Away => "away",
            Self::Dnd => "dnd",
            Self::Offline => "offline",
        }
    }

    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s {
            "online" => Some(Self::Online),
            "away" => Some(Self::Away),
            "dnd" => Some(Self::Dnd),
            "offline" => Some(Self::Offline),
            _ => None,
        }
    }

    pub fn is_manual(&self) -> bool {
        matches!(self, Self::Away | Self::Dnd)
    }
}
