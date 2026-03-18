use anyhow::Result;
use chrono::{Duration, Utc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};

use crate::config::CheckerConfig;
use crate::db::Database;
use crate::models::Proxy;

/// Guard to prevent concurrent check cycles
static CHECK_RUNNING: AtomicBool = AtomicBool::new(false);

/// Shared direct HTTP client (not proxied) for GeoIP lookups etc.
static DIRECT_CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();

fn direct_client() -> &'static reqwest::Client {
    DIRECT_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .pool_max_idle_per_host(2)
            .tcp_nodelay(true)
            .build()
            .expect("Failed to build direct HTTP client")
    })
}

/// Run a check cycle: fetch due proxies and check them
pub async fn run_check_cycle(db: &Database, checker_cfg: &CheckerConfig) -> Result<(usize, usize)> {
    // Prevent concurrent check cycles from overlapping
    if CHECK_RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        info!("Check cycle already running, skipping");
        return Ok((0, 0));
    }
    let result = run_check_cycle_inner(db, checker_cfg).await;
    CHECK_RUNNING.store(false, Ordering::SeqCst);
    result
}

async fn run_check_cycle_inner(
    db: &Database,
    checker_cfg: &CheckerConfig,
) -> Result<(usize, usize)> {
    let proxies = db
        .get_proxies_due_for_check(checker_cfg.max_concurrent as i64 * 2)
        .await?;

    if proxies.is_empty() {
        return Ok((0, 0));
    }

    let total = proxies.len();
    info!(count = total, "Starting proxy check cycle");

    let semaphore = Arc::new(Semaphore::new(checker_cfg.max_concurrent));
    let db = db.clone();
    let timeout = std::time::Duration::from_secs(checker_cfg.timeout_secs);
    let targets: Arc<[String]> = checker_cfg.targets.clone().into();

    let mut handles = Vec::new();

    for proxy in proxies {
        let permit = semaphore.clone().acquire_owned().await?;
        let db = db.clone();
        let targets = targets.clone();
        let checker_cfg = checker_cfg.clone();

        let handle = tokio::spawn(async move {
            let result = check_single_proxy(&db, &proxy, &targets, timeout, &checker_cfg).await;
            drop(permit);
            result
        });
        handles.push(handle);
    }

    let mut success_count = 0;
    for handle in handles {
        match handle.await {
            Ok(Ok(true)) => success_count += 1,
            Ok(Ok(false)) => {}
            Ok(Err(e)) => warn!(error = %e, "Check task error"),
            Err(e) => warn!(error = %e, "Check task panicked"),
        }
    }

    info!(
        total = total,
        success = success_count,
        fail = total - success_count,
        "Check cycle complete"
    );

    // Force jemalloc to return unused memory to OS after each cycle
    crate::mem_monitor::purge_jemalloc();

    Ok((success_count, total - success_count))
}

/// Check a single proxy against all targets
async fn check_single_proxy(
    db: &Database,
    proxy: &Proxy,
    targets: &[String],
    timeout: std::time::Duration,
    checker_cfg: &CheckerConfig,
) -> Result<bool> {
    let proxy_addr = format!("{}:{}", proxy.ip, proxy.port);

    // Build the reqwest::Client ONCE per proxy, reuse across all targets
    let proxy_url = match proxy.protocol.as_str() {
        "socks5" => format!("socks5://{}", proxy_addr),
        "socks4" => format!("socks4://{}", proxy_addr),
        _ => format!("http://{}", proxy_addr),
    };
    let req_proxy = reqwest::Proxy::all(&proxy_url)?;
    let client = Arc::new(
        reqwest::Client::builder()
            .proxy(req_proxy)
            .timeout(timeout)
            .danger_accept_invalid_certs(true)
            .pool_max_idle_per_host(0)
            .tcp_nodelay(true)
            .redirect(reqwest::redirect::Policy::limited(3))
            .build()?,
    );

    // Check all targets in parallel to minimize per-proxy check time
    let mut target_handles = Vec::new();
    for target in targets {
        let client = client.clone();
        let t = target.clone();
        let handle = tokio::spawn(async move {
            let start = Instant::now();
            let result = check_with_client(&client, &t).await;
            let elapsed = start.elapsed().as_millis() as f64;
            (t, result, elapsed)
        });
        target_handles.push(handle);
    }

    let mut any_success = false;
    let mut total_latency = 0.0;
    let mut success_checks = 0;

    for handle in target_handles {
        match handle.await {
            Ok((target, result, elapsed)) => match result {
                Ok(()) => {
                    any_success = true;
                    success_checks += 1;
                    total_latency += elapsed;

                    db.insert_check_log(proxy.id, &target, true, Some(elapsed), None)
                        .await?;

                    debug!(
                        proxy = %proxy_addr,
                        target = %target,
                        latency_ms = elapsed,
                        "Check passed"
                    );
                }
                Err(e) => {
                    db.insert_check_log(proxy.id, &target, false, None, Some(&e.to_string()))
                        .await?;

                    debug!(
                        proxy = %proxy_addr,
                        target = %target,
                        error = %e,
                        "Check failed"
                    );
                }
            },
            Err(e) => {
                warn!(error = %e, "Target check task panicked");
            }
        }
    }

    let avg_latency = if success_checks > 0 {
        Some(total_latency / success_checks as f64)
    } else {
        None
    };

    // Calculate next check time from configured success/fail intervals.
    // fail_count remains historical total and is never reset on success.
    let next_check = calculate_next_check(proxy, any_success, checker_cfg);

    db.update_proxy_check(proxy.id, any_success, avg_latency, next_check)
        .await?;

    // Recalculate score
    let score = calculate_score(proxy, any_success, avg_latency);
    db.update_proxy_score(proxy.id, score).await?;

    // Detect metadata from response if successful, reusing the proxy client
    if any_success {
        detect_and_update_metadata(db, proxy, &client).await.ok();
    }

    // Explicitly drop client to free TLS context and connection pool
    drop(client);

    Ok(any_success)
}

