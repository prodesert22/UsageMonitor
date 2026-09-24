# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Full per-release
notes live in [`releases/`](releases/).

## [Unreleased]

### Added
- GNOME Shell extension (45–49) with the KDE widget layout: top-bar
  indicator, one card per provider/account, cost, and Preferences with
  General/Providers/Order/Theme/Updates pages. `usage-monitor-cli widget
  install gnome` installs it locally (no store needed, GSettings schemas
  compiled in place); `widget gnome` emits the same JSON contract as the
  other widgets. The store listing ships only the extension — without the
  CLI the popup shows a guided install notice instead of failing.

## [0.9.0]

Brings Gemini browser login, per-account bar pins, bar window checkboxes,
a widget self-update flow, and moves opencode-go to the official Zen usage
endpoint (workspace pinning removed).

### Added
- `usage-monitor-cli gemini login` — browser-based Gemini OAuth login (PKCE)
  that writes `~/.gemini/oauth_creds.json`, plus `gemini status` and
  `gemini logout`.
- **Pin provider to panel bar (KDE) / popup header (Waybar)**: the settings
  now list one entry per account — a lone login keeps the provider name
  (`codex`); several accounts show `provider/account` (`codex/work`). A
  provider-level pin keeps working as before. The pin dropdown is disabled
  while "Show bar text" is off.
- **Session/Weekly/Monthly window checkboxes** in the General settings page
  (both KDE and Waybar): pick which usage windows compose the bar/header
  text. Named extra rate limits are matched to their own slot by id and no
  longer masquerade as Monthly; any remainder shows up in the tooltip.
- `usage-monitor-cli widget kde` — quick-waybar smoke test command: the
  `kde` subcommand now tests pin resolution and cache, outputting the
  summary and pinned percent for each test.
- Parse-error output now suggests the exact `git commit` command when `.go`
  or `.json` changes are detected on-disk but the helper was compiled
  without them.
- **Widget update notice with changelog.** Both widgets compare their installed
  version with the CLI binary on every popup open (local, no network) and show
  a banner — "Update X available (installed Y)" — with **What's new** (release
  notes: the `releases/vX.Y.Z.md` file from the repo first, GitHub Releases
  API next, embedded CHANGELOG.md fallback when offline; cached for a day),
  **Update now** (one-click reinstall through the same
  path as `usage-monitor-cli widget install <target>`, no Plasma restart
  needed) and **Dismiss** (hides the popup notice for that version only; the
  settings **Updates** tab always shows a pending update).
- Dedicated **Updates** tab in both widgets' settings (versions, status,
  one-click reinstall, release-page link), which also fetches and renders the
  changelog — dismissing the popup banner never hides it.
- `usage-monitor-cli widget check-update [kde|waybar|all]` — installed vs
  binary versions without changing anything; `widget changelog <version>` —
  release notes (GitHub, embedded fallback); `widget sync [target]` now
  accepts an optional target so the widget's Update button syncs only itself.

### Fixed
- **Codex additional rate limits no longer masquerade as Monthly.** Extra
  rate windows are now matched to their slot by window id; unnamed remainders
  show in the tooltip as "additional" instead of filling the Monthly bar.
- **KDE popup no longer shows a phantom horizontal scrollbar.** The usage
  list scrolls vertically only now, so long wrapped lines (e.g. provider
  error messages) don't summon a horizontal bar (Plasma caches widget QML —
  restart `plasmashell` after updating for the new UI to take effect).

### Changed
- **opencode-go provider moved to the official Zen usage endpoint.**
  `GET https://opencode.ai/zen/go/v1/usage` with `Authorization: Bearer <key>`
  replaces the dashboard-cookie scraping (the legacy workspace pages now
  redirect to the console login, so the old session cookie is rejected). The
  key is auto-detected from `~/.local/share/opencode/auth.json` (the
  `opencode-go` entry, falling back to `opencode`) or `OPENCODE_API_KEY`,
  falling back to `opencode-go set token <key>`; a pasted `Bearer <key>` value
  is accepted while legacy cookie values are rejected with a migration hint.
  The same rolling (5h) / weekly / monthly used percents are reported, a
  rate-limited window shows as exhausted, and HTTP 429 maps to a rate-limited
  error with `retry-after`. Workspace pinning (`opencode-go workspace
  add|remove|list`, the widget workspace managers, and the `workspaces` fetch
  plumbing) was removed — one key covers the whole account, and stale
  `workspaces = [...]` config entries are ignored. If you used the cookie
  setup, set the API key and re-enable.
