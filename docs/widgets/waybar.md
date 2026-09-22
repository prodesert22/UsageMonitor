# Waybar widget

The Waybar integration has two parts:

1. **The bar module** — a small Python wrapper (`usage-monitor-waybar`) that
   calls `usage-monitor-cli widget waybar`, validates the JSON, and returns a
   stale fallback payload instead of printing a traceback on failure.
2. **The popup** (`usage-monitor-waybar-popup`) — the KDE Plasma widget's Qt
   Quick interface, ported to plain Qt Quick and opened from the module's
   `on-click`: provider cards with usage bars, cost, refresh/pin buttons and the
   full settings window (General, Providers, Order, Theme).

Both are embedded in the CLI binary (asset tree under
[`usage-monitor-cli/assets/waybar/`](../../usage-monitor-cli/assets/waybar)) and
written to disk by `widget install`. The bar module works on its own; the popup
is optional and only needs Qt when you actually click.

## Files

- `usage-monitor-cli/assets/waybar/usage-monitor-waybar` — bar module wrapper
- `usage-monitor-cli/assets/waybar/usage_monitor_waybar.py` — wrapper implementation
- `usage-monitor-cli/assets/waybar/usage-monitor-waybar-popup` — popup launcher
- `usage-monitor-cli/assets/waybar/usage_monitor_waybar_popup.py` — Qt bootstrap
  (bindings, single instance, window placement, QML backend)
- `usage-monitor-cli/assets/waybar/usage_monitor_waybar_data.py` — data/theme
  helper (fetch, cache, settings, themes); also usable straight from a shell
- `usage-monitor-cli/assets/waybar/ui/*.qml` — the popup interface
- `usage-monitor-cli/assets/waybar/usage-monitor-waybar.desktop` — desktop entry
  (stable `app_id`/`WM_CLASS` for compositor rules and the XDG portal)
- `widgets/waybar/tests/` — unit tests, loaded directly from the asset tree

## Install

Build/install the CLI first:

```bash
cargo install --path usage-monitor-cli
```

Then install the Waybar files:

```bash
usage-monitor-cli widget install waybar
```

