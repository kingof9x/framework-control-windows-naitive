use eframe::egui::{self, Ui};
use egui_plot::{Corner, Legend, Line, MarkerShape, Plot, PlotBounds, PlotPoint, PlotPoints, Points, VLine};

use crate::api::{BatterySample, CurveConfig, TelemetrySample};
use crate::theme;

fn minutes_ago(ts: i64, now: i64) -> f64 {
    (ts - now) as f64 / 60_000.0
}


/// A live marker on the curve editor: current temperature and, when the fans
/// are calibrated, the duty they are actually running at.
pub struct Probe {
    pub label: String,
    pub temp: f64,
    pub duty: Option<f64>,
}

/// Interactive fan curve editor. Returns true when the curve changed.
pub fn curve_editor(ui: &mut Ui, id: &str, curve: &mut CurveConfig, probes: &[Probe], drag: &mut Option<usize>) -> bool {
    let pal = theme::colors(ui.ctx());
    let accent = theme::colors(ui.ctx()).accent;
    let live = pal.series(2);
    let mut changed = false;
    let pts_snapshot = curve.points.clone();

    let res = Plot::new(id.to_string())
        .height(260.0)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .allow_double_click_reset(false)
        .show_background(false)
        .legend(Legend::default().position(Corner::LeftTop))
        .x_axis_formatter(|m, _| format!("{:.0}°C", m.value))
        .y_axis_formatter(|m, _| format!("{:.0}%", m.value))
        .label_formatter(|_, p| format!("{:.0}°C → {:.0}%", p.x, p.y))
        .show(ui, |pui| {
            pui.set_plot_bounds(PlotBounds::from_min_max([20.0, -2.0], [100.0, 105.0]));
            let mut line: Vec<[f64; 2]> = vec![[20.0, pts_snapshot.first().map(|p| p[1]).unwrap_or(0) as f64]];
            line.extend(pts_snapshot.iter().map(|p| [p[0] as f64, p[1] as f64]));
            line.push([100.0, pts_snapshot.last().map(|p| p[1]).unwrap_or(100) as f64]);
            pui.line(Line::new(PlotPoints::from(line)).color(accent).width(2.5_f32).fill(0.0_f32));
            pui.points(
                Points::new(PlotPoints::from(
                    pts_snapshot.iter().map(|p| [p[0] as f64, p[1] as f64]).collect::<Vec<_>>(),
                ))
                .radius(6.0_f32)
                .color(accent),
            );
            for (i, p) in probes.iter().enumerate() {
                let c = if probes.len() > 1 { pal.series(i + 2) } else { live };
                match p.duty {
                    Some(d) => pui.points(
                        Points::new(vec![[p.temp, d]])
                            .shape(MarkerShape::Diamond)
                            .radius(7.0_f32)
                            .color(c)
                            .name(format!("{} {:.0}°C · {:.0}%", p.label, p.temp, d)),
                    ),
                    None => pui.vline(VLine::new(p.temp).color(c).width(1.5_f32).name(format!("{} {:.0}°C", p.label, p.temp))),
                }
            }

            // Hit-test in screen space so the grab radius is constant.
            let pointer = pui.pointer_coordinate();
            let near = pointer.and_then(|ptr| {
                let ps = pui.screen_from_plot(ptr);
                pts_snapshot
                    .iter()
                    .enumerate()
                    .map(|(i, p)| (i, pui.screen_from_plot(PlotPoint::new(p[0] as f64, p[1] as f64)).distance(ps)))
                    .filter(|(_, d)| *d < 12.0)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(i, _)| i)
            });
            let r = pui.response();
            (pointer, near, r.drag_started(), r.dragged(), r.drag_stopped(), r.clicked(), r.secondary_clicked())
        });

    let (pointer, near, started, dragging, stopped, clicked, secondary) = res.inner;
    if near.is_some() || drag.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    if started {
        *drag = near;
    }
    if let (true, Some(i), Some(p)) = (dragging, *drag, pointer) {
        if i < curve.points.len() {
            let lo = if i == 0 { 20 } else { curve.points[i - 1][0] + 1 };
            let hi = if i + 1 == curve.points.len() { 100 } else { curve.points[i + 1][0] - 1 };
            let t = (p.x.round() as i64).clamp(lo as i64, hi.max(lo) as i64) as u32;
            let d = p.y.round().clamp(0.0, 100.0) as u32;
            if curve.points[i] != [t, d] {
                curve.points[i] = [t, d];
                changed = true;
            }
        }
    }
    if stopped {
        *drag = None;
    }
    if clicked && near.is_none() {
        if let Some(p) = pointer {
            let t = p.x.round().clamp(20.0, 100.0) as u32;
            if !curve.points.iter().any(|q| q[0] == t) {
                curve.points.push([t, p.y.round().clamp(0.0, 100.0) as u32]);
                curve.points.sort_by_key(|q| q[0]);
                changed = true;
            }
        }
    }
    if secondary && curve.points.len() > 2 {
        if let Some(i) = near {
            curve.points.remove(i);
            changed = true;
        }
    }
    changed
}

