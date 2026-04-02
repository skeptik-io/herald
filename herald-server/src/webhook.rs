use std::sync::Arc;
use std::time::Duration;

use hmac::{Hmac, Mac};
use sha2::Sha256;
use tracing::{debug, warn};

use crate::config::WebhookConfig;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, serde::Serialize)]
pub struct WebhookEvent {
    pub event: String,
    pub room: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sent_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

fn sign(secret: &str, timestamp: i64, body: &str) -> String {
    // HMAC-SHA256 accepts keys of any length (long keys are hashed first)
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC-SHA256 accepts any key length");
    let payload = format!("{timestamp}.{body}");
    mac.update(payload.as_bytes());
    let result = mac.finalize();
    format!("sha256={}", hex::encode(result.into_bytes()))
}

pub fn deliver(config: Arc<WebhookConfig>, client: reqwest::Client, event: WebhookEvent) {
    tokio::spawn(async move {
        let body = match serde_json::to_string(&event) {
            Ok(b) => b,
            Err(e) => {
                warn!("webhook: failed to serialize event: {e}");
                return;
            }
        };

        let timestamp = crate::ws::connection::now_millis() / 1000;
        let signature = sign(&config.secret, timestamp, &body);

        for attempt in 0..=config.retries {
            let result = client
                .post(&config.url)
                .header("Content-Type", "application/json")
                .header("X-Herald-Signature", &signature)
                .header("X-Herald-Timestamp", timestamp.to_string())
                .body(body.clone())
                .timeout(Duration::from_secs(10))
                .send()
                .await;

            match result {
                Ok(resp) if resp.status().is_success() => {
                    debug!(
                        event = %event.event,
                        room = %event.room,
                        attempt = attempt,
                        "webhook delivered"
                    );
                    return;
                }
                Ok(resp) => {
                    warn!(
                        event = %event.event,
                        status = %resp.status(),
                        attempt = attempt,
                        "webhook rejected"
                    );
                }
                Err(e) => {
                    warn!(
                        event = %event.event,
                        error = %e,
                        attempt = attempt,
                        "webhook delivery failed"
                    );
                }
            }

            if attempt < config.retries {
                let backoff = Duration::from_secs(1 << attempt);
                tokio::time::sleep(backoff).await;
            }
        }

        warn!(
            event = %event.event,
            room = %event.room,
            retries = config.retries,
            "webhook delivery exhausted all retries"
        );
    });
}

pub fn verify_signature(secret: &str, timestamp: i64, body: &str, signature: &str) -> bool {
    let expected = sign(secret, timestamp, body);
    // Constant-time comparison
    if expected.len() != signature.len() {
        return false;
    }
    expected
        .as_bytes()
        .iter()
        .zip(signature.as_bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_and_verify() {
        let secret = "test-secret";
        let timestamp = 1712000001;
        let body = r#"{"event":"message.new","room":"chat"}"#;

        let sig = sign(secret, timestamp, body);
        assert!(sig.starts_with("sha256="));
        assert!(verify_signature(secret, timestamp, body, &sig));
        assert!(!verify_signature("wrong-secret", timestamp, body, &sig));
        assert!(!verify_signature(secret, timestamp + 1, body, &sig));
    }
}
