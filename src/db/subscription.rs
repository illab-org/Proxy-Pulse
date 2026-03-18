use anyhow::Result;

use crate::models::SubscriptionSource;

use super::Database;

impl Database {
    pub async fn create_subscription_source(
        &self,
        name: &str,
        source_type: &str,
        url: Option<&str>,
        content: Option<&str>,
        protocol_hint: &str,
        group_name: &str,
        sync_interval_secs: i64,
    ) -> Result<i64> {
        let result = sqlx::query(
            r#"
            INSERT INTO subscription_sources (name, source_type, url, content, protocol_hint, group_name, sync_interval_secs, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, datetime('now'), datetime('now'))
            RETURNING id
            "#,
        )
        .bind(name)
        .bind(source_type)
        .bind(url)
        .bind(content)
        .bind(protocol_hint)
        .bind(group_name)
        .bind(sync_interval_secs)
        .fetch_one(&self.pool)
        .await?;

        let id: i64 = sqlx::Row::get(&result, "id");
        Ok(id)
    }

    pub async fn get_subscription_source_by_id(
        &self,
        id: i64,
    ) -> Result<Option<SubscriptionSource>> {
        let source = sqlx::query_as::<_, SubscriptionSource>(
            r#"
                 SELECT id, name, source_type, url, content, protocol_hint, group_name, is_enabled,
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
                 SELECT id, name, source_type, url, content, protocol_hint, group_name, is_enabled,
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
                 SELECT id, name, source_type, url, content, protocol_hint, group_name, is_enabled,
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

    pub async fn get_sources_due_for_sync(&self) -> Result<Vec<SubscriptionSource>> {
        let sources = sqlx::query_as::<_, SubscriptionSource>(
            r#"
                 SELECT id, name, source_type, url, content, protocol_hint, group_name, is_enabled,
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

    pub async fn update_subscription_group(&self, id: i64, group_name: &str) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE subscription_sources SET group_name = ?, updated_at = datetime('now') WHERE id = ?",
        )
        .bind(group_name)
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
}
