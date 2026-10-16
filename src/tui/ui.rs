//! Drawing: the top line, then the current tab.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Tabs};

use super::app::{App, FLASH_MS, Overlay, Pane, Tab, clock};
use super::detail::{self, Target};
use super::{charts, ports};
use crate::diff::{Event, EventKind};

/// Draws the whole screen. Takes `&mut App` only to keep each pane's scroll
/// offset in range of what it shows and to record its height.
pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    app.size = (area.width, area.height);
    let prompt_rows = u16::from(app.prompt || app.label_prompt.is_some());
    let [top, body, prompt] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(prompt_rows),
    ])
    .areas(area);
    top_line(f, top, app);
    match app.tab {
        Tab::Ports => ports_tab(f, body, app),
        Tab::Charts => charts::draw(f, body, app),
        Tab::Events => {
            let mut lines: Vec<Line> = app
                .visible_events()
                .into_iter()
                .map(|e| event_line(e, app))
                .collect();
            if lines.is_empty() {
                let text = if app.events.is_empty() {
                    "no events yet"
                } else {
                    "no events match the filter"
                };
                lines.push(Line::from(text.dark_gray()));
            }
            list_pane(f, body, app, Pane::Events, "Events", lines, &[]);
        }
    }
    if let Some(p) = &app.label_prompt {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                p.title.clone().cyan().bold(),
                Span::raw(p.text.clone()),
                " ".reversed(),
            ])),
            prompt,
        );
    } else if app.prompt {
        let text = app.filter().unwrap_or_default().to_owned();
        f.render_widget(
            Paragraph::new(Line::from(vec![
                "/".cyan().bold(),
                Span::raw(text),
                " ".reversed(),
            ])),
            prompt,
        );
    }
    overlay(f, body, app);
}

/// The detail or help popup, centred over `area`.
fn overlay(f: &mut Frame, area: Rect, app: &mut App) {
    let popup = match (&app.overlay, &app.snapshot) {
        (Some(Overlay::Help), _) => detail::help(),
        (Some(Overlay::Detail(Target::Device(name))), Some(s)) => {
            detail::device(s, &app.live_ghosts(), name, app.view_now_ms())
        }
        (Some(Overlay::Detail(Target::Socket(key))), Some(s)) => detail::socket(s, key),
        _ => return,
    };
    let content_width = popup.lines.iter().map(Line::width).max().unwrap_or(0) as u16;
    let width = (content_width + 4).clamp(60, 96).min(area.width);
    let height = (popup.lines.len() as u16 + 2).min(area.height);
    let [_, mid, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .areas(area);
    let [_, rect, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(width),
        Constraint::Fill(1),
    ])
    .areas(mid);
    let block = Block::bordered()
        .border_style(Style::new().fg(Color::Cyan))
        .title(Span::raw(format!(" {} ", popup.title)).bold().cyan())
        .title_bottom(Line::from(" Esc/q close · j/k scroll ".dark_gray()).right_aligned());
    let inner = block.inner(rect).inner(Margin::new(1, 0));
    f.render_widget(Clear, rect);
    f.render_widget(block, rect);
    let height = inner.height as usize;
    app.overlay_height = height;
    app.overlay_scroll = app
        .overlay_scroll
        .min(popup.lines.len().saturating_sub(height));
    let shown: Vec<Line> = popup
        .lines
        .into_iter()
        .skip(app.overlay_scroll)
        .take(height)
        .collect();
    f.render_widget(Paragraph::new(shown), inner);
}

