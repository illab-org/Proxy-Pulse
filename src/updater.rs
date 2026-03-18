use chrono::Timelike;
use std::path::PathBuf;
use tokio::process::Command;
use tracing::{error, info, warn};

use crate::db::Database;

const REPO: &str = "illab-org/Proxy-Pulse";
const CHECK_INTERVAL_SECS: u64 = 30; // Check every 30 seconds

#[derive(Clone)]
struct ReleaseEntry {
    version: String,
    date: String,
}

/// Check if the release binary asset exists for the current platform
async fn release_has_binary(version: &str) -> bool {
    let (os_name, arch_name) = detect_platform();
    let ext = if os_name == "windows" {
        "zip"
    } else {
        "tar.gz"
    };
    let artifact = format!("proxy-pulse-{}-{}.{}", os_name, arch_name, ext);
    let url = format!(
        "https://github.com/{}/releases/download/{}/{}",
        REPO, version, artifact
    );

    let client = reqwest::Client::builder()
        .user_agent("Proxy-Pulse-Updater")
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build();

    let Ok(client) = client else { return false };
    // GitHub returns 302 redirect for existing assets, 404 for missing
    match client.head(&url).send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            status == 200 || status == 302
        }
        Err(_) => false,
    }
}

fn detect_platform() -> (&'static str, &'static str) {
    let os = if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "unknown"
    };
    let arch = if cfg!(target_arch = "x86_64") {
        "amd64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "unknown"
    };
    (os, arch)
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

    let hour = chrono::Local::now().hour();
    match schedule.as_str() {
        "anytime" => true,
        "night" => hour < 6,                     // 00:00–06:00
        "morning" => (6..12).contains(&hour),    // 06:00–12:00
        "afternoon" => (12..18).contains(&hour), // 12:00–18:00
        "evening" => hour >= 18,                 // 18:00–00:00
        _ => true,
    }
}

/// Check GitHub for a newer version and trigger update if available (auto-update: skip if no binary)
pub async fn check_and_update() -> anyhow::Result<bool> {
    check_and_update_inner(false).await
}

/// Manual update: returns error if binary not yet available
pub async fn manual_update() -> anyhow::Result<bool> {
    check_and_update_inner(true).await
}

/// Update to a specific version (rollback or skip-ahead)
pub async fn update_to_version(version: &str) -> anyhow::Result<bool> {
    let current_version = env!("CARGO_PKG_VERSION");
    let target = version.trim_start_matches('v');

    if target == current_version {
        return Ok(false);
    }

    let tag = if version.starts_with('v') {
        version.to_string()
    } else {
        format!("v{}", version)
    };

    if !release_has_binary(&tag).await {
        return Err(anyhow::anyhow!("BINARY_NOT_READY"));
    }

    info!(
        current = current_version,
        target = target,
        "Updating to specific version"
    );

    if is_docker() {
        download_and_replace_binary(&tag).await?;
        info!("Binary updated in Docker container, exiting for restart");
        std::process::exit(0);
    }

    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    let run_script = find_or_download_run_script(&exe_dir).await?;
    let status: std::process::ExitStatus = Command::new(&run_script)
        .env("PP_VERSION", &tag)
        .status()
        .await?;

    if !status.success() {
        error!(code = ?status.code(), "Run script exited with error");
    }

    Ok(true)
}

async fn check_and_update_inner(manual: bool) -> anyhow::Result<bool> {
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

    // Check if binary asset exists for this platform
    let tag = if latest_tag.starts_with('v') {
        latest_tag.clone()
    } else {
        format!("v{}", latest_tag)
    };
    if !release_has_binary(&tag).await {
        if manual {
            return Err(anyhow::anyhow!("BINARY_NOT_READY"));
        }
        info!(version = %latest_tag, "Binary not yet available, skipping auto-update");
        return Ok(false);
    }

    info!(
        current = current_version,
        latest = latest_version,
        "New version available, triggering update"
    );

    if is_docker() {
        // Docker: download binary directly, replace in-place, then exit.
        // Docker's restart policy will restart the container with the new binary.
        download_and_replace_binary(&tag).await?;
        info!("Binary updated in Docker container, exiting for restart");
        std::process::exit(0);
    }

    // Non-Docker: use the run script
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

/// Detect if running inside a Docker container
fn is_docker() -> bool {
    // Most reliable: /.dockerenv exists in Docker containers
    if std::path::Path::new("/.dockerenv").exists() {
        return true;
    }
    // Fallback: check cgroup for "docker" or "containerd"
    if let Ok(cgroup) = std::fs::read_to_string("/proc/1/cgroup") {
        if cgroup.contains("docker") || cgroup.contains("containerd") {
            return true;
        }
    }
    // Fallback: check for container environment variable
    if std::env::var("container").is_ok() {
        return true;
    }
    false
}

/// Download the latest binary and replace the current executable (for Docker updates)
async fn download_and_replace_binary(tag: &str) -> anyhow::Result<()> {
    let (os_name, arch_name) = detect_platform();
    let ext = if os_name == "windows" {
        "zip"
    } else {
        "tar.gz"
    };
    let artifact = format!("proxy-pulse-{}-{}", os_name, arch_name);
    let pkg_file = format!("{}.{}", artifact, ext);
    let url = format!(
        "https://github.com/{}/releases/download/{}/{}",
        REPO, tag, pkg_file
    );

    info!(url = %url, "Downloading update binary");

    let client = reqwest::Client::builder()
        .user_agent("Proxy-Pulse-Updater")
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() && resp.status().as_u16() != 302 {
        return Err(anyhow::anyhow!(
            "Failed to download binary: HTTP {}",
            resp.status()
        ));
    }
    let bytes = resp.bytes().await?;

    let exe_path = std::env::current_exe()?;
    let exe_dir = exe_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let pkg_path = exe_dir.join(&pkg_file);

    // Write archive
    tokio::fs::write(&pkg_path, &bytes).await?;

    // Extract — for tar.gz, extract then move binary into place
    let output = Command::new("tar")
        .args([
            "xzf",
            &pkg_path.to_string_lossy(),
            "-C",
            &exe_dir.to_string_lossy(),
        ])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("Failed to extract archive: {}", stderr));
    }

    // The archive extracts to a file named like "proxy-pulse-linux-amd64"
    let extracted = exe_dir.join(&artifact);
    if extracted.exists() {
        tokio::fs::rename(&extracted, &exe_path).await?;
    }

    // Clean up archive
    let _ = tokio::fs::remove_file(&pkg_path).await;

    info!(path = %exe_path.display(), "Binary replaced successfully");
    Ok(())
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
            format!("https://raw.githubusercontent.com/{}/main/run.ps1", REPO),
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
