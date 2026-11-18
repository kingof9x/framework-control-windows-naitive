use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui::{self, RichText, Ui};

use crate::api::*;
use crate::app::App;
use crate::calibration::{Calibration, Outcome};
use crate::plots::{self, Probe};
use crate::state::Snapshot;
use crate::widgets::{self, card, hint, section_title, segmented};

const NAME_MAX: usize = 24;

#[derive(Default)]
pub struct FansUi {
    /// Selected fan tab: None = all fans (profile-wide settings).
    tab: Option<usize>,
    drag: Option<usize>,
    renaming: Option<(String, String)>,
    confirm_delete: bool,
    calibration: Option<Calibration>,
}

impl FansUi {
    pub fn select_tab(&mut self, tab: Option<usize>) {
        self.tab = tab;
    }
    pub fn open_calibration(&mut self) {
        self.calibration = Some(Calibration::new());
    }
}

pub fn mode_label(m: FanMode) -> &'static str {
    match m {
        FanMode::Disabled => "Auto",
        FanMode::Manual => "Manual",
        FanMode::Curve => "Curve",
    }
}

/// Calibration wizard, drawn on top of everything.
pub fn modal(app: &mut App, ctx: &egui::Context) {
    if let Some(c) = &mut app.fans.calibration {
        if let Outcome::Closed = c.show(ctx, &app.backend) {
            app.fans.calibration = None;
            app.refresh();
        }
    }
}

fn new_id() -> String {
    let n = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{:08x}", (n as u64 ^ (n >> 64) as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) as u32)
}

fn next_name(profiles: &[FanProfile]) -> String {
    let mut n = profiles.len() + 1;
    while profiles.iter().any(|p| p.name == format!("Profile {n}")) {
        n += 1;
    }
    format!("Profile {n}")
}

/// Hottest of the selected sensors (all sensors when none are selected).
fn pick_temp(temps: &BTreeMap<String, i32>, sensors: &[String]) -> Option<f64> {
    let sel = sensors.iter().filter_map(|s| temps.get(s)).max();
    sel.or_else(|| temps.values().max()).map(|v| *v as f64)
}

pub fn show(app: &mut App, ui: &mut Ui, snap: &Snapshot) {
    let latest = snap.thermal.last().cloned().unwrap_or_default();
    let sensors: Vec<String> = latest.temps.keys().cloned().collect();
    let calibration = snap.config.as_ref().and_then(|c| c.fan.calibration.clone());
    let fan_count = latest.rpms.len().max(calibration.as_ref().map(|c| c.fans.len()).unwrap_or(0)).max(1);

    let App { drafts, fans: fui, backend, .. } = app;
    let Some(fan) = drafts.fan.value.as_mut() else {
        ui.label("Loading…");
        return;
    };
    let mut dirty = false;

    card(ui, |ui| dirty |= profile_bar(ui, fan, fui));
    ui.add_space(12.0);

    let Some(idx) = fan.profiles.iter().position(|p| p.id == fan.active_profile) else { return };
    if fui.tab.is_some_and(|t| t >= fan_count) {
        fui.tab = None;
    }
    card(ui, |ui| {
        let profile = &mut fan.profiles[idx];
        ui.horizontal(|ui| {
            for (mode, label) in [(FanMode::Disabled, "Auto (EC)"), (FanMode::Manual, "Manual"), (FanMode::Curve, "Curve")] {
                if segmented(ui, profile.mode == mode, label) {
                    profile.mode = mode;
                    dirty = true;
                }
            }
            if profile.mode != FanMode::Disabled && fan_count > 1 {
                ui.add_space(16.0);
                ui.label(RichText::new("Fan:").weak());
                if segmented(ui, fui.tab.is_none(), "All") {
                    fui.tab = None;
                }
                for i in 0..fan_count {
                    let custom = profile.overrides.iter().any(|o| {
                        o.index as usize == i
                            && match profile.mode {
                                FanMode::Manual => o.manual.is_some(),
                                _ => o.curve.is_some(),
                            }
                    });
                    let label = if custom { format!("Fan {} •", i + 1) } else { format!("Fan {}", i + 1) };
                    if segmented(ui, fui.tab == Some(i), &label) {
                        fui.tab = Some(i);
                    }
                }
            }
        });
        ui.add_space(10.0);
        match profile.mode {
            FanMode::Disabled => {
                ui.label("The embedded controller manages fan speed on its own.");
            }
            FanMode::Manual => dirty |= manual_editor(ui, profile, fui.tab, fan_count),
            FanMode::Curve => {
                dirty |= curve_section(ui, profile, fui, &latest, &sensors, calibration.as_ref(), fan_count);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let label = if calibration.is_some() { "Recalibrate" } else { "Calibrate for live RPM" };
                    if ui.button(label).clicked() {
                        fui.calibration = Some(Calibration::new());
                    }
                    hint(
                        ui,
                        if calibration.is_some() {
                            "◆ markers show each fan's real duty, estimated from its RPM."
                        } else {
                            "Calibrate once to see each fan's real duty on the curve."
                        },
                    );
                });
            }
        }
    });
    let _ = backend;

    if dirty {
        drafts.fan.touch();
    }

    ui.add_space(12.0);
    card(ui, |ui| {
        section_title(ui, "Live fan speed");
        plots::rpm_plot(ui, &snap.thermal, 140.0);
    });
}

