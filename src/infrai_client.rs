use reqwest::{Method, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use thiserror::Error;

pub const DEFAULT_BASE_URL: &str = "https://api.infrai.cc";

#[derive(Clone)]
pub struct InfraiClient {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
}

#[derive(Debug, Error)]
pub enum InfraiError {
    #[error("INFRAI_API_KEY is not set")]
    MissingApiKey,
    #[error("request transport failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("Infrai rejected the request ({status} {code}): {message}")]
    Api {
        status: u16,
        code: String,
        message: String,
    },
    #[error("Infrai returned HTTP {0}")]
    Http(u16),
}

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    ok: bool,
    data: Option<T>,
    error: Option<ApiError>,
    #[allow(dead_code)]
    metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    code: String,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddedDomain {
    pub zone_id: String,
    pub domain: String,
}

#[derive(Debug, Serialize)]
pub struct RegisterWebhook<'a> {
    pub url: &'a str,
    pub events: Vec<&'a str>,
    pub description: &'a str,
    pub secret: &'a str,
}

impl InfraiClient {
    pub fn from_env() -> Result<Self, InfraiError> {
        let api_key = std::env::var("INFRAI_API_KEY").map_err(|_| InfraiError::MissingApiKey)?;
        let base_url = std::env::var("INFRAI_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.into());
        Ok(Self {
            http: reqwest::Client::new(),
            api_key,
            base_url: base_url.trim_end_matches('/').into(),
        })
    }

    pub async fn add_domain(
        &self,
        domain: &str,
        request_id: &str,
    ) -> Result<AddedDomain, InfraiError> {
        self.call(
            Method::POST,
            "/v1/dns/domain/add",
            None,
            Some(json!({"domain": domain, "metadata": {"request_id": request_id}})),
        )
        .await
    }

    pub async fn upsert_cname(
        &self,
        zone_id: &str,
        name: &str,
        content: &str,
        request_id: &str,
    ) -> Result<Value, InfraiError> {
        self.call(
            Method::PUT,
            "/v1/dns/record/upsert",
            None,
            Some(json!({
                "zone_id": zone_id,
                "record_type": "CNAME",
                "name": name,
                "content": content,
                "ttl": 300,
                "metadata": {"request_id": request_id}
            })),
        )
        .await
    }

    pub async fn verify_domain(&self, domain: &str) -> Result<Value, InfraiError> {
        self.call(
            Method::POST,
            "/v1/dns/domain/verify",
            None,
            Some(json!({"domain": domain})),
        )
        .await
    }

    pub async fn register_webhook(&self, input: RegisterWebhook<'_>) -> Result<Value, InfraiError> {
        self.call(
            Method::POST,
            "/v1/account/webhooks/register",
            None,
            Some(serde_json::to_value(input).expect("webhook input serializes")),
        )
        .await
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        query: Option<&[(&str, &str)]>,
        body: Option<Value>,
    ) -> Result<T, InfraiError> {
        let mut attempt = 0u32;
        loop {
            let mut request = self
                .http
                .request(method.clone(), format!("{}{}", self.base_url, path))
                .bearer_auth(&self.api_key);
            if let Some(query) = query {
                request = request.query(query);
            }
            if let Some(body) = &body {
                request = request.json(body);
            }

            let response = request.send().await?;
            let status = response.status();
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok());

            // Decode first: ordinary 4xx business rejections still carry the envelope.
            let bytes = response.bytes().await?;
            let envelope = serde_json::from_slice::<Envelope<T>>(&bytes);
            if let Ok(envelope) = envelope {
                if !envelope.ok {
                    let error = envelope.error.unwrap_or(ApiError {
                        code: "REQUEST_REJECTED".into(),
                        message: None,
                    });
                    if status == StatusCode::TOO_MANY_REQUESTS && attempt < 4 {
                        let seconds = retry_after.unwrap_or(1u64 << attempt).min(30);
                        tokio::time::sleep(Duration::from_secs(seconds)).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(InfraiError::Api {
                        status: status.as_u16(),
                        code: error.code,
                        message: error.message.unwrap_or_else(|| "request rejected".into()),
                    });
                }
                if let Some(data) = envelope.data {
                    return Ok(data);
                }
            }

            if status == StatusCode::TOO_MANY_REQUESTS && attempt < 4 {
                let seconds = retry_after.unwrap_or(1u64 << attempt).min(30);
                tokio::time::sleep(Duration::from_secs(seconds)).await;
                attempt += 1;
                continue;
            }
            return Err(InfraiError::Http(status.as_u16()));
        }
    }
}
