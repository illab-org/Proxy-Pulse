use serde::{Deserialize, Serialize};

fn default_fail_intervals_secs() -> Vec<u64> {
    vec![15, 30, 60, 120, 180, 300, 600, 900, 1200, 1800]
}

/// Checker configuration — stored in database (system_settings table)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckerConfig {
    // Interval for proxies that just succeeded.
    pub interval_secs: u64,
    pub timeout_secs: u64,
    pub max_concurrent: usize,
    pub targets: Vec<String>,
    #[serde(default = "default_fail_intervals_secs")]
    pub fail_intervals_secs: Vec<u64>,
}

impl Default for CheckerConfig {
    fn default() -> Self {
        Self {
            interval_secs: 60,
            timeout_secs: 10,
            max_concurrent: 200,
            targets: vec![
                "https://httpbin.org/ip".to_string(),
                "https://www.cloudflare.com/cdn-cgi/trace".to_string(),
            ],
            fail_intervals_secs: default_fail_intervals_secs(),
        }
    }
}
