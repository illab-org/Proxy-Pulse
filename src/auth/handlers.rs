use axum::{
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use chrono::{Duration, Utc};
use serde::Deserialize;
use std::sync::Arc;

use super::{extract_token, generate_token, AuthResponse, TOKEN_EXPIRY_HOURS};
use crate::api::AppState;

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

#[derive(Debug, serde::Serialize)]
pub struct StatusResponse {
    pub needs_setup: bool,
}

pub async fn auth_status(State(state): State<Arc<AppState>>) -> Json<StatusResponse> {
    let needs_setup = !state.db.has_any_user().await.unwrap_or(true);
    Json(StatusResponse { needs_setup })
}

pub async fn setup(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SetupRequest>,
) -> Result<Json<AuthResponse>, (StatusCode, Json<AuthResponse>)> {
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
                error: Some(
                    "Username cannot be empty and password must be at least 6 characters"
                        .to_string(),
                ),
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

    let user_id = state
        .db
        .create_user(body.username.trim(), &password_hash, "admin")
        .await
        .map_err(|e| {
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
    state
        .db
        .create_session(&token, user_id, expires_at)
        .await
        .map_err(|e| {
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
    state
        .db
        .create_session(&token, user_id, expires_at)
        .await
        .map_err(|_| err_response())?;

    Ok(Json(AuthResponse {
        success: true,
        token: Some(token),
        error: None,
    }))
}

pub async fn logout(State(state): State<Arc<AppState>>, req: Request) -> impl IntoResponse {
    if let Some(token) = extract_token(&req) {
        let _ = state.db.delete_session(&token).await;
    }
    Json(serde_json::json!({ "success": true }))
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

pub async fn change_password(
    State(state): State<Arc<AppState>>,
    req: Request,
) -> Result<Json<AuthResponse>, (StatusCode, Json<AuthResponse>)> {
    let err = |msg: &str| {
        (
            StatusCode::BAD_REQUEST,
            Json(AuthResponse {
                success: false,
                token: None,
                error: Some(msg.to_string()),
            }),
        )
    };

    let token = extract_token(&req).ok_or_else(|| err("Authentication required"))?;
    let user_id = state
        .db
        .validate_session(&token)
        .await
        .map_err(|_| err("Invalid session"))?
        .ok_or_else(|| err("Invalid session"))?;

    let body_bytes = axum::body::to_bytes(req.into_body(), 1024 * 16)
        .await
        .map_err(|_| err("Invalid request body"))?;
    let body: ChangePasswordRequest =
        serde_json::from_slice(&body_bytes).map_err(|_| err("Invalid request body"))?;

    if body.new_password.len() < 6 {
        return Err(err("New password must be at least 6 characters"));
    }

    let current_hash = state
        .db
        .get_user_password_hash(user_id)
        .await
        .map_err(|_| err("Failed to verify password"))?
        .ok_or_else(|| err("User not found"))?;

    if !bcrypt::verify(&body.current_password, &current_hash).unwrap_or(false) {
        return Err(err("Current password is incorrect"));
    }

    let new_hash = bcrypt::hash(&body.new_password, bcrypt::DEFAULT_COST)
        .map_err(|_| err("Failed to hash password"))?;
    state
        .db
        .update_user_password(user_id, &new_hash)
        .await
        .map_err(|_| err("Failed to update password"))?;

    Ok(Json(AuthResponse {
        success: true,
        token: None,
        error: None,
    }))
}
