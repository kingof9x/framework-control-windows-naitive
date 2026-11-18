use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui::{self, RichText, Ui};

use crate::api::*;
use crate::app::App;
use crate::calibration::{Calibration, Outcome};
use crate::plots::{self, Probe};
use crate::state::Snapshot;
use crate::theme;
use crate::widgets::{accent_button, button, card, group, section, segmented, switch, Icon};

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

    let App { drafts, fans: fui, .. } = app;
    let Some(fan) = drafts.fan.value.as_mut() else {
        card(ui, Some(Icon::Fan), "Loading…", "Waiting for the service.", false, |_| {});
        return;
    };
    let mut dirty = false;

    section(ui, "Profiles");
    dirty |= profile_bar(ui, fan, fui);

    let Some(idx) = fan.profiles.iter().position(|p| p.id == fan.active_profile) else { return };
    if fui.tab.is_some_and(|t| t >= fan_count) {
        fui.tab = None;
    }
    let profile = &mut fan.profiles[idx];
    section(ui, "Fan control");
    let mode_help = match profile.mode {
        FanMode::Disabled => "The laptop's embedded controller manages fan speed on its own.",
        FanMode::Manual => "The fans run at a fixed speed you choose.",
        FanMode::Curve => "Fan speed follows temperature along a curve you draw.",
    };
    card(ui, Some(Icon::Fan), "Mode", mode_help, false, |ui| {
        for (mode, label) in [(FanMode::Disabled, "Auto"), (FanMode::Manual, "Manual"), (FanMode::Curve, "Curve")] {
            if segmented(ui, profile.mode == mode, label) {
                profile.mode = mode;
                dirty = true;
            }
        }
    });
    if profile.mode != FanMode::Disabled && fan_count > 1 {
        card(ui, Some(Icon::Sliders), "Fans", "Set both fans together, or give one fan its own settings (marked •).", false, |ui| {
            if segmented(ui, fui.tab.is_none(), "Both") {
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
        });
    }
    match profile.mode {
        FanMode::Disabled => {}
        FanMode::Manual => dirty |= manual_editor(ui, profile, fui.tab, fan_count),
        FanMode::Curve => {
            dirty |= curve_section(ui, profile, fui, &latest, &sensors, calibration.as_ref(), fan_count);
            let help = if calibration.is_some() {
                "The ◆ markers on the curve show each fan's real speed, estimated from its RPM."
            } else {
                "Measure your fans once (about a minute) to see their real speed on the curve."
            };
            card(ui, Some(Icon::Target), "Live fan speed on the curve", help, false, |ui| {
                let label = if calibration.is_some() { "Recalibrate" } else { "Calibrate" };
                if button(ui, label).clicked() {
                    fui.calibration = Some(Calibration::new());
                }
            });
        }
    }

    if dirty {
        drafts.fan.touch();
    }

    section(ui, "Fan speed");
    group(ui, "", "", |ui| plots::rpm_plot(ui, &snap.thermal, 140.0));
}

