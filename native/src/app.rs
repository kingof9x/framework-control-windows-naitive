use std::sync::mpsc::Sender;
use std::sync::Arc;

use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Stroke, Ui, Vec2};
use egui_plot::{Corner, Legend, Line, Plot, PlotBounds, PlotPoint, PlotPoints, Points, VLine};
use serde_json::json;

use crate::api::*;
use crate::state::{self, Cmd, Shared, Snapshot};

const SERIES: [Color32; 8] = [
    Color32::from_rgb(0x4c, 0x9a, 0xff),
    Color32::from_rgb(0xff, 0x8a, 0x3d),
    Color32::from_rgb(0x3d, 0xc9, 0x8f),
    Color32::from_rgb(0xe0, 0x5c, 0xc8),
    Color32::from_rgb(0xf2, 0xc1, 0x3a),
    Color32::from_rgb(0x8d, 0x7b, 0xff),
    Color32::from_rgb(0x4f, 0xd1, 0xd9),
    Color32::from_rgb(0xff, 0x6b, 0x6b),
];
pub const ACCENT: Color32 = Color32::from_rgb(0x4c, 0x9a, 0xff);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Overview,
    Fans,
    Power,
    Battery,
    Settings,
}

impl Page {
    const ALL: [Page; 5] = [Page::Overview, Page::Fans, Page::Power, Page::Battery, Page::Settings];
    fn label(self) -> &'static str {
        match self {
            Page::Overview => "Overview",
            Page::Fans => "Fans",
            Page::Power => "Power",
            Page::Battery => "Battery",
            Page::Settings => "Settings",
        }
    }
    fn icon(self) -> &'static str {
        match self {
            Page::Overview => "⌂",
            Page::Fans => "✳",
            Page::Power => "⚡",
            Page::Battery => "▮",
            Page::Settings => "⚙",
        }
    }
}

/// Locally edited copies of config sections; only replaced from the server
/// while the user has no unsaved edits.
#[derive(Default)]
struct Drafts {
    rev: u64,
    fan: Option<FanControlConfig>,
    fan_dirty: bool,
    power: Option<PowerConfig>,
    power_dirty: bool,
    battery: Option<BatteryConfig>,
    battery_dirty: bool,
}

pub struct App {
    shared: Shared,
    tx: Sender<Cmd>,
    page: Page,
    drafts: Drafts,
    power_tab_ac: bool,
    drag_point: Option<usize>,
    source: String,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, backend: Arc<dyn Backend>, page: Option<&str>) -> Self {
        crate::theme::install(&cc.egui_ctx);
        let source = backend.describe();
        let (shared, tx) = state::spawn(backend, cc.egui_ctx.clone());
        let page = match page.unwrap_or("") {
            "fans" => Page::Fans,
            "power" => Page::Power,
            "battery" => Page::Battery,
            "settings" => Page::Settings,
            _ => Page::Overview,
        };
        Self {
            shared,
            tx,
            page,
            drafts: Drafts::default(),
            power_tab_ac: true,
            drag_point: None,
            source,
        }
    }

    fn sync_drafts(&mut self, snap: &Snapshot) {
        let Some(cfg) = &snap.config else { return };
        if self.drafts.rev == snap.config_rev && self.drafts.fan.is_some() {
            return;
        }
        self.drafts.rev = snap.config_rev;
        if !self.drafts.fan_dirty {
            self.drafts.fan = Some(cfg.fan.clone());
        }
        if !self.drafts.power_dirty {
            self.drafts.power = Some(cfg.power.clone());
        }
        if !self.drafts.battery_dirty {
            self.drafts.battery = Some(cfg.battery.clone());
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let shared = self.shared.clone();
        let snap = shared.lock().unwrap();
        self.sync_drafts(&snap);

        egui::TopBottomPanel::top("header")
            .frame(egui::Frame::new().inner_margin(Margin::symmetric(16, 10)).fill(ctx.style().visuals.panel_fill))
            .show(ctx, |ui| self.header(ui, &snap));

        egui::SidePanel::left("nav")
            .exact_width(176.0)
            .resizable(false)
            .frame(egui::Frame::new().inner_margin(Margin::same(10)).fill(ctx.style().visuals.panel_fill))
            .show(ctx, |ui| self.nav(ui, &snap));

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .inner_margin(Margin::same(16))
                    .fill(ctx.style().visuals.extreme_bg_color),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    if !snap.connected && snap.health.is_none() {
                        disconnected(ui, &snap, &self.source);
                        return;
                    }
                    match self.page {
                        Page::Overview => self.overview(ui, &snap),
                        Page::Fans => self.fans(ui, &snap),
                        Page::Power => self.power(ui, &snap),
                        Page::Battery => self.battery(ui, &snap),
                        Page::Settings => self.settings(ui, &snap),
                    }
                });
            });
    }
}

