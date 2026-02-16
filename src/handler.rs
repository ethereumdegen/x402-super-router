use std::path::Path;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::Json;
use axum::routing::{MethodRouter, get};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::AppState;
use crate::domain_types::DomainU256;
use crate::endpoints::{EndpointDef, PostProcess, extract_url};
use crate::x402;

#[derive(Deserialize)]
struct PromptQuery {
    prompt: Option<String>,
}

#[derive(Serialize)]
struct GenerateResponse {
    url: String,
    prompt: String,
    cached: bool,
    #[serde(rename = "type")]
    media_type: String,
}

async fn download_url(http_client: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let resp = http_client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Failed to download {}: {}", url, e))?;
    if !resp.status().is_success() {
        return Err(format!("Download failed with status {}", resp.status()));
    }
    resp.bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| format!("Failed to read download bytes: {}", e))
}

fn error_response(msg: String) -> axum::response::Response {
    axum::response::Response::builder()
        .status(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
        .header("content-type", "text/plain")
        .body(axum::body::Body::from(msg))
        .unwrap()
}

async fn handle_endpoint(
    state: &AppState,
    headers: &HeaderMap,
    prompt: Option<&str>,
    endpoint: &EndpointDef,
) -> Result<Json<GenerateResponse>, axum::response::Response> {
    let cost = DomainU256::from_string(&endpoint.cost).map_err(|e| {
        tracing::error!("Bad cost in endpoint {}: {}", endpoint.path, e);
        error_response(format!("Internal config error: {}", e))
    })?;

    x402::require_x402_payment(
        &state.config,
        &state.http_client,
        headers,
        cost,
        &endpoint.path,
        &endpoint.description,
    )
    .await?;

    let effective = prompt.unwrap_or(&endpoint.default_prompt);

    let hash = {
        let mut h = Sha256::new();
        h.update(effective.trim().to_lowercase().as_bytes());
        hex::encode(h.finalize())
    };

    let filename = format!("{}.{}", hash, endpoint.output_extension);
    let cache_path = Path::new(&endpoint.cache_dir).join(&filename);

    let was_cached = cache_path.exists();
    if was_cached {
        tracing::info!(
            "[{}] Cache hit for prompt: {}",
            endpoint.path,
            effective
        );
        let url = public_url(&state.config, &endpoint.static_serve_path, &filename);
        return Ok(Json(GenerateResponse {
            url,
            prompt: effective.to_string(),
            cached: true,
            media_type: endpoint.media_type.clone(),
        }));
    }

    tracing::info!("[{}] Generating for: {}", endpoint.path, effective);

    // Build fal request body: merge request_params + prompt
    let mut body_map = serde_json::Map::new();
    for (k, v) in &endpoint.request_params {
        body_map.insert(k.clone(), v.clone());
    }
    body_map.insert(
        "prompt".to_string(),
        serde_json::Value::String(effective.to_string()),
    );
    let request_body = serde_json::Value::Object(body_map);

    let fal_url = format!("https://fal.run/{}", endpoint.fal_model);
    let response = state
        .http_client
        .post(&fal_url)
        .header(
            "Authorization",
            format!("Key {}", state.config.fal_key),
        )
        .json(&request_body)
        .send()
        .await
        .map_err(|e| {
            tracing::error!("[{}] FAL request failed: {}", endpoint.path, e);
            error_response(format!("FAL request failed: {}", e))
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        tracing::error!("[{}] FAL error {}: {}", endpoint.path, status, body);
        return Err(error_response(format!("FAL error {}: {}", status, body)));
    }

    let resp_json: serde_json::Value = response.json().await.map_err(|e| {
        tracing::error!("[{}] Failed to parse FAL response: {}", endpoint.path, e);
        error_response(format!("Failed to parse FAL response: {}", e))
    })?;

    let result_url = extract_url(&resp_json, &endpoint.response_url_path).map_err(|e| {
        tracing::error!(
            "[{}] Failed to extract URL from FAL response: {}. Response: {}",
            endpoint.path,
            e,
            serde_json::to_string_pretty(&resp_json).unwrap_or_default()
        );
        error_response(format!("No result URL in FAL response: {}", e))
    })?;

    let result_bytes = download_url(&state.http_client, &result_url)
        .await
        .map_err(|e| {
            tracing::error!("[{}] Download failed: {}", endpoint.path, e);
            error_response(e)
        })?;

    std::fs::create_dir_all(&endpoint.cache_dir).map_err(|e| {
        tracing::error!("[{}] Failed to create cache dir: {}", endpoint.path, e);
        error_response(format!("Failed to create cache dir: {}", e))
    })?;

    // Post-process or save directly
    match &endpoint.post_process {
        PostProcess::None => {
            std::fs::write(&cache_path, &result_bytes).map_err(|e| {
                tracing::error!("[{}] Failed to save file: {}", endpoint.path, e);
                error_response(format!("Failed to save file: {}", e))
            })?;
        }
        PostProcess::FfmpegToGif {
            input_extension,
            ffmpeg_args,
        } => {
            let tmp_path =
                Path::new(&endpoint.cache_dir).join(format!("{}.{}", hash, input_extension));

            std::fs::write(&tmp_path, &result_bytes).map_err(|e| {
                tracing::error!("[{}] Failed to save temp file: {}", endpoint.path, e);
                error_response(format!("Failed to save temp file: {}", e))
            })?;

            let mut cmd_args = vec![
                "-i".to_string(),
                tmp_path.to_string_lossy().to_string(),
            ];
            cmd_args.extend(ffmpeg_args.iter().cloned());
            cmd_args.push("-y".to_string());
            cmd_args.push(cache_path.to_string_lossy().to_string());

            let output = tokio::process::Command::new("ffmpeg")
                .args(&cmd_args)
                .output()
                .await
                .map_err(|e| {
                    tracing::error!("[{}] ffmpeg failed: {}", endpoint.path, e);
                    error_response(format!(
                        "ffmpeg failed to execute (is it installed?): {}",
                        e
                    ))
                })?;

            let _ = std::fs::remove_file(&tmp_path);

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::error!("[{}] ffmpeg conversion failed: {}", endpoint.path, stderr);
                return Err(error_response(format!(
                    "ffmpeg conversion failed: {}",
                    stderr
                )));
            }
        }
    }

    tracing::info!("[{}] Saved to {}", endpoint.path, cache_path.display());

    let url = public_url(&state.config, &endpoint.static_serve_path, &filename);
    Ok(Json(GenerateResponse {
        url,
        prompt: effective.to_string(),
        cached: false,
        media_type: endpoint.media_type.clone(),
    }))
}

fn public_url(config: &crate::config::Config, serve_path: &str, filename: &str) -> String {
    format!(
        "{}{}/{}",
        config.public_url.trim_end_matches('/'),
        serve_path,
        filename
    )
}

/// Create an Axum route handler for a given endpoint definition.
pub fn make_endpoint_route(endpoint: Arc<EndpointDef>) -> MethodRouter<AppState> {
    get(move |State(state): State<AppState>,
              headers: HeaderMap,
              Query(query): Query<PromptQuery>| {
        let ep = Arc::clone(&endpoint);
        async move { handle_endpoint(&state, &headers, query.prompt.as_deref(), &ep).await }
    })
}
