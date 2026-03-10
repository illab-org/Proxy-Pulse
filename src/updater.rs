use std::path::PathBuf;
use tokio::process::Command;
use tracing::{error, info, warn};

use crate::db::Database;

const REPO: &str = "OpenInfra-Labs/Proxy-Pulse";
const CHECK_INTERVAL_SECS: u64 = 3600; // Check every hour

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

            if enabled {
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

/// Fetch the latest release tag from GitHub API
pub async fn fetch_latest_version() -> anyhow::Result<String> {
    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        REPO
    );

    let client = reqwest::Client::builder()
        .user_agent("Proxy-Pulse-Updater")
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let resp = client.get(&url).send().await?;
    let json: serde_json::Value = resp.json().await?;

    json.get("tag_name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("No tag_name in GitHub response"))
}

/// Fetch all releases from GitHub API (tag, date, body)
pub async fn fetch_releases() -> anyhow::Result<Vec<serde_json::Value>> {
    let url = format!(
        "https://api.github.com/repos/{}/releases?per_page=50",
        REPO
    );

    let client = reqwest::Client::builder()
        .user_agent("Proxy-Pulse-Updater")
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let resp = client.get(&url).send().await?;
    let releases: Vec<serde_json::Value> = resp.json().await?;

    Ok(releases
        .into_iter()
        .filter_map(|r| {
            let tag = r.get("tag_name")?.as_str()?.to_string();
            let date = r.get("published_at")?.as_str()?.to_string();
            let body = r.get("body").and_then(|b| b.as_str()).unwrap_or("").to_string();
            Some(serde_json::json!({
                "version": tag,
                "date": date,
                "notes": body,
            }))
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