// ---------------- Chrome ----------------

impl App {
    fn header(&mut self, ui: &mut Ui, snap: &Snapshot) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Framework Control").size(18.0).strong());
            if let Some(v) = snap.versions.as_ref().and_then(|v| v.mainboard_type.clone()) {
                ui.label(RichText::new(v).weak());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (color, text) = if snap.connected {
                    (Color32::from_rgb(0x3d, 0xc9, 0x8f), "Service connected")
                } else {
                    (Color32::from_rgb(0xff, 0x6b, 0x6b), "Service offline")
                };
                ui.label(RichText::new(text).weak());
                let (r, p) = ui.allocate_painter(Vec2::splat(10.0), egui::Sense::hover());
                p.circle_filled(r.rect.center(), 4.0, color);
                if let Some((msg, err)) = &snap.write_status {
                    let c = if *err { Color32::from_rgb(0xff, 0x6b, 0x6b) } else { ui.visuals().weak_text_color() };
                    ui.add_space(12.0);
                    ui.label(RichText::new(msg).color(c));
                }
            });
        });
    }

    fn nav(&mut self, ui: &mut Ui, snap: &Snapshot) {
        ui.add_space(4.0);
        for p in Page::ALL {
            let selected = self.page == p;
            let text = RichText::new(format!("{}   {}", p.icon(), p.label())).size(14.5);
            let btn = egui::Button::new(text)
                .min_size(Vec2::new(ui.available_width(), 34.0))
                .corner_radius(CornerRadius::same(6))
                .fill(if selected { ui.visuals().selection.bg_fill } else { Color32::TRANSPARENT })
                .stroke(Stroke::NONE);
            if ui.add(btn).clicked() {
                self.page = p;
            }
            ui.add_space(2.0);
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            if let Some(h) = &snap.health {
                ui.label(RichText::new(format!("Service v{}", h.service_version)).small().weak());
            }
            ui.label(RichText::new(&self.source).small().weak());
        });
    }
}

fn disconnected(ui: &mut Ui, snap: &Snapshot, source: &str) {
    card(ui, |ui| {
        ui.heading("Can't reach the Framework Control service");
        ui.add_space(6.0);
        ui.label(format!("Tried {source}. Make sure the service is installed and running."));
        ui.label("This app only replaces the browser UI; the background service still does the hardware work.");
        if let Some(e) = &snap.last_error {
            ui.add_space(6.0);
            ui.label(RichText::new(e).weak());
        }
        ui.add_space(4.0);
        ui.label(RichText::new("Use --port <n> if the service listens on a non-default port.").weak());
    });
}

// ---------------- Pages ----------------

