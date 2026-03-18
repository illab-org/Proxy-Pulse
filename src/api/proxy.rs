use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::get,
    Router,
};
use serde::Deserialize;
use std::sync::Arc;

use super::{ApiResponse, AppState, ErrorResponse};
use crate::models::{ProxyResponse, ProxyStats};

#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct TopParams {
    pub limit: Option<i64>,
    pub group: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ExportParams {
    pub sort: Option<String>,
    pub limit: Option<i64>,
    pub country: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct ProxyListResponse {
    pub proxies: Vec<ProxyResponse>,
    pub count: usize,
}

pub fn proxy_api_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/proxy/random", get(get_random_proxy))
        .route("/api/v1/proxy/top", get(get_top_proxies))
        .route("/api/v1/proxy/groups", get(get_proxy_groups))
        .route(
            "/api/v1/proxy/country/:country",
            get(get_proxies_by_country),
        )
        .route("/api/v1/proxy/all", get(get_all_proxies))
        .route("/api/v1/proxy/json", get(get_proxies_json))
        .route("/api/v1/proxy/txt", get(get_proxies_txt))
        .route("/api/v1/proxy/csv", get(get_proxies_csv))
        .route("/api/v1/proxy/stats", get(get_stats))
        .route("/api/v1/proxy/countries", get(get_countries))
        .route("/api/v1/health", get(health_check))
        .route("/api/v1/demo-mode", get(get_demo_mode))
}

async fn get_random_proxy(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ApiResponse<Option<ProxyResponse>>>, (StatusCode, Json<ErrorResponse>)> {
    match state.db.get_random_alive_proxy().await {
        Ok(proxy) => Ok(Json(ApiResponse {
            success: true,
            data: proxy.map(ProxyResponse::from),
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

async fn get_top_proxies(
    State(state): State<Arc<AppState>>,
    Query(params): Query<TopParams>,
) -> Result<Json<ApiResponse<ProxyListResponse>>, (StatusCode, Json<ErrorResponse>)> {
    let limit = params.limit.unwrap_or(10).min(100);
    let group = params.group.as_deref();

    match state.db.get_top_proxies(limit, group).await {
        Ok(proxies) => {
            let count = proxies.len();
            let proxies: Vec<ProxyResponse> =
                proxies.into_iter().map(ProxyResponse::from).collect();
            Ok(Json(ApiResponse {
                success: true,
                data: ProxyListResponse { proxies, count },
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

async fn get_proxy_groups(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ApiResponse<Vec<String>>>, (StatusCode, Json<ErrorResponse>)> {
    match state.db.get_proxy_groups().await {
        Ok(groups) => Ok(Json(ApiResponse {
            success: true,
            data: groups,
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

async fn get_proxies_by_country(
    State(state): State<Arc<AppState>>,
    Path(country): Path<String>,
    Query(params): Query<TopParams>,
) -> Result<Json<ApiResponse<ProxyListResponse>>, (StatusCode, Json<ErrorResponse>)> {
    let limit = params.limit.unwrap_or(20).min(100);

    match state.db.get_proxies_by_country(&country, limit).await {
        Ok(proxies) => {
            let count = proxies.len();
            let proxies: Vec<ProxyResponse> =
                proxies.into_iter().map(ProxyResponse::from).collect();
            Ok(Json(ApiResponse {
                success: true,
                data: ProxyListResponse { proxies, count },
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

async fn get_all_proxies(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<ApiResponse<ProxyListResponse>>, (StatusCode, Json<ErrorResponse>)> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).min(100);

    match state.db.get_all_proxies(page, per_page).await {
        Ok(proxies) => {
            let count = proxies.len();
            let proxies: Vec<ProxyResponse> =
                proxies.into_iter().map(ProxyResponse::from).collect();
            Ok(Json(ApiResponse {
                success: true,
                data: ProxyListResponse { proxies, count },
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

async fn get_proxies_json(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ExportParams>,
) -> Result<Json<ApiResponse<ProxyListResponse>>, (StatusCode, Json<ErrorResponse>)> {
    let sort = params.sort.as_deref().unwrap_or("score");
    let limit = params.limit.map(|l| l.max(1));
    let country = params.country.as_deref();

    match state.db.get_proxies_sorted(sort, limit, country).await {
        Ok(proxies) => {
            let count = proxies.len();
            let proxies: Vec<ProxyResponse> =
                proxies.into_iter().map(ProxyResponse::from).collect();
            Ok(Json(ApiResponse {
                success: true,
                data: ProxyListResponse { proxies, count },
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

async fn get_proxies_txt(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ExportParams>,
) -> impl IntoResponse {
    let sort = params.sort.as_deref().unwrap_or("score");
    let limit = params.limit.map(|l| l.max(1));
    let country = params.country.as_deref();

    match state.db.get_proxies_sorted(sort, limit, country).await {
        Ok(proxies) => {
            let txt: String = proxies
                .iter()
                .map(|p| format!("{}://{}:{}", p.protocol, p.ip, p.port))
                .collect::<Vec<_>>()
                .join("\n");
            (
                StatusCode::OK,
                [("content-type", "text/plain; charset=utf-8")],
                txt,
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            [("content-type", "text/plain; charset=utf-8")],
            format!("Error: {}", e),
        ),
    }
}

async fn get_proxies_csv(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ExportParams>,
) -> impl IntoResponse {
    let sort = params.sort.as_deref().unwrap_or("score");
    let limit = params.limit.map(|l| l.max(1));
    let country = params.country.as_deref();

    match state.db.get_proxies_sorted(sort, limit, country).await {
        Ok(proxies) => {
            let mut csv = String::from(
                "ip,port,protocol,country,score,latency_ms,success_count,fail_count,success_rate\n",
            );
            for p in &proxies {
                let total = p.success_count + p.fail_count;
                let rate = if total > 0 {
                    (p.success_count as f64 / total as f64) * 100.0
                } else {
                    0.0
                };
                csv.push_str(&format!(
                    "{},{},{},{},{:.1},{:.0},{},{},{:.1}\n",
                    p.ip,
                    p.port,
                    p.protocol,
                    p.country,
                    p.score,
                    p.avg_latency_ms,
                    p.success_count,
                    p.fail_count,
                    rate
                ));
            }
            (
                StatusCode::OK,
                [("content-type", "text/csv; charset=utf-8")],
                csv,
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            [("content-type", "text/csv; charset=utf-8")],
            format!("Error: {}", e),
        ),
    }
}

async fn get_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ApiResponse<ProxyStats>>, (StatusCode, Json<ErrorResponse>)> {
    match state.db.get_stats().await {
        Ok(stats) => Ok(Json(ApiResponse {
            success: true,
            data: stats,
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

async fn get_countries(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ApiResponse<Vec<String>>>, (StatusCode, Json<ErrorResponse>)> {
    match state.db.get_alive_countries().await {
        Ok(countries) => Ok(Json(ApiResponse {
            success: true,
            data: countries,
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

async fn health_check() -> Json<ApiResponse<serde_json::Value>> {
    Json(ApiResponse {
        success: true,
        data: serde_json::json!({
            "status": "running",
            "version": env!("CARGO_PKG_VERSION")
        }),
    })
}

async fn get_demo_mode(State(state): State<Arc<AppState>>) -> Json<ApiResponse<bool>> {
    Json(ApiResponse {
        success: true,
        data: state.demo_mode,
    })
}
