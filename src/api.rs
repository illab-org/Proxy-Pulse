use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::checker;
use crate::config::AppConfig;
use crate::db::Database;
use crate::models::{ProxyAdminResponse, ProxyResponse, ProxyStats, SubscriptionSourceResponse};
use crate::sources;

/// Shared application state
pub struct AppState {
    pub db: Database,
    pub config: Arc<AppConfig>,
}

#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct TopParams {
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub success: bool,
    pub data: T,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub success: bool,
    pub error: String,
}

#[derive(Debug, Serialize)]
pub struct ProxyListResponse {
    pub proxies: Vec<ProxyResponse>,
    pub count: usize,
}

// ── Admin request/response types ──

#[derive(Debug, Deserialize)]
pub struct AdminProxyListParams {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    pub alive: Option<bool>,
    pub protocol: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AdminProxyListResponse {
    pub proxies: Vec<ProxyAdminResponse>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
}

#[derive(Debug, Deserialize)]
pub struct ImportProxiesRequest {
    pub content: String,
    pub protocol_hint: Option<String>, // "auto", "http", "socks4", "socks5"
}

#[derive(Debug, Serialize)]
pub struct ImportResult {
    pub imported: usize,
}

#[derive(Debug, Deserialize)]
pub struct AddSourceRequest {
    pub name: String,
    pub source_type: String, // "url" or "text"
    pub url: Option<String>,
    pub content: Option<String>,
    pub protocol_hint: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ToggleSourceRequest {
    pub enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct DeleteResult {
    pub deleted: u64,
}

#[derive(Debug, Serialize)]
pub struct AddSourceResult {
    pub id: i64,
    pub synced: usize,
}

#[derive(Debug, Serialize)]
pub struct SyncResult {
    pub synced: usize,
}

/// Build the API router (public + admin)
pub fn api_router() -> Router<Arc<AppState>> {
    Router::new()
        // Public API
        .route("/api/v1/proxy/random", get(get_random_proxy))
        .route("/api/v1/proxy/top", get(get_top_proxies))
        .route("/api/v1/proxy/country/:country", get(get_proxies_by_country))
        .route("/api/v1/proxy/all", get(get_all_proxies))
        .route("/api/v1/proxy/stats", get(get_stats))
        .route("/api/v1/health", get(health_check))
        // Admin API — Proxy management
        .route("/api/v1/admin/proxy/list", get(admin_get_proxies))
        .route("/api/v1/admin/proxy/import", post(admin_import_proxies))
        .route("/api/v1/admin/proxy/purge-dead", post(admin_delete_dead_proxies))
        .route("/api/v1/admin/proxy/delete/:id", post(admin_delete_proxy))
        // Admin API — Subscription source management
        .route("/api/v1/admin/source/list", get(admin_get_sources))
        .route("/api/v1/admin/source/add", post(admin_add_source))
        .route("/api/v1/admin/source/delete/:id", post(admin_delete_source))
        .route("/api/v1/admin/source/:id/toggle", post(admin_toggle_source))
        .route("/api/v1/admin/source/sync", post(admin_sync_sources))
}

/// GET /api/v1/proxy/random
async fn get_random_proxy(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ApiResponse<Option<ProxyResponse>>>, (StatusCode, Json<ErrorResponse>)> {
    match state.db.get_random_alive_proxy().await {
        Ok(proxy) => Ok(Json(ApiResponse {
            success: true,
            data: proxy.map(ProxyResponse::from),
        })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                success: false,
                error: e.to_string(),
            }),
        )),
    }
}

/// GET /api/v1/proxy/top?limit=10
async fn get_top_proxies(
    State(state): State<Arc<AppState>>,
    Query(params): Query<TopParams>,
) -> Result<Json<ApiResponse<ProxyListResponse>>, (StatusCode, Json<ErrorResponse>)> {
    let limit = params.limit.unwrap_or(10).min(100);

    match state.db.get_top_proxies(limit).await {
        Ok(proxies) => {
            let count = proxies.len();
            let proxies: Vec<ProxyResponse> = proxies.into_iter().map(ProxyResponse::from).collect();
            Ok(Json(ApiResponse {
                success: true,
                data: ProxyListResponse { proxies, count },
            }))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                success: false,
                error: e.to_string(),
            }),
        )),
    }
}

