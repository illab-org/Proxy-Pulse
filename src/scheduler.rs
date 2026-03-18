use tokio::time::{interval, Duration};
use tracing::{error, info};

use crate::checker;
use crate::db::Database;

/// Start all background scheduler tasks
pub async fn start_schedulers(db: Database) {
    let db_source = db.clone();

    // Subscription auto-sync scheduler (runs every 60s, checks per-source intervals)
    tokio::spawn(async move {
        info!("Starting subscription auto-sync scheduler (per-source intervals)");

        // Run all enabled subscriptions immediately on startup
        match crate::sources::sync_subscription_sources(&db_source).await {
            Ok(count) => {
                info!(count = count, "Initial subscription sync complete");
                if count > 0 {
                    let db2 = db_source.clone();
                    tokio::spawn(async move {
                        let cfg = db2.get_checker_config().await;
                        match checker::run_check_cycle(&db2, &cfg).await {
                            Ok((s, f)) => {
                                info!(success = s, fail = f, "Post-sync check cycle complete")
                            }
                            Err(e) => error!(error = %e, "Post-sync check cycle failed"),
                        }
                    });
                }
            }
            Err(e) => error!(error = %e, "Initial subscription sync failed"),
        }

        // Check every 60 seconds which sources are due for sync
        let mut ticker = interval(Duration::from_secs(60));
        ticker.tick().await; // Skip immediate tick (already ran above)

        loop {
            ticker.tick().await;

            // Get sources that are due for sync based on their individual intervals
            match db_source.get_sources_due_for_sync().await {
                Ok(due_sources) if !due_sources.is_empty() => {
                    info!(
                        count = due_sources.len(),
                        "Subscription sources due for auto-sync"
                    );
                    let mut synced_total = 0usize;
                    for source in &due_sources {
                        match crate::sources::sync_single_subscription(&db_source, source).await {
                            Ok(count) => {
                                let _ = db_source
                                    .update_subscription_sync_result(source.id, count as i64, None)
                                    .await;
                                info!(
                                    source_id = source.id,
                                    name = %source.name,
                                    count = count,
                                    interval_secs = source.sync_interval_secs,
                                    "Subscription auto-synced"
                                );
                                synced_total += count;
                            }
                            Err(e) => {
                                let _ = db_source
                                    .update_subscription_sync_result(
                                        source.id,
                                        0,
                                        Some(&e.to_string()),
                                    )
                                    .await;
                                error!(
                                    source_id = source.id,
                                    name = %source.name,
                                    error = %e,
                                    "Subscription auto-sync failed"
                                );
                            }
                        }
                    }
                    // Trigger immediate check after syncing new proxies
                    if synced_total > 0 {
                        let db2 = db_source.clone();
                        tokio::spawn(async move {
                            let cfg = db2.get_checker_config().await;
                            match checker::run_check_cycle(&db2, &cfg).await {
                                Ok((s, f)) => {
                                    info!(success = s, fail = f, "Post-autosync check complete")
                                }
                                Err(e) => error!(error = %e, "Post-autosync check failed"),
                            }
                        });
                    }
                }
                Ok(_) => {} // No sources due
                Err(e) => error!(error = %e, "Failed to query sources due for sync"),
            }
        }
    });

    let db_checker = db.clone();

    // Proxy checker scheduler — 1s tick scans due queue only.
    tokio::spawn(async move {
        info!("Starting proxy checker scheduler");

        // Wait a bit for initial source sync to populate proxies
        tokio::time::sleep(Duration::from_secs(5)).await;

        let mut ticker = interval(Duration::from_secs(1));
        ticker.tick().await;

        loop {
            ticker.tick().await;

            // Re-read config from DB each cycle so admin changes take effect immediately
            let cfg = db_checker.get_checker_config().await;

            match checker::run_check_cycle(&db_checker, &cfg).await {
                Ok((s, f)) => info!(success = s, fail = f, "Check cycle complete"),
                Err(e) => error!(error = %e, "Check cycle failed"),
            }
        }
    });

    let db_cleanup = db.clone();

    // Log cleanup scheduler (every 6 hours)
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(21600)); // 6 hours

        loop {
            ticker.tick().await;
            match db_cleanup.cleanup_old_logs(3).await {
                Ok(count) if count > 0 => info!(deleted = count, "Old check logs cleaned up"),
                _ => {}
            }
            // Cap total check logs to prevent unbounded growth
            match db_cleanup.cap_check_logs(50_000).await {
                Ok(count) if count > 0 => info!(deleted = count, "Check logs capped"),
                _ => {}
            }
            // Clean up expired sessions
            match db_cleanup.cleanup_expired_sessions().await {
                Ok(count) if count > 0 => info!(deleted = count, "Expired sessions cleaned up"),
                _ => {}
            }
        }
    });
}
