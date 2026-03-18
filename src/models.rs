use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Proxy {
    pub id: i64,
    pub ip: String,
    pub port: u16,
    pub protocol: String,  // http, https, socks4, socks5
    pub anonymity: String, // transparent, anonymous, elite
    pub country: String,
    pub score: f64,
    pub is_alive: bool,
    pub success_count: i64,
    pub fail_count: i64,
    pub consecutive_fails: i64,
    pub avg_latency_ms: f64,
    pub last_check_at: Option<NaiveDateTime>,
    pub last_success_at: Option<NaiveDateTime>,
    pub next_check_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub source: String,
    pub group_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyResponse {
    pub proxy: String,
    pub protocol: String,
    pub country: String,
    pub anonymity: String,
    pub score: f64,
    pub latency_ms: f64,
    pub is_alive: bool,
    pub success_count: i64,
    pub fail_count: i64,
    pub success_rate: f64,
    pub group: String,
}

impl From<Proxy> for ProxyResponse {
    fn from(p: Proxy) -> Self {
        let total = p.success_count + p.fail_count;
        let success_rate = if total > 0 {
            (p.success_count as f64 / total as f64) * 100.0
        } else {
            0.0
        };
        Self {
            proxy: format!("{}:{}", p.ip, p.port),
            protocol: p.protocol,
            country: p.country,
            anonymity: p.anonymity,
            score: p.score,
            latency_ms: p.avg_latency_ms,
            is_alive: p.is_alive,
            success_count: p.success_count,
            fail_count: p.fail_count,
            success_rate,
            group: p.group_name,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyStats {
    pub total_proxies: i64,
    pub alive_proxies: i64,
    pub dead_proxies: i64,
    pub avg_score: f64,
    pub avg_latency_ms: f64,
    pub country_distribution: Vec<CountryCount>,
    pub latency_distribution: Vec<LatencyBucket>,
    pub protocol_distribution: Vec<ProtocolCount>,
    pub score_distribution: Vec<ScoreBucket>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CountryCount {
    pub country: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyBucket {
    pub range: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ProtocolCount {
    pub protocol: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreBucket {
    pub range: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[allow(dead_code)]
pub struct CheckLog {
    pub id: i64,
    pub proxy_id: i64,
    pub target: String,
    pub success: bool,
    pub latency_ms: Option<f64>,
    pub error: Option<String>,
    pub checked_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SubscriptionSource {
    pub id: i64,
    pub name: String,
    pub source_type: String, // "url" or "text"
    pub url: Option<String>,
    pub content: Option<String>, // raw text content for "text" type
    pub protocol_hint: String,   // "auto", "http", "socks4", "socks5"
    pub is_enabled: bool,
    pub sync_interval_secs: i64,
    pub proxy_count: i64,
    pub last_sync_at: Option<NaiveDateTime>,
    pub last_error: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionSourceResponse {
    pub id: i64,
    pub name: String,
    pub source_type: String,
    pub url: Option<String>,
    pub protocol_hint: String,
    pub is_enabled: bool,
    pub sync_interval_secs: i64,
    pub proxy_count: i64,
    pub last_sync_at: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
}

impl From<SubscriptionSource> for SubscriptionSourceResponse {
    fn from(s: SubscriptionSource) -> Self {
        Self {
            id: s.id,
            name: s.name,
            source_type: s.source_type,
            url: s.url,
            protocol_hint: s.protocol_hint,
            is_enabled: s.is_enabled,
            sync_interval_secs: s.sync_interval_secs,
            proxy_count: s.proxy_count,
            last_sync_at: s
                .last_sync_at
                .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string()),
            last_error: s.last_error,
            created_at: s.created_at.format("%Y-%m-%d %H:%M:%S").to_string(),
        }
    }
}

/// Proxy with full details for admin view
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyAdminResponse {
    pub id: i64,
    pub proxy: String,
    pub protocol: String,
    pub country: String,
    pub anonymity: String,
    pub score: f64,
    pub latency_ms: f64,
    pub is_alive: bool,
    pub success_count: i64,
    pub fail_count: i64,
    pub consecutive_fails: i64,
    pub source: String,
    pub group: String,
    pub last_check_at: Option<String>,
    pub last_success_at: Option<String>,
    pub next_check_at: Option<String>,
    pub created_at: String,
}

impl From<Proxy> for ProxyAdminResponse {
    fn from(p: Proxy) -> Self {
        Self {
            id: p.id,
            proxy: format!("{}:{}", p.ip, p.port),
            protocol: p.protocol,
            country: p.country,
            anonymity: p.anonymity,
            score: p.score,
            latency_ms: p.avg_latency_ms,
            is_alive: p.is_alive,
            success_count: p.success_count,
            fail_count: p.fail_count,
            consecutive_fails: p.consecutive_fails,
            source: p.source,
            group: p.group_name,
            last_check_at: p
                .last_check_at
                .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string()),
            last_success_at: p
                .last_success_at
                .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string()),
            next_check_at: p
                .next_check_at
                .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string()),
            created_at: p.created_at.format("%Y-%m-%d %H:%M:%S").to_string(),
        }
    }
}
