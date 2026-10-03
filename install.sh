#!/bin/sh
# ink installer — https://github.com/borghei/ink
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/borghei/ink/main/install.sh | sh
#
# Environment:
#   INK_VERSION=v0.11.1                  install a specific release (default: latest)
#   INK_INSTALL_DIR="$HOME/.local/bin"   install somewhere else (default: /usr/local/bin)
#
# Options:
#   --print-asset   print the release asset this machine would get, then exit
#
# Detection can be overridden for testing (used with --print-asset):
#   INK_TEST_UNAME_S, INK_TEST_UNAME_M, INK_TEST_LIBC (glibc|musl|termux),
#   INK_TEST_GLIBC_VERSION, INK_TEST_LONG_BIT
#
# The whole script lives in functions and only `main` on the last line runs
# anything, so a truncated download cannot execute half an install.

set -eu

REPO="borghei/ink"
# Oldest glibc the ink-linux-{amd64,arm64} binaries support (built on Ubuntu
# 22.04). Anything older gets the static musl build instead.
MIN_GLIBC="2.35"

say() {
  printf '%s\n' "$*"
}

err() {
  printf 'error: %s\n' "$*" >&2
}

supported_list() {
  cat >&2 <<'EOF'

Prebuilt binaries exist for:
  Linux   x86_64, aarch64 (glibc 2.35+ or static musl), armv7 (static musl)
  macOS   x86_64, arm64
  Windows x64, ARM64 (use install.ps1)

Anywhere else, build from source with Rust:
  cargo install ink-md
EOF
}

uname_s() {
  if [ -n "${INK_TEST_UNAME_S:-}" ]; then printf '%s\n' "$INK_TEST_UNAME_S"; else uname -s; fi
}

uname_m() {
  if [ -n "${INK_TEST_UNAME_M:-}" ]; then printf '%s\n' "$INK_TEST_UNAME_M"; else uname -m; fi
}

# Prints glibc, musl or termux. "glibc" only means "not musl/termux"; the
# glibc version check decides whether the glibc build will actually run.
detect_libc() {
  if [ -n "${INK_TEST_LIBC:-}" ]; then
    printf '%s\n' "$INK_TEST_LIBC"
    return
  fi
  case "${PREFIX:-}" in
    *com.termux*) echo termux; return ;;
  esac
  if [ -n "${TERMUX_VERSION:-}" ]; then
    echo termux
    return
  fi
  if [ -f /etc/alpine-release ]; then
    echo musl
    return
  fi
  for f in /lib/ld-musl-*; do
    if [ -e "$f" ]; then
      echo musl
      return
    fi
  done
  if command -v ldd >/dev/null 2>&1 && ldd --version 2>&1 | grep -qi musl; then
    echo musl
    return
  fi
  echo glibc
}

# Prints the glibc version as MAJOR.MINOR, or nothing if it cannot be read.
glibc_version() {
  if [ -n "${INK_TEST_GLIBC_VERSION+x}" ]; then
    printf '%s\n' "$INK_TEST_GLIBC_VERSION"
    return
  fi
  v=""
  if command -v getconf >/dev/null 2>&1; then
    v=$(getconf GNU_LIBC_VERSION 2>/dev/null | sed -n 's/^glibc \([0-9][0-9]*\.[0-9][0-9]*\).*/\1/p')
  fi
  if [ -z "$v" ] && command -v ldd >/dev/null 2>&1; then
    v=$(ldd --version 2>&1 | head -n 1 | sed -n 's/.*[^0-9.]\([0-9][0-9]*\.[0-9][0-9]*\)[^0-9]*$/\1/p')
  fi
  printf '%s\n' "$v"
}

