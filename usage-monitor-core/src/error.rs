use thiserror::Error;

fn rate_limited_message(provider: &str, retry_after: &Option<u64>) -> String {
    match retry_after {
        Some(secs) if *secs > 0 => format!("Rate limited by '{}', retry after {}s", provider, secs),
        _ => format!("Rate limited by '{}', try again in a few minutes", provider),
    }
}

#[derive(Error, Debug, Clone, PartialEq)]
pub enum SpendPanelError {
    #[error("Provider '{0}' not found")]
    ProviderNotFound(String),

    #[error("Authentication failed for '{0}': {1}")]
    AuthFailed(String, String),

    #[error("HTTP request failed: {0}")]
    NetworkError(String),

    #[error("{}", rate_limited_message(.0, .1))]
    RateLimited(String, Option<u64>),

    #[error("Failed to parse response from '{0}': {1}")]
    ParseError(String, String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Provider '{0}' returned error: {1}")]
    ProviderError(String, String),

    #[error("Timeout fetching '{0}'")]
    Timeout(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limited_message_with_seconds() {
        let e = SpendPanelError::RateLimited("claude".into(), Some(120));
        assert_eq!(e.to_string(), "Rate limited by 'claude', retry after 120s");
    }

    #[test]
    fn test_rate_limited_message_zero_seconds() {
        let e = SpendPanelError::RateLimited("claude".into(), Some(0));
        assert_eq!(
            e.to_string(),
            "Rate limited by 'claude', try again in a few minutes"
        );
    }

    #[test]
    fn test_rate_limited_message_no_seconds() {
        let e = SpendPanelError::RateLimited("codex".into(), None);
        assert_eq!(
            e.to_string(),
            "Rate limited by 'codex', try again in a few minutes"
        );
    }
}
