//! The vim-first keymap: crossterm key events in, [`Action`]s out, with the
//! pending state (`g`, `Ctrl-w`, `:`) kept between keys. No counts: the
//! lists are short, and bare `1`/`2`/`3` jump to a tab. [`BINDINGS`] lists
//! every binding for the help overlay; tests hold it to the keymap.

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
    /// `⏎` / `o`: details of the selected row.
    Open,
    /// `/`: the filter prompt.
    Search,
    /// `n`.
    NextMatch,
    /// `N`.
    PrevMatch,
    /// `?`.
    Help,
    /// `Esc`: close a popup, else clear the filter.
    Back,
    /// `q`: close a popup, else quit.
    Close,
    /// `:q⏎`, `Ctrl-c`: quit from anywhere.
    Quit,
    /// A key bound in a later milestone (label).
    NotYet,
    /// Nothing: an unbound key, or a key that only extends a pending
    /// sequence.
    None,
}

/// One entry of the help overlay: key sequences, each with the action it
/// gives, and what they do. The keymap's tests feed every sequence listed
/// here and check its action, and check every action is listed, so the help
/// can't drift from [`Pending::feed`].
#[derive(Debug, Clone, Copy)]
pub struct Binding {
    pub section: &'static str,
    /// As shown: `j`, `gg`, `Ctrl-w h`, `⏎`, `:q⏎`. A space separates keys;
    /// a token is one named key (`Home`, `Ctrl-d`, `Esc`, …), else each of
    /// its characters is a key.
    pub keys: &'static [(&'static str, Action)],
    pub help: &'static str,
}

const fn bind(
    section: &'static str,
    keys: &'static [(&'static str, Action)],
    help: &'static str,
) -> Binding {
    Binding {
        section,
        keys,
        help,
    }
}

const MOVE: &str = "Move";
const PANES: &str = "Panes and tabs";
const VIEW: &str = "Details and filter";
const OTHER: &str = "Other";

/// Every binding, in help order.
pub const BINDINGS: &[Binding] = &[
    bind(
        MOVE,
        &[("j", Action::Move(1)), ("↓", Action::Move(1))],
        "down",
    ),
    bind(
        MOVE,
        &[("k", Action::Move(-1)), ("↑", Action::Move(-1))],
        "up",
    ),
    bind(MOVE, &[("gg", Action::Top), ("Home", Action::Top)], "top"),
    bind(
        MOVE,
        &[("G", Action::Bottom), ("End", Action::Bottom)],
        "bottom",
    ),
    bind(
        MOVE,
        &[("Ctrl-d", Action::HalfDown), ("PgDn", Action::HalfDown)],
        "half a page down",
    ),
    bind(
        MOVE,
        &[("Ctrl-u", Action::HalfUp), ("PgUp", Action::HalfUp)],
        "half a page up",
    ),
    bind(
        MOVE,
        &[("h", Action::Left), ("←", Action::Left)],
        "collapse a hub, else the pane to the left",
    ),
    bind(
        MOVE,
        &[("l", Action::Right), ("→", Action::Right)],
        "expand a hub, else the pane to the right",
    ),
    bind(
        PANES,
        &[
            ("Ctrl-w h", Action::Focus(Dir::Left)),
            ("Ctrl-w j", Action::Focus(Dir::Down)),
            ("Ctrl-w k", Action::Focus(Dir::Up)),
            ("Ctrl-w l", Action::Focus(Dir::Right)),
        ],
        "focus the pane left / below / above / right",
    ),
    bind(
        PANES,
        &[("gt", Action::NextTab), ("Tab", Action::NextTab)],
        "next tab",
    ),
    bind(
        PANES,
        &[("gT", Action::PrevTab), ("Shift-Tab", Action::PrevTab)],
        "previous tab",
    ),
    bind(
        PANES,
        &[
            ("1", Action::GoTab(0)),
            ("2", Action::GoTab(1)),
            ("3", Action::GoTab(2)),
        ],
        "Ports / Charts / Events tab",
    ),
    bind(
        VIEW,
        &[("⏎", Action::Open), ("o", Action::Open)],
        "details of the selected device or socket",
    ),
    bind(
        VIEW,
        &[("/", Action::Search)],
        "filter devices (on Events: event lines)",
    ),
    bind(VIEW, &[("n", Action::NextMatch)], "next match"),
    bind(VIEW, &[("N", Action::PrevMatch)], "previous match"),
    bind(
        VIEW,
        &[("Esc", Action::Back)],
        "close a popup, else clear the filter",
    ),
    bind(OTHER, &[("p", Action::Pause)], "pause / resume updates"),
    bind(OTHER, &[("c", Action::ClearEvents)], "clear events"),
    bind(OTHER, &[("?", Action::Help)], "this help"),
    bind(OTHER, &[("q", Action::Close)], "close a popup, else quit"),
    bind(
        OTHER,
        &[(":q⏎", Action::Quit), ("Ctrl-c", Action::Quit)],
        "quit",
    ),
];