/// GET /api/v1/proxy/country/:country
async fn get_proxies_by_country(
    State(state): State<Arc<AppState>>,
    Path(country): Path<String>,
    Query(params): Query<TopParams>,
) -> Result<Json<ApiResponse<ProxyListResponse>>, (StatusCode, Json<ErrorResponse>)> {
    let limit = params.limit.unwrap_or(20).min(100);

    match state.db.get_proxies_by_country(&country, limit).await {
        Ok(proxies) => {
            let count = proxies.len();
            let proxies: Vec<ProxyResponse> = proxies.into_iter().map(ProxyResponse::from).collect();
            Ok(Json(ApiResponse {
                success: true,
                data: ProxyListResponse { proxies, count },
            }))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                success: false,
                error: e.to_string(),
            }),
        )),
    }
}

/// GET /api/v1/proxy/all?page=1&per_page=20
async fn get_all_proxies(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<ApiResponse<ProxyListResponse>>, (StatusCode, Json<ErrorResponse>)> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).min(100);

    match state.db.get_all_proxies(page, per_page).await {
        Ok(proxies) => {
            let count = proxies.len();
            let proxies: Vec<ProxyResponse> = proxies.into_iter().map(ProxyResponse::from).collect();
            Ok(Json(ApiResponse {
                success: true,
                data: ProxyListResponse { proxies, count },
            }))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                success: false,
                error: e.to_string(),
            }),
        )),
    }
}

/// GET /api/v1/proxy/stats
async fn get_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ApiResponse<ProxyStats>>, (StatusCode, Json<ErrorResponse>)> {
    match state.db.get_stats().await {
        Ok(stats) => Ok(Json(ApiResponse {
            success: true,
            data: stats,
        })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                success: false,
                error: e.to_string(),
            }),
        )),
    }
}

/// GET /api/v1/health
async fn health_check() -> Json<ApiResponse<String>> {
    Json(ApiResponse {
        success: true,
        data: "Proxy Pulse is running".to_string(),
    })
}

// ═══════════════════════════════════════════════
//  Admin API Handlers
// ═══════════════════════════════════════════════

/// GET /api/v1/admin/proxy/list?page=1&per_page=20&alive=true&protocol=http
async fn admin_get_proxies(
    State(state): State<Arc<AppState>>,
    Query(params): Query<AdminProxyListParams>,
) -> Result<Json<ApiResponse<AdminProxyListResponse>>, (StatusCode, Json<ErrorResponse>)> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).min(200);

    match state
        .db
        .get_all_proxies_admin(page, per_page, params.alive, params.protocol.as_deref())
        .await
    {
        Ok((proxies, total)) => {
            let proxies: Vec<ProxyAdminResponse> =
                proxies.into_iter().map(ProxyAdminResponse::from).collect();
            Ok(Json(ApiResponse {
                success: true,
                data: AdminProxyListResponse {
                    proxies,
                    total,
                    page,
                    per_page,
                },
            }))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                success: false,
                error: e.to_string(),
            }),
        )),
    }
}

/// POST /api/v1/admin/proxy/import  — Bulk import proxies from text
async fn admin_import_proxies(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ImportProxiesRequest>,
) -> Result<Json<ApiResponse<ImportResult>>, (StatusCode, Json<ErrorResponse>)> {
    let protocol_hint = body.protocol_hint.as_deref().unwrap_or("auto");
    let proxies = sources::parse_proxy_list(&body.content);

    match sources::import_proxies_with_hint(&state.db, &proxies, "admin:import", protocol_hint)
        .await
    {
        Ok(count) => {
            // Trigger immediate check for newly imported proxies
            spawn_immediate_check(state.db.clone(), state.config.clone());
            Ok(Json(ApiResponse {
                success: true,
                data: ImportResult { imported: count },
            }))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                success: false,
                error: e.to_string(),
            }),
        )),
    }
}

/// DELETE /api/v1/admin/proxy/:id
async fn admin_delete_proxy(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<ApiResponse<bool>>, (StatusCode, Json<ErrorResponse>)> {
    match state.db.delete_proxy(id).await {
        Ok(deleted) => Ok(Json(ApiResponse {
            success: true,
            data: deleted,
        })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                success: false,
                error: e.to_string(),
            }),
        )),
    }
}

