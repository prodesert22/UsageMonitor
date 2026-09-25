#!/usr/bin/env bash
# Validate the usage-monitor-cli distribution pipeline on any Linux host,
# including Arch (where .deb/.rpm/AppImage/Flatpak are "foreign" formats).
#
#   dist/check.sh              # full: builds every artifact and smoke-tests it
#   dist/check.sh --quick      # static only: syntax + metadata, no builds
#   dist/check.sh --install-tools  # cargo-install cargo-deb/generate-rpm first
#
# Full mode needs: cargo, makepkg (Arch) or ar+tar, bsdtar, python3, zstd.
# Missing optional tools (cargo-deb, appimagetool, flatpak-builder) produce
# SKIP, not failure. Network is only needed to download appimagetool.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MODE="full"
INSTALL_TOOLS=0
for arg in "$@"; do
  case "$arg" in
    --quick) MODE="quick" ;;
    --install-tools) INSTALL_TOOLS=1 ;;
    *) echo "Unknown argument: $arg (want --quick or --install-tools)" >&2; exit 1 ;;
  esac
done

PASS=0; FAIL=0; SKIP=0
pass() { PASS=$((PASS + 1)); echo "  ok: $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL: $1"; }
skip() { SKIP=$((SKIP + 1)); echo "  skip: $1"; }
have() { command -v "$1" >/dev/null 2>&1; }

section() { echo "== $1"; }

if [ "$INSTALL_TOOLS" -eq 1 ]; then
  have cargo-deb || cargo install cargo-deb
  have cargo-generate-rpm || cargo install cargo-generate-rpm
fi

# ---------------------------------------------------------------- static ---
section "static: shell syntax"
for f in dist/install.sh dist/make-tarball.sh dist/build-all.sh dist/appimage/make-appimage.sh \
         dist/flatpak/make-source-tarball.sh dist/flatpak/render-manifest.sh \
         dist/appimage/AppRun dist/check.sh; do
  if bash -n "$REPO_ROOT/$f"; then pass "bash -n $f"; else fail "bash -n $f"; fi
done

section "static: metadata parses"
if have python3; then
  python3 - "$REPO_ROOT" <<'EOF' && pass "YAML/TOML/XML parse" || fail "YAML/TOML/XML parse"
import sys, tomllib, xml.dom.minidom
root = sys.argv[1]
try:
    import yaml
    have_yaml = True
except ImportError:
    have_yaml = False
# Cargo packaging sections must exist.
cargo = tomllib.load(open(f"{root}/usage-monitor-cli/Cargo.toml", "rb"))
assert "deb" in cargo["package"]["metadata"], "missing [package.metadata.deb]"
assert "generate-rpm" in cargo["package"]["metadata"], "missing [package.metadata.generate-rpm]"
# Flatpak cargo config must be valid TOML with a vendor dir.
cfg = tomllib.load(open(f"{root}/dist/flatpak/cargo-config.toml", "rb"))
assert cfg["source"]["vendored-sources"]["directory"] == "vendor"
# AppStream metainfo must be well-formed XML with an id.
xml.dom.minidom.parse(f"{root}/dist/dev.usage_monitor.UsageMonitor.metainfo.xml")
if have_yaml:
    wf = yaml.safe_load(open(f"{root}/.github/workflows/release.yml"))
    assert "dist" in wf["jobs"] and "flatpak" in wf["jobs"], "dist/flatpak jobs missing"
    tpl = open(f"{root}/dist/flatpak/manifest.template.yml").read()
    assert "@@VERSION@@" in tpl and "@@SOURCE_SHA256@@" in tpl, "template placeholders"
    assert "@@" not in tpl.replace("@@VERSION@@", "").replace("@@SOURCE_SHA256@@", ""), "stray @@"
    assert wf["jobs"]["dist"]["needs"] == ["verify", "github-release"]
else:
    print("pyyaml missing: skipped workflow/template YAML checks")
EOF
else
  skip "python3 missing"
fi

