use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    TokenExpired,
    TokenInvalid,
    Unauthorized,
    NotSubscribed,
    StreamNotFound,
    RateLimited,
    BadRequest,
    Internal,
}

impl ErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TokenExpired => "TOKEN_EXPIRED",
            Self::TokenInvalid => "TOKEN_INVALID",
            Self::Unauthorized => "UNAUTHORIZED",
            Self::NotSubscribed => "NOT_SUBSCRIBED",
            Self::StreamNotFound => "STREAM_NOT_FOUND",
            Self::RateLimited => "RATE_LIMITED",
            Self::BadRequest => "BAD_REQUEST",
            Self::Internal => "INTERNAL",
        }
    }

    pub fn http_status(&self) -> u16 {
        match self {
            Self::TokenExpired | Self::TokenInvalid => 401,
            Self::Unauthorized => 403,
            Self::NotSubscribed | Self::StreamNotFound => 404,
            Self::RateLimited => 429,
            Self::BadRequest => 400,
            Self::Internal => 500,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeraldError {
    pub code: ErrorCode,
    pub message: String,
}

impl HeraldError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for HeraldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for HeraldError {}
