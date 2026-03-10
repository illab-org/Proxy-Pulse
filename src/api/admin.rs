use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::{demo_guard, ApiResponse, AppState, ErrorResponse};
use crate::db::Database;
use crate::models::{ProxyAdminResponse, SubscriptionSourceResponse};
use crate::sources;

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
    pub protocol_hint: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ImportResult {
    pub imported: usize,
}

#[derive(Debug, Deserialize)]
pub struct AddSourceRequest {
    pub name: String,
    pub source_type: String,
    pub url: Option<String>,
    pub content: Option<String>,
    pub protocol_hint: Option<String>,
    pub sync_interval_secs: Option<i64>,
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

pub fn admin_api_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/admin/proxy/list", get(admin_get_proxies))
        .route("/api/v1/admin/proxy/import", post(admin_import_proxies))
        .route(
            "/api/v1/admin/proxy/purge-dead",
            post(admin_delete_dead_proxies),
        )
        .route("/api/v1/admin/proxy/delete/:id", post(admin_delete_proxy))
        .route("/api/v1/admin/source/list", get(admin_get_sources))
        .route("/api/v1/admin/source/add", post(admin_add_source))
        .route(
            "/api/v1/admin/source/delete/:id",
            post(admin_delete_source),
        )
        .route(
            "/api/v1/admin/source/:id/toggle",
            post(admin_toggle_source),
        )
        .route("/api/v1/admin/source/sync", post(admin_sync_sources))
        .route("/api/v1/admin/settings/checker", get(admin_get_checker_settings))
        .route("/api/v1/admin/settings/checker", post(admin_save_checker_settings))
}

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

async fn admin_import_proxies(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ImportProxiesRequest>,
) -> Result<Json<ApiResponse<ImportResult>>, (StatusCode, Json<ErrorResponse>)> {
    demo_guard(&state)?;
    let protocol_hint = body.protocol_hint.as_deref().unwrap_or("auto");
    let proxies = sources::parse_proxy_list(&body.content);

    match sources::import_proxies_with_hint(&state.db, &proxies, "admin:import", protocol_hint)
        .await
    {
        Ok(count) => {
            spawn_immediate_check(state.db.clone());
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

async fn admin_delete_proxy(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<ApiResponse<bool>>, (StatusCode, Json<ErrorResponse>)> {
    demo_guard(&state)?;
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

async fn admin_delete_dead_proxies(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ApiResponse<DeleteResult>>, (StatusCode, Json<ErrorResponse>)> {
    demo_guard(&state)?;
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

async fn admin_get_sources(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ApiResponse<Vec<SubscriptionSourceResponse>>>, (StatusCode, Json<ErrorResponse>)>
{
    match state.db.get_all_subscription_sources().await {
        Ok(sources) => {
            let data: Vec<SubscriptionSourceResponse> = sources
                .into_iter()
                .map(SubscriptionSourceResponse::from)
                .collect();
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

async fn admin_add_source(
    State(state): State<Arc<AppState>>,
    Json(body): Json<AddSourceRequest>,
) -> Result<Json<ApiResponse<AddSourceResult>>, (StatusCode, Json<ErrorResponse>)> {
    demo_guard(&state)?;
    let protocol_hint = body.protocol_hint.as_deref().unwrap_or("auto");
    let sync_interval_secs = body.sync_interval_secs.unwrap_or(21600);

    let id = match state
        .db
        .create_subscription_source(
            &body.name,
            &body.source_type,
            body.url.as_deref(),
            body.content.as_deref(),
            protocol_hint,
            sync_interval_secs,
        )
        .await
    {
        Ok(id) => id,
        Err(e) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    success: false,
                    error: e.to_string(),
                }),
            ))
        }
    };

    // Immediately sync this new source
    let synced = match state.db.get_subscription_source_by_id(id).await {
        Ok(Some(source)) => {
            match sources::sync_single_subscription(&state.db, &source).await {
                Ok(count) => {
                    let _ = state
                        .db
                        .update_subscription_sync_result(id, count as i64, None)
                        .await;
                    count
                }
                Err(e) => {
                    let _ = state
                        .db
                        .update_subscription_sync_result(id, 0, Some(&e.to_string()))
                        .await;
                    0
                }
            }
        }
        _ => 0,
    };

    if synced > 0 {
        spawn_immediate_check(state.db.clone());
    }

    Ok(Json(ApiResponse {
        success: true,
        data: AddSourceResult { id, synced },
    }))
}

async fn admin_delete_source(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<ApiResponse<bool>>, (StatusCode, Json<ErrorResponse>)> {
    demo_guard(&state)?;
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

async fn admin_toggle_source(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(body): Json<ToggleSourceRequest>,
) -> Result<Json<ApiResponse<bool>>, (StatusCode, Json<ErrorResponse>)> {
    demo_guard(&state)?;
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

async fn admin_sync_sources(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ApiResponse<SyncResult>>, (StatusCode, Json<ErrorResponse>)> {
    demo_guard(&state)?;
    match sources::sync_subscription_sources(&state.db).await {
        Ok(count) => {
            if count > 0 {
                spawn_immediate_check(state.db.clone());
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

fn spawn_immediate_check(db: Database) {
    tokio::spawn(async move {
        let checker_cfg = db.get_checker_config().await;
        match crate::checker::run_check_cycle(&db, &checker_cfg).await {
            Ok((s, f)) => tracing::info!(success = s, fail = f, "Immediate check cycle complete"),
            Err(e) => tracing::warn!(error = %e, "Immediate check cycle failed"),
        }
    });
}

// ── Checker Settings ──

async fn admin_get_checker_settings(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let cfg = state.db.get_checker_config().await;
    Ok(Json(serde_json::json!({
        "success": true,
        "data": cfg
    })))
}

async fn admin_save_checker_settings(
    State(state): State<Arc<AppState>>,
    Json(body): Json<crate::config::CheckerConfig>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    demo_guard(&state)?;

    // Basic validation
    if body.interval_secs < 10 || body.interval_secs > 86400 {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse {
            success: false,
            error: "interval_secs must be between 10 and 86400".to_string(),
        })));
    }
    if body.timeout_secs < 1 || body.timeout_secs > 120 {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse {
            success: false,
            error: "timeout_secs must be between 1 and 120".to_string(),
        })));
    }
    if body.max_concurrent < 1 || body.max_concurrent > 10000 {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse {
            success: false,
            error: "max_concurrent must be between 1 and 10000".to_string(),
        })));
    }
    if body.targets.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse {
            success: false,
            error: "At least one target URL is required".to_string(),
        })));
    }

    state.db.save_checker_config(&body).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse {
            success: false,
            error: e.to_string(),
        }))
    })?;

    Ok(Json(serde_json::json!({ "success": true })))
}
