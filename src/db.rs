use anyhow::Result;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use chrono::{NaiveDateTime, Utc};

use crate::models::{
    CheckLog, CountryCount, LatencyBucket, ProtocolCount, Proxy, ProxyStats, ScoreBucket,
    SubscriptionSource,
};

#[derive(Clone)]
pub struct Database {
    pub pool: SqlitePool,
}

impl Database {
    pub async fn new(url: &str) -> Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(10)
            .connect(url)
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
                UNIQUE(ip, port)
            );
            "#,
        )
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

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_proxies_alive ON proxies(is_alive);",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_proxies_score ON proxies(score DESC);",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_proxies_country ON proxies(country);",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_proxies_next_check ON proxies(next_check_at);",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_check_logs_proxy ON check_logs(proxy_id, checked_at DESC);",
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

        // Migration: add sync_interval_secs column if it doesn't exist
        let _ = sqlx::query(
            "ALTER TABLE subscription_sources ADD COLUMN sync_interval_secs INTEGER NOT NULL DEFAULT 21600",
        )
        .execute(&self.pool)
        .await;

        // Ensure all existing sources use the 6h default interval
        let _ = sqlx::query(
            "UPDATE subscription_sources SET sync_interval_secs = 21600 WHERE sync_interval_secs IS NULL OR sync_interval_secs = 0",
        )
        .execute(&self.pool)
        .await;

        // Users table (single-user auth)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                username TEXT NOT NULL UNIQUE,
                password_hash TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Sessions table (token-based auth)
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

        Ok(())
    }

    // ── Proxy CRUD ──

    pub async fn upsert_proxy(
        &self,
        ip: &str,
        port: u16,
        protocol: &str,
        source: &str,
    ) -> Result<i64> {
        let now = Utc::now().naive_utc();
        let next_check = now;

        let result = sqlx::query(
            r#"
            INSERT INTO proxies (ip, port, protocol, source, created_at, updated_at, next_check_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(ip, port) DO UPDATE SET
                protocol = COALESCE(excluded.protocol, proxies.protocol),
                source = excluded.source,
                updated_at = excluded.updated_at
            RETURNING id
            "#,
        )
        .bind(ip)
        .bind(port as i32)
        .bind(protocol)
        .bind(source)
        .bind(now)
        .bind(now)
        .bind(next_check)
        .fetch_one(&self.pool)
        .await?;

        let id: i64 = sqlx::Row::get(&result, "id");
        Ok(id)
    }

    pub async fn get_proxies_due_for_check(&self, limit: i64) -> Result<Vec<Proxy>> {
        let now = Utc::now().naive_utc();
        let proxies = sqlx::query_as::<_, Proxy>(
            r#"
            SELECT id, ip, port, protocol, anonymity, country, score, 
                   is_alive, success_count, fail_count, consecutive_fails,
                   avg_latency_ms, last_check_at, last_success_at, next_check_at,
                   created_at, updated_at, source
            FROM proxies
            WHERE next_check_at IS NULL OR next_check_at <= ?
            ORDER BY next_check_at ASC
            LIMIT ?
            "#,
        )
        .bind(now)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(proxies)
    }

    pub async fn update_proxy_check(
        &self,
        id: i64,
        success: bool,
        latency_ms: Option<f64>,
        next_check_at: NaiveDateTime,
    ) -> Result<()> {
        let now = Utc::now().naive_utc();

        if success {
            let latency = latency_ms.unwrap_or(0.0);
            sqlx::query(
                r#"
                UPDATE proxies SET
                    is_alive = 1,
                    success_count = success_count + 1,
                    consecutive_fails = 0,
                    avg_latency_ms = CASE 
                        WHEN success_count = 0 THEN ?
                        ELSE (avg_latency_ms * success_count + ?) / (success_count + 1)
                    END,
                    last_check_at = ?,
                    last_success_at = ?,
                    next_check_at = ?,
                    updated_at = ?
                WHERE id = ?
                "#,
            )
            .bind(latency)
            .bind(latency)
            .bind(now)
            .bind(now)
            .bind(next_check_at)
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        } else {
            sqlx::query(
                r#"
                UPDATE proxies SET
                    fail_count = fail_count + 1,
                    consecutive_fails = consecutive_fails + 1,
                    is_alive = CASE WHEN consecutive_fails >= 2 THEN 0 ELSE is_alive END,
                    last_check_at = ?,
                    next_check_at = ?,
                    updated_at = ?
                WHERE id = ?
                "#,
            )
            .bind(now)
            .bind(next_check_at)
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    pub async fn update_proxy_score(&self, id: i64, score: f64) -> Result<()> {
        sqlx::query("UPDATE proxies SET score = ?, updated_at = datetime('now') WHERE id = ?")
            .bind(score)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_proxy_metadata(
        &self,
        id: i64,
        country: &str,
        anonymity: &str,
        protocol: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE proxies SET
                country = ?,
                anonymity = ?,
                protocol = ?,
                updated_at = datetime('now')
            WHERE id = ?
            "#,
        )
        .bind(country)
        .bind(anonymity)
        .bind(protocol)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ── Check Log ──

    pub async fn insert_check_log(
        &self,
        proxy_id: i64,
        target: &str,
        success: bool,
        latency_ms: Option<f64>,
        error: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO check_logs (proxy_id, target, success, latency_ms, error, checked_at)
            VALUES (?, ?, ?, ?, ?, datetime('now'))
            "#,
        )
        .bind(proxy_id)
        .bind(target)
        .bind(success)
        .bind(latency_ms)
        .bind(error)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ── API Queries ──

    pub async fn get_random_alive_proxy(&self) -> Result<Option<Proxy>> {
        let proxy = sqlx::query_as::<_, Proxy>(
            r#"
            SELECT id, ip, port, protocol, anonymity, country, score,
                   is_alive, success_count, fail_count, consecutive_fails,
                   avg_latency_ms, last_check_at, last_success_at, next_check_at,
                   created_at, updated_at, source
            FROM proxies
            WHERE is_alive = 1 AND score >= 30
            ORDER BY RANDOM()
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(proxy)
    }

    pub async fn get_top_proxies(&self, limit: i64) -> Result<Vec<Proxy>> {
        let proxies = sqlx::query_as::<_, Proxy>(
            r#"
            SELECT id, ip, port, protocol, anonymity, country, score,
                   is_alive, success_count, fail_count, consecutive_fails,
                   avg_latency_ms, last_check_at, last_success_at, next_check_at,
                   created_at, updated_at, source
            FROM proxies
            WHERE is_alive = 1
            ORDER BY score DESC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(proxies)
    }

    pub async fn get_proxies_by_country(&self, country: &str, limit: i64) -> Result<Vec<Proxy>> {
        let proxies = sqlx::query_as::<_, Proxy>(
            r#"
            SELECT id, ip, port, protocol, anonymity, country, score,
                   is_alive, success_count, fail_count, consecutive_fails,
                   avg_latency_ms, last_check_at, last_success_at, next_check_at,
                   created_at, updated_at, source
            FROM proxies
            WHERE is_alive = 1 AND LOWER(country) = LOWER(?)
            ORDER BY score DESC
            LIMIT ?
            "#,
        )
        .bind(country)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(proxies)
    }

    pub async fn get_all_proxies(&self, page: i64, per_page: i64) -> Result<Vec<Proxy>> {
        let offset = (page - 1) * per_page;
        let proxies = sqlx::query_as::<_, Proxy>(
            r#"
            SELECT id, ip, port, protocol, anonymity, country, score,
                   is_alive, success_count, fail_count, consecutive_fails,
                   avg_latency_ms, last_check_at, last_success_at, next_check_at,
                   created_at, updated_at, source
            FROM proxies
            ORDER BY score DESC
            LIMIT ? OFFSET ?
            "#,
        )
        .bind(per_page)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        Ok(proxies)
    }

    /// Get alive proxies sorted by score or latency, with optional limit
    pub async fn get_proxies_sorted(&self, sort: &str, limit: Option<i64>) -> Result<Vec<Proxy>> {
        let order_clause = match sort {
            "latency" => "avg_latency_ms ASC",
            "success_rate" => "CAST(success_count AS REAL) / MAX(success_count + fail_count, 1) DESC",
            "success_count" => "success_count DESC",
            _ => "score DESC",
        };

        let sql = if let Some(lim) = limit {
            format!(
                r#"SELECT id, ip, port, protocol, anonymity, country, score,
                       is_alive, success_count, fail_count, consecutive_fails,
                       avg_latency_ms, last_check_at, last_success_at, next_check_at,
                       created_at, updated_at, source
                FROM proxies
                WHERE is_alive = 1
                ORDER BY {}
                LIMIT {}"#,
                order_clause, lim
            )
        } else {
            format!(
                r#"SELECT id, ip, port, protocol, anonymity, country, score,
                       is_alive, success_count, fail_count, consecutive_fails,
                       avg_latency_ms, last_check_at, last_success_at, next_check_at,
                       created_at, updated_at, source
                FROM proxies
                WHERE is_alive = 1
                ORDER BY {}"#,
                order_clause
            )
        };

        let proxies = sqlx::query_as::<_, Proxy>(&sql)
            .fetch_all(&self.pool)
            .await?;

        Ok(proxies)
    }

    pub async fn get_stats(&self) -> Result<ProxyStats> {
        // Combined basic stats in a single query (was 5 separate queries)
        let basics: (i64, i64, i64, f64, f64) = sqlx::query_as(
            r#"
            SELECT
                COUNT(*),
                SUM(CASE WHEN is_alive = 1 THEN 1 ELSE 0 END),
                SUM(CASE WHEN is_alive = 0 AND last_check_at IS NOT NULL THEN 1 ELSE 0 END),
                COALESCE((SELECT AVG(score) FROM proxies WHERE is_alive = 1), 0.0),
                COALESCE((SELECT AVG(avg_latency_ms) FROM proxies WHERE is_alive = 1 AND avg_latency_ms > 0), 0.0)
            FROM proxies
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        // Country distribution
        let countries = sqlx::query_as::<_, CountryCount>(
            r#"
            SELECT country, COUNT(*) as count 
            FROM proxies WHERE is_alive = 1 
            GROUP BY country ORDER BY count DESC LIMIT 20
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        // Latency distribution - single query with CASE WHEN (was 5 queries)
        let latency_dist = self.get_latency_distribution().await?;

        // Protocol distribution
        let protocols = sqlx::query_as::<_, ProtocolCount>(
            r#"
            SELECT protocol, COUNT(*) as count 
            FROM proxies WHERE is_alive = 1 
            GROUP BY protocol ORDER BY count DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        // Score distribution - single query with CASE WHEN (was 5 queries)
        let score_dist = self.get_score_distribution().await?;

        Ok(ProxyStats {
            total_proxies: basics.0,
            alive_proxies: basics.1,
            dead_proxies: basics.2,
            avg_score: basics.3,
            avg_latency_ms: basics.4,
            country_distribution: countries,
            latency_distribution: latency_dist,
            protocol_distribution: protocols,
            score_distribution: score_dist,
        })
    }

    async fn get_latency_distribution(&self) -> Result<Vec<LatencyBucket>> {
        let row: (i64, i64, i64, i64, i64) = sqlx::query_as(
            r#"
            SELECT
                SUM(CASE WHEN avg_latency_ms >= 0 AND avg_latency_ms < 100 THEN 1 ELSE 0 END),
                SUM(CASE WHEN avg_latency_ms >= 100 AND avg_latency_ms < 300 THEN 1 ELSE 0 END),
                SUM(CASE WHEN avg_latency_ms >= 300 AND avg_latency_ms < 500 THEN 1 ELSE 0 END),
                SUM(CASE WHEN avg_latency_ms >= 500 AND avg_latency_ms < 1000 THEN 1 ELSE 0 END),
                SUM(CASE WHEN avg_latency_ms >= 1000 THEN 1 ELSE 0 END)
            FROM proxies WHERE is_alive = 1
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(vec![
            LatencyBucket { range: "0-100ms".to_string(), count: row.0 },
            LatencyBucket { range: "100-300ms".to_string(), count: row.1 },
            LatencyBucket { range: "300-500ms".to_string(), count: row.2 },
            LatencyBucket { range: "500-1000ms".to_string(), count: row.3 },
            LatencyBucket { range: "1000ms+".to_string(), count: row.4 },
        ])
    }

    async fn get_score_distribution(&self) -> Result<Vec<ScoreBucket>> {
        let row: (i64, i64, i64, i64, i64) = sqlx::query_as(
            r#"
            SELECT
                SUM(CASE WHEN score >= 0 AND score < 20 THEN 1 ELSE 0 END),
                SUM(CASE WHEN score >= 20 AND score < 40 THEN 1 ELSE 0 END),
                SUM(CASE WHEN score >= 40 AND score < 60 THEN 1 ELSE 0 END),
                SUM(CASE WHEN score >= 60 AND score < 80 THEN 1 ELSE 0 END),
                SUM(CASE WHEN score >= 80 AND score <= 100 THEN 1 ELSE 0 END)
            FROM proxies
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(vec![
            ScoreBucket { range: "0-20".to_string(), count: row.0 },
            ScoreBucket { range: "20-40".to_string(), count: row.1 },
            ScoreBucket { range: "40-60".to_string(), count: row.2 },
            ScoreBucket { range: "60-80".to_string(), count: row.3 },
            ScoreBucket { range: "80-100".to_string(), count: row.4 },
        ])
    }

    #[allow(dead_code)]
    pub async fn get_check_logs_for_proxy(&self, proxy_id: i64, limit: i64) -> Result<Vec<CheckLog>> {
        let logs = sqlx::query_as::<_, CheckLog>(
            r#"
            SELECT id, proxy_id, target, success, latency_ms, error, checked_at
            FROM check_logs
            WHERE proxy_id = ?
            ORDER BY checked_at DESC
            LIMIT ?
            "#,
        )
        .bind(proxy_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(logs)
    }

    pub async fn cleanup_old_logs(&self, days: i64) -> Result<u64> {
        let result = sqlx::query(
            "DELETE FROM check_logs WHERE checked_at < datetime('now', ? || ' days')",
        )
        .bind(-days)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    // ── Admin: Proxy Management ──

    pub async fn delete_proxy(&self, id: i64) -> Result<bool> {
        let result = sqlx::query("DELETE FROM proxies WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn delete_all_dead_proxies(&self) -> Result<u64> {
        let result = sqlx::query("DELETE FROM proxies WHERE is_alive = 0 AND last_check_at IS NOT NULL")
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    pub async fn get_all_proxies_admin(&self, page: i64, per_page: i64, filter_alive: Option<bool>, filter_protocol: Option<&str>) -> Result<(Vec<Proxy>, i64)> {
        let offset = (page - 1) * per_page;

        let mut where_clauses = Vec::new();
        if let Some(alive) = filter_alive {
            where_clauses.push(format!("is_alive = {}", if alive { 1 } else { 0 }));
        }
        if let Some(proto) = filter_protocol {
            if !proto.is_empty() && proto != "all" {
                where_clauses.push(format!("protocol = '{}'", proto.replace('\'', "''")));
            }
        }

        let where_sql = if where_clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_clauses.join(" AND "))
        };

        let count_sql = format!("SELECT COUNT(*) FROM proxies {}", where_sql);
        let total: (i64,) = sqlx::query_as(&count_sql)
            .fetch_one(&self.pool)
            .await?;

        let query_sql = format!(
            r#"
            SELECT id, ip, port, protocol, anonymity, country, score,
                   is_alive, success_count, fail_count, consecutive_fails,
                   avg_latency_ms, last_check_at, last_success_at, next_check_at,
                   created_at, updated_at, source
            FROM proxies {}
            ORDER BY updated_at DESC
            LIMIT ? OFFSET ?
            "#,
            where_sql
        );

        let proxies = sqlx::query_as::<_, Proxy>(&query_sql)
            .bind(per_page)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;

        Ok((proxies, total.0))
    }

    // ── Admin: Subscription Sources ──

    pub async fn create_subscription_source(
        &self,
        name: &str,
        source_type: &str,
        url: Option<&str>,
        content: Option<&str>,
        protocol_hint: &str,
        sync_interval_secs: i64,
    ) -> Result<i64> {
        let result = sqlx::query(
            r#"
            INSERT INTO subscription_sources (name, source_type, url, content, protocol_hint, sync_interval_secs, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, datetime('now'), datetime('now'))
            RETURNING id
            "#,
        )
        .bind(name)
        .bind(source_type)
        .bind(url)
        .bind(content)
        .bind(protocol_hint)
        .bind(sync_interval_secs)
        .fetch_one(&self.pool)
        .await?;

        let id: i64 = sqlx::Row::get(&result, "id");
        Ok(id)
    }

    pub async fn get_subscription_source_by_id(&self, id: i64) -> Result<Option<SubscriptionSource>> {
        let source = sqlx::query_as::<_, SubscriptionSource>(
            r#"
            SELECT id, name, source_type, url, content, protocol_hint, is_enabled,
                   sync_interval_secs, proxy_count, last_sync_at, last_error, created_at, updated_at
            FROM subscription_sources
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(source)
    }

    pub async fn get_all_subscription_sources(&self) -> Result<Vec<SubscriptionSource>> {
        let sources = sqlx::query_as::<_, SubscriptionSource>(
            r#"
            SELECT id, name, source_type, url, content, protocol_hint, is_enabled,
                   sync_interval_secs, proxy_count, last_sync_at, last_error, created_at, updated_at
            FROM subscription_sources
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(sources)
    }

    pub async fn get_enabled_subscription_sources(&self) -> Result<Vec<SubscriptionSource>> {
        let sources = sqlx::query_as::<_, SubscriptionSource>(
            r#"
            SELECT id, name, source_type, url, content, protocol_hint, is_enabled,
                   sync_interval_secs, proxy_count, last_sync_at, last_error, created_at, updated_at
            FROM subscription_sources
            WHERE is_enabled = 1
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(sources)
    }

    pub async fn delete_subscription_source(&self, id: i64) -> Result<bool> {
        let result = sqlx::query("DELETE FROM subscription_sources WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Get enabled subscription sources that are due for sync based on their individual intervals
    pub async fn get_sources_due_for_sync(&self) -> Result<Vec<SubscriptionSource>> {
        let sources = sqlx::query_as::<_, SubscriptionSource>(
            r#"
            SELECT id, name, source_type, url, content, protocol_hint, is_enabled,
                   sync_interval_secs, proxy_count, last_sync_at, last_error, created_at, updated_at
            FROM subscription_sources
            WHERE is_enabled = 1
              AND (last_sync_at IS NULL
                   OR datetime(last_sync_at, '+' || sync_interval_secs || ' seconds') <= datetime('now'))
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(sources)
    }

    pub async fn toggle_subscription_source(&self, id: i64, enabled: bool) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE subscription_sources SET is_enabled = ?, updated_at = datetime('now') WHERE id = ?",
        )
        .bind(enabled)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn update_subscription_sync_result(
        &self,
        id: i64,
        proxy_count: i64,
        error: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE subscription_sources SET
                proxy_count = ?,
                last_sync_at = datetime('now'),
                last_error = ?,
                updated_at = datetime('now')
            WHERE id = ?
            "#,
        )
        .bind(proxy_count)
        .bind(error)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ── Auth ──

    pub async fn has_any_user(&self) -> Result<bool> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0 > 0)
    }

    pub async fn create_user(&self, username: &str, password_hash: &str) -> Result<i64> {
        let result = sqlx::query(
            "INSERT INTO users (username, password_hash) VALUES (?, ?)",
        )
        .bind(username)
        .bind(password_hash)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    pub async fn get_user_by_username(&self, username: &str) -> Result<Option<(i64, String)>> {
        let row = sqlx::query_as::<_, (i64, String)>(
            "SELECT id, password_hash FROM users WHERE username = ?",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn create_session(&self, token: &str, user_id: i64, expires_at: NaiveDateTime) -> Result<()> {
        sqlx::query(
            "INSERT INTO sessions (token, user_id, expires_at) VALUES (?, ?, ?)",
        )
        .bind(token)
        .bind(user_id)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn validate_session(&self, token: &str) -> Result<Option<i64>> {
        let now = Utc::now().naive_utc();
        let row = sqlx::query_as::<_, (i64,)>(
            "SELECT user_id FROM sessions WHERE token = ? AND expires_at > ?",
        )
        .bind(token)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.0))
    }

    pub async fn refresh_session(&self, token: &str, new_expires: NaiveDateTime) -> Result<()> {
        sqlx::query("UPDATE sessions SET expires_at = ? WHERE token = ?")
            .bind(new_expires)
            .bind(token)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_session(&self, token: &str) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE token = ?")
            .bind(token)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn cleanup_expired_sessions(&self) -> Result<u64> {
        let now = Utc::now().naive_utc();
        let result = sqlx::query("DELETE FROM sessions WHERE expires_at <= ?")
            .bind(now)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }
}
