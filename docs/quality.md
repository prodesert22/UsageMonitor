# Quality checks

Use the local quality gate as the Rust equivalent of an ESLint-style workflow:

```bash
python scripts/check_quality.py
```

It runs (skipping any tool that is not installed):

- `cargo fmt -p usage-monitor-cli --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `ruff check widgets scripts` — Python lint (config in [`ruff.toml`](../ruff.toml))
- widget Python unit tests (`unittest discover -s widgets`)
- `qmllint` for the KDE QML files

## Dependencies

The gate assumes these tools are on `$PATH`; missing ones are skipped:

| Tool      | Used for         |
|-----------|------------------|
| `cargo`   | Rust build/lint/test |
| `ruff`    | Python lint      |
| `python`  | Widget tests     |
| `qmllint` | QML validation   |

## Notes

The KDE widget is a faithful port of the `codexbar-kde` plasmoid, so its Python
helper (`usage_monitor_kde.py`) is intentionally a single file rather than the
smaller modules used earlier. Prefer splitting responsibilities into a module,
component, or test helper when a file grows for reasons unrelated to that port.
