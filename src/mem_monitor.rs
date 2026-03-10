//! Memory monitoring — logs per-category breakdown every N seconds.
//!
//! Uses jemalloc's `stats.allocated` / `stats.resident` / `stats.mapped` for
//! allocator-level stats, plus `/proc/self/status` for OS-level RSS on Linux.

use tokio::time::{interval, Duration};
use tracing::info;

/// Readable byte formatting
fn fmt_bytes(b: usize) -> String {
    const MB: usize = 1024 * 1024;
    if b >= MB {
        format!("{:.1} MB", b as f64 / MB as f64)
    } else {
        format!("{:.1} KB", b as f64 / 1024.0)
    }
}

/// Read key memory fields from `/proc/self/status` (Linux only).
/// Returns (VmRSS, VmHWM, VmSize, RssAnon, RssFile, RssShmem) in bytes.
#[cfg(target_os = "linux")]
fn read_proc_status() -> Option<ProcMemInfo> {
    let content = std::fs::read_to_string("/proc/self/status").ok()?;
    let mut info = ProcMemInfo::default();
    for line in content.lines() {
        let mut parts = line.split_whitespace();
        let key = parts.next()?;
        let val: usize = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
        let val_bytes = val * 1024; // /proc/self/status reports in kB
        match key {
            "VmRSS:" => info.vm_rss = val_bytes,
            "VmHWM:" => info.vm_hwm = val_bytes,
            "VmSize:" => info.vm_size = val_bytes,
            "RssAnon:" => info.rss_anon = val_bytes,
            "RssFile:" => info.rss_file = val_bytes,
            "RssShmem:" => info.rss_shmem = val_bytes,
            _ => {}
        }
    }
    Some(info)
}

#[cfg(target_os = "linux")]
#[derive(Default)]
struct ProcMemInfo {
    vm_rss: usize,
    vm_hwm: usize,
    vm_size: usize,
    rss_anon: usize,
    rss_file: usize,
    rss_shmem: usize,
}

/// Read jemalloc epoch + stats via `tikv_jemalloc_ctl`.
#[cfg(not(target_env = "msvc"))]
fn read_jemalloc_stats() -> Option<JemallocStats> {
    use tikv_jemalloc_ctl::{epoch, stats};

    // Advance jemalloc's stats epoch to get fresh numbers.
    epoch::advance().ok()?;

    Some(JemallocStats {
        allocated: stats::allocated::read().ok()?,
        active: stats::active::read().ok()?,
        metadata: stats::metadata::read().ok()?,
        resident: stats::resident::read().ok()?,
        mapped: stats::mapped::read().ok()?,
        retained: stats::retained::read().ok()?,
    })
}

#[cfg(not(target_env = "msvc"))]
struct JemallocStats {
    /// Bytes currently allocated by the application.
    allocated: usize,
    /// Bytes in active pages (allocated + internal fragmentation).
    active: usize,
    /// Bytes used by jemalloc metadata.
    metadata: usize,
    /// Bytes mapped into the process (≈ RSS contribution from allocator).
    resident: usize,
    /// Total bytes mapped via mmap.
    mapped: usize,
    /// Bytes retained (virtual but not yet returned to OS).
    retained: usize,
}

/// Spawn the memory monitor background task.
/// Logs every `interval_secs` (default 1).
/// Also runs a periodic jemalloc purge every 30 seconds to proactively reclaim memory.
pub fn spawn_monitor(interval_secs: u64) {
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(interval_secs));
        let mut purge_counter: u64 = 0;
        loop {
            tick.tick().await;
            purge_counter += 1;
            log_memory_stats();

            // Periodic purge every 30 seconds
            if purge_counter % 30 == 0 {
                purge_jemalloc();
            }
        }
    });
}

/// Force jemalloc to return all unused dirty/muzzy pages to the OS immediately.
/// Called after check cycles and periodically by the monitor.
#[cfg(not(target_env = "msvc"))]
pub fn purge_jemalloc() {
    use tikv_jemalloc_ctl::raw;
    // "arena.4096.purge" = MALLCTL_ARENAS_ALL purge (4096 is the special "all arenas" index)
    let key = b"arena.4096.purge\0";
    unsafe {
        let _ = raw::write(key, 0u64);
    }
}

#[cfg(target_env = "msvc")]
pub fn purge_jemalloc() {
    // No-op on MSVC (no jemalloc)
}

fn log_memory_stats() {
    let mut parts: Vec<String> = Vec::new();

    // ── jemalloc stats ──
    #[cfg(not(target_env = "msvc"))]
    if let Some(je) = read_jemalloc_stats() {
        let frag = if je.active > 0 {
            ((je.active - je.allocated) as f64 / je.active as f64) * 100.0
        } else {
            0.0
        };
        parts.push(format!(
            "jemalloc [ allocated: {} | active: {} | metadata: {} | resident: {} | mapped: {} | retained: {} | frag: {:.1}% ]",
            fmt_bytes(je.allocated),
            fmt_bytes(je.active),
            fmt_bytes(je.metadata),
            fmt_bytes(je.resident),
            fmt_bytes(je.mapped),
            fmt_bytes(je.retained),
            frag,
        ));
    }

    // ── Linux /proc/self/status ──
    #[cfg(target_os = "linux")]
    if let Some(pm) = read_proc_status() {
        parts.push(format!(
            "OS [ RSS: {} (anon: {} + file: {} + shmem: {}) | HWM: {} | VMSize: {} ]",
            fmt_bytes(pm.vm_rss),
            fmt_bytes(pm.rss_anon),
            fmt_bytes(pm.rss_file),
            fmt_bytes(pm.rss_shmem),
            fmt_bytes(pm.vm_hwm),
            fmt_bytes(pm.vm_size),
        ));
    }

    // ── macOS (fallback — no /proc) ──
    #[cfg(target_os = "macos")]
    {
        // On macOS we only have jemalloc stats above; add a note.
        if parts.is_empty() {
            parts.push("(no OS-level memory info on macOS, jemalloc stats only)".into());
        }
    }

    if !parts.is_empty() {
        info!("[MEM] {}", parts.join(" | "));
    }
}