impl App {
    fn overview(&mut self, ui: &mut Ui, snap: &Snapshot) {
        if snap.health.as_ref().is_some_and(|h| !h.cli_present) {
            card(ui, |ui| {
                ui.label(
                    RichText::new("⚠ framework_tool was not found by the service, so live readings are unavailable.")
                        .color(Color32::from_rgb(0xf2, 0xc1, 0x3a)),
                );
            });
            ui.add_space(12.0);
        }
        let latest = snap.thermal.last();
        let hottest = latest.and_then(|s| s.temps.iter().max_by_key(|(_, v)| **v).map(|(k, v)| (k.clone(), *v)));
        let rpm = latest.map(|s| s.rpms.clone()).unwrap_or_default();
        let bat = snap.power.as_ref().and_then(|p| p.battery.clone());

        let status = |b: &BatteryInfo| {
            if b.charging == Some(true) {
                "Charging"
            } else if b.ac_present == Some(true) {
                "Plugged in"
            } else {
                "On battery"
            }
        };
        let profile = snap.config.as_ref().and_then(|c| c.fan.active());
        stat_row(
            ui,
            &[
                (
                    "Hottest sensor",
                    hottest.as_ref().map(|(_, v)| format!("{v} °C")).unwrap_or("—".into()),
                    hottest.map(|(k, _)| k).unwrap_or_default(),
                ),
                (
                    "Fans (RPM)",
                    if rpm.is_empty() { "—".into() } else { rpm.iter().map(|r| r.to_string()).collect::<Vec<_>>().join(" / ") },
                    format!("{} fan(s)", rpm.len()),
                ),
                (
                    "Battery",
                    bat.as_ref().and_then(|b| b.percentage).map(|p| format!("{p}%")).unwrap_or("—".into()),
                    bat.as_ref().map(|b| status(b).to_string()).unwrap_or_default(),
                ),
                (
                    "Battery power",
                    bat.as_ref().and_then(|b| b.watts()).map(|w| format!("{:+.1} W", w)).unwrap_or("—".into()),
                    "+ in / − out".into(),
                ),
                (
                    "Fan profile",
                    profile.map(|p| p.name.clone()).unwrap_or("—".into()),
                    profile.map(|p| mode_label(p.mode).to_string()).unwrap_or_default(),
                ),
            ],
        );
        ui.add_space(12.0);

        card(ui, |ui| {
            section_title(ui, "Temperatures");
            temp_plot(ui, &snap.thermal, 240.0);
        });
        ui.add_space(12.0);
        card(ui, |ui| {
            section_title(ui, "Fan speed");
            rpm_plot(ui, &snap.thermal, 160.0);
        });
    }

