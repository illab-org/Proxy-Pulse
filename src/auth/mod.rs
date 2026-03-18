mod api_keys;
mod handlers;
mod middleware;
mod users;

pub use api_keys::{create_api_key, delete_api_key, list_api_keys};
pub use handlers::{auth_status, change_password, login, logout, setup};
pub use middleware::{
    admin_auth_middleware, auth_middleware, page_admin_middleware, page_auth_middleware,
    proxy_api_auth_middleware,
};
pub use users::{
    create_user_handler, delete_user_handler, get_me, get_preferences, list_users,
    save_preferences, update_user_handler,
};

use axum::extract::Request;
use axum::http::header;
use serde::Serialize;

const TOKEN_EXPIRY_HOURS: i64 = 24;

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub success: bool,
    pub token: Option<String>,
    pub error: Option<String>,
}

fn generate_token() -> String {
    use sha2::{Digest, Sha256};
    let uuid1 = uuid::Uuid::new_v4();
    let uuid2 = uuid::Uuid::new_v4();
    let now = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
    let mut hasher = Sha256::new();
    hasher.update(format!("{}-{}-{}", uuid1, uuid2, now));
    hex::encode(hasher.finalize())
}

fn extract_token(req: &Request) -> Option<String> {
    // Try Bearer token from Authorization header first
    if let Some(auth) = req.headers().get(header::AUTHORIZATION) {
        if let Ok(auth_str) = auth.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                return Some(token.to_string());
            }
        }
    }
    // Fallback: extract from pp_token cookie (survives redirects from CDN/proxy)
    if let Some(cookie) = req.headers().get(header::COOKIE) {
        if let Ok(cookies) = cookie.to_str() {
            for c in cookies.split(';') {
                if let Some(token) = c.trim().strip_prefix("pp_token=") {
                    return Some(token.to_string());
                }
            }
        }
    }
    None
}

fn extract_bearer_token(req: &Request) -> Option<String> {
    req.headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.to_string())
}

fn extract_api_key(req: &Request) -> Option<String> {
    if let Some(key) = req.headers().get("X-API-Key").and_then(|v| v.to_str().ok()) {
        if key.starts_with("ppk_") {
            return Some(key.to_string());
        }
    }
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

fn hash_api_key(key: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}
