use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const MAX_RETRIES: u32 = 5;
pub const BASE_DELAY_SECS: u64 = 2;

#[derive(Debug, Clone, PartialEq)]
pub enum DeliveryStatus {
    Pending,
    Delivered,
    Failed,
}

#[derive(Debug, Clone)]
pub struct WebhookEndpoint {
    pub id: String,
    pub url: String,
    pub secret: String,
    pub active: bool,
}

#[derive(Debug, Clone)]
pub struct WebhookDelivery {
    pub id: String,
    pub endpoint_id: String,
    pub payload: String,
    pub status: DeliveryStatus,
    pub attempts: u32,
    pub next_retry_at: Option<u64>,
}

/// Validate that a URL is well-formed and uses https.
pub fn validate_url(url: &str) -> bool {
    url.starts_with("https://") && url.len() > 10
}

/// Compute HMAC-SHA256 signature for a payload using the endpoint secret.
pub fn sign_payload(secret: &str, payload: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(payload.as_bytes());
    let result = mac.finalize();
    hex::encode(result.into_bytes())
}

/// Verify an incoming webhook signature.
pub fn verify_signature(secret: &str, payload: &str, signature: &str) -> bool {
    let expected = sign_payload(secret, payload);
    // Constant-time comparison to prevent timing attacks.
    expected.len() == signature.len()
        && expected
            .bytes()
            .zip(signature.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

/// Compute the next retry timestamp using exponential backoff.
/// delay = BASE_DELAY_SECS * 2^attempt (capped at 1 hour).
pub fn next_retry_at(attempt: u32) -> u64 {
    let delay = BASE_DELAY_SECS.saturating_mul(1u64 << attempt.min(10));
    let delay = delay.min(3600);
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
        + delay
}

/// Determine whether a delivery should be retried.
pub fn should_retry(delivery: &WebhookDelivery) -> bool {
    delivery.status == DeliveryStatus::Failed && delivery.attempts < MAX_RETRIES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_url_valid() {
        assert!(validate_url("https://example.com/webhook"));
    }

    #[test]
    fn test_validate_url_rejects_http() {
        assert!(!validate_url("http://example.com/webhook"));
    }

    #[test]
    fn test_sign_and_verify() {
        let secret = "my-secret";
        let payload = r#"{"event":"payment","amount":100}"#;
        let sig = sign_payload(secret, payload);
        assert!(verify_signature(secret, payload, &sig));
    }

    #[test]
    fn test_verify_rejects_wrong_signature() {
        assert!(!verify_signature("secret", "payload", "badsig"));
    }

    #[test]
    fn test_exponential_backoff_increases() {
        let t0 = next_retry_at(0);
        let t1 = next_retry_at(1);
        assert!(t1 > t0);
    }

    #[test]
    fn test_should_retry_when_failed_and_under_limit() {
        let d = WebhookDelivery {
            id: "1".into(),
            endpoint_id: "e1".into(),
            payload: "{}".into(),
            status: DeliveryStatus::Failed,
            attempts: 2,
            next_retry_at: None,
        };
        assert!(should_retry(&d));
    }

    #[test]
    fn test_should_not_retry_when_max_attempts_reached() {
        let d = WebhookDelivery {
            id: "1".into(),
            endpoint_id: "e1".into(),
            payload: "{}".into(),
            status: DeliveryStatus::Failed,
            attempts: MAX_RETRIES,
            next_retry_at: None,
        };
        assert!(!should_retry(&d));
    }

    #[test]
    fn test_should_not_retry_when_delivered() {
        let d = WebhookDelivery {
            id: "1".into(),
            endpoint_id: "e1".into(),
            payload: "{}".into(),
            status: DeliveryStatus::Delivered,
            attempts: 1,
            next_retry_at: None,
        };
        assert!(!should_retry(&d));
    }
}
