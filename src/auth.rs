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

    let user_id = state.db.create_user(body.username.trim(), &password_hash, "admin").await.map_err(|e| {
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

// ── Change Password ──

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
            Json(AuthResponse { success: false, token: None, error: Some(msg.to_string()) }),
        )
    };

    // Extract user from token
    let token = extract_token(&req).ok_or_else(|| err("Authentication required"))?;
    let user_id = state.db.validate_session(&token).await
        .map_err(|_| err("Invalid session"))?
        .ok_or_else(|| err("Invalid session"))?;

    // Parse body manually since we already consumed the request for token extraction
    let body_bytes = axum::body::to_bytes(req.into_body(), 1024 * 16).await
        .map_err(|_| err("Invalid request body"))?;
    let body: ChangePasswordRequest = serde_json::from_slice(&body_bytes)
        .map_err(|_| err("Invalid request body"))?;

    if body.new_password.len() < 6 {
        return Err(err("New password must be at least 6 characters"));
    }

    // Verify current password
    let current_hash = state.db.get_user_password_hash(user_id).await
        .map_err(|_| err("Failed to verify password"))?
        .ok_or_else(|| err("User not found"))?;

    if !bcrypt::verify(&body.current_password, &current_hash).unwrap_or(false) {
        return Err(err("Current password is incorrect"));
    }

    // Hash and update
    let new_hash = bcrypt::hash(&body.new_password, bcrypt::DEFAULT_COST)
        .map_err(|_| err("Failed to hash password"))?;
    state.db.update_user_password(user_id, &new_hash).await
        .map_err(|_| err("Failed to update password"))?;

    Ok(Json(AuthResponse { success: true, token: None, error: None }))
}

// ── API Key Management ──

#[derive(Debug, Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
    pub expires_in: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ApiKeyCreatedResponse {
    pub success: bool,
    pub id: i64,
    pub key: String,
}

#[derive(Debug, Serialize)]
pub struct ApiKeyInfo {
    pub id: i64,
    pub name: String,
    pub preview: String,
    pub expires_at: Option<String>,
    pub expired: bool,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct ApiKeyListResponse {
    pub success: bool,
    pub keys: Vec<ApiKeyInfo>,
}

fn hash_api_key(key: &str) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

pub async fn create_api_key(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateApiKeyRequest>,
) -> Result<Json<ApiKeyCreatedResponse>, (StatusCode, Json<AuthResponse>)> {
    let err = |msg: &str| {
        (
            StatusCode::BAD_REQUEST,
            Json(AuthResponse { success: false, token: None, error: Some(msg.to_string()) }),
        )
    };

    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(err("API key name cannot be empty"));
    }

    // Generate a unique API key with "ppk_" prefix
    let raw_key = format!("ppk_{}", generate_token());
    let key_hash = hash_api_key(&raw_key);
    let preview = format!("ppk_{}...{}", &raw_key[4..12], &raw_key[raw_key.len()-4..]);

    // Calculate expiry
    let expires_at = body.expires_in.as_deref().and_then(|v| {
        let hours: i64 = match v {
            "1h" => 1,
            "24h" => 24,
            "7d" => 24 * 7,
            "30d" => 24 * 30,
            "90d" => 24 * 90,
            "365d" => 24 * 365,
            _ => return None,
        };
        Some((Utc::now().naive_utc() + Duration::hours(hours)).format("%Y-%m-%d %H:%M:%S").to_string())
    });

    let id = state.db.create_api_key(&name, &key_hash, &preview, expires_at.as_deref()).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AuthResponse { success: false, token: None, error: Some(format!("Failed to create API key: {}", e)) }),
        )
    })?;

    Ok(Json(ApiKeyCreatedResponse { success: true, id, key: raw_key }))
}

pub async fn list_api_keys(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ApiKeyListResponse>, (StatusCode, Json<AuthResponse>)> {
    let keys = state.db.get_all_api_keys().await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AuthResponse { success: false, token: None, error: Some("Failed to list API keys".to_string()) }),
        )
    })?;

    let keys: Vec<ApiKeyInfo> = keys.into_iter().map(|(id, name, preview, expires_at, created_at)| {
        let expired = expires_at.as_ref().map_or(false, |exp| {
            chrono::NaiveDateTime::parse_from_str(exp, "%Y-%m-%d %H:%M:%S")
                .map_or(false, |dt| dt < Utc::now().naive_utc())
        });
        ApiKeyInfo { id, name, preview, expires_at, expired, created_at }
    }).collect();

    Ok(Json(ApiKeyListResponse { success: true, keys }))
}

