//! The TUI without a terminal: keys against the app state, and screens
//! rendered through ratatui's `TestBackend` from the fixture machines and
//! hand-made histories and events.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use plugii::collect::{self, CollectOpts};
use plugii::diff::{self, Event, EventKind};
use plugii::history::{History, Sample};
use plugii::labels::{self, Key};
use plugii::model::{CpuSample, LinkResets, Snapshot, Usb3Side, UsbDevice};
use plugii::sysfs::Root;
use plugii::tui::App;
use plugii::tui::app::{self, LabelsFile, Overlay, Pane, Tab};
use plugii::tui::detail::Target;
use plugii::tui::ports::RowKind;
use plugii::tui::ui;
use plugii::watch;
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

/// daisy-dock with daisy's labels plus `extra` lines.
fn daisy_dock_labelled(extra: &str) -> Snapshot {
    let text = std::fs::read_to_string(fixtures().join("labels/daisy")).unwrap() + extra;
    let root = Root::new(fixtures().join("sysfs/daisy-dock"));
    let mut snap = collect::snapshot_with(
        &root,
        &labels::Labels::parse(&text),
        &CollectOpts::offline(),
        Some(Default::default()),
        None,
    );
    snap.taken_at_ms = T0;
    snap
}

#[test]
fn ports_daisy_dock_with_device_labels_120x40() {
    let mut app = app_with(daisy_dock_labelled(
        "device:05e3:0626  Desk switch\ndevice:0B95:1790  Ethernet (desk)\n",
    ));
    let screen = render(&mut app, 120, 40);
    // The label in place of the product, vid:pid kept.
    assert!(screen.contains("  5G  05e3:0626  Desk switch "), "{screen}");
    assert!(
        screen.contains("  5G  0b95:1790  Ethernet (desk) "),
        "{screen}"
    );
    assert!(screen.contains("480M  05e3:0610  USB2.1 Hub "), "{screen}");
    insta::assert_snapshot!(screen);
    // The filter finds devices by label too.
    keys(&mut app, "/ethernet");
    app.on_key(key(KeyCode::Enter));
    assert_eq!(app.selected_row().unwrap().id, "7-1.2");
}

#[test]
fn detail_shows_the_device_label() {
    let mut app = app_with(daisy_dock_labelled("device:0b95:1790  Ethernet (desk)\n"));
    select(&mut app, "7-1.2");
    keys(&mut app, "o");
    let screen = render(&mut app, 120, 40);
    assert!(
        screen.contains("│ label              Ethernet (desk) "),
        "{screen}"
    );
}

/// The app on daisy-dock with daisy's labels file copied into a temporary
/// config directory, where `L` writes.
fn labelling() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let path = labels::path(dir.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::copy(fixtures().join("labels/daisy"), &path).unwrap();
    let shared = Arc::new(RwLock::new(labels::read(&path).unwrap()));
    let mut app = App::new(0);
    app.now_ms = T0;
    app.labels_file = Some(LabelsFile::new(
        path,
        "~/.config/plugii/labels".into(),
        shared,
    ));
    let snap = snapshot("daisy-dock", "daisy");
    let sample = Sample {
        t_ms: snap.taken_at_ms,
        ..Sample::default()
    };
    app.on_update(snap, Vec::new(), sample);
    (dir, app)
}

fn labels_text(app: &App) -> String {
    std::fs::read_to_string(&app.labels_file.as_ref().unwrap().path).unwrap()
}

fn prompt(app: &App) -> Option<(Key, String, String)> {
    app.label_prompt
        .as_ref()
        .map(|p| (p.key.clone(), p.title.clone(), p.text.clone()))
}

fn notice(app: &App) -> Option<(String, bool)> {
    app.notice.as_ref().map(|n| (n.text.clone(), n.error))
}

#[test]
fn l_on_a_socket_row_opens_the_socket_prompt() {
    let (_dir, mut app) = labelling();
    let before = labels_text(&app);
    let hinge = (
        Key::Socket("0000:06:00.3-1".into()),
        "label socket 0000:06:00.3-1: ".to_owned(),
        "USB-C (hinge)".to_owned(),
    );
    // The card's title, its halves and its USB-C row all label the socket.
    for id in [
        "0000:06:00.3-1",
        "0000:06:00.3-1/usb3",
        "0000:06:00.3-1/usb2",
    ] {
        select(&mut app, id);
        keys(&mut app, "L");
        assert_eq!(prompt(&app), Some(hinge.clone()), "{id}");
        let screen = render(&mut app, 120, 40);
        assert_eq!(
            screen.lines().last().unwrap(),
            "label socket 0000:06:00.3-1: USB-C (hinge)"
        );
        // Esc cancels: nothing written, the tree keeps its selection.
        app.on_key(key(KeyCode::Esc));
        assert_eq!(prompt(&app), None);
        assert_eq!(app.selected_row().unwrap().id, id);
    }
    select(&mut app, "0000:06:00.4-1/typec");
    keys(&mut app, "L");
    assert_eq!(
        prompt(&app).unwrap().1,
        "label socket 0000:06:00.4-1: ".to_owned()
    );
    app.on_key(ctrl('c'));
    assert_eq!(prompt(&app), None);
    assert!(!app.should_quit);
    assert_eq!(labels_text(&app), before);
    assert_eq!(notice(&app), None);
}

