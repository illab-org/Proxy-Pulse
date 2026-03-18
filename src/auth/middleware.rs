use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use chrono::{Duration, Utc};
use std::sync::Arc;

use super::{
    extract_api_key, extract_bearer_token, extract_token, hash_api_key, TOKEN_EXPIRY_HOURS,
};
use crate::api::AppState;

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

pub async fn page_auth_middleware(
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
        Some(ref t) => match state.db.validate_session(t).await {
            Ok(Some(_)) => {
                let new_expires = Utc::now().naive_utc() + Duration::hours(TOKEN_EXPIRY_HOURS);
                let _ = state.db.refresh_session(t, new_expires).await;
                next.run(req).await
            }
            _ => axum::response::Redirect::to("/login").into_response(),
        },
        None => axum::response::Redirect::to("/login").into_response(),
    }
}

pub async fn proxy_api_auth_middleware(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    // Try session token first
    if let Some(token) = extract_bearer_token(&req) {
        if let Ok(Some(_)) = state.db.validate_session(&token).await {
            let new_expires = Utc::now().naive_utc() + Duration::hours(TOKEN_EXPIRY_HOURS);
            let _ = state.db.refresh_session(&token, new_expires).await;
            return next.run(req).await;
        }
    }

    // Try API key
    if let Some(key) = extract_api_key(&req) {
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
        Ok(Some(user_id)) => match state.db.get_user_role(user_id).await {
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
        },
        _ => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "success": false, "error": "Invalid or expired token" })),
        )
            .into_response(),
    }
}

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
        Some(ref t) => match state.db.validate_session(t).await {
            Ok(Some(user_id)) => match state.db.get_user_role(user_id).await {
                Ok(Some(role)) if role == "admin" => {
                    let new_expires = Utc::now().naive_utc() + Duration::hours(TOKEN_EXPIRY_HOURS);
                    let _ = state.db.refresh_session(t, new_expires).await;
                    next.run(req).await
                }
                _ => axum::response::Redirect::to("/").into_response(),
            },
            _ => axum::response::Redirect::to("/login").into_response(),
        },
        None => axum::response::Redirect::to("/login").into_response(),
    }
}