fn top_line(f: &mut Frame, area: Rect, app: &App) {
    let tabs = Tabs::new(Tab::ALL.iter().map(|t| t.title()))
        .select(app.tab.index())
        .highlight_style(
            Style::new()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        )
        .divider("│");
    let tabs_width = Tab::ALL
        .iter()
        .map(|t| t.title().len() as u16 + 3)
        .sum::<u16>();
    let status = status_spans(app);
    let status_width = status.iter().map(|s| s.width() as u16).sum::<u16>();
    let [tabs_area, host_area, status_area] = Layout::horizontal([
        Constraint::Length(tabs_width),
        Constraint::Min(0),
        Constraint::Length(status_width + 1),
    ])
    .areas(area);
    f.render_widget(tabs, tabs_area);
    let host = match (&app.replay, &app.snapshot) {
        (Some(r), _) => Some(r.host.clone()),
        (None, Some(s)) => Some(s.host.clone()),
        (None, None) => None,
    };
    if let Some(host) = host {
        f.render_widget(
            Paragraph::new(Line::from(vec![" ".into(), host.bold()])),
            host_area,
        );
    }
    f.render_widget(
        Paragraph::new(Line::from(status)).right_aligned(),
        status_area,
    );
}

fn status_spans(app: &App) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let pending = app.pending_keys();
    if !pending.is_empty() {
        out.push(pending.cyan());
        out.push("  ".into());
    }
    if let Some(filter) = app.filter().filter(|f| !f.is_empty() && !app.prompt) {
        out.push(format!("/{filter}").cyan());
        out.push("  ".into());
    }
    if let Some(n) = &app.notice {
        out.push(if n.error {
            n.text.clone().red()
        } else {
            n.text.clone().green()
        });
        out.push("  ".into());
    }
    if let Some(w) = &app.warning {
        out.push(w.clone().yellow());
        out.push("  ".into());
    }
    if let Some(file) = &app.recording {
        out.push("● REC ".red().bold());
        out.push(file.clone().red());
        out.push("  ".into());
    }
    if let Some(r) = &app.replay {
        if r.ended {
            out.push("end of recording".yellow());
            out.push("  ".into());
        }
        let (icon, style) = if r.paused {
            ("⏸", Style::new().fg(Color::Black).bg(Color::Yellow).bold())
        } else {
            ("▶", Style::new().fg(Color::Cyan).bold())
        };
        out.push(Span::styled(
            format!(
                "{icon} replay {} / {} ×{}",
                duration(r.at_ms.saturating_sub(r.start_ms)),
                duration(r.end_ms.saturating_sub(r.start_ms)),
                r.speed
            ),
            style,
        ));
        out.push("  ".into());
        out.push(r.file.clone().dark_gray());
    } else if app.paused {
        out.push(" PAUSED ".black().on_yellow().bold());
    } else {
        match &app.snapshot {
            Some(s) => {
                out.push(format!("updated {}", clock(s.taken_at_ms, app.utc_offset_s)).dark_gray())
            }
            None => out.push("waiting for the first scan…".dark_gray()),
        }
    }
    out
}

/// `HH:MM:SS` for a span of ms.
fn duration(ms: u64) -> String {
    let s = ms / 1000;
    format!("{:02}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
}

pub(super) fn block(app: &App, pane: Pane, title: &str) -> Block<'static> {
    let focused = app.focus() == pane;
    let border = if focused {
        Style::new().fg(Color::Cyan)
    } else {
        Style::new().fg(Color::DarkGray)
    };
    let title = if focused {
        Span::raw(format!(" {title} ")).bold().cyan()
    } else {
        Span::raw(format!(" {title} ")).bold()
    };
    Block::bordered().border_style(border).title(title)
}