    fn fans(&mut self, ui: &mut Ui, snap: &Snapshot) {
        let Some(fan) = self.drafts.fan.as_mut() else {
            ui.label("Loading…");
            return;
        };
        let mut dirty = false;
        let sensors: Vec<String> = snap.thermal.last().map(|s| s.temps.keys().cloned().collect()).unwrap_or_default();

        card(ui, |ui| {
            ui.horizontal(|ui| {
                section_title(ui, "Profile");
                let current = fan.active().map(|p| p.name.clone()).unwrap_or_default();
                egui::ComboBox::from_id_salt("profile")
                    .selected_text(current)
                    .width(180.0)
                    .show_ui(ui, |ui| {
                        for p in &fan.profiles {
                            if ui.selectable_label(p.id == fan.active_profile, &p.name).clicked() {
                                fan.active_profile = p.id.clone();
                                dirty = true;
                            }
                        }
                    });
            });
            ui.add_space(8.0);
            let Some(idx) = fan.profiles.iter().position(|p| p.id == fan.active_profile) else { return };
            let profile = &mut fan.profiles[idx];

            ui.horizontal(|ui| {
                for (mode, label) in [(FanMode::Disabled, "Auto (EC)"), (FanMode::Manual, "Manual"), (FanMode::Curve, "Curve")] {
                    if segmented(ui, profile.mode == mode, label) {
                        profile.mode = mode;
                        dirty = true;
                    }
                }
            });
            ui.add_space(10.0);

            match profile.mode {
                FanMode::Disabled => {
                    ui.label("The embedded controller manages fan speed on its own.");
                }
                FanMode::Manual => {
                    let m = profile.manual.get_or_insert(ManualConfig { duty_pct: 50 });
                    ui.horizontal(|ui| {
                        ui.label("Fan duty");
                        dirty |= ui
                            .add(egui::Slider::new(&mut m.duty_pct, 0..=100).suffix(" %"))
                            .changed();
                    });
                }
                FanMode::Curve => {
                    let curve = profile.curve.get_or_insert_with(CurveConfig::default);
                    let current_temp = snap.thermal.last().and_then(|s| {
                        curve
                            .sensors
                            .iter()
                            .filter_map(|n| s.temps.get(n))
                            .max()
                            .copied()
                            .or_else(|| s.temps.values().max().copied())
                    });
                    dirty |= curve_editor(ui, curve, current_temp, &mut self.drag_point);
                    ui.label(
                        RichText::new("Drag points to move them · click empty space to add · right-click a point to remove")
                            .small()
                            .weak(),
                    );
                    ui.add_space(8.0);
                    egui::Grid::new("curve_opts").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                        ui.label("Sensors");
                        ui.horizontal_wrapped(|ui| {
                            for s in &sensors {
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
                        ui.label("Max change per step");
                        dirty |= ui
                            .add(egui::Slider::new(&mut curve.rate_limit_pct_per_step, 1..=100).suffix(" %"))
                            .changed();
                        ui.end_row();
                        ui.label("Update interval");
                        dirty |= ui
                            .add(egui::Slider::new(&mut curve.poll_ms, 500..=10_000).step_by(250.0).suffix(" ms"))
                            .changed();
                        ui.end_row();
                    });
                }
            }
        });

        if dirty {
            self.drafts.fan_dirty = true;
        }
        if self.drafts.fan_dirty {
            ui.add_space(10.0);
            if let Some(action) = save_bar(ui) {
                if action {
                    let fan = self.drafts.fan.as_ref().unwrap();
                    let _ = self.tx.send(Cmd::Patch(json!({
                        "fan": { "profiles": fan.profiles, "active_profile": fan.active_profile }
                    })));
                }
                self.drafts.fan_dirty = false;
                self.drafts.rev = u64::MAX; // force resync from server
            }
        }

        ui.add_space(12.0);
        card(ui, |ui| {
            section_title(ui, "Live fan speed");
            rpm_plot(ui, &snap.thermal, 140.0);
        });
    }

    fn power(&mut self, ui: &mut Ui, snap: &Snapshot) {
        let Some(pc) = snap.power.as_ref().map(|p| p.power_control.clone()) else {
            ui.label("Loading…");
            return;
        };
        let Some(draft) = self.drafts.power.as_mut() else { return };
        let caps = &pc.capabilities;
        let mut dirty = false;

        card(ui, |ui| {
            ui.horizontal(|ui| {
                section_title(ui, "Power profile");
                ui.add_space(8.0);
                if segmented(ui, self.power_tab_ac, "🔌 Plugged in") {
                    self.power_tab_ac = true;
                }
                if segmented(ui, !self.power_tab_ac, "▮ On battery") {
                    self.power_tab_ac = false;
                }
            });
            ui.add_space(10.0);
            let prof = if self.power_tab_ac {
                draft.ac.get_or_insert_with(Default::default)
            } else {
                draft.battery.get_or_insert_with(Default::default)
            };

            let any = caps.supports_tdp || caps.supports_thermal || caps.supports_epp || caps.supports_governor;
            if !any {
                ui.label("No power controls are available. On Windows, install RyzenAdj from the web UI's Power panel.");
            }
            egui::Grid::new("power_grid").num_columns(3).spacing([16.0, 10.0]).show(ui, |ui| {
                if caps.supports_tdp {
                    let (lo, hi) = (caps.tdp_min_watts.unwrap_or(5), caps.tdp_max_watts.unwrap_or(120));
                    let s = prof.tdp_watts.get_or_insert(SettingU32 { enabled: false, value: 30 });
                    dirty |= setting_row_u32(ui, "TDP limit", s, lo..=hi, " W", pc.current_state.tdp_limit_watts);
                }
                if caps.supports_thermal {
                    let s = prof.thermal_limit_c.get_or_insert(SettingU32 { enabled: false, value: 95 });
                    dirty |= setting_row_u32(ui, "Thermal limit", s, 60..=100, " °C", pc.current_state.thermal_limit_c);
                }
                if caps.supports_epp {
                    let opts = caps.available_epp_preferences.clone().unwrap_or_default();
                    let s = prof.epp_preference.get_or_insert(SettingString::default());
                    dirty |= setting_row_choice(ui, "Energy preference", s, &opts, pc.current_state.epp_preference.clone());
                }
                if caps.supports_governor {
                    let opts = caps.available_governors.clone().unwrap_or_default();
                    let s = prof.governor.get_or_insert(SettingString::default());
                    dirty |= setting_row_choice(ui, "CPU governor", s, &opts, pc.current_state.governor.clone());
                }
                if caps.supports_frequency_limits {
                    let (lo, hi) = (caps.frequency_min_mhz.unwrap_or(400), caps.frequency_max_mhz.unwrap_or(5000));
                    let s = prof.min_freq_mhz.get_or_insert(SettingU32 { enabled: false, value: lo });
                    dirty |= setting_row_u32(ui, "Min frequency", s, lo..=hi, " MHz", pc.current_state.min_freq_mhz);
                    let s = prof.max_freq_mhz.get_or_insert(SettingU32 { enabled: false, value: hi });
                    dirty |= setting_row_u32(ui, "Max frequency", s, lo..=hi, " MHz", pc.current_state.max_freq_mhz);
                }
            });
        });

        if dirty {
            self.drafts.power_dirty = true;
        }
        if self.drafts.power_dirty {
            ui.add_space(10.0);
            if let Some(action) = save_bar(ui) {
                if action {
                    let p = self.drafts.power.as_ref().unwrap();
                    let _ = self.tx.send(Cmd::Patch(json!({ "power": p })));
                }
                self.drafts.power_dirty = false;
                self.drafts.rev = u64::MAX;
            }
        }
    }

    fn battery(&mut self, ui: &mut Ui, snap: &Snapshot) {
        let Some(b) = snap.power.as_ref().and_then(|p| p.battery.clone()) else {
            ui.label("No battery information.");
            return;
        };
        let health = match (b.last_full_charge_capacity_mah, b.design_capacity_mah) {
            (Some(f), Some(d)) if d > 0 => format!("{:.0}%", f as f32 * 100.0 / d as f32),
            _ => "—".into(),
        };
        stat_row(
            ui,
            &[
                (
                    "Charge",
                    b.percentage.map(|p| format!("{p}%")).unwrap_or("—".into()),
                    (if b.charging == Some(true) {
                        "Charging"
                    } else if b.ac_present == Some(true) {
                        "Plugged in"
                    } else {
                        "On battery"
                    })
                    .into(),
                ),
                (
                    "Health",
                    health,
                    format!(
                        "{} / {} mAh",
                        b.last_full_charge_capacity_mah.unwrap_or(0),
                        b.design_capacity_mah.unwrap_or(0)
                    ),
                ),
                ("Cycles", b.cycle_count.map(|c| c.to_string()).unwrap_or("—".into()), String::new()),
                (
                    "Power",
                    b.watts().map(|w| format!("{:+.1} W", w)).unwrap_or("—".into()),
                    b.present_voltage_mv.map(|v| format!("{:.2} V", v as f32 / 1000.0)).unwrap_or_default(),
                ),
            ],
        );
        ui.add_space(12.0);

        let Some(draft) = self.drafts.battery.as_mut() else { return };
        let mut dirty = false;
        card(ui, |ui| {
            section_title(ui, "Charging");
            egui::Grid::new("bat_grid").num_columns(3).spacing([16.0, 10.0]).show(ui, |ui| {
                let s = draft.charge_limit_max_pct.get_or_insert(SettingU8 { enabled: false, value: 80 });
                ui.checkbox(&mut s.enabled, "Charge limit").changed().then(|| dirty = true);
                ui.add_enabled_ui(s.enabled, |ui| {
                    dirty |= ui.add(egui::Slider::new(&mut s.value, 25..=100).suffix(" %")).changed();
                });
                ui.label(
                    RichText::new(b.charge_limit_max_pct.map(|v| format!("EC: {v}%")).unwrap_or_default()).weak(),
                );
                ui.end_row();

                let r = draft.charge_rate_c.get_or_insert(SettingF32 { enabled: false, value: 0.5 });
                ui.checkbox(&mut r.enabled, "Charge rate limit").changed().then(|| dirty = true);
                ui.add_enabled_ui(r.enabled, |ui| {
                    dirty |= ui
                        .add(egui::Slider::new(&mut r.value, 0.05..=1.0).step_by(0.05).suffix(" C"))
                        .changed();
                });
                ui.label(RichText::new("1.0C ≈ no limit").weak());
                ui.end_row();

                let enabled = r.enabled;
                let th = draft.charge_rate_soc_threshold_pct.get_or_insert(0);
                ui.label("  …only above");
                ui.add_enabled_ui(enabled, |ui| {
                    dirty |= ui.add(egui::Slider::new(th, 0..=100).suffix(" %")).changed();
                });
                ui.label(RichText::new("0 = always").weak());
                ui.end_row();
            });
        });
        if dirty {
            self.drafts.battery_dirty = true;
        }
        if self.drafts.battery_dirty {
            ui.add_space(10.0);
            if let Some(action) = save_bar(ui) {
                if action {
                    let d = self.drafts.battery.as_ref().unwrap();
                    let _ = self.tx.send(Cmd::Patch(json!({ "battery": d })));
                }
                self.drafts.battery_dirty = false;
                self.drafts.rev = u64::MAX;
            }
        }

        ui.add_space(12.0);
        card(ui, |ui| {
            section_title(ui, "History");
            battery_plot(ui, &snap.battery_history);
        });
    }

    fn settings(&mut self, ui: &mut Ui, snap: &Snapshot) {
        let Some(cfg) = snap.config.clone() else { return };
        card(ui, |ui| {
            section_title(ui, "Service");
            egui::Grid::new("svc").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                ui.label("Telemetry interval");
                let mut poll = cfg.telemetry.poll_ms;
                let r = ui.add(egui::Slider::new(&mut poll, 500..=10_000).step_by(250.0).suffix(" ms"));
                if r.drag_stopped() || (r.changed() && !r.dragged()) {
                    let _ = self.tx.send(Cmd::Patch(json!({ "telemetry": { "poll_ms": poll } })));
                }
                ui.end_row();
                ui.label("Install updates automatically");
                let mut auto = cfg.updates.auto_install;
                if ui.checkbox(&mut auto, "").changed() {
                    let _ = self.tx.send(Cmd::Patch(json!({ "updates": { "auto_install": auto } })));
                }
                ui.end_row();
                if let Some(u) = &snap.update {
                    ui.label("Version");
                    if u.updates_enabled && u.latest_version != u.current_version {
                        ui.label(format!("{} → {} available", u.current_version, u.latest_version));
                    } else {
                        ui.label(format!("{} (up to date)", u.current_version));
                    }
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
            if ui.button("Refresh everything").clicked() {
                let _ = self.tx.send(Cmd::RefreshAll);
            }
        });
        ui.add_space(12.0);
        card(ui, |ui| {
            section_title(ui, "Device");
            egui::Grid::new("dev").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                let mut row = |k: &str, v: Option<String>| {
                    ui.label(RichText::new(k).weak());
                    ui.label(v.unwrap_or("—".into()));
                    ui.end_row();
                };
                let v = snap.versions.clone().unwrap_or_default();
                let s = snap.system.clone().unwrap_or_default();
                row("Mainboard", v.mainboard_type);
                row("BIOS", v.uefi_version.map(|x| format!("{x} ({})", v.uefi_release_date.unwrap_or_default())));
                row("EC", v.ec_build_version);
                row("framework_tool", v.tool_version);
                row("CPU", Some(s.cpu));
                row("dGPU", s.dgpu);
                row("Memory", Some(format!("{:.0} GB", s.memory_total_mb as f64 / 1024.0)));
                row("OS", Some(s.os));
            });
        });
    }
}

