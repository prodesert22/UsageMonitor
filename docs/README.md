# Usage Monitor documentation

Usage Monitor is a Linux port of [CodexBar](https://github.com/steipete/CodexBar)
as a Rust library + CLI for monitoring AI service usage from the terminal.

## Guides

- [Installation](installation.md) — build from source, install, prerequisites, PATH.
- [Commands & usage](commands.md) — full CLI command reference and examples.
- [Architecture](architecture.md) — workspace layout, data model, provider trait,
  registry, fetch flow, configuration, and error model.
- [Configuration](configuration.md) — the `config.toml` format, accounts,
  enable/disable resolution, and environment variables.
- [Desktop widgets](./widgets/README.md) — KDE Plasma 6 and Waybar integration.
- [Quality checks](quality.md) — rustfmt, Clippy, QML/Python/widget checks, and
  file-size limits.
- [Adding a provider](adding-a-provider.md) — step-by-step guide to porting a new
  provider, with conventions and a checklist.
- [Credits & license](credits.md) — attribution to CodexBar and licensing.

## Providers

- [Provider index](providers/README.md) — every supported provider, what it
  monitors, and how it authenticates.
- Each provider has its own page under [`providers/`](providers/) with auth keys,
  data sources, behavior, and multi-account examples.

## Quick links

- Top-level [README](../README.md) — overview, command reference, examples.
- [CHANGELOG](../CHANGELOG.md) — per-version summary.
- [`releases/`](../releases/) — full release notes.
