use axum::{
    Router,
    extract::{Query, State},
    http::HeaderMap,
    response::Json,
    routing::get,
};
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

mod config;
mod domain_types;
mod fal;
mod x402;

use config::Config;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub http_client: reqwest::Client,
}

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

#[derive(Serialize)]
struct EndpointInfo {
    path: &'static str,
    description: &'static str,
    cost: String,
}

#[derive(Serialize)]
struct InfoResponse {
    service: &'static str,
    version: &'static str,
    endpoints: Vec<EndpointInfo>,
    token: &'static str,
    network: &'static str,
}

async fn generate_image(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<PromptQuery>,
) -> Result<Json<GenerateResponse>, axum::response::Response> {
    x402::require_x402_payment(
        &state.config,
        &state.http_client,
        &headers,
        state.config.cost_per_image,
        "/generate_image",
        "Generate an AI image (1000 STARKBOT)",
    )
    .await?;

    let prompt_str = query.prompt.as_deref();
    let effective = prompt_str.unwrap_or("a fun colorful surreal meme illustration");

    let was_cached = {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(effective.trim().to_lowercase().as_bytes());
        let hash = hex::encode(h.finalize());
        std::path::Path::new("public/images")
            .join(format!("{}.png", hash))
            .exists()
    };

    let filename = fal::generate_image(&state.config, &state.http_client, prompt_str)
        .await
        .map_err(|e| {
            tracing::error!("Image generation failed: {}", e);
            axum::response::Response::builder()
                .status(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
                .header("content-type", "text/plain")
                .body(axum::body::Body::from(format!(
                    "Image generation failed: {}",
                    e
                )))
                .unwrap()
        })?;

    let url = fal::image_url(&state.config, &filename);

    Ok(Json(GenerateResponse {
        url,
        prompt: effective.to_string(),
        cached: was_cached,
        media_type: "image".to_string(),
    }))
}

async fn generate_gif(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<PromptQuery>,
) -> Result<Json<GenerateResponse>, axum::response::Response> {
    x402::require_x402_payment(
        &state.config,
        &state.http_client,
        &headers,
        state.config.cost_per_gif,
        "/generate_gif",
        "Generate an animated GIF (1000 STARKBOT)",
    )
    .await?;

    let prompt_str = query.prompt.as_deref();
    let effective = prompt_str.unwrap_or("a fun random weird surreal animated meme");

    let was_cached = {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(effective.trim().to_lowercase().as_bytes());
        let hash = hex::encode(h.finalize());
        std::path::Path::new("public/gifs")
            .join(format!("{}.gif", hash))
            .exists()
    };

    let filename = fal::generate_gif(&state.config, &state.http_client, prompt_str)
        .await
        .map_err(|e| {
            tracing::error!("GIF generation failed: {}", e);
            axum::response::Response::builder()
                .status(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
                .header("content-type", "text/plain")
                .body(axum::body::Body::from(format!(
                    "GIF generation failed: {}",
                    e
                )))
                .unwrap()
        })?;

    let url = fal::gif_url(&state.config, &filename);

    Ok(Json(GenerateResponse {
        url,
        prompt: effective.to_string(),
        cached: was_cached,
        media_type: "gif".to_string(),
    }))
}

async fn info() -> Json<InfoResponse> {
    Json(InfoResponse {
        service: "x402-gif-machine",
        version: env!("CARGO_PKG_VERSION"),
        endpoints: vec![
            EndpointInfo {
                path: "/generate_image",
                description: "Generate an AI image from a text prompt",
                cost: "1000 STARKBOT".to_string(),
            },
            EndpointInfo {
                path: "/generate_gif",
                description: "Generate an animated GIF from a text prompt",
                cost: "1000 STARKBOT".to_string(),
            },
        ],
        token: "0x587Cd533F418825521f3A1daa7CCd1E7339A1B07",
        network: "base",
    })
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "x402_gif_machine=debug,tower_http=debug".into()),
        )
        .init();

    let config = Config::from_env();
    let port = config.port;

    tracing::info!("x402-gif-machine starting");
    tracing::info!(
        "  Image cost: {} {}",
        config.cost_per_image,
        config.payment_token_symbol
    );
    tracing::info!(
        "  GIF cost: {} {}",
        config.cost_per_gif,
        config.payment_token_symbol
    );
    tracing::info!("  Network: {}", config.payment_network);
    tracing::info!("  Wallet: {}", config.wallet_address);
    tracing::info!("  Facilitator: {}", config.facilitator_url);
    tracing::info!("  Public URL: {}", config.public_url);

    let state = AppState {
        config,
        http_client: reqwest::Client::new(),
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
        .expose_headers(Any);

    let app = Router::new()
        .route("/generate_image", get(generate_image))
        .route("/generate_gif", get(generate_gif))
        .route("/", get(info))
        .nest_service("/images", ServeDir::new("public/images"))
        .nest_service("/gifs", ServeDir::new("public/gifs"))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    tracing::info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind");

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
            tracing::info!("Shutting down");
        })
        .await
        .expect("Server failed");
}
