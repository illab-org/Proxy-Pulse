use axum::{
    extract::{Request, State},
    http::StatusCode,
    response::Json,
};
use serde::Deserialize;
use std::sync::Arc;

use super::{extract_token, AuthResponse};
use crate::api::AppState;

// ── Current User Info ──

pub async fn get_me(
    State(state): State<Arc<AppState>>,
    req: Request,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<AuthResponse>)> {
    let err = |msg: &str| {
        (
            StatusCode::UNAUTHORIZED,
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

    let (username, role) = state
        .db
        .get_user_info(user_id)
        .await
        .map_err(|_| err("User not found"))?
        .ok_or_else(|| err("User not found"))?;

    Ok(Json(serde_json::json!({
        "success": true,
        "username": username,
        "role": role
    })))
}

// ── User Preferences ──

#[derive(Debug, Deserialize)]
pub struct PreferencesRequest {
    pub theme: String,
    pub language: String,
    #[serde(default = "default_timezone")]
    pub timezone: String,
}

fn default_timezone() -> String {
    "auto".to_string()
}

pub async fn get_preferences(
    State(state): State<Arc<AppState>>,
    req: Request,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<AuthResponse>)> {
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

    let (theme, language, timezone) = state
        .db
        .get_user_preferences(user_id)
        .await
        .map_err(|_| err("Failed to load preferences"))?;

    Ok(Json(serde_json::json!({
        "success": true,
        "theme": theme,
        "language": language,
        "timezone": timezone
    })))
}

pub async fn save_preferences(
    State(state): State<Arc<AppState>>,
    req: Request,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<AuthResponse>)> {
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
    let body: PreferencesRequest =
        serde_json::from_slice(&body_bytes).map_err(|_| err("Invalid request body"))?;

    let valid_themes = ["system", "light", "dark"];
    let valid_langs = ["default", "en", "zh-CN", "zh-TW", "ja"];
    if !valid_themes.contains(&body.theme.as_str())
        || !valid_langs.contains(&body.language.as_str())
    {
        return Err(err("Invalid theme or language value"));
    }

    // Validate timezone: must be "auto" or a valid IANA timezone name (basic format check)
    if body.timezone != "auto"
        && (body.timezone.len() > 50
            || !body
                .timezone
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '/' || c == '_' || c == '-' || c == '+'))
    {
        return Err(err("Invalid timezone value"));
    }

    state
        .db
        .save_user_preferences(user_id, &body.theme, &body.language, &body.timezone)
        .await
        .map_err(|_| err("Failed to save preferences"))?;

    Ok(Json(serde_json::json!({ "success": true })))
}

// ── User Management (admin only) ──

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub role: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserRequest {
    pub role: Option<String>,
    pub password: Option<String>,
}

pub async fn list_users(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<AuthResponse>)> {
    let users = state.db.get_all_users().await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AuthResponse {
                success: false,
                token: None,
                error: Some("Failed to list users".to_string()),
            }),
        )
    })?;

    let users_json: Vec<serde_json::Value> = users
        .into_iter()
        .map(|(id, username, role, created_at)| {
            serde_json::json!({
                "id": id,
                "username": username,
                "role": role,
                "created_at": created_at
            })
        })
        .collect();

    Ok(Json(
        serde_json::json!({ "success": true, "users": users_json }),
    ))
}

pub async fn create_user_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateUserRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<AuthResponse>)> {
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

    let username = body.username.trim().to_string();
    if username.is_empty() || body.password.len() < 6 {
        return Err(err(
            "Username cannot be empty and password must be at least 6 characters",
        ));
    }

    let valid_roles = ["admin", "user"];
    if !valid_roles.contains(&body.role.as_str()) {
        return Err(err("Role must be 'admin' or 'user'"));
    }

    let password_hash = bcrypt::hash(&body.password, bcrypt::DEFAULT_COST)
        .map_err(|_| err("Failed to hash password"))?;

    let user_id = state
        .db
        .create_user(&username, &password_hash, &body.role)
        .await
        .map_err(|e| {
            let msg = if e.to_string().contains("UNIQUE") {
                "Username already exists".to_string()
            } else {
                format!("Failed to create user: {}", e)
            };
            (
                StatusCode::CONFLICT,
                Json(AuthResponse {
                    success: false,
                    token: None,
                    error: Some(msg),
                }),
            )
        })?;

    Ok(Json(serde_json::json!({ "success": true, "id": user_id })))
}

pub async fn delete_user_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<i64>,
    req: Request,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<AuthResponse>)> {
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
    let current_user_id = state
        .db
        .validate_session(&token)
        .await
        .map_err(|_| err("Invalid session"))?
        .ok_or_else(|| err("Invalid session"))?;

    if id == current_user_id {
        return Err(err("Cannot delete your own account"));
    }

    let target_role = state
        .db
        .get_user_role(id)
        .await
        .map_err(|_| err("User not found"))?
        .ok_or_else(|| err("User not found"))?;

    if target_role == "admin" {
        let admin_count = state
            .db
            .count_admins()
            .await
            .map_err(|_| err("Database error"))?;
        if admin_count <= 1 {
            return Err(err("Cannot delete the last admin user"));
        }
    }

    let deleted = state
        .db
        .delete_user(id)
        .await
        .map_err(|_| err("Failed to delete user"))?;
    Ok(Json(
        serde_json::json!({ "success": true, "deleted": deleted }),
    ))
}

pub async fn update_user_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<i64>,
    Json(body): Json<UpdateUserRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<AuthResponse>)> {
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

    if let Some(ref role) = body.role {
        let valid_roles = ["admin", "user"];
        if !valid_roles.contains(&role.as_str()) {
            return Err(err("Role must be 'admin' or 'user'"));
        }
        // Prevent demoting the last admin
        if role == "user" {
            let current_role = state
                .db
                .get_user_role(id)
                .await
                .map_err(|_| err("User not found"))?
                .ok_or_else(|| err("User not found"))?;
            if current_role == "admin" {
                let admin_count = state
                    .db
                    .count_admins()
                    .await
                    .map_err(|_| err("Database error"))?;
                if admin_count <= 1 {
                    return Err(err("Cannot demote the last admin user"));
                }
            }
        }
    }

    let password_hash = if let Some(ref pw) = body.password {
        if pw.len() < 6 {
            return Err(err("Password must be at least 6 characters"));
        }
        Some(bcrypt::hash(pw, bcrypt::DEFAULT_COST).map_err(|_| err("Failed to hash password"))?)
    } else {
        None
    };

    state
        .db
        .update_user(id, body.role.as_deref(), password_hash.as_deref())
        .await
        .map_err(|_| err("Failed to update user"))?;

    Ok(Json(serde_json::json!({ "success": true })))
}
