#!/usr/bin/env bash
# Drafts CHANGELOG.md's section for VERSION with git-cliff (cliff.toml) from
# the commits since the last tag that touch the shipped code, and moves what
# is under [Unreleased] into it, merged by ### heading. Updates the link
# references at the end. The result is a draft to edit before committing.
set -euo pipefail

version=${1:?usage: changelog-draft.sh VERSION}
root=$(cd "$(dirname "$0")/../.." && pwd)
changelog=$root/CHANGELOG.md
repo=https://github.com/rdlu/plugii

if grep -q "^## \[$version\]" "$changelog"; then
  echo "CHANGELOG.md already has a $version section" >&2
  exit 1
fi
previous=$(git -C "$root" describe --tags --abbrev=0 --match 'v[0-9]*')

draft=$(cd "$root" && git cliff --unreleased --tag "v$version" \
  --include-path 'src/**' --include-path Cargo.toml --include-path Cargo.lock \
  2>/dev/null)

# Keep a Changelog's heading order. Entries under a heading appear in the
# order read: the hand-written [Unreleased] ones first, then the draft's.
merged=$(printf '%s\n' "$draft" | awk -v changelog="$changelog" '
  function add(heading, line) {
    if (!(heading in body)) { body[heading] = "" }
    body[heading] = body[heading] line "\n"
  }
  BEGIN {
    while ((getline line < changelog) > 0) {
      if (line ~ /^## /) { inside = (line == "## [Unreleased]"); continue }
      if (line ~ /^\[[^]]*\]: /) inside = 0
      if (!inside) continue
      if (line ~ /^### /) { heading = substr(line, 5); continue }
      if (line != "" && heading != "") add(heading, line)
    }
  }
  /^## / { title = $0; next }
  /^### / { heading = substr($0, 5); next }
  # A hand-written "Updated dependencies, …" line wins over the generic one.
  $0 == "- Updated dependencies." && body["Changed"] ~ /(^|\n)- Updated dependencies/ { next }
  $0 != "" && heading != "" { add(heading, $0) }
  END {
    print title
    n = split("Added Changed Deprecated Removed Fixed Security", order, " ")
    for (i = 1; i <= n; i++) {
      if (order[i] in body) {
        printf "\n### %s\n\n%s", order[i], body[order[i]]
        delete body[order[i]]
      }
    }
    for (h in body) printf "\n### %s\n\n%s", h, body[h]
  }
')

if [ "$(printf '%s\n' "$merged" | wc -l)" -le 1 ]; then
  echo "nothing to release since $previous: no user-facing commits and an empty [Unreleased]" >&2
  exit 1
fi

tmp=$(mktemp)
awk -v merged="$merged" -v version="$version" -v previous="$previous" -v repo="$repo" '
  $0 == "## [Unreleased]" { print; print ""; print merged; print ""; skip = 1; next }
  skip && /^## / { skip = 0 }
  skip && /^\[[^]]*\]: / { skip = 0 }
  skip { next }
  /^\[Unreleased\]: / {
    print "[Unreleased]: " repo "/compare/v" version "...HEAD"
    print "[" version "]: " repo "/releases/tag/v" version
    next
  }
  { print }
' "$changelog" >"$tmp"
mv "$tmp" "$changelog"

echo "drafted the $version section in CHANGELOG.md from the commits since $previous:"
echo "reword it for users before committing"
