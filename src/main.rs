mod api;
mod checker;
mod config;
mod db;
mod models;
mod scheduler;
mod sources;

use std::sync::Arc;

use axum::Router;
use axum::response::Html;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::api::AppState;
use crate::config::AppConfig;
use crate::db::Database;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    info!("Starting Proxy Pulse v0.1.0");

    // Load configuration
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.yaml".to_string());

    let config = AppConfig::load(&config_path)?;
    info!(config = %config_path, "Configuration loaded");

    // Initialize database
    let db = Database::new(&config.database.url).await?;
    info!("Database initialized");

    // Create shared state
    let config = Arc::new(config);
    let state = Arc::new(AppState { db: db.clone(), config: config.clone() });

    // Start background schedulers
    scheduler::start_schedulers(db, config.clone()).await;
    info!("Background schedulers started");

    // Build application router
    let app = Router::new()
        // Admin page route
        .route("/admin", axum::routing::get(admin_page))
        // API routes
        .merge(api::api_router())
        // Serve static files (dashboard)
        .nest_service("/static", ServeDir::new("static"))
        // Serve index.html at root
        .fallback_service(ServeDir::new("static").append_index_html_on_directories(true))
        // Middleware
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any))
        .layer(TraceLayer::new_for_http())
        // Shared state
        .with_state(state);

    let addr = format!("{}:{}", config.server.host, config.server.port);
    info!(addr = %addr, "Starting HTTP server");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Serve admin.html page
async fn admin_page() -> Html<String> {
    match tokio::fs::read_to_string("static/admin.html").await {
        Ok(content) => Html(content),
        Err(_) => Html("<h1>Admin page not found</h1>".to_string()),
    }
}
