use anyhow::Result;

use crate::models::{CountryCount, LatencyBucket, ProtocolCount, ProxyStats, ScoreBucket};

use super::Database;

impl Database {
    pub async fn get_stats(&self) -> Result<ProxyStats> {
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

        let countries = sqlx::query_as::<_, CountryCount>(
            r#"
            SELECT country, COUNT(*) as count
            FROM proxies WHERE is_alive = 1
            GROUP BY country ORDER BY count DESC LIMIT 20
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let latency_dist = self.get_latency_distribution().await?;

        let protocols = sqlx::query_as::<_, ProtocolCount>(
            r#"
            SELECT protocol, COUNT(*) as count
            FROM proxies WHERE is_alive = 1
            GROUP BY protocol ORDER BY count DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

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
        let rows: Vec<(f64,)> = sqlx::query_as(
            "SELECT avg_latency_ms FROM proxies WHERE is_alive = 1 AND avg_latency_ms > 0 ORDER BY avg_latency_ms",
        )
        .fetch_all(&self.pool)
        .await?;

        if rows.is_empty() {
            return Ok(vec![]);
        }

        let values: Vec<f64> = rows.into_iter().map(|r| r.0).collect();
        let n = values.len();

        if n < 5 {
            let mut buckets = Vec::new();
            for v in &values {
                let label = format!("{}ms", v.round() as i64);
                if let Some(last) = buckets.last_mut() {
                    let b: &mut LatencyBucket = last;
                    if b.range == label {
                        b.count += 1;
                        continue;
                    }
                }
                buckets.push(LatencyBucket {
                    range: label,
                    count: 1,
                });
            }
            return Ok(buckets);
        }

        let chunk = n / 5;
        let mut boundaries = Vec::with_capacity(6);
        boundaries.push(values[0]);
        for i in 1..5 {
            let idx = i * chunk;
            let raw = values[idx];
            let nice = if raw < 100.0 {
                (raw / 10.0).ceil() * 10.0
            } else if raw < 1000.0 {
                (raw / 50.0).ceil() * 50.0
            } else {
                (raw / 100.0).ceil() * 100.0
            };
            let prev = *boundaries.last().unwrap();
            if nice <= prev {
                boundaries.push(prev + 1.0);
            } else {
                boundaries.push(nice);
            }
        }
        boundaries.push(f64::INFINITY);

        let mut buckets: Vec<LatencyBucket> = Vec::with_capacity(5);
        for i in 0..5 {
            let lo = boundaries[i];
            let hi = boundaries[i + 1];
            let count = values.iter().filter(|&&v| v >= lo && v < hi).count() as i64;
            let range = if hi.is_infinite() {
                format!("{}ms+", lo.round() as i64)
            } else {
                format!("{}-{}ms", lo.round() as i64, hi.round() as i64)
            };
            buckets.push(LatencyBucket { range, count });
        }

        // Merge empty buckets
        loop {
            if let Some(pos) = buckets.iter().position(|b| b.count == 0) {
                if buckets.len() <= 1 {
                    break;
                }
                buckets.remove(pos);
            } else {
                break;
            }
        }

        Ok(buckets)
    }

    async fn get_score_distribution(&self) -> Result<Vec<ScoreBucket>> {
        let row: (i64, i64, i64, i64, i64, i64) = sqlx::query_as(
            r#"
            SELECT
                SUM(CASE WHEN score >= 0 AND score < 20 THEN 1 ELSE 0 END),
                SUM(CASE WHEN score >= 20 AND score < 40 THEN 1 ELSE 0 END),
                SUM(CASE WHEN score >= 40 AND score < 60 THEN 1 ELSE 0 END),
                SUM(CASE WHEN score >= 60 AND score < 80 THEN 1 ELSE 0 END),
                SUM(CASE WHEN score >= 80 AND score < 90 THEN 1 ELSE 0 END),
                SUM(CASE WHEN score >= 90 AND score <= 100 THEN 1 ELSE 0 END)
            FROM proxies
            WHERE success_count >= 1
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(vec![
            ScoreBucket { range: "0-20".to_string(), count: row.0 },
            ScoreBucket { range: "20-40".to_string(), count: row.1 },
            ScoreBucket { range: "40-60".to_string(), count: row.2 },
            ScoreBucket { range: "60-80".to_string(), count: row.3 },
            ScoreBucket { range: "80-90".to_string(), count: row.4 },
            ScoreBucket { range: "90-100".to_string(), count: row.5 },
        ])
    }
}
