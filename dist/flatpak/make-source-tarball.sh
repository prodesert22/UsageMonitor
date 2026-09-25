#!/usr/bin/env bash
# Build the Flatpak source bundle:
#   flatpak-source-<version>.tar.zst
#
# Single top-level dir `usage-monitor-<version>/` containing the source tree
# plus a `vendor/` dir (from `cargo vendor`) and `flatpak-cargo-config.toml`,
# so the Flatpak manifest needs only ONE archive source and the build CWD is
# deterministic. `vendor/` maps to [source.vendored-sources] in that config.
#
# Usage: dist/flatpak/make-source-tarball.sh [--version X.Y.Z] [--out-dir DIR]
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VERSION="$(grep -m1 '^version' "$REPO_ROOT/Cargo.toml" | cut -d'"' -f2)"
OUT_DIR="$REPO_ROOT/dist/artifacts"
SOURCE_REF=""

while [ $# -gt 0 ]; do
  case "$1" in
    --ref) SOURCE_REF="$2"; shift 2 ;;
    --version) VERSION="$2"; shift 2 ;;
    --version=*) VERSION="${1#--version=}"; shift ;;
    --out-dir) OUT_DIR="$2"; shift 2 ;;
    --out-dir=*) OUT_DIR="${1#--out-dir=}"; shift ;;
    *) echo "Unknown argument: $1" >&2; exit 1 ;;
  esac
done

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
ROOT="$STAGE/usage-monitor-$VERSION"

SOURCE_REF="${SOURCE_REF:-v$VERSION}"
echo "==> git archive $SOURCE_REF"
git -C "$REPO_ROOT" archive --format=tar --prefix="usage-monitor-$VERSION/" "$SOURCE_REF" | tar -x -C "$STAGE"

echo "==> cargo vendor (from the archived v$VERSION tree)"
cargo vendor --locked --manifest-path "$ROOT/Cargo.toml" "$ROOT/vendor" >/dev/null
cp "$ROOT/dist/flatpak/cargo-config.toml" "$ROOT/flatpak-cargo-config.toml"

mkdir -p "$OUT_DIR"
OUT="$OUT_DIR/flatpak-source-$VERSION.tar.zst"
tar --zstd -cf "$OUT" -C "$STAGE" "usage-monitor-$VERSION"
echo "wrote $OUT"