// ----------------------------------------------------------- clean charts ---

/// A readable name for a sensor as framework_tool reports it.
pub fn sensor_label(raw: &str) -> String {
    let known = match raw {
        "APU" => Some("Processor"),
        "F75303_CPU" | "F57397_CPU" | "CPU" => Some("Near the processor"),
        "F75303_DDR" | "F57397_DDR" | "DDR" => Some("Memory"),
        "F75303_Local" | "F57397_Local" | "Local" => Some("Inside the case"),
        "F75303_VCCGT" | "F57397_VCCGT" => Some("Graphics power"),
        "dGPU" | "dGPU temp" => Some("Graphics chip"),
        "dGPU VR" => Some("Graphics power"),
        "dGPU VRAM" => Some("Graphics memory"),
        "dGPU AMB" => Some("Graphics module"),
        "Battery" => Some("Battery"),
        "Ambient" => Some("Ambient"),
        _ => None,
    };
    if let Some(k) = known {
        return k.to_string();
    }
    // Unknown: drop a sensor-chip prefix like "F75303_" and tidy the rest.
    let s = raw.split_once('_').filter(|(chip, _)| chip.chars().any(|c| c.is_ascii_digit())).map(|(_, rest)| rest).unwrap_or(raw);
    s.replace('_', " ")
}

/// Which group a sensor belongs in for the grouped charts.
pub fn sensor_group(raw: &str) -> &'static str {
    if raw.starts_with("dGPU") || raw.contains("VCCGT") {
        "Graphics"
    } else if raw == "APU" || raw.ends_with("CPU") {
        "Processor"
    } else {
        "Memory and case"
    }
}

/// Sensors worth showing: everything with a real reading now (a switched-off graphics module reads 0 °C),
/// processor first.
pub fn active_sensors(samples: &[TelemetrySample]) -> Vec<String> {
    let Some(last) = samples.last() else { return Vec::new() };
    let rank = |n: &str| match sensor_group(n) {
        "Processor" => 0,
        "Graphics" => 2,
        _ => 1,
    };
    let mut names: Vec<String> = last.temps.iter().filter(|(_, v)| **v > 0).map(|(k, _)| k.clone()).collect();
    names.sort_by_key(|n| (rank(n), if n == "APU" { 0 } else { 1 }, n.clone()));
    names
}

pub fn series_of(samples: &[TelemetrySample], name: &str) -> Vec<[f64; 2]> {
    let now = samples.last().map(|s| s.ts_ms).unwrap_or(0);
    samples
        .iter()
        .filter_map(|s| s.temps.get(name).filter(|v| **v > 0).map(|v| [minutes_ago(s.ts_ms, now), *v as f64]))
        .collect()
}

pub fn fan_series(samples: &[TelemetrySample], fan: usize) -> Vec<[f64; 2]> {
    let now = samples.last().map(|s| s.ts_ms).unwrap_or(0);
    samples.iter().filter_map(|s| s.rpms.get(fan).map(|v| [minutes_ago(s.ts_ms, now), *v as f64])).collect()
}

/// A chart without legend box or vertical grid lines; each line is labelled at its right end.
pub fn clean_chart(
    ui: &mut Ui,
    id: &str,
    series: &[(String, Vec<[f64; 2]>, egui::Color32)],
    height: f32,
    unit: &'static str,
    fill: bool,
) {
    clean_chart_span(ui, id, series, height, unit, fill, Span::Minutes);
}

/// The time axis of a chart: x values are minutes or hours before now (negative).
#[derive(Clone, Copy, PartialEq)]
pub enum Span {
    Minutes,
    Hours,
}

