use anyhow::Result;
use chrono::{NaiveDateTime, Utc};

use super::Database;

impl Database {
    // ── Users ──

    pub async fn has_any_user(&self) -> Result<bool> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0 > 0)
    }

    pub async fn create_user(
        &self,
        username: &str,
        password_hash: &str,
        role: &str,
    ) -> Result<i64> {
        let result =
            sqlx::query("INSERT INTO users (username, password_hash, role) VALUES (?, ?, ?)")
                .bind(username)
                .bind(password_hash)
                .bind(role)
                .execute(&self.pool)
                .await?;
        let user_id = result.last_insert_rowid();

        // Apply system default preferences for new user
        let theme = self
            .get_setting("system.default_theme")
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| "system".to_string());
        let language = self
            .get_setting("system.default_language")
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| "en".to_string());
        let timezone = self
            .get_setting("system.default_timezone")
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| "auto".to_string());
        let _ = self
            .save_user_preferences(user_id, &theme, &language, &timezone)
            .await;

        Ok(user_id)
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

    pub async fn update_user_password(&self, user_id: i64, password_hash: &str) -> Result<()> {
        sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
            .bind(password_hash)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_user(
        &self,
        user_id: i64,
        role: Option<&str>,
        password_hash: Option<&str>,
    ) -> Result<bool> {
        if let Some(r) = role {
            sqlx::query("UPDATE users SET role = ? WHERE id = ?")
                .bind(r)
                .bind(user_id)
                .execute(&self.pool)
                .await?;
        }
        if let Some(ph) = password_hash {
            sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
                .bind(ph)
                .bind(user_id)
                .execute(&self.pool)
                .await?;
        }
        Ok(true)
    }

    pub async fn get_user_password_hash(&self, user_id: i64) -> Result<Option<String>> {
        let row = sqlx::query_as::<_, (String,)>("SELECT password_hash FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.0))
    }

    pub async fn get_user_info(&self, user_id: i64) -> Result<Option<(String, String)>> {
        let row =
            sqlx::query_as::<_, (String, String)>("SELECT username, role FROM users WHERE id = ?")
                .bind(user_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row)
    }

    pub async fn get_user_role(&self, user_id: i64) -> Result<Option<String>> {
        let row = sqlx::query_as::<_, (String,)>("SELECT role FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.0))
    }

    pub async fn get_all_users(&self) -> Result<Vec<(i64, String, String, String)>> {
        let rows = sqlx::query_as::<_, (i64, String, String, String)>(
            "SELECT id, username, role, created_at FROM users ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn delete_user(&self, id: i64) -> Result<bool> {
        let result = sqlx::query("DELETE FROM users WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn count_admins(&self) -> Result<i64> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users WHERE role = 'admin'")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0)
    }

    // ── Sessions ──

    pub async fn create_session(
        &self,
        token: &str,
        user_id: i64,
        expires_at: NaiveDateTime,
    ) -> Result<()> {
        sqlx::query("INSERT INTO sessions (token, user_id, expires_at) VALUES (?, ?, ?)")
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

    // ── API Keys ──

    pub async fn create_api_key(
        &self,
        name: &str,
        key_hash: &str,
        preview: &str,
        expires_at: Option<&str>,
    ) -> Result<i64> {
        let result = sqlx::query(
            "INSERT INTO api_keys (name, key_hash, preview, expires_at) VALUES (?, ?, ?, ?)",
        )
        .bind(name)
        .bind(key_hash)
        .bind(preview)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    pub async fn validate_api_key(&self, key_hash: &str) -> Result<bool> {
        let row = sqlx::query_as::<_, (i64,)>(
            "SELECT id FROM api_keys WHERE key_hash = ? AND (expires_at IS NULL OR expires_at > datetime('now'))",
        )
        .bind(key_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.is_some())
    }

    pub async fn get_all_api_keys(
        &self,
    ) -> Result<Vec<(i64, String, String, Option<String>, String)>> {
        let rows = sqlx::query_as::<_, (i64, String, String, Option<String>, String)>(
            "SELECT id, name, preview, expires_at, created_at FROM api_keys ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn delete_api_key(&self, id: i64) -> Result<bool> {
        let result = sqlx::query("DELETE FROM api_keys WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    // ── User Preferences ──

    pub async fn get_user_preferences(&self, user_id: i64) -> Result<(String, String, String)> {
        let row = sqlx::query_as::<_, (String, String, String)>(
            "SELECT theme, language, timezone FROM user_preferences WHERE user_id = ?",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.unwrap_or_else(|| ("system".to_string(), "en".to_string(), "auto".to_string())))
    }

    pub async fn save_user_preferences(
        &self,
        user_id: i64,
        theme: &str,
        language: &str,
        timezone: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO user_preferences (user_id, theme, language, timezone, updated_at)
            VALUES (?, ?, ?, ?, datetime('now'))
            ON CONFLICT(user_id) DO UPDATE SET
                theme = excluded.theme,
                language = excluded.language,
                timezone = excluded.timezone,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(user_id)
        .bind(theme)
        .bind(language)
        .bind(timezone)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
