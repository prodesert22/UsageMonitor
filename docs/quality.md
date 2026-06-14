# Quality checks

Use the root pre-commit hook as the local quality gate:

```bash
.githooks/pre-commit
```

It runs:

- `cargo fmt -p usage-monitor-cli --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `ruff check widgets` — Python lint (config in [`ruff.toml`](../ruff.toml))
- widget Python unit tests (`unittest discover -s widgets`)
- `qmllint` for the KDE QML files, if installed

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
ruff check widgets
python -m unittest discover -s widgets -p 'test_*.py'
```

If `qmllint` or `qmllint-qt6` is installed, the hook also validates the KDE QML
files. `ruff` is required for the hook; install it with
`pip install -r requirements-dev.txt` or inside `.venv`.

## Notes

The KDE widget is a faithful port of the `codexbar-kde` plasmoid, so its Python
helper (`usage_monitor_kde.py`) is intentionally a single file rather than the
smaller modules used earlier. Prefer splitting responsibilities into a module,
component, or test helper when a file grows for reasons unrelated to that port.
