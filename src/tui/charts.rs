//! The Charts tab: power and CPU on one time axis, with plug markers.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::symbols::Marker;
use ratatui::text::Span;
use ratatui::widgets::{Axis, Chart, Dataset, GraphType, LegendPosition};

use super::app::{App, Pane};
use super::ui::block;
use crate::diff::EventKind;
use crate::history::Sample;

/// A legend name, a colour and the points.
type Series = (&'static str, Color, Vec<(f64, f64)>);

/// The longest window shown: the whole history.
const MAX_WINDOW_S: u64 = 600;

pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    let [top, bottom] =
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(area);
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
    let x_axis = || {
        Axis::default()
            .bounds([-(window_s as f64), 0.0])
            .labels(x_labels(window_s))
            .style(Style::new().fg(Color::DarkGray))
    };

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
    let power_marks = mark_lines(&marks, lo, hi);
    let mut datasets: Vec<Dataset> = marker_sets(&power_marks);
    datasets.extend(named_sets(&power));
    let chart = Chart::new(datasets)
        .block(block(app, Pane::PowerChart, "Power (W)"))
        .x_axis(x_axis())
        .y_axis(
            Axis::default()
                .bounds([lo, hi])
                .labels([
                    Span::raw(format!("{lo:.0} W")),
                    Span::raw(format!("{:.0} W", (lo + hi) / 2.0)),
                    Span::raw(format!("{hi:.0} W")),
                ])
                .style(Style::new().fg(Color::DarkGray)),
        )
        .legend_position(Some(LegendPosition::TopLeft))
        .hidden_legend_constraints((Constraint::Ratio(1, 1), Constraint::Ratio(1, 1)));
    f.render_widget(chart, top);

    // CPU: clock on the axis; temperature scaled so its top is the axis top.
    let ghz = series(|s| s.cpu_ghz);
    let temp = series(|s| s.cpu_temp_c);
    let ghz_top = (ghz.iter().map(|p| p.1).fold(1.0, f64::max) * 1.1 * 2.0).ceil() / 2.0;
    let temp_top = (temp.iter().map(|p| p.1).fold(100.0, f64::max) / 10.0).ceil() * 10.0;
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
    let label = |frac: f64| {
        Span::raw(format!(
            "{:.1} GHz {:>3.0} °C",
            ghz_top * frac,
            temp_top * frac
        ))
    };
    let chart = Chart::new(datasets)
        .block(block(app, Pane::CpuChart, "CPU (GHz, °C)"))
        .x_axis(x_axis())
        .y_axis(
            Axis::default()
                .bounds([0.0, ghz_top])
                .labels([label(0.0), label(0.5), label(1.0)])
                .style(Style::new().fg(Color::DarkGray)),
        )
        .legend_position(Some(LegendPosition::TopLeft))
        .hidden_legend_constraints((Constraint::Ratio(1, 1), Constraint::Ratio(1, 1)));
    f.render_widget(chart, bottom);
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