section "static: .desktop required keys"
if grep -qE '^Name=' "$REPO_ROOT/dist/usage-monitor-cli.desktop" \
   && grep -qE '^Exec=usage-monitor-cli' "$REPO_ROOT/dist/usage-monitor-cli.desktop" \
   && grep -qE '^Icon=usage-monitor' "$REPO_ROOT/dist/usage-monitor-cli.desktop" \
   && grep -qE '^Type=Application' "$REPO_ROOT/dist/usage-monitor-cli.desktop"; then
  pass ".desktop keys"
else
  fail ".desktop keys"
fi

section "static: PKGBUILD fields"
if grep -qE "^pkgname=usage-monitor-cli-bin" "$REPO_ROOT/dist/arch/PKGBUILD" \
   && grep -qE "^pkgver=$(grep -m1 '^version' "$REPO_ROOT/Cargo.toml" | cut -d'"' -f2)" "$REPO_ROOT/dist/arch/PKGBUILD" \
   && grep -qE "^package\(\)" "$REPO_ROOT/dist/arch/PKGBUILD"; then
  pass "PKGBUILD pkgname/pkgver/package()"
else
  fail "PKGBUILD pkgname/pkgver/package()"
fi

section "packaging regression tests"
if python3 -m unittest discover -s "$REPO_ROOT/dist/tests" -p 'test_*.py'; then
  pass "packaging regression tests"
else
  fail "packaging regression tests (requires Python 3.11+ and PyYAML)"
fi

if [ "$MODE" = "quick" ]; then
  echo "---"
  echo "quick: pass=$PASS fail=$FAIL skip=$SKIP"
  [ "$FAIL" -eq 0 ]
  exit $?
fi

# ----------------------------------------------------------------- build ---
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
VERSION="$(grep -m1 '^version' "$REPO_ROOT/Cargo.toml" | cut -d'"' -f2)"
ARCH="$(uname -m)"

section "build: release binary + generate-dist"
if have cargo; then
  cargo build --release --locked --target-dir "$REPO_ROOT/target" --manifest-path "$REPO_ROOT/Cargo.toml" >/dev/null
  "$REPO_ROOT/target/release/usage-monitor-cli" generate-dist "$REPO_ROOT/usage-monitor-cli/dist-gen" >/dev/null
  [ -s "$REPO_ROOT/usage-monitor-cli/dist-gen/man/man1/usage-monitor-cli.1" ] \
    && [ -s "$REPO_ROOT/usage-monitor-cli/dist-gen/completions/usage-monitor-cli.bash" ] \
    && pass "generate-dist outputs" || fail "generate-dist outputs"
else
  fail "cargo missing"; exit 1
fi

section "format: .deb"
if have cargo-deb && have dpkg-shlibdeps; then
  DEB="$WORK/package.deb"
  (cd "$REPO_ROOT" && cargo deb -p usage-monitor-cli --locked --output "$DEB" >/dev/null)
  mkdir -p "$WORK/deb" && ar x "$DEB" --output "$WORK/deb"
  DEBLIST="$(tar -tf "$WORK/deb/data.tar."* | sort)"
  for want in ./usr/bin/usage-monitor-cli ./usr/share/man/man1/usage-monitor-cli.1.gz \
              ./usr/share/bash-completion/completions/usage-monitor-cli \
              ./usr/share/zsh/vendor-completions/_usage-monitor-cli \
              ./usr/share/fish/vendor_completions.d/usage-monitor-cli.fish \
              ./usr/share/applications/usage-monitor-cli.desktop \
              ./usr/share/icons/hicolor/256x256/apps/usage-monitor.png \
              ./usr/share/metainfo/dev.usage_monitor.UsageMonitor.metainfo.xml \
              ./usr/share/doc/usage-monitor-cli/changelog.Debian.gz; do
    echo "$DEBLIST" | grep -qx "$want" && pass "deb: $want" || fail "deb: $want"
  done
  tar -xOf "$WORK/deb/control.tar."* ./control | grep -q '^Depends: ' \
    && pass "deb: control Depends" || fail "deb: control Depends"
