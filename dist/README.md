# Distribution packaging

This directory holds everything needed to ship `usage-monitor-cli` outside
`cargo install`: desktop metadata, package definitions, bundle scripts, and
the Flatpak manifest template. The release workflow
(`.github/workflows/release.yml`, jobs `dist` + `flatpak`) runs all of it
and uploads the results to the GitHub release.

## Formats

| Format            | Produced by                                              | Install example                                    |
|-------------------|----------------------------------------------------------|----------------------------------------------------|
| `.deb`            | `cargo deb -p usage-monitor-cli` (+ `[package.metadata.deb]`) | `sudo dpkg -i usage-monitor-cli_*_amd64.deb` |
| `.rpm`            | `cargo generate-rpm -p usage-monitor-cli` (+ `[package.metadata.generate-rpm]`) | `sudo rpm -i usage-monitor-cli-*.rpm` |
| `.pkg.tar.zst`    | `dist/arch/PKGBUILD` (`usage-monitor-cli-bin`, AUR) built from the generic tarball | `makepkg -si` |
| `.AppImage`       | `dist/appimage/make-appimage.sh`                         | `chmod +x UsageMonitor-*.AppImage && ./UsageMonitor-*.AppImage` |
| Flatpak sources + manifest | `dist/flatpak/manifest.template.yml` rendered per release, validated by the `flatpak` CI job | Flathub submission pending; see below |
| `.tar.gz` genérico| `dist/make-tarball.sh` (binary + completions + man + `install.sh`) | `./install.sh --prefix ~/.local` |

All formats ship the same payload: the `usage-monitor-cli` binary (widgets
embedded), shell completions (bash/zsh/fish), the man page, the `.desktop`
entry, the 256px hicolor icon, and the AppStream metainfo file.

## Key files

- `usage-monitor-cli.desktop` — desktop entry (`Terminal=true`; the CLI has
  no GUI of its own, the widgets are installed via `widget install`).
- `dev.usage_monitor.UsageMonitor.metainfo.xml` — AppStream metadata.
- `install.sh` — tarball installer (`--prefix`, `--uninstall`).
- `make-tarball.sh` — assembles the generic tarball from a release build.
- `arch/PKGBUILD` — binary AUR package; bump `pkgver` per release and refresh
  with `updpkgsums` once the release tarball URL is live (`sha256sums` stays
  `SKIP` only as a local placeholder — the AUR copy must pin real sums).
- `appimage/{AppRun,make-appimage.sh}` — AppDir assembly + `appimagetool`
  invocation (works without FUSE; accepts either a raw `.AppImage` tool or its
  extracted `AppRun`).
- `flatpak/` — `manifest.template.yml` + `cargo-config.toml` +
  `make-source-tarball.sh` (tag source + `cargo vendor`) + `render-manifest.sh`.

## The `generate-dist` contract

`usage-monitor-cli generate-dist <dir>` (hidden subcommand, defined in
`usage-monitor-cli/src/cli.rs`, implemented in `src/dist_assets.rs`) emits
completions + man page from the live clap definition, so generated files can
never drift from the real CLI. Every packager calls it first and stages into
`usage-monitor-cli/dist-gen/` (gitignored); the deb/rpm metadata reference
those exact `dist-gen/...` paths.

## Validating locally (including on Arch)

One command builds every artifact:

```bash
./dist/build-all.sh --out-dir dist/artifacts
```

Options: `--skip-appimage`, `--skip-arch`, `--skip-flatpak`,
`--appimagetool PATH`, `--install-tools` (cargo-installs cargo-deb and
cargo-generate-rpm). Use an empty output directory to avoid mixing versions.
Missing optional tools print SKIP; build failures return a non-zero exit.
Debian packages must be built on Debian/Ubuntu with `dpkg-dev` installed so
`cargo-deb` can infer the actual library dependencies. Python 3.11+ and PyYAML
are required for manifest rendering and packaging regression tests.

`dist/check.sh` inspects available formats and smoke-tests the tarball and AppImage:

```bash
./dist/check.sh --quick   # static only: syntax + metadata, no builds (~seconds)
./dist/check.sh           # local build checks; reports formats skipped on this host
```

