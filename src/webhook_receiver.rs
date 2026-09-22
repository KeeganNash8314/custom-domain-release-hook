use crate::domain_release::{DomainVerified, ReleaseLedger};
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use hmac::{Hmac, Mac};
use serde_json::json;
use sha2::Sha256;
use thiserror::Error;

#[derive(Clone)]
pub struct WebhookState {
    pub secret: String,
    pub ledger: ReleaseLedger,
}

#[derive(Debug, Error)]
pub enum WebhookError {
    #[error("signature header is missing")]
    MissingSignature,
    #[error("signature is invalid")]
    InvalidSignature,
    #[error("notification JSON is invalid: {0}")]
    InvalidJson(#[from] serde_json::Error),
}

impl IntoResponse for WebhookError {
    fn into_response(self) -> Response {
        let status = match self {
            Self::MissingSignature | Self::InvalidSignature => StatusCode::UNAUTHORIZED,
            Self::InvalidJson(_) => StatusCode::BAD_REQUEST,
        };
        (status, Json(json!({"error": self.to_string()}))).into_response()
    }
}

pub async fn receive(
    State(state): State<WebhookState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, WebhookError> {
    let supplied = headers
        .get("x-infrai-signature")
        .and_then(|value| value.to_str().ok())
        .ok_or(WebhookError::MissingSignature)?;
    let signature = supplied.strip_prefix("sha256=").unwrap_or(supplied);
    let signature = hex::decode(signature).map_err(|_| WebhookError::InvalidSignature)?;
    let mut mac = Hmac::<Sha256>::new_from_slice(state.secret.as_bytes())
        .map_err(|_| WebhookError::InvalidSignature)?;
    mac.update(&body);
    mac.verify_slice(&signature)
        .map_err(|_| WebhookError::InvalidSignature)?;

    let notification: DomainVerified = serde_json::from_slice(&body)?;
    let released = state.ledger.apply_verification(&notification).await;
    Ok((StatusCode::OK, Json(json!({"released": released}))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request, routing::post, Router};
    use hmac::Mac;
    use tower::ServiceExt;

    #[tokio::test]
    async fn signed_notification_advances_the_release() {
        let ledger = ReleaseLedger::default();
        ledger
            .track("build-42".into(), "docs.example.com".into())
            .await;
        let secret = "local-test-secret";
        let body = br#"{"event":"dns.domain.verified","data":{"domain":"docs.example.com"}}"#;
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let signature = hex::encode(mac.finalize().into_bytes());
        let app = Router::new()
            .route("/webhooks/infrai", post(receive))
            .with_state(WebhookState {
                secret: secret.into(),
                ledger,
            });

        let response = app
            .oneshot(
                Request::post("/webhooks/infrai")
                    .header("x-infrai-signature", format!("sha256={signature}"))
                    .body(Body::from(body.as_slice()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
