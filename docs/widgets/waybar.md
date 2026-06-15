# Waybar widget

The Waybar integration is a small Python wrapper that calls
`usage-monitor-cli widget waybar`, validates the JSON, and returns a stale
fallback payload instead of printing a traceback on failure. The wrapper and its
implementation are embedded in the CLI binary (asset tree under
[`usage-monitor-cli/assets/waybar/`](../../usage-monitor-cli/assets/waybar)) and
written to disk by the `widget install` subcommand.

## Files

- `usage-monitor-cli/assets/waybar/usage-monitor-waybar` — executable wrapper
- `usage-monitor-cli/assets/waybar/usage_monitor_waybar.py` — Python implementation
- `widgets/waybar/tests/test_waybar_wrapper.py` — unit tests (load the helper
  directly from the asset tree)

## Install

Build/install the CLI first:

```bash
cargo install --path usage-monitor-cli
```

Then install the Waybar wrapper:

```bash
usage-monitor-cli widget install waybar
```

This writes the wrapper under `~/.local/share/usage-monitor/waybar/`, symlinks
`~/.local/bin/usage-monitor-waybar` to it, and prints a ready-to-paste module
block. Remove it with `usage-monitor-cli widget uninstall waybar`, and inspect
resolved paths with `usage-monitor-cli widget doctor`. The installer records the
version and adds a login autostart entry running `usage-monitor-cli widget sync`,
so upgrading the CLI refreshes the wrapper automatically on the next login (see
[Automatic upgrades](README.md#automatic-upgrades)).

## Configure Waybar

Wiring a Waybar module is **two** edits in `~/.config/waybar/config.jsonc`, not
one. Pasting only the module object is the most common reason the widget never
shows up — Waybar ignores a `custom/*` definition that no bar references.

**1. Define the module** (assumes `~/.local/bin` is on `PATH`; otherwise use the
absolute path printed by the installer):

```jsonc
"custom/usage-monitor": {
  "exec": "usage-monitor-waybar",
  "return-type": "json",
  "interval": 30,
  "format": "{text}",
  "tooltip": true
}
```

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

## CSS classes

Usage Monitor emits `class` as `ok`, `warning`, `critical`, or `stale`.

```css
#custom-usage-monitor.ok { color: #8ccf7e; }
#custom-usage-monitor.warning { color: #e5c07b; }
#custom-usage-monitor.critical { color: #e06c75; font-weight: bold; }
#custom-usage-monitor.stale { color: #888888; }
```

## Troubleshooting

- Module not showing at all: confirm `"custom/usage-monitor"` is listed in a
  `modules-left/center/right` array, not just defined — a defined-but-unreferenced
  module is silently ignored. Then reload with `killall -SIGUSR2 waybar`.
- Run `usage-monitor-waybar` manually; it should print one JSON object.
- If it prints `class: stale`, run `usage-monitor-cli widget waybar` directly to
  inspect the underlying CLI error.
- The installer marks the wrapper executable; if you run the asset copy in place,
  ensure it is executable with `chmod +x`.

## Tests

```bash
python -m unittest discover -s widgets/waybar -p 'test_*.py'
```
