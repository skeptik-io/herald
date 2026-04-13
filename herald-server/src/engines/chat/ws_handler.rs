use std::sync::Arc;

use herald_core::protocol::*;

use crate::state::AppState;
use crate::ws::connection::ConnContext;
use crate::ws::fanout::fanout_to_stream;

pub async fn handle_typing(state: &Arc<AppState>, ctx: &ConnContext, stream: &str, active: bool) {
    state
        .typing
        .set_typing(&ctx.tenant_id, stream, &ctx.user_id, active);
    let msg = ServerMessage::Typing {
        payload: TypingPayload {
            stream: stream.to_string(),
            user_id: ctx.user_id.clone(),
            active,
        },
    };
    fanout_to_stream(
        state,
        &ctx.tenant_id,
        stream,
        &msg,
        Some(ctx.conn_id),
        Some(&ctx.user_id),
    )
    .await;
}