/// The `/` prompt's keys. It takes every key, so `j` or `q` type text.
pub const PROMPT_KEYS: &[(&str, &str)] = &[
    (
        "text",
        "filters as you type: vid:pid, product, manufacturer",
    ),
    ("⏎", "apply"),
    ("Esc  Ctrl-c", "cancel and clear the filter"),
    ("Backspace", "delete a character"),
];

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
        if ctrl && key.code == KeyCode::Char('c') {
            // Ctrl-c quits: in raw mode it would otherwise do nothing at all.
            *self = Self::default();
            return Action::Quit;
        }
        if key.code == KeyCode::Esc {
            // Esc cancels a pending sequence; on its own it goes back.
            let pending = !self.is_empty();
            *self = Self::default();
            return if pending { Action::None } else { Action::Back };
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
            KeyCode::Char('q') => Action::Close,
            KeyCode::Char('p') => Action::Pause,
            KeyCode::Char('c') => Action::ClearEvents,
            KeyCode::Enter | KeyCode::Char('o') => Action::Open,
            KeyCode::Char('/') => Action::Search,
            KeyCode::Char('n') => Action::NextMatch,
            KeyCode::Char('N') => Action::PrevMatch,
            KeyCode::Char('?') => Action::Help,
            KeyCode::Char('L') => Action::NotYet,
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
    fn esc_cancels_a_sequence_else_goes_back() {
        let mut p = Pending::default();
        p.feed(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        assert_eq!(p.text(), "g");
        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(p.feed(esc), Action::None);
        assert!(p.is_empty());
        assert_eq!(p.feed(esc), Action::Back);
    }

    #[test]
    fn view_keys() {
        assert_eq!(
            feed("o/nN?q"),
            [
                Action::Open,
                Action::Search,
                Action::NextMatch,
                Action::PrevMatch,
                Action::Help,
                Action::Close,
            ]
        );
        let mut p = Pending::default();
        assert_eq!(
            p.feed(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Action::Open
        );
        assert_eq!(
            p.feed(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Action::Quit
        );
        // `l` never labels, `h` never opens help; `L` comes later.
        assert_eq!(feed("lhL"), [Action::Right, Action::Left, Action::NotYet]);
    }

    /// A help token as key events: a named key, else one key per character.
    fn token_keys(token: &str) -> Vec<KeyEvent> {
        let plain = |code| KeyEvent::new(code, KeyModifiers::NONE);
        if let Some(c) = token.strip_prefix("Ctrl-") {
            let c = c.chars().next().unwrap();
            return vec![KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)];
        }
        let named = match token {
            "Home" => Some(KeyCode::Home),
            "End" => Some(KeyCode::End),
            "PgDn" => Some(KeyCode::PageDown),
            "PgUp" => Some(KeyCode::PageUp),
            "Tab" => Some(KeyCode::Tab),
            "Shift-Tab" => Some(KeyCode::BackTab),
            "Esc" => Some(KeyCode::Esc),
            _ => None,
        };
        if let Some(code) = named {
            return vec![plain(code)];
        }
        token
            .chars()
            .map(|c| {
                plain(match c {
                    '⏎' => KeyCode::Enter,
                    '←' => KeyCode::Left,
                    '↓' => KeyCode::Down,
                    '↑' => KeyCode::Up,
                    '→' => KeyCode::Right,
                    c => KeyCode::Char(c),
                })
            })
            .collect()
    }

    #[test]
    fn help_table_matches_the_keymap() {
        for b in BINDINGS {
            for (shown, action) in b.keys {
                let mut p = Pending::default();
                let keys: Vec<KeyEvent> = shown.split(' ').flat_map(token_keys).collect();
                let last = keys.into_iter().map(|k| p.feed(k)).last();
                assert_eq!(last, Some(*action), "{shown}");
                assert!(p.is_empty(), "{shown}");
            }
        }
    }

    #[test]
    fn every_action_is_in_the_help() {
        // A new action fails to compile here until it is sorted below.
        let listed = |a: Action| match a {
            Action::Move(_)
            | Action::Top
            | Action::Bottom
            | Action::HalfDown
            | Action::HalfUp
            | Action::Left
            | Action::Right
            | Action::Focus(_)
            | Action::NextTab
            | Action::PrevTab
            | Action::GoTab(_)
            | Action::Pause
            | Action::ClearEvents
            | Action::Open
            | Action::Search
            | Action::NextMatch
            | Action::PrevMatch
            | Action::Help
            | Action::Back
            | Action::Close
            | Action::Quit => true,
            Action::NotYet | Action::None => false,
        };
        let all = [
            Action::Move(1),
            Action::Move(-1),
            Action::Top,
            Action::Bottom,
            Action::HalfDown,
            Action::HalfUp,
            Action::Left,
            Action::Right,
            Action::Focus(Dir::Left),
            Action::Focus(Dir::Down),
            Action::Focus(Dir::Up),
            Action::Focus(Dir::Right),
            Action::NextTab,
            Action::PrevTab,
            Action::GoTab(0),
            Action::GoTab(1),
            Action::GoTab(2),
            Action::Pause,
            Action::ClearEvents,
            Action::Open,
            Action::Search,
            Action::NextMatch,
            Action::PrevMatch,
            Action::Help,
            Action::Back,
            Action::Close,
            Action::Quit,
        ];
        for a in all.into_iter().filter(|&a| listed(a)) {
            assert!(
                BINDINGS.iter().any(|b| b.keys.iter().any(|(_, x)| *x == a)),
                "{a:?} is not in the help"
            );
        }
    }
}
