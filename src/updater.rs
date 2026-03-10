use std::path::PathBuf;
use chrono::Timelike;
use tokio::process::Command;
use tracing::{error, info, warn};

use crate::db::Database;

const REPO: &str = "OpenInfra-Labs/Proxy-Pulse";
const CHECK_INTERVAL_SECS: u64 = 30; // Check every 30 seconds

#[derive(Clone)]
struct ReleaseEntry {
    version: String,
    date: String,
}

/// Spawn the auto-update background task
pub fn spawn_auto_updater(db: Database) {
    tokio::spawn(async move {
        // Wait 30 seconds after startup before first check
        tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

        loop {
            // Check if auto-update is enabled
            let enabled = db
                .get_setting("system.auto_update")
                .await
                .ok()
                .flatten()
                .map(|v| v != "false")
                .unwrap_or(true); // default: enabled

            if enabled && is_within_schedule(&db).await {
                match check_and_update().await {
                    Ok(true) => info!("Auto-update triggered, process will restart via run script"),
                    Ok(false) => info!("No update available"),
                    Err(e) => warn!(error = %e, "Auto-update check failed"),
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(CHECK_INTERVAL_SECS)).await;
        }
    });
}

/// Check if the current time is within the configured install schedule
async fn is_within_schedule(db: &Database) -> bool {
    let schedule = db
        .get_setting("system.install_schedule")
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "anytime".to_string());

    match schedule.as_str() {
        "anytime" => true,
        "night" => {
            let hour = chrono::Local::now().hour();
            hour < 6 // 00:00–06:00
        }
        "custom" => {
            let from = db
                .get_setting("system.install_schedule_from")
                .await
                .ok()
                .flatten()
                .unwrap_or_else(|| "02:00".to_string());
            let to = db
                .get_setting("system.install_schedule_to")
                .await
                .ok()
                .flatten()
                .unwrap_or_else(|| "06:00".to_string());
            is_time_in_range(&from, &to)
        }
        _ => true,
    }
}

fn parse_hm(s: &str) -> Option<(u32, u32)> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() == 2 {
        Some((parts[0].parse().ok()?, parts[1].parse().ok()?))
    } else {
        None
    }
}

fn is_time_in_range(from: &str, to: &str) -> bool {
    let now = chrono::Local::now();
    let current = now.hour() * 60 + now.minute();
    let Some((fh, fm)) = parse_hm(from) else { return true };
    let Some((th, tm)) = parse_hm(to) else { return true };
    let start = fh * 60 + fm;
    let end = th * 60 + tm;
    if start <= end {
        current >= start && current < end
    } else {
        // Wraps midnight, e.g. 22:00–06:00
        current >= start || current < end
    }
}

/// Check GitHub for a newer version and trigger update if available
pub async fn check_and_update() -> anyhow::Result<bool> {
    let current_version = env!("CARGO_PKG_VERSION");
    let latest_tag = fetch_latest_version().await?;
    let latest_version = latest_tag.trim_start_matches('v');

    info!(
        current = current_version,
        latest = latest_version,
        "Version check"
    );

    if !is_newer(latest_version, current_version) {
        return Ok(false);
    }

    info!(
        current = current_version,
        latest = latest_version,
        "New version available, triggering update"
    );

    // Find run script in the binary's directory
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    let run_script = find_or_download_run_script(&exe_dir).await?;

    // Execute the run script (it will stop this process and start the new one)
    let status: std::process::ExitStatus = Command::new(&run_script).status().await?;

    if !status.success() {
        error!(
            code = ?status.code(),
            "Run script exited with error"
        );
    }

    Ok(true)
}

/// Fetch releases from GitHub Atom feed (not subject to API rate limits)
async fn fetch_atom_releases() -> anyhow::Result<Vec<ReleaseEntry>> {
    let url = format!("https://github.com/{}/releases.atom", REPO);
    let client = reqwest::Client::builder()
        .user_agent("Proxy-Pulse-Updater")
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let text = client.get(&url).send().await?.text().await?;
    let entries = parse_atom_entries(&text);

    if entries.is_empty() {
        return Err(anyhow::anyhow!("No releases found in Atom feed"));
    }

    Ok(entries)
}

