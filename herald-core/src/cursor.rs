use serde::{Deserialize, Serialize};

use crate::event::Sequence;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Cursor {
    pub stream_id: String,
    pub user_id: String,
    pub seq: Sequence,
    pub updated_at: i64,
}