// ---------------- Widgets ----------------

fn card(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(ui.visuals().panel_fill)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
}

fn section_title(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(15.0).strong());
}

/// A row of equally sized stat tiles that always fits the window width.
fn stat_row(ui: &mut Ui, tiles: &[(&str, String, String)]) {
    ui.columns(tiles.len(), |cols| {
        for (col, (title, value, sub)) in cols.iter_mut().zip(tiles) {
            stat_tile(col, title, value, sub);
        }
    });
}

fn stat_tile(ui: &mut Ui, title: &str, value: &str, sub: &str) {
    egui::Frame::new()
        .fill(ui.visuals().panel_fill)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).small().weak());
            ui.label(RichText::new(value).size(22.0).strong());
            ui.label(RichText::new(if sub.is_empty() { " " } else { sub }).small().weak());
        });
}

fn segmented(ui: &mut Ui, selected: bool, label: &str) -> bool {
    let (fill, text) = if selected {
        (ACCENT, RichText::new(label).color(Color32::WHITE))
    } else {
        (ui.visuals().widgets.inactive.weak_bg_fill, RichText::new(label))
    };
    ui.add(
        egui::Button::new(text)
            .fill(fill)
            .corner_radius(CornerRadius::same(6))
            .min_size(Vec2::new(92.0, 28.0)),
    )
    .clicked()
}

