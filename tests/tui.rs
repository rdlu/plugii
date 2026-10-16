//! The TUI without a terminal: keys against the app state, and screens
//! rendered through ratatui's `TestBackend` from the fixture machines and
//! hand-made histories and events.

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use plugii::collect::{self, CollectOpts};
use plugii::diff::{self, Event, EventKind};
use plugii::history::{History, Sample};
use plugii::labels;
use plugii::model::{Snapshot, Usb3Side};
use plugii::sysfs::Root;
use plugii::tui::App;
use plugii::tui::app::{self, Overlay, Pane, Tab};
use plugii::tui::detail::Target;
use plugii::tui::ports::RowKind;
use plugii::tui::ui;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

/// 2025-10-16 12:00:00 UTC; the clock shows UTC (offset 0) in tests.
const T0: u64 = 1_760_616_000_000;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn snapshot(name: &str, labels_name: &str) -> Snapshot {
    let labels = labels::read(&fixtures().join("labels").join(labels_name)).unwrap();
    let dir = fixtures().join("kernel_log").join(labels_name);
    let log = (
        std::fs::read_to_string(dir.join("warnings.log")).unwrap(),
        std::fs::read_to_string(dir.join("all.log")).unwrap(),
    );
    let root = Root::new(fixtures().join("sysfs").join(name));
    let mut snap = collect::snapshot_with(&root, &labels, &CollectOpts::offline(), Some(log), None);
    snap.taken_at_ms = T0;
    snap
}

fn app_with(snap: Snapshot) -> App {
    let mut app = App::new(0);
    app.now_ms = T0;
    let sample = Sample {
        t_ms: snap.taken_at_ms,
        ..Sample::default()
    };
    app.on_update(snap, Vec::new(), sample);
    app
}

fn daisy_dock() -> App {
    app_with(snapshot("daisy-dock", "daisy"))
}