/// DELETE /api/v1/admin/proxy/dead
async fn admin_delete_dead_proxies(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ApiResponse<DeleteResult>>, (StatusCode, Json<ErrorResponse>)> {
    match state.db.delete_all_dead_proxies().await {
        Ok(count) => Ok(Json(ApiResponse {
            success: true,
            data: DeleteResult { deleted: count },
        })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                success: false,
                error: e.to_string(),
            }),
        )),
    }
}

/// GET /api/v1/admin/source/list
async fn admin_get_sources(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ApiResponse<Vec<SubscriptionSourceResponse>>>, (StatusCode, Json<ErrorResponse>)>
{
    match state.db.get_all_subscription_sources().await {
        Ok(sources) => {
            let data: Vec<SubscriptionSourceResponse> =
                sources.into_iter().map(SubscriptionSourceResponse::from).collect();
            Ok(Json(ApiResponse {
                success: true,
                data,
            }))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                success: false,
                error: format!("{}", e),
            }),
        )),
    }
}

/// POST /api/v1/admin/source/add — Create source + immediate sync & check
async fn admin_add_source(
    State(state): State<Arc<AppState>>,
    Json(body): Json<AddSourceRequest>,
) -> Result<Json<ApiResponse<AddSourceResult>>, (StatusCode, Json<ErrorResponse>)> {
    let protocol_hint = body.protocol_hint.as_deref().unwrap_or("auto");

    let id = match state
        .db
        .create_subscription_source(
            &body.name,
            &body.source_type,
            body.url.as_deref(),
            body.content.as_deref(),
            protocol_hint,
        )
        .await
    {
        Ok(id) => id,
        Err(e) => return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                success: false,
                error: e.to_string(),
            }),
        )),
    };

    // Immediately sync this new source
    let synced = match state.db.get_subscription_source_by_id(id).await {
        Ok(Some(source)) => {
            match sources::sync_single_subscription(&state.db, &source).await {
                Ok(count) => {
                    let _ = state.db.update_subscription_sync_result(id, count as i64, None).await;
                    count
                }
                Err(e) => {
                    let _ = state.db.update_subscription_sync_result(id, 0, Some(&e.to_string())).await;
                    0
                }
            }
        }
        _ => 0,
    };

    // Trigger immediate check for newly imported proxies
    if synced > 0 {
        spawn_immediate_check(state.db.clone(), state.config.clone());
    }

    Ok(Json(ApiResponse {
        success: true,
        data: AddSourceResult { id, synced },
    }))
}

/// DELETE /api/v1/admin/source/:id
async fn admin_delete_source(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<ApiResponse<bool>>, (StatusCode, Json<ErrorResponse>)> {
    match state.db.delete_subscription_source(id).await {
        Ok(deleted) => Ok(Json(ApiResponse {
            success: true,
            data: deleted,
        })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                success: false,
                error: e.to_string(),
            }),
        )),
    }
}

/// POST /api/v1/admin/source/:id/toggle
async fn admin_toggle_source(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(body): Json<ToggleSourceRequest>,
) -> Result<Json<ApiResponse<bool>>, (StatusCode, Json<ErrorResponse>)> {
    match state.db.toggle_subscription_source(id, body.enabled).await {
        Ok(updated) => Ok(Json(ApiResponse {
            success: true,
            data: updated,
        })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                success: false,
                error: e.to_string(),
            }),
        )),
    }
}

/// POST /api/v1/admin/source/sync — Trigger manual sync of all enabled subscription sources
async fn admin_sync_sources(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ApiResponse<SyncResult>>, (StatusCode, Json<ErrorResponse>)> {
    match sources::sync_subscription_sources(&state.db).await {
        Ok(count) => {
            // Trigger immediate check for newly synced proxies
            if count > 0 {
                spawn_immediate_check(state.db.clone(), state.config.clone());
            }
            Ok(Json(ApiResponse {
                success: true,
                data: SyncResult { synced: count },
            }))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                success: false,
                error: e.to_string(),
            }),
        )),
    }
}

/// Spawn a background task to immediately check proxies that are due
fn spawn_immediate_check(db: Database, config: Arc<AppConfig>) {
    tokio::spawn(async move {
        match checker::run_check_cycle(&db, &config.checker, &config.scoring).await {
            Ok((s, f)) => tracing::info!(success = s, fail = f, "Immediate check cycle complete"),
            Err(e) => tracing::warn!(error = %e, "Immediate check cycle failed"),
        }
    });
}
