use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::api::AppState;

const TOKEN_EXPIRY_HOURS: i64 = 24;

#[derive(Debug, Deserialize)]
pub struct SetupRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub success: bool,
    pub token: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub needs_setup: bool,
}

/// Generate a secure random token
fn generate_token() -> String {
    use sha2::{Sha256, Digest};
    let uuid1 = uuid::Uuid::new_v4();
    let uuid2 = uuid::Uuid::new_v4();
    let now = Utc::now().timestamp_nanos_opt().unwrap_or(0);
    let mut hasher = Sha256::new();
    hasher.update(format!("{}-{}-{}", uuid1, uuid2, now));
    hex::encode(hasher.finalize())
}

/// Check if initial setup is needed
pub async fn auth_status(
    State(state): State<Arc<AppState>>,
) -> Json<StatusResponse> {
    let needs_setup = !state.db.has_any_user().await.unwrap_or(true);
    Json(StatusResponse { needs_setup })
}

/// Initial user setup (only works when no users exist)
pub async fn setup(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SetupRequest>,
) -> Result<Json<AuthResponse>, (StatusCode, Json<AuthResponse>)> {
    // Only allow setup when no users exist
    let has_users = state.db.has_any_user().await.unwrap_or(true);
    if has_users {
        return Err((
            StatusCode::FORBIDDEN,
            Json(AuthResponse {
                success: false,
                token: None,
                error: Some("Setup already completed".to_string()),
            }),
        ));
    }

    if body.username.trim().is_empty() || body.password.len() < 6 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AuthResponse {
                success: false,
                token: None,
                error: Some("Username cannot be empty and password must be at least 6 characters".to_string()),
            }),
        ));
    }

    let password_hash = bcrypt::hash(&body.password, bcrypt::DEFAULT_COST).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AuthResponse {
                success: false,
                token: None,
                error: Some("Failed to hash password".to_string()),
            }),
        )
    })?;

    let user_id = state.db.create_user(body.username.trim(), &password_hash).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AuthResponse {
                success: false,
                token: None,
                error: Some(format!("Failed to create user: {}", e)),
            }),
        )
    })?;

    let token = generate_token();
    let expires_at = Utc::now().naive_utc() + Duration::hours(TOKEN_EXPIRY_HOURS);
    state.db.create_session(&token, user_id, expires_at).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AuthResponse {
                success: false,
                token: None,
                error: Some(format!("Failed to create session: {}", e)),
            }),
        )
    })?;

    Ok(Json(AuthResponse {
        success: true,
        token: Some(token),
        error: None,
    }))
}

/// Login with username and password
pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, (StatusCode, Json<AuthResponse>)> {
    let err_response = || {
        (
            StatusCode::UNAUTHORIZED,
            Json(AuthResponse {
                success: false,
                token: None,
                error: Some("Invalid username or password".to_string()),
            }),
        )
    };

    let (user_id, password_hash) = state
        .db
        .get_user_by_username(&body.username)
        .await
        .map_err(|_| err_response())?
        .ok_or_else(err_response)?;

    let valid = bcrypt::verify(&body.password, &password_hash).unwrap_or(false);
    if !valid {
        return Err(err_response());
    }

    let token = generate_token();
    let expires_at = Utc::now().naive_utc() + Duration::hours(TOKEN_EXPIRY_HOURS);
    state.db.create_session(&token, user_id, expires_at).await.map_err(|_| err_response())?;

    Ok(Json(AuthResponse {
        success: true,
        token: Some(token),
        error: None,
    }))
}

/// Logout — invalidate the token
pub async fn logout(
    State(state): State<Arc<AppState>>,
    req: Request,
) -> impl IntoResponse {
    if let Some(token) = extract_token(&req) {
        let _ = state.db.delete_session(&token).await;
    }
    Json(serde_json::json!({ "success": true }))
}

/// Extract token from Authorization header or query param
fn extract_token(req: &Request) -> Option<String> {
    // Try Authorization: Bearer <token>
    if let Some(auth) = req.headers().get(header::AUTHORIZATION) {
        if let Ok(auth_str) = auth.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                return Some(token.to_string());
            }
        }
    }
    None
}

/// Auth middleware — validates token and refreshes expiry on each request
pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    let token = match extract_token(&req) {
        Some(t) => t,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "success": false, "error": "Authentication required" })),
            )
                .into_response();
        }
    };

    match state.db.validate_session(&token).await {
        Ok(Some(_user_id)) => {
            // Refresh token expiry on each operation
            let new_expires = Utc::now().naive_utc() + Duration::hours(TOKEN_EXPIRY_HOURS);
            let _ = state.db.refresh_session(&token, new_expires).await;
            next.run(req).await
        }
        _ => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "success": false, "error": "Invalid or expired token" })),
        )
            .into_response(),
    }
}

/// Page-level auth middleware — for HTML pages, redirects to /login instead of returning JSON
pub async fn page_auth_middleware(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    // Check for token in cookie
    let token = req
        .headers()
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|c| {
                let c = c.trim();
                c.strip_prefix("pp_token=").map(|t| t.to_string())
            })
        });

    match token {
        Some(ref t) => {
            match state.db.validate_session(t).await {
                Ok(Some(_)) => {
                    let new_expires = Utc::now().naive_utc() + Duration::hours(TOKEN_EXPIRY_HOURS);
                    let _ = state.db.refresh_session(t, new_expires).await;
                    next.run(req).await
                }
                _ => axum::response::Redirect::to("/login").into_response(),
            }
        }
        None => axum::response::Redirect::to("/login").into_response(),
    }
}
