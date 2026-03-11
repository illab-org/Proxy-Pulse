use axum::{
    body::Body,
    extract::{Multipart, Path, Query, State},
    http::StatusCode,
    response::{Json, Response},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::{demo_guard, ApiResponse, AppState, ErrorResponse};
use crate::db::Database;
use crate::models::{ProxyAdminResponse, SubscriptionSourceResponse};
use crate::sources;
use crate::updater;

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
        .route("/api/v1/admin/settings/system", get(admin_get_system_settings))
        .route("/api/v1/admin/settings/system", post(admin_save_system_settings))
        .route("/api/v1/admin/db/export", get(admin_export_db))
        .route("/api/v1/admin/db/import", post(admin_import_db))
        .route("/api/v1/admin/update/check", get(admin_check_update))
        .route("/api/v1/admin/update/releases", get(admin_get_releases))
        .route("/api/v1/admin/update/trigger", post(admin_trigger_update))
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

#[derive(Debug, Deserialize)]
pub struct SystemSettingsRequest {
    pub auto_update: Option<bool>,
    pub install_schedule: Option<String>,
    pub default_language: Option<String>,
    pub default_timezone: Option<String>,
    pub default_theme: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TriggerUpdateRequest {
    pub version: Option<String>,
}

async fn admin_get_system_settings(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let auto_update = state.db.get_setting("system.auto_update").await
        .ok().flatten().map(|v| v != "false").unwrap_or(true);
    let install_schedule = state.db.get_setting("system.install_schedule").await
        .ok().flatten().unwrap_or_else(|| "anytime".to_string());
    let default_language = state.db.get_setting("system.default_language").await
        .ok().flatten().unwrap_or_else(|| "auto".to_string());
    let default_timezone = state.db.get_setting("system.default_timezone").await
        .ok().flatten().unwrap_or_else(|| "auto".to_string());
    let default_theme = state.db.get_setting("system.default_theme").await
        .ok().flatten().unwrap_or_else(|| "system".to_string());

    Ok(Json(serde_json::json!({
        "success": true,
        "data": {
            "auto_update": auto_update,
            "install_schedule": install_schedule,
            "default_language": default_language,
            "default_timezone": default_timezone,
            "default_theme": default_theme
        }
    })))
}

async fn admin_save_system_settings(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SystemSettingsRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    demo_guard(&state)?;

    if let Some(auto_update) = body.auto_update {
        state.db.set_setting("system.auto_update", if auto_update { "true" } else { "false" }).await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse {
                success: false, error: e.to_string(),
            })))?;
    }

    if let Some(ref schedule) = body.install_schedule {
        let valid = ["anytime", "night", "morning", "afternoon", "evening"];
        if valid.contains(&schedule.as_str()) {
            state.db.set_setting("system.install_schedule", schedule).await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse {
                    success: false, error: e.to_string(),
                })))?;
        }
    }

    if let Some(ref lang) = body.default_language {
        let valid = ["auto", "en", "zh-CN", "zh-TW", "ja"];
        if !valid.contains(&lang.as_str()) {
            return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse {
                success: false,
                error: "Invalid language. Must be one of: auto, en, zh-CN, zh-TW, ja".to_string(),
            })));
        }
        state.db.set_setting("system.default_language", lang).await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse {
                success: false, error: e.to_string(),
            })))?;
    }

    if let Some(ref tz) = body.default_timezone {
        state.db.set_setting("system.default_timezone", tz).await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse {
                success: false, error: e.to_string(),
            })))?;
    }

    if let Some(ref theme) = body.default_theme {
        let valid = ["system", "light", "dark"];
        if !valid.contains(&theme.as_str()) {
            return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse {
                success: false,
                error: "Invalid theme. Must be one of: system, light, dark".to_string(),
            })));
        }
        state.db.set_setting("system.default_theme", theme).await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse {
                success: false, error: e.to_string(),
            })))?;
    }

    Ok(Json(serde_json::json!({ "success": true })))
}

