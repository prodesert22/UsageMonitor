# Quality checks

Use the root pre-commit hook as the local quality gate:

```bash
.githooks/pre-commit
```

It runs:

- `cargo fmt -p usage-monitor-cli --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `ruff check widgets usage-monitor-cli/assets/waybar` — Python lint for the
  widget tests and the embedded Waybar helpers (config in
  [`ruff.toml`](../ruff.toml))
- widget Python unit tests (`unittest discover -s widgets`)
- `qmllint` for the KDE and Waybar QML files, if installed

## Dependencies

The gate assumes these tools are on `$PATH`:

| Tool      | Used for         |
|-----------|------------------|
| `cargo`   | Rust build/lint/test |
| `ruff`    | Python lint      |
| `python`  | Widget tests     |
| `qmllint` | Optional QML validation |

## Pre-commit hook

Install the repository hook path once to run the full quality gate before every
commit:

```bash
git config core.hooksPath .githooks
chmod +x .githooks/pre-commit
```

The hook runs:

```bash
cargo fmt -p usage-monitor-cli --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
ruff check widgets usage-monitor-cli/assets/waybar
python -m unittest discover -s widgets -p 'test_*.py'
```

If `qmllint` or `qmllint-qt6` is installed, the hook also validates every QML
file under `usage-monitor-cli/assets` (KDE plasmoid and Waybar popup). `ruff` is
required for the hook; install it with
`pip install -r requirements-dev.txt` or inside `.venv`.

## Notes

The KDE widget is a faithful port of the `codexbar-kde` plasmoid, so its Python
helper (`usage_monitor_kde.py`) is intentionally a single file rather than the
smaller modules used earlier. Prefer splitting responsibilities into a module,
component, or test helper when a file grows for reasons unrelated to that port.

The Waybar popup is in turn a port of that KDE widget: `usage_monitor_waybar_data.py`
mirrors the KDE helper's presentation/theme layer and the QML under
`assets/waybar/ui/` mirrors its interface. Each widget's asset tree has to be
self-contained — the installer materializes one directory per target and the
helpers must run from it alone — so behaviour changes that apply to both widgets
have to be made in both trees, and their tests
(`widgets/kde/tests`, `widgets/waybar/tests`) are the guard for that.
