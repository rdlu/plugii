//! The Charts tab's lower panes: a link-speed strip per socket on the
//! charts' time axis, URB/s sparklines, and the charger table.

use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, RenderDirection, Row, Sparkline, Table};

use super::app::{App, Pane};
use super::charts::{TimeAxis, fit};
use super::stats::{ChargerRow, Link, Negotiated, SocketStats};
use super::ui::block;
use crate::health::milli;

/// Columns for the current rate at the end of a sparkline row.
const RATE_W: u16 = 8;

/// One row per socket: a cell per time bucket, coloured by the link at the
/// bucket's end, under the charts' x axis.
pub fn links(f: &mut Frame, area: Rect, app: &mut App, axis: &TimeAxis) {
    if area.height == 0 {
        return;
    }
    let legend = Line::from(vec![
        " ".into(),
        "·".dark_gray(),
        " none  ".into(),
        "▄".yellow(),
        " USB 2  ".into(),
        "█".green(),
        " 5G+  ".into(),
        "▒".red(),
        " hub stuck at USB 2 ".into(),
    ])
    .right_aligned();
    let block = block(app, Pane::Links, "Link speed").title_top(legend);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let left = axis.plot_left(inner.width);
    let cells = inner.width.saturating_sub(left) as u64;
    let window_ms = axis.window_s * 1000;
    let start = axis.now_ms.saturating_sub(window_ms);
    let lines: Vec<Line> = app
        .stats
        .sockets
        .iter()
        .map(|s| {
            let mut spans = name_spans(s, left);
            spans.extend((0..cells).map(|i| {
                let end = start + window_ms * (i + 1) / cells;
                link_cell(s.link_at(end))
            }));
            Line::from(spans)
        })
        .collect();
    let lines = or_none(lines);
    let offset = scroll(app, Pane::Links, lines.len(), inner.height);
    let shown: Vec<Line> = lines.into_iter().skip(offset).collect();
    f.render_widget(Paragraph::new(shown), inner);
}

/// One row per socket: the last rates as a sparkline, newest on the right,
/// and the current rate.
pub fn urbs(f: &mut Frame, area: Rect, app: &mut App, axis: &TimeAxis) {
    if area.height == 0 {
        return;
    }
    let note = Line::from(" a bar per scan, newest on the right ".dark_gray()).right_aligned();
    let block = block(app, Pane::Urbs, "URB/s").title_top(note);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let sockets = &app.stats.sockets;
    if sockets.is_empty() {
        f.render_widget(Paragraph::new(or_none(Vec::new())), inner);
        return;
    }
    let left = axis.plot_left(inner.width);
    let offset = scroll(app, Pane::Urbs, app.stats.sockets.len(), inner.height);
    let spark_w = inner.width.saturating_sub(left + RATE_W);
    for (row, s) in app
        .stats
        .sockets
        .iter()
        .skip(offset)
        .take(inner.height as usize)
        .enumerate()
    {
        let y = inner.y + row as u16;
        let line = Rect::new(inner.x, y, inner.width, 1);
        f.render_widget(Paragraph::new(Line::from(name_spans(s, left))), line);
        let data: Vec<u64> = s
            .rates
            .iter()
            .rev()
            .take(spark_w as usize)
            .map(|r| r.round() as u64)
            .collect();
        let max = data.iter().copied().max().unwrap_or(0).max(1);
        let spark = Sparkline::default()
            .data(&data)
            .max(max)
            .direction(RenderDirection::RightToLeft)
            .style(Style::new().fg(Color::Cyan));
        f.render_widget(spark, Rect::new(inner.x + left, y, spark_w, 1));
        let rate = s.rate().map_or("—".into(), rate_text);
        f.render_widget(
            Paragraph::new(rate).right_aligned(),
            Rect::new(inner.x + left + spark_w, y, inner.width - left - spark_w, 1),
        );
    }
}

