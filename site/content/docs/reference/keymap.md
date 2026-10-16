---
title: TUI keymap
description: Every key binding in plugii watch and plugii replay, grouped as the in-app help overlay shows them.
weight: 11
---

The keys of `plugii watch` and `plugii replay`. They are vim-first, and the arrow keys always work too.
There are no counts (`5j`): the lists are short, and bare <kbd>1</kbd> <kbd>2</kbd> <kbd>3</kbd> jump to a tab.
Press <kbd>?</kbd> in the TUI to see this list.

> [!NOTE]
> This page is built from the `BINDINGS` and `PROMPT_KEYS` tables in
> plugii's `src/tui/keys.rs`, the same tables the <kbd>?</kbd> help overlay
> shows and the keymap's tests check: Hugo parses the Rust source on every
> build, so the page can't drift from the code.

{{% keymap %}}