/// Check a proxy against a single target URL using a shared client
async fn check_with_client(client: &reqwest::Client, target: &str) -> Result<()> {
    let resp = client.get(target).send().await?;
    let status = resp.status();
    drop(resp);

    if status.is_success() || status.is_redirection() {
        Ok(())
    } else {
        anyhow::bail!("HTTP {}", status.as_u16())
    }
}

/// Calculate next check time from checker settings.
/// Success uses success interval; failure uses tier by historical fail_count.
fn calculate_next_check(
    proxy: &Proxy,
    success: bool,
    checker_cfg: &CheckerConfig,
) -> chrono::NaiveDateTime {
    let now = Utc::now().naive_utc();

    if success {
        now + Duration::seconds(checker_cfg.interval_secs as i64)
    } else {
        let fail_total_after_this_check = (proxy.fail_count + 1).max(1) as usize;
        let idx = fail_total_after_this_check.saturating_sub(1).min(9);
        let secs = checker_cfg
            .fail_intervals_secs
            .get(idx)
            .copied()
            .unwrap_or(60)
            .max(1);
        now + Duration::seconds(secs as i64)
    }
}

/// Calculate health score (0-100)
///   Success rate:   60 pts  (sigmoid: c=80%, k=15)
///   Success count:  10 pts  (100 successes → full score)
///   Country:         6 pts  (tier-based ranking)
///   Proxy type:      4 pts  (more secure = higher)
///   Latency:        20 pts  (≤100ms=20, ≥5000ms=0, linear)
fn calculate_score(proxy: &Proxy, current_success: bool, avg_latency: Option<f64>) -> f64 {
    let total_checks = proxy.success_count + proxy.fail_count + 1;
    let successes = proxy.success_count + if current_success { 1 } else { 0 };

    // Success rate component (0-60): sigmoid function centered at 80%
    let rate = successes as f64 / total_checks as f64;
    let success_rate_score = 60.0 / (1.0 + (-15.0_f64 * (rate - 0.80)).exp());

    // Success count component (0-10): 100 successes = full 10
    let success_count_score = ((successes as f64 / 100.0) * 10.0).min(10.0);

    // Country component (0-6): tiered ranking
    let country_score = country_tier_score(&proxy.country);

    // Proxy type component (0-4): more secure = higher
    let type_score = match proxy.protocol.as_str() {
        "socks5" => 4.0, // most versatile & secure
        "https" => 3.0,
        "socks4" => 2.0,
        "http" => 1.0,
        _ => 0.0,
    };

    // Latency component (0-20): ≤100ms = 20, ≥5000ms = 0, linear between
    let latency_score = match avg_latency.or(Some(proxy.avg_latency_ms)) {
        Some(ms) if ms > 0.0 => {
            if ms <= 100.0 {
                20.0
            } else if ms >= 5000.0 {
                0.0
            } else {
                20.0 * (5000.0 - ms) / 4900.0
            }
        }
        _ => 0.0,
    };

    let score =
        success_rate_score + success_count_score + country_score + type_score + latency_score;
    score.clamp(0.0, 100.0)
}

