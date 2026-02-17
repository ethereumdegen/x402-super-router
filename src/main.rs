use std::sync::Arc;

use actix_cors::Cors;
use actix_governor::{Governor, GovernorConfigBuilder};
use actix_web::{web, App, HttpResponse, HttpServer, middleware};
use serde::Serialize;

mod cleanup;
mod config;
mod db;
mod domain_types;
mod endpoints;
mod handler;
mod s3;
mod x402;

use config::Config;
use endpoints::EndpointDef;

pub struct AppState {
    pub config: Config,
    pub http_client: reqwest::Client,
    pub endpoints: Arc<Vec<EndpointDef>>,
    pub db_pool: sqlx::PgPool,
    pub s3_client: aws_sdk_s3::Client,
}

#[derive(Serialize)]
struct QualityInfo {
    quality: String,
    model: String,
    cost: String,
    cost_raw: String,
    description: String,
}

#[derive(Serialize)]
struct RouteInfo {
    method: &'static str,
    route: String,
    media_type: String,
    default_quality: &'static str,
    qualities: Vec<QualityInfo>,
}

#[derive(Serialize)]
struct InfoResponse {
    service: &'static str,
    version: &'static str,
    routes: Vec<RouteInfo>,
    token: String,
    network: String,
}

async fn info(state: web::Data<AppState>) -> HttpResponse {
    let grouped = endpoints::group_by_route(&state.endpoints);
    let mut routes: Vec<RouteInfo> = Vec::new();

    let mut route_keys: Vec<String> = grouped.keys().cloned().collect();
    route_keys.sort();

    for route in &route_keys {
        let quality_map = &grouped[route];
        let sample = quality_map.values().next().unwrap();

        let mut qualities: Vec<QualityInfo> = Vec::new();
        let mut quality_keys: Vec<String> = quality_map.keys().cloned().collect();
        quality_keys.sort();

        for q in &quality_keys {
            let ep = &quality_map[q];
            let raw_cost = domain_types::DomainU256::from_human_amount(
                &ep.cost,
                state.config.payment_token_decimals,
            )
            .expect("cost validated at startup");
            qualities.push(QualityInfo {
                quality: q.clone(),
                model: ep.fal_model.clone(),
                cost: ep.cost.clone(),
                cost_raw: raw_cost.to_string(),
                description: ep.description.clone(),
            });
        }

        routes.push(RouteInfo {
            method: "POST",
            route: route.clone(),
            media_type: sample.media_type.clone(),
            default_quality: "low",
            qualities,
        });
    }

    HttpResponse::Ok().json(InfoResponse {
        service: "x402-super-router",
        version: env!("CARGO_PKG_VERSION"),
        routes,
        token: state.config.payment_token_address.clone(),
        network: state.config.payment_network.clone(),
    })
}

async fn info_text(state: web::Data<AppState>) -> HttpResponse {
    let mut out = String::new();
    out.push_str("x402-super-router\n");
    out.push_str(&format!("version: {}\n", env!("CARGO_PKG_VERSION")));
    out.push_str(&format!("network: {}\n", state.config.payment_network));
    out.push_str(&format!("token: {} ({})\n", state.config.payment_token_symbol, state.config.payment_token_address));
    out.push_str(&format!("wallet: {}\n", state.config.wallet_address));
    out.push_str("\n--- routes ---\n");

    let grouped = endpoints::group_by_route(&state.endpoints);
    let mut route_keys: Vec<String> = grouped.keys().cloned().collect();
    route_keys.sort();

    for route in &route_keys {
        let quality_map = &grouped[route];
        out.push_str(&format!("\n  POST {}  {{\"quality\": \"low|medium|high\", \"prompt\": \"<text>\"}}\n", route));

        let mut quality_keys: Vec<String> = quality_map.keys().cloned().collect();
        quality_keys.sort();

        for q in &quality_keys {
            let ep = &quality_map[q];
            let raw_cost = domain_types::DomainU256::from_human_amount(
                &ep.cost,
                state.config.payment_token_decimals,
            )
            .expect("cost validated at startup");
            out.push_str(&format!(
                "    {} : {} {} (model: {}, raw: {})\n",
                q, ep.cost, state.config.payment_token_symbol, ep.fal_model, raw_cost
            ));
        }
    }

    out.push_str("\n--- payment ---\n\n");
    out.push_str("  Send a POST request with JSON body to any route above.\n");
    out.push_str("  Without an X-PAYMENT header, you'll receive a 402 with payment requirements.\n");
    out.push_str("  Include a valid X-PAYMENT header (base64-encoded permit) to generate content.\n");
    HttpResponse::Ok()
        .content_type("text/plain")
        .body(out)
}