async fn admin_export_db(
    State(state): State<Arc<AppState>>,
) -> Result<Response<Body>, (StatusCode, Json<ErrorResponse>)> {
    // Checkpoint WAL to ensure the main DB file is up-to-date
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(&state.db.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse {
            success: false, error: e.to_string(),
        })))?;

    let data = tokio::fs::read(&state.db_path).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse {
            success: false, error: format!("Failed to read database file: {}", e),
        }))
    })?;

    Ok(Response::builder()
        .status(200)
        .header("Content-Type", "application/x-sqlite3")
        .header("Content-Disposition", "attachment; filename=\"proxy_pulse.db\"")
        .body(Body::from(data))
        .unwrap())
}

async fn admin_import_db(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    demo_guard(&state)?;

    let field = multipart.next_field().await.map_err(|e| {
        (StatusCode::BAD_REQUEST, Json(ErrorResponse {
            success: false, error: format!("Invalid upload: {}", e),
        }))
    })?.ok_or_else(|| {
        (StatusCode::BAD_REQUEST, Json(ErrorResponse {
            success: false, error: "No file uploaded".to_string(),
        }))
    })?;

    let data = field.bytes().await.map_err(|e| {
        (StatusCode::BAD_REQUEST, Json(ErrorResponse {
            success: false, error: format!("Failed to read upload: {}", e),
        }))
    })?;

    // Validate it's a SQLite file (magic header)
    if data.len() < 16 || &data[..16] != b"SQLite format 3\0" {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse {
            success: false,
            error: "Invalid SQLite database file".to_string(),
        })));
    }

    // Write to a temporary file next to the DB, then rename atomically
    let import_path = format!("{}.import", state.db_path);
    tokio::fs::write(&import_path, &data).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse {
            success: false, error: format!("Failed to write file: {}", e),
        }))
    })?;

    // Close all pool connections
    state.db.pool.close().await;

    // Replace the database file
    if let Err(e) = tokio::fs::rename(&import_path, &state.db_path).await {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse {
            success: false, error: format!("Failed to replace database: {}", e),
        })));
    }

    // Remove WAL and SHM files if they exist
    let _ = tokio::fs::remove_file(format!("{}-wal", state.db_path)).await;
    let _ = tokio::fs::remove_file(format!("{}-shm", state.db_path)).await;

    // Schedule process exit so the run script / Docker / systemd restarts us
    tokio::spawn(async {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        std::process::exit(0);
    });

    Ok(Json(serde_json::json!({
        "success": true,
        "data": { "message": "Database imported. The service will restart automatically." }
    })))
}

async fn admin_check_update() -> Json<serde_json::Value> {
    let current = env!("CARGO_PKG_VERSION");
    match updater::fetch_latest_version().await {
        Ok(tag) => {
            let latest = tag.trim_start_matches('v');
            let update_available = updater::is_newer(latest, current);
            Json(serde_json::json!({
                "success": true,
                "data": {
                    "current_version": current,
                    "latest_version": latest,
                    "update_available": update_available,
                }
            }))
        }
        Err(e) => Json(serde_json::json!({
            "success": false,
            "error": format!("Failed to check for updates: {}", e),
        })),
    }
}

async fn admin_get_releases() -> Json<serde_json::Value> {
    match updater::fetch_releases().await {
        Ok(releases) => Json(serde_json::json!({
            "success": true,
            "data": releases,
        })),
        Err(e) => Json(serde_json::json!({
            "success": false,
            "error": format!("Failed to fetch releases: {}", e),
        })),
    }
}

async fn admin_trigger_update(
    State(state): State<Arc<AppState>>,
    body: Option<Json<TriggerUpdateRequest>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    demo_guard(&state)?;

    let target_version = body.and_then(|b| b.version.clone());

    let result = if let Some(ref ver) = target_version {
        updater::update_to_version(ver).await
    } else {
        updater::manual_update().await
    };

    match result {
        Ok(true) => Ok(Json(serde_json::json!({
            "success": true,
            "data": { "message": "Update triggered. The service will restart automatically." }
        }))),
        Ok(false) => Ok(Json(serde_json::json!({
            "success": true,
            "data": { "message": "Already up to date." }
        }))),
        Err(e) if e.to_string() == "BINARY_NOT_READY" => Err((
            StatusCode::ACCEPTED,
            Json(ErrorResponse {
                success: false,
                error: "BINARY_NOT_READY".to_string(),
            }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                success: false,
                error: format!("Update failed: {}", e),
            }),
        )),
    }
}
