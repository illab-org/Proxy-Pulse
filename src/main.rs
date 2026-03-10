mod api;
mod auth;
mod checker;
mod config;
mod db;
mod models;
mod mem_monitor;
mod scheduler;
mod sources;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

use std::sync::Arc;

use axum::{middleware, Router};
use axum::http::header;
use axum::response::Html;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
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

    // Check for --demo flag
    let args: Vec<String> = std::env::args().collect();
    let demo_mode = args.iter().any(|a| a == "--demo");
    if demo_mode {
        info!("🔒 DEMO MODE enabled — all write/mutation API endpoints will return 403");
    }

    // Load configuration (skip --demo when looking for config path)
    let config_path = args.iter()
        .skip(1)
        .find(|a| *a != "--demo")
        .cloned()
        .unwrap_or_else(|| "config.yaml".to_string());

    let config = AppConfig::load(&config_path)?;
    info!(config = %config_path, "Configuration loaded");

    // Initialize database
    let db = Database::new(&config.database.url).await?;
    info!("Database initialized");

    // Create shared state
    let config = Arc::new(config);
    let state = Arc::new(AppState { db: db.clone(), config: config.clone(), demo_mode });

    // Start background schedulers
    scheduler::start_schedulers(db, config.clone()).await;
    info!("Background schedulers started");

    // Start memory monitor (logs every 1 second)
    mem_monitor::spawn_monitor(1);
    info!("Memory monitor started (1s interval)");

    // Build application router
    //
    // Auth-free routes: /login, /api/v1/auth/status|setup|login|logout, /static/*
    // Proxy export routes: accept session token OR permanent API key
    // All other routes: require session token only
    //
    let auth_api_routes = Router::new()
        .route("/api/v1/auth/status", axum::routing::get(auth::auth_status))
        .route("/api/v1/auth/setup", axum::routing::post(auth::setup))
        .route("/api/v1/auth/login", axum::routing::post(auth::login))
        .route("/api/v1/auth/logout", axum::routing::post(auth::logout));

    // Proxy export routes — accept session token OR API key
    let proxy_api = api::proxy_api_router()
        .layer(middleware::from_fn_with_state(state.clone(), auth::proxy_api_auth_middleware));

    // Admin/internal API routes — session token only
    let admin_api = api::admin_api_router()
        .layer(middleware::from_fn_with_state(state.clone(), auth::auth_middleware));

    // Auth-management routes (change password, API keys) — session token only
    let auth_mgmt = Router::new()
        .route("/api/v1/auth/change-password", axum::routing::post(auth::change_password))
        .route("/api/v1/auth/api-keys", axum::routing::get(auth::list_api_keys))
        .route("/api/v1/auth/api-keys", axum::routing::post(auth::create_api_key))
        .route("/api/v1/auth/api-keys/:id", axum::routing::delete(auth::delete_api_key))
        .layer(middleware::from_fn_with_state(state.clone(), auth::auth_middleware));

    // Protected page routes (redirect to /login if no cookie)
    let protected_pages = Router::new()
        .route("/", axum::routing::get(dashboard_page))
        .route("/admin", axum::routing::get(admin_page))
        .layer(middleware::from_fn_with_state(state.clone(), auth::page_auth_middleware));

    let app = Router::new()
        // Login page (public)
        .route("/login", axum::routing::get(login_page))
        // Auth API (public)
        .merge(auth_api_routes)
        // Auth management (session-protected)
        .merge(auth_mgmt)
        // Protected pages
        .merge(protected_pages)
        // Proxy export API (session or API key)
        .merge(proxy_api)
        // Admin API (session only)
        .merge(admin_api)
        // Static assets (public — CSS/JS/i18n needed for login page)
        .nest_service("/static", ServeDir::new("static"))
        // Cache-Control: browsers must revalidate static files on every request
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-cache, must-revalidate"),
        ))
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

/// Serve login.html page (public)
async fn login_page() -> Html<String> {
    match tokio::fs::read_to_string("static/login.html").await {
        Ok(content) => Html(content),
        Err(_) => Html("<h1>Login page not found</h1>".to_string()),
    }
}

/// Serve index.html dashboard (requires auth)
async fn dashboard_page() -> Html<String> {
    match tokio::fs::read_to_string("static/index.html").await {
        Ok(content) => Html(content),
        Err(_) => Html("<h1>Dashboard not found</h1>".to_string()),
    }
}

/// Serve admin.html page (requires auth)
async fn admin_page() -> Html<String> {
    match tokio::fs::read_to_string("static/admin.html").await {
        Ok(content) => Html(content),
        Err(_) => Html("<h1>Admin page not found</h1>".to_string()),
    }
}
