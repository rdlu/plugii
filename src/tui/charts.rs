//! The Charts tab: power and CPU on one time axis, with plug markers; below
//! them the link-speed strips on the same axis, URB/s sparklines and the
//! charger table ([`activity`]).

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Chart, Dataset, GraphType, LegendPosition};

use super::activity;
use super::app::{App, Pane};
use super::ui::block;
use crate::diff::EventKind;
use crate::history::Sample;

/// A legend name, a colour and the points.
type Series = (&'static str, Color, Vec<(f64, f64)>);

/// The longest window shown: the whole history.
const MAX_WINDOW_S: u64 = 600;
/// Rows the two charts keep however many sockets and chargers there are.
const CHARTS_MIN: u16 = 16;
/// The fewest rows the link, URB and charger panes take when all three
/// show: one row each, plus the table's header.
const LOWER_MIN: [u16; 3] = [3, 3, 4];
/// The widest the label column gets for socket names.
const LABELS_MAX: usize = 18;

/// The time axis every pane on the tab shares.
#[derive(Debug, Clone, Copy)]
pub struct TimeAxis {
    /// Unix ms at the right edge.
    pub now_ms: u64,
    pub window_s: u64,
    /// Columns left of the axis line, inside the border: the y labels or
    /// the socket names.
    pub labels: u16,
}

impl TimeAxis {
    /// Columns left of the plot inside a pane `inner_width` wide, as
    /// ratatui's `Chart` lays them out: the labels (at most a third of the
    /// width) and the axis line.
    pub fn plot_left(&self, inner_width: u16) -> u16 {
        self.labels.min(inner_width / 3) + 1
    }
}

pub fn draw(f: &mut Frame, area: Rect, app: &mut App) {
    let [power_area, cpu_area, links_area, urbs_area, chargers_area] = areas(area, app);
    let now = app.view_now_ms();
    let window_s = window_s(app, now);
    let start_ms = now.saturating_sub(window_s * 1000);
    let samples: Vec<&Sample> = app
        .history
        .since(start_ms)
        .filter(|s| s.t_ms <= now)
        .collect();
    let x = |t_ms: u64| (t_ms as f64 - now as f64) / 1000.0;
    let series = |get: fn(&Sample) -> Option<f32>| -> Vec<(f64, f64)> {
        samples
            .iter()
            .filter_map(|s| Some((x(s.t_ms), f64::from(get(s)?))))
            .collect()
    };
    // Added / Removed event times, as vertical lines.
    let marks: Vec<(f64, Color)> = app
        .events
        .iter()
        .filter(|e| e.t_ms >= start_ms && e.t_ms <= now)
        .filter_map(|e| match e.kind {
            EventKind::Added => Some((x(e.t_ms), Color::Green)),
            EventKind::Removed => Some((x(e.t_ms), Color::Red)),
            _ => None,
        })
        .collect();

    // Power: PD contract, battery flow, CPU package, in W.
    let power = [
        ("USB-C PD", Color::Cyan, series(|s| s.pd_w)),
        ("battery", Color::Yellow, series(|s| s.battery_w)),
        ("CPU package", Color::Magenta, series(|s| s.cpu_package_w)),
    ];
    let values = power.iter().flat_map(|(_, _, d)| d.iter().map(|p| p.1));
    let lo = values.clone().fold(0.0, f64::min);
    let hi = values.fold(0.0, f64::max);
    let (lo, hi) = (
        (lo * 1.1 / 5.0).floor() * 5.0,
        ((hi * 1.1 / 5.0).ceil() * 5.0).max(5.0),
    );
    let power_labels = [
        format!("{lo:.0} W"),
        format!("{:.0} W", (lo + hi) / 2.0),
        format!("{hi:.0} W"),
    ];

    // CPU: clock on the axis; temperature scaled so its top is the axis top.
    let ghz = series(|s| s.cpu_ghz);
    let temp = series(|s| s.cpu_temp_c);
    let ghz_top = (ghz.iter().map(|p| p.1).fold(1.0, f64::max) * 1.1 * 2.0).ceil() / 2.0;
    let temp_top = (temp.iter().map(|p| p.1).fold(100.0, f64::max) / 10.0).ceil() * 10.0;
    let cpu_label = |frac: f64| format!("{:.1} GHz {:>3.0} °C", ghz_top * frac, temp_top * frac);
    let cpu_labels = [cpu_label(0.0), cpu_label(0.5), cpu_label(1.0)];

    // One label column for every pane, so their time axes line up.
    let names = app
        .stats
        .sockets
        .iter()
        .map(|s| Span::raw(s.name.as_str()).width())
        .max()
        .unwrap_or(0)
        .min(if app.narrow() { 0 } else { LABELS_MAX });
    let labels = power_labels
        .iter()
        .chain(&cpu_labels)
        .map(|l| Span::raw(l.as_str()).width())
        .fold(names, usize::max) as u16;
    let axis = TimeAxis {
        now_ms: now,
        window_s,
        labels,
    };
    let y_labels = |labels: [String; 3]| {
        labels.map(|l| Span::raw(format!("{l:>w$}", w = axis.labels as usize)))
    };
    let x_axis = || {
        Axis::default()
            .bounds([-(window_s as f64), 0.0])
            .labels(x_labels(window_s))
            .style(Style::new().fg(Color::DarkGray))
    };

    let power_marks = mark_lines(&marks, lo, hi);
    let mut datasets: Vec<Dataset> = marker_sets(&power_marks);
    datasets.extend(named_sets(&power));
    let chart = Chart::new(datasets)
        .block(block(app, Pane::PowerChart, "Power (W)"))
        .x_axis(x_axis())
        .y_axis(
            Axis::default()
                .bounds([lo, hi])
                .labels(y_labels(power_labels))
                .labels_alignment(Alignment::Right)
                .style(Style::new().fg(Color::DarkGray)),
        )
        .legend_position(Some(LegendPosition::TopLeft))
        .hidden_legend_constraints((Constraint::Ratio(1, 1), Constraint::Ratio(1, 1)));
    f.render_widget(chart, power_area);

    let temp_scaled: Vec<(f64, f64)> = temp
        .iter()
        .map(|&(t, c)| (t, c / temp_top * ghz_top))
        .collect();
    let cpu = [
        ("avg GHz", Color::Blue, ghz),
        ("temp °C", Color::Red, temp_scaled),
    ];
    let cpu_marks = mark_lines(&marks, 0.0, ghz_top);
    let mut datasets: Vec<Dataset> = marker_sets(&cpu_marks);
    datasets.extend(named_sets(&cpu));
    let chart = Chart::new(datasets)
        .block(block(app, Pane::CpuChart, "CPU (GHz, °C)"))
        .x_axis(x_axis())
        .y_axis(
            Axis::default()
                .bounds([0.0, ghz_top])
                .labels(y_labels(cpu_labels))
                .labels_alignment(Alignment::Right)
                .style(Style::new().fg(Color::DarkGray)),
        )
        .legend_position(Some(LegendPosition::TopLeft))
        .hidden_legend_constraints((Constraint::Ratio(1, 1), Constraint::Ratio(1, 1)));
    f.render_widget(chart, cpu_area);

    activity::links(f, links_area, app, &axis);
    activity::urbs(f, urbs_area, app, &axis);
    activity::chargers(f, chargers_area, app);
    let lower = [links_area, urbs_area, chargers_area];
    if let [shown] = lower.iter().filter(|a| a.height > 0).collect::<Vec<_>>()[..] {
        // One pane at a time: say how to reach the others.
        let hint = Line::from(" ^W j/k: link speed · URB/s · chargers ".dark_gray());
        let row = Rect::new(
            shown.x + 1,
            shown.bottom() - 1,
            shown.width.saturating_sub(2),
            1,
        );
        f.render_widget(hint.right_aligned(), row);
    }
}

/// The power and CPU charts on top, the link strips, URB/s and charger
/// panes below at the height their rows want. When that leaves the charts
/// too little, the panes below shrink (and scroll) first, down to one row
/// each; below that only one of them shows.
fn areas(area: Rect, app: &App) -> [Rect; 5] {
    let sockets = app.stats.sockets.len().max(1) as u16;
    let chargers = app.stats.chargers.len().max(1) as u16;
    let mut lower = [sockets + 2, sockets + 2, chargers + 3];
    let want: u16 = lower.iter().sum();
    let budget = want.min(area.height.saturating_sub(CHARTS_MIN));
    if budget >= LOWER_MIN.iter().sum() {
        // Take rows from the tallest pane until they fit.
        while lower.iter().sum::<u16>() > budget {
            let i = (0..3)
                .filter(|&i| lower[i] > LOWER_MIN[i])
                .max_by_key(|&i| (lower[i], std::cmp::Reverse(i)))
                .expect("the minimums fit");
            lower[i] -= 1;
        }
    } else {
        // Too short for all three: the focused one (else the link strips)
        // gets the rows; `Ctrl-w j` / `k` page through them.
        let shown = match app.focus() {
            Pane::Urbs => 1,
            Pane::Chargers => 2,
            _ => 0,
        };
        lower = [0; 3];
        if budget >= LOWER_MIN[shown] {
            lower[shown] = budget;
        }
    }
    let charts = area.height.saturating_sub(lower.iter().sum());
    let power = charts * 11 / 20;
    let [a, b, c, d, e] = Layout::vertical([
        Constraint::Length(power),
        Constraint::Length(charts - power),
        Constraint::Length(lower[0]),
        Constraint::Length(lower[1]),
        Constraint::Length(lower[2]),
    ])
    .areas(area);
    [a, b, c, d, e]
}

/// The history's span rounded up to a minute, between 1 and 10 minutes.
fn window_s(app: &App, now: u64) -> u64 {
    let span_ms = app
        .history
        .iter()
        .next()
        .map_or(0, |s| now.saturating_sub(s.t_ms));
    span_ms.div_ceil(60_000).clamp(1, MAX_WINDOW_S / 60) * 60
}

fn x_labels(window_s: u64) -> Vec<Span<'static>> {
    let at = |s: u64| {
        if window_s >= 120 {
            format!("−{}m", s / 60)
        } else {
            format!("−{s}s")
        }
    };
    vec![
        Span::raw(at(window_s)),
        Span::raw(at(window_s / 2)),
        Span::raw("now"),
    ]
}

