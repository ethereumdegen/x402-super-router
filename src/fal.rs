use sha2::{Digest, Sha256};
use std::path::Path;

use crate::config::Config;

const IMAGE_DIR: &str = "public/images";
const GIF_DIR: &str = "public/gifs";

fn prompt_hash(prompt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prompt.trim().to_lowercase().as_bytes());
    hex::encode(hasher.finalize())
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

/// Generate a static image via FAL flux/schnell, save to cache, return filename
pub async fn generate_image(
    config: &Config,
    http_client: &reqwest::Client,
    prompt: Option<&str>,
) -> Result<String, String> {
    let effective_prompt = prompt.unwrap_or("a fun colorful surreal meme illustration");
    let hash = prompt_hash(effective_prompt);
    let filename = format!("{}.png", hash);

    // Check cache
    if Path::new(IMAGE_DIR).join(&filename).exists() {
        tracing::info!("Cache hit for image prompt: {}", effective_prompt);
        return Ok(filename);
    }

    tracing::info!("Generating image for: {}", effective_prompt);

    let request_body = serde_json::json!({
        "prompt": effective_prompt,
        "num_inference_steps": 4,
        "image_size": "square",
        "num_images": 1,
        "output_format": "png",
        "enable_safety_checker": true
    });

    let response = http_client
        .post("https://fal.run/fal-ai/flux/schnell")
        .header("Authorization", format!("Key {}", config.fal_key))
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("FAL request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("FAL error {}: {}", status, body));
    }

    let resp_json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse FAL response: {}", e))?;

    let image_url = resp_json["images"][0]["url"]
        .as_str()
        .ok_or_else(|| {
            format!(
                "No image URL in FAL response: {}",
                serde_json::to_string_pretty(&resp_json).unwrap_or_default()
            )
        })?;

    let image_bytes = download_url(http_client, image_url).await?;

    std::fs::create_dir_all(IMAGE_DIR)
        .map_err(|e| format!("Failed to create image dir: {}", e))?;

    let image_path = Path::new(IMAGE_DIR).join(&filename);
    std::fs::write(&image_path, &image_bytes)
        .map_err(|e| format!("Failed to save image: {}", e))?;

    tracing::info!("Saved image to {}", image_path.display());
    Ok(filename)
}

/// Generate an animated GIF via FAL animatediff, save to cache, return filename
pub async fn generate_gif(
    config: &Config,
    http_client: &reqwest::Client,
    prompt: Option<&str>,
) -> Result<String, String> {
    let effective_prompt = prompt.unwrap_or("a fun random weird surreal animated meme");
    let hash = prompt_hash(effective_prompt);
    let filename = format!("{}.gif", hash);

    // Check cache
    if Path::new(GIF_DIR).join(&filename).exists() {
        tracing::info!("Cache hit for gif prompt: {}", effective_prompt);
        return Ok(filename);
    }

    tracing::info!("Generating GIF for: {}", effective_prompt);

    let request_body = serde_json::json!({
        "prompt": effective_prompt,
        "negative_prompt": "(bad quality, worst quality:1.2), ugly, blurry",
        "num_frames": 16,
        "num_inference_steps": 8,
        "guidance_scale": 2.0,
        "fps": 8,
        "video_size": "square"
    });

    let response = http_client
        .post("https://fal.run/fal-ai/fast-animatediff/turbo/text-to-video")
        .header("Authorization", format!("Key {}", config.fal_key))
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("FAL request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("FAL error {}: {}", status, body));
    }

    let resp_json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse FAL response: {}", e))?;

    let video_url = resp_json["video"]["url"]
        .as_str()
        .ok_or_else(|| {
            format!(
                "No video URL in FAL response: {}",
                serde_json::to_string_pretty(&resp_json).unwrap_or_default()
            )
        })?;

    // Download the MP4 from FAL CDN
    let video_bytes = download_url(http_client, video_url).await?;

    std::fs::create_dir_all(GIF_DIR)
        .map_err(|e| format!("Failed to create gif dir: {}", e))?;

    // Save MP4 temporarily, convert to GIF with ffmpeg
    let tmp_mp4 = Path::new(GIF_DIR).join(format!("{}.mp4", hash));
    let gif_path = Path::new(GIF_DIR).join(&filename);

    std::fs::write(&tmp_mp4, &video_bytes)
        .map_err(|e| format!("Failed to save temp MP4: {}", e))?;

    let output = tokio::process::Command::new("ffmpeg")
        .args([
            "-i",
            tmp_mp4.to_str().unwrap(),
            "-vf",
            "fps=10,scale=512:-1:flags=lanczos,split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse",
            "-loop",
            "0",
            "-y",
            gif_path.to_str().unwrap(),
        ])
        .output()
        .await
        .map_err(|e| format!("ffmpeg failed to execute (is it installed?): {}", e))?;

    // Clean up temp MP4
    let _ = std::fs::remove_file(&tmp_mp4);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ffmpeg conversion failed: {}", stderr));
    }

    tracing::info!("Saved GIF to {}", gif_path.display());
    Ok(filename)
}

pub fn image_url(config: &Config, filename: &str) -> String {
    format!(
        "{}/images/{}",
        config.public_url.trim_end_matches('/'),
        filename
    )
}

pub fn gif_url(config: &Config, filename: &str) -> String {
    format!(
        "{}/gifs/{}",
        config.public_url.trim_end_matches('/'),
        filename
    )
}
