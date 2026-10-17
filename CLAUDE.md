# plugii

Rust CLI + ratatui TUI: a live Linux USB port monitor. The agreed plan, with
architecture and milestones, is in `docs/PLAN.md`. It rewrites the `usb-ports`
bash script; feature parity with it is the baseline.

## Commands

```sh
cargo build
cargo test                                   # insta snapshots: cargo insta review
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo run -- [args]
```

All three of test, fmt --check and clippy must pass before a commit
(`mise run check`). `mise tasks` lists the rest: `test-live`, `review`,
`capture <name>`, `outdated`, `update`, `watch`, `install`, `push`, and for
the docs site in `site/` (Hugo + Hextra): `docs` (dev server on
http://127.0.0.1:1313/) and `docs-build` (`site/public/`, gitignored), and `demos` (re-records
`site/demos/` with VHS). Releases (`docs/RELEASE.md`): `changelog`
(drafts the next CHANGELOG section with git-cliff), `release-check`
(crate contents, `cargo publish --dry-run`, PKGBUILD checks), `release-tag`
(pushes main and the `vX.Y.Z` tag), and for `packaging/aur/`: `aur-srcinfo`
and `aur-test` (makepkg + namcap from a `git archive` of HEAD).

CI, release and Pages workflows are in `.github/workflows/`; every action is
pinned to a commit SHA with its version in a comment.

## Conventions

- Branches: work on a `feat/`, `fix/`, `docs/` or `chore/` branch off main,
  push it with `mise run push`, and open a PR to main. main only moves by
  merged PRs; there is no `dev` branch.
- Library + thin binary: everything lives in `src/lib.rs` modules so tests can
  reach it; `src/main.rs` only dispatches.
- sysfs readers take a root path (`/` or a fixture dir), never hard-code
  `/sys`, so tests run against fixture trees under `tests/fixtures/`.
- No async runtime: std threads + channels.
- The model (`src/model.rs`) is plain serde data: no behaviour beyond small
  derived values, all types `Debug, Clone, PartialEq, Serialize, Deserialize`.
- Labels file semantics match the script's `read -r key text` parsing.

Moving machines? Read `docs/HANDOVER.md` first if it exists.