/// Profile picker, create/rename/delete/reorder and power-source auto-switch.
fn profile_bar(ui: &mut Ui, fan: &mut FanControlConfig, fui: &mut FansUi) -> bool {
    let mut dirty = false;
    ui.horizontal(|ui| {
        section_title(ui, "Profile");
        if let Some((id, buf)) = &mut fui.renaming {
            let r = ui.add(egui::TextEdit::singleline(buf).char_limit(NAME_MAX).desired_width(180.0));
            r.request_focus();
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
            if enter || (r.lost_focus() && !esc) || ui.button("✔").clicked() {
                let name = buf.trim().to_string();
                if !name.is_empty() {
                    if let Some(p) = fan.profiles.iter_mut().find(|p| &p.id == id) {
                        if p.name != name {
                            p.name = name;
                            dirty = true;
                        }
                    }
                }
                fui.renaming = None;
            } else if esc {
                fui.renaming = None;
            }
            return;
        }
        let current = fan.active().map(|p| p.name.clone()).unwrap_or_default();
        egui::ComboBox::from_id_salt("profile").selected_text(current).width(180.0).show_ui(ui, |ui| {
            for p in &fan.profiles {
                let mut label = p.name.clone();
                if fan.auto_switch.ac.as_deref() == Some(&p.id) {
                    label.push_str("  (plugged in)");
                }
                if fan.auto_switch.battery.as_deref() == Some(&p.id) {
                    label.push_str("  (on battery)");
                }
                if ui.selectable_label(p.id == fan.active_profile, label).clicked() && p.id != fan.active_profile {
                    fan.active_profile = p.id.clone();
                    fui.tab = None;
                    dirty = true;
                }
            }
        });
        if ui.button("+ New").on_hover_text("Duplicate the current profile").clicked() {
            if let Some(src) = fan.active().cloned() {
                let p = FanProfile { id: new_id(), name: next_name(&fan.profiles), ..src };
                fui.renaming = Some((p.id.clone(), p.name.clone()));
                fan.active_profile = p.id.clone();
                fan.profiles.push(p);
                dirty = true;
            }
        }
        if ui.button("Rename").clicked() {
            if let Some(p) = fan.active() {
                fui.renaming = Some((p.id.clone(), p.name.clone()));
            }
        }
        let pos = fan.profiles.iter().position(|p| p.id == fan.active_profile).unwrap_or(0);
        if ui.add_enabled(pos > 0, egui::Button::new("▲")).on_hover_text("Move up").clicked() {
            fan.profiles.swap(pos, pos - 1);
            dirty = true;
        }
        if ui.add_enabled(pos + 1 < fan.profiles.len(), egui::Button::new("▼")).on_hover_text("Move down").clicked() {
            fan.profiles.swap(pos, pos + 1);
            dirty = true;
        }
        if fui.confirm_delete {
            ui.label("Delete this profile?");
            if ui.button(RichText::new("Delete").color(crate::theme::error(ui.visuals().dark_mode))).clicked() {
                let id = fan.active_profile.clone();
                fan.profiles.retain(|p| p.id != id);
                for slot in [&mut fan.auto_switch.ac, &mut fan.auto_switch.battery] {
                    if slot.as_deref() == Some(&id) {
                        *slot = None;
                    }
                }
                fan.active_profile = fan.profiles[0].id.clone();
                fui.confirm_delete = false;
                fui.tab = None;
                dirty = true;
            }
            if ui.button("Cancel").clicked() {
                fui.confirm_delete = false;
            }
        } else if ui.add_enabled(fan.profiles.len() > 1, egui::Button::new("Delete")).clicked() {
            fui.confirm_delete = true;
        }
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Switch automatically:").weak());
        let names: Vec<(String, String)> = fan.profiles.iter().map(|p| (p.id.clone(), p.name.clone())).collect();
        for (label, slot) in [("Plugged in", &mut fan.auto_switch.ac), ("On battery", &mut fan.auto_switch.battery)] {
            ui.label(label);
            let selected = slot
                .as_ref()
                .and_then(|id| names.iter().find(|(i, _)| i == id))
                .map(|(_, n)| n.clone())
                .unwrap_or("Don't switch".into());
            egui::ComboBox::from_id_salt(label).selected_text(selected).width(140.0).show_ui(ui, |ui| {
                if ui.selectable_label(slot.is_none(), "Don't switch").clicked() {
                    *slot = None;
                    dirty = true;
                }
                for (id, name) in &names {
                    if ui.selectable_label(slot.as_deref() == Some(id), name).clicked() {
                        *slot = Some(id.clone());
                        dirty = true;
                    }
                }
            });
            ui.add_space(8.0);
        }
    });
    dirty
}