fn ports_tab(f: &mut Frame, area: Rect, app: &mut App) {
    let Some(snap) = app.snapshot.clone() else {
        f.render_widget(
            Paragraph::new("waiting for the first scan…".dark_gray()).block(block(
                app,
                Pane::Sockets,
                "Sockets",
            )),
            area,
        );
        return;
    };
    let usb_c = ports::usb_c_lines(&snap);
    let power_width = if app.narrow() {
        area.width
    } else {
        area.width - area.width / 2
    };
    let power = ports::power_lines(&snap, power_width.saturating_sub(2) as usize);
    let (sockets_area, usb_c_area, power_area, health_area) = if app.narrow() {
        let health_rows =
            ports::health_lines(&app.findings, area.width.saturating_sub(2) as usize).len() as u16;
        let [a, b, c, d] = Layout::vertical([
            Constraint::Min(8),
            Constraint::Length(usb_c.len() as u16 + 2),
            Constraint::Length(power.len() as u16 + 2),
            Constraint::Length(health_rows + 2),
        ])
        .areas(area);
        (a, b, c, d)
    } else {
        let [left, right] =
            Layout::horizontal([Constraint::Length(area.width / 2), Constraint::Min(0)])
                .areas(area);
        let [b, c, d] = Layout::vertical([
            Constraint::Length(usb_c.len() as u16 + 2),
            Constraint::Length(power.len() as u16 + 2),
            Constraint::Min(3),
        ])
        .areas(right);
        (left, b, c, d)
    };

    let rows = app.socket_rows();
    let width = sockets_area.width.saturating_sub(2) as usize;
    let now = app.view_now_ms();
    let flashing: Vec<&str> = app
        .events
        .iter()
        .filter(|e| e.t_ms <= now && now - e.t_ms < FLASH_MS)
        .filter_map(|e| e.socket.as_deref())
        .collect();
    let mut flash = Vec::new();
    let lines: Vec<Line> = rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            if flashing.iter().any(|s| row.socket_matches(s)) {
                flash.push(i);
            }
            ports::row_line(row, width)
        })
        .collect();
    let lines = if lines.is_empty() {
        vec![Line::from("no hotplug USB ports found".dark_gray())]
    } else {
        lines
    };
    list_pane(
        f,
        sockets_area,
        app,
        Pane::Sockets,
        "Sockets",
        lines,
        &flash,
    );
    scroll_pane(f, usb_c_area, app, Pane::UsbC, "USB-C / PD", usb_c);
    scroll_pane(f, power_area, app, Pane::Power, "Power", power);
    let health = ports::health_lines(&app.findings, health_area.width.saturating_sub(2) as usize);
    scroll_pane(f, health_area, app, Pane::Health, "Health", health);
}

/// A pane with a selected row, kept in view; rows in `flash` changed just now.
fn list_pane(
    f: &mut Frame,
    area: Rect,
    app: &mut App,
    pane: Pane,
    title: &str,
    mut lines: Vec<Line<'static>>,
    flash: &[usize],
) {
    let block = block(app, pane, title);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let height = inner.height as usize;
    let focused = app.focus() == pane;
    let state = app.pane_mut(pane);
    state.height = height;
    if state.sel < state.offset {
        state.offset = state.sel;
    } else if height > 0 && state.sel >= state.offset + height {
        state.offset = state.sel + 1 - height;
    }
    state.offset = state.offset.min(lines.len().saturating_sub(height));
    let (sel, offset) = (state.sel, state.offset);
    for &i in flash {
        if let Some(line) = lines.get_mut(i) {
            line.style = Style::new().fg(Color::Black).bg(Color::Yellow);
        }
    }
    if focused && let Some(line) = lines.get_mut(sel) {
        line.style = line.style.add_modifier(Modifier::REVERSED);
    }
    let shown: Vec<Line> = lines.into_iter().skip(offset).take(height).collect();
    f.render_widget(Paragraph::new(shown), inner);
}

/// A pane that only scrolls.
fn scroll_pane(
    f: &mut Frame,
    area: Rect,
    app: &mut App,
    pane: Pane,
    title: &str,
    lines: Vec<Line<'static>>,
) {
    let block = block(app, pane, title);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let height = inner.height as usize;
    let state = app.pane_mut(pane);
    state.height = height;
    state.offset = state.offset.min(lines.len().saturating_sub(height));
    let shown: Vec<Line> = lines.into_iter().skip(state.offset).take(height).collect();
    f.render_widget(Paragraph::new(shown), inner);
}

/// `HH:MM:SS  + text`, coloured by kind.
fn event_line(e: &Event, app: &App) -> Line<'static> {
    let style = match e.kind {
        EventKind::Added => Style::new().fg(Color::Green),
        EventKind::Removed => Style::new().fg(Color::Red),
        EventKind::Changed => Style::new().fg(Color::Yellow),
        EventKind::Problem => Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
    };
    Line::from(vec![
        format!("{}  ", clock(e.t_ms, app.utc_offset_s)).dark_gray(),
        Span::styled(format!("{} {}", e.kind.sign(), e.text), style),
    ])
}
