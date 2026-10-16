//! The TUI without a terminal: keys against the app state, and screens
//! rendered through ratatui's `TestBackend` from the fixture machines and
//! hand-made histories and events.

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use plugii::collect::{self, CollectOpts};
use plugii::diff::{Event, EventKind};
use plugii::history::{History, Sample};
use plugii::labels;
use plugii::model::Snapshot;
use plugii::sysfs::Root;
use plugii::tui::App;
use plugii::tui::app::{self, Pane, Tab};
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
fn top_and_bottom_and_counts() {
    let mut app = daisy_dock();
    let len = app.socket_rows().len();
    assert!(len > 12, "{len}");
    keys(&mut app, "G");
    assert_eq!(sel(&app), len - 1);
    keys(&mut app, "gg");
    assert_eq!(sel(&app), 0);
    keys(&mut app, "5j");
    assert_eq!(sel(&app), 5);
    keys(&mut app, "4k");
    assert_eq!(sel(&app), 1);
    app.on_key(key(KeyCode::Down));
    assert_eq!(sel(&app), 2);
    app.on_key(key(KeyCode::Up));
    assert_eq!(sel(&app), 1);
    keys(&mut app, "k");
    keys(&mut app, "k");
    assert_eq!(sel(&app), 0);
    keys(&mut app, "6gg");
    assert_eq!(sel(&app), 5);
    keys(&mut app, "99j");
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
fn later_keys_are_no_ops() {
    let mut app = daisy_dock();
    keys(&mut app, "4j");
    let before = (app.tab, sel(&app), app.focus());
    for c in ['o', '/', 'n', 'N', 'L', '?'] {
        app.on_key(key(KeyCode::Char(c)));
    }
    app.on_key(key(KeyCode::Enter));
    assert_eq!((app.tab, sel(&app), app.focus()), before);
    assert!(!app.should_quit);
}
