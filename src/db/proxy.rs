use anyhow::Result;
use chrono::Utc;

use crate::models::{CheckLog, Proxy};

use super::Database;

impl Database {
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
            ON CONFLICT(ip, port, protocol) DO UPDATE SET
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
        next_check_at: chrono::NaiveDateTime,
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

    pub async fn get_max_success_count(&self) -> Result<i64> {
        let row: (i64,) = sqlx::query_as("SELECT COALESCE(MAX(success_count), 0) FROM proxies")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0)
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

    pub async fn get_proxies_sorted(
        &self,
        sort: &str,
        limit: Option<i64>,
        country: Option<&str>,
    ) -> Result<Vec<Proxy>> {
        let order_clause = match sort {
            "latency" => "avg_latency_ms ASC",
            "success_rate" => {
                "CAST(success_count AS REAL) / MAX(success_count + fail_count, 1) DESC"
            }
            "success_count" => "success_count DESC",
            _ => "score DESC",
        };

        let has_country = matches!(country, Some(c) if !c.is_empty() && c != "all");
        let country_filter = if has_country { " AND country = ?" } else { "" };

        let limit_clause = match limit {
            Some(lim) => format!(" LIMIT {}", lim),
            None => String::new(),
        };

        let sql = format!(
            r#"SELECT id, ip, port, protocol, anonymity, country, score,
                   is_alive, success_count, fail_count, consecutive_fails,
                   avg_latency_ms, last_check_at, last_success_at, next_check_at,
                   created_at, updated_at, source
            FROM proxies
            WHERE is_alive = 1{}
            ORDER BY {}{}"#,
            country_filter, order_clause, limit_clause
        );

        let mut query = sqlx::query_as::<_, Proxy>(&sql);
        if has_country {
            query = query.bind(country.unwrap());
        }

        let proxies = query.fetch_all(&self.pool).await?;

        Ok(proxies)
    }

    pub async fn get_alive_countries(&self) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT country FROM proxies WHERE is_alive = 1 ORDER BY country",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    #[allow(dead_code)]
    pub async fn get_check_logs_for_proxy(
        &self,
        proxy_id: i64,
        limit: i64,
    ) -> Result<Vec<CheckLog>> {
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

        let deleted = result.rows_affected();

        // Reclaim disk space after large deletions
        if deleted > 0 {
            let _ = sqlx::query("VACUUM")
                .execute(&self.pool)
                .await;
        }

        Ok(deleted)
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
        let result =
            sqlx::query("DELETE FROM proxies WHERE is_alive = 0 AND last_check_at IS NOT NULL")
                .execute(&self.pool)
                .await?;
        Ok(result.rows_affected())
    }

    pub async fn get_all_proxies_admin(
        &self,
        page: i64,
        per_page: i64,
        filter_alive: Option<bool>,
        filter_protocol: Option<&str>,
    ) -> Result<(Vec<Proxy>, i64)> {
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
}
