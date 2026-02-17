use std::path::Path;

use actix_web::{web, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::AppState;
use crate::db;
use crate::domain_types::DomainU256;
use crate::endpoints::{EndpointDef, PostProcess, QualityMap, extract_url};
use crate::s3;
use crate::x402;

#[derive(Deserialize)]
pub struct PromptQuery {
    pub prompt: Option<String>,
    #[serde(default = "default_quality")]
    pub quality: String,
}

fn default_quality() -> String {
    "low".to_string()
}

#[derive(Serialize)]
struct GenerateResponse {
    url: String,
    prompt: String,
    cached: bool,
    #[serde(rename = "type")]
    media_type: String,
    quality: String,
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

pub async fn handle_generate(
    req: HttpRequest,
    state: web::Data<AppState>,
    quality_map: web::Data<QualityMap>,
    query: web::Query<PromptQuery>,
) -> HttpResponse {
    let quality = &query.quality;
    let endpoint = match quality_map.get(quality.as_str()) {
        Some(ep) => ep,
        None => {
            let valid: Vec<&String> = quality_map.keys().collect();
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": format!("Invalid quality '{}'. Valid options: {:?}", quality, valid)
            }));
        }
    };

    match handle_endpoint_inner(&state, &req, query.prompt.as_deref(), endpoint, quality).await {
        Ok(resp) => resp,
        Err(resp) => resp,
    }
}

async fn handle_endpoint_inner(
    state: &AppState,
    req: &HttpRequest,
    prompt: Option<&str>,
    endpoint: &EndpointDef,
    quality: &str,
) -> Result<HttpResponse, HttpResponse> {
    let cost = DomainU256::from_human_amount(&endpoint.cost, state.config.payment_token_decimals)
        .map_err(|e| {
            tracing::error!("Bad cost in endpoint {}: {}", endpoint.path, e);
            HttpResponse::InternalServerError().body(format!("Internal config error: {}", e))
        })?;

    let (payment_tx, payer_address) = x402::require_x402_payment(
        &state.config,
        &state.http_client,
        req.headers(),
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

    // Cache check: query DB instead of filesystem
    if let Ok(Some(record)) = db::find_by_prompt_hash(&state.db_pool, &hash, &endpoint.path).await
    {
        tracing::info!(
            "[{}] Cache hit for prompt: {}",
            endpoint.path,
            effective
        );
        return Ok(HttpResponse::Ok().json(GenerateResponse {
            url: record.s3_url,
            prompt: effective.to_string(),
            cached: true,
            media_type: endpoint.media_type.clone(),
            quality: quality.to_string(),
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
            HttpResponse::InternalServerError().body(format!("FAL request failed: {}", e))
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        tracing::error!("[{}] FAL error {}: {}", endpoint.path, status, body);
        return Err(HttpResponse::InternalServerError().body(format!("FAL error {}: {}", status, body)));
    }

    let resp_json: serde_json::Value = response.json().await.map_err(|e| {
        tracing::error!("[{}] Failed to parse FAL response: {}", endpoint.path, e);
        HttpResponse::InternalServerError().body(format!("Failed to parse FAL response: {}", e))
    })?;

    let result_url = extract_url(&resp_json, &endpoint.response_url_path).map_err(|e| {
        tracing::error!(
            "[{}] Failed to extract URL from FAL response: {}. Response: {}",
            endpoint.path,
            e,
            serde_json::to_string_pretty(&resp_json).unwrap_or_default()
        );
        HttpResponse::InternalServerError().body(format!("No result URL in FAL response: {}", e))
    })?;

    let result_bytes = download_url(&state.http_client, &result_url)
        .await
        .map_err(|e| {
            tracing::error!("[{}] Download failed: {}", endpoint.path, e);
            HttpResponse::InternalServerError().body(e)
        })?;

    // Post-process if needed (ffmpeg uses tmp/ dir for temp files)
    let final_bytes = match &endpoint.post_process {
        PostProcess::None => result_bytes,
        PostProcess::FfmpegToGif {
            input_extension,
            ffmpeg_args,
        } => {
            std::fs::create_dir_all("tmp").map_err(|e| {
                HttpResponse::InternalServerError().body(format!("Failed to create tmp dir: {}", e))
            })?;

            let tmp_input = Path::new("tmp").join(format!("{}.{}", hash, input_extension));
            let tmp_output = Path::new("tmp").join(format!("{}.{}", hash, endpoint.output_extension));

            std::fs::write(&tmp_input, &result_bytes).map_err(|e| {
                tracing::error!("[{}] Failed to save temp file: {}", endpoint.path, e);
                HttpResponse::InternalServerError().body(format!("Failed to save temp file: {}", e))
            })?;

            let mut cmd_args = vec![
                "-i".to_string(),
                tmp_input.to_string_lossy().to_string(),
            ];
            cmd_args.extend(ffmpeg_args.iter().cloned());
            cmd_args.push("-y".to_string());
            cmd_args.push(tmp_output.to_string_lossy().to_string());

            let output = tokio::process::Command::new("ffmpeg")
                .args(&cmd_args)
                .output()
                .await
                .map_err(|e| {
                    tracing::error!("[{}] ffmpeg failed: {}", endpoint.path, e);
                    HttpResponse::InternalServerError().body(format!(
                        "ffmpeg failed to execute (is it installed?): {}",
                        e
                    ))
                })?;

            let _ = std::fs::remove_file(&tmp_input);

            if !output.status.success() {
                let _ = std::fs::remove_file(&tmp_output);
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::error!("[{}] ffmpeg conversion failed: {}", endpoint.path, stderr);
                return Err(HttpResponse::InternalServerError().body(format!(
                    "ffmpeg conversion failed: {}",
                    stderr
                )));
            }

            let converted = std::fs::read(&tmp_output).map_err(|e| {
                HttpResponse::InternalServerError().body(format!("Failed to read ffmpeg output: {}", e))
            })?;
            let _ = std::fs::remove_file(&tmp_output);
            converted
        }
    };

    // S3 key: endpoint_path_without_leading_slash/hash.ext
    let path_segment = endpoint.path.trim_start_matches('/');
    let s3_key = format!("{}/{}.{}", path_segment, hash, endpoint.output_extension);

    let content_type = match endpoint.output_extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "mp4" => "video/mp4",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    };

    let file_size = final_bytes.len() as i64;

    // Upload to S3
    s3::upload_file(
        &state.s3_client,
        &state.config.s3_bucket,
        &s3_key,
        final_bytes,
        content_type,
    )
    .await
    .map_err(|e| {
        tracing::error!("[{}] S3 upload failed: {}", endpoint.path, e);
        HttpResponse::InternalServerError().body(e)
    })?;

    let cdn_url = s3::cdn_url(&state.config, &s3_key);

    tracing::info!("[{}] Uploaded to S3: {}", endpoint.path, cdn_url);

    // Insert DB record
    if let Err(e) = db::insert_media(
        &state.db_pool,
        &endpoint.path,
        effective,
        &hash,
        &s3_key,
        &cdn_url,
        &endpoint.media_type,
        file_size,
        payer_address.as_deref(),
        payment_tx.as_deref(),
    )
    .await
    {
        tracing::error!("[{}] Failed to insert DB record: {}", endpoint.path, e);
    }

    Ok(HttpResponse::Ok().json(GenerateResponse {
        url: cdn_url,
        prompt: effective.to_string(),
        cached: false,
        media_type: endpoint.media_type.clone(),
        quality: quality.to_string(),
    }))
}
