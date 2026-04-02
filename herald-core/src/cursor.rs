use serde::{Deserialize, Serialize};

use crate::message::Sequence;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cursor {
    pub room_id: String,
    pub user_id: String,
    pub seq: Sequence,
    pub updated_at: i64,
}