Full mode on Arch needs `cargo`, `makepkg`, `bsdtar`, `python3`, `zstd`, plus
`cargo-deb` / `cargo-generate-rpm` (`./dist/check.sh --install-tools` installs
both via `cargo install`). Missing optional tools print SKIP, never failure.
Formats foreign to Arch are inspected without their native managers: `.deb`
via `ar`+`tar`, `.rpm` via `bsdtar`, `.AppImage` via `--appimage-extract`.
The local checker tests the Arch `package()` function and renders the Flatpak
manifest; it does not build a complete Arch package or Flatpak sandbox. CI
builds and installs Arch in a container and builds Flatpak offline from the
current commit, including on pull requests. A SKIP is not a validated build.

To generate each artifact by hand instead:

```bash
cargo build --release
./target/release/usage-monitor-cli generate-dist usage-monitor-cli/dist-gen

cargo install cargo-deb cargo-generate-rpm
cargo deb -p usage-monitor-cli                    # target/debian/*.deb
cargo generate-rpm -p usage-monitor-cli           # target/generate-rpm/*.rpm
./dist/make-tarball.sh --out-dir dist/artifacts   # usage-monitor-cli-*.tar.gz
./dist/appimage/make-appimage.sh --out-dir dist/artifacts  # needs appimagetool
```

And to test each one on Arch:

```bash
# .deb without dpkg: unpack and inspect
ar x target/debian/*.deb --output /tmp/debcheck
tar -tf /tmp/debcheck/data.tar.xz | sort

# .rpm without rpm: list payload
bsdtar -tf target/generate-rpm/*.rpm | sort
# real install test: docker run -v $PWD:/pkg fedora \
#   bash -c 'dnf install -y /pkg/target/generate-rpm/*.rpm && usage-monitor-cli list'

# .tar.gz: install to a scratch prefix and run
tar -xzf dist/artifacts/usage-monitor-cli-*.tar.gz -C /tmp
/tmp/usage-monitor-cli-*/install.sh --prefix /tmp/tbprefix
/tmp/tbprefix/bin/usage-monitor-cli list
/tmp/usage-monitor-cli-*/install.sh --prefix /tmp/tbprefix --uninstall

# .pkg.tar.zst: full makepkg from the release tarball
mkdir pkgtest && cp dist/arch/PKGBUILD pkgtest/
cp dist/artifacts/usage-monitor-cli-*.tar.gz pkgtest/
cd pkgtest && makepkg   # needs the source URL live, or override source= to the bare local filename

# .AppImage without FUSE: extract and run the inner AppRun
./UsageMonitor-*.AppImage --appimage-extract
./squashfs-root/AppRun widget doctor

# .flatpak: needs flatpak-builder
./dist/flatpak/make-source-tarball.sh --out-dir /tmp   # only works on a tagged release commit
flatpak-builder --force-clean build-dir dev.usage_monitor.UsageMonitor.yml
```

## Releasing a new version

1. Bump versions on every surface (see AGENTS.md rule 4).
2. Create tag `vX.Y.Z`; manual dispatch also requires an existing tag.
3. The `dist` job builds deb/rpm/tarball/AppImage/Arch package/Flatpak
   source bundle, renders the Flatpak manifest, writes `SHA256SUMS`, and
   uploads everything to a draft GitHub release.
4. The `flatpak` job rebuilds the manifest from the uploaded artifacts offline.
   After it succeeds, the crate is published and the GitHub draft becomes public.
5. Update the AUR `usage-monitor-cli-bin` copy: new `pkgver`, real
   `sha256sums` from the release `SHA256SUMS`.
6. Refresh `docs/installation.md` if install steps changed.

## Future: Flathub / AUR submission

- **AUR**: push `dist/arch/PKGBUILD` (+ `.SRCINFO` via `makepkg --printsrcinfo`)
  to a `usage-monitor-cli-bin` AUR repo.
- **Flathub**: submit the rendered manifest + `flatpak-source-*.tar.zst`
  hosting; Flathub prefers `generated-sources.json` style vendoring, so the
  manifest may need adapting to their linter before acceptance.

## Local Flatpak sources

`make-source-tarball.sh` defaults to `v<version>`. For a development commit,
pass `--ref HEAD`; uncommitted files are not included. Render with
`--source-path /absolute/path/flatpak-source-X.Y.Z.tar.zst` to use the local
archive instead of downloading from GitHub. Install the runtime, SDK and Rust
SDK extension listed in the manifest before running `flatpak-builder`.

AppImage packages the executable and assets but relies on compatible host
glibc and OpenSSL 3 libraries. Use native packages for desktop widget installs:
widget autostart must not refer to a temporary AppImage mount.
