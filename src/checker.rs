use anyhow::Result;
use chrono::{Duration, Utc};
use std::time::Instant;
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};
use std::sync::Arc;

use crate::config::{CheckerConfig, ScoringConfig};
use crate::db::Database;
use crate::models::Proxy;

/// Run a check cycle: fetch due proxies and check them
pub async fn run_check_cycle(
    db: &Database,
    checker_cfg: &CheckerConfig,
    scoring_cfg: &ScoringConfig,
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
    let targets = checker_cfg.targets.clone();
    let scoring_cfg = scoring_cfg.clone();

    let mut handles = Vec::new();

    for proxy in proxies {
        let permit = semaphore.clone().acquire_owned().await?;
        let db = db.clone();
        let targets = targets.clone();
        let scoring_cfg = scoring_cfg.clone();

        let handle = tokio::spawn(async move {
            let result = check_single_proxy(&db, &proxy, &targets, timeout, &scoring_cfg).await;
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

    Ok((success_count, total - success_count))
}

/// Check a single proxy against all targets
async fn check_single_proxy(
    db: &Database,
    proxy: &Proxy,
    targets: &[String],
    timeout: std::time::Duration,
    scoring_cfg: &ScoringConfig,
) -> Result<bool> {
    let proxy_addr = format!("{}:{}", proxy.ip, proxy.port);
    let mut any_success = false;
    let mut total_latency = 0.0;
    let mut success_checks = 0;

    for target in targets {
        let start = Instant::now();
        let result = check_proxy_against_target(&proxy_addr, &proxy.protocol, target, timeout).await;
        let elapsed = start.elapsed().as_millis() as f64;

        match result {
            Ok(()) => {
                any_success = true;
                success_checks += 1;
                total_latency += elapsed;

                db.insert_check_log(proxy.id, target, true, Some(elapsed), None)
                    .await?;

                debug!(
                    proxy = %proxy_addr,
                    target = %target,
                    latency_ms = elapsed,
                    "Check passed"
                );
            }
            Err(e) => {
                db.insert_check_log(proxy.id, target, false, None, Some(&e.to_string()))
                    .await?;

                debug!(
                    proxy = %proxy_addr,
                    target = %target,
                    error = %e,
                    "Check failed"
                );
            }
        }
    }

    let avg_latency = if success_checks > 0 {
        Some(total_latency / success_checks as f64)
    } else {
        None
    };

    // Calculate next check time using adaptive backoff
    let next_check = calculate_next_check(proxy, any_success);

    db.update_proxy_check(proxy.id, any_success, avg_latency, next_check)
        .await?;

    // Recalculate score
    let score = calculate_score(proxy, any_success, avg_latency, scoring_cfg);
    db.update_proxy_score(proxy.id, score).await?;

    // Detect metadata from response if successful
    if any_success {
        detect_and_update_metadata(db, proxy).await.ok();
    }

    Ok(any_success)
}

/// Check a proxy against a single target URL
async fn check_proxy_against_target(
    proxy_addr: &str,
    protocol: &str,
    target: &str,
    timeout: std::time::Duration,
) -> Result<()> {
    let proxy_url = match protocol {
        "socks5" => format!("socks5://{}", proxy_addr),
        "socks4" => format!("socks5://{}", proxy_addr), // reqwest uses socks5 for both
        "https" => format!("https://{}", proxy_addr),
        _ => format!("http://{}", proxy_addr),
    };

    let proxy = reqwest::Proxy::all(&proxy_url)?;
    let client = reqwest::Client::builder()
        .proxy(proxy)
        .timeout(timeout)
        .danger_accept_invalid_certs(true)
        .build()?;

    let resp = client.get(target).send().await?;

    if resp.status().is_success() || resp.status().is_redirection() {
        Ok(())
    } else {
        anyhow::bail!("HTTP status: {}", resp.status())
    }
}

/// Adaptive backoff: calculate next check time based on consecutive failures
fn calculate_next_check(proxy: &Proxy, success: bool) -> chrono::NaiveDateTime {
    let now = Utc::now().naive_utc();

    if success {
        // Successful — check again at normal interval (5 min)
        now + Duration::minutes(5)
    } else {
        let consecutive = proxy.consecutive_fails + 1; // +1 for this failure
        let minutes = match consecutive {
            0..=1 => 1,
            2..=3 => 5,
            4..=5 => 15,
            6..=10 => 30,
            _ => 60,
        };
        now + Duration::minutes(minutes)
    }
}

/// Calculate health score (0-100)
fn calculate_score(
    proxy: &Proxy,
    current_success: bool,
    avg_latency: Option<f64>,
    cfg: &ScoringConfig,
) -> f64 {
    let total_checks = proxy.success_count + proxy.fail_count + 1;
    let successes = proxy.success_count + if current_success { 1 } else { 0 };

    // Success rate component (0-100)
    let success_rate = (successes as f64 / total_checks as f64) * 100.0;

    // Latency component (0-100) — lower latency = higher score
    let latency_score = match avg_latency.or(Some(proxy.avg_latency_ms)) {
        Some(ms) if ms > 0.0 => {
            if ms <= 100.0 {
                100.0
            } else if ms <= 500.0 {
                100.0 - ((ms - 100.0) / 400.0) * 60.0
            } else if ms <= 2000.0 {
                40.0 - ((ms - 500.0) / 1500.0) * 30.0
            } else {
                10.0
            }
        }
        _ => 50.0, // Unknown latency
    };

    // Stability component (0-100) — based on consecutive success
    let stability_score = if proxy.consecutive_fails == 0 && current_success {
        let uptime_bonus = (proxy.success_count as f64).min(100.0);
        50.0 + (uptime_bonus / 100.0) * 50.0
    } else {
        let penalty = (proxy.consecutive_fails as f64 * 15.0).min(100.0);
        (50.0 - penalty).max(0.0)
    };

    let score = success_rate * cfg.weight_success_rate
        + latency_score * cfg.weight_latency
        + stability_score * cfg.weight_stability;

    score.clamp(0.0, 100.0)
}

/// Try to detect proxy metadata (anonymity, protocol detection)
async fn detect_and_update_metadata(db: &Database, proxy: &Proxy) -> Result<()> {
    let proxy_addr = format!("{}:{}", proxy.ip, proxy.port);

    // Try to detect anonymity via httpbin
    let anonymity = detect_anonymity(&proxy_addr, &proxy.protocol).await
        .unwrap_or_else(|_| "unknown".to_string());

    // Detect protocol by testing different protocols
    let protocol = detect_protocol(&proxy.ip, proxy.port).await
        .unwrap_or_else(|_| proxy.protocol.clone());

    // Country detection — use the proxy's IP directly with a GeoIP API
    let country = if proxy.country == "unknown" {
        detect_country_by_ip(&proxy.ip).await
            .unwrap_or_else(|_| "unknown".to_string())
    } else {
        proxy.country.clone()
    };

    db.update_proxy_metadata(proxy.id, &country, &anonymity, &protocol)
        .await?;

    Ok(())
}

async fn detect_anonymity(proxy_addr: &str, protocol: &str) -> Result<String> {
    let proxy_url = format!("{}://{}", protocol, proxy_addr);
    let proxy = reqwest::Proxy::all(&proxy_url)?;

    let client = reqwest::Client::builder()
        .proxy(proxy)
        .timeout(std::time::Duration::from_secs(10))
        .danger_accept_invalid_certs(true)
        .build()?;

    let resp = client
        .get("https://httpbin.org/headers")
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;

    if let Some(headers) = resp.get("headers").and_then(|h| h.as_object()) {
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

async fn detect_protocol(ip: &str, port: u16) -> Result<String> {
    let addr = format!("{}:{}", ip, port);

    // Try SOCKS5
    let socks_proxy = reqwest::Proxy::all(format!("socks5://{}", addr))?;
    let client = reqwest::Client::builder()
        .proxy(socks_proxy)
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    if client.get("https://httpbin.org/ip").send().await.is_ok() {
        return Ok("socks5".to_string());
    }

    // Try HTTPS
    let https_proxy = reqwest::Proxy::all(format!("https://{}", addr))?;
    let client = reqwest::Client::builder()
        .proxy(https_proxy)
        .timeout(std::time::Duration::from_secs(5))
        .danger_accept_invalid_certs(true)
        .build()?;

    if client.get("https://httpbin.org/ip").send().await.is_ok() {
        return Ok("https".to_string());
    }

    Ok("http".to_string())
}

/// Detect country from the proxy's IP address using free GeoIP APIs (direct, not through proxy)
async fn detect_country_by_ip(ip: &str) -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()?;

    // Try ip-api.com first (free, no key required, 45 req/min)
    if let Ok(resp) = client
        .get(&format!("http://ip-api.com/json/{}?fields=status,countryCode", ip))
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
    if let Ok(resp) = client
        .get(&format!("https://ipwho.is/{}", ip))
        .send()
        .await
    {
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