# version_ge A B: true when MAJOR.MINOR A >= B.
version_ge() {
  a_major=${1%%.*}
  a_minor=${1#*.}
  a_minor=${a_minor%%.*}
  b_major=${2%%.*}
  b_minor=${2#*.}
  b_minor=${b_minor%%.*}
  [ "$a_major" -gt "$b_major" ] || { [ "$a_major" -eq "$b_major" ] && [ "$a_minor" -ge "$b_minor" ]; }
}

long_bit() {
  if [ -n "${INK_TEST_LONG_BIT:-}" ]; then
    printf '%s\n' "$INK_TEST_LONG_BIT"
  else
    getconf LONG_BIT 2>/dev/null || echo 64
  fi
}

# Linux: pick the glibc build when it will run, otherwise the static one.
# $1 = amd64 | arm64
linux_asset() {
  arch=$1
  libc=$(detect_libc)
  case "$libc" in
    termux)
      err "Termux/Android is not supported by the prebuilt binaries yet."
      printf '%s\n' "Build it from source instead:  pkg install rust && cargo install ink-md" >&2
      return 1
      ;;
    musl)
      printf 'ink-linux-%s-musl\n' "$arch"
      return
      ;;
  esac
  # A 64-bit ARM kernel with a 32-bit userland (e.g. Raspberry Pi OS 32-bit)
  # reports aarch64 but has no 64-bit glibc; the static build still runs.
  if [ "$arch" = arm64 ] && [ "$(long_bit)" = 32 ]; then
    printf 'ink-linux-arm64-musl\n'
    return
  fi
  gv=$(glibc_version)
  if [ -n "$gv" ] && version_ge "$gv" "$MIN_GLIBC"; then
    printf 'ink-linux-%s\n' "$arch"
  else
    # Old or unreadable glibc: the static build runs regardless.
    printf 'ink-linux-%s-musl\n' "$arch"
  fi
}

select_asset() {
  os=$(uname_s)
  machine=$(uname_m)
  case "$os" in
    Linux)
      case "$machine" in
        x86_64 | amd64) linux_asset amd64 ;;
        aarch64 | arm64) linux_asset arm64 ;;
        armv7l | armv7 | armhf | armv8l)
          if [ "$(detect_libc)" = termux ]; then
            linux_asset arm64 # prints the Termux message and fails
          else
            echo ink-linux-armv7-musl
          fi
          ;;
        *)
          err "unsupported Linux architecture: $machine"
          supported_list
          return 1
          ;;
      esac
      ;;
    Darwin)
      case "$machine" in
        arm64 | aarch64) echo ink-macos-arm64 ;;
        x86_64)
          # An x86_64 shell under Rosetta on Apple Silicon: get the native build.
          if [ -z "${INK_TEST_UNAME_M:-}" ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = 1 ]; then
            echo ink-macos-arm64
          else
            echo ink-macos-amd64
          fi
          ;;
        *)
          err "unsupported macOS architecture: $machine"
          supported_list
          return 1
          ;;
      esac
      ;;
    MINGW* | MSYS* | CYGWIN* | Windows_NT)
      err "on Windows, use the PowerShell installer:"
      printf '%s\n' "  irm https://raw.githubusercontent.com/$REPO/main/install.ps1 | iex" >&2
      return 1
      ;;
    *)
      err "unsupported operating system: $os"
      supported_list
      return 1
      ;;
  esac
}

# download URL FILE: curl, or wget as a fallback. Returns non-zero on any
# HTTP error (404 included).
download() {
  if command -v curl >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -fsSL --retry 3 "$1" -o "$2"
  elif command -v wget >/dev/null 2>&1; then
    wget -q --https-only -O "$2" "$1"
  else
    err "neither curl nor wget is installed"
    return 1
  fi
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d' ' -f1
  elif command -v openssl >/dev/null 2>&1; then
    openssl dgst -sha256 -r "$1" | cut -d' ' -f1
  else
    return 1
  fi
}

