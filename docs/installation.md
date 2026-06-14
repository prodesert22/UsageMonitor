# Installation

Usage Monitor is a Rust workspace with a core library and a CLI binary. There are
no prebuilt binaries — build from source with Cargo.

## Prerequisites

- **Rust** (edition 2024 — Rust 1.85 or newer). Install via [rustup](https://rustup.rs):
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```
- A C toolchain and `pkg-config` + OpenSSL headers, which `reqwest` may need for
  TLS on some distros:
  - Debian/Ubuntu: `sudo apt install build-essential pkg-config libssl-dev`
  - Fedora: `sudo dnf install gcc pkg-config openssl-devel`
  - Arch: `sudo pacman -S base-devel pkg-config openssl`

## Build from source

```bash
git clone https://github.com/prodesert22/UsageMonitor.git
cd UsageMonitor
cargo build --release
# binary: ./target/release/usage-monitor-cli
```

Run it without installing:

```bash
./target/release/usage-monitor-cli list
```

## Install

```bash
cargo install --path usage-monitor-cli
# installs `usage-monitor-cli` into ~/.cargo/bin
```

Make sure `~/.cargo/bin` is on your `PATH` (rustup adds this for you; otherwise add
`export PATH="$HOME/.cargo/bin:$PATH"` to your shell profile).

Verify:

```bash
usage-monitor-cli --help
usage-monitor-cli list
```

## First run

`list` shows every provider and its resolved state. Nothing is enabled until
credentials are detected or you configure an account:

```bash
# See all providers (everything auto-disabled on a fresh machine)
usage-monitor-cli list

# Configure a provider, then fetch
usage-monitor-cli deepseek set api_key sk-...
usage-monitor-cli fetch deepseek
```

Configuration is stored at `$XDG_CONFIG_HOME/usage-monitor/config.toml`
(or `~/.config/usage-monitor/config.toml`). See [Configuration](configuration.md).

## Troubleshooting

- **`usage-monitor-cli: command not found`** — `~/.cargo/bin` is not on your
  `PATH`. Add `export PATH="$HOME/.cargo/bin:$PATH"` to your shell profile, or run
  the binary directly from `./target/release/usage-monitor-cli`.
- **Build fails on `openssl-sys` / `pkg-config`** — install the TLS prerequisites
  from the [Prerequisites](#prerequisites) section (`libssl-dev` / `openssl-devel`
  / `openssl` plus `pkg-config`). On systems without OpenSSL headers you can force
  the bundled TLS backend by building with the `vendored-openssl` toolchain, or
  install the distro package above.
- **`error: package requires Rust 1.85 or newer`** — update your toolchain with
  `rustup update stable` (the workspace uses edition 2024).
- **`cannot resolve config path (HOME not set)`** — the CLI writes config to
  `$XDG_CONFIG_HOME/usage-monitor/config.toml` (falling back to
  `~/.config/usage-monitor/config.toml`); make sure `HOME` or `XDG_CONFIG_HOME`
  is set in the environment that runs the binary.
- **Nothing is enabled / `fetch` prints "No enabled providers"** — providers stay
  auto-disabled until credentials are detected or you configure an account. See
  [Configuration](configuration.md) and the per-provider docs under
  [`docs/providers/`](providers/README.md).

## Updating

```bash
cd UsageMonitor
git pull
cargo install --path usage-monitor-cli   # or cargo build --release
```

## Uninstall

```bash
cargo uninstall usage-monitor-cli
rm -f ~/.config/usage-monitor/config.toml   # optional: remove saved config
```