This writes everything under `~/.local/share/usage-monitor/waybar/`, symlinks
`~/.local/bin/usage-monitor-waybar` and `~/.local/bin/usage-monitor-waybar-popup`
to it, installs
`~/.local/share/applications/usage-monitor-waybar.desktop`, and prints a
ready-to-paste module block. Remove it with
`usage-monitor-cli widget uninstall waybar`, and inspect resolved paths with
`usage-monitor-cli widget doctor`. The installer records the version and adds a
login autostart entry running `usage-monitor-cli widget sync`, so upgrading the
CLI refreshes both parts automatically on the next login (see
[Automatic upgrades](README.md#automatic-upgrades)). The popup also shows an
update banner with the release notes and a one-click reinstall when it is
older than the CLI (see [Update notice in the widgets](README.md#update-notice-in-the-widgets)).

### Popup dependency: Qt for Python

The popup needs **PySide6** (preferred) or **PyQt6**. Install the one your
distribution ships:

| Distribution | Command |
| --- | --- |
| Arch / Manjaro / EndeavourOS / CachyOS | `sudo pacman -S pyside6` |
| Fedora / RHEL | `sudo dnf install python3-pyside6` |
| Debian / Ubuntu / Pop!\_OS / Mint | `sudo apt install python3-pyside6.qtquick python3-pyside6.qtqml` |
| openSUSE | `sudo zypper install python3-pyside6` |
| NixOS | add `python3Packages.pyside6` to your packages |
| Void | `sudo xbps-install -S python3-pyside6` |
| Alpine | `sudo apk add py3-pyside6` |
| Gentoo | `sudo emerge dev-python/pyside6` |
| FreeBSD | `sudo pkg install py311-pyside6` |
| anything else | `pip install --user PySide6` |

Check what was detected — binding, session, paths, resolved theme:

```bash
usage-monitor-waybar-popup --doctor
```

Without a binding the popup prints the matching install command and exits; the
bar module keeps working.

## Configure Waybar

Wiring a Waybar module is **two** edits in `~/.config/waybar/config.jsonc`, not
one. Pasting only the module object is the most common reason the widget never
shows up — Waybar ignores a `custom/*` definition that no bar references.

**1. Define the module** (assumes `~/.local/bin` is on `PATH`; otherwise use the
absolute paths printed by the installer):

```jsonc
"custom/usage-monitor": {
  "exec": "usage-monitor-waybar",
  "return-type": "json",
  "interval": 300,
  "format": "{text}",
  "tooltip": true,
  "on-click": "usage-monitor-waybar-popup",
  "on-click-right": "usage-monitor-waybar-popup --settings"
}
```

`interval` is 300 s on purpose. Provider endpoints behind subscription plans
(Claude, Codex) rate-limit undocumented endpoints hard, and a rate-limited fetch
turns *every* provider into an error — which is what makes a bar flip to "⚠".
The module serves its cache between fetches anyway (see
[Caching and pacing](#caching-and-pacing)), so a shorter interval buys nothing.

**2. Add its name to a bar position array** so Waybar actually renders it:

```jsonc
"modules-right": [
  "...",
  "custom/usage-monitor",
  "clock"
]
```

(Use `modules-left` or `modules-center` if you prefer; the string must match the
module key exactly, including the `custom/` prefix.)

**3. Reload Waybar** to pick up the change:

```bash
killall -SIGUSR2 waybar     # live reload
# or: killall waybar; waybar &
```

To run the CLI directly without the wrapper, swap the `exec` in step 1 for
`"usage-monitor-cli widget waybar"`.

If Waybar cannot find the CLI, set an absolute path:

```bash
export USAGE_MONITOR_BIN="$HOME/.cargo/bin/usage-monitor-cli"
```

## The popup

Clicking the module toggles the popup. The first click starts a resident process
that binds a socket under `$XDG_RUNTIME_DIR`; every later click talks to that
process, so opening is instant and you never stack two popups. Pass `--once` if
you would rather have the process exit when the popup closes.

On Wayland it opens as an anchored layer surface under the bar — see
[Placement on Wayland](#placement-on-wayland) if it shows up centred instead.

What it shows — the same layout as the KDE Plasma widget:

- header with the current summary, a busy indicator, and buttons for **Refresh**,
  **Cost**, **Keep open** (pin: the popup no longer auto-hides on focus loss),
  **Settings** and **Close**;
- one card per provider/account: name, headline percentage, account line,
  Session/Weekly/Monthly bars with reset times, stale markers, per-provider
  errors and 30-day cost once fetched;
- a settings window with four pages:
  - **General** — refresh interval, show account email, keep-open default, popup
    position, pinned provider or account (`provider`, or `provider/account`
    when a provider has several logins), the Session/Weekly/Monthly windows
    shown in the header text, clear cache;
  - **Providers** — enable/disable, add/remove named accounts (with the same
    per-provider credential fields and setup hints as the Plasma widget) and
    opencode-go workspaces;
  - **Order** — reorder the provider cards;
  - **Theme** — follow the desktop colors, a bundled theme, an installed KDE
    color scheme, or a custom palette, plus the transparency slider and a live
    preview.

Settings are applied with the **Apply** button and stored in
`~/.config/usage-monitor-waybar/state.json`; the cache lives in
`~/.cache/usage-monitor-waybar/last.json`. They are separate from the KDE
widget's own state, so both widgets can run on the same machine with different
settings.

### Caching and pacing

The bar module and the popup are separate processes with separate timers, and
both need the same numbers. They share one fetched value:

- A fetch is only made when the last one is older than **`minFetchIntervalSeconds`**
  (default 180 s, in Settings → General). Inside that window both read the shared
  cache in `~/.cache/usage-monitor-waybar/`.
- The fetch takes an inter-process lock, so a bar tick and a popup refresh that
  land together produce one round of provider calls, not two.
- Failed attempts reset the timer too: retrying a rate-limited endpoint every
  tick is what keeps it rate-limited.
- When a provider fails, its **last good value stays on the bar**, marked
  `stale`, instead of the whole module dropping to "⚠". The popup shows the same
  card with the error underneath.
- The popup's **Refresh** button always fetches, ignoring the interval.

Set `minFetchIntervalSeconds` to `0` to disable pacing (not recommended with
Claude or Codex enabled).

### Launcher options

```text
usage-monitor-waybar-popup [--anchor <where>] [--width N] [--height N]
                           [--margin N] [--once] [--no-single-instance]
                           [--settings] [--quit] [--doctor]
```

`--anchor` is one of `auto` (default), `cursor`, `center`, `top`, `bottom`,
`top-left`, `top-right`, `bottom-left`, `bottom-right`. It overrides the
**General → Popup position** setting for that click.

### Theming

The popup resolves its palette itself (the Plasma widget got it from Kirigami):

- **Follow the desktop colors** (default) reads KDE's `kdeglobals` when the
  session has one, otherwise the GTK/GNOME dark preference
  (`gsettings org.gnome.desktop.interface color-scheme`, then
  `~/.config/gtk-3.0/settings.ini`) and picks a light or dark palette. Force it
  with `USAGE_MONITOR_DARK=0|1`.
- **Built-in themes**: macOS Dark/Light, Nord, Dracula, Tokyo Night, Gruvbox
  Dark, Catppuccin Mocha.
- **Installed KDE theme / color scheme**: any `.colors` file in the XDG data
  directories (no Plasma session needed, just the files).
- **Custom**: named palettes with per-token colors, font family/sizes, bar
  height, corner radius.

Transparency needs a compositor: Wayland always has one, on X11 it needs
picom/compton — without one the transparent area is painted black.

### Placement on Wayland

A Wayland client cannot move its own window, which is why an ordinary Qt popup
opens wherever the compositor puts it — usually dead centre. Bars solve this
with the **wlr-layer-shell** protocol: a layer surface anchors to a screen edge
and the compositor keeps it clear of the exclusive zone Waybar reserves, so
"anchored top-right" lands exactly under the bar.

The popup uses that protocol through Qt's LayerShellQt integration when it is
available, which is the case on KWin, Hyprland, Sway, niri, river and the other
wlroots-style compositors:

- Install the integration if the popup still opens centred:
  `layer-shell-qt` (Arch, openSUSE), `qt6-wayland` + `layer-shell-qt`
  (Fedora: `qt6-qtwayland` and `layer-shell-qt`), `qt6-layer-shell`/
  `libkf6layershellqt` (Debian/Ubuntu). It must belong to the **same Qt** the
  Python binding uses — a distribution PySide6 with a distribution
  layer-shell-qt is the combination that works; a `pip install PySide6` ships
  its own Qt and will not see it.
- `usage-monitor-waybar-popup --doctor` reports `layerShell: true` when the
  integration was found.
- The **Popup position** setting (or `--anchor`) picks the anchored edge:
  `top-right` is the default, `center` leaves the surface unanchored so the
  compositor centres it. `--margin` is the gap from the edge.
- `USAGE_MONITOR_LAYER_SHELL=0` turns the whole thing off.

Without the integration, the compositor decides. The window's `app_id`
(X11: `WM_CLASS`) is `usage-monitor-waybar` — the installed desktop entry
(`~/.local/share/applications/usage-monitor-waybar.desktop`) is what makes that
id stick — so a rule can place it:

```ini
# Hyprland (~/.config/hypr/hyprland.conf)
windowrulev2 = float, class:^(usage-monitor-waybar)$
windowrulev2 = move 100%-440 40, class:^(usage-monitor-waybar)$
```

```text
# Sway (~/.config/sway/config)
for_window [app_id="usage-monitor-waybar"] floating enable, move position 1470 40
```

```text
# river
riverctl rule-add -app-id usage-monitor-waybar float
```

Under X11 (i3, bspwm, Openbox, XFCE…) no rule is needed: the launcher places the
popup itself, following the pointer by default.

## CSS classes

The bar module emits `class` as `ok`, `warning`, `critical`, or `stale`. The
popup is themed in its own settings; Waybar CSS only styles the bar entry.

```css
#custom-usage-monitor.ok { color: #8ccf7e; }
#custom-usage-monitor.warning { color: #e5c07b; }
#custom-usage-monitor.critical { color: #e06c75; font-weight: bold; }
#custom-usage-monitor.stale { color: #888888; }
```

## Shell access to the same data

The data helper is a normal CLI too — useful for debugging or for scripting a
different bar:

```bash
python ~/.local/share/usage-monitor/waybar/usage_monitor_waybar_data.py summary
python ~/.local/share/usage-monitor/waybar/usage_monitor_waybar_data.py themes
python ~/.local/share/usage-monitor/waybar/usage_monitor_waybar_data.py doctor
```

Subcommands: `summary`, `cache`, `settings`, `state`, `cost`, `themes`,
`cache-clear`, `doctor`, `set-state`, `batch-set-state`.

## Troubleshooting

- Module not showing at all: confirm `"custom/usage-monitor"` is listed in a
  `modules-left/center/right` array, not just defined — a defined-but-unreferenced
  module is silently ignored. Then reload with `killall -SIGUSR2 waybar`.
- Run `usage-monitor-waybar` manually; it should print one JSON object.
- If it prints `class: stale`, the numbers come from the cache because the last
  fetch failed. The `tooltip` field names the failing provider; run
  `usage-monitor-cli widget waybar` for the raw error.
- Bar shows "⚠": that means *no* provider answered **and** there is no cache
  yet — usually every provider failing at once (rate limit, expired token,
  missing credentials). Check the tooltip, then fix the provider
  (`usage-monitor-cli <provider> show`). If it is a rate limit, raise
  `interval` and `minFetchIntervalSeconds`.
- Popup does nothing on click: run `usage-monitor-waybar-popup --doctor`. A
  `"qtBinding": "missing"` means Qt for Python is not installed (see the table
  above).
- Popup opens centred on Wayland instead of under the bar: the layer-shell
  integration is missing for the Qt in use. Check
  `usage-monitor-waybar-popup --doctor` (`layerShell`), install the
  `layer-shell-qt` package for your distribution's Qt, and use the
  distribution's PySide6/PyQt6 rather than a pip install. Failing that, add the
  compositor rule above.
- Popup seems stuck open/closed: `usage-monitor-waybar-popup --quit` ends the
  resident process; the next click starts a fresh one.
- The installer marks the scripts executable; if you run the asset copy in place,
  ensure they are executable with `chmod +x`.

## Tests

```bash
python -m unittest discover -s widgets/waybar -p 'test_*.py'
```

`test_popup_qml.py` instantiates the popup and settings QML in a real Qt 6 `qml`
runtime with a stub backend; it is skipped when no runtime is installed.