fn override_mut(profile: &mut FanProfile, i: usize) -> &mut FanOverride {
    if let Some(pos) = profile.overrides.iter().position(|o| o.index as usize == i) {
        return &mut profile.overrides[pos];
    }
    profile.overrides.push(FanOverride { index: i as u32, manual: None, curve: None });
    profile.overrides.sort_by_key(|o| o.index);
    let pos = profile.overrides.iter().position(|o| o.index as usize == i).unwrap();
    &mut profile.overrides[pos]
}

fn prune_overrides(profile: &mut FanProfile) {
    profile.overrides.retain(|o| o.manual.is_some() || o.curve.is_some());
}

fn manual_editor(ui: &mut Ui, profile: &mut FanProfile, tab: Option<usize>, fan_count: usize) -> bool {
    let mut dirty = false;
    let global = profile.manual.as_ref().map(|m| m.duty_pct).unwrap_or(50);
    match tab {
        None => {
            let m = profile.manual.get_or_insert(ManualConfig { duty_pct: 50 });
            ui.horizontal(|ui| {
                ui.label("Fan duty");
                dirty |= ui.add(egui::Slider::new(&mut m.duty_pct, 0..=100).suffix(" %")).changed();
            });
            for i in 0..fan_count {
                if let Some(d) = profile.overrides.iter().find(|o| o.index as usize == i).and_then(|o| o.manual.as_ref()) {
                    hint(ui, &format!("Fan {} uses its own duty: {}%", i + 1, d.duty_pct));
                }
            }
        }
        Some(i) => {
            let has = profile.overrides.iter().any(|o| o.index as usize == i && o.manual.is_some());
            if has {
                let ov = override_mut(profile, i);
                let m = ov.manual.as_mut().unwrap();
                ui.horizontal(|ui| {
                    ui.label(format!("Fan {} duty", i + 1));
                    dirty |= ui.add(egui::Slider::new(&mut m.duty_pct, 0..=100).suffix(" %")).changed();
                });
                if ui.button("Use the profile's duty instead").clicked() {
                    override_mut(profile, i).manual = None;
                    prune_overrides(profile);
                    dirty = true;
                }
            } else {
                ui.label(format!("Fan {} follows the profile's duty ({global}%).", i + 1));
                if ui.button(format!("Customize Fan {}", i + 1)).clicked() {
                    override_mut(profile, i).manual = Some(ManualConfig { duty_pct: global });
                    dirty = true;
                }
            }
        }
    }
    dirty
}

