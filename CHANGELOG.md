# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Full per-release
notes live in [`releases/`](releases/).

## [0.5.0] — unreleased

Adds 13 new native Linux fetchers ported from the CodexBar macOS app, bringing
the total to 26 providers with real usage fetching.

### Added
- Native fetchers for `cursor`, `copilot`, `perplexity`, `gemini`,
  `antigravity`, `abacus`, `devin`, `kimi`, `kimik2`, `minimax`, `mistral`,
  `ollama`, and `zai`, each with mock-server tests.
- Per-provider docs under `docs/providers/` for all 13.
- Dynamic provider config subcommands (no per-provider clap enum variant).
- `help`/`-h`/`--help` (and a bare `<provider>` / `<provider> account`) now print
  contextual usage for dynamic provider and account commands.

### Changed
- Dropped the catalog-only provider registrations: the registry now holds only
  the 26 providers with real fetchers (`grok`/`windsurf` need a separate
  protobuf RPC port and are not included).

### Fixed
- `<provider> account add -h` (and other help flags on dynamic subcommands) no
  longer get consumed as an account name/value — help is intercepted before
  parsing, so it can't create a junk account.

### Notes
- Browser-cookie providers take the session cookie/token from config or an env
  var instead of auto-importing from a browser (which is macOS-specific).
- The OAuth providers `gemini` and `antigravity` reuse the respective CLI/app
  credentials (`~/.gemini/oauth_creds.json`, `~/.codexbar/antigravity/oauth_creds.json`)
  with automatic token refresh.

See [releases/v0.5.0.md](releases/v0.5.0.md).

## [0.4.0]

Multi-account support: every provider can track several logins or keys side by
side, with per-account `add`/`remove`/`list`/`set`/`unset`/`enable`/`disable`/`auto`
commands. See [releases/v0.4.0.md](releases/v0.4.0.md).

## [0.3.3]

Hardens OpenCode Go workspace configuration validation.
See [releases/v0.3.3.md](releases/v0.3.3.md).

## [0.3.2]

Improves OpenCode Go terminal output for multiple workspaces (framed headers).
See [releases/v0.3.2.md](releases/v0.3.2.md).

## [0.3.1]

Runs enabled providers in parallel during a bare `fetch`.
See [releases/v0.3.1.md](releases/v0.3.1.md).

## [0.3.0]

Standardizes the CLI around provider-first configuration commands and eases
OpenCode Go cookie auth. See [releases/v0.3.0.md](releases/v0.3.0.md).

## [0.2.0]

Adds the `opencode-go` provider with multi-workspace support and provider config
commands. See [releases/v0.2.0.md](releases/v0.2.0.md).

## [0.1.3]

Adds the provider enable/disable system, mirroring CodexBar's toggles.
See [releases/v0.1.3.md](releases/v0.1.3.md).

## [0.1.2]

Adds the `codex` provider for ChatGPT-plan Codex usage.
See [releases/v0.1.2.md](releases/v0.1.2.md).

## [0.1.1]

Adds color to the CLI usage bars. See [releases/v0.1.1.md](releases/v0.1.1.md).

## [0.1.0]

Initial release: a Linux port of [CodexBar](https://github.com/steipete/CodexBar)
as a Rust library + CLI for monitoring AI service usage from the terminal, with
the `claude`, `anthropic`, and `openai` providers.
See [releases/v0.1.0.md](releases/v0.1.0.md).
