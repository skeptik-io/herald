use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

const MAX_ID_LEN: usize = 255;
const MAX_BODY_LEN: usize = 65_536; // 64KB
const MAX_META_SIZE: usize = 16_384; // 16KB
const MAX_NAME_LEN: usize = 512;

type ValidationError = Box<(StatusCode, axum::response::Response)>;

/// Validate an entity ID (tenant, stream, user).
/// Must be 1-255 chars, alphanumeric + hyphen + underscore + dot.
pub fn validate_id(id: &str, field: &str) -> Result<(), ValidationError> {
    if id.is_empty() {
        return Err(bad_request(&format!("{field} must not be empty")));
    }
    if id.len() > MAX_ID_LEN {
        return Err(bad_request(&format!(
            "{field} exceeds maximum length ({MAX_ID_LEN})"
        )));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(bad_request(&format!(
            "{field} contains invalid characters (allowed: alphanumeric, hyphen, underscore, dot)"
        )));
    }
    Ok(())
}

/// Validate a display name.
pub fn validate_name(name: &str, field: &str) -> Result<(), ValidationError> {
    if name.is_empty() {
        return Err(bad_request(&format!("{field} must not be empty")));
    }
    if name.len() > MAX_NAME_LEN {
        return Err(bad_request(&format!(
            "{field} exceeds maximum length ({MAX_NAME_LEN})"
        )));
    }
    if name.contains('\0') {
        return Err(bad_request(&format!("{field} contains null bytes")));
    }
    Ok(())
}

/// Validate an event body.
pub fn validate_body(body: &str) -> Result<(), ValidationError> {
    if body.len() > MAX_BODY_LEN {
        return Err(bad_request(&format!(
            "event body exceeds maximum length ({MAX_BODY_LEN} bytes)"
        )));
    }
    Ok(())
}

/// Validate a meta JSON field.
pub fn validate_meta(meta: &Option<serde_json::Value>) -> Result<(), ValidationError> {
    if let Some(m) = meta {
        let size = serde_json::to_string(m).map(|s| s.len()).unwrap_or(0);
        if size > MAX_META_SIZE {
            return Err(bad_request(&format!(
                "meta field exceeds maximum size ({MAX_META_SIZE} bytes)"
            )));
        }
    }
    Ok(())
}

#[derive(serde::Deserialize)]
pub struct PaginationQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

impl PaginationQuery {
    pub fn resolve(&self) -> (usize, usize) {
        let limit = self.limit.unwrap_or(50).min(500);
        let offset = self.offset.unwrap_or(0);
        (limit, offset)
    }
}

const MAX_ATTACHMENTS: usize = 10;

/// Validate attachment metadata in the meta field.
pub fn validate_attachments(meta: &Option<serde_json::Value>) -> Result<(), ValidationError> {
    if let Some(m) = meta {
        if let Some(attachments) = m.get("attachments") {
            if let Some(arr) = attachments.as_array() {
                if arr.len() > MAX_ATTACHMENTS {
                    return Err(bad_request(&format!(
                        "maximum {MAX_ATTACHMENTS} attachments per event"
                    )));
                }
                for (i, att) in arr.iter().enumerate() {
                    if att.get("url").and_then(|v| v.as_str()).is_none() {
                        return Err(bad_request(&format!(
                            "attachment[{i}] missing required 'url' field"
                        )));
                    }
                }
            }
        }
    }
    Ok(())
}

fn bad_request(msg: &str) -> ValidationError {
    Box::new((
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"error": msg})).into_response(),
    ))
}
