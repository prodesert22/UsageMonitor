use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq)]
pub enum SpendPanelError {
    #[error("Provider '{0}' not found")]
    ProviderNotFound(String),

    #[error("Authentication failed for '{0}': {1}")]
    AuthFailed(String, String),

    #[error("HTTP request failed: {0}")]
    NetworkError(String),

    #[error("Rate limited by '{0}', retry after {1:?}s")]
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
