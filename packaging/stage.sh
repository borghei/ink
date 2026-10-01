#!/bin/sh
# Generate the man page and shell completions that ship in the .deb/.rpm
# packages and the release archives.
#
#   packaging/stage.sh <path-to-ink-binary> <out-dir>
#
# Writes <out-dir>/ink.1, <out-dir>/ink.1.gz and
# <out-dir>/completions/{ink.bash,_ink,ink.fish,_ink.ps1,ink.elv}.
# Used by release.yml (with the real x86_64 Linux binary) and by the packaging
# job in ci.yml (with a stub), so nfpm.yaml's paths are exercised on every PR.
set -eu

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <ink-binary> <out-dir>" >&2
  exit 2
fi
bin=$1
out=$2

mkdir -p "$out/completions"
"$bin" man > "$out/ink.1"
# -n: no name/timestamp in the gzip header, so the .gz is reproducible.
gzip -n -9 -c "$out/ink.1" > "$out/ink.1.gz"
"$bin" completions bash > "$out/completions/ink.bash"
"$bin" completions zsh > "$out/completions/_ink"
"$bin" completions fish > "$out/completions/ink.fish"
"$bin" completions powershell > "$out/completions/_ink.ps1"
"$bin" completions elvish > "$out/completions/ink.elv"

# An empty file here means the binary printed nothing; fail loudly rather
# than ship a blank man page.
for f in "$out/ink.1" "$out"/completions/*; do
  if [ ! -s "$f" ]; then
    echo "::error::stage.sh: $f is empty" >&2
    exit 1
  fi
done
ls -l "$out" "$out/completions"
