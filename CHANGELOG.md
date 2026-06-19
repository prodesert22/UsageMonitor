# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Full per-release
notes live in [`releases/`](releases/).

## [0.7.1]

Fixes KDE widget presentation details found after the 0.7.0 widget installer
work.

### Changed
- Bumped the CLI workspace and KDE plasmoid version to `0.7.1`.

### Fixed
- Widget reset descriptions are now capitalized at the Rust payload layer (for
  example, `Resets at HH:MM`), so the KDE UI no longer renders duplicate text
  like `Resets: resets ...`.
- The KDE widget installer now registers the Usage Monitor icon in the user
  hicolor theme and the plasmoid metadata references it, so Plasma's widget
  chooser shows the project icon instead of the stock monitor icon.

See [releases/v0.7.1.md](releases/v0.7.1.md).

## [0.7.0]

Adds a built-in widget installer and folds the desktop widgets into the CLI
crate so a single `cargo install` ships everything.

### Added
- `usage-monitor-cli widget install <kde|waybar|all>` installs the embedded
  widgets: the KDE plasmoid via `kpackagetool6` (with a `--force` direct-copy
  fallback) and the Waybar wrapper symlinked into `~/.local/bin`. Companion
  `widget uninstall <target>` and `widget doctor` (prints resolved install
  paths and tool availability) round out the workflow.
- Automatic widget upgrades: `widget install` records the installed version and
  adds a login autostart entry that runs the new `widget sync`, which reinstalls
  any installed widget older than the CLI. Upgrading the binary now propagates to
  the desktop widgets on the next login without a manual reinstall.

### Changed
- Merged the former `usage-monitor-core` crate into `usage-monitor-cli` as
  internal library modules so crates.io publishing only needs the CLI package.
- Widget sources now live in the CLI asset tree
  (`usage-monitor-cli/assets/{kde,waybar}`) and are embedded in the binary; the
  `widgets/` directory keeps only the Python unit tests, which load the helpers
  from the asset tree.
- Bumped the CLI workspace and KDE plasmoid version to `0.7.0`.

### Fixed
- The widget installer no longer copies `__pycache__`/`.pyc` files from the
  asset tree into the installed KDE plasmoid or Waybar staging directory.
- The KDE panel bar now shows the bundled Usage Monitor logo and scales it to the
  panel thickness, so the icon is no longer clipped (and hidden) on thin panels.

See [releases/v0.7.0.md](releases/v0.7.0.md).

## [0.6.1]

Polishes the project presentation and keeps the KDE plasmoid on Plasma's stock
system-monitor icon.

### Added
- Project logo asset under `assets/` and a centered logo in the README.
- README badges for CI, license, latest GitHub release, Rust 2024 edition, last
  commit, and Linux platform support.
- GitHub Actions CI workflow covering Rust formatting, Clippy, Rust tests, Ruff,
  and widget Python tests.

### Changed
- The KDE plasmoid metadata, About page, and panel representation use Plasma's
  default `utilities-system-monitor` icon again.
- Bumped the CLI/core workspace and KDE plasmoid version to `0.6.1`.

See [releases/v0.6.1.md](releases/v0.6.1.md).

## [0.6.0]

Adds desktop widget support for KDE Plasma 6 and Waybar.

### Added
- `usage-monitor-cli widget waybar` emits Waybar-compatible single-line JSON, and
  `usage-monitor-cli widget kde` emits the same payload (with `--pretty`) for the
  KDE helper.
- KDE Plasma 6 plasmoid under `widgets/kde/package`, a faithful port of the
  `codexbar-kde` plasmoid adapted to Usage Monitor: usage popup with one card per
  provider/account (Session/Weekly/Monthly bars, reset times, stale/error states),
  a "keep popup open" pin button, and a native KDE config window split into
  **General**, **Providers**, and **Order** pages plus the native **About** page.
- KDE account management: add/remove named accounts per provider with a form
  shaped to each provider's auth type (API key / token / cookie / OAuth
  credentials path), plus add/remove opencode-go workspaces.
- Account emails now appear for each account: Codex decodes its email from the
  OAuth `id_token`, shown in the CLI text output (`Account:` line), the widget
  JSON (`account_email`), and the KDE usage cards (falling back to the plan when
  no email is available, e.g. Claude).
- Waybar wrapper script under `widgets/waybar/usage-monitor-waybar`.
- Widget docs (`docs/widgets/`), a local quality gate, and an Installation
  troubleshooting section.

### Changed
- The CLI crate was split into smaller Rust modules (`cli`, `commands`,
  `dynamic`, `fetch`, `output`, `widget`).
- The CLI now identifies itself as `usage-monitor-cli` consistently (version
  string and help output).
- KDE settings moved out of the popup into the native KDE config dialog, with
  working Apply/OK/Cancel for display preferences; provider toggles, accounts,
  workspaces, and cache clear apply immediately.
- The plasmoid About page is populated from `metadata.json` and its bug-report
  link points to the project issues.

See [releases/v0.6.0.md](releases/v0.6.0.md).

## [0.5.2]

Localizes the human-readable output's timestamps; `--json` stays raw UTC.

### Changed
- `Collected at` renders in the system-local timezone with the UTC offset
  (e.g. `00:16 14/06/2026 (UTC-03:00)`).
- Reset times are relative and local: today → `resets at HH:MM`, tomorrow →
  `resets tomorrow at HH:MM`, otherwise `resets <Weekday> dd/mm at HH:MM`.
- `--json` output is unchanged (raw UTC RFC 3339 timestamps).

See [releases/v0.5.2.md](releases/v0.5.2.md).

## [0.5.1]

Adds the two protobuf-based providers, bringing the total to 28 native fetchers.

### Added
- `grok` — credit usage via the `GetGrokCreditsConfig` gRPC-Web RPC (the
  protobuf response is generically scanned). Bearer-token or cookie auth.
- `windsurf` — daily/weekly quota via the `GetPlanStatus` Connect RPC (raw
  protobuf, exact field numbers). Devin-session-token auth.
- `provider::proto` — a minimal, bounds-checked protobuf wire reader/encoder
  shared by the protobuf providers.
- Per-provider docs for `grok` and `windsurf`.

See [releases/v0.5.1.md](releases/v0.5.1.md).

## [0.5.0]

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
