// Proxy Pulse — open-source proxy pool management & network quality monitoring
mod api;
mod auth;
mod checker;
mod config;
mod db;
mod mem_monitor;
mod models;
mod scheduler;
mod sources;
mod updater;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

/// Configure jemalloc: aggressive memory return to OS.
/// dirty_decay_ms:  how long freed dirty pages linger before purging (default 10000)
/// muzzy_decay_ms:  how long purged-but-mapped pages linger (default 10000)
/// narenas:         limit arena count to reduce per-arena overhead
#[cfg(not(target_env = "msvc"))]
#[allow(non_upper_case_globals)]
#[export_name = "malloc_conf"]
pub static malloc_conf: &[u8] = b"dirty_decay_ms:1000,muzzy_decay_ms:1000,narenas:4\0";

use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::{middleware, Router};
use rust_embed::Embed;
use tower_http::cors::{Any, CorsLayer};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

#[derive(Embed)]
#[folder = "static/"]
struct StaticAssets;

use crate::api::AppState;
use crate::db::Database;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging: daily rolling file (7-day retention) + stdout
    let file_appender = tracing_appender::rolling::RollingFileAppender::builder()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("proxy-pulse")
        .filename_suffix("log")
        .max_log_files(7)
        .build("logs")
        .expect("failed to initialize rolling file appender");
    let (file_writer, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_ansi(false)
                .with_writer(file_writer),
        )
        .init();

    info!("Starting Proxy Pulse v{}", env!("CARGO_PKG_VERSION"));

    // Check for --demo flag
    let args: Vec<String> = std::env::args().collect();
    let demo_mode = args.iter().any(|a| a == "--demo");
    if demo_mode {
        info!("🔒 DEMO MODE enabled — all write/mutation API endpoints will return 403");
    }

    // Database URL: from CLI arg, env var, or default
    let db_url = args
        .iter()
        .skip(1)
        .find(|a| *a != "--demo")
        .cloned()
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .unwrap_or_else(|| "sqlite://proxy_pulse.db?mode=rwc".to_string());

    // Initialize database
    let db = Database::new(&db_url).await?;
    info!("Database initialized");

    // Create shared state
    let db_path = db_url
        .strip_prefix("sqlite://")
        .unwrap_or(&db_url)
        .split('?')
        .next()
        .unwrap_or("proxy_pulse.db")
        .to_string();
    let state = Arc::new(AppState {
        db: db.clone(),
        demo_mode,
        db_path,
    });

    // Start background schedulers
    scheduler::start_schedulers(db.clone()).await;
    info!("Background schedulers started");

    // Start auto-updater
    updater::spawn_auto_updater(db);
    info!("Auto-updater started");

    // Start memory monitor (logs every 60 seconds)
    mem_monitor::spawn_monitor(60);
    info!("Memory monitor started (60s interval)");

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
    let proxy_api = api::proxy_api_router().layer(middleware::from_fn_with_state(
        state.clone(),
        auth::proxy_api_auth_middleware,
    ));

    // Admin/internal API routes — admin role only
    let admin_api = api::admin_api_router()
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::admin_auth_middleware,
        ))
        .layer(DefaultBodyLimit::disable());

    // Auth-management routes (change password, API keys, preferences, me) — session token only
    let auth_mgmt = Router::new()
        .route("/api/v1/auth/me", axum::routing::get(auth::get_me))
        .route(
            "/api/v1/auth/change-password",
            axum::routing::post(auth::change_password),
        )
        .route(
            "/api/v1/auth/api-keys",
            axum::routing::get(auth::list_api_keys),
        )
        .route(
            "/api/v1/auth/api-keys",
            axum::routing::post(auth::create_api_key),
        )
        .route(
            "/api/v1/auth/api-keys/:id",
            axum::routing::delete(auth::delete_api_key),
        )
        .route(
            "/api/v1/auth/preferences",
            axum::routing::get(auth::get_preferences),
        )
        .route(
            "/api/v1/auth/preferences",
            axum::routing::put(auth::save_preferences),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ));

    // Protected page routes (redirect to /login if no cookie)
    let protected_pages = Router::new()
        .route("/", axum::routing::get(dashboard_page))
        .route("/settings", axum::routing::get(settings_page))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::page_auth_middleware,
        ));

    // Admin page (admin role only, redirects to / if not admin)
    let admin_page_route = Router::new()
        .route("/admin", axum::routing::get(admin_page))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::page_admin_middleware,
        ));

    // User management routes (admin only)
    let user_mgmt = Router::new()
        .route("/api/v1/admin/users", axum::routing::get(auth::list_users))
        .route(
            "/api/v1/admin/users",
            axum::routing::post(auth::create_user_handler),
        )
        .route(
            "/api/v1/admin/users/:id",
            axum::routing::delete(auth::delete_user_handler),
        )
        .route(
            "/api/v1/admin/users/:id",
            axum::routing::put(auth::update_user_handler),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::admin_auth_middleware,
        ));

    let app = Router::new()
        // Login page (public)
        .route("/login", axum::routing::get(login_page))
        // Auth API (public)
        .merge(auth_api_routes)
        // Auth management (session-protected)
        .merge(auth_mgmt)
        // Protected pages
        .merge(protected_pages)
        // Admin page (admin only)
        .merge(admin_page_route)
        // Proxy export API (session or API key)
        .merge(proxy_api)
        // Admin API (admin only)
        .merge(admin_api)
        // User management API (admin only)
        .merge(user_mgmt)
        // Static assets (public — CSS/JS/i18n needed for login page)
        .route("/static/*path", axum::routing::get(static_handler))
        .route("/favicon.ico", axum::routing::get(favicon_handler))
        // Cache-Control: browsers must revalidate static files on every request
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-cache, must-revalidate"),
        ))
        // Middleware
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(TraceLayer::new_for_http())
        // Shared state
        .with_state(state);

    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let addr = format!("{}:{}", host, port);
    info!(addr = %addr, "Starting HTTP server");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

fn serve_embedded_html(path: &str) -> Html<String> {
    match StaticAssets::get(path) {
        Some(file) => Html(String::from_utf8_lossy(file.data.as_ref()).into_owned()),
        None => Html(format!("<h1>{path} not found</h1>")),
    }
}

/// Serve login.html page (public)
async fn login_page() -> Html<String> {
    serve_embedded_html("login.html")
}

/// Serve index.html dashboard (requires auth)
async fn dashboard_page() -> Html<String> {
    serve_embedded_html("index.html")
}

/// Serve admin.html page (requires admin role)
async fn admin_page() -> Html<String> {
    serve_embedded_html("admin.html")
}

/// Serve settings.html page (requires auth)
async fn settings_page() -> Html<String> {
    serve_embedded_html("settings.html")
}

/// Serve embedded static assets (CSS/JS/i18n)
async fn static_handler(axum::extract::Path(path): axum::extract::Path<String>) -> Response {
    match StaticAssets::get(&path) {
        Some(file) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            ([(header::CONTENT_TYPE, mime.as_ref())], file.data).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Serve favicon.ico (embedded SVG)
async fn favicon_handler() -> Response {
    match StaticAssets::get("favicon.svg") {
        Some(file) => ([(header::CONTENT_TYPE, "image/svg+xml")], file.data).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