/// Returns Some(true) = save, Some(false) = discard.
fn save_bar(ui: &mut Ui) -> Option<bool> {
    let mut out = None;
    ui.horizontal(|ui| {
        ui.label(RichText::new("You have unsaved changes").weak());
        if ui
            .add(egui::Button::new(RichText::new("Apply").color(Color32::WHITE)).fill(ACCENT).min_size(Vec2::new(80.0, 28.0)))
            .clicked()
        {
            out = Some(true);
        }
        if ui.add(egui::Button::new("Discard").min_size(Vec2::new(80.0, 28.0))).clicked() {
            out = Some(false);
        }
    });
    out
}

fn setting_row_u32(
    ui: &mut Ui,
    label: &str,
    s: &mut SettingU32,
    range: std::ops::RangeInclusive<u32>,
    suffix: &str,
    current: Option<u32>,
) -> bool {
    let mut dirty = ui.checkbox(&mut s.enabled, label).changed();
    ui.add_enabled_ui(s.enabled, |ui| {
        dirty |= ui.add(egui::Slider::new(&mut s.value, range).suffix(suffix)).changed();
    });
    ui.label(RichText::new(current.map(|c| format!("now {c}{suffix}")).unwrap_or_default()).weak());
    ui.end_row();
    dirty
}

