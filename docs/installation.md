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
