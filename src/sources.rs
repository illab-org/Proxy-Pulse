use anyhow::Result;
use tracing::{info, warn};

use crate::config::ProviderConfig;
use crate::db::Database;

/// Parsed proxy entry from a source
#[derive(Debug, Clone)]
pub struct RawProxy {
    pub ip: String,
    pub port: u16,
    pub protocol: String,
}

/// Synchronize all configured proxy sources
pub async fn sync_sources(db: &Database, providers: &[ProviderConfig]) -> Result<usize> {
    let mut total = 0;

    for provider in providers {
        match provider.provider_type.as_str() {
            "file" => {
                if let Some(path) = &provider.path {
                    match load_from_file(path).await {
                        Ok(proxies) => {
                            let count = import_proxies(db, &proxies, &format!("file:{}", path)).await?;
                            info!(source = %path, count = count, "Synced file source");
                            total += count;
                        }
                        Err(e) => warn!(source = %path, error = %e, "Failed to load file source"),
                    }
                }
            }
            "url" => {
                if let Some(url) = &provider.url {
                    match load_from_url(url).await {
                        Ok(proxies) => {
                            let count = import_proxies(db, &proxies, &format!("url:{}", url)).await?;
                            info!(source = %url, count = count, "Synced URL source");
                            total += count;
                        }
                        Err(e) => warn!(source = %url, error = %e, "Failed to load URL source"),
                    }
                }
            }
            other => {
                warn!(provider_type = %other, "Unknown provider type, skipping");
            }
        }
    }

    Ok(total)
}

/// Load proxies from a local file
async fn load_from_file(path: &str) -> Result<Vec<RawProxy>> {
    let content = tokio::fs::read_to_string(path).await?;
    Ok(parse_proxy_list(&content))
}

/// Load proxies from a remote URL
async fn load_from_url(url: &str) -> Result<Vec<RawProxy>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let resp = client.get(url).send().await?;
    let content = resp.text().await?;
    Ok(parse_proxy_list(&content))
}

/// Parse a proxy list text (one proxy per line)
/// Supported formats:
///   ip:port
///   protocol://ip:port
///   ip:port:protocol
pub fn parse_proxy_list(content: &str) -> Vec<RawProxy> {
    let mut proxies = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }

        if let Some(proxy) = parse_proxy_line(line) {
            proxies.push(proxy);
        }
    }

    proxies
}

fn parse_proxy_line(line: &str) -> Option<RawProxy> {
    // Try protocol://ip:port format
    if line.contains("://") {
        let parts: Vec<&str> = line.splitn(2, "://").collect();
        if parts.len() == 2 {
            let protocol = parts[0].to_lowercase();
            let addr_parts: Vec<&str> = parts[1].splitn(2, ':').collect();
            if addr_parts.len() == 2 {
                let ip = addr_parts[0].to_string();
                if let Ok(port) = addr_parts[1].parse::<u16>() {
                    return Some(RawProxy { ip, port, protocol });
                }
            }
        }
    }

    // Try ip:port or ip:port:protocol format
    let parts: Vec<&str> = line.split(':').collect();
    match parts.len() {
        2 => {
            let ip = parts[0].to_string();
            if let Ok(port) = parts[1].parse::<u16>() {
                return Some(RawProxy {
                    ip,
                    port,
                    protocol: "http".to_string(),
                });
            }
        }
        3 => {
            let ip = parts[0].to_string();
            if let Ok(port) = parts[1].parse::<u16>() {
                let protocol = parts[2].to_lowercase();
                return Some(RawProxy { ip, port, protocol });
            }
        }
        _ => {}
    }

    None
}

/// Import parsed proxies into the database (deduplication via upsert)
pub async fn import_proxies(db: &Database, proxies: &[RawProxy], source: &str) -> Result<usize> {
    let mut count = 0;
    for proxy in proxies {
        db.upsert_proxy(&proxy.ip, proxy.port, &proxy.protocol, source)
            .await?;
        count += 1;
    }
    Ok(count)
}

/// Import proxies with an explicit protocol hint override
/// If protocol_hint is "auto", use the protocol parsed from the line
pub async fn import_proxies_with_hint(
    db: &Database,
    proxies: &[RawProxy],
    source: &str,
    protocol_hint: &str,
) -> Result<usize> {
    let mut count = 0;
    for proxy in proxies {
        let protocol = if protocol_hint == "auto" {
            &proxy.protocol
        } else {
            protocol_hint
        };
        db.upsert_proxy(&proxy.ip, proxy.port, protocol, source)
            .await?;
        count += 1;
    }
    Ok(count)
}

/// Synchronize subscription sources from the database
pub async fn sync_subscription_sources(db: &Database) -> Result<usize> {
    let sources = db.get_enabled_subscription_sources().await?;
    let mut total = 0;

    for source in &sources {
        let result = sync_single_subscription(db, source).await;
        match result {
            Ok(count) => {
                db.update_subscription_sync_result(source.id, count as i64, None).await?;
                info!(source_id = source.id, name = %source.name, count = count, "Subscription synced");
                total += count;
            }
            Err(e) => {
                db.update_subscription_sync_result(source.id, 0, Some(&e.to_string())).await?;
                warn!(source_id = source.id, name = %source.name, error = %e, "Subscription sync failed");
            }
        }
    }

    Ok(total)
}

pub async fn sync_single_subscription(
    db: &Database,
    source: &crate::models::SubscriptionSource,
) -> Result<usize> {
    let proxies = match source.source_type.as_str() {
        "url" => {
            let url = source.url.as_deref().ok_or_else(|| anyhow::anyhow!("URL is empty"))?;
            load_from_url(url).await?
        }
        "text" => {
            let content = source.content.as_deref().unwrap_or("");
            parse_proxy_list(content)
        }
        _ => return Err(anyhow::anyhow!("Unknown source type: {}", source.source_type)),
    };

    let source_tag = format!("sub:{}:{}", source.id, source.name);
    let count = import_proxies_with_hint(db, &proxies, &source_tag, &source.protocol_hint).await?;
    Ok(count)
}