fn setting_row_choice(ui: &mut Ui, label: &str, s: &mut SettingString, opts: &[String], current: Option<String>) -> bool {
    let mut dirty = ui.checkbox(&mut s.enabled, label).changed();
    ui.add_enabled_ui(s.enabled, |ui| {
        egui::ComboBox::from_id_salt(label).selected_text(s.value.clone()).show_ui(ui, |ui| {
            for o in opts {
                if ui.selectable_label(&s.value == o, o).clicked() {
                    s.value = o.clone();
                    dirty = true;
                }
            }
        });
    });
    ui.label(RichText::new(current.map(|c| format!("now {c}")).unwrap_or_default()).weak());
    ui.end_row();
    dirty
}

fn mode_label(m: FanMode) -> &'static str {
    match m {
        FanMode::Disabled => "Auto",
        FanMode::Manual => "Manual",
        FanMode::Curve => "Curve",
    }
}

// ---------------- Plots ----------------

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

fn temp_plot(ui: &mut Ui, samples: &[TelemetrySample], height: f32) {
    let now = samples.last().map(|s| s.ts_ms).unwrap_or(0);
    let names: Vec<String> = samples.last().map(|s| s.temps.keys().cloned().collect()).unwrap_or_default();
    time_plot("temps", height)
        .y_axis_formatter(|m, _| format!("{:.0}°", m.value))
        .include_y(30.0)
        .include_y(90.0)
        .show(ui, |pui| {
            for (i, name) in names.iter().enumerate() {
                let pts: PlotPoints = samples
                    .iter()
                    .filter_map(|s| s.temps.get(name).map(|v| [minutes_ago(s.ts_ms, now), *v as f64]))
                    .collect();
                pui.line(Line::new(pts).name(name).color(SERIES[i % SERIES.len()]).width(1.8_f32));
            }
        });
}

