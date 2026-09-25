# Installation

Usage Monitor ships prebuilt packages for Linux alongside the Cargo install.
All formats bundle the same `usage-monitor-cli` binary (desktop widgets are
embedded — add them afterwards with `usage-monitor-cli widget install`).

## Prebuilt binaries (recommended)

Download the latest release from
[GitHub Releases](https://github.com/prodesert22/UsageMonitor/releases)
(check `SHA256SUMS` to verify your download) and pick your format:

### Debian / Ubuntu — `.deb`

```bash
sudo dpkg -i usage-monitor-cli_0.10.2-1_amd64.deb
sudo apt-get install -f   # only if dependency errors appear
```

Declares the OpenSSL and libc libraries detected from the linked binary plus
`python3`; recommends `kpackagetool6`, `waybar`, and `gnome-shell` for the
widget backends.

### Fedora / RHEL / openSUSE — `.rpm`

```bash
sudo rpm -i usage-monitor-cli-0.10.2-1.x86_64.rpm
# or: sudo dnf install ./usage-monitor-cli-0.10.2-1.x86_64.rpm
```

Requires `openssl-libs` + `python3`.

### Arch Linux — `.pkg.tar.zst` / AUR

Install the binary package from the release:

```bash
sudo pacman -U usage-monitor-cli-bin-0.10.2-1-x86_64.pkg.tar.zst
```

Or build from the AUR (`usage-monitor-cli-bin` repacks the release tarball,
no Rust toolchain needed):

```bash
git clone https://aur.archlinux.org/usage-monitor-cli-bin.git
cd usage-monitor-cli-bin
makepkg -si
```

### Generic tarball — `.tar.gz`

Any distro with `bash` + `coreutils`:

```bash
tar -xzf usage-monitor-cli-0.10.2-linux-x86_64.tar.gz
cd usage-monitor-cli-0.10.2-linux-x86_64
./install.sh                    # installs to /usr/local (needs sudo)
./install.sh --prefix ~/.local  # unprivileged install
./install.sh --uninstall        # remove again
```

The tarball carries the binary, man page, shell completions
(bash/zsh/fish), `.desktop` entry, icon, and AppStream metainfo.

### AppImage

No install, no root — download, allow execution, run:

```bash
chmod +x UsageMonitor-0.10.2-linux-x86_64.AppImage
./UsageMonitor-0.10.2-linux-x86_64.AppImage list
```

For desktop widgets, install a native package or the generic tarball. AppImage
mount paths are temporary, while widget autostart stores the executable path.
The AppImage requires compatible host glibc and OpenSSL 3 libraries; it does
not bundle these libraries.

### Flatpak

A per-release manifest (`dev.usage_monitor.UsageMonitor.yml`) is published
with the other artifacts; Flathub submission is still pending. Until then,
build it locally with `flatpak-builder` (see `dist/README.md`):

```bash
flatpak-builder --force-clean build-dir dev.usage_monitor.UsageMonitor.yml
flatpak-builder --run build-dir dev.usage_monitor.UsageMonitor.yml usage-monitor-cli list
```

Sandbox note: payload commands (`widget waybar|kde|gnome`) run fully
sandboxed, but `widget install` needs host access — prefer a host package
(deb/rpm/tarball) when you want the desktop widgets. The CLI does not implement
host re-execution. Sandbox configuration and credentials are separate from the
host configuration; configure accounts explicitly inside the sandbox.
The manifest requires the Freedesktop 24.08 Platform, SDK, and
`org.freedesktop.Sdk.Extension.rust-stable//24.08`. Release artifacts include
the source archive and manifest, not a prebuilt `.flatpak` bundle.

## Cargo install

```bash
cargo install usage-monitor-cli
# installs `usage-monitor-cli` into ~/.cargo/bin
```

Make sure `~/.cargo/bin` is on your `PATH` (rustup adds this for you;
otherwise add `export PATH="$HOME/.cargo/bin:$PATH"` to your shell profile).

## Build from source

Prerequisites:

- **Rust** (edition 2024 — Rust 1.85 or newer). Install via [rustup](https://rustup.rs):
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```
- A C toolchain and `pkg-config` + OpenSSL headers, which `reqwest` may need for
  TLS on some distros:
  - Debian/Ubuntu: `sudo apt install build-essential pkg-config libssl-dev`
  - Fedora: `sudo dnf install gcc pkg-config openssl-devel`
  - Arch: `sudo pacman -S base-devel pkg-config openssl`

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

Install locally:

```bash
cargo install --path usage-monitor-cli
```

Verify:

```bash
usage-monitor-cli --help
usage-monitor-cli list
```

Packagers: `usage-monitor-cli generate-dist <dir>` (hidden subcommand) emits
shell completions + man page from the live clap definition; see
[`dist/README.md`](../dist/README.md) for the deb/rpm/Arch/tarball/AppImage/Flatpak
pipeline.

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

Optionally install a desktop widget afterwards:

```bash
usage-monitor-cli widget install all   # KDE plasmoid + Waybar wrapper + GNOME extension
```

## Troubleshooting

- **`usage-monitor-cli: command not found`** — the install location is not on
  your `PATH`. For `~/.local` installs add `export PATH="$HOME/.local/bin:$PATH"`
  to your shell profile; for Cargo installs use `~/.cargo/bin` instead. Or run
  the binary by absolute path.
- **Build fails on `openssl-sys` / `pkg-config`** — install the TLS prerequisites
  from the [Build from source](#build-from-source) section (`libssl-dev` /
  `openssl-devel` / `openssl` plus `pkg-config`). On systems without OpenSSL headers you can force
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

Packaged installs update through the same channel (apt/dnf/pacman, a newer
tarball/AppImage, or the widget's **Update now** banner which reinstalls from
the current binary). For source installs:

```bash
cd UsageMonitor
git pull
cargo install --path usage-monitor-cli   # or cargo build --release
```

## Uninstall

```bash
# distro package
sudo dpkg -r usage-monitor-cli        # Debian/Ubuntu
sudo rpm -e usage-monitor-cli         # Fedora/RHEL
sudo pacman -Rs usage-monitor-cli-bin # Arch
# tarball
./install.sh --uninstall
# cargo
cargo uninstall usage-monitor-cli
rm -f ~/.config/usage-monitor/config.toml   # optional: remove saved config
```
