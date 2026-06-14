# Waybar widget

The Waybar integration is a wrapper script at
[`widgets/waybar/usage-monitor-waybar`](../../widgets/waybar/usage-monitor-waybar)
that calls `usage-monitor-cli widget waybar`, validates the JSON, and returns a
stale fallback payload instead of printing a traceback on failure.

## Files

- `usage-monitor-waybar` — executable wrapper used by Waybar
- `usage_monitor_waybar.py` — Python implementation
- `tests/test_waybar_wrapper.py` — unit tests

## Usage

Build/install the CLI first:

```bash
cargo install --path usage-monitor-cli
```

Use the wrapper in your Waybar config:

```jsonc
"custom/usage-monitor": {
  "exec": "/path/to/UsageMonitor/widgets/waybar/usage-monitor-waybar",
  "return-type": "json",
  "interval": 30,
  "format": "{text}",
  "tooltip": true
}
```

Or call the CLI directly:

```jsonc
"custom/usage-monitor": {
  "exec": "usage-monitor-cli widget waybar",
  "return-type": "json",
  "interval": 30,
  "format": "{text}",
  "tooltip": true
}
```

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

- Run `widgets/waybar/usage-monitor-waybar` manually; it should print one JSON
  object.
- If it prints `class: stale`, run `usage-monitor-cli widget waybar` directly to
  inspect the underlying CLI error.
- Check the wrapper is executable: `chmod +x widgets/waybar/usage-monitor-waybar`.

## Tests

```bash
python -m unittest discover -s widgets/waybar -p 'test_*.py'
```
