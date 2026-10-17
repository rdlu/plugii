# Cutting a release

For now GitHub releases are the only channel: a `vX.Y.Z` tag builds the
tarballs and publishes the release. crates.io and the AUR come later; their
steps are at the end, ready but not part of a release yet.

## What the workflows do

- **CI** (`.github/workflows/ci.yml`), on pushes to `main` and on pull
  requests: rustfmt, clippy `-D warnings`, the tests (insta snapshots
  fail instead of updating), a build with the MSRV from Cargo.toml's
  `rust-version`, and the docs site with the pinned hugo.
- **Release** (`release.yml`), on a `v*` tag: fails unless the tag is
  `v` + Cargo.toml's version and CHANGELOG.md has a section for it. Builds
  `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu` natively on
  the ubuntu-22.04 runners, so the binaries need glibc 2.35 or newer. Each
  `plugii-X.Y.Z-<target>.tar.gz` holds the binary, man pages, bash/zsh/fish
  completions, README, CHANGELOG and LICENSE, next to a `.sha256`. Then
  `gh release create` publishes them with the CHANGELOG section as notes.
- **Pages** (`pages.yml`), on pushes to `main` that touch the site (or
  `src/tui/keys.rs` and the snapshots it shows), and by hand: builds `site/`
  with the pinned hugo and deploys it to <https://plugii.dlu.one/>.

## Checklist

1. **Bump the version** in Cargo.toml (`version = "X.Y.Z"`), then
   `cargo check` so Cargo.lock follows.
2. **Draft the CHANGELOG.md section:** `mise run changelog`. It runs
   git-cliff (`cliff.toml`; `pacman -S git-cliff`) over the commits since
   the last tag that touch `src/`, `Cargo.toml` or `Cargo.lock`, groups
   them by first word (Add → Added, Fix → Fixed, Drop/Remove → Removed,
   the rest → Changed), collapses dependency bumps into one line, and
   merges in whatever was written under `[Unreleased]` by hand. It dates
   the section today and updates the link references at the bottom.
   Then edit it: commit subjects are for developers, so reword them for
   users, drop what nobody would notice, and add a `### Security` entry
   naming the advisory for a security fix.
3. **Check:**

   ```sh
   mise run check           # fmt, clippy, tests
   mise run docs-build      # the site still builds
   mise run release-check   # version + CHANGELOG, crate contents, PKGBUILDs
   ```

   Commit (`Release X.Y.Z`) on a `release/X.Y.Z` branch, push it
   (`mise run push`) and open a PR to main. When CI is green, merge it and
   pull main locally.
4. **Tag and push the tag:**

   ```sh
   mise run release-tag
   ```

   On a clean main that matches GitHub main, it creates the annotated tag
   `vX.Y.Z` and pushes it. Nothing pushes main itself: it only moves by
   merged PRs. The push goes over SSH and retries up to 3 times, since the
   security key needs a touch. By hand, that's
   `git tag -a vX.Y.Z -m "plugii X.Y.Z"` and
   `git push origin refs/tags/vX.Y.Z`.
5. **Wait for the Release workflow** (Actions tab, or `gh run watch`).
   When it's green, check the release page: two tarballs, two `.sha256`,
   and the notes. If it fails before publishing, fix it through a PR, delete the
   tag locally and on GitHub (`git push origin :refs/tags/vX.Y.Z`), and
   tag again.
6. **Check the site**: the Pages workflow runs when main's push touched
   `site/`; otherwise run it from the Actions tab (workflow_dispatch).

### First release only

From the first release on, `main` is the release branch: it carries what
has been released (or is about to be), and changes arrive through pull
requests from here on.

Before or right after pushing the tag, in the repository's
**Settings → Pages**:

1. **Source: GitHub Actions.** Then run the Pages workflow once by hand,
   since the push to main came before Pages existed.
2. **Custom domain:** `plugii.dlu.one` (DNS is done and the domain is
   verified). `site/static/CNAME` carries the same name.
3. **Enforce HTTPS**, once GitHub has issued the certificate (the box is
   greyed out until then, sometimes for a few minutes).

## Dependency updates

Dependabot (`.github/dependabot.yml`) opens a grouped PR per week for cargo
and for the pinned GitHub Actions, plus a PR right away for any crate with
a security advisory. CI runs on each; merge when it's green. The MSRV job
fails an update that needs a newer Rust than `rust-version`: close it, or
raise `rust-version` on purpose.

Merging doesn't release anything. The binaries are statically linked, so
an update only reaches users with the next release:

- **Security fix in a crate the binary uses** (check `cargo tree -i
  <crate> -e normal`): cut a patch release soon, with a `### Security`
  entry in CHANGELOG.md naming the advisory.
- **Routine bumps, dev-dependencies, Actions:** no release of their own;
  they ship with the next one. A one-line `### Changed` entry
  ("Updated dependencies") is enough.

`mise run outdated` shows what's pending, including major versions that
Dependabot proposes on their own (they're outside the group).

## Later: crates.io

Not part of a release yet. `cargo install --locked plugii` would then work
for anyone with a Rust toolchain.

- **What publishing does:** `cargo publish` packages the crate (what
  `cargo package --list` shows: the sources, README, CHANGELOG, LICENSE;
  the test fixtures, site and demos stay out through Cargo.toml's
  `include`), builds it once from the package to verify, and uploads it.
- **It's permanent.** A published version can never be deleted or
  overwritten, only *yanked*: `cargo yank --version X.Y.Z` stops new
  projects from picking it, but anyone can still download it. A mistake
  means publishing X.Y.Z+1. Read the package list before the first one.
- **The name:** the first publish claims `plugii` for your crates.io
  account; it was free on 2026-10-16. There's no reserving it without
  publishing something.
- **The API token:** log in to crates.io with GitHub, verify your email
  address, create a token under Account Settings → API Tokens (scope it to
  `publish-new` and `publish-update`, and to the `plugii` crate once it
  exists), then `cargo login` and paste it. It's stored in
  `~/.cargo/credentials.toml`.
- **Dry run first:** `cargo publish --dry-run` does everything except the
  upload (`mise run release-check` runs it). Then, after the tag's release
  workflow is green and from the tagged commit, `cargo publish`.
- Once crates.io is a channel, add `cargo install --locked plugii` to the
  install docs (README and `site/content/docs/getting-started.md`).

## Later: AUR

The PKGBUILDs are in `packaging/aur/` (`plugii` from the tag's source
tarball, `plugii-bin` from the release tarballs), tested with
`mise run aur-test`. `packaging/aur/README.md` has the details. Per
release, once the GitHub release exists:

1. Set `pkgver=X.Y.Z` and `pkgrel=1` in both PKGBUILDs.
2. `updpkgsums` in each directory (pacman-contrib), replacing the `SKIP`
   placeholders with the real checksums.
3. `mise run aur-srcinfo`, then `mise run aur-test`, and commit.
4. Copy each `PKGBUILD` and `.SRCINFO` into its AUR clone
   (`ssh://aur@aur.archlinux.org/plugii.git`,
   `ssh://aur@aur.archlinux.org/plugii-bin.git`), commit and push. This
   needs your AUR account and SSH key.
5. Add `paru -S plugii` / `plugii-bin` to the install docs.
