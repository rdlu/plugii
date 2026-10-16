#!/usr/bin/env bash
# Installs the hugo that mise.toml pins, from the official release tarball,
# checked against the release's checksums file. Usage: install-hugo.sh DIR.
# In GitHub Actions, DIR is added to $GITHUB_PATH.
set -euo pipefail

dir=${1:?usage: install-hugo.sh DIR}
root=$(cd "$(dirname "$0")/../.." && pwd)
version=$(sed -n 's/^hugo = "\([0-9.]*\)"$/\1/p' "$root/mise.toml")
[ -n "$version" ] || { echo "no hugo version in mise.toml" >&2; exit 1; }

case "$(uname -m)" in
  x86_64) arch=amd64 ;;
  aarch64) arch=arm64 ;;
  *) echo "unsupported architecture $(uname -m)" >&2; exit 1 ;;
esac

tarball="hugo_${version}_linux-${arch}.tar.gz"
sums="hugo_${version}_checksums.txt"
base="https://github.com/gohugoio/hugo/releases/download/v${version}"
mkdir -p "$dir"
cd "$dir"
curl -fsSLO --retry 3 "$base/$tarball"
curl -fsSLO --retry 3 "$base/$sums"
grep " ${tarball}\$" "$sums" | sha256sum -c -
tar -xzf "$tarball" hugo
rm "$tarball" "$sums"
./hugo version

if [ -n "${GITHUB_PATH:-}" ]; then
  echo "$PWD" >> "$GITHUB_PATH"
fi
