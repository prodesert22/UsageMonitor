# Architecture

Usage Monitor is a Cargo workspace with one publishable Rust package:

```
usage-monitor-cli/      CLI binary plus library modules for providers/config/model
docs/                   This documentation, plus per-provider specs
releases/               Per-version release notes
```

The package has both a binary target and library modules. The binary parses
arguments, loads config, asks the registry to fetch, and renders the results. All
provider logic, the data model, and configuration live in library modules under
`usage-monitor-cli/src/` so they can be reused internally and unit-tested
independently while publishing only one crate.

## Data model (`model/`)

A fetch produces a `UsageSnapshot` — the single shape every provider maps onto:

- `provider_id`, `account_id`, `account_label`, `collected_at`
- up to three headline `RateWindow`s (`primary`/`secondary`/`tertiary`) plus
  `extra_rate_windows` (named) for additional model/quota lanes
- optional `credits` (`CreditsSnapshot`), `cost` (`CostSnapshot`), and `plan`
  (`PlanInfo`)

Key types:

- **`RateWindow`** — a usage window with `used`/`limit`, a derived `usage_ratio`,
  a `status` (`Normal`/`Warning`/`Critical`/`Exhausted`/`Unknown`, from the
  ratio), an optional `resets_at`, and a `window_minutes` (e.g. 1440 = daily).
- **`CreditsSnapshot`** — `balance` + `currency`, with optional `total`, `used`,
  `bonus`, `purchased`, and `renews_at`.
- **`CostSnapshot`** — `total_cost` + `currency`, optional `daily_costs` and a
  `spend_limit`.
- **`PlanInfo`** — plan `name`/`tier`/`price`/`billing_period`.

Because every provider returns the same `UsageSnapshot`, the CLI renders all of
them with one code path (text or `--json`).

## Providers (`provider/`)

Every provider implements one trait:

```rust
#[async_trait]
pub trait UsageProvider: Send + Sync {
    fn metadata(&self) -> &ProviderMetadata;
    async fn fetch_usage(&self, ctx: &ProviderContext)
        -> Result<UsageSnapshot, SpendPanelError>;
    fn detect_credentials(&self) -> bool { false }
}
```

- **`ProviderMetadata`** — static `id`, `name`, `description`, `auth_methods`,
  `website`.
- **`ProviderContext`** — the per-fetch config: a `HashMap<String, String>` of
  resolved values (`api_key`, `token`, `cookie`, `credentials_path`, `base_url`,
  …) plus a `timeout_secs`. The CLI builds it from the active account's config
  and any CLI overrides.
- **`detect_credentials`** — lets a provider auto-enable when its credentials are
  present (an env var, or a credentials file on disk), without an explicit toggle.

Providers are self-contained modules (`provider/<id>.rs`). Most are HTTP/JSON;
two speak protobuf (`grok` over gRPC-Web, `windsurf` over Connect) and share the
minimal wire reader in [`provider/proto.rs`](../usage-monitor-cli/src/provider/proto.rs).
Each provider exposes a `with_base_url` constructor so its tests can point at a
mock HTTP server.

## Registry (`provider/registry.rs`)

`ProviderRegistry::with_defaults()` constructs and registers every provider. The
registry is the orchestration layer:

- `enabled_ids(config)` / `provider_state(id, config)` — resolve which providers
  are on (see [Configuration](configuration.md)).
- `provider_targets` / `enabled_targets` — expand each enabled provider into one
  `AccountTarget` per configured account (plus the implicit auto-detected
  `default`).
- `fetch_targets(...)` — fetches a list of targets **concurrently**
  (`futures_util::join_all`) and stamps each snapshot with its account id/label.

## Fetch flow

```
CLI args ─▶ AppConfig::load() ─▶ ProviderRegistry::with_defaults()
        ─▶ registry.enabled_targets(config)        (which providers × accounts)
        ─▶ registry.fetch_targets(...)             (concurrent HTTP/protobuf)
              └─ provider.fetch_usage(ctx) ─▶ UsageSnapshot
        ─▶ render text or JSON
```

A bare `fetch` runs every enabled account concurrently; `fetch <provider>` runs a
single provider; `--account <name>` restricts to one account.

## Configuration (`config.rs`)

`AppConfig` is the persisted state, serialized to TOML at
`$XDG_CONFIG_HOME/usage-monitor/config.toml` (or `~/.config/...`). It holds
per-provider settings, each with named accounts and an optional explicit
enable/disable toggle. `resolve_state(id, credentials_detected)` decides the final
`ProviderState`. Details in [Configuration](configuration.md).

## Error model (`error.rs`)

All fallible paths return `SpendPanelError`, a `thiserror` enum with variants for
the common failure modes: `ProviderNotFound`, `AuthFailed`, `NetworkError`,
`RateLimited`, `ParseError`, `ConfigError`, `ProviderError`, and `Timeout`. A
failed fetch for one account never aborts the others — each target's result is
reported independently.

## Concurrency

Fetching is async on Tokio. Independent provider/account fetches run in parallel,
so a bare `fetch` is bounded by the slowest provider rather than their sum.
