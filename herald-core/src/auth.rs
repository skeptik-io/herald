use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    pub sub: String,
    #[serde(default)]
    pub tenant: String,
    pub rooms: Vec<String>,
    pub exp: u64,
    pub iat: u64,
    pub iss: String,
    #[serde(default)]
    pub watchlist: Vec<String>,
}