#[test]
fn l_on_a_device_row_labels_its_vid_pid() {
    let (_dir, mut app) = labelling();
    select(&mut app, "7-1.2");
    keys(&mut app, "L");
    assert_eq!(
        prompt(&app),
        Some((
            Key::Device {
                vendor_id: 0x0b95,
                product_id: 0x1790
            },
            "label device 0b95:1790 (AX88179B): ".into(),
            String::new()
        ))
    );
    // Every key types: `q`, `j`, `?`, `L`, digits.
    keys(&mut app, "qj?L1");
    assert_eq!(prompt(&app).unwrap().2, "qj?L1");
    assert!(!app.should_quit);
    assert!(app.overlay.is_none());
    assert_eq!(app.selected_row().unwrap().id, "7-1.2");
    // Ctrl-u clears, Backspace deletes.
    app.on_key(ctrl('u'));
    keys(&mut app, "Ethernet (desk)x");
    app.on_key(key(KeyCode::Backspace));
    assert_eq!(
        render(&mut app, 120, 40).lines().last().unwrap(),
        "label device 0b95:1790 (AX88179B): Ethernet (desk)"
    );

    app.on_key(key(KeyCode::Enter));
    assert_eq!(prompt(&app), None);
    let daisy = std::fs::read_to_string(fixtures().join("labels/daisy")).unwrap();
    assert_eq!(
        labels_text(&app),
        format!("{daisy}device:0b95:1790  Ethernet (desk)\n")
    );
    assert_eq!(
        notice(&app),
        Some((
            "labelled device 0b95:1790 \"Ethernet (desk)\" in ~/.config/plugii/labels".into(),
            false
        ))
    );
    // Shown at once, and handed to the watcher.
    let screen = render(&mut app, 120, 40);
    assert!(
        screen.contains("5G  0b95:1790  Ethernet (desk)"),
        "{screen}"
    );
    assert!(screen.contains("labelled device 0b95:1790"), "{screen}");
    let shared = watch::current(&app.labels_file.as_ref().unwrap().labels);
    assert_eq!(shared.device_label(0x0b95, 0x1790), Some("Ethernet (desk)"));

    // A scan the watcher took with the old labels shows the new ones.
    let stale = snapshot("daisy-dock", "daisy");
    app.on_update(stale, Vec::new(), Sample::default());
    assert!(render(&mut app, 120, 40).contains("0b95:1790  Ethernet (desk)"));

    // `L` again starts from the label; an empty one removes it.
    keys(&mut app, "L");
    assert_eq!(prompt(&app).unwrap().2, "Ethernet (desk)");
    app.on_key(ctrl('u'));
    app.on_key(key(KeyCode::Enter));
    assert_eq!(labels_text(&app), daisy);
    assert_eq!(
        notice(&app),
        Some((
            "unlabelled device 0b95:1790 in ~/.config/plugii/labels".into(),
            false
        ))
    );
    assert!(render(&mut app, 120, 40).contains("5G  0b95:1790  AX88179B"));
}

/// The UCSI port is found by its label matching the socket's, so it is
/// renamed with the socket and the two stay together.
#[test]
fn relabelling_a_socket_keeps_its_usb_c_port() {
    let (_dir, mut app) = labelling();
    select(&mut app, "0000:06:00.4-1");
    keys(&mut app, "L");
    app.on_key(ctrl('u'));
    keys(&mut app, "USB-C (left)");
    app.on_key(key(KeyCode::Enter));
    let text = labels_text(&app);
    assert!(text.contains("\n0000:06:00.4-1  USB-C (left)\n"), "{text}");
    assert!(text.contains("\ntypec:port0  USB-C (left)\n"), "{text}");
    assert!(text.contains("# UCSI ports: port0 = caps lock"), "{text}");
    let s = app.snapshot.as_ref().unwrap();
    let socket = s
        .sockets
        .iter()
        .find(|x| x.key == "0000:06:00.4-1")
        .unwrap();
    assert_eq!(socket.label.as_deref(), Some("USB-C (left)"));
    assert_eq!(socket.typec_port.as_deref(), Some("port0"));
    assert_eq!(s.typec.ports[0].label.as_deref(), Some("USB-C (left)"));
    let screen = render(&mut app, 120, 40);
    assert!(screen.contains("─ USB-C (left) ─"), "{screen}");
    assert!(screen.contains("port0 · USB-C (left)  partner"), "{screen}");
    assert_eq!(app.selected_row().unwrap().id, "0000:06:00.4-1");
}