pub async fn delete_api_key(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<AuthResponse>)> {
    let deleted = state.db.delete_api_key(id).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AuthResponse { success: false, token: None, error: Some("Failed to delete API key".to_string()) }),
        )
    })?;

    Ok(Json(serde_json::json!({ "success": true, "deleted": deleted })))
}

/// Middleware for proxy export endpoints: accepts either session token OR API key
pub async fn proxy_api_auth_middleware(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    // Try session token first (Bearer header)
    if let Some(token) = extract_bearer_token(&req) {
        if let Ok(Some(_)) = state.db.validate_session(&token).await {
            let new_expires = Utc::now().naive_utc() + Duration::hours(TOKEN_EXPIRY_HOURS);
            let _ = state.db.refresh_session(&token, new_expires).await;
            return next.run(req).await;
        }
    }

    // Try API key from query param ?api_key=ppk_xxx or header X-API-Key
    let api_key = extract_api_key(&req);
    if let Some(key) = api_key {
        let key_hash = hash_api_key(&key);
        if let Ok(true) = state.db.validate_api_key(&key_hash).await {
            return next.run(req).await;
        }
    }

    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "success": false, "error": "Authentication required. Use Bearer token or API key." })),
    )
        .into_response()
}

fn extract_bearer_token(req: &Request) -> Option<String> {
    req.headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.to_string())
}

fn extract_api_key(req: &Request) -> Option<String> {
    // Try X-API-Key header
    if let Some(key) = req.headers().get("X-API-Key").and_then(|v| v.to_str().ok()) {
        if key.starts_with("ppk_") {
            return Some(key.to_string());
        }
    }

    // Try ?api_key= query param
    if let Some(query) = req.uri().query() {
        for pair in query.split('&') {
            if let Some(key) = pair.strip_prefix("api_key=") {
                let decoded = urlencoding::decode(key).unwrap_or_else(|_| key.into());
                if decoded.starts_with("ppk_") {
                    return Some(decoded.to_string());
                }
            }
        }
    }

    None
}

// ── Admin-only auth middleware (JSON 403 for non-admins) ──

pub async fn admin_auth_middleware(
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
        Ok(Some(user_id)) => {
            // Check role
            match state.db.get_user_role(user_id).await {
                Ok(Some(role)) if role == "admin" => {
                    let new_expires = Utc::now().naive_utc() + Duration::hours(TOKEN_EXPIRY_HOURS);
                    let _ = state.db.refresh_session(&token, new_expires).await;
                    next.run(req).await
                }
                _ => (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({ "success": false, "error": "Admin access required" })),
                )
                    .into_response(),
            }
        }
        _ => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "success": false, "error": "Invalid or expired token" })),
        )
            .into_response(),
    }
}

// ── Page-level admin middleware (redirects non-admins to /) ──

pub async fn page_admin_middleware(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
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
                Ok(Some(user_id)) => {
                    match state.db.get_user_role(user_id).await {
                        Ok(Some(role)) if role == "admin" => {
                            let new_expires = Utc::now().naive_utc() + Duration::hours(TOKEN_EXPIRY_HOURS);
                            let _ = state.db.refresh_session(t, new_expires).await;
                            next.run(req).await
                        }
                        _ => axum::response::Redirect::to("/").into_response(),
                    }
                }
                _ => axum::response::Redirect::to("/login").into_response(),
            }
        }
        None => axum::response::Redirect::to("/login").into_response(),
    }
}

// ── Current User Info ──

pub async fn get_me(
    State(state): State<Arc<AppState>>,
    req: Request,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<AuthResponse>)> {
    let err = |msg: &str| {
        (
            StatusCode::UNAUTHORIZED,
            Json(AuthResponse { success: false, token: None, error: Some(msg.to_string()) }),
        )
    };

    let token = extract_token(&req).ok_or_else(|| err("Authentication required"))?;
    let user_id = state.db.validate_session(&token).await
        .map_err(|_| err("Invalid session"))?
        .ok_or_else(|| err("Invalid session"))?;

    let (username, role) = state.db.get_user_info(user_id).await
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
}