fn parse_atom_entries(xml: &str) -> Vec<ReleaseEntry> {
    let mut entries = Vec::new();
    let mut rest = xml;

    while let Some(start) = rest.find("<entry>") {
        let after_start = &rest[start..];
        if let Some(end) = after_start.find("</entry>") {
            let entry_xml = &after_start[..end + 8];

            let version = extract_xml_tag(entry_xml, "title").unwrap_or_default();
            let date = extract_xml_tag(entry_xml, "updated").unwrap_or_default();

            if !version.is_empty() {
                entries.push(ReleaseEntry { version, date });
            }

            rest = &after_start[end + 8..];
        } else {
            break;
        }
    }

    entries
}

fn extract_xml_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);

    let start_pos = xml.find(&open)?;
    let after_open = &xml[start_pos..];
    let content_start = after_open.find('>')? + 1;
    let content = &after_open[content_start..];
    let end_pos = content.find(&close)?;

    Some(content[..end_pos].trim().to_string())
}

/// Fetch the latest release version from GitHub Atom feed
pub async fn fetch_latest_version() -> anyhow::Result<String> {
    let entries = fetch_atom_releases().await?;
    entries
        .first()
        .map(|e| e.version.clone())
        .ok_or_else(|| anyhow::anyhow!("No releases found"))
}

/// Fetch all releases from GitHub Atom feed
pub async fn fetch_releases() -> anyhow::Result<Vec<serde_json::Value>> {
    let entries = fetch_atom_releases().await?;
    Ok(entries
        .into_iter()
        .map(|e| {
            serde_json::json!({
                "version": e.version,
                "date": e.date,
                "notes": "",
            })
        })
        .collect())
}

/// Compare two semver strings, return true if `latest` > `current`
pub fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |v: &str| -> (u32, u32, u32) {
        let parts: Vec<u32> = v.split('.').filter_map(|p| p.parse().ok()).collect();
        (
            parts.first().copied().unwrap_or(0),
            parts.get(1).copied().unwrap_or(0),
            parts.get(2).copied().unwrap_or(0),
        )
    };
    parse(latest) > parse(current)
}

/// Find the run script in the exe directory, or download it from GitHub
async fn find_or_download_run_script(exe_dir: &PathBuf) -> anyhow::Result<PathBuf> {
    // Check for existing run script
    let run_path = if cfg!(windows) {
        exe_dir.join("run.ps1")
    } else {
        exe_dir.join("run")
    };

    if run_path.exists() {
        info!(path = %run_path.display(), "Found existing run script");
        return Ok(run_path);
    }

    // Download run script from GitHub main branch
    let (script_url, script_name) = if cfg!(windows) {
        (
            format!(
                "https://raw.githubusercontent.com/{}/main/run.ps1",
                REPO
            ),
            "run.ps1",
        )
    } else {
        (
            format!("https://raw.githubusercontent.com/{}/main/run", REPO),
            "run",
        )
    };

    info!(url = %script_url, "Downloading run script");

    let client = reqwest::Client::builder()
        .user_agent("Proxy-Pulse-Updater")
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let resp = client.get(&script_url).send().await?;
    let content = resp.bytes().await?;

    let dest = exe_dir.join(script_name);
    tokio::fs::write(&dest, &content).await?;

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = tokio::fs::metadata(&dest).await?.permissions();
        perms.set_mode(0o755);
        tokio::fs::set_permissions(&dest, perms).await?;
    }

    info!(path = %dest.display(), "Run script downloaded");
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_newer() {
        assert!(is_newer("1.2.0", "1.1.2"));
        assert!(is_newer("2.0.0", "1.9.9"));
        assert!(is_newer("1.1.3", "1.1.2"));
        assert!(!is_newer("1.1.2", "1.1.2"));
        assert!(!is_newer("1.1.1", "1.1.2"));
        assert!(!is_newer("1.0.0", "1.1.2"));
    }
}
