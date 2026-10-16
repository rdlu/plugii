---
title: TUI keymap
description: Every key binding in plugii watch and plugii replay, grouped as the in-app help overlay shows them.
weight: 5
---

The keys of `plugii watch` and `plugii replay`. They are vim-first, and the arrow keys always work too.
There are no counts (`5j`): the lists are short, and bare <kbd>1</kbd> <kbd>2</kbd> <kbd>3</kbd> jump to a tab.
Press <kbd>?</kbd> in the TUI to see this list.

> [!NOTE]
> Everything below is rendered by Hugo itself from the `BINDINGS` and
> `PROMPT_KEYS` tables in plugii's `src/tui/keys.rs`: the `keymap` shortcode
> parses the Rust source with regexes on every build, with no generator step.
> The real site regenerates this page from the code, so it can't drift.

{{% keymap %}}