- Gemini accounts without a managed Cloud project are onboarded automatically
  (`onboardUser` with bounded polling) instead of failing.
- Workspace crate version, KDE plasmoid metadata and Waybar popup version
  bumped to `0.9.0`.

See [releases/v0.9.0.md](releases/v0.9.0.md).

## [0.8.1]

### Added
- **Waybar popup**: the KDE Plasma widget's QML interface, ported to plain Qt
  Quick and opened from the module's `on-click`
  (`usage-monitor-waybar-popup`, installed alongside the bar wrapper by
  `usage-monitor-cli widget install waybar`). Same popup layout as the plasmoid —
  header with summary/busy indicator, Refresh, Cost, Keep open (pin) and
  Settings, provider cards with Session/Weekly/Monthly bars, reset times, stale
  markers, per-provider errors and 30-day cost — plus a settings window with the
  General, Providers, Order and Theme pages.
- Waybar popup theming: follow the desktop colors (KDE `kdeglobals`, else the
  GTK/GNOME dark preference, overridable with `USAGE_MONITOR_DARK`), seven
  bundled themes (macOS Dark/Light, Nord, Dracula, Tokyo Night, Gruvbox Dark,
  Catppuccin Mocha), installed KDE color schemes, named custom palettes and the
  transparency slider, with a live preview.
- Waybar popup settings live in `~/.config/usage-monitor-waybar/state.json` and
  the cache in `~/.cache/usage-monitor-waybar/last.json`, separate from the KDE
  widget so both can run on the same machine.
- New data helper `usage_monitor_waybar_data.py` with a shell CLI (`summary`,
  `cache`, `settings`, `state`, `cost`, `themes`, `cache-clear`, `doctor`,
  `set-state`, `batch-set-state`).
- Popup launcher options: `--anchor`, `--width`, `--height`, `--margin`,
  `--once`, `--no-single-instance`, `--settings`, `--quit`, `--doctor`.

### Fixed
- **Bar dropped to "⚠" on any failed fetch.** The module piped a single
  `usage-monitor-cli widget waybar` call straight out, so one rate-limited or
  expired-credential round replaced the percentage with the warning glyph. It
  now goes through the shared helper: failing providers keep their last good
  value, the module is marked `stale`, and "⚠" is left for "nothing answered and
  there is no cache".
- **Providers were polled twice and rate-limited themselves.** The bar and the
  popup fetched independently on 30 s timers. They now share one fetched value
  for `minFetchIntervalSeconds` (default 180 s, Settings → General), take an
  inter-process lock around the fetch, and back off after failures too; the
  Refresh button still forces a fetch. The installer's module snippet now
  suggests `"interval": 300`.
