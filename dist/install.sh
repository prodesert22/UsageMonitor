#!/usr/bin/env bash
# Install/uninstall the generic usage-monitor-cli tarball.
#
#   ./install.sh [--prefix /usr/local] [--uninstall]
#
# Installs: bin/usage-monitor-cli, man page, shell completions
# (bash/zsh/fish), .desktop entry, hicolor icon and AppStream metainfo.
set -euo pipefail

PREFIX="/usr/local"
UNINSTALL=0

while [ $# -gt 0 ]; do
  case "$1" in
    --prefix) PREFIX="${2:?missing value for --prefix}"; shift 2 ;;
    --prefix=*) PREFIX="${1#--prefix=}"; shift ;;
    --uninstall) UNINSTALL=1; shift ;;
    -h|--help)
      echo "Usage: install.sh [--prefix DIR] [--uninstall]"
      exit 0 ;;
    *) echo "Unknown argument: $1" >&2; exit 1 ;;
  esac
done

SRC_DIR="$(cd "$(dirname "$0")" && pwd)"
FILES=(
  "bin/usage-monitor-cli:bin/usage-monitor-cli:755"
  "share/man/man1/usage-monitor-cli.1:share/man/man1/usage-monitor-cli.1:644"
  "share/bash-completion/completions/usage-monitor-cli:share/bash-completion/completions/usage-monitor-cli:644"
  "share/zsh/vendor-completions/_usage-monitor-cli:share/zsh/vendor-completions/_usage-monitor-cli:644"
  "share/fish/vendor_completions.d/usage-monitor-cli.fish:share/fish/vendor_completions.d/usage-monitor-cli.fish:644"
  "share/applications/usage-monitor-cli.desktop:share/applications/usage-monitor-cli.desktop:644"
  "share/icons/hicolor/256x256/apps/usage-monitor.png:share/icons/hicolor/256x256/apps/usage-monitor.png:644"
  "share/metainfo/dev.usage_monitor.UsageMonitor.metainfo.xml:share/metainfo/dev.usage_monitor.UsageMonitor.metainfo.xml:644"
)

if [ "$UNINSTALL" -eq 1 ]; then
  for entry in "${FILES[@]}"; do
    dest="$(echo "$entry" | cut -d: -f2)"
    rm -f "$PREFIX/$dest"
  done
  echo "usage-monitor-cli removed from $PREFIX"
  exit 0
fi

for entry in "${FILES[@]}"; do
  src="$(echo "$entry" | cut -d: -f1)"
  dest="$(echo "$entry" | cut -d: -f2)"
  mode="$(echo "$entry" | cut -d: -f3)"
  if [ ! -f "$SRC_DIR/$src" ]; then
    echo "error: $src not in tarball" >&2
    exit 1
  fi
  install -Dm"$mode" "$SRC_DIR/$src" "$PREFIX/$dest"
done

echo "usage-monitor-cli installed to $PREFIX/bin/usage-monitor-cli"
