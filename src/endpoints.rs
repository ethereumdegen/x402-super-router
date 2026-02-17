use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct EndpointsConfig {
    pub endpoints: Vec<EndpointDef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EndpointDef {
    pub route: String,
    pub quality: String,
    pub path: String,
    pub fal_model: String,
    pub cost: String,
    pub description: String,
    pub response_url_path: String,
    pub request_params: HashMap<String, serde_json::Value>,
    pub default_prompt: String,
    pub media_type: String,
    pub output_extension: String,
    pub post_process: PostProcess,
}

#[derive(Debug, Clone, Deserialize)]
pub enum PostProcess {
    None,
    FfmpegToGif {
        input_extension: String,
        ffmpeg_args: Vec<String>,
    },
}

/// Maps quality level (e.g. "low", "medium", "high") to an EndpointDef.
pub type QualityMap = HashMap<String, EndpointDef>;

/// Group flat endpoint list by route, producing a map of route -> quality -> EndpointDef.
pub fn group_by_route(endpoints: &[EndpointDef]) -> HashMap<String, QualityMap> {
    let mut grouped: HashMap<String, QualityMap> = HashMap::new();
    for ep in endpoints {
        grouped
            .entry(ep.route.clone())
            .or_default()
            .insert(ep.quality.clone(), ep.clone());
    }
    grouped
}

pub fn load_endpoints(path: &str) -> EndpointsConfig {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("Failed to read endpoints config '{}': {}", path, e));
    ron::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse endpoints config '{}': {}", path, e))
}

/// Extract a value from nested JSON using a dot-separated path.
/// Numeric segments index into arrays, string segments index into objects.
/// e.g. "images.0.url" or "video.url"
pub fn extract_url(json: &serde_json::Value, dot_path: &str) -> Result<String, String> {
    let mut current = json;
    for segment in dot_path.split('.') {
        if let Ok(idx) = segment.parse::<usize>() {
            current = current
                .get(idx)
                .ok_or_else(|| format!("Array index {} not found in path '{}'", idx, dot_path))?;
        } else {
            current = current
                .get(segment)
                .ok_or_else(|| format!("Key '{}' not found in path '{}'", segment, dot_path))?;
        }
    }
    current
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| format!("Value at path '{}' is not a string", dot_path))
}
