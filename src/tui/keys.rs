//! The vim-first keymap: crossterm key events in, [`Action`]s out, with the
//! pending state (`g`, `Ctrl-w`, `:`) kept between keys. No counts: the
//! lists are short, and bare `1`/`2`/`3` jump to a tab.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// A direction, for `Ctrl-w h/j/k/l`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Left,
    Down,
    Up,
    Right,
}

/// What a complete key sequence asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// `j` / `k` (negative is up).
    Move(i64),
    /// `gg`.
    Top,
    /// `G`.
    Bottom,
    HalfDown,
    HalfUp,
    /// `h` / `←`: collapse a hub, else the pane to the left.
    Left,
    /// `l` / `→`: expand a hub, else the pane to the right.
    Right,
    /// `Ctrl-w h/j/k/l`.
    Focus(Dir),
    /// `gt`, `Tab`.
    NextTab,
    /// `gT`, `Shift-Tab`.
    PrevTab,
    /// `1` / `2` / `3` (0-based here).
    GoTab(usize),
    Pause,
    ClearEvents,
    Quit,
    /// A key bound in a later milestone (details, filter, label, help).
    NotYet,
    /// Nothing: an unbound key, `Esc`, or a key that only extends a pending
    /// sequence.
    None,
}

/// Keys typed so far that don't make an action yet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pending {
    prefix: Prefix,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Prefix {
    #[default]
    None,
    /// `g`, waiting for `g` / `t` / `T`.
    G,
    /// `Ctrl-w`, waiting for `h` / `j` / `k` / `l`.
    CtrlW,
    /// `:`, waiting for `q`.
    Colon,
    /// `:q`, waiting for `⏎`.
    ColonQ,
}

impl Pending {
    /// The keys typed so far, for the status line: `g`, `^W`, `:q`.
    pub fn text(&self) -> String {
        match self.prefix {
            Prefix::None => "",
            Prefix::G => "g",
            Prefix::CtrlW => "^W",
            Prefix::Colon => ":",
            Prefix::ColonQ => ":q",
        }
        .to_owned()
    }

    pub fn is_empty(&self) -> bool {
        self.prefix == Prefix::None
    }

    /// Feeds one key; returns the action it completes, if any.
    pub fn feed(&mut self, key: KeyEvent) -> Action {
        if key.kind == KeyEventKind::Release {
            return Action::None;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if key.code == KeyCode::Esc || (ctrl && key.code == KeyCode::Char('c')) {
            // Ctrl-c quits: in raw mode it would otherwise do nothing at all.
            let quit = ctrl;
            *self = Self::default();
            return if quit { Action::Quit } else { Action::None };
        }
        let prefix = std::mem::take(&mut self.prefix);
        match prefix {
            Prefix::G => {
                return match key.code {
                    KeyCode::Char('g') => Action::Top,
                    KeyCode::Char('t') => Action::NextTab,
                    KeyCode::Char('T') => Action::PrevTab,
                    _ => Action::None,
                };
            }
            Prefix::CtrlW => {
                // Vim takes both `Ctrl-w j` and `Ctrl-w Ctrl-j`.
                return match key.code {
                    KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => {
                        Action::Focus(Dir::Left)
                    }
                    KeyCode::Char('j') | KeyCode::Down => Action::Focus(Dir::Down),
                    KeyCode::Char('k') | KeyCode::Up => Action::Focus(Dir::Up),
                    KeyCode::Char('l') | KeyCode::Right => Action::Focus(Dir::Right),
                    _ => Action::None,
                };
            }
            Prefix::Colon => {
                if key.code == KeyCode::Char('q') {
                    self.prefix = Prefix::ColonQ;
                }
                return Action::None;
            }
            Prefix::ColonQ => {
                return if key.code == KeyCode::Enter {
                    Action::Quit
                } else {
                    Action::None
                };
            }
            Prefix::None => {}
        }
        if ctrl {
            return match key.code {
                KeyCode::Char('d') => Action::HalfDown,
                KeyCode::Char('u') => Action::HalfUp,
                KeyCode::Char('w') => {
                    self.prefix = Prefix::CtrlW;
                    Action::None
                }
                _ => Action::None,
            };
        }
        match key.code {
            KeyCode::Char(c @ '1'..='3') => Action::GoTab(c as usize - '1' as usize),
            KeyCode::Char('j') | KeyCode::Down => Action::Move(1),
            KeyCode::Char('k') | KeyCode::Up => Action::Move(-1),
            KeyCode::Char('h') | KeyCode::Left => Action::Left,
            KeyCode::Char('l') | KeyCode::Right => Action::Right,
            KeyCode::Char('g') => {
                self.prefix = Prefix::G;
                Action::None
            }
            KeyCode::Char('G') => Action::Bottom,
            KeyCode::Home => Action::Top,
            KeyCode::End => Action::Bottom,
            KeyCode::PageDown => Action::HalfDown,
            KeyCode::PageUp => Action::HalfUp,
            KeyCode::Tab => Action::NextTab,
            KeyCode::BackTab => Action::PrevTab,
            KeyCode::Char(':') => {
                self.prefix = Prefix::Colon;
                Action::None
            }
            KeyCode::Char('q') => Action::Quit,
            KeyCode::Char('p') => Action::Pause,
            KeyCode::Char('c') => Action::ClearEvents,
            KeyCode::Enter | KeyCode::Char('o' | '/' | 'n' | 'N' | 'L' | '?') => Action::NotYet,
            _ => Action::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(keys: &str) -> Vec<Action> {
        let mut p = Pending::default();
        keys.chars()
            .map(|c| p.feed(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)))
            .collect()
    }

    #[test]
    fn digits_jump_to_tabs_not_counts() {
        assert_eq!(feed("1"), [Action::GoTab(0)]);
        assert_eq!(feed("3j"), [Action::GoTab(2), Action::Move(1)]);
        assert_eq!(feed("5j"), [Action::None, Action::Move(1)]);
    }

    #[test]
    fn g_sequences() {
        assert_eq!(feed("gt").last(), Some(&Action::NextTab));
        assert_eq!(feed("gT").last(), Some(&Action::PrevTab));
        assert_eq!(feed("gx").last(), Some(&Action::None));
        // A stray key after `g` drops it: `j` then moves.
        assert_eq!(feed("gxj").last(), Some(&Action::Move(1)));
    }

    #[test]
    fn colon_q_needs_enter() {
        let mut p = Pending::default();
        for c in [':', 'q'] {
            assert_eq!(
                p.feed(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)),
                Action::None
            );
        }
        assert_eq!(p.text(), ":q");
        assert_eq!(
            p.feed(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Action::Quit
        );
        assert!(p.is_empty());
    }

    #[test]
    fn esc_cancels() {
        let mut p = Pending::default();
        p.feed(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        assert_eq!(p.text(), "g");
        assert_eq!(
            p.feed(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            Action::None
        );
        assert!(p.is_empty());
    }

    #[test]
    fn later_milestone_keys_do_nothing_yet() {
        assert!(feed("o/nNL?").iter().all(|a| *a == Action::NotYet));
    }
}
