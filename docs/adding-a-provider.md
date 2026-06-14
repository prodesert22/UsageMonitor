# Adding a provider

Every provider is a self-contained module in
`usage-monitor-core/src/provider/<id>.rs` that implements the `UsageProvider`
trait and maps the upstream response onto a `UsageSnapshot`. This guide walks
through the established pattern.

## 1. Create the module

Model it on an existing provider with a similar auth/shape — for example
[`deepseek.rs`](../usage-monitor-core/src/provider/deepseek.rs) (API-key balance),
[`cursor.rs`](../usage-monitor-core/src/provider/cursor.rs) (browser cookie), or
[`windsurf.rs`](../usage-monitor-core/src/provider/windsurf.rs) (protobuf).

A minimal HTTP/JSON provider:

```rust
use async_trait::async_trait;
use crate::error::SpendPanelError;
use crate::model::{CreditsSnapshot, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

pub struct AcmeProvider {
    metadata: ProviderMetadata,
    base_url: Option<String>, // overridable in tests
}

impl AcmeProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "acme",
                name: "Acme",
                description: "Acme API balance monitor",
                auth_methods: &["api_key", "env"],
                website: Some("https://acme.example"),
            },
            base_url: None,
        }
    }

    pub fn with_base_url(url: &str) -> Self {
        let mut p = Self::new();
        p.base_url = Some(url.to_string());
        p
    }

    fn api_base(&self) -> &str {
        self.base_url.as_deref().unwrap_or("https://api.acme.example")
    }

    fn resolve_key(ctx: &ProviderContext) -> Result<String, SpendPanelError> {
        // config first (api_key / token), then env vars
        for key in ["api_key", "token"] {
            if let Some(v) = ctx.config.get(key).map(|s| s.trim()).filter(|s| !s.is_empty()) {
                return Ok(v.to_string());
            }
        }
        if let Ok(v) = std::env::var("ACME_API_KEY") {
            if !v.trim().is_empty() { return Ok(v.trim().to_string()); }
        }
        Err(SpendPanelError::AuthFailed(
            "acme".into(),
            "no API key in api_key/token config or ACME_API_KEY".into(),
        ))
    }
}

impl Default for AcmeProvider {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl UsageProvider for AcmeProvider {
    fn metadata(&self) -> &ProviderMetadata { &self.metadata }

    fn detect_credentials(&self) -> bool {
        std::env::var("ACME_API_KEY").map(|v| !v.trim().is_empty()).unwrap_or(false)
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let key = Self::resolve_key(ctx)?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;
        let resp = client
            .get(format!("{}/balance", self.api_base().trim_end_matches('/')))
            .header("Authorization", format!("Bearer {}", key))
            .send().await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;
        // map status → AuthFailed / ProviderError, then parse the body and
        // build a UsageSnapshot (credits / rate windows / plan).
        let mut snapshot = UsageSnapshot::new("acme");
        snapshot.credits = Some(CreditsSnapshot::new(0.0, "USD"));
        Ok(snapshot)
    }
}
```

## 2. Register it

- Add `pub mod acme;` in
  [`provider/mod.rs`](../usage-monitor-core/src/provider/mod.rs) (keep the list
  alphabetical).
- Register it in `with_defaults()` in
  [`provider/registry.rs`](../usage-monitor-core/src/provider/registry.rs):
  ```rust
  reg.register(Box::new(super::acme::AcmeProvider::new()));
  ```

No CLI change is needed: the CLI dispatches provider config commands
(`acme set ...`, `acme account ...`, `enable acme`, …) dynamically by id.

## 3. Conventions

- **Auth resolution order**: explicit account config keys first, then env vars.
  Trim values and strip surrounding quotes (see the `clean` helper most modules
  carry).
- **Status mapping**: `401`/`403` → `SpendPanelError::AuthFailed`; other non-2xx →
  `SpendPanelError::ProviderError`; bad JSON/proto → `SpendPanelError::ParseError`.
- **`detect_credentials`**: only return true for a zero-config signal (an env var,
  or a credentials file). Account-configured providers auto-enable separately.
- **`UsageSnapshot` mapping**: use `primary`/`secondary`/`tertiary` rate windows
  for the headline quotas, `extra_rate_windows` for additional model lanes,
  `credits`/`cost` for balances/spend, and `plan` for the subscription name.
- **Percent vs. fraction**: confirm whether the upstream value is `0..1` or
  `0..100` before mapping — getting this wrong is the most common bug.
- **Protobuf providers**: reuse [`provider/proto.rs`](../usage-monitor-core/src/provider/proto.rs)
  for the wire reader/encoder rather than pulling in a codegen dependency.

## 4. Tests

Add a `#[cfg(test)] mod tests` with:

- pure unit tests for parsing/mapping (feed sample JSON/bytes into the parse fn);
- a `wiremock` integration test using `with_base_url(&server.uri())` for the happy
  path and at least one `401` → `AuthFailed` case.

```bash
cargo test -p usage-monitor-core -- provider::acme
cargo test            # whole workspace
cargo clippy          # keep it warning-clean
```

The CLI test `test_list_shows_all_providers_auto_disabled_without_credentials`
asserts the total provider count — update it when you add a provider.

## 5. Document & release

- Add `docs/providers/acme.md` (auth, data source, behavior, multi-account
  example) and a row in [`docs/providers/README.md`](providers/README.md) and the
  top-level [README](../README.md) provider table.
- Add a release-notes file under `releases/` and a `CHANGELOG.md` entry following
  the existing format.

## Checklist

- [ ] `provider/<id>.rs` implementing `UsageProvider` (+ `with_base_url`)
- [ ] `pub mod <id>;` in `provider/mod.rs`
- [ ] registered in `registry.rs` `with_defaults()`
- [ ] unit + wiremock tests, `cargo test` green, `cargo clippy` clean
- [ ] updated the provider-count CLI test
- [ ] `docs/providers/<id>.md` + README/index rows
- [ ] release notes + CHANGELOG entry