/// Profile picker, create/rename/delete/reorder and power-source auto-switch.
fn profile_bar(ui: &mut Ui, fan: &mut FanControlConfig, fui: &mut FansUi) -> bool {
    let mut dirty = false;
    card(ui, Some(Icon::Options), "Active profile", "Each profile is a complete set of fan settings.", false, |ui| {
        if let Some((id, buf)) = &mut fui.renaming {
            let r = ui.add(egui::TextEdit::singleline(buf).char_limit(NAME_MAX).desired_width(200.0));
            r.request_focus();
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
            if enter || (r.lost_focus() && !esc) || accent_button(ui, "Save").clicked() {
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
        egui::ComboBox::from_id_salt("profile").selected_text(current).width(200.0).show_ui(ui, |ui| {
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
    });

    card(ui, Some(Icon::Edit), "Manage profiles", "Make a copy of this profile, rename it, change its place in the list, or delete it.", false, |ui| {
        if fui.confirm_delete {
            let c = theme::colors(ui.ctx());
            ui.label(RichText::new("Delete this profile?").color(c.text2));
            if accent_button(ui, "Delete").clicked() {
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
            if button(ui, "Cancel").clicked() {
                fui.confirm_delete = false;
            }
            return;
        }
        if button(ui, "New").on_hover_text("Copy the current profile").clicked() {
            if let Some(src) = fan.active().cloned() {
                let p = FanProfile { id: new_id(), name: next_name(&fan.profiles), ..src };
                fui.renaming = Some((p.id.clone(), p.name.clone()));
                fan.active_profile = p.id.clone();
                fan.profiles.push(p);
                dirty = true;
            }
        }
        if button(ui, "Rename").clicked() {
            if let Some(p) = fan.active() {
                fui.renaming = Some((p.id.clone(), p.name.clone()));
            }
        }
        let pos = fan.profiles.iter().position(|p| p.id == fan.active_profile).unwrap_or(0);
        if ui.add_enabled(pos > 0, egui::Button::new("Move up").min_size(egui::vec2(0.0, 32.0))).clicked() {
            fan.profiles.swap(pos, pos - 1);
            dirty = true;
        }
        if ui.add_enabled(pos + 1 < fan.profiles.len(), egui::Button::new("Move down").min_size(egui::vec2(0.0, 32.0))).clicked() {
            fan.profiles.swap(pos, pos + 1);
            dirty = true;
        }
        if ui.add_enabled(fan.profiles.len() > 1, egui::Button::new("Delete").min_size(egui::vec2(0.0, 32.0))).clicked() {
            fui.confirm_delete = true;
        }
    });

    let names: Vec<(String, String)> = fan.profiles.iter().map(|p| (p.id.clone(), p.name.clone())).collect();
    for (title, help, slot) in [
        ("When plugged in", "Switch to this profile when you plug in.", &mut fan.auto_switch.ac),
        ("On battery", "Switch to this profile when you unplug.", &mut fan.auto_switch.battery),
    ] {
        card(ui, Some(Icon::Plug), title, help, false, |ui| {
            let selected = slot
                .as_ref()
                .and_then(|id| names.iter().find(|(i, _)| i == id))
                .map(|(_, n)| n.clone())
                .unwrap_or("Don't switch".into());
            egui::ComboBox::from_id_salt(title).selected_text(selected).width(200.0).show_ui(ui, |ui| {
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
        });
    }
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
            let own: Vec<String> = (0..fan_count)
                .filter_map(|i| {
                    let d = profile.overrides.iter().find(|o| o.index as usize == i).and_then(|o| o.manual.as_ref())?;
                    Some(format!("Fan {} uses its own speed ({}%).", i + 1, d.duty_pct))
                })
                .collect();
            let m = profile.manual.get_or_insert(ManualConfig { duty_pct: 50 });
            card(ui, Some(Icon::Gauge), "Fan speed", &own.join(" "), false, |ui| {
                dirty |= ui.add(egui::Slider::new(&mut m.duty_pct, 0..=100).suffix(" %")).changed();
            });
        }
        Some(i) => {
            let has = profile.overrides.iter().any(|o| o.index as usize == i && o.manual.is_some());
            if has {
                let m = override_mut(profile, i).manual.as_mut().unwrap();
                let mut reset = false;
                card(ui, Some(Icon::Gauge), &format!("Fan {} speed", i + 1), "This fan's own speed.", false, |ui| {
                    dirty |= ui.add(egui::Slider::new(&mut m.duty_pct, 0..=100).suffix(" %")).changed();
                    reset = button(ui, "Use both-fans speed").clicked();
                });
                if reset {
                    override_mut(profile, i).manual = None;
                    prune_overrides(profile);
                    dirty = true;
                }
            } else {
                card(
                    ui,
                    Some(Icon::Gauge),
                    &format!("Fan {} speed", i + 1),
                    &format!("Follows the speed set for both fans ({global}%)."),
                    false,
                    |ui| {
                        if button(ui, "Give it its own speed").clicked() {
                            override_mut(profile, i).manual = Some(ManualConfig { duty_pct: global });
                            dirty = true;
                        }
                    },
                );
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
            group(ui, "Curve", "Drag points to move them, click empty space to add one, right-click a point to remove it.", |ui| {
                dirty |= plots::curve_editor(ui, "curve_all", &mut g.curve, &probes, &mut fui.drag);
            });
            dirty |= curve_options(ui, "opts_all", &mut g.curve, sensors, Some(&mut g.poll_ms));
        }
        Some(i) => match fan_curve(profile, i) {
            Some(c) => {
                let probes: Vec<Probe> = probe(i, true, &c).into_iter().collect();
                let ov = override_mut(profile, i);
                let curve = ov.curve.as_mut().unwrap();
                let mut reset = false;
                group(ui, &format!("Fan {} curve", i + 1), "This fan's own curve. Drag, click or right-click points to change it.", |ui| {
                    dirty |= plots::curve_editor(ui, &format!("curve_{i}"), curve, &probes, &mut fui.drag);
                });
                dirty |= curve_options(ui, &format!("opts_{i}"), curve, sensors, None);
                card(ui, Some(Icon::Sync), "Use the curve for both fans", "Remove this fan's own curve.", false, |ui| {
                    reset = button(ui, "Remove").clicked();
                });
                if reset {
                    override_mut(profile, i).curve = None;
                    prune_overrides(profile);
                    dirty = true;
                }
            }
            None => {
                card(ui, Some(Icon::Curve), &format!("Fan {} curve", i + 1), "Follows the curve set for both fans.", false, |ui| {
                    if accent_button(ui, "Give it its own curve").clicked() {
                        override_mut(profile, i).curve = Some(global.curve.clone());
                        dirty = true;
                    }
                });
            }
        },
    }
    dirty
}

fn curve_options(ui: &mut Ui, id: &str, curve: &mut CurveConfig, sensors: &[String], poll_ms: Option<&mut u64>) -> bool {
    let mut dirty = false;
    group(ui, "Sensors", "The fans follow the hottest of the sensors you tick (all of them if none are ticked).", |ui| {
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
                ui.add_space(8.0);
            }
        });
    });
    card(ui, Some(Icon::Temperature), "Hysteresis", "How far the temperature must drop before the fans slow down.", false, |ui| {
        dirty |= ui.add(egui::Slider::new(&mut curve.hysteresis_c, 0..=10).suffix(" °C")).changed();
    });
    card(ui, Some(Icon::Speed), "Speed-up limit", "The most the fan speed can rise in one step.", false, |ui| {
        dirty |= ui.add(egui::Slider::new(&mut curve.rate_limit_pct_per_step, 1..=100).suffix(" %")).changed();
    });
    let mut separate = curve.rate_limit_down_pct_per_step.is_some();
    card(ui, Some(Icon::Sort), "Separate slow-down limit", "Off: slowing down uses the speed-up limit.", false, |ui| {
        if let Some(down) = curve.rate_limit_down_pct_per_step.as_mut() {
            dirty |= ui.add(egui::Slider::new(down, 1..=100).suffix(" %")).changed();
            ui.add_space(12.0);
        }
        if switch(ui, &mut separate) {
            curve.rate_limit_down_pct_per_step = separate.then_some(curve.rate_limit_pct_per_step);
            dirty = true;
        }
    });
    if let Some(poll) = poll_ms {
        card(ui, Some(Icon::Timer), "Update interval", "How often the service checks the temperature and sets the fans.", false, |ui| {
            dirty |= ui.add(egui::Slider::new(poll, 500..=10_000).step_by(250.0).suffix(" ms")).changed();
        });
    }
    let _ = id;
    dirty
}