else
  skip "cargo-deb/dpkg-shlibdeps missing (build .deb on Debian/Ubuntu)"
fi

section "format: .rpm"
if have cargo-generate-rpm && have bsdtar; then
  (cd "$REPO_ROOT" && cargo generate-rpm -p usage-monitor-cli --output "$WORK/package.rpm" >/dev/null)
  RPMLIST="$(bsdtar -tf "$WORK/package.rpm" | sort)"
  for want in ./usr/bin/usage-monitor-cli ./usr/share/man/man1/usage-monitor-cli.1 \
              ./usr/share/bash-completion/completions/usage-monitor-cli \
              ./usr/share/zsh/vendor-completions/_usage-monitor-cli \
              ./usr/share/fish/vendor_completions.d/usage-monitor-cli.fish \
              ./usr/share/applications/usage-monitor-cli.desktop \
              ./usr/share/icons/hicolor/256x256/apps/usage-monitor.png \
              ./usr/share/metainfo/dev.usage_monitor.UsageMonitor.metainfo.xml; do
    echo "$RPMLIST" | grep -qx "$want" && pass "rpm: $want" || fail "rpm: $want"
  done
else
  skip "cargo-generate-rpm/bsdtar missing"
fi

section "format: .tar.gz + install.sh roundtrip"
"$REPO_ROOT/dist/make-tarball.sh" --out-dir "$WORK/artifacts" >/dev/null
TARBALL="$(echo "$WORK"/artifacts/*.tar.gz)"
tar -tzf "$TARBALL" > "$WORK/tarball-files"
grep -q "bin/usage-monitor-cli" "$WORK/tarball-files" \
  && pass "tarball: binary present" || fail "tarball: binary present"
tar -xzf "$TARBALL" -C "$WORK"
STAGE="$WORK/usage-monitor-cli-$VERSION-linux-$ARCH"
"$STAGE/install.sh" --prefix "$WORK/prefix" >/dev/null
"$WORK/prefix/bin/usage-monitor-cli" --help >"$WORK/help.txt" 2>&1 \
  && grep -q 'Usage: usage-monitor-cli' "$WORK/help.txt" \
  && pass "installed binary --help" || fail "installed binary --help"
"$WORK/prefix/bin/usage-monitor-cli" list >"$WORK/list.txt" 2>&1 \
  && [ -s "$WORK/list.txt" ] \
  && pass "installed binary list" || fail "installed binary list"
"$STAGE/install.sh" --prefix "$WORK/prefix" --uninstall >/dev/null \
  && [ ! -e "$WORK/prefix/bin/usage-monitor-cli" ] \
  && pass "install.sh --uninstall" || fail "install.sh --uninstall"

section "format: .pkg.tar.zst (PKGBUILD)"
if have makepkg; then
  mkdir -p "$WORK/pkgb" && cp "$REPO_ROOT/dist/arch/PKGBUILD" "$WORK/pkgb/"
  if (cd "$WORK/pkgb" && makepkg --packagelist > "$WORK/packagelist") \
     && grep -q "usage-monitor-cli-bin-$VERSION-.*$ARCH" "$WORK/packagelist"; then
    pass "makepkg --packagelist"
  else
    fail "makepkg --packagelist"
  fi
  rm -rf "$WORK/pkgsrc" "$WORK/pkgdir" && mkdir -p "$WORK/pkgsrc" "$WORK/pkgdir"
  tar -xzf "$TARBALL" -C "$WORK/pkgsrc"
  (srcdir="$WORK/pkgsrc" pkgdir="$WORK/pkgdir" CARCH="$ARCH" \
    bash -c 'source "$0" && package' "$WORK/pkgb/PKGBUILD") \
    && [ -x "$WORK/pkgdir/usr/bin/usage-monitor-cli" ] \
    && pass "PKGBUILD package()" || fail "PKGBUILD package()"
