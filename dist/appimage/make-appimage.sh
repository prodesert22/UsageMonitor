#!/usr/bin/env bash
# Assemble the AppDir and build the AppImage:
#   UsageMonitor-<version>-linux-<arch>.AppImage
#
# Usage: dist/appimage/make-appimage.sh [--out-dir dist/artifacts]
#        [--appimagetool PATH]
#
# When --appimagetool is omitted, the continuous appimagetool build is downloaded
# to the system temp dir (release workflow does exactly this).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT_DIR="$REPO_ROOT/dist/artifacts"
APPIMAGETOOL=""
ARCH="$(uname -m)"
APPIMAGETOOL_URL="${APPIMAGETOOL_URL:-https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$ARCH.AppImage}"

while [ $# -gt 0 ]; do
  case "$1" in
    --out-dir) OUT_DIR="$2"; shift 2 ;;
    --out-dir=*) OUT_DIR="${1#--out-dir=}"; shift ;;
    --appimagetool) APPIMAGETOOL="$2"; shift 2 ;;
    --appimagetool=*) APPIMAGETOOL="${1#--appimagetool=}"; shift ;;
    *) echo "Unknown argument: $1" >&2; exit 1 ;;
  esac
done

VERSION="$(grep -m1 '^version' "$REPO_ROOT/Cargo.toml" | cut -d'"' -f2)"
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|aarch64) APPIMAGE_ARCH="$ARCH" ;;
  *) echo "Unsupported AppImage architecture: $ARCH" >&2; exit 1 ;;
esac

echo "==> cargo build --release"
cargo build --release --locked --target-dir "$REPO_ROOT/target" --manifest-path "$REPO_ROOT/Cargo.toml"

BIN="$REPO_ROOT/target/release/usage-monitor-cli"
GEN="$REPO_ROOT/usage-monitor-cli/dist-gen"
echo "==> generate-dist"
"$BIN" generate-dist "$GEN"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
APPDIR="$WORK/AppDir"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share"

install -Dm755 "$BIN" "$APPDIR/usr/bin/usage-monitor-cli"
install -Dm644 "$GEN/man/man1/usage-monitor-cli.1" \
  "$APPDIR/usr/share/man/man1/usage-monitor-cli.1"
install -Dm644 "$GEN/completions/usage-monitor-cli.bash" \
  "$APPDIR/usr/share/bash-completion/completions/usage-monitor-cli"
install -Dm644 "$GEN/completions/_usage-monitor-cli" \
  "$APPDIR/usr/share/zsh/vendor-completions/_usage-monitor-cli"
install -Dm644 "$GEN/completions/usage-monitor-cli.fish" \
  "$APPDIR/usr/share/fish/vendor_completions.d/usage-monitor-cli.fish"
install -Dm644 "$REPO_ROOT/dist/usage-monitor-cli.desktop" \
  "$APPDIR/dev.usage_monitor.UsageMonitor.desktop"
install -Dm644 "$REPO_ROOT/dist/usage-monitor-cli.desktop" \
  "$APPDIR/usr/share/applications/usage-monitor-cli.desktop"
install -Dm644 "$REPO_ROOT/usage-monitor-cli/assets/kde/package/contents/images/usage-monitor.png" \
  "$APPDIR/usage-monitor.png"
install -Dm644 "$REPO_ROOT/usage-monitor-cli/assets/kde/package/contents/images/usage-monitor.png" \
  "$APPDIR/usr/share/icons/hicolor/256x256/apps/usage-monitor.png"
install -Dm644 "$REPO_ROOT/dist/dev.usage_monitor.UsageMonitor.metainfo.xml" \
  "$APPDIR/usr/share/metainfo/dev.usage_monitor.UsageMonitor.appdata.xml"
install -Dm755 "$REPO_ROOT/dist/appimage/AppRun" "$APPDIR/AppRun"
ln -s usage-monitor.png "$APPDIR/.DirIcon"

if [ -z "$APPIMAGETOOL" ]; then
  APPIMAGETOOL="$WORK/appimagetool.AppImage"
  echo "==> downloading appimagetool"
  curl -fsSL -o "$APPIMAGETOOL" "$APPIMAGETOOL_URL"
  chmod +x "$APPIMAGETOOL"
  (cd "$WORK" && "$APPIMAGETOOL" --appimage-extract >extract.log 2>&1)
  APPIMAGETOOL="$WORK/squashfs-root/AppRun"
elif [[ "$APPIMAGETOOL" == *.AppImage ]]; then
  APPIMAGETOOL="$(readlink -f "$APPIMAGETOOL")"
  (cd "$WORK" && "$APPIMAGETOOL" --appimage-extract >extract.log 2>&1)
  APPIMAGETOOL="$WORK/squashfs-root/AppRun"
fi

mkdir -p "$OUT_DIR"
OUT="$OUT_DIR/UsageMonitor-$VERSION-linux-$ARCH.AppImage"
echo "==> appimagetool"
ARCH="$APPIMAGE_ARCH" "$APPIMAGETOOL" "$APPDIR" "$OUT"
echo "wrote $OUT"
