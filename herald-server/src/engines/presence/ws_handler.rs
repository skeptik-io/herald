use std::sync::Arc;

use tokio::sync::mpsc;

use herald_core::protocol::*;

use crate::state::AppState;
use crate::ws::connection::ConnContext;

pub async fn handle_presence_set(
    state: &Arc<AppState>,
    ctx: &ConnContext,
    _tx: &mpsc::Sender<ServerMessage>,
    _ref_: Option<String>,
    status: herald_core::presence::PresenceStatus,
    until: Option<String>,
) {
    // Parse the `until` ISO 8601 string to unix millis for the tracker.
    let until_ms = until.as_deref().and_then(parse_iso8601_millis);

    state.presence.set_manual(
        &ctx.tenant_id,
        &ctx.user_id,
        status,
        until_ms,
        until.clone(),
    );

    // Persist override to WAL (or remove if clearing)
    if status == herald_core::presence::PresenceStatus::Online {
        // Clearing override — remove from WAL
        let _ = crate::store::presence_overrides::remove(&*state.db, &ctx.tenant_id, &ctx.user_id)
            .await;
    } else {
        // Setting override — persist to WAL
        let po = crate::registry::presence::PersistedOverride {
            tenant_id: ctx.tenant_id.clone(),
            user_id: ctx.user_id.clone(),
            status,
            expires_at_ms: until_ms,
            until_iso: until.clone(),
        };
        let _ = crate::store::presence_overrides::save(&*state.db, &po).await;
    }

    // Broadcast to all member streams + __presence.  Do NOT exclude the
    // sender — other sessions for the same user need the update.
    crate::ws::connection::broadcast_presence_change(state, &ctx.tenant_id, &ctx.user_id).await;

    // Notify watchlist watchers of the override
    let watchers = state.connections.get_watchers(&ctx.tenant_id, &ctx.user_id);
    if !watchers.is_empty() {
        let msg = ServerMessage::PresenceChanged {
            payload: PresenceChangedPayload {
                user_id: ctx.user_id.clone(),
                presence: status,
                until,
                last_seen_at: None,
            },
        };
        for watcher_id in &watchers {
            state
                .connections
                .send_to_user(&ctx.tenant_id, watcher_id, &msg);
        }
    }
}

/// Called when a user's first connection is established.
/// Broadcasts presence change and notifies watchlist watchers.
pub async fn on_connect(
    state: &Arc<AppState>,
    tenant_id: &str,
    user_id: &str,
    watchlist: &[String],
    tx: &mpsc::Sender<ServerMessage>,
) {
    // Broadcast presence change to all streams
    crate::ws::connection::broadcast_presence_change(state, tenant_id, user_id).await;

    // Set up watchlist and send initial presence status for watched users
    if !watchlist.is_empty() {
        state
            .connections
            .set_watchlist(tenant_id, user_id, watchlist.to_vec());

        // Send initial online status for watched users who are already connected.
        // Check resolved presence: if a user has an offline/dnd/away override,
        // don't report them as online.
        let online_ids: Vec<String> = watchlist
            .iter()
            .filter(|uid| {
                let resolved = state.presence.resolve_status(
                    tenant_id,
                    uid,
                    &state.connections,
                    state.config.presence.manual_override_ttl_secs,
                );
                resolved == herald_core::presence::PresenceStatus::Online
            })
            .cloned()
            .collect();
        if !online_ids.is_empty() {
            let _ = tx
                .send(ServerMessage::WatchlistOnline {
                    payload: WatchlistPayload {
                        user_ids: online_ids,
                        last_seen_at: None,
                    },
                })
                .await;
        }
    }

    // Notify users who have this user in their watchlist (only on first connection)
    if state.connections.user_connection_count(tenant_id, user_id) == 1 {
        // Clear last-seen — user is now online
        state.presence.clear_last_seen(tenant_id, user_id);
        // Check resolved presence before notifying — if user has an override,
        // send actual status instead of assuming "online"
        let resolved = state.presence.resolve(
            tenant_id,
            user_id,
            &state.connections,
            state.config.presence.manual_override_ttl_secs,
        );

        let watchers = state.connections.get_watchers(tenant_id, user_id);
        if !watchers.is_empty() {
            if resolved.status == herald_core::presence::PresenceStatus::Online {
                let msg = ServerMessage::WatchlistOnline {
                    payload: WatchlistPayload {
                        user_ids: vec![user_id.to_string()],
                        last_seen_at: None,
                    },
                };
                for watcher_id in &watchers {
                    state.connections.send_to_user(tenant_id, watcher_id, &msg);
                }
            } else {
                let msg = ServerMessage::PresenceChanged {
                    payload: PresenceChangedPayload {
                        user_id: user_id.to_string(),
                        presence: resolved.status,
                        until: resolved.until,
                        last_seen_at: None,
                    },
                };
                for watcher_id in &watchers {
                    state.connections.send_to_user(tenant_id, watcher_id, &msg);
                }
            }
        }
    }
}