fn rpm_plot(ui: &mut Ui, samples: &[TelemetrySample], height: f32) {
    let now = samples.last().map(|s| s.ts_ms).unwrap_or(0);
    let fans = samples.last().map(|s| s.rpms.len()).unwrap_or(0);
    time_plot("rpm", height)
        .y_grid_spacer(egui_plot::uniform_grid_spacer(|_| [500.0, 1000.0, 2000.0]))
        .y_axis_formatter(|m, _| if m.value >= 1000.0 { format!("{:.0}k", m.value / 1000.0) } else { format!("{:.0}", m.value) })
        .include_y(0.0)
        .include_y(6000.0)
        .show(ui, |pui| {
            for f in 0..fans {
                let pts: PlotPoints = samples
                    .iter()
                    .filter_map(|s| s.rpms.get(f).map(|v| [minutes_ago(s.ts_ms, now), *v as f64]))
                    .collect();
                pui.line(Line::new(pts).name(format!("Fan {}", f + 1)).color(SERIES[(f + 1) % SERIES.len()]).width(1.8_f32));
            }
        });
}

fn battery_plot(ui: &mut Ui, samples: &[BatterySample]) {
    let now = samples.last().map(|s| s.ts_ms).unwrap_or(0);
    let hrs = |ts: i64| (ts - now) as f64 / 3_600_000.0;
    Plot::new("battery_hist")
        .height(200.0)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .show_background(false)
        .legend(Legend::default().position(Corner::LeftTop))
        .include_y(0.0)
        .include_y(100.0)
        .include_x(-1.0)
        .include_x(0.0)
        .x_axis_formatter(|m, _| if m.value.abs() < 0.01 { "now".into() } else { format!("{:.0}h", m.value) })
        .show(ui, |pui| {
            let pct: PlotPoints =
                samples.iter().filter_map(|s| s.charge_pct.map(|p| [hrs(s.ts_ms), p as f64])).collect();
            pui.line(Line::new(pct).name("Charge %").color(SERIES[2]).width(2.0_f32));
            let w: PlotPoints = samples.iter().filter_map(|s| s.watts.map(|p| [hrs(s.ts_ms), p as f64])).collect();
            pui.line(Line::new(w).name("Watts").color(SERIES[1]).width(1.2_f32));
        });
}

/// Interactive fan curve editor. Returns true when the curve changed.
fn curve_editor(ui: &mut Ui, curve: &mut CurveConfig, current_temp: Option<i32>, drag: &mut Option<usize>) -> bool {
    let mut changed = false;
    let pts_snapshot = curve.points.clone();
    let accent = ACCENT;

    let res = Plot::new("curve_editor")
        .height(260.0)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .allow_double_click_reset(false)
        .show_background(false)
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
            if let Some(t) = current_temp {
                pui.vline(VLine::new(t as f64).color(Color32::from_rgb(0xff, 0x8a, 0x3d)).width(1.5_f32).name(format!("{t}°C now")));
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
        let lo = if i == 0 { 20 } else { curve.points[i - 1][0] + 1 };
        let hi = if i + 1 == curve.points.len() { 100 } else { curve.points[i + 1][0] - 1 };
        let t = (p.x.round() as i64).clamp(lo as i64, hi.max(lo) as i64) as u32;
        let d = p.y.round().clamp(0.0, 100.0) as u32;
        if curve.points[i] != [t, d] {
            curve.points[i] = [t, d];
            changed = true;
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