async fn health() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({"status": "ok"}))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "x402_super_router=debug,actix_web=info".into()),
        )
        .init();

    let config = Config::from_env();
    let port = config.port;

    let endpoints_config = endpoints::load_endpoints(&config.endpoints_config_path);

    // Validate all costs parse at startup
    for ep in &endpoints_config.endpoints {
        let raw = domain_types::DomainU256::from_human_amount(&ep.cost, config.payment_token_decimals)
            .unwrap_or_else(|e| panic!("Bad cost '{}' for endpoint {}: {}", ep.cost, ep.path, e));
        tracing::info!(
            "  {} [{}] cost: {} {} (raw: {})",
            ep.route,
            ep.quality,
            ep.cost,
            config.payment_token_symbol,
            raw
        );
    }

    // Group endpoints by route and validate each route has a "low" variant (the default)
    let grouped = endpoints::group_by_route(&endpoints_config.endpoints);
    for (route, quality_map) in &grouped {
        if !quality_map.contains_key("low") {
            panic!("Route '{}' is missing a 'low' quality variant (required as default)", route);
        }
    }

    let endpoint_defs = Arc::new(endpoints_config.endpoints);

    // Initialize DB pool
    tracing::info!("Connecting to database...");
    let db_pool = db::create_pool(&config.database_url).await;
    tracing::info!("Database connected");

    // Initialize S3 client
    let s3_client = s3::create_s3_client(&config);
    tracing::info!("S3 client initialized (endpoint: {})", config.s3_endpoint);

    // Create tmp dir for ffmpeg temp files
    std::fs::create_dir_all("tmp").expect("Failed to create tmp/ directory");

    tracing::info!("x402-super-router starting");
    if config.test_mode {
        tracing::warn!("  *** TEST_MODE ENABLED — all x402 payments are bypassed ***");
    }
    tracing::info!("  Network: {}", config.payment_network);
    tracing::info!("  Wallet: {}", config.wallet_address);
    tracing::info!("  Facilitator: {}", config.facilitator_url);
    tracing::info!("  S3 Bucket: {}", config.s3_bucket);
    tracing::info!("  S3 CDN: {}", config.s3_cdn_url);
    tracing::info!("  Routes: {}", grouped.len());
    for (route, quality_map) in &grouped {
        let qualities: Vec<&String> = quality_map.keys().collect();
        tracing::info!("    {} -> qualities: {:?}", route, qualities);
        for (q, ep) in quality_map {
            tracing::info!("      {} : {} ({})", q, ep.fal_model, ep.description);
        }
    }

    // Spawn cleanup worker with broadcast shutdown channel
    let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel::<()>(1);
    tokio::spawn(cleanup::run_cleanup_worker(
        db_pool.clone(),
        s3_client.clone(),
        config.s3_bucket.clone(),
        shutdown_rx,
    ));
    tracing::info!("Cleanup worker spawned");

    // Rate limiting: 10 requests per minute per IP on generation endpoints
    let governor_conf = GovernorConfigBuilder::default()
        .seconds_per_request(6)
        .burst_size(10)
        .finish()
        .unwrap();

    let state = web::Data::new(AppState {
        config,
        http_client: reqwest::Client::new(),
        endpoints: Arc::clone(&endpoint_defs),
        db_pool,
        s3_client,
    });

    // Build the grouped quality maps for route registration
    let grouped_for_factory = Arc::new(grouped);

    tracing::info!("Listening on 0.0.0.0:{}", port);

    HttpServer::new(move || {
        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header()
            .expose_any_header();

        let mut app = App::new()
            .app_data(state.clone())
            .wrap(cors)
            .wrap(middleware::Logger::default())
            .route("/", web::get().to(info_text))
            .route("/api", web::get().to(info))
            .route("/api/health", web::get().to(health));

        // Register one route per group, injecting the QualityMap as app_data
        for (route, quality_map) in grouped_for_factory.as_ref() {
            let qm_data = web::Data::new(quality_map.clone());
            app = app.service(
                web::resource(route)
                    .app_data(qm_data)
                    .wrap(Governor::new(&governor_conf))
                    .route(web::post().to(handler::handle_generate)),
            );
        }

        app
    })
    .bind(format!("0.0.0.0:{}", port))?
    .run()
    .await?;

    // Server stopped — signal cleanup worker
    tracing::info!("Shutting down");
    let _ = shutdown_tx.send(());

    Ok(())
}
