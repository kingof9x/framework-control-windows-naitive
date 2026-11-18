use eframe::egui::{self, Ui};
use egui_plot::{Corner, Legend, Line, MarkerShape, Plot, PlotBounds, PlotPoint, PlotPoints, Points, VLine};

use crate::api::{BatterySample, CurveConfig, TelemetrySample};
use crate::theme;

fn minutes_ago(ts: i64, now: i64) -> f64 {
    (ts - now) as f64 / 60_000.0
}

fn time_plot(id: &str, height: f32) -> Plot<'static> {
    Plot::new(id.to_string())
        .height(height)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .show_background(false)
        .y_axis_min_width(36.0)
        .include_x(-5.0)
        .include_x(0.0)
        .legend(Legend::default().position(Corner::LeftTop))
        .x_axis_formatter(|m, _| {
            if m.value.abs() < 0.01 {
                "now".into()
            } else {
                format!("{:.0}m", m.value)
            }
        })
}

/// Temperature history. `only` limits the series shown (empty = all).
pub fn temp_plot(ui: &mut Ui, samples: &[TelemetrySample], only: &[String], height: f32) {
    let pal = theme::colors(ui.ctx());
    let now = samples.last().map(|s| s.ts_ms).unwrap_or(0);
    let names: Vec<String> = samples.last().map(|s| s.temps.keys().cloned().collect()).unwrap_or_default();
    time_plot("temps", height)
        .y_axis_formatter(|m, _| format!("{:.0}°", m.value))
        .include_y(30.0)
        .include_y(90.0)
        .show(ui, |pui| {
            for (i, name) in names.iter().enumerate() {
                if !only.is_empty() && !only.contains(name) {
                    continue;
                }
                let pts: PlotPoints = samples
                    .iter()
                    .filter_map(|s| s.temps.get(name).map(|v| [minutes_ago(s.ts_ms, now), *v as f64]))
                    .collect();
                pui.line(Line::new(pts).name(name).color(pal.series(i)).width(1.8_f32));
            }
        });
}

pub fn rpm_plot(ui: &mut Ui, samples: &[TelemetrySample], height: f32) {
    let pal = theme::colors(ui.ctx());
    let now = samples.last().map(|s| s.ts_ms).unwrap_or(0);
    let fans = samples.last().map(|s| s.rpms.len()).unwrap_or(0);
    time_plot("rpm", height)
        .y_grid_spacer(egui_plot::uniform_grid_spacer(|_| [500.0, 1000.0, 2000.0]))
        .y_axis_formatter(|m, _| {
            if m.value >= 1000.0 {
                format!("{:.0}k", m.value / 1000.0)
            } else {
                format!("{:.0}", m.value)
            }
        })
        .include_y(0.0)
        .include_y(6000.0)
        .show(ui, |pui| {
            for f in 0..fans {
                let pts: PlotPoints = samples
                    .iter()
                    .filter_map(|s| s.rpms.get(f).map(|v| [minutes_ago(s.ts_ms, now), *v as f64]))
                    .collect();
                pui.line(Line::new(pts).name(format!("Fan {}", f + 1)).color(pal.series(f + 1)).width(1.8_f32));
            }
        });
}

pub fn battery_plot(ui: &mut Ui, samples: &[BatterySample]) {
    let pal = theme::colors(ui.ctx());
    let now = samples.last().map(|s| s.ts_ms).unwrap_or(0);
    let hrs = |ts: i64| (ts - now) as f64 / 3_600_000.0;
    Plot::new("battery_hist")
        .height(200.0)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .show_background(false)
        .y_axis_min_width(36.0)
        .legend(Legend::default().position(Corner::LeftTop))
        .include_y(0.0)
        .include_y(100.0)
        .include_x(-1.0)
        .include_x(0.0)
        .x_axis_formatter(|m, _| if m.value.abs() < 0.01 { "now".into() } else { format!("{:.0}h", m.value) })
        .show(ui, |pui| {
            let pct: PlotPoints =
                samples.iter().filter_map(|s| s.charge_pct.map(|p| [hrs(s.ts_ms), p as f64])).collect();
            pui.line(Line::new(pct).name("Charge %").color(pal.series(2)).width(2.0_f32));
            let w: PlotPoints = samples.iter().filter_map(|s| s.watts.map(|p| [hrs(s.ts_ms), p as f64])).collect();
            pui.line(Line::new(w).name("Watts").color(pal.series(1)).width(1.2_f32));
        });
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
