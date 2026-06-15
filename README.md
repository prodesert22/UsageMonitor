# Usage Monitor

<p align="center">
  <img src="assets/UsageMonitor.png" alt="Usage Monitor logo" width="128">
</p>

[![CI](https://github.com/prodesert22/UsageMonitor/actions/workflows/ci.yml/badge.svg)](https://github.com/prodesert22/UsageMonitor/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/usage-monitor-cli?logo=rust)](https://crates.io/crates/usage-monitor-cli)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/prodesert22/UsageMonitor?include_prereleases&logo=github)](https://github.com/prodesert22/UsageMonitor/releases)
[![Rust](https://img.shields.io/badge/rust-2024-edition?logo=rust)](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0.html)
[![Last commit](https://img.shields.io/github/last-commit/prodesert22/UsageMonitor)](https://github.com/prodesert22/UsageMonitor/commits/main)
[![Platform](https://img.shields.io/badge/platform-linux-lightgrey)](docs/installation.md)

AI API usage monitor for your terminal.

Collects, stores, and displays consumption metrics from AI services like
Anthropic Claude, OpenAI, DeepSeek, Groq, and many more — all without
depending on external servers.

A Linux port of [CodexBar](https://github.com/steipete/CodexBar) by
[Peter Steinberger](https://github.com/steipete), reimplemented in Rust.

## Documentation

Full docs live in [`docs/`](docs/README.md):

- [Installation](docs/installation.md) — prerequisites, build, install, PATH.
- [Commands & usage](docs/commands.md) — full CLI reference and examples.
- [Configuration](docs/configuration.md) — `config.toml`, accounts, env vars.
- [Provider index](docs/providers/README.md) — every provider and its auth.
- [Desktop widgets](docs/widgets/README.md) — KDE Plasma 6 and Waybar integration.
- [Architecture](docs/architecture.md) — crates, data model, provider trait,
  registry, fetch flow.
- [Adding a provider](docs/adding-a-provider.md) — porting guide + checklist.
- [Quality checks](docs/quality.md) — rustfmt, Clippy, QML/widget tests.
- [Credits & license](docs/credits.md).

## Install

From [crates.io](https://crates.io/crates/usage-monitor-cli):

```bash
cargo install usage-monitor-cli
# installs `usage-monitor-cli` into ~/.cargo/bin (make sure it is on your PATH)
```

Or from a local checkout: `cargo install --path usage-monitor-cli`. Then,
optionally, install a desktop widget:

```bash
usage-monitor-cli widget install all   # KDE plasmoid + Waybar wrapper
```

See [docs/installation.md](docs/installation.md) for prerequisites and PATH
setup.

## Usage

```bash
usage-monitor-cli list           # providers and their resolved state
usage-monitor-cli fetch          # every enabled provider, concurrently
usage-monitor-cli fetch claude   # a single provider
```

Providers auto-enable when their credentials are detected; persist a key or
cookie when there is nothing to detect:

```bash
usage-monitor-cli anthropic set api_key sk-ant-...
usage-monitor-cli fetch anthropic --json     # machine-readable output
```

Each provider can hold several named **accounts** (e.g. a personal and a work
Claude), and `fetch` emits one block per enabled account. The full command
reference, the multi-account model, and more examples are in
[docs/commands.md](docs/commands.md).

## Providers

28 native Linux fetchers ship today — subscription plans (`claude`, `codex`,
`gemini`, …), metered APIs (`anthropic`, `openai`, `deepseek`, `groq`, …), and
cookie-based dashboards (`cursor`, `perplexity`, `mistral`, …). Auth and
extraction were ported from the CodexBar macOS app; browser-cookie providers
take the cookie/token from config rather than auto-importing from a browser.

The full table with each provider's auth and auto-enable rule lives in
[docs/providers/](docs/providers/README.md).

## Desktop widgets

Usage Monitor ships KDE Plasma 6 and Waybar widgets, embedded in the binary and
installed with `usage-monitor-cli widget install <kde|waybar|all>`. See
[docs/widgets/](docs/widgets/README.md).

## Structure

```
usage-monitor-cli/      CLI package, internal library modules, and embedded
                        widget assets (assets/kde, assets/waybar)
widgets/                Widget unit tests (KDE/Waybar helpers)
docs/                   Guides + per-provider specifications
releases/               Per-version release notes
```

See [docs/architecture.md](docs/architecture.md) for a detailed breakdown.

## Tests

```bash
cargo test --workspace
python -m unittest discover -s widgets -p 'test_*.py'   # widget helpers
.githooks/pre-commit                                    # full quality gate
```

See [docs/quality.md](docs/quality.md) for the complete quality gate.

## Credits

Concept, provider research, and original macOS implementation:
[steipete/CodexBar](https://github.com/steipete/CodexBar) (MIT). This project
ports the idea to Linux as a Rust library + CLI. See
[docs/credits.md](docs/credits.md).

## License

MIT — see [LICENSE](LICENSE).
