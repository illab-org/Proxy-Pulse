use serde::{Deserialize, Serialize};

/// Checker configuration — stored in database (system_settings table)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckerConfig {
    pub interval_secs: u64,
    pub timeout_secs: u64,
    pub max_concurrent: usize,
    pub targets: Vec<String>,
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
        }
    }
}