/// Called when a user's last connection disconnects (after linger timeout).
/// Broadcasts offline presence and cleans up watchlist entries.
pub async fn on_last_disconnect(state: &Arc<AppState>, tenant_id: &str, user_id: &str) {
    // Stamp last-seen and persist to WAL
    let now_ms = crate::ws::connection::now_millis();
    state.presence.stamp_last_seen(tenant_id, user_id, now_ms);
    let entry = crate::registry::presence::PersistedLastSeen {
        tenant_id: tenant_id.to_string(),
        user_id: user_id.to_string(),
        last_seen_at_ms: now_ms,
    };
    let _ = crate::store::last_seen::save(&*state.db, &entry).await;

    crate::ws::connection::broadcast_presence_change(state, tenant_id, user_id).await;

    // Notify watchlist watchers
    let watchers = state.connections.get_watchers(tenant_id, user_id);
    if !watchers.is_empty() {
        let msg = ServerMessage::WatchlistOffline {
            payload: WatchlistPayload {
                user_ids: vec![user_id.to_string()],
                last_seen_at: Some(std::collections::HashMap::from([(
                    user_id.to_string(),
                    now_ms,
                )])),
            },
        };
        for watcher_id in &watchers {
            state.connections.send_to_user(tenant_id, watcher_id, &msg);
        }
    }

    // Clean up watchlist entries for the disconnected user
    state.connections.cleanup_watchlist(tenant_id, user_id);
}

/// Parse an ISO 8601 / RFC 3339 date-time string to Unix millis.
fn parse_iso8601_millis(s: &str) -> Option<i64> {
    let s = s.trim();
    let normalized = if let Some(stripped) = s.strip_suffix('Z') {
        format!("{stripped}+00:00")
    } else {
        s.to_string()
    };

    let parts: Vec<&str> = normalized.splitn(2, 'T').collect();
    if parts.len() != 2 {
        return None;
    }

    let date_parts: Vec<u32> = parts[0].split('-').filter_map(|p| p.parse().ok()).collect();
    if date_parts.len() != 3 {
        return None;
    }

    let time_str = parts[1];
    let (time_part, tz_offset_secs) = if let Some(pos) = time_str.rfind('+') {
        let (t, tz) = time_str.split_at(pos);
        (t, parse_tz_offset(tz))
    } else if let Some(pos) = time_str.rfind('-') {
        if pos > 2 {
            let (t, tz) = time_str.split_at(pos);
            (t, parse_tz_offset(tz))
        } else {
            (time_str, Some(0))
        }
    } else {
        (time_str, Some(0))
    };

    let tz_offset = tz_offset_secs?;
    let time_part = time_part.split('.').next().unwrap_or(time_part);
    let time_parts: Vec<u32> = time_part
        .split(':')
        .filter_map(|p| p.parse().ok())
        .collect();
    if time_parts.len() < 2 {
        return None;
    }

    let (year, month, day) = (date_parts[0] as i64, date_parts[1], date_parts[2]);
    let (hour, min, sec) = (
        time_parts[0] as i64,
        time_parts[1] as i64,
        *time_parts.get(2).unwrap_or(&0) as i64,
    );

    let mut days = 0i64;
    for y in 1970..year {
        days += if is_leap(y) { 366 } else { 365 };
    }
    let month_days = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    for m in 1..month {
        days += month_days[m as usize] as i64;
        if m == 2 && is_leap(year) {
            days += 1;
        }
    }
    days += day as i64 - 1;

    let unix_secs = days * 86400 + hour * 3600 + min * 60 + sec - tz_offset;
    Some(unix_secs * 1000)
}

fn parse_tz_offset(s: &str) -> Option<i64> {
    let sign = if s.starts_with('+') { 1i64 } else { -1i64 };
    let s = &s[1..];
    let parts: Vec<&str> = s.split(':').collect();
    let hours: i64 = parts.first()?.parse().ok()?;
    let mins: i64 = parts.get(1).and_then(|m| m.parse().ok()).unwrap_or(0);
    Some(sign * (hours * 3600 + mins * 60))
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}