/// The screen as text, one line per row, trailing blanks trimmed.
fn render(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| ui::draw(f, app)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut out = String::new();
    for y in 0..height {
        let mut line = String::new();
        let mut skip = 0;
        for x in 0..width {
            let cell = &buffer[(x, y)];
            if skip == 0 {
                line.push_str(cell.symbol());
            }
            skip = std::cmp::max(skip, unicode_width(cell.symbol())).saturating_sub(1);
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

fn unicode_width(s: &str) -> usize {
    use ratatui::text::Span;
    Span::raw(s).width()
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn keys(app: &mut App, text: &str) {
    for c in text.chars() {
        app.on_key(key(KeyCode::Char(c)));
    }
}

fn sel(app: &App) -> usize {
    app.pane(app.focus()).sel
}

#[test]
fn ports_daisy_dock_120x40() {
    let mut app = daisy_dock();
    insta::assert_snapshot!(render(&mut app, 120, 40));
}

#[test]
fn ports_xps_120x40() {
    let mut app = app_with(snapshot("xps", "xps"));
    insta::assert_snapshot!(render(&mut app, 120, 40));
}

#[test]
fn ports_daisy_dock_80x40_stacks_panels() {
    let mut app = daisy_dock();
    let screen = render(&mut app, 80, 40);
    assert!(app.narrow());
    insta::assert_snapshot!(screen);
}

/// A minute of samples: a charger arriving 20 s in, the CPU loaded after it.
fn history_app() -> App {
    let mut app = daisy_dock();
    app.history = History::new(app::HISTORY_LEN);
    for i in 0..60u64 {
        let t = T0 - (59 - i) * 1000;
        let charging = i >= 20;
        let load = if i >= 30 { 1.0 } else { 0.2 };
        let wave = (i as f32 / 4.0).sin();
        app.history.push(Sample {
            t_ms: t,
            pd_w: charging.then_some(45.0),
            battery_w: Some(if charging { 12.0 + wave } else { -8.0 + wave }),
            cpu_package_w: Some(6.0 + 20.0 * load + 2.0 * wave),
            cpu_ghz: Some(1.4 + 2.4 * load + 0.2 * wave),
            cpu_temp_c: Some(45.0 + 40.0 * load + 3.0 * wave),
        });
    }
    app
}

fn event(t_ms: u64, kind: EventKind, socket: Option<&str>, text: &str) -> Event {
    Event {
        t_ms,
        kind,
        socket: socket.map(str::to_owned),
        text: text.to_owned(),
    }
}

fn some_events() -> Vec<Event> {
    vec![
        event(
            T0 - 40_000,
            EventKind::Added,
            Some("USB-C (caps lock)"),
            "charger on USB-C (caps lock)",
        ),
        event(
            T0 - 39_000,
            EventKind::Changed,
            None,
            "AC online, battery Not charging",
        ),
        event(
            T0 - 25_000,
            EventKind::Added,
            Some("USB-C (hinge)"),
            "05e3:0626 hub @5G on USB-C (hinge)",
        ),
        event(
            T0 - 12_000,
            EventKind::Problem,
            Some("USB-C (hinge)"),
            "link reset on USB-C (hinge)",
        ),
        event(
            T0 - 5_000,
            EventKind::Removed,
            Some("USB-A (left)"),
            "046d:c52b Unifying Receiver @12M on USB-A (left)",
        ),
    ]
}

#[test]
fn charts_tab() {
    let mut app = history_app();
    app.events.extend(some_events());
    keys(&mut app, "2");
    assert_eq!(app.tab, Tab::Charts);
    insta::assert_snapshot!(render(&mut app, 120, 40));
}

#[test]
fn events_tab() {
    let mut app = daisy_dock();
    let snap = app.snapshot.clone().unwrap();
    app.on_update(snap, some_events(), Sample::default());
    keys(&mut app, "3");
    insta::assert_snapshot!(render(&mut app, 100, 12));
}

#[test]
fn flash_marks_a_socket_that_just_changed() {
    let mut app = daisy_dock();
    let snap = app.snapshot.clone().unwrap();
    app.on_update(
        snap,
        vec![event(
            T0 - 500,
            EventKind::Added,
            Some("USB-C (hinge)"),
            "hub",
        )],
        Sample::default(),
    );
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let flash_bg = |app: &mut App, terminal: &mut Terminal<TestBackend>| {
        terminal.draw(|f| ui::draw(f, app)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..40)
            .find(|&y| {
                (0..60)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .contains("USB-C (hinge)")
            })
            .map(|y| buffer[(4, y)].bg)
    };
    assert_eq!(
        flash_bg(&mut app, &mut terminal),
        Some(ratatui::style::Color::Yellow)
    );
    app.now_ms = T0 + 3000;
    assert_ne!(
        flash_bg(&mut app, &mut terminal),
        Some(ratatui::style::Color::Yellow)
    );
}

#[test]
fn top_and_bottom() {
    let mut app = daisy_dock();
    let len = app.socket_rows().len();
    assert!(len > 12, "{len}");
    keys(&mut app, "G");
    assert_eq!(sel(&app), len - 1);
    keys(&mut app, "gg");
    assert_eq!(sel(&app), 0);
    keys(&mut app, "jjjjj");
    assert_eq!(sel(&app), 5);
    keys(&mut app, "kkkk");
    assert_eq!(sel(&app), 1);
    app.on_key(key(KeyCode::Down));
    assert_eq!(sel(&app), 2);
    app.on_key(key(KeyCode::Up));
    assert_eq!(sel(&app), 1);
    keys(&mut app, "k");
    keys(&mut app, "k");
    assert_eq!(sel(&app), 0);
    keys(&mut app, "G");
    keys(&mut app, "j");
    assert_eq!(sel(&app), len - 1);
}

#[test]
fn half_pages_follow_the_drawn_height() {
    let mut app = daisy_dock();
    render(&mut app, 120, 20);
    // 20 rows: top line and borders leave 17 for the socket list.
    assert_eq!(app.pane(Pane::Sockets).height, 17);
    app.on_key(ctrl('d'));
    assert_eq!(sel(&app), 8);
    app.on_key(ctrl('d'));
    assert_eq!(sel(&app), 16);
    app.on_key(ctrl('u'));
    assert_eq!(sel(&app), 8);
}

#[test]
fn tabs() {
    let mut app = daisy_dock();
    keys(&mut app, "gt");
    assert_eq!(app.tab, Tab::Charts);
    keys(&mut app, "gt");
    assert_eq!(app.tab, Tab::Events);
    keys(&mut app, "gt");
    assert_eq!(app.tab, Tab::Ports);
    keys(&mut app, "gT");
    assert_eq!(app.tab, Tab::Events);
    keys(&mut app, "1");
    assert_eq!(app.tab, Tab::Ports);
    keys(&mut app, "3");
    assert_eq!(app.tab, Tab::Events);
    keys(&mut app, "2");
    assert_eq!(app.tab, Tab::Charts);
    app.on_key(key(KeyCode::Tab));
    assert_eq!(app.tab, Tab::Events);
    app.on_key(key(KeyCode::BackTab));
    assert_eq!(app.tab, Tab::Charts);
}

#[test]
fn ctrl_w_moves_between_panes() {
    let mut app = daisy_dock();
    assert_eq!(app.focus(), Pane::Sockets);
    app.on_key(ctrl('w'));
    assert_eq!(app.pending_keys(), "^W");
    keys(&mut app, "l");
    assert_eq!(app.focus(), Pane::UsbC);
    app.on_key(ctrl('w'));
    keys(&mut app, "j");
    assert_eq!(app.focus(), Pane::Power);
    app.on_key(ctrl('w'));
    keys(&mut app, "j");
    assert_eq!(app.focus(), Pane::Health);
    app.on_key(ctrl('w'));
    keys(&mut app, "j");
    assert_eq!(app.focus(), Pane::Health);
    app.on_key(ctrl('w'));
    keys(&mut app, "h");
    assert_eq!(app.focus(), Pane::Sockets);

    // Narrow: one column, panels below.
    app.size = (80, 40);
    app.on_key(ctrl('w'));
    keys(&mut app, "l");
    assert_eq!(app.focus(), Pane::Sockets);
    app.on_key(ctrl('w'));
    keys(&mut app, "j");
    assert_eq!(app.focus(), Pane::UsbC);

    // Each tab keeps its own focus.
    keys(&mut app, "2");
    assert_eq!(app.focus(), Pane::PowerChart);
    app.on_key(ctrl('w'));
    keys(&mut app, "j");
    assert_eq!(app.focus(), Pane::CpuChart);
    keys(&mut app, "1");
    assert_eq!(app.focus(), Pane::UsbC);
}

#[test]
fn h_and_l_fold_hubs_or_change_panes() {
    let mut app = daisy_dock();
    let rows = app.socket_rows();
    let hub = rows.iter().position(|r| r.hub_name().is_some()).unwrap();
    let name = rows[hub].hub_name().unwrap().to_owned();
    for _ in 0..hub {
        keys(&mut app, "j");
    }
    keys(&mut app, "h");
    assert!(app.collapsed.contains(&name));
    assert_eq!(app.focus(), Pane::Sockets);
    assert!(app.socket_rows().len() < rows.len());
    insta::assert_snapshot!(render(&mut app, 120, 40));
    keys(&mut app, "l");
    assert!(app.collapsed.is_empty());
    assert_eq!(app.socket_rows().len(), rows.len());
    assert_eq!(app.focus(), Pane::Sockets);

    // Not on a hub: `l` / `h` move to the next / previous pane in the row.
    keys(&mut app, "gg");
    keys(&mut app, "l");
    assert_eq!(app.focus(), Pane::UsbC);
    app.on_key(key(KeyCode::Left));
    assert_eq!(app.focus(), Pane::Sockets);
}

#[test]
fn colon_q_and_q_quit() {
    let mut app = daisy_dock();
    keys(&mut app, ":q");
    assert!(!app.should_quit);
    app.on_key(key(KeyCode::Enter));
    assert!(app.should_quit);

    let mut app = daisy_dock();
    keys(&mut app, ":");
    app.on_key(key(KeyCode::Esc));
    keys(&mut app, "q");
    assert!(app.should_quit);
}

#[test]
fn pause_holds_updates_until_resumed() {
    let mut app = daisy_dock();
    keys(&mut app, "p");
    assert!(app.paused);
    let mut newer = app.snapshot.clone().unwrap();
    newer.taken_at_ms = T0 + 1000;
    let sample = Sample {
        t_ms: T0 + 1000,
        ..Sample::default()
    };
    app.on_update(newer, some_events(), sample);
    assert_eq!(app.snapshot.as_ref().unwrap().taken_at_ms, T0);
    assert!(app.events.is_empty());
    assert_eq!(app.history.len(), 1);
    assert!(
        render(&mut app, 120, 40)
            .lines()
            .next()
            .unwrap()
            .contains("PAUSED")
    );

    keys(&mut app, "p");
    assert!(!app.paused);
    assert_eq!(app.snapshot.as_ref().unwrap().taken_at_ms, T0 + 1000);
    assert_eq!(app.events.len(), 5);
    assert_eq!(app.history.len(), 2);
}

#[test]
fn events_follow_until_moved_up_and_clear() {
    let mut app = daisy_dock();
    keys(&mut app, "3");
    let snap = app.snapshot.clone().unwrap();
    app.on_update(snap.clone(), some_events(), Sample::default());
    assert_eq!(sel(&app), 4);
    keys(&mut app, "k");
    assert!(!app.events_follow);
    app.on_update(snap.clone(), some_events(), Sample::default());
    assert_eq!(sel(&app), 3);
    keys(&mut app, "G");
    assert!(app.events_follow);
    assert_eq!(sel(&app), 9);
    keys(&mut app, "c");
    assert!(app.events.is_empty());
    assert_eq!(sel(&app), 0);
}

#[test]
fn label_key_is_a_no_op_for_now() {
    let mut app = daisy_dock();
    keys(&mut app, "4j");
    let before = (app.tab, sel(&app), app.focus());
    keys(&mut app, "L");
    assert_eq!((app.tab, sel(&app), app.focus()), before);
    assert!(app.overlay.is_none());
    assert!(!app.should_quit);
}

/// Selects the row with this id in the Sockets tree.
fn select(app: &mut App, id: &str) {
    keys(app, "gg");
    let i = app
        .socket_rows()
        .iter()
        .position(|r| r.id == id)
        .unwrap_or_else(|| panic!("no row {id}"));
    for _ in 0..i {
        keys(app, "j");
    }
    assert_eq!(app.selected_row().unwrap().id, id);
}

fn detail_target(app: &App) -> Option<Target> {
    match &app.overlay {
        Some(Overlay::Detail(t)) => Some(t.clone()),
        _ => None,
    }
}

#[test]
fn enter_and_o_open_details_esc_and_q_close() {
    let mut app = daisy_dock();
    select(&mut app, "7-1.2");
    app.on_key(key(KeyCode::Enter));
    assert_eq!(detail_target(&app), Some(Target::Device("7-1.2".into())));
    // `j` / `k` scroll the popup, not the tree.
    keys(&mut app, "jj");
    assert_eq!(app.selected_row().unwrap().id, "7-1.2");
    app.on_key(key(KeyCode::Esc));
    assert!(app.overlay.is_none());

    keys(&mut app, "o");
    assert!(app.overlay.is_some());
    // `q` closes the popup; a second `q` quits.
    keys(&mut app, "q");
    assert!(app.overlay.is_none());
    assert!(!app.should_quit);

    // A socket title opens the socket's detail.
    keys(&mut app, "gg");
    keys(&mut app, "o");
    assert_eq!(
        detail_target(&app),
        Some(Target::Socket("0000:05:00.4-1".into()))
    );
    keys(&mut app, "q");
    keys(&mut app, "q");
    assert!(app.should_quit);
}

#[test]
fn detail_scrolls_when_long() {
    let mut app = daisy_dock();
    select(&mut app, "7-1.2");
    keys(&mut app, "o");
    let full = render(&mut app, 120, 40);
    let short = render(&mut app, 120, 14);
    assert!(full.contains("vendor:product"));
    assert!(short.contains("vendor:product"));
    keys(&mut app, "jjj");
    let scrolled = render(&mut app, 120, 14);
    assert!(!scrolled.contains("vendor:product"), "{scrolled}");
    assert_eq!(app.overlay_scroll, 3);
    keys(&mut app, "G");
    render(&mut app, 120, 14);
    assert!(app.overlay_scroll > 3);
    keys(&mut app, "gg");
    assert_eq!(app.overlay_scroll, 0);
}

#[test]
fn detail_of_the_ax88179b() {
    let mut app = daisy_dock();
    select(&mut app, "7-1.2");
    app.on_key(key(KeyCode::Enter));
    let screen = render(&mut app, 120, 40);
    assert!(screen.contains("0b95:1790 AX88179B"));
    assert!(screen.contains("linked at its best speed (5G)"));
    insta::assert_snapshot!(screen);
}

#[test]
fn detail_of_the_docks_usb2_hub() {
    let mut app = daisy_dock();
    select(&mut app, "6-1");
    keys(&mut app, "o");
    let screen = render(&mut app, 120, 40);
    assert!(screen.contains("USB 2 half of a USB 3 hub (its USB 3 half is linked)"));
    insta::assert_snapshot!(screen);
}

#[test]
fn detail_of_a_socket() {
    let mut app = daisy_dock();
    select(&mut app, "0000:06:00.3-1");
    keys(&mut app, "o");
    insta::assert_snapshot!(render(&mut app, 120, 40));
}

#[test]
fn the_tree_marks_devices_linked_below_their_speed() {
    // The dock's USB 2 hub reads "capable 5G, linked 480M": not marked.
    let mut app = daisy_dock();
    assert!(!render(&mut app, 120, 40).contains('↓'));

    // With its USB 3 half gone the same hub is stuck: marked.
    let mut snap = snapshot("daisy-dock", "daisy");
    let hinge = snap
        .sockets
        .iter_mut()
        .find(|s| s.key == "0000:06:00.3-1")
        .unwrap();
    if let Usb3Side::Paired(h) = &mut hinge.usb3 {
        h.device = None;
    }
    let mut app = app_with(snap);
    let screen = render(&mut app, 120, 40);
    assert!(screen.contains("480M↓ 05e3:0610  USB2.1 Hub"), "{screen}");
}

#[test]
fn help_toggles_with_question_mark() {
    let mut app = daisy_dock();
    keys(&mut app, "?");
    assert_eq!(app.overlay, Some(Overlay::Help));
    let screen = render(&mut app, 120, 40);
    for b in plugii::tui::keys::BINDINGS {
        assert!(screen.contains(b.help), "{}", b.help);
    }
    insta::assert_snapshot!(screen);
    keys(&mut app, "?");
    assert!(app.overlay.is_none());
    keys(&mut app, "?");
    app.on_key(key(KeyCode::Esc));
    assert!(app.overlay.is_none());
    keys(&mut app, "?q");
    assert!(app.overlay.is_none());
    assert!(!app.should_quit);
    // `h` never opens help.
    keys(&mut app, "h");
    assert!(app.overlay.is_none());
}

#[test]
fn filter_prompt_takes_every_key() {
    let mut app = daisy_dock();
    keys(&mut app, "/");
    assert!(app.prompt);
    // `q`, `j`, `?`, `o` and digits type text.
    keys(&mut app, "qj?o1");
    assert!(!app.should_quit);
    assert!(app.overlay.is_none());
    assert_eq!(app.tab, Tab::Ports);
    assert_eq!(app.sockets_filter, "qj?o1");
    let last = render(&mut app, 120, 40).lines().last().unwrap().to_owned();
    assert_eq!(last, "/qj?o1");
    app.on_key(key(KeyCode::Backspace));
    assert_eq!(app.sockets_filter, "qj?o");
    // Esc cancels and clears.
    app.on_key(key(KeyCode::Esc));
    assert!(!app.prompt);
    assert_eq!(app.sockets_filter, "");
    keys(&mut app, "q");
    assert!(app.should_quit);
}

#[test]
fn filtered_tree_for_hub() {
    let mut app = daisy_dock();
    let all = app.socket_rows().len();
    keys(&mut app, "/HUB");
    app.on_key(key(KeyCode::Enter));
    assert!(!app.prompt);
    assert_eq!(app.sockets_filter, "HUB");
    let rows = app.socket_rows();
    assert!(rows.len() < all);
    // Every socket title stays; the AX88179B (no "hub" in it) goes.
    assert_eq!(
        rows.iter()
            .filter(|r| matches!(r.kind, RowKind::Socket { .. }))
            .count(),
        4
    );
    assert!(!rows.iter().any(|r| r.id == "7-1.2"));
    // ⏎ jumped to the first match.
    assert_eq!(app.selected_row().unwrap().id, "7-1");
    let screen = render(&mut app, 120, 40);
    assert!(screen.lines().next().unwrap().contains("/HUB"));
    insta::assert_snapshot!(screen);

    // `n` / `N` step through the matches, wrapping.
    keys(&mut app, "n");
    assert_eq!(app.selected_row().unwrap().id, "6-1");
    keys(&mut app, "n");
    assert_eq!(app.selected_row().unwrap().id, "6-1.2");
    keys(&mut app, "n");
    assert_eq!(app.selected_row().unwrap().id, "7-1");
    keys(&mut app, "N");
    assert_eq!(app.selected_row().unwrap().id, "6-1.2");

    // Esc on the tree clears the filter, keeping the selection.
    app.on_key(key(KeyCode::Esc));
    assert_eq!(app.sockets_filter, "");
    assert_eq!(app.socket_rows().len(), all);
    assert_eq!(app.selected_row().unwrap().id, "6-1.2");
}

#[test]
fn filter_matches_vid_pid_and_manufacturer() {
    let mut app = daisy_dock();
    keys(&mut app, "/0b95:1790");
    let ids: Vec<String> = app
        .socket_rows()
        .iter()
        .filter(|r| matches!(r.kind, RowKind::Device { .. }))
        .map(|r| r.id.clone())
        .collect();
    // The AX88179B and the hub above it.
    assert_eq!(ids, ["7-1", "7-1.2"]);
    app.on_key(key(KeyCode::Esc));
    keys(&mut app, "/asix");
    app.on_key(key(KeyCode::Enter));
    assert_eq!(app.selected_row().unwrap().id, "7-1.2");
}

#[test]
fn filter_on_events() {
    let mut app = daisy_dock();
    let snap = app.snapshot.clone().unwrap();
    app.on_update(snap, some_events(), Sample::default());
    keys(&mut app, "3/hinge");
    app.on_key(key(KeyCode::Enter));
    assert_eq!(app.visible_events().len(), 2);
    let screen = render(&mut app, 100, 12);
    assert!(screen.contains("05e3:0626 hub"));
    assert!(!screen.contains("Unifying"));
    assert!(screen.lines().next().unwrap().contains("/hinge"));
    keys(&mut app, "n");
    assert_eq!(sel(&app), 0);
    keys(&mut app, "N");
    assert_eq!(sel(&app), 1);
    app.on_key(key(KeyCode::Esc));
    assert_eq!(app.visible_events().len(), 5);
}

#[test]
fn n_without_a_filter_does_nothing() {
    let mut app = daisy_dock();
    keys(&mut app, "jj");
    keys(&mut app, "nN");
    assert_eq!(sel(&app), 2);
}

/// daisy-dock at T0 − 1 s, then daisy-empty at T0 with its diff.
fn unplugged() -> App {
    let mut dock = snapshot("daisy-dock", "daisy");
    dock.taken_at_ms = T0 - 1000;
    let mut app = app_with(dock.clone());
    let empty = snapshot("daisy-empty", "daisy");
    let events = diff::diff(&dock, &empty);
    assert!(events.iter().any(|e| e.kind == EventKind::Removed));
    app.on_update(empty, events, Sample::default());
    app
}

#[test]
fn ghosts_stay_30_s() {
    let mut app = unplugged();
    let ghosts = |app: &App| {
        app.socket_rows()
            .iter()
            .filter(|r| r.id.starts_with("ghost:"))
            .count()
    };
    // Both halves' hubs with what hung off them.
    assert_eq!(ghosts(&app), 4);
    app.now_ms = T0 + 10_000;
    let screen = render(&mut app, 120, 40);
    assert!(screen.contains("(gone)"));
    insta::assert_snapshot!(screen);

    // Details still work on a ghost.
    select(&mut app, "ghost:7-1.2");
    keys(&mut app, "o");
    let detail = render(&mut app, 120, 40);
    assert!(detail.contains("unplugged 10 s ago"), "{detail}");
    keys(&mut app, "q");

    app.now_ms = T0 + 40_000;
    assert_eq!(ghosts(&app), 0);
    assert!(!render(&mut app, 120, 40).contains("(gone)"));
}

#[test]
fn a_ghost_goes_when_its_device_comes_back() {
    let mut app = unplugged();
    let mut back = snapshot("daisy-dock", "daisy");
    back.taken_at_ms = T0 + 2000;
    app.now_ms = T0 + 2000;
    app.on_update(back, Vec::new(), Sample::default());
    assert!(app.ghosts.is_empty());
    assert!(!app.socket_rows().iter().any(|r| r.id.starts_with("ghost:")));
}

#[test]
fn no_ghosts_without_a_removed_event() {
    let mut dock = snapshot("daisy-dock", "daisy");
    dock.taken_at_ms = T0 - 1000;
    let mut app = app_with(dock);
    app.on_update(
        snapshot("daisy-empty", "daisy"),
        Vec::new(),
        Sample::default(),
    );
    assert!(app.ghosts.is_empty());
}