/// Country tier scoring (0-6)
fn country_tier_score(country: &str) -> f64 {
    if country == "unknown" || country.is_empty() {
        return 0.0;
    }
    // Tier 1 (6 pts): major datacenter / premium regions
    const TIER1: &[&str] = &[
        "US", "GB", "DE", "JP", "SG", "NL", "CA", "AU", "FR", "SE", "CH", "IE", "FI", "NO", "DK",
    ];
    // Tier 2 (4.5 pts): solid infrastructure countries
    const TIER2: &[&str] = &[
        "KR", "TW", "HK", "IT", "ES", "BR", "IN", "PL", "CZ", "RO", "AT", "BE", "NZ", "IL", "ZA",
    ];
    // Tier 3 (3 pts): decent regions
    const TIER3: &[&str] = &[
        "RU", "UA", "TR", "MX", "AR", "CL", "CO", "TH", "VN", "ID", "PH", "MY", "PT", "GR", "HU",
        "BG",
    ];
    let code = country.to_uppercase();
    let c = code.as_str();
    if TIER1.contains(&c) {
        6.0
    } else if TIER2.contains(&c) {
        4.5
    } else if TIER3.contains(&c) {
        3.0
    } else {
        1.5
    } // known but unlisted country
}

/// Try to detect proxy metadata (anonymity, protocol detection)
/// Only runs for proxies that still have unknown metadata to avoid redundant work.
async fn detect_and_update_metadata(
    db: &Database,
    proxy: &Proxy,
    client: &reqwest::Client,
) -> Result<()> {
    // Skip anonymity redetection if already known
    let anonymity = if proxy.anonymity == "unknown" {
        detect_anonymity(client)
            .await
            .unwrap_or_else(|_| "unknown".to_string())
    } else {
        proxy.anonymity.clone()
    };

    // Skip protocol redetection if already known
    let protocol = proxy.protocol.clone();

    // Country detection — only if unknown
    let country = if proxy.country == "unknown" {
        detect_country_by_ip(&proxy.ip)
            .await
            .unwrap_or_else(|_| "unknown".to_string())
    } else {
        proxy.country.clone()
    };

    // Only write to DB if something actually changed
    if anonymity != proxy.anonymity || protocol != proxy.protocol || country != proxy.country {
        db.update_proxy_metadata(proxy.id, &country, &anonymity, &protocol)
            .await?;
    }

    Ok(())
}

async fn detect_anonymity(client: &reqwest::Client) -> Result<String> {
    let resp = client.get("https://httpbin.org/headers").send().await?;

    // Limit body read to prevent memory bloat from unexpected large responses
    let bytes = resp.bytes().await?;
    if bytes.len() > 8192 {
        anyhow::bail!("Response too large for anonymity detection");
    }
    let body: serde_json::Value = serde_json::from_slice(&bytes)?;
    drop(bytes);

    if let Some(headers) = body.get("headers").and_then(|h| h.as_object()) {
        let has_via = headers.contains_key("Via");
        let has_forwarded = headers.contains_key("X-Forwarded-For");

        if !has_via && !has_forwarded {
            Ok("elite".to_string())
        } else if has_forwarded {
            Ok("transparent".to_string())
        } else {
            Ok("anonymous".to_string())
        }
    } else {
        Ok("unknown".to_string())
    }
}

/// Detect country from the proxy's IP address using free GeoIP APIs (direct, not through proxy)
async fn detect_country_by_ip(ip: &str) -> Result<String> {
    let client = direct_client();

    // Try ip-api.com first (free, no key required, 45 req/min)
    if let Ok(resp) = client
        .get(&format!(
            "http://ip-api.com/json/{}?fields=status,countryCode",
            ip
        ))
        .send()
        .await
    {
        if let Ok(json) = resp.json::<serde_json::Value>().await {
            if json.get("status").and_then(|s| s.as_str()) == Some("success") {
                if let Some(cc) = json.get("countryCode").and_then(|c| c.as_str()) {
                    if !cc.is_empty() {
                        return Ok(cc.to_lowercase());
                    }
                }
            }
        }
    }

    // Fallback: ipinfo.io (free tier, 50k/month)
    if let Ok(resp) = client
        .get(&format!("https://ipinfo.io/{}/json", ip))
        .send()
        .await
    {
        if let Ok(json) = resp.json::<serde_json::Value>().await {
            if let Some(cc) = json.get("country").and_then(|c| c.as_str()) {
                if !cc.is_empty() {
                    return Ok(cc.to_lowercase());
                }
            }
        }
    }

    // Fallback: ipwho.is (free, unlimited)
    if let Ok(resp) = client.get(&format!("https://ipwho.is/{}", ip)).send().await {
        if let Ok(json) = resp.json::<serde_json::Value>().await {
            if json.get("success").and_then(|s| s.as_bool()) == Some(true) {
                if let Some(cc) = json.get("country_code").and_then(|c| c.as_str()) {
                    if !cc.is_empty() {
                        return Ok(cc.to_lowercase());
                    }
                }
            }
        }
    }

    Ok("unknown".to_string())
}
