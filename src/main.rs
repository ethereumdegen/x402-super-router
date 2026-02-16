use std::sync::Arc;

use axum::{Router, response::Json, routing::get};
use serde::Serialize;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

mod config;
mod domain_types;
mod endpoints;
mod handler;
mod x402;

use config::Config;
use endpoints::EndpointDef;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub http_client: reqwest::Client,
    pub endpoints: Arc<Vec<EndpointDef>>,
}

#[derive(Serialize)]
struct EndpointInfo {
    path: String,
    description: String,
    cost: String,
    media_type: String,
}

#[derive(Serialize)]
struct InfoResponse {
    service: &'static str,
    version: &'static str,
    endpoints: Vec<EndpointInfo>,
    token: String,
    network: String,
}

async fn info(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Json<InfoResponse> {
    let endpoint_infos: Vec<EndpointInfo> = state
        .endpoints
        .iter()
        .map(|ep| EndpointInfo {
            path: ep.path.clone(),
            description: ep.description.clone(),
            cost: ep.cost.clone(),
            media_type: ep.media_type.clone(),
        })
        .collect();

    Json(InfoResponse {
        service: "x402-super-router",
        version: env!("CARGO_PKG_VERSION"),
        endpoints: endpoint_infos,
        token: state.config.payment_token_address.clone(),
        network: state.config.payment_network.clone(),
    })
}

async fn info_text(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> String {
    let mut out = String::new();
    out.push_str("x402-super-router\n");
    out.push_str(&format!("version: {}\n", env!("CARGO_PKG_VERSION")));
    out.push_str(&format!("network: {}\n", state.config.payment_network));
    out.push_str(&format!("token: {} ({})\n", state.config.payment_token_symbol, state.config.payment_token_address));
    out.push_str(&format!("wallet: {}\n", state.config.wallet_address));
    out.push_str("\n--- endpoints ---\n\n");
    for ep in state.endpoints.iter() {
        out.push_str(&format!("  GET {}\n", ep.path));
        out.push_str(&format!("    {}\n", ep.description));
        out.push_str(&format!("    model: {}\n", ep.fal_model));
        out.push_str(&format!("    type: {}\n", ep.media_type));
        out.push_str(&format!("    cost: {} (raw wei)\n", ep.cost));
        out.push_str(&format!("    query: ?prompt=<text>  (default: \"{}\")\n", ep.default_prompt));
        out.push_str("\n");
    }
    out.push_str("--- payment ---\n\n");
    out.push_str("  Send a GET request to any endpoint above.\n");
    out.push_str("  Without an X-PAYMENT header, you'll receive a 402 with payment requirements.\n");
    out.push_str("  Include a valid X-PAYMENT header (base64-encoded permit) to generate content.\n");
    out
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "x402_super_router=debug,tower_http=debug".into()),
        )
        .init();

    let config = Config::from_env();
    let port = config.port;

    let endpoints_config = endpoints::load_endpoints(&config.endpoints_config_path);

    // Validate all costs parse as DomainU256 at startup
    for ep in &endpoints_config.endpoints {
        domain_types::DomainU256::from_string(&ep.cost)
            .unwrap_or_else(|e| panic!("Bad cost '{}' for endpoint {}: {}", ep.cost, ep.path, e));
    }

    let endpoint_defs = Arc::new(endpoints_config.endpoints);

    tracing::info!("x402-super-router starting");
    tracing::info!("  Network: {}", config.payment_network);
    tracing::info!("  Wallet: {}", config.wallet_address);
    tracing::info!("  Facilitator: {}", config.facilitator_url);
    tracing::info!("  Public URL: {}", config.public_url);
    tracing::info!("  Endpoints loaded: {}", endpoint_defs.len());
    for ep in endpoint_defs.iter() {
        tracing::info!("    {} -> {} ({})", ep.path, ep.fal_model, ep.description);
    }

    let state = AppState {
        config,
        http_client: reqwest::Client::new(),
        endpoints: Arc::clone(&endpoint_defs),
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
        .expose_headers(Any);

    // Build router dynamically from endpoint config
    let mut app = Router::new()
        .route("/", get(info_text))
        .route("/api", get(info));

    for ep in endpoint_defs.iter() {
        // Create cache dir at startup
        std::fs::create_dir_all(&ep.cache_dir)
            .unwrap_or_else(|e| panic!("Failed to create cache dir '{}': {}", ep.cache_dir, e));

        let ep_arc = Arc::new(ep.clone());
        app = app
            .route(&ep.path, handler::make_endpoint_route(ep_arc))
            .nest_service(&ep.static_serve_path, ServeDir::new(&ep.cache_dir));
    }

    let app = app
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
