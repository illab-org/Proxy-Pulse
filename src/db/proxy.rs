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
        subscription_id: Option<i64>,
        group_name: &str,
    ) -> Result<i64> {
        let now = Utc::now().naive_utc();
        let next_check = now;

        let result = sqlx::query(
            r#"
            INSERT INTO proxies (ip, port, protocol, source, subscription_id, group_name, created_at, updated_at, next_check_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(ip, port, protocol) DO UPDATE SET
                source = excluded.source,
                subscription_id = COALESCE(excluded.subscription_id, proxies.subscription_id),
                updated_at = excluded.updated_at
            RETURNING id
            "#,
        )
        .bind(ip)
        .bind(port as i32)
        .bind(protocol)
        .bind(source)
        .bind(subscription_id)
        .bind(group_name)
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
                     created_at, updated_at, source, subscription_id, group_name
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
            SELECT p.id, p.ip, p.port, p.protocol, p.anonymity, p.country, p.score,
                   p.is_alive, p.success_count, p.fail_count, p.consecutive_fails,
                   p.avg_latency_ms, p.last_check_at, p.last_success_at, p.next_check_at,
                   p.created_at, p.updated_at, p.source, p.subscription_id,
                     p.group_name
            FROM proxies p
            WHERE p.is_alive = 1 AND p.score >= 30
            ORDER BY RANDOM()
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(proxy)
    }

    pub async fn get_top_proxies(&self, limit: i64, group: Option<&str>) -> Result<Vec<Proxy>> {
        let has_group = matches!(group, Some(g) if !g.is_empty() && !g.eq_ignore_ascii_case("all"));
        let group_filter = if has_group {
            " AND p.group_name = ?"
        } else {
            ""
        };
        let sql = format!(
            r#"
             SELECT p.id, p.ip, p.port, p.protocol, p.anonymity, p.country, p.score,
                 p.is_alive, p.success_count, p.fail_count, p.consecutive_fails,
                 p.avg_latency_ms, p.last_check_at, p.last_success_at, p.next_check_at,
                 p.created_at, p.updated_at, p.source, p.subscription_id,
                                 p.group_name
            FROM proxies p
             WHERE p.is_alive = 1{}
             ORDER BY p.score DESC
            LIMIT ?
            "#,
            group_filter
        );

        let mut query = sqlx::query_as::<_, Proxy>(&sql);
        if has_group {
            query = query.bind(group.unwrap());
        }

        let proxies = query.bind(limit).fetch_all(&self.pool).await?;

        Ok(proxies)
    }

    pub async fn get_proxy_groups(&self) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT name FROM (
                SELECT 'default' AS name
                UNION
                SELECT DISTINCT TRIM(name) AS name
                FROM proxy_groups
                WHERE name IS NOT NULL AND TRIM(name) <> ''
                UNION
                SELECT DISTINCT TRIM(s.group_name) AS name
                FROM subscription_sources s
                WHERE s.group_name IS NOT NULL AND TRIM(s.group_name) <> ''
                UNION
                SELECT DISTINCT TRIM(p.group_name) AS name
                FROM proxies p
                WHERE p.group_name IS NOT NULL AND TRIM(p.group_name) <> ''
            ) g
            WHERE LOWER(name) <> 'all'
            ORDER BY CASE WHEN name = 'default' THEN 0 ELSE 1 END, name
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    pub async fn create_proxy_group(&self, name: &str) -> Result<()> {
        sqlx::query("INSERT OR IGNORE INTO proxy_groups (name) VALUES (?)")
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn rename_proxy_group(&self, old_name: &str, new_name: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT OR IGNORE INTO proxy_groups (name) VALUES (?)")
            .bind(new_name)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE subscription_sources SET group_name = ?, updated_at = datetime('now') WHERE group_name = ?",
        )
        .bind(new_name)
        .bind(old_name)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE proxies SET group_name = ?, updated_at = datetime('now') WHERE group_name = ?")
            .bind(new_name)
            .bind(old_name)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM proxy_groups WHERE name = ? AND name <> 'default'")
            .bind(old_name)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn delete_proxy_group(&self, name: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE subscription_sources SET group_name = 'default', updated_at = datetime('now') WHERE group_name = ?")
            .bind(name)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE proxies SET group_name = 'default', updated_at = datetime('now') WHERE group_name = ?")
            .bind(name)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM proxy_groups WHERE name = ? AND name <> 'default'")
            .bind(name)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn update_proxy_group(&self, id: i64, group_name: &str) -> Result<bool> {
        sqlx::query("INSERT OR IGNORE INTO proxy_groups (name) VALUES (?)")
            .bind(group_name)
            .execute(&self.pool)
            .await?;

        let result = sqlx::query(
            "UPDATE proxies SET group_name = ?, updated_at = datetime('now') WHERE id = ?",
        )
        .bind(group_name)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn get_proxies_by_country(&self, country: &str, limit: i64) -> Result<Vec<Proxy>> {
        let proxies = sqlx::query_as::<_, Proxy>(
            r#"
            SELECT p.id, p.ip, p.port, p.protocol, p.anonymity, p.country, p.score,
                   p.is_alive, p.success_count, p.fail_count, p.consecutive_fails,
                   p.avg_latency_ms, p.last_check_at, p.last_success_at, p.next_check_at,
                   p.created_at, p.updated_at, p.source, p.subscription_id,
                     p.group_name
            FROM proxies p
            WHERE p.is_alive = 1 AND LOWER(p.country) = LOWER(?)
            ORDER BY p.score DESC
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
            SELECT p.id, p.ip, p.port, p.protocol, p.anonymity, p.country, p.score,
                   p.is_alive, p.success_count, p.fail_count, p.consecutive_fails,
                   p.avg_latency_ms, p.last_check_at, p.last_success_at, p.next_check_at,
                   p.created_at, p.updated_at, p.source, p.subscription_id,
                     p.group_name
            FROM proxies p
            ORDER BY p.score DESC
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
        let country_filter = if has_country { " AND p.country = ?" } else { "" };

        let limit_clause = match limit {
            Some(lim) => format!(" LIMIT {}", lim),
            None => String::new(),
        };

        let sql = format!(
            r#"SELECT p.id, p.ip, p.port, p.protocol, p.anonymity, p.country, p.score,
                   p.is_alive, p.success_count, p.fail_count, p.consecutive_fails,
                   p.avg_latency_ms, p.last_check_at, p.last_success_at, p.next_check_at,
                   p.created_at, p.updated_at, p.source, p.subscription_id,
                     p.group_name
            FROM proxies p
            WHERE p.is_alive = 1{}
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
        let result =
            sqlx::query("DELETE FROM check_logs WHERE checked_at < datetime('now', ? || ' days')")
                .bind(-days)
                .execute(&self.pool)
                .await?;

        let deleted = result.rows_affected();

        // Reclaim disk space after large deletions
        if deleted > 0 {
            let _ = sqlx::query("VACUUM").execute(&self.pool).await;
        }

        Ok(deleted)
    }

    /// Cap check logs to a maximum number of entries
    pub async fn cap_check_logs(&self, max_entries: i64) -> Result<u64> {
        let result = sqlx::query(
            "DELETE FROM check_logs WHERE id NOT IN (SELECT id FROM check_logs ORDER BY id DESC LIMIT ?)"
        )
        .bind(max_entries)
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
        filter_group: Option<&str>,
        filter_status: Option<&str>,
        search: Option<&str>,
    ) -> Result<(Vec<Proxy>, i64)> {
        let offset = (page - 1) * per_page;

        let mut where_clauses = Vec::new();
        if let Some(status) = filter_status {
            match status {
                "alive" => where_clauses.push("p.is_alive = 1".to_string()),
                "dead" => {
                    where_clauses.push("p.is_alive = 0 AND p.last_check_at IS NOT NULL".to_string())
                }
                "untested" => where_clauses.push("p.last_check_at IS NULL".to_string()),
                _ => {}
            }
        } else if let Some(alive) = filter_alive {
            where_clauses.push(format!("p.is_alive = {}", if alive { 1 } else { 0 }));
        }
        if let Some(proto) = filter_protocol {
            if !proto.is_empty() && proto != "all" {
                where_clauses.push("p.protocol = ?".to_string());
            }
        }
        if let Some(group) = filter_group {
            if !group.is_empty() && !group.eq_ignore_ascii_case("all") {
                where_clauses.push("p.group_name = ?".to_string());
            }
        }
        let search_term = search
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| format!("%{}%", s));
        if search_term.is_some() {
            where_clauses.push(
                "(p.ip LIKE ? OR CAST(p.port AS TEXT) LIKE ? OR p.country LIKE ? OR p.source LIKE ? OR p.protocol LIKE ?)"
                    .to_string(),
            );
        }

        let where_sql = if where_clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_clauses.join(" AND "))
        };

        let count_sql = format!("SELECT COUNT(*) FROM proxies p {}", where_sql);
        let mut count_query = sqlx::query_as::<_, (i64,)>(&count_sql);
        if let Some(proto) = filter_protocol {
            if !proto.is_empty() && proto != "all" {
                count_query = count_query.bind(proto);
            }
        }
        if let Some(group) = filter_group {
            if !group.is_empty() && !group.eq_ignore_ascii_case("all") {
                count_query = count_query.bind(group);
            }
        }
        if let Some(term) = &search_term {
            count_query = count_query
                .bind(term)
                .bind(term)
                .bind(term)
                .bind(term)
                .bind(term);
        }
        let total: (i64,) = count_query.fetch_one(&self.pool).await?;

        let query_sql = format!(
            r#"
            SELECT p.id, p.ip, p.port, p.protocol, p.anonymity, p.country, p.score,
                   p.is_alive, p.success_count, p.fail_count, p.consecutive_fails,
                   p.avg_latency_ms, p.last_check_at, p.last_success_at, p.next_check_at,
                   p.created_at, p.updated_at, p.source, p.subscription_id,
                     p.group_name
                        FROM proxies p
                        {}
            ORDER BY p.updated_at DESC
            LIMIT ? OFFSET ?
            "#,
            where_sql
        );

        let mut data_query = sqlx::query_as::<_, Proxy>(&query_sql);
        if let Some(proto) = filter_protocol {
            if !proto.is_empty() && proto != "all" {
                data_query = data_query.bind(proto);
            }
        }
        if let Some(group) = filter_group {
            if !group.is_empty() && !group.eq_ignore_ascii_case("all") {
                data_query = data_query.bind(group);
            }
        }
        if let Some(term) = &search_term {
            data_query = data_query
                .bind(term)
                .bind(term)
                .bind(term)
                .bind(term)
                .bind(term);
        }
        let proxies = data_query
            .bind(per_page)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;

        Ok((proxies, total.0))
    }
}
