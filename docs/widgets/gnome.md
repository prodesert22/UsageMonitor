# GNOME Shell extension

A top-bar indicator with the same layout as the KDE Plasma widget: the panel
shows the project logo plus the overall usage, and the popup shows one card
per provider/account with Session/Weekly/Monthly bars, cost, and the full
settings (General, Providers, Order, Theme, Updates).

The extension is embedded in the CLI binary (asset tree under
[`usage-monitor-cli/assets/gnome/`](../../usage-monitor-cli/assets/gnome))
and needs GNOME Shell 45–49 (it uses the ESM extension format).

## Install (recommended: via the CLI)

Build/install the CLI first:

```bash
cargo install --path usage-monitor-cli
```

Then install the extension locally — no extensions.gnome.org account needed:

```bash
usage-monitor-cli widget install gnome
```

This copies the extension to
`~/.local/share/gnome-shell/extensions/usage-monitor@usage-monitor.dev/`,
compiles its GSettings schemas, and records the version for the login
`widget sync` upgrade path (see
[Automatic upgrades](README.md#automatic-upgrades)). Then enable it:

```bash
gnome-extensions enable usage-monitor@usage-monitor.dev
```

or toggle it in GNOME Extension Manager. **On Wayland, log out and back in**
after installing or updating — unlike X11 (`Alt+F2 r`), a Wayland session
cannot restart the Shell in place. Remove it with
`usage-monitor-cli widget uninstall gnome`, and inspect resolved paths with
`usage-monitor-cli widget doctor`.

If the CLI is not on `PATH`, set an absolute path:

```bash
export USAGE_MONITOR_BIN="$HOME/.cargo/bin/usage-monitor-cli"
```

## Install from extensions.gnome.org

The store listing ships only the extension (its JavaScript); the store
forbids bundled binaries, so the CLI still has to be installed separately
(see above). If the popup says **"usage-monitor-cli not found"**, install
the CLI and reopen the menu — nothing else breaks: the panel keeps the last
cached values marked stale.

Do not keep the store copy and the CLI-installed copy enabled at the same
time: they share one uuid and the local copy shadows the store one, which
confuses the version shown in Extension Manager.

## What it shows

- Panel: logo + percentage (or a pinned provider), colored by threshold
  (accent below 70 %, warning 70–89 %, critical at 90 %+ and bold), dimmed
  while stale. Hidden text mode via **Show bar text**.
- Popup header: title, summary line (with `· cached/stale` when applicable),
  **Refresh** and **Settings** buttons.
- One card per provider/account: name, headline percentage, account
  email/label/plan, "Using last successful value" or the error, usage bars
  with reset times, and 30-day cost once fetched.
- **Preferences** (top-bar menu → Settings, or Extension Manager):
  - **General** — refresh interval, show bar text, show account email, show
    decimal places, pin to top bar (`provider` or `provider/account`), the
    Session/Weekly/Monthly windows shown in the bar text, clear cache.
  - **Providers** — enable/disable toggles (same as
    `usage-monitor-cli enable|disable <provider>`). Named accounts are added
    in a terminal; the page shows the exact commands per auth type
    (API key, token, cookie, OAuth/CLI logins).
  - **Order** — reorder the provider cards (empty = CLI order).
  - **Theme** — follow the GNOME color-scheme, light/dark palettes, a
    bundled theme (macOS Dark/Light, Nord, Dracula, Tokyo Night), or a
    custom palette, plus popup opacity, bar height and corner radius.
  - **Updates** — installed vs CLI version, one-click
    `widget sync gnome` reinstall (relogin on Wayland afterwards), and the
    same release-notes lookup as the other widgets.
- Settings live in GSettings (`org.gnome.shell.extensions.usage-monitor`);
  the usage cache lives in `~/.cache/usage-monitor-gnome/last.json`,
  separate from the KDE/Waybar state.

## Shell access to the same data

```bash
usage-monitor-cli widget gnome
```

`widget gnome` emits the same contract as `widget kde`/`widget waybar`
(`text`, `tooltip`, `class`, `percentage`, `providers[]`, … — see
[Widgets](README.md#cli-contract)).

## Troubleshooting

- Panel shows `--`: the CLI is missing or no fetch succeeded yet — open the
  menu for the guided notice, or run `usage-monitor-cli widget gnome` in a
  terminal for the raw error.
- Extension installed but not loading: confirm the uuid is enabled
  (`gnome-extensions list --enabled`) and relog on Wayland.
- Old UI after upgrade: `usage-monitor-cli widget install gnome` again, then
  relog; `widget sync` at login handles this automatically once installed.

## Tests

```bash
python -m unittest discover -s widgets/gnome -p 'test_*.py'
```
