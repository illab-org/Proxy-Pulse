mod admin;
mod proxy;

pub use admin::admin_api_router;
pub use proxy::proxy_api_router;

use axum::http::StatusCode;
use axum::response::Json;
use serde::Serialize;

use crate::db::Database;

/// Shared application state
pub struct AppState {
    pub db: Database,
    pub demo_mode: bool,
    pub db_path: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ApiResponse<T: Serialize> {
    pub success: bool,
    pub data: T,
}

#[derive(Debug, Serialize)]
pub(crate) struct ErrorResponse {
    pub success: bool,
    pub error: String,
}

pub(crate) fn demo_guard(state: &AppState) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    if state.demo_mode {
        Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                success: false,
                error: "Operation not allowed in demo mode".to_string(),
            }),
        ))
    } else {
        Ok(())
    }
}
