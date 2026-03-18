mod auth;
mod proxy;
mod stats;
mod subscription;

use anyhow::Result;
use semver::Version;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::str::FromStr;
use tracing::info;

use crate::config::CheckerConfig;

#[derive(Clone)]
pub struct Database {
    pub pool: SqlitePool,
}

impl Database {
    pub async fn new(url: &str) -> Result<Self> {
        let opts = SqliteConnectOptions::from_str(url)?
            .create_if_missing(true)
            .pragma("journal_mode", "WAL")
            .pragma("synchronous", "NORMAL")
            .pragma("cache_size", "-2000")
            .pragma("mmap_size", "0")
            .pragma("journal_size_limit", "67108864");

        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .min_connections(1)
            .idle_timeout(std::time::Duration::from_secs(60))
            .connect_with(opts)
            .await?;

        let db = Self { pool };
        db.run_migrations().await?;
        Ok(db)
    }

    async fn run_migrations(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS proxies (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ip TEXT NOT NULL,
                port INTEGER NOT NULL,
                protocol TEXT NOT NULL DEFAULT 'http',
                anonymity TEXT NOT NULL DEFAULT 'unknown',
                country TEXT NOT NULL DEFAULT 'unknown',
                score REAL NOT NULL DEFAULT 0.0,
                is_alive INTEGER NOT NULL DEFAULT 0,
                success_count INTEGER NOT NULL DEFAULT 0,
                fail_count INTEGER NOT NULL DEFAULT 0,
                consecutive_fails INTEGER NOT NULL DEFAULT 0,
                avg_latency_ms REAL NOT NULL DEFAULT 0.0,
                last_check_at TEXT,
                last_success_at TEXT,
                next_check_at TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                source TEXT NOT NULL DEFAULT 'unknown',
                subscription_id INTEGER,
                group_name TEXT NOT NULL DEFAULT 'default',
                UNIQUE(ip, port, protocol)
            );
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Migration: add subscription_id to proxies if missing
        let _ = sqlx::query("ALTER TABLE proxies ADD COLUMN subscription_id INTEGER")
            .execute(&self.pool)
            .await;

        // Migration: add group_name to proxies if missing
        let _ = sqlx::query(
            "ALTER TABLE proxies ADD COLUMN group_name TEXT NOT NULL DEFAULT 'default'",
        )
        .execute(&self.pool)
        .await;

        // Group registry table for admin management
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS proxy_groups (
                name TEXT PRIMARY KEY,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query("INSERT OR IGNORE INTO proxy_groups (name) VALUES ('default')")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS check_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                proxy_id INTEGER NOT NULL,
                target TEXT NOT NULL,
                success INTEGER NOT NULL DEFAULT 0,
                latency_ms REAL,
                error TEXT,
                checked_at TEXT NOT NULL DEFAULT (datetime('now')),
                FOREIGN KEY (proxy_id) REFERENCES proxies(id) ON DELETE CASCADE
            );
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_proxies_alive ON proxies(is_alive);")
            .execute(&self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_proxies_score ON proxies(score DESC);")
            .execute(&self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_proxies_country ON proxies(country);")
            .execute(&self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_proxies_next_check ON proxies(next_check_at);")
            .execute(&self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_proxies_subscription ON proxies(subscription_id);")
            .execute(&self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_proxies_group ON proxies(group_name);")
            .execute(&self.pool)
            .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_check_logs_proxy ON check_logs(proxy_id, checked_at DESC);",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_check_logs_checked_at ON check_logs(checked_at);",
        )
        .execute(&self.pool)
        .await?;

        // Subscription sources table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS subscription_sources (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                source_type TEXT NOT NULL DEFAULT 'url',
                url TEXT,
                content TEXT,
                protocol_hint TEXT NOT NULL DEFAULT 'auto',
                group_name TEXT NOT NULL DEFAULT 'default',
                is_enabled INTEGER NOT NULL DEFAULT 1,
                sync_interval_secs INTEGER NOT NULL DEFAULT 21600,
                proxy_count INTEGER NOT NULL DEFAULT 0,
                last_sync_at TEXT,
                last_error TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Migration: add group_name to subscription_sources if missing
        let _ = sqlx::query(
            "ALTER TABLE subscription_sources ADD COLUMN group_name TEXT NOT NULL DEFAULT 'default'",
        )
        .execute(&self.pool)
        .await;

        let _ = sqlx::query(
            "UPDATE subscription_sources SET group_name = 'default' WHERE group_name IS NULL OR TRIM(group_name) = ''",
        )
        .execute(&self.pool)
        .await;

        // Migration: add sync_interval_secs column if it doesn't exist
        let _ = sqlx::query(
            "ALTER TABLE subscription_sources ADD COLUMN sync_interval_secs INTEGER NOT NULL DEFAULT 21600",
        )
        .execute(&self.pool)
        .await;

        let _ = sqlx::query(
            "UPDATE subscription_sources SET sync_interval_secs = 21600 WHERE sync_interval_secs IS NULL OR sync_interval_secs = 0",
        )
        .execute(&self.pool)
        .await;

        // Users table (multi-user auth with roles)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                username TEXT NOT NULL UNIQUE,
                password_hash TEXT NOT NULL,
                role TEXT NOT NULL DEFAULT 'user',
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Sessions table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                token TEXT PRIMARY KEY,
                user_id INTEGER NOT NULL,
                expires_at TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
            );
            "#,
        )
        .execute(&self.pool)
        .await?;

        // API keys table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS api_keys (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                key_hash TEXT NOT NULL UNIQUE,
                preview TEXT NOT NULL,
                expires_at TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            "#,
        )
        .execute(&self.pool)
        .await?;

        let _ = sqlx::query("ALTER TABLE api_keys ADD COLUMN expires_at TEXT")
            .execute(&self.pool)
            .await;

        // Migration: add role column to users if missing
        let _ = sqlx::query("ALTER TABLE users ADD COLUMN role TEXT NOT NULL DEFAULT 'user'")
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("UPDATE users SET role = 'admin' WHERE role = 'user' AND id IN (SELECT id FROM users ORDER BY id LIMIT 1)")
            .execute(&self.pool)
            .await;

        // User preferences table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS user_preferences (
                user_id INTEGER PRIMARY KEY,
                theme TEXT NOT NULL DEFAULT 'system',
                language TEXT NOT NULL DEFAULT 'en',
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
            );
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Migration: add timezone column to user_preferences if missing
        let _ = sqlx::query(
            "ALTER TABLE user_preferences ADD COLUMN timezone TEXT NOT NULL DEFAULT 'auto'",
        )
        .execute(&self.pool)
        .await;

        // System settings table (key-value store for checker config etc.)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS system_settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Repair broken check_logs FK from previous migrations (SQLite 3.26+ bug)
        self.repair_check_logs_fk().await?;

        // Migration: change UNIQUE(ip, port) → UNIQUE(ip, port, protocol)
        self.migrate_proxy_unique_constraint().await?;

        // Versioned migration system (database_meta + ordered upgrades)
        self.initialize_database_versioning("1.0.0").await?;
        self.run_versioned_migrations().await?;

        Ok(())
    }

