#!/usr/bin/env bash
# Build EVERY distribution artifact in one go:
#   .deb, .rpm, .pkg.tar.zst, .AppImage, Flatpak source bundle + rendered
#   manifest, generic .tar.gz, and SHA256SUMS — all into one directory.
#
# Usage:
#   dist/build-all.sh [--out-dir dist/artifacts]
#                     [--skip-appimage] [--skip-arch] [--skip-flatpak]
#                     [--appimagetool PATH] [--install-tools]
#
# Requirements on Arch: cargo, makepkg (base-devel), bsdtar, zstd, curl.
# cargo-deb / cargo-generate-rpm are used when installed (or installed with
# --install-tools); appimagetool is downloaded unless --appimagetool is given.
# The Flatpak source bundle needs the release tag (v$VERSION) to exist;
# without it that step prints SKIP. makepkg refuses root: that step skips
# when run as uid 0.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT_DIR="$REPO_ROOT/dist/artifacts"
SKIP_APPIMAGE=0; SKIP_ARCH=0; SKIP_FLATPAK=0
APPIMAGETOOL=""; INSTALL_TOOLS=0

while [ $# -gt 0 ]; do
  case "$1" in
    --out-dir) OUT_DIR="$2"; shift 2 ;;
    --out-dir=*) OUT_DIR="${1#--out-dir=}"; shift ;;
    --skip-appimage) SKIP_APPIMAGE=1; shift ;;
    --skip-arch) SKIP_ARCH=1; shift ;;
    --skip-flatpak) SKIP_FLATPAK=1; shift ;;
    --appimagetool) APPIMAGETOOL="$2"; shift 2 ;;
    --appimagetool=*) APPIMAGETOOL="${1#--appimagetool=}"; shift ;;
    --install-tools) INSTALL_TOOLS=1; shift ;;
    *) echo "Unknown argument: $1" >&2; exit 1 ;;
  esac
done

have() { command -v "$1" >/dev/null 2>&1; }
say() { echo "==> $1"; }
skipmsg() { echo "    SKIP: $1"; }
FAILED=""
failstep() { echo "    FAIL: $1"; FAILED="$FAILED $1"; }

VERSION="$(grep -m1 '^version' "$REPO_ROOT/Cargo.toml" | cut -d'"' -f2)"
ARCH="$(uname -m)"
mkdir -p "$OUT_DIR"
OUT_DIR="$(cd "$OUT_DIR" && pwd)"
if find "$OUT_DIR" -mindepth 1 -maxdepth 1 -print -quit | grep -q .; then
  echo "Output directory must be empty: $OUT_DIR" >&2
  exit 1
fi

if [ "$INSTALL_TOOLS" -eq 1 ]; then
  have cargo-deb || cargo install cargo-deb
  have cargo-generate-rpm || cargo install cargo-generate-rpm
fi

say "cargo build --release + generate-dist"
cargo build --release --locked --target-dir "$REPO_ROOT/target" --manifest-path "$REPO_ROOT/Cargo.toml"
"$REPO_ROOT/target/release/usage-monitor-cli" generate-dist "$REPO_ROOT/usage-monitor-cli/dist-gen"

say ".deb"
if have cargo-deb && have dpkg-shlibdeps; then
  (cd "$REPO_ROOT" && cargo deb -p usage-monitor-cli --locked --output "$OUT_DIR/")
else
  skipmsg "cargo-deb/dpkg-shlibdeps missing (build .deb on Debian/Ubuntu)"
fi

say ".rpm"
if have cargo-generate-rpm; then
  (cd "$REPO_ROOT" && cargo generate-rpm -p usage-monitor-cli --output "$OUT_DIR/")
else
  skipmsg "cargo-generate-rpm missing (rerun with --install-tools)"
fi

say ".tar.gz"
"$REPO_ROOT/dist/make-tarball.sh" --out-dir "$OUT_DIR"
TARBALL="usage-monitor-cli-$VERSION-linux-$ARCH.tar.gz"

say ".AppImage"
AIT_TMP=""
if [ "$SKIP_APPIMAGE" -eq 1 ]; then
  skipmsg "--skip-appimage"
elif [ -z "$APPIMAGETOOL" ] && have appimagetool; then
  APPIMAGETOOL="appimagetool"
