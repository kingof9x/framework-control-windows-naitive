use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, Ui};
use serde_json::json;

use crate::api::*;
use crate::app::App;
use crate::state::Snapshot;
use crate::task::{finished, Task};
use crate::theme::THEMES;
use crate::version::{self, is_newer};
use crate::widgets::{card, hint, section_title, status_line};

#[derive(Default)]
pub struct SettingsUi {
    /// Overview temperature graph filter (empty = all sensors).
    pub sensor_filter: Vec<String>,
    update_job: Option<Task<()>>,
    update_msg: Option<(String, bool)>,
    tool_versions: Option<FrameworkToolVersions>,
    tool_fetch: Option<Task<FrameworkToolVersions>>,
    tool_switch: Option<Task<()>>,
    tool_msg: Option<(String, bool)>,
    logs_open: bool,
    logs: Option<Result<String, String>>,
    logs_job: Option<Task<String>>,
}

impl SettingsUi {
    pub fn open_logs(&mut self, backend: &std::sync::Arc<dyn Backend>, ctx: &egui::Context) {
        self.logs_open = true;
        let b = backend.clone();
        self.logs_job = Some(Task::spawn(ctx, move || b.logs()));
    }
}

pub fn show(app: &mut App, ui: &mut Ui, snap: &Snapshot) {
    poll_jobs(app, ui.ctx());
    let Some(cfg) = snap.config.clone() else {
        ui.label("Loading…");
        return;
    };

    card(ui, |ui| updates(app, ui, snap, &cfg));
    ui.add_space(12.0);

    card(ui, |ui| {
        section_title(ui, "Appearance");
        ui.horizontal(|ui| {
            ui.label("Theme");
            let current = cfg.ui.theme.clone().unwrap_or_else(|| "system".into());
            let label = THEMES.iter().find(|(v, _)| *v == current).map(|(_, l)| *l).unwrap_or("System");
            egui::ComboBox::from_id_salt("theme").selected_text(label).show_ui(ui, |ui| {
                for (value, label) in THEMES {
                    if ui.selectable_label(current == value, label).clicked() {
                        app.patch(json!({ "ui": { "theme": value } }));
                    }
                }
            });
            hint(ui, "Shared with the web UI.");
        });
    });
    ui.add_space(12.0);

    card(ui, |ui| {
        section_title(ui, "Features");
        hint(ui, "Turning a feature off hides it and stops the service's background work for it.");
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            for (id, label) in [
                (PanelId::Telemetry, "Sensors & telemetry"),
                (PanelId::Fan, "Fan control"),
                (PanelId::Power, "Power"),
                (PanelId::Battery, "Battery"),
            ] {
                let mut on = cfg.ui.panel_enabled(id);
                if ui.checkbox(&mut on, label).changed() {
                    app.patch(json!({ "ui": { "panels": cfg.ui.with_panel(id, on) } }));
                }
                ui.add_space(10.0);
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Sensor sample interval");
            let mut poll = cfg.telemetry.poll_ms;
            let r = ui.add(egui::Slider::new(&mut poll, 500..=10_000).step_by(250.0).suffix(" ms"));
            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                app.patch(json!({ "telemetry": { "poll_ms": poll } }));
            }
        });
    });
    ui.add_space(12.0);

    card(ui, |ui| framework_tool(app, ui, &cfg));
    ui.add_space(12.0);

    card(ui, |ui| {
        section_title(ui, "Logs");
        ui.horizontal(|ui| {
            hint(ui, "The last 500 lines from the background service.");
            if ui.button("View logs").clicked() {
                app.settings.logs_open = true;
                fetch_logs(app, ui.ctx());
            }
        });
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
    ui.add_space(12.0);

    card(ui, |ui| {
        section_title(ui, "About");
        ui.horizontal_wrapped(|ui| {
            ui.label("Native Windows front end for");
            ui.hyperlink_to("Framework Control", version::UPSTREAM_URL);
            ui.label("by Kemal Ozturk. Releases:");
            ui.hyperlink_to(version::REPO, version::RELEASES_URL);
        });
    });
}

fn poll_jobs(app: &mut App, ctx: &egui::Context) {
    let s = &mut app.settings;
    if let Some(r) = finished(&mut s.update_job) {
        s.update_msg = Some(match r {
            Ok(()) => {
                // The installer replaces this exe, so get out of its way.
                app.close_at = Some(Instant::now() + Duration::from_secs(4));
                ("Installer started. This window will close; reopen Framework Control from the Start menu when it finishes.".into(), false)
            }
            Err(e) => (format!("Update failed: {e}"), true),
        });
    }
    if let Some(r) = finished(&mut s.tool_fetch) {
        match r {
            Ok(v) => s.tool_versions = Some(v),
            Err(e) => s.tool_msg = Some((format!("Couldn't list versions: {e}"), true)),
        }
    }
    if let Some(r) = finished(&mut s.tool_switch) {
        s.tool_msg = Some(match r {
            Ok(()) => ("Switched. The service is restarting framework_tool.".into(), false),
            Err(e) => (e, true),
        });
        s.tool_versions = None;
        let _ = app.tx.send(crate::state::Cmd::RefreshAll);
    }
    if let Some(r) = finished(&mut s.logs_job) {
        s.logs = Some(r);
    }
    if s.tool_versions.is_none() && s.tool_fetch.is_none() && s.tool_switch.is_none() && s.tool_msg.is_none() {
        let b = app.backend.clone();
        s.tool_fetch = Some(Task::spawn(ctx, move || b.framework_tool_versions()));
    }
}

fn updates(app: &mut App, ui: &mut Ui, snap: &Snapshot, cfg: &Config) {
    section_title(ui, "Updates");
    match &snap.update {
        None => hint(ui, "Checking GitHub for the latest release…"),
        Some(u) if !u.updates_enabled => {
            ui.label(format!("Version {}", u.current_version));
            hint(ui, "In-app updates are not available for this install.");
        }
        Some(u) => {
            let newer = is_newer(&u.latest_version, &u.current_version);
            ui.horizontal(|ui| {
                if newer {
                    ui.label(
                        RichText::new(format!("v{} → v{}", u.current_version, u.latest_version))
                            .color(crate::theme::warning(ui.visuals().dark_mode)),
                    );
                    let busy = app.settings.update_job.is_some() || app.close_at.is_some();
                    if ui.add_enabled(!busy, egui::Button::new(if busy { "Installing…" } else { "Install" })).clicked() {
                        let b = app.backend.clone();
                        app.settings.update_job = Some(Task::spawn(ui.ctx(), move || b.apply_update()));
                    }
                } else {
                    ui.label(format!("Up to date · v{}", u.current_version));
                }
                if ui.button("Check now").clicked() {
                    app.refresh();
                }
            });
            if newer {
                hint(ui, "Installs the latest release from GitHub. Your settings are kept.");
            }
            let mut auto = cfg.updates.auto_install;
            if ui.checkbox(&mut auto, "Automatically keep up to date").changed() {
                app.patch(json!({ "updates": { "auto_install": auto } }));
            }
        }
    }
    ui.horizontal(|ui| {
        ui.hyperlink_to("Release notes and downloads", version::RELEASES_URL);
    });
    status_line(ui, &app.settings.update_msg);
}

fn framework_tool(app: &mut App, ui: &mut Ui, cfg: &Config) {
    section_title(ui, "framework_tool version");
    hint(ui, "Selecting a version installs it and switches the service to it.");
    let s = &mut app.settings;
    let busy = s.tool_switch.is_some();
    ui.horizontal(|ui| match &s.tool_versions {
        None => {
            ui.spinner();
        }
        Some(v) => {
            let current_tag = v.current_version.as_ref().map(|c| if c.starts_with('v') { c.clone() } else { format!("v{c}") });
            let latest_label = format!("Latest{}", v.latest_tag.as_ref().map(|t| format!(" ({t})")).unwrap_or_default());
            let selected = if cfg.framework_tool.latest {
                latest_label.clone()
            } else {
                current_tag.clone().unwrap_or("Unknown".into())
            };
            let mut choice: Option<Option<String>> = None;
            ui.add_enabled_ui(!busy, |ui| {
                egui::ComboBox::from_id_salt("tool").selected_text(selected).width(180.0).show_ui(ui, |ui| {
                    if ui.selectable_label(cfg.framework_tool.latest, &latest_label).clicked() {
                        choice = Some(None);
                    }
                    for t in &v.available_tags {
                        let sel = !cfg.framework_tool.latest && current_tag.as_deref() == Some(t);
                        if ui.selectable_label(sel, t).clicked() {
                            choice = Some(Some(t.clone()));
                        }
                    }
                });
            });
            if busy {
                ui.spinner();
                ui.label("Installing…");
            }
            if cfg.framework_tool.latest && v.latest_tag.is_some() && current_tag.is_some() && v.latest_tag != current_tag {
                ui.label(
                    RichText::new(format!("Latest is enabled but running {}", current_tag.clone().unwrap_or_default()))
                        .color(crate::theme::warning(ui.visuals().dark_mode)),
                );
            }
            if let Some(c) = choice {
                let b = app.backend.clone();
                s.tool_msg = None;
                s.tool_switch = Some(Task::spawn(ui.ctx(), move || b.switch_framework_tool(c)));
            }
        }
    });
    status_line(ui, &s.tool_msg);
}

fn fetch_logs(app: &mut App, ctx: &egui::Context) {
    if app.settings.logs_job.is_none() {
        let b = app.backend.clone();
        app.settings.logs_job = Some(Task::spawn(ctx, move || b.logs()));
    }
}

/// Floating windows owned by the settings page (logs viewer).
pub fn windows(app: &mut App, ctx: &egui::Context) {
    if !app.settings.logs_open {
        return;
    }
    if let Some(r) = finished(&mut app.settings.logs_job) {
        app.settings.logs = Some(r);
    }
    let mut open = true;
    let mut refresh = false;
    egui::Window::new("Service logs")
        .open(&mut open)
        .default_size([760.0, 480.0])
        .collapsible(false)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                refresh = ui.button("Refresh").clicked();
                if let Some(Ok(text)) = &app.settings.logs {
                    if ui.button("Copy all").clicked() {
                        ui.ctx().copy_text(text.clone());
                    }
                }
                if app.settings.logs_job.is_some() {
                    ui.spinner();
                }
            });
            ui.separator();
            egui::ScrollArea::both().stick_to_bottom(true).auto_shrink(false).show(ui, |ui| match &app.settings.logs {
                None => {
                    ui.spinner();
                }
                Some(Ok(text)) => {
                    ui.add(egui::Label::new(RichText::new(text).monospace()).wrap_mode(egui::TextWrapMode::Extend));
                }
                Some(Err(e)) => {
                    ui.label(RichText::new(format!("Couldn't load logs: {e}")).color(crate::theme::error(ui.visuals().dark_mode)));
                }
            });
        });
    if refresh {
        fetch_logs(app, ctx);
    }
    app.settings.logs_open = open;
}