- **Popup opened in the middle of the screen on Wayland.** It is now a
  wlr-layer-shell surface (via Qt's LayerShellQt integration) anchored to the
  configured screen edge, so the compositor places it under the bar, outside
  Waybar's exclusive zone. Falls back to the previous behaviour when the
  integration is missing; `USAGE_MONITOR_LAYER_SHELL=0` disables it, and
  `--doctor` reports `layerShell`.
- Installed desktop entry (`usage-monitor-waybar.desktop`) so the popup keeps a
  stable `app_id`/`WM_CLASS` for compositor rules and the XDG portal.
- The stored "Keep open" pin was read once at startup, before the settings
  payload arrived, and then written back — unpinning a pinned popup.
- **OpenCode Go showed usage 100× too high.** The dashboard now reports
  `usagePercent` as a percentage with decimal places (0.7 = 0.7 %), but the
  provider still treated every value ≤ 1.0 as a ratio and multiplied it by
  100 — so 0.7 % rendered as 70 % in the CLI and widgets. Values are now used
  as reported and clamped to 0–100.

### Notes for non-Plasma systems
- The popup uses PySide6 (or PyQt6) and reports the matching install command for
  the running distribution when neither is present; the Waybar module keeps
  working without Qt.
- Icons are drawn as vectors instead of freedesktop icon names, so no icon theme
  is required, and Qt Quick Controls' Fusion style is used with a palette built
  from the resolved theme.
- The first click starts a resident process holding a socket under
  `$XDG_RUNTIME_DIR` (per-uid `/tmp` fallback); later clicks toggle the existing
  window.
- Placement: on X11 the launcher positions the popup (pointer-following by
  default); on Wayland the compositor decides, so the window sets
  `app_id`/`WM_CLASS` `usage-monitor-waybar` and the docs give ready-made
  Hyprland/Sway/river rules.

### Changed
- Workspace crate version, KDE plasmoid metadata and Waybar popup version bumped
  to `0.8.1`.

See [releases/v0.8.1.md](releases/v0.8.1.md).

## [0.8.0]

Adds theming to the KDE Plasma widget: bundled themes, installed KDE color
schemes, named custom themes and a transparency slider.

### Added
- KDE widget **Theme** settings page: choose between following the current
  desktop theme (default), one of five bundled themes (macOS Dark, macOS Light,
  Nord, Dracula, Tokyo Night), any KDE color scheme or Plasma desktop theme
  already installed on the system (including third-party ones from the KDE
  Store), or a fully custom palette — colors, font family, text/title/small
  sizes, bar height, corner radius and background opacity — with a live preview
  in the config dialog.
- **Transparency slider** (0–70 %) in the Theme page, applied in every mode —
  including "follow the current desktop theme", where the translucent layer uses
  the inherited Plasma colors. Stored as `themeOpacity`. Above 0 % the popup is
  opened as the widget's own `PlasmaCore.Dialog` with `NoBackground`, because the
  popup window Plasma creates for applets cannot drop its background and would
  otherwise show through as solid; a desktop-placed widget also drops the applet
  card (`Plasmoid.backgroundHints: NoBackground`). Only the fill fades — content
  stays opaque — and a full-opacity outline keeps the popup's shape readable at
  high transparency. The settings preview fades the same way, so the slider shows
  the background changing while the sample content stays solid.
- Custom mode holds **several named themes**: New/Delete buttons plus a name
  field and a per-theme base theme, so you build your own palettes instead of
  editing one anonymous override set. Stored as a JSON array in the
  `customThemes` state key with `themeCustomId` selecting the active one; themes
  configured with the previous flat keys are carried over as a theme named
  "Custom".
- Popup chrome follows the selected theme too: toolbar icons, hover/pressed
  backgrounds, busy indicator and scrollbars are painted from the widget palette
  instead of the desktop color scheme, so they stay visible on light themes.
- Helper command `usage_monitor_kde.py themes` prints the resolved palette plus
  the built-in/installed theme catalog; the `summary` payload now carries the
  theme too, so the panel bar restyles on the next refresh.

### Fixed
- **Security:** the KDE settings pages no longer build the helper command line by
  hand. `batchSetState` escaped only `\` and `"` while wrapping the payload in
  single quotes, so a quote in a theme name, colour or font family broke out of
  the shell string; the payload is now `JSON.stringify`-ed and passed through the
  existing `shellQuote`.
- Theme editor fields (name, colours, font family, sizes) and the transparency
  slider re-sync with the stored value: editing a control replaced its QML
  binding, so switching themes kept showing the previous values — and wrote them
  into the newly selected theme — while Reset no longer moved the slider.
- Deleting the last custom theme no longer resurrects the pre-`customThemes`
  palette: an explicit empty list is now distinguished from a missing key.
- Non-finite numbers (`nan`, `inf`) in `state.json` can no longer reach the
  payload, where they serialized as bare NaN/Infinity and made every helper
  command unparsable for QML.
- Scheme mode no longer rescans and re-parses every installed color scheme on
  each refresh tick: ids resolve straight to their file (`find_scheme`) and each
  file is parsed once instead of two or three times.
- Settings preview mirrors the built-in themes' metrics (corner radius, sizes),
  which the catalog now carries, and no longer snaps back to the previous theme
  right after Apply.
- Provider card titles stay one step above body text instead of jumping to the
  popup-heading size when a custom theme sets a title size.
- The popup's selection colour follows the desktop again: it used to force the
  widget's fixed accent into `Kirigami.Theme.highlightColor` even while following
  the system theme. The other Kirigami roles are deliberately left inheriting —
  overriding them made Plasma paint the scrollbar handle with the visited-link
  colour (purple). The palette also stays a plain object: parking it in the item
  tree as a hidden Item left Kirigami's attached theme uninitialised, reporting
  `#000000` for every role and rendering the widget black in "follow the current
  desktop theme". A QML smoke test (`widgets/kde/tests/test_theme_palette_qml.py`)
  guards both.
- The translucent popup no longer reopens when its panel icon is clicked to close
  it, and drops its reference to the compact item when Plasma recreates it.
- Stale `stateRevision` writes (Plasma's generic Apply loop) are ignored instead
  of triggering a reload from a not-yet-written `state.json`.
- `color_value` rejects the 4-digit `#argb` form, which QColor cannot parse.
- KDE config dialog: Apply now repaints the panel bar and popup immediately.
  Settings that live in the helper's `state.json` emitted no KConfig signal, so
  the applet only picked them up on the next refresh tick (up to the whole
  refresh interval); the pages now bump a `stateRevision` KConfig key after the
  write and the applet reloads from cache at once.
- KDE popup: the usage list no longer paints the desktop style's sunken frame
  over the widget background, which made themed palettes and transparency
  invisible behind an opaque fill.
- KDE config pages: edits update the page live again. `setPending()` mutated the
  pending object in place and reassigned the same reference, which does not
  invalidate QML bindings — selecting "Custom" only revealed its options after
  reopening the dialog.

### Changed
- Workspace crate version and KDE plasmoid metadata bumped to `0.8.0`.

See [releases/v0.8.0.md](releases/v0.8.0.md).

## [0.7.3]

Fixes the Kimi provider's usage window mapping against the real API and lets the
KDE panel bar show every usage window a provider exposes.

### Added
- Kimi now reports the billing-cycle total (`totalQuota`, shown as "uso total"
  in the Kimi UI) as a third `Total` rate window, alongside the 5-hour session
  and 7-day weekly windows.
- The KDE panel bar text now joins all windows of the pinned provider (for
  example `28% • 46% • 31%` for session, weekly, and total) instead of only the
  first two.

### Fixed
- Kimi window mapping: the scope-level detail is the 7-day (weekly) quota and
  `limits[0]` is the 5-hour session, identified by its `window` descriptor
  (`duration` + `timeUnit`); previously the session was mislabeled as a generic
  "Rate limit" and the weekly window carried no duration.

See [releases/v0.7.3.md](releases/v0.7.3.md).

## [0.7.2]

Adds the Kimi provider to the KDE widget UI and overhauls the multi-account
documentation across the project.

### Added
- Kimi provider now appears in the KDE widget settings UI (`PROVIDER_NAMES`,
  `CONNECT_HINTS`) and CLI widget payload (`provider_display_name`).
- Step-by-step token extraction guide in `docs/providers/kimi.md` (how to copy
  the `kimi-auth` cookie from browser DevTools).
- Comprehensive multi-account documentation in `docs/providers/README.md` with
  auth-type table, `CODEX_HOME`/`HOME` isolation strategies for OAuth providers,
  config keys reference, and auto-detected default behaviour.

### Changed
- Workspace crate version and KDE plasmoid metadata bumped to `0.7.2`.

See [releases/v0.7.2.md](releases/v0.7.2.md).

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