pub async fn get_preferences(
    State(state): State<Arc<AppState>>,
    req: Request,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<AuthResponse>)> {
    let err = |msg: &str| {
        (
            StatusCode::BAD_REQUEST,
            Json(AuthResponse { success: false, token: None, error: Some(msg.to_string()) }),
        )
    };

    let token = extract_token(&req).ok_or_else(|| err("Authentication required"))?;
    let user_id = state.db.validate_session(&token).await
        .map_err(|_| err("Invalid session"))?
        .ok_or_else(|| err("Invalid session"))?;

    let (theme, language) = state.db.get_user_preferences(user_id).await
        .map_err(|_| err("Failed to load preferences"))?;

    Ok(Json(serde_json::json!({
        "success": true,
        "theme": theme,
        "language": language
    })))
}

pub async fn save_preferences(
    State(state): State<Arc<AppState>>,
    req: Request,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<AuthResponse>)> {
    let err = |msg: &str| {
        (
            StatusCode::BAD_REQUEST,
            Json(AuthResponse { success: false, token: None, error: Some(msg.to_string()) }),
        )
    };

    let token = extract_token(&req).ok_or_else(|| err("Authentication required"))?;
    let user_id = state.db.validate_session(&token).await
        .map_err(|_| err("Invalid session"))?
        .ok_or_else(|| err("Invalid session"))?;

    let body_bytes = axum::body::to_bytes(req.into_body(), 1024 * 16).await
        .map_err(|_| err("Invalid request body"))?;
    let body: PreferencesRequest = serde_json::from_slice(&body_bytes)
        .map_err(|_| err("Invalid request body"))?;

    // Validate values
    let valid_themes = ["system", "light", "dark"];
    let valid_langs = ["en", "zh-CN", "zh-TW", "ja"];
    if !valid_themes.contains(&body.theme.as_str()) || !valid_langs.contains(&body.language.as_str()) {
        return Err(err("Invalid theme or language value"));
    }

    state.db.save_user_preferences(user_id, &body.theme, &body.language).await
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

pub async fn list_users(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<AuthResponse>)> {
    let users = state.db.get_all_users().await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AuthResponse { success: false, token: None, error: Some("Failed to list users".to_string()) }),
        )
    })?;

    let users_json: Vec<serde_json::Value> = users.into_iter().map(|(id, username, role, created_at)| {
        serde_json::json!({
            "id": id,
            "username": username,
            "role": role,
            "created_at": created_at
        })
    }).collect();

    Ok(Json(serde_json::json!({ "success": true, "users": users_json })))
}

pub async fn create_user_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateUserRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<AuthResponse>)> {
    let err = |msg: &str| {
        (
            StatusCode::BAD_REQUEST,
            Json(AuthResponse { success: false, token: None, error: Some(msg.to_string()) }),
        )
    };

    let username = body.username.trim().to_string();
    if username.is_empty() || body.password.len() < 6 {
        return Err(err("Username cannot be empty and password must be at least 6 characters"));
    }

    let valid_roles = ["admin", "user"];
    if !valid_roles.contains(&body.role.as_str()) {
        return Err(err("Role must be 'admin' or 'user'"));
    }

    let password_hash = bcrypt::hash(&body.password, bcrypt::DEFAULT_COST)
        .map_err(|_| err("Failed to hash password"))?;

    let user_id = state.db.create_user(&username, &password_hash, &body.role).await
        .map_err(|e| {
            let msg = if e.to_string().contains("UNIQUE") {
                "Username already exists".to_string()
            } else {
                format!("Failed to create user: {}", e)
            };
            (
                StatusCode::CONFLICT,
                Json(AuthResponse { success: false, token: None, error: Some(msg) }),
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
            Json(AuthResponse { success: false, token: None, error: Some(msg.to_string()) }),
        )
    };

    // Get current user to prevent self-deletion
    let token = extract_token(&req).ok_or_else(|| err("Authentication required"))?;
    let current_user_id = state.db.validate_session(&token).await
        .map_err(|_| err("Invalid session"))?
        .ok_or_else(|| err("Invalid session"))?;

    if id == current_user_id {
        return Err(err("Cannot delete your own account"));
    }

    // Prevent deleting the last admin
    let target_role = state.db.get_user_role(id).await
        .map_err(|_| err("User not found"))?
        .ok_or_else(|| err("User not found"))?;

    if target_role == "admin" {
        let admin_count = state.db.count_admins().await.map_err(|_| err("Database error"))?;
        if admin_count <= 1 {
            return Err(err("Cannot delete the last admin user"));
        }
    }

    let deleted = state.db.delete_user(id).await.map_err(|_| err("Failed to delete user"))?;
    Ok(Json(serde_json::json!({ "success": true, "deleted": deleted })))
}