else
  skip "makepkg missing (non-Arch host)"
fi

section "format: .AppImage"
AIT=""
have appimagetool && AIT="appimagetool"
if [ -z "$AIT" ] && have curl; then
  if curl -fsSL --max-time 120 -o "$WORK/appimagetool.AppImage" \
      "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$ARCH.AppImage"; then
    chmod +x "$WORK/appimagetool.AppImage"
    (cd "$WORK" && ./appimagetool.AppImage --appimage-extract >/dev/null 2>&1) \
      && AIT="$WORK/squashfs-root/AppRun"
  fi
fi
if [ -n "$AIT" ]; then
  "$REPO_ROOT/dist/appimage/make-appimage.sh" --out-dir "$WORK/artifacts" --appimagetool "$AIT" >/dev/null
  AIIMG="$(echo "$WORK"/artifacts/*.AppImage)"
  mkdir -p "$WORK/aiex" && cp "$AIIMG" "$WORK/aiex/" && (cd "$WORK/aiex" && ./"${AIIMG##*/}" --appimage-extract >/dev/null 2>&1)
  "$WORK/aiex/squashfs-root/AppRun" --help > "$WORK/appimage-help" 2>&1 \
    && grep -q 'Usage: usage-monitor-cli' "$WORK/appimage-help" \
    && pass "AppImage AppRun --help" || fail "AppImage AppRun --help"
  [ "$(readlink "$WORK/aiex/squashfs-root/.DirIcon")" = usage-monitor.png ] \
    && [ -f "$WORK/aiex/squashfs-root/dev.usage_monitor.UsageMonitor.desktop" ] \
    && [ -f "$WORK/aiex/squashfs-root/usage-monitor.png" ] \
    && [ -f "$WORK/aiex/squashfs-root/usr/share/metainfo/dev.usage_monitor.UsageMonitor.appdata.xml" ] \
    && pass "AppImage desktop+icon" || fail "AppImage desktop+icon"
  # Portability baseline: release AppImages are built on Ubuntu 22.04, so the
  # bundled binary must not require anything newer than GLIBC_2.35. Local
  # builds on newer distros only warn here (the release job enforces it).
  if have objdump; then
    MAXGLIBC="$(objdump -T "$WORK/aiex/squashfs-root/usr/bin/usage-monitor-cli" 2>/dev/null \
      | grep -oE 'GLIBC_[0-9.]+' | sort -Vu | tail -1)"
    echo "  info: AppImage binary max GLIBC requirement: ${MAXGLIBC:-unknown}"
    case "${MAXGLIBC:-}" in
      GLIBC_2.3[0-5]) pass "AppImage glibc baseline ($MAXGLIBC)" ;;
      *) skip "AppImage glibc baseline $MAXGLIBC > 2.35 (release builds on Ubuntu 22.04)" ;;
    esac
  else
    skip "objdump missing for glibc baseline check"
  fi
else
  # No appimagetool and no network: at least validate the staged inputs.
  [ -x "$REPO_ROOT/dist/appimage/AppRun" ] \
    && [ -f "$REPO_ROOT/dist/usage-monitor-cli.desktop" ] \
    && pass "AppImage inputs present (tool skipped)" \
    || fail "AppImage inputs present (tool skipped)"
  skip "appimagetool unavailable (no network?)"
fi

section "format: .flatpak manifest"
"$REPO_ROOT/dist/flatpak/render-manifest.sh" --version "$VERSION" \
  --source-sha256 "0$(printf '0%.0s' {1..63})" --out "$WORK/manifest.yml" >/dev/null \
  && pass "render-manifest.sh" || fail "render-manifest.sh"
if have flatpak-builder; then
  skip "flatpak-builder present but full build needs the release bundle (CI covers it)"
else
  skip "flatpak-builder missing (pacman -S flatpak-builder to build locally)"
fi

echo "---"
echo "full: pass=$PASS fail=$FAIL skip=$SKIP"
[ "$FAIL" -eq 0 ]