pub fn clean_chart_span(
    ui: &mut Ui,
    id: &str,
    series: &[(String, Vec<[f64; 2]>, egui::Color32)],
    height: f32,
    unit: &'static str,
    fill: bool,
    span: Span,
) {
    let pal = theme::colors(ui.ctx());
    let (lo, hi) = series
        .iter()
        .flat_map(|(_, p, _)| p.iter().map(|q| q[1]))
        .fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
    let (lo, hi) = if lo > hi { (0.0, 1.0) } else { (lo, hi) };
    let pad = ((hi - lo) * 0.2).max(2.0);
    // Readings that can't go below zero keep zero as the floor; signed ones (watts) don't.
    let floor = if lo >= 0.0 { (lo - pad).max(0.0) } else { lo - pad };
    let earliest = series.iter().filter_map(|(_, p, _)| p.first().map(|q| q[0])).fold(0.0_f64, f64::min);
    let (start, label_room) = match span {
        // At least the last 10 minutes; more if the service has more history.
        Span::Minutes => ((-10.0_f64).min(earliest), 2.4),
        Span::Hours => (earliest.floor().min(-1.0), (-earliest).max(1.0) * 0.24),
    };
    let steps = match unit {
        "" => [500.0, 1000.0, 2000.0],
        "%" => [10.0, 20.0, 50.0],
        " W" => [5.0, 10.0, 20.0],
        _ => [10.0, 20.0, 50.0],
    };
    Plot::new(id.to_string())
        .height(height)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .show_background(false)
        .show_grid([false, true])
        .y_axis_min_width(36.0)
        .include_x(start)
        .include_x(label_room) // room for the end labels
        .include_y(floor)
        .include_y(hi + pad)
        .set_margin_fraction(egui::vec2(0.0, 0.05))
        .x_axis_formatter(move |m, _| {
            if m.value > 0.01 {
                String::new()
            } else if m.value.abs() < 0.01 {
                "now".into()
            } else if span == Span::Hours {
                format!("{:.0} h", -m.value)
            } else {
                format!("{:.0} min", -m.value)
            }
        })
        .y_grid_spacer(egui_plot::uniform_grid_spacer(move |_| steps))
        .y_axis_formatter(move |m, _| format!("{:.0}{unit}", m.value))
        .label_formatter(move |name, p| if name.is_empty() { String::new() } else { format!("{name}: {:.0}{unit}", p.y) })
        .show(ui, |pui| {
            // End labels, nudged apart vertically so close lines don't print on top of each other.
            let gap = ((hi + pad) - floor) * 0.14;
            let mut ends: Vec<(usize, f64)> =
                series.iter().enumerate().filter_map(|(i, (_, p, _))| p.last().map(|l| (i, l[1]))).collect();
            ends.sort_by(|a, b| b.1.total_cmp(&a.1));
            let mut placed: Vec<(usize, f64)> = Vec::new();
            for (i, y) in ends {
                let y = placed.last().map_or(y, |(_, prev)| y.min(prev - gap));
                placed.push((i, y));
            }
            for (i, (label, pts, color)) in series.iter().enumerate() {
                let Some(last) = pts.last().copied() else { continue };
                let mut line = Line::new(PlotPoints::from(pts.clone())).name(label).color(*color).width(2.0_f32);
                if fill && series.len() == 1 {
                    line = line.fill(floor as f32);
                }
                pui.line(line);
                let y = placed.iter().find(|(j, _)| *j == i).map_or(last[1], |(_, y)| *y);
                pui.text(
                    egui_plot::Text::new(
                        PlotPoint::new(last[0] + label_room * 0.06, y),
                        egui::RichText::new(format!("{label} {:.0}{unit}", last[1])).size(12.0).color(*color),
                    )
                    .anchor(egui::Align2::LEFT_CENTER),
                );
            }
        });
    let _ = pal;
}

/// Fan speeds as a clean chart, one labelled line per fan.
pub fn fan_chart(ui: &mut Ui, id: &str, samples: &[TelemetrySample], height: f32) {
    let c = theme::colors(ui.ctx());
    let fans = samples.last().map(|s| s.rpms.len()).unwrap_or(0);
    let series: Vec<_> = (0..fans).map(|f| (format!("Fan {}", f + 1), fan_series(samples, f), c.series(f + 1))).collect();
    clean_chart(ui, id, &series, height, "", false);
}

fn battery_series(samples: &[BatterySample], value: impl Fn(&BatterySample) -> Option<f32>) -> Vec<[f64; 2]> {
    let now = samples.last().map(|s| s.ts_ms).unwrap_or(0);
    samples
        .iter()
        .filter_map(|s| value(s).map(|v| [(s.ts_ms - now) as f64 / 3_600_000.0, v as f64]))
        .collect()
}

pub fn battery_charge_chart(ui: &mut Ui, samples: &[BatterySample]) {
    let c = theme::colors(ui.ctx());
    let series = vec![("Charge".to_string(), battery_series(samples, |s| s.charge_pct), c.accent)];
    clean_chart_span(ui, "battery_charge", &series, 150.0, "%", true, Span::Hours);
}

pub fn battery_power_chart(ui: &mut Ui, samples: &[BatterySample]) {
    let c = theme::colors(ui.ctx());
    let series = vec![("Power".to_string(), battery_series(samples, |s| s.watts), c.series(1))];
    clean_chart_span(ui, "battery_power", &series, 130.0, " W", false, Span::Hours);
}
