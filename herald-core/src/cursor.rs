use serde::{Deserialize, Serialize};

use crate::event::Sequence;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cursor {
    pub stream_id: String,
    pub user_id: String,
    pub seq: Sequence,
    pub updated_at: i64,
}