#[test]
fn a_label_that_cannot_be_written_is_an_error() {
    let (dir, mut app) = labelling();
    // The directory the file would go in is a file.
    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, "").unwrap();
    app.labels_file.as_mut().unwrap().path = blocker.join("plugii/labels");
    select(&mut app, "7-1.2");
    keys(&mut app, "Lx");
    app.on_key(key(KeyCode::Enter));
    let (text, error) = notice(&app).unwrap();
    assert!(error);
    assert!(
        text.starts_with("can't write ~/.config/plugii/labels: "),
        "{text}"
    );
    assert!(render(&mut app, 120, 40).contains("0b95:1790  AX88179B"));
}

#[test]
fn l_without_a_config_directory_says_so() {
    let mut app = daisy_dock();
    select(&mut app, "7-1.2");
    keys(&mut app, "L");
    assert_eq!(prompt(&app), None);
    assert!(notice(&app).unwrap().1);
    // Only on the Ports tree.
    let (_dir, mut app) = labelling();
    keys(&mut app, "3L");
    assert_eq!(prompt(&app), None);
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

/// Hand-set URB counters: each device's counter goes up by `per_s(name)`
/// per call, from where the fixture left it.
fn add_urbs(d: &mut UsbDevice, counts: &mut HashMap<String, u64>, per_s: &dyn Fn(&str) -> u64) {
    let n = counts
        .entry(d.sysfs_name.clone())
        .or_insert(d.urbnum.unwrap_or(0));
    *n += per_s(&d.sysfs_name);
    d.urbnum = Some(*n);
    for child in &mut d.children {
        add_urbs(child, counts, per_s);
    }
}

fn cpu(ghz: f32, package_w: f32, temp_c: f32) -> Option<CpuSample> {
    Some(CpuSample {
        util_pct: 50,
        avg_khz: (ghz * 1e6) as u32,
        max_khz: 4_900_000,
        min_khz: Some(400_000),
        package_w: Some(package_w),
        package_temp_c: Some(temp_c),
    })
}

/// Feeds `snaps` to a fresh app one second apart, ending at `T0`, with the
/// events between them as the run loop makes them; opens the Charts tab.
fn session(snaps: Vec<Snapshot>) -> App {
    let mut app = App::new(0);
    let n = snaps.len() as u64;
    let mut prev: Option<Snapshot> = None;
    for (i, mut snap) in snaps.into_iter().enumerate() {
        let t = T0 - (n - 1 - i as u64) * 1000;
        snap.taken_at_ms = t;
        app.now_ms = t;
        let events = prev
            .as_ref()
            .map(|p| diff::diff(p, &snap))
            .unwrap_or_default();
        let sample = Sample::from_snapshot(&snap);
        prev = Some(snap.clone());
        app.on_update(snap, events, sample);
    }
    keys(&mut app, "2");
    app
}

/// A minute and a half on the ThinkPad: nothing plugged; the dock, whose
/// hub first comes up at USB 2 only and whose disk copies for a while; a
/// gap; then a plain charger on the same port.
fn daisy_session() -> App {
    let empty = snapshot("daisy-empty", "daisy");
    let dock = snapshot("daisy-dock", "daisy");
    let charger = snapshot("daisy-charger", "daisy");
    let mut counts = HashMap::new();
    let snaps = (0..90u64)
        .map(|i| {
            let mut snap = match i {
                0..20 => empty.clone(),
                20..50 => dock.clone(),
                50..55 => empty.clone(),
                _ => charger.clone(),
            };
            let busy = (30..42).contains(&i);
            let per_s = |name: &str| match name {
                "7-1.2" if busy => 4000 + 300 * (i % 4),
                "7-1.2" => 120 + 10 * (i % 3),
                _ => 2,
            };
            for socket in &mut snap.sockets {
                if let Usb3Side::Paired(half) = &mut socket.usb3 {
                    if (20..24).contains(&i) {
                        half.device = None;
                    }
                    if let Some(d) = &mut half.device {
                        add_urbs(d, &mut counts, &per_s);
                    }
                }
                if let Some(d) = &mut socket.usb2.device {
                    add_urbs(d, &mut counts, &per_s);
                }
            }
            let load = if busy { 1.0 } else { 0.2 };
            let wave = (i as f32 / 4.0).sin();
            snap.cpu = cpu(
                1.4 + 2.6 * load + 0.2 * wave,
                6.0 + 22.0 * load,
                50.0 + 30.0 * load,
            );
            let resets = match i {
                0..21 => 0,
                21..36 => 1,
                _ => 3,
            };
            if resets > 0 {
                snap.kernel_log.link_resets = vec![LinkResets {
                    place: "USB-C (hinge)".into(),
                    count: resets,
                }];
            }
            snap
        })
        .collect();
    session(snaps)
}

#[test]
fn charts_daisy_session_120x40() {
    let mut app = daisy_session();
    assert_eq!(app.stats.chargers.len(), 2);
    assert_eq!(app.stats.active, Some(1));
    insta::assert_snapshot!(render(&mut app, 120, 40));
}

#[test]
fn charts_daisy_session_80x40() {
    let mut app = daisy_session();
    let screen = render(&mut app, 80, 40);
    assert!(app.narrow());
    insta::assert_snapshot!(screen);
}

#[test]
fn charts_daisy_session_120x24_pages_the_lower_panes() {
    let mut app = daisy_session();
    let screen = render(&mut app, 120, 24);
    assert!(screen.contains("Link speed"));
    assert!(!screen.contains("Chargers this session"));
    insta::assert_snapshot!(screen);
    // `Ctrl-w j` past the CPU chart pages through them.
    for _ in 0..4 {
        app.on_key(ctrl('w'));
        keys(&mut app, "j");
    }
    assert_eq!(app.focus(), Pane::Chargers);
    let screen = render(&mut app, 120, 24);
    assert!(!screen.contains("Link speed"));
    assert!(screen.contains("drawing 3.25 A"));
}

#[test]
fn charts_lower_panes_shrink_and_scroll() {
    let mut app = daisy_session();
    // 28 body rows: the charts keep 16, the three panes share 12.
    render(&mut app, 120, 29);
    for pane in [Pane::Links, Pane::Urbs] {
        assert_eq!(app.pane(pane).height, 2, "{pane:?}");
    }
    // Below its header.
    assert_eq!(app.pane(Pane::Chargers).height, 1);
    for _ in 0..2 {
        app.on_key(ctrl('w'));
        keys(&mut app, "j");
    }
    assert_eq!(app.focus(), Pane::Links);
    keys(&mut app, "j");
    let screen = render(&mut app, 120, 29);
    assert_eq!(app.pane(Pane::Links).offset, 1);
    let mut lines = screen.lines().skip_while(|l| !l.contains("Link speed"));
    assert!(lines.nth(1).unwrap().starts_with("│USB-A (left)     │"));
    keys(&mut app, "G");
    render(&mut app, 120, 29);
    assert_eq!(app.pane(Pane::Links).offset, 2);
    keys(&mut app, "gg");
    render(&mut app, 120, 29);
    assert_eq!(app.pane(Pane::Links).offset, 0);
}

/// The XPS: unplugged, then on a 47 W charger that lists its PDOs, then on
/// the dock, whose PDOs UCSI doesn't report.
#[test]
fn charts_xps_session_120x40() {
    let empty = snapshot("xps-empty", "xps");
    let charger = snapshot("xps", "xps");
    let dock = snapshot("xps-dock-charger", "xps");
    let snaps = (0..40u64)
        .map(|i| {
            let mut snap = match i {
                0..10 => empty.clone(),
                10..25 => charger.clone(),
                _ => dock.clone(),
            };
            snap.cpu = cpu(2.0 + (i % 5) as f32 * 0.3, 10.0 + i as f32 * 0.5, 60.0);
            snap
        })
        .collect();
    let mut app = session(snaps);
    assert_eq!(app.stats.chargers.len(), 2);
    insta::assert_snapshot!(render(&mut app, 120, 40));
}

#[test]
fn pause_holds_the_chart_stats_too() {
    let mut app = daisy_dock();
    assert_eq!(app.stats.chargers.len(), 1);
    keys(&mut app, "p");
    for (i, name) in ["daisy-empty", "daisy-charger"].into_iter().enumerate() {
        let mut snap = snapshot(name, "daisy");
        snap.taken_at_ms = T0 + 1000 * (i as u64 + 1);
        app.on_update(snap, Vec::new(), Sample::default());
    }
    assert_eq!(app.stats.chargers.len(), 1);
    assert_eq!(app.stats.sockets[0].links.len(), 1);
    keys(&mut app, "p");
    // Not reporting offers, a charger drawing other amps is another one.
    assert_eq!(app.stats.chargers.len(), 2);
    assert_eq!(app.stats.sockets[0].links.len(), 3);
}
