#!/usr/bin/env bash
# Build the generic Linux tarball:
#   usage-monitor-cli-<version>-linux-<arch>.tar.gz
#
# Layout inside:
#   usage-monitor-cli-<version>-linux-<arch>/
#     bin/usage-monitor-cli
#     share/man, share/bash-completion, share/zsh, share/fish,
#     share/applications, share/icons, share/metainfo
#     install.sh  LICENSE  README.md  CHANGELOG.md
#
# Usage: dist/make-tarball.sh [--out-dir dist/artifacts]
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT_DIR="$REPO_ROOT/dist/artifacts"
while [ $# -gt 0 ]; do
  case "$1" in
    --out-dir) OUT_DIR="$2"; shift 2 ;;
    --out-dir=*) OUT_DIR="${1#--out-dir=}"; shift ;;
    *) echo "Unknown argument: $1" >&2; exit 1 ;;
  esac
done

VERSION="$(grep -m1 '^version' "$REPO_ROOT/Cargo.toml" | cut -d'"' -f2)"
ARCH="$(uname -m)"
STAGE_NAME="usage-monitor-cli-$VERSION-linux-$ARCH"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

echo "==> cargo build --release"
cargo build --release --locked --target-dir "$REPO_ROOT/target" --manifest-path "$REPO_ROOT/Cargo.toml"

BIN="$REPO_ROOT/target/release/usage-monitor-cli"
# Package-relative staging dir for generated completions/man. The deb and rpm
# metadata below reference these same `dist-gen/...` paths, so every format
# ships byte-identical generated files from one `generate-dist` run.
GEN="$REPO_ROOT/usage-monitor-cli/dist-gen"
echo "==> generate-dist"
"$BIN" generate-dist "$GEN"

ROOT="$STAGE/$STAGE_NAME"
mkdir -p "$ROOT/bin" "$ROOT/share"
install -Dm755 "$BIN" "$ROOT/bin/usage-monitor-cli"
install -Dm644 "$GEN/man/man1/usage-monitor-cli.1" \
  "$ROOT/share/man/man1/usage-monitor-cli.1"
install -Dm644 "$GEN/completions/usage-monitor-cli.bash" \
  "$ROOT/share/bash-completion/completions/usage-monitor-cli"
install -Dm644 "$GEN/completions/_usage-monitor-cli" \
  "$ROOT/share/zsh/vendor-completions/_usage-monitor-cli"
install -Dm644 "$GEN/completions/usage-monitor-cli.fish" \
  "$ROOT/share/fish/vendor_completions.d/usage-monitor-cli.fish"
install -Dm644 "$REPO_ROOT/dist/usage-monitor-cli.desktop" \
  "$ROOT/share/applications/usage-monitor-cli.desktop"
install -Dm644 "$REPO_ROOT/usage-monitor-cli/assets/kde/package/contents/images/usage-monitor.png" \
  "$ROOT/share/icons/hicolor/256x256/apps/usage-monitor.png"
install -Dm644 "$REPO_ROOT/dist/dev.usage_monitor.UsageMonitor.metainfo.xml" \
  "$ROOT/share/metainfo/dev.usage_monitor.UsageMonitor.metainfo.xml"
install -Dm755 "$REPO_ROOT/dist/install.sh" "$ROOT/install.sh"
install -Dm644 "$REPO_ROOT/LICENSE" "$ROOT/LICENSE"
install -Dm644 "$REPO_ROOT/README.md" "$ROOT/README.md"
install -Dm644 "$REPO_ROOT/CHANGELOG.md" "$ROOT/CHANGELOG.md"

mkdir -p "$OUT_DIR"
TARBALL="$OUT_DIR/$STAGE_NAME.tar.gz"
tar -czf "$TARBALL" -C "$STAGE" "$STAGE_NAME"
echo "wrote $TARBALL"