/// The chargers seen this session, the one charging now marked.
pub fn chargers(f: &mut Frame, area: Rect, app: &mut App) {
    if area.height == 0 {
        return;
    }
    let block = block(app, Pane::Chargers, "Chargers this session");
    let inner = block.inner(area);
    f.render_widget(block, area);
    if app.stats.chargers.is_empty() {
        f.render_widget(
            Paragraph::new("nothing has charged the machine yet".dark_gray()),
            inner,
        );
        return;
    }
    let offset = scroll(
        app,
        Pane::Chargers,
        app.stats.chargers.len(),
        inner.height.saturating_sub(1),
    );
    let active = app.stats.active;
    let port_w = app
        .stats
        .chargers
        .iter()
        .map(|c| Line::from(c.port_label.as_str()).width())
        .fold(4, usize::max) as u16;
    let rows: Vec<Row> = app
        .stats
        .chargers
        .iter()
        .enumerate()
        .skip(offset)
        .map(|(i, c)| charger_row(c, active == Some(i)))
        .collect();
    let header = Row::new([
        "",
        "Charger",
        "Port",
        "Negotiated",
        "CPU max",
        "Pkg max",
        "Resets",
        "Charging",
    ])
    .style(Style::new().add_modifier(Modifier::BOLD));
    let widths = [
        Constraint::Length(1),
        Constraint::Length(12),
        Constraint::Max(port_w),
        Constraint::Length(14),
        Constraint::Length(7),
        Constraint::Length(7),
        Constraint::Length(6),
        Constraint::Length(10),
    ];
    f.render_widget(
        Table::new(rows, widths).header(header).column_spacing(1),
        inner,
    );
}

fn charger_row(c: &ChargerRow, active: bool) -> Row<'static> {
    let charger = match c.best_w {
        Some(w) => format!("{w:.0} W"),
        None => "not reported".into(),
    };
    let negotiated = match c.negotiated {
        Negotiated::Watts(w) => format!("{w:.0} W"),
        Negotiated::Drawing { ma } => format!("drawing {} A", milli(ma)),
        Negotiated::Unknown => "—".into(),
    };
    let mut time = duration(c.charging_ms);
    if c.sessions > 1 {
        time.push_str(&format!(" ×{}", c.sessions));
    }
    let marker = if active { "●".green() } else { "".into() };
    Row::new([
        Cell::from(marker),
        Cell::from(charger),
        Cell::from(c.port_label.clone()),
        Cell::from(negotiated),
        Cell::from(c.max_ghz.map_or("—".into(), |g| format!("{g:.1} GHz"))),
        Cell::from(c.max_package_w.map_or("—".into(), |w| format!("{w:.0} W"))),
        Cell::from(c.link_resets.to_string()),
        Cell::from(time),
    ])
}

/// The socket's name in the label column, then the axis line.
fn name_spans(s: &SocketStats, left: u16) -> Vec<Span<'static>> {
    let width = left.saturating_sub(1) as usize;
    vec![
        Span::raw(fit(&s.name, width)),
        Span::styled("│", Style::new().fg(Color::DarkGray)),
    ]
}

fn link_cell(link: Option<Link>) -> Span<'static> {
    match link {
        None => " ".into(),
        Some(Link::None) => "·".dark_gray(),
        Some(Link::Usb2) => "▄".yellow(),
        Some(Link::SuperSpeed) => "█".green(),
        Some(Link::Stuck) => "▒".red(),
    }
}

fn or_none(lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    if lines.is_empty() {
        vec![Line::from("no hotplug USB ports found".dark_gray())]
    } else {
        lines
    }
}

/// Records the pane's height and keeps its offset within `len` rows;
/// returns the offset.
fn scroll(app: &mut App, pane: Pane, len: usize, height: u16) -> usize {
    let state = app.pane_mut(pane);
    state.height = height as usize;
    state.offset = state.offset.min(len.saturating_sub(height as usize));
    state.offset
}

/// `0/s`, `530/s`, `1.2k/s`, `3.4M/s`.
pub fn rate_text(rate: f64) -> String {
    if rate < 999.5 {
        format!("{rate:.0}/s")
    } else if rate < 999_950.0 {
        format!("{:.1}k/s", rate / 1e3)
    } else {
        format!("{:.1}M/s", rate / 1e6)
    }
}

/// `35s`, `4m 05s`, `2h 10m`.
pub fn duration(ms: u64) -> String {
    let s = ms / 1000;
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m {:02}s", s / 60, s % 60),
        _ => format!("{}h {:02}m", s / 3600, s % 3600 / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_and_durations() {
        assert_eq!(rate_text(0.0), "0/s");
        assert_eq!(rate_text(530.4), "530/s");
        assert_eq!(rate_text(1234.0), "1.2k/s");
        assert_eq!(rate_text(3_400_000.0), "3.4M/s");
        assert_eq!(duration(35_400), "35s");
        assert_eq!(duration(245_000), "4m 05s");
        assert_eq!(duration(7_800_000), "2h 10m");
    }
}