fn mark_lines(marks: &[(f64, Color)], lo: f64, hi: f64) -> Vec<(Vec<(f64, f64)>, Color)> {
    marks
        .iter()
        .map(|&(x, color)| (vec![(x, lo), (x, hi)], color))
        .collect()
}

fn marker_sets(lines: &[(Vec<(f64, f64)>, Color)]) -> Vec<Dataset<'_>> {
    lines
        .iter()
        .map(|(data, color)| {
            Dataset::default()
                .data(data)
                .graph_type(GraphType::Line)
                .marker(Marker::Braille)
                .style(Style::new().fg(*color).dim())
        })
        .collect()
}

/// A line per series with data, named for the legend.
fn named_sets(series: &[Series]) -> Vec<Dataset<'_>> {
    series
        .iter()
        .filter(|(_, _, data)| !data.is_empty())
        .map(|(name, color, data)| {
            Dataset::default()
                .name(name.fg(*color))
                .data(data)
                .graph_type(GraphType::Line)
                .marker(Marker::Braille)
                .style(Style::new().fg(*color))
        })
        .collect()
}

/// `text` cut to `width` columns with an ellipsis, padded to `width`.
pub fn fit(text: &str, width: usize) -> String {
    if Line::from(text).width() <= width {
        return format!("{text:<width$}");
    }
    let mut out: String = text.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    format!("{out:<width$}")
}
