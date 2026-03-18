use axum::{extract::State, http::StatusCode, response::Json};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::{generate_token, hash_api_key, AuthResponse};
use crate::api::AppState;

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

pub async fn create_api_key(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateApiKeyRequest>,
) -> Result<Json<ApiKeyCreatedResponse>, (StatusCode, Json<AuthResponse>)> {
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

    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(err("API key name cannot be empty"));
    }

    let raw_key = format!("ppk_{}", generate_token());
    let key_hash = hash_api_key(&raw_key);
    let preview = format!(
        "ppk_{}...{}",
        &raw_key[4..12],
        &raw_key[raw_key.len() - 4..]
    );

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
        Some(
            (Utc::now().naive_utc() + Duration::hours(hours))
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
        )
    });

    let id = state
        .db
        .create_api_key(&name, &key_hash, &preview, expires_at.as_deref())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AuthResponse {
                    success: false,
                    token: None,
                    error: Some(format!("Failed to create API key: {}", e)),
                }),
            )
        })?;

    Ok(Json(ApiKeyCreatedResponse {
        success: true,
        id,
        key: raw_key,
    }))
}

pub async fn list_api_keys(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ApiKeyListResponse>, (StatusCode, Json<AuthResponse>)> {
    let keys = state.db.get_all_api_keys().await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AuthResponse {
                success: false,
                token: None,
                error: Some("Failed to list API keys".to_string()),
            }),
        )
    })?;

    let keys: Vec<ApiKeyInfo> = keys
        .into_iter()
        .map(|(id, name, preview, expires_at, created_at)| {
            let expired = expires_at.as_ref().map_or(false, |exp| {
                chrono::NaiveDateTime::parse_from_str(exp, "%Y-%m-%d %H:%M:%S")
                    .map_or(false, |dt| dt < Utc::now().naive_utc())
            });
            ApiKeyInfo {
                id,
                name,
                preview,
                expires_at,
                expired,
                created_at,
            }
        })
        .collect();

    Ok(Json(ApiKeyListResponse {
        success: true,
        keys,
    }))
}

pub async fn delete_api_key(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<AuthResponse>)> {
    let deleted = state.db.delete_api_key(id).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AuthResponse {
                success: false,
                token: None,
                error: Some("Failed to delete API key".to_string()),
            }),
        )
    })?;

    Ok(Json(
        serde_json::json!({ "success": true, "deleted": deleted }),
    ))
}
