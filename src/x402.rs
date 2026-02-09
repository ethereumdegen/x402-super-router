use axum::{
    body::Body,
    http::{header, HeaderMap, StatusCode},
    response::Response,
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::domain_types::DomainU256;

// ── x402 Protocol Types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequiredResponse {
    pub x402_version: u32,
    pub accepts: Vec<PaymentRequirements>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequirements {
    pub scheme: String,
    pub network: String,
    pub max_amount_required: String,
    pub resource: String,
    pub description: String,
    pub mime_type: String,
    pub pay_to: String,
    pub max_timeout_seconds: u64,
    pub asset: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyRequest {
    pub x402_version: u32,
    pub payment_payload: serde_json::Value,
    pub payment_requirements: PaymentRequirements,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyResponse {
    pub is_valid: bool,
    #[serde(default)]
    pub invalid_reason: Option<String>,
    #[serde(default)]
    pub payer: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettleResponse {
    pub success: bool,
    pub network: String,
    #[serde(default)]
    pub transaction: Option<String>,
    #[serde(default)]
    pub error_reason: Option<String>,
    #[serde(default)]
    pub payer: Option<String>,
}

// ── Helpers ──

fn build_payment_requirements(
    config: &Config,
    amount: DomainU256,
    resource: &str,
    description: &str,
) -> PaymentRequirements {
    PaymentRequirements {
        scheme: "permit".to_string(),
        network: config.payment_network.clone(),
        max_amount_required: amount.to_string(),
        resource: resource.to_string(),
        description: description.to_string(),
        mime_type: "application/json".to_string(),
        pay_to: config.wallet_address.clone(),
        max_timeout_seconds: 300,
        asset: config.payment_token_address.clone(),
        extra: Some(serde_json::json!({
            "token": config.payment_token_symbol,
            "address": config.payment_token_address,
            "decimals": config.payment_token_decimals,
            "name": config.payment_token_name,
            "version": config.payment_token_version,
            "facilitatorSigner": config.facilitator_signer,
            "minimum_amount": true
        })),
    }
}

fn payment_required_response(
    config: &Config,
    amount: DomainU256,
    resource: &str,
    description: &str,
) -> Response {
    let requirements = build_payment_requirements(config, amount, resource, description);
    let response = PaymentRequiredResponse {
        x402_version: 1,
        accepts: vec![requirements],
        error: None,
    };
    let body = serde_json::to_string(&response).unwrap_or_default();

    Response::builder()
        .status(StatusCode::PAYMENT_REQUIRED)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap()
}

fn error_response(status: StatusCode, message: &str) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from(message.to_string()))
        .unwrap()
}

async fn verify_payment(
    http_client: &reqwest::Client,
    facilitator_url: &str,
    verify_request: &VerifyRequest,
) -> Result<VerifyResponse, String> {
    let url = format!("{}/verify", facilitator_url);
    let response = http_client
        .post(&url)
        .json(verify_request)
        .send()
        .await
        .map_err(|e| format!("Failed to contact facilitator: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Facilitator error: {} - {}", status, body));
    }

    response
        .json::<VerifyResponse>()
        .await
        .map_err(|e| format!("Failed to parse verify response: {}", e))
}

async fn settle_payment(
    http_client: &reqwest::Client,
    facilitator_url: &str,
    settle_request: &VerifyRequest,
) -> Result<SettleResponse, String> {
    let url = format!("{}/settle", facilitator_url);
    let response = http_client
        .post(&url)
        .json(settle_request)
        .send()
        .await
        .map_err(|e| format!("Failed to contact facilitator for settlement: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Settlement error: {} - {}", status, body));
    }

    response
        .json::<SettleResponse>()
        .await
        .map_err(|e| format!("Failed to parse settle response: {}", e))
}

// ── Public API ──

/// Check X-PAYMENT header, verify with facilitator, and settle.
/// Returns Ok(tx_hash) on success, Err(Response) if payment is missing/invalid.
pub async fn require_x402_payment(
    config: &Config,
    http_client: &reqwest::Client,
    headers: &HeaderMap,
    amount: DomainU256,
    resource: &str,
    description: &str,
) -> Result<Option<String>, Response> {
    let payment_header = headers.get("X-PAYMENT").and_then(|v| v.to_str().ok());

    match payment_header {
        None => Err(payment_required_response(config, amount, resource, description)),
        Some(payment) => {
            let requirements = build_payment_requirements(config, amount, resource, description);

            let payload_bytes = BASE64.decode(payment).map_err(|e| {
                error_response(
                    StatusCode::BAD_REQUEST,
                    &format!("Invalid payment encoding: {}", e),
                )
            })?;

            let payment_payload: serde_json::Value =
                serde_json::from_slice(&payload_bytes).map_err(|e| {
                    error_response(
                        StatusCode::BAD_REQUEST,
                        &format!("Invalid payment JSON: {}", e),
                    )
                })?;

            let verify_request = VerifyRequest {
                x402_version: 1,
                payment_payload,
                payment_requirements: requirements,
            };

            let verify_resp =
                verify_payment(http_client, &config.facilitator_url, &verify_request)
                    .await
                    .map_err(|e| {
                        tracing::error!("Verification error: {}", e);
                        error_response(StatusCode::BAD_GATEWAY, &e)
                    })?;

            if !verify_resp.is_valid {
                let reason = verify_resp.invalid_reason.unwrap_or_default();
                tracing::warn!("Payment invalid: {}", reason);
                return Err(error_response(
                    StatusCode::PAYMENT_REQUIRED,
                    &format!("Payment invalid: {}", reason),
                ));
            }

            // Settle synchronously
            let settle_resp =
                settle_payment(http_client, &config.facilitator_url, &verify_request)
                    .await
                    .map_err(|e| {
                        tracing::error!("Settlement error: {}", e);
                        error_response(StatusCode::BAD_GATEWAY, &e)
                    })?;

            if settle_resp.success {
                tracing::info!("Payment settled: {:?}", settle_resp.transaction);
                Ok(settle_resp.transaction)
            } else {
                let reason = settle_resp.error_reason.unwrap_or_default();
                tracing::error!("Settlement failed: {}", reason);
                Err(error_response(
                    StatusCode::PAYMENT_REQUIRED,
                    &format!("Settlement failed: {}", reason),
                ))
            }
        }
    }
}
