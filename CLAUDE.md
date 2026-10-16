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
`capture <name>`, `outdated`, `update`, `watch`, `push`.

## Conventions

- Library + thin binary: everything lives in `src/lib.rs` modules so tests can
  reach it; `src/main.rs` only dispatches.
- sysfs readers take a root path (`/` or a fixture dir), never hard-code
  `/sys`, so tests run against fixture trees under `tests/fixtures/`.
- No async runtime: std threads + channels.
- The model (`src/model.rs`) is plain serde data: no behaviour beyond small
  derived values, all types `Debug, Clone, PartialEq, Serialize, Deserialize`.
- Labels file semantics match the script's `read -r key text` parsing.

Moving machines? Read `docs/HANDOVER.md` first if it exists.