main() {
  if [ "${1:-}" = "--print-asset" ]; then
    select_asset
    return
  fi
  if [ "$#" -gt 0 ]; then
    err "unknown argument: $1 (the only option is --print-asset)"
    return 2
  fi

  install_dir="${INK_INSTALL_DIR:-/usr/local/bin}"
  asset=$(select_asset)

  version="${INK_VERSION:-}"
  if [ -n "$version" ]; then
    case "$version" in
      v*) ;;
      *) version="v$version" ;;
    esac
    base="https://github.com/$REPO/releases/download/$version"
    label="$version"
  else
    # The /latest/download redirect avoids the rate-limited GitHub API.
    base="https://github.com/$REPO/releases/latest/download"
    label="latest"
  fi

  tmpdir=$(mktemp -d 2>/dev/null || mktemp -d -t ink-install)
  trap 'rm -rf "$tmpdir"' EXIT
  trap 'rm -rf "$tmpdir"; exit 130' INT TERM

  say "Installing ink ($label) for $(uname_s)/$(uname_m): $asset"

  # Checksums first: it doubles as the list of what this release ships, so a
  # missing asset gets a real explanation instead of a bare 404.
  if ! download "$base/SHA256SUMS" "$tmpdir/SHA256SUMS"; then
    err "could not download $base/SHA256SUMS"
    if [ -n "${INK_VERSION:-}" ]; then
      err "does release $version exist? See https://github.com/$REPO/releases"
    fi
    return 1
  fi
  expected=$(awk -v f="$asset" '$2 == f || $2 == "*" f { print $1; exit }' "$tmpdir/SHA256SUMS")
  if [ -z "$expected" ]; then
    err "release $label has no $asset."
    case "$asset" in
      *-musl)
        if [ -n "${INK_VERSION:-}" ]; then
          err "Static musl builds first shipped in v0.9.0; $version predates them."
          err "Unset INK_VERSION (or pick a newer one) to get it."
        else
          err "Static musl builds first shipped in v0.9.0; the latest release has none."
        fi
        ;;
    esac
    err "You can always build from source with: cargo install ink-md"
    return 1
  fi

  if ! download "$base/$asset" "$tmpdir/ink"; then
    err "could not download $base/$asset"
    return 1
  fi

  say "Verifying checksum..."
  if ! actual=$(sha256_of "$tmpdir/ink"); then
    err "no sha256sum, shasum or openssl found; cannot verify the download. Aborting."
    return 1
  fi
  if [ "$expected" != "$actual" ]; then
    err "checksum mismatch for $asset"
    err "  expected: $expected"
    err "  actual:   $actual"
    return 1
  fi
  say "Checksum OK."
  chmod 755 "$tmpdir/ink"

  sudo=""
  if [ -d "$install_dir" ]; then
    [ -w "$install_dir" ] || sudo=sudo
  elif ! mkdir -p "$install_dir" 2>/dev/null; then
    sudo=sudo
  fi
  if [ -n "$sudo" ] && ! command -v sudo >/dev/null 2>&1; then
    err "$install_dir is not writable and sudo is not available."
    err "Install somewhere you own instead, e.g.:"
    printf '%s\n' "  curl -fsSL https://raw.githubusercontent.com/$REPO/main/install.sh | INK_INSTALL_DIR=\"\$HOME/.local/bin\" sh" >&2
    return 1
  fi
  if [ -n "$sudo" ]; then
    say "Installing to $install_dir (requires sudo)..."
    sudo mkdir -p "$install_dir"
    sudo mv "$tmpdir/ink" "$install_dir/ink"
  else
    mv "$tmpdir/ink" "$install_dir/ink"
  fi

  if installed=$("$install_dir/ink" --version 2>/dev/null); then
    say "$installed installed to $install_dir/ink"
  else
    say "ink installed to $install_dir/ink, but it did not run here."
    say "Please report this at https://github.com/$REPO/issues with the output of: uname -a"
  fi
  case ":$PATH:" in
    *":$install_dir:"*) ;;
    *) say "Note: $install_dir is not on your PATH. Add it, e.g.: export PATH=\"$install_dir:\$PATH\"" ;;
  esac
  say "Run 'ink --help' to get started."
}

main "$@"
