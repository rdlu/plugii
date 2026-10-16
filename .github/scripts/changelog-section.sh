#!/usr/bin/env bash
# Prints the body of CHANGELOG.md's section for VERSION (between its
# `## [VERSION]` heading and the next `## ` heading or the link references
# at the end), for release notes.
# Fails when there is no such section or it is empty.
set -euo pipefail

version=${1:?usage: changelog-section.sh VERSION}
root=$(cd "$(dirname "$0")/../.." && pwd)

notes=$(awk -v v="$version" '
  /^## / { if (found) exit; if (index($0, "## [" v "]") == 1) { found = 1; next } }
  /^\[[^]]*\]: / { if (found) exit }
  found { print }
' "$root/CHANGELOG.md")

if [ -z "$(printf '%s' "$notes" | tr -d '[:space:]')" ]; then
  echo "CHANGELOG.md has no section for $version" >&2
  exit 1
fi
printf '%s\n' "$notes"
