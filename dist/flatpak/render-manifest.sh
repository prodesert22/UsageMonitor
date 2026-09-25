#!/usr/bin/env bash
# Render dist/flatpak/manifest.template.yml with release URLs + checksums.
#
# Usage: dist/flatpak/render-manifest.sh --version X.Y.Z \
#   --source-sha256 <sha> --out manifest.yml
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VERSION=""
SOURCE_SHA256=""
OUT=""
SOURCE_PATH=""

while [ $# -gt 0 ]; do
  case "$1" in
    --source-path) SOURCE_PATH="$2"; shift 2 ;;
    --version) VERSION="$2"; shift 2 ;;
    --version=*) VERSION="${1#--version=}"; shift ;;
    --source-sha256) SOURCE_SHA256="$2"; shift 2 ;;
    --source-sha256=*) SOURCE_SHA256="${1#--source-sha256=}"; shift ;;
    --out) OUT="$2"; shift 2 ;;
    --out=*) OUT="${1#--out=}"; shift ;;
    *) echo "Unknown argument: $1" >&2; exit 1 ;;
  esac
done

[ -n "$VERSION" ] || { echo "--version is required" >&2; exit 1; }
[ -n "$SOURCE_SHA256" ] || { echo "--source-sha256 is required" >&2; exit 1; }
[ -n "$OUT" ] || { echo "--out is required" >&2; exit 1; }

[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "invalid version" >&2; exit 1; }
[[ "$SOURCE_SHA256" =~ ^[a-fA-F0-9]{64}$ ]] || { echo "invalid SHA256" >&2; exit 1; }

sed -e "s/@@VERSION@@/$VERSION/g" -e "s/@@SOURCE_SHA256@@/$SOURCE_SHA256/g" \
  "$REPO_ROOT/dist/flatpak/manifest.template.yml" > "$OUT"
if grep -q '@@' "$OUT"; then
  echo "error: unrendered placeholders left in $OUT" >&2
  exit 1
fi
python3 - "$OUT" "$SOURCE_PATH" <<'PYTHON'
import os
import pathlib
import sys
import yaml
out = pathlib.Path(sys.argv[1])
manifest = yaml.safe_load(out.read_text())
if sys.argv[2]:
    source = manifest['modules'][0]['sources'][0]
    del source['url']
    manifest_dir = out.resolve().parent
    archive = pathlib.Path(sys.argv[2]).resolve(strict=True)
    source['path'] = os.path.relpath(archive, manifest_dir)
    out.write_text(yaml.safe_dump(manifest, sort_keys=False))
PYTHON
echo "wrote $OUT"