elif [ -z "$APPIMAGETOOL" ]; then
  AIT_TMP="$(mktemp -d)"
  AIT_DL="$AIT_TMP/appimagetool.AppImage"
  if curl -fsSL --max-time 180 -o "$AIT_DL" \
      "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$ARCH.AppImage"; then
    chmod +x "$AIT_DL"
    AIT_LOG="$AIT_TMP/extract.log"
    if (cd "$AIT_TMP" && "$AIT_DL" --appimage-extract >"$AIT_LOG" 2>&1) \
       && [ -x "$AIT_TMP/squashfs-root/AppRun" ]; then
      APPIMAGETOOL="$AIT_TMP/squashfs-root/AppRun"
    else
      skipmsg "appimagetool --appimage-extract failed; log:"
      sed 's/^/      /' "$AIT_LOG" | head -20
      APPIMAGETOOL="SKIP"
    fi
  else
    skipmsg "appimagetool download failed (offline?)"; APPIMAGETOOL="SKIP"
  fi
fi
if [ "$SKIP_APPIMAGE" -eq 0 ] && [ -n "$APPIMAGETOOL" ] && [ "$APPIMAGETOOL" != "SKIP" ]; then
  if ! "$REPO_ROOT/dist/appimage/make-appimage.sh" --out-dir "$OUT_DIR" --appimagetool "$APPIMAGETOOL"; then
    failstep "make-appimage.sh (see output above)"
  fi
fi
[ -n "$AIT_TMP" ] && rm -rf "$AIT_TMP"

say ".pkg.tar.zst"
if [ "$SKIP_ARCH" -eq 1 ]; then
  skipmsg "--skip-arch"
elif ! have makepkg; then
  skipmsg "makepkg missing (non-Arch host)"
elif [ "$(id -u)" -eq 0 ]; then
  skipmsg "makepkg refuses root (run as a normal user)"
else
  PKGWORK="$(mktemp -d)"; trap 'rm -rf "$PKGWORK"' EXIT
  cp "$REPO_ROOT/dist/arch/PKGBUILD" "$PKGWORK/"
  cp "$OUT_DIR/$TARBALL" "$PKGWORK/"
  # Bare filename (no file:// scheme): makepkg uses the local copy directly
  # instead of downloading. A relative file:// URL is rejected by curl.
  SHA="$(sha256sum "$PKGWORK/$TARBALL" | cut -d' ' -f1)"
  sed -i "s|^source=.*|source=(\"$TARBALL\")|; s|^sha256sums=.*|sha256sums=(\"$SHA\")|" "$PKGWORK/PKGBUILD"
  PKG_LOG="$PKGWORK/makepkg.log"
  if (cd "$PKGWORK" && makepkg --noconfirm >"$PKG_LOG" 2>&1); then
    cp "$PKGWORK"/*.pkg.tar.zst "$OUT_DIR/"
  else
    failstep "makepkg (needs base-devel; last log lines:)"
    tail -15 "$PKG_LOG" | sed 's/^/      /'
  fi
  rm -rf "$PKGWORK"; trap - EXIT
fi

say "flatpak source + manifest"
if [ "$SKIP_FLATPAK" -eq 1 ]; then
  skipmsg "--skip-flatpak"
elif git -C "$REPO_ROOT" rev-parse "v$VERSION" >/dev/null 2>&1; then
  if [ "$(git -C "$REPO_ROOT" rev-parse HEAD)" != "$(git -C "$REPO_ROOT" rev-parse "v$VERSION")" ]; then
    echo "    NOTE: bundle reflects tag v$VERSION, not the working tree"
  fi
  "$REPO_ROOT/dist/flatpak/make-source-tarball.sh" --out-dir "$OUT_DIR"
  SHA="$(sha256sum "$OUT_DIR/flatpak-source-$VERSION.tar.zst" | cut -d' ' -f1)"
  "$REPO_ROOT/dist/flatpak/render-manifest.sh" --version "$VERSION" \
    --source-sha256 "$SHA" --out "$OUT_DIR/dev.usage_monitor.UsageMonitor.yml"
else
  skipmsg "tag v$VERSION not found (flatpak bundle needs a tagged release commit)"
fi

say "SHA256SUMS"
(cd "$OUT_DIR" && find . -maxdepth 1 -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 -r sha256sum > SHA256SUMS)

echo
echo "Artifacts in $OUT_DIR:"
find "$OUT_DIR" -maxdepth 1 -type f -print
if [ -n "$FAILED" ]; then
  echo "FAILED STEPS:$FAILED"
  exit 1
fi
echo "build-all: finished (review SKIP messages for unavailable formats)"