fn curve_section(
    ui: &mut Ui,
    profile: &mut FanProfile,
    fui: &mut FansUi,
    latest: &TelemetrySample,
    sensors: &[String],
    calibration: Option<&FanCalibration>,
    fan_count: usize,
) -> bool {
    let mut dirty = false;
    let global = profile.curve.get_or_insert_with(GlobalCurveConfig::default).clone();
    let fan_curve = |p: &FanProfile, i: usize| -> Option<CurveConfig> {
        p.overrides.iter().find(|o| o.index as usize == i).and_then(|o| o.curve.clone())
    };
    let probe = |i: usize, custom: bool, curve: &CurveConfig| -> Option<Probe> {
        let temp = pick_temp(&latest.temps, &curve.sensors)?;
        let duty = calibration.and_then(|c| c.duty_for_rpm(i, *latest.rpms.get(i)?));
        let label = if custom { format!("Fan {} (own curve)", i + 1) } else { format!("Fan {}", i + 1) };
        Some(Probe { label, temp, duty })
    };

    match fui.tab {
        None => {
            let probes: Vec<Probe> = if calibration.is_some() {
                (0..fan_count)
                    .filter_map(|i| match fan_curve(profile, i) {
                        Some(c) => probe(i, true, &c),
                        None => probe(i, false, &global.curve),
                    })
                    .collect()
            } else {
                pick_temp(&latest.temps, &global.curve.sensors)
                    .map(|t| vec![Probe { label: "Now".into(), temp: t, duty: None }])
                    .unwrap_or_default()
            };
            let g = profile.curve.as_mut().unwrap();
            dirty |= plots::curve_editor(ui, "curve_all", &mut g.curve, &probes, &mut fui.drag);
            hint(ui, "Drag points to move them · click empty space to add · right-click a point to remove");
            ui.add_space(6.0);
            dirty |= curve_options(ui, "opts_all", &mut g.curve, sensors, Some(&mut g.poll_ms));
        }
        Some(i) => match fan_curve(profile, i) {
            Some(c) => {
                let probes: Vec<Probe> = probe(i, true, &c).into_iter().collect();
                let ov = override_mut(profile, i);
                let curve = ov.curve.as_mut().unwrap();
                dirty |= plots::curve_editor(ui, &format!("curve_{i}"), curve, &probes, &mut fui.drag);
                hint(ui, "Drag points to move them · click empty space to add · right-click a point to remove");
                ui.add_space(6.0);
                dirty |= curve_options(ui, &format!("opts_{i}"), curve, sensors, None);
                if ui.button("Use the profile's curve instead").clicked() {
                    override_mut(profile, i).curve = None;
                    prune_overrides(profile);
                    dirty = true;
                }
            }
            None => {
                ui.label(format!("Fan {} follows the profile's curve.", i + 1));
                if widgets::primary_button(ui, &format!("Give Fan {} its own curve", i + 1)).clicked() {
                    override_mut(profile, i).curve = Some(global.curve.clone());
                    dirty = true;
                }
            }
        },
    }
    dirty
}

fn curve_options(ui: &mut Ui, id: &str, curve: &mut CurveConfig, sensors: &[String], poll_ms: Option<&mut u64>) -> bool {
    let mut dirty = false;
    egui::Grid::new(id).num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
        ui.label("Sensors");
        ui.horizontal_wrapped(|ui| {
            for s in sensors {
                let mut on = curve.sensors.contains(s);
                if ui.checkbox(&mut on, s).changed() {
                    if on {
                        curve.sensors.push(s.clone());
                    } else {
                        curve.sensors.retain(|x| x != s);
                    }
                    dirty = true;
                }
            }
        });
        ui.end_row();
        ui.label("Hysteresis");
        dirty |= ui.add(egui::Slider::new(&mut curve.hysteresis_c, 0..=10).suffix(" °C")).changed();
        ui.end_row();
        ui.label("Max increase per step");
        dirty |= ui.add(egui::Slider::new(&mut curve.rate_limit_pct_per_step, 1..=100).suffix(" %")).changed();
        ui.end_row();
        let mut separate = curve.rate_limit_down_pct_per_step.is_some();
        if ui.checkbox(&mut separate, "Separate decrease limit").changed() {
            curve.rate_limit_down_pct_per_step = separate.then_some(curve.rate_limit_pct_per_step);
            dirty = true;
        }
        if let Some(down) = curve.rate_limit_down_pct_per_step.as_mut() {
            dirty |= ui.add(egui::Slider::new(down, 1..=100).suffix(" %")).changed();
        } else {
            ui.label(RichText::new("same as increase").weak());
        }
        ui.end_row();
        if let Some(poll) = poll_ms {
            ui.label("Update interval");
            dirty |= ui.add(egui::Slider::new(poll, 500..=10_000).step_by(250.0).suffix(" ms")).changed();
            ui.end_row();
        }
    });
    dirty
}
