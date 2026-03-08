use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing::{error, info};

use crate::config::AppConfig;
use crate::db::Database;
use crate::sources;
use crate::checker;

/// Start all background scheduler tasks
pub async fn start_schedulers(db: Database, config: Arc<AppConfig>) {
    let db_source = db.clone();
    let config_source = config.clone();

    // Subscription auto-sync scheduler (FIRST PRIORITY — runs every 60s, checks per-source intervals)
    tokio::spawn(async move {
        info!("Starting subscription auto-sync scheduler (per-source intervals)");

        // Run all enabled subscriptions immediately on startup
        match sources::sync_subscription_sources(&db_source).await {
            Ok(count) => {
                info!(count = count, "Initial subscription sync complete");
                if count > 0 {
                    let db2 = db_source.clone();
                    let cfg2 = config_source.clone();
                    tokio::spawn(async move {
                        match checker::run_check_cycle(&db2, &cfg2.checker, &cfg2.scoring).await {
                            Ok((s, f)) => info!(success = s, fail = f, "Post-sync check cycle complete"),
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
                    info!(count = due_sources.len(), "Subscription sources due for auto-sync");
                    let mut synced_total = 0usize;
                    for source in &due_sources {
                        match sources::sync_single_subscription(&db_source, source).await {
                            Ok(count) => {
                                let _ = db_source.update_subscription_sync_result(
                                    source.id, count as i64, None,
                                ).await;
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
                                let _ = db_source.update_subscription_sync_result(
                                    source.id, 0, Some(&e.to_string()),
                                ).await;
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
                        let cfg2 = config_source.clone();
                        tokio::spawn(async move {
                            match checker::run_check_cycle(&db2, &cfg2.checker, &cfg2.scoring).await {
                                Ok((s, f)) => info!(success = s, fail = f, "Post-autosync check complete"),
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

    let db_provider = db.clone();
    let config_provider = config.clone();

    // Provider source sync scheduler (config-based sources, lower priority)
    tokio::spawn(async move {
        let interval_secs = config_provider.sources.sync_interval_secs;
        info!(interval_secs = interval_secs, "Starting provider source sync scheduler");

        // Run immediately on startup
        match sources::sync_sources(&db_provider, &config_provider.sources.providers).await {
            Ok(count) => info!(count = count, "Initial provider source sync complete"),
            Err(e) => error!(error = %e, "Initial provider source sync failed"),
        }

        let mut ticker = interval(Duration::from_secs(interval_secs));
        ticker.tick().await; // Skip immediate tick

        loop {
            ticker.tick().await;
            match sources::sync_sources(&db_provider, &config_provider.sources.providers).await {
                Ok(count) => info!(count = count, "Provider source sync complete"),
                Err(e) => error!(error = %e, "Provider source sync failed"),
            }
        }
    });

    let db_checker = db.clone();
    let config_checker = config.clone();

    // Proxy checker scheduler
    tokio::spawn(async move {
        let interval_secs = config_checker.checker.interval_secs;
        info!(interval_secs = interval_secs, "Starting proxy checker scheduler");

        // Wait a bit for initial source sync to populate proxies
        tokio::time::sleep(Duration::from_secs(5)).await;

        let mut ticker = interval(Duration::from_secs(interval_secs));

        loop {
            ticker.tick().await;
            match checker::run_check_cycle(
                &db_checker,
                &config_checker.checker,
                &config_checker.scoring,
            )
            .await
            {
                Ok((s, f)) => info!(success = s, fail = f, "Check cycle complete"),
                Err(e) => error!(error = %e, "Check cycle failed"),
            }
        }
    });

    let db_cleanup = db.clone();

    // Log cleanup scheduler (daily)
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(86400)); // 24 hours

        loop {
            ticker.tick().await;
            match db_cleanup.cleanup_old_logs(7).await {
                Ok(count) => info!(deleted = count, "Old check logs cleaned up"),
                Err(e) => error!(error = %e, "Log cleanup failed"),
            }
        }
    });
}