    async fn initialize_database_versioning(&self, default_version: &str) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS database_meta (
                id INTEGER PRIMARY KEY CHECK(id = 1),
                version TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO database_meta (id, version, updated_at)
            VALUES (1, ?, datetime('now'))
            ON CONFLICT(id) DO NOTHING
            "#,
        )
        .bind(default_version)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn current_db_version(&self) -> Result<String> {
        let row: (String,) = sqlx::query_as("SELECT version FROM database_meta WHERE id = 1")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0)
    }

    async fn run_versioned_migrations(&self) -> Result<()> {
        #[derive(Clone, Copy)]
        struct DbMigration {
            version: &'static str,
            description: &'static str,
        }

        // Keep this list append-only; never reorder historical versions.
        let mut migrations = vec![
            DbMigration {
                version: "1.0.1",
                description: "create migration logs table",
            },
            DbMigration {
                version: "1.1.0",
                description: "normalize default group and enforce metadata indexes",
            },
            DbMigration {
                version: "1.2.0",
                description: "add subscription group field and proxy subscription linkage",
            },
        ];

        migrations.sort_by(|a, b| {
            Version::parse(a.version)
                .expect("invalid migration semver")
                .cmp(&Version::parse(b.version).expect("invalid migration semver"))
        });

        let current = self.current_db_version().await?;
        let current_ver = Version::parse(&current)?;

        for migration in migrations {
            let target_ver = Version::parse(migration.version)?;
            if target_ver <= current_ver {
                continue;
            }

            info!(
                from = %current,
                to = migration.version,
                step = migration.description,
                "Applying database migration"
            );

            let mut tx = self.pool.begin().await?;

            match migration.version {
                "1.0.1" => Self::migration_1_0_1(&mut tx).await?,
                "1.1.0" => Self::migration_1_1_0(&mut tx).await?,
                "1.2.0" => Self::migration_1_2_0(&mut tx).await?,
                _ => unreachable!("unknown migration version"),
            }

            sqlx::query("UPDATE database_meta SET version = ?, updated_at = datetime('now') WHERE id = 1")
                .bind(migration.version)
                .execute(&mut *tx)
                .await?;

            // Optional audit trail for applied migrations.
            let _ = sqlx::query(
                "INSERT INTO migration_logs (version, description, applied_at) VALUES (?, ?, datetime('now'))",
            )
            .bind(migration.version)
            .bind(migration.description)
            .execute(&mut *tx)
            .await;

            tx.commit().await?;

            info!(version = migration.version, "Database migration applied");
        }

        Ok(())
    }

    async fn migration_1_0_1(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS migration_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                version TEXT NOT NULL UNIQUE,
                description TEXT NOT NULL,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            "#,
        )
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    async fn migration_1_1_0(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>) -> Result<()> {
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_proxies_group ON proxies(group_name)")
            .execute(&mut **tx)
            .await?;

        sqlx::query("INSERT OR IGNORE INTO proxy_groups (name) VALUES ('default')")
            .execute(&mut **tx)
            .await?;

        sqlx::query("UPDATE proxies SET group_name = 'default' WHERE group_name IS NULL OR TRIM(group_name) = ''")
            .execute(&mut **tx)
            .await?;

        Ok(())
    }

    async fn migration_1_2_0(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>) -> Result<()> {
        let _ = sqlx::query(
            "ALTER TABLE subscription_sources ADD COLUMN group_name TEXT NOT NULL DEFAULT 'default'",
        )
        .execute(&mut **tx)
        .await;

        sqlx::query(
            "UPDATE subscription_sources SET group_name = 'default' WHERE group_name IS NULL OR TRIM(group_name) = ''",
        )
        .execute(&mut **tx)
        .await?;

        let _ = sqlx::query("ALTER TABLE proxies ADD COLUMN subscription_id INTEGER")
            .execute(&mut **tx)
            .await;

        // Backfill linkage from legacy source tag format: sub:{id}:{name}
        sqlx::query(
            r#"
            UPDATE proxies
            SET subscription_id = CAST(
                substr(source, 5, instr(substr(source, 5), ':') - 1) AS INTEGER
            )
            WHERE subscription_id IS NULL
              AND source LIKE 'sub:%:%'
              AND instr(substr(source, 5), ':') > 1
            "#,
        )
        .execute(&mut **tx)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_proxies_subscription ON proxies(subscription_id)")
            .execute(&mut **tx)
            .await?;

        Ok(())
    }

    // ── System Settings ──

    pub async fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT value FROM system_settings WHERE key = ?")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|r| r.0))
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO system_settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_checker_config(&self) -> CheckerConfig {
        let interval = self
            .get_setting("checker.interval_secs")
            .await
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60);
        let timeout = self
            .get_setting("checker.timeout_secs")
            .await
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);
        let max_concurrent = self
            .get_setting("checker.max_concurrent")
            .await
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok())
            .unwrap_or(200);
        let targets = self
            .get_setting("checker.targets")
            .await
            .ok()
            .flatten()
            .map(|v| serde_json::from_str(&v).unwrap_or_default())
            .unwrap_or_else(|| {
                vec![
                    "https://httpbin.org/ip".to_string(),
                    "https://www.cloudflare.com/cdn-cgi/trace".to_string(),
                ]
            });
        CheckerConfig {
            interval_secs: interval,
            timeout_secs: timeout,
            max_concurrent,
            targets,
        }
    }

    pub async fn save_checker_config(&self, cfg: &CheckerConfig) -> Result<()> {
        self.set_setting("checker.interval_secs", &cfg.interval_secs.to_string())
            .await?;
        self.set_setting("checker.timeout_secs", &cfg.timeout_secs.to_string())
            .await?;
        self.set_setting("checker.max_concurrent", &cfg.max_concurrent.to_string())
            .await?;
        self.set_setting("checker.targets", &serde_json::to_string(&cfg.targets)?)
            .await?;
        Ok(())
    }

    /// Repair check_logs FK reference broken by SQLite 3.26+ auto-updating
    /// FK definitions during ALTER TABLE RENAME.
    async fn repair_check_logs_fk(&self) -> Result<()> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='check_logs'",
        )
        .fetch_optional(&self.pool)
        .await?;

        let Some((sql,)) = row else { return Ok(()) };

        if !sql.contains("proxies_old") {
            return Ok(());
        }

        info!("Repairing check_logs table: fixing broken foreign key reference");

        let mut tx = self.pool.begin().await?;

        sqlx::query("PRAGMA legacy_alter_table=ON")
            .execute(&mut *tx)
            .await?;

        sqlx::query("ALTER TABLE check_logs RENAME TO _check_logs_repair")
            .execute(&mut *tx)
            .await?;

        sqlx::query(
            r#"
            CREATE TABLE check_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                proxy_id INTEGER NOT NULL,
                target TEXT NOT NULL,
                success INTEGER NOT NULL DEFAULT 0,
                latency_ms REAL,
                error TEXT,
                checked_at TEXT NOT NULL DEFAULT (datetime('now')),
                FOREIGN KEY (proxy_id) REFERENCES proxies(id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query("INSERT INTO check_logs SELECT * FROM _check_logs_repair")
            .execute(&mut *tx)
            .await?;

        sqlx::query("DROP TABLE _check_logs_repair")
            .execute(&mut *tx)
            .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_check_logs_proxy ON check_logs(proxy_id, checked_at DESC)",
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query("PRAGMA legacy_alter_table=OFF")
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        info!("check_logs table repaired");

        Ok(())
    }

    /// Migrate proxies table: UNIQUE(ip, port) → UNIQUE(ip, port, protocol).
    async fn migrate_proxy_unique_constraint(&self) -> Result<()> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT sql FROM sqlite_master WHERE type='table' AND name='proxies'")
                .fetch_optional(&self.pool)
                .await?;

        let Some((sql,)) = row else { return Ok(()) };

        if sql.contains("UNIQUE(ip, port, protocol)") {
            return Ok(());
        }

        info!("Migrating proxies table: UNIQUE(ip, port) → UNIQUE(ip, port, protocol)");

        let mut tx = self.pool.begin().await?;

        // Prevent SQLite 3.26+ from updating FK references in other tables
        sqlx::query("PRAGMA legacy_alter_table=ON")
            .execute(&mut *tx)
            .await?;

        sqlx::query("ALTER TABLE proxies RENAME TO proxies_old")
            .execute(&mut *tx)
            .await?;

        sqlx::query(
            r#"
            CREATE TABLE proxies (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ip TEXT NOT NULL,
                port INTEGER NOT NULL,
                protocol TEXT NOT NULL DEFAULT 'http',
                anonymity TEXT NOT NULL DEFAULT 'unknown',
                country TEXT NOT NULL DEFAULT 'unknown',
                score REAL NOT NULL DEFAULT 0.0,
                is_alive INTEGER NOT NULL DEFAULT 0,
                success_count INTEGER NOT NULL DEFAULT 0,
                fail_count INTEGER NOT NULL DEFAULT 0,
                consecutive_fails INTEGER NOT NULL DEFAULT 0,
                avg_latency_ms REAL NOT NULL DEFAULT 0.0,
                last_check_at TEXT,
                last_success_at TEXT,
                next_check_at TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                source TEXT NOT NULL DEFAULT 'unknown',
                subscription_id INTEGER,
                group_name TEXT NOT NULL DEFAULT 'default',
                UNIQUE(ip, port, protocol)
            )
            "#,
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO proxies (id, ip, port, protocol, anonymity, country, score,
                is_alive, success_count, fail_count, consecutive_fails, avg_latency_ms,
                last_check_at, last_success_at, next_check_at, created_at, updated_at, source, subscription_id, group_name)
            SELECT id, ip, port, protocol, anonymity, country, score,
                is_alive, success_count, fail_count, consecutive_fails, avg_latency_ms,
                last_check_at, last_success_at, next_check_at, created_at, updated_at, source,
                NULL,
                'default'
            FROM proxies_old
            "#,
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query("DROP TABLE proxies_old")
            .execute(&mut *tx)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_proxies_alive ON proxies(is_alive)")
            .execute(&mut *tx)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_proxies_score ON proxies(score DESC)")
            .execute(&mut *tx)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_proxies_country ON proxies(country)")
            .execute(&mut *tx)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_proxies_next_check ON proxies(next_check_at)")
            .execute(&mut *tx)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_proxies_subscription ON proxies(subscription_id)")
            .execute(&mut *tx)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_proxies_group ON proxies(group_name)")
            .execute(&mut *tx)
            .await?;

        sqlx::query("PRAGMA legacy_alter_table=OFF")
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        info!(
            "Migration complete: proxies table now supports same IP:port with different protocols"
        );

        Ok(())
    }
}
