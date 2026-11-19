use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, Ui};
use serde_json::json;

use crate::api::*;
use crate::app::App;
use crate::state::Snapshot;
use crate::task::{finished, Task};
use crate::theme::{self, THEMES};
use crate::version::{self, is_newer};
use crate::widgets::{accent_button, button, card, group, section, status_line, switch, Icon};

/// How an update was started.
pub enum UpdateStart {
    /// By the app, with the installer visible.
    App(crate::updater::PendingUpdate),
    /// By the background service (fallback when the app can't reach GitHub).
    Service,
}

#[derive(Default)]
pub struct SettingsUi {
    /// Overview: the sensor shown in the single chart.
    pub chart_focus: Option<String>,
    chart_view: Option<crate::pages::overview::ChartView>,
    update_job: Option<Task<UpdateStart>>,
    update_msg: Option<(String, bool)>,
    /// An update started earlier that hasn't finished (read once from the app's own settings).
    pending: Option<Option<crate::updater::PendingUpdate>>,
    tool_versions: Option<FrameworkToolVersions>,
    tool_fetch: Option<Task<FrameworkToolVersions>>,
    tool_switch: Option<Task<()>>,
    tool_msg: Option<(String, bool)>,
    logs_open: bool,
    logs: Option<Result<String, String>>,
    logs_job: Option<Task<String>>,
}

impl SettingsUi {
    /// The Overview's temperature view, remembered in the app's own per-user settings.
    pub fn chart_view(&mut self) -> crate::pages::overview::ChartView {
        use crate::pages::overview::ChartView;
        *self.chart_view.get_or_insert_with(|| {
            match crate::launcher::LocalSettings::load().chart_view.as_deref() {
                Some("single") => ChartView::Single,
                _ => ChartView::Grouped,
            }
        })
    }

    pub fn set_chart_view(&mut self, view: crate::pages::overview::ChartView) {
        use crate::pages::overview::ChartView;
        self.chart_view = Some(view);
        let mut local = crate::launcher::LocalSettings::load();
        local.chart_view = Some(if view == ChartView::Single { "single" } else { "grouped" }.into());
        let _ = local.save();
    }

    pub fn open_logs(&mut self, backend: &std::sync::Arc<dyn Backend>, ctx: &egui::Context) {
        self.logs_open = true;
        let b = backend.clone();
        self.logs_job = Some(Task::spawn(ctx, move || b.logs()));
    }
}

pub fn show(app: &mut App, ui: &mut Ui, snap: &Snapshot) {
    poll_jobs(app, ui.ctx());
    let Some(cfg) = snap.config.clone() else {
        card(ui, Some(Icon::Settings), "Loading…", "Waiting for the service.", false, |_| {});
        return;
    };

    section(ui, "Appearance");
    card(
        ui,
        Some(Icon::Theme),
        "App theme",
        "Follows Windows' light or dark mode and accent colour, like White Rabbit.",
        false,
        |ui| {
            let current = cfg.ui.theme.clone().filter(|t| THEMES.iter().any(|(v, _)| v == t)).unwrap_or_else(|| "system".into());
            let label = THEMES.iter().find(|(v, _)| *v == current).map(|(_, l)| *l).unwrap_or("Use my Windows setting");
            egui::ComboBox::from_id_salt("theme").selected_text(label).width(220.0).show_ui(ui, |ui| {
                for (value, label) in THEMES {
                    if ui.selectable_label(current == value, label).clicked() {
                        app.patch(json!({ "ui": { "theme": value } }));
                    }
                }
            });
        },
    );

    section(ui, "Updates");
    updates(app, ui, snap, &cfg);

    section(ui, "Features");
    for (id, icon, label, help) in [
        (PanelId::Telemetry, Icon::Pulse, "Sensors", "Temperature and fan speed history."),
        (PanelId::Fan, Icon::Fan, "Fan control", "Fan profiles, manual speed and curves."),
        (PanelId::Power, Icon::Power, "Power", "TDP and thermal limits for plugged in and on battery."),
        (PanelId::Battery, Icon::Battery, "Battery", "Charge limits and charge history."),
    ] {
        let mut on = cfg.ui.panel_enabled(id);
        card(ui, Some(icon), label, &format!("{help} Off also stops the service's background work for it."), false, |ui| {
            if switch(ui, &mut on) {
                app.patch(json!({ "ui": { "panels": cfg.ui.with_panel(id, on) } }));
            }
        });
    }
    card(ui, Some(Icon::Timer), "Sensor sample interval", "How often the service reads temperatures and fan speeds.", false, |ui| {
        let mut poll = cfg.telemetry.poll_ms;
        let r = ui.add(egui::Slider::new(&mut poll, 500..=10_000).step_by(250.0).suffix(" ms"));
        if r.drag_stopped() || (r.changed() && !r.dragged()) {
            app.patch(json!({ "telemetry": { "poll_ms": poll } }));
        }
    });

    section(ui, "Hardware");
    framework_tool(app, ui, &cfg);
    if card(ui, Some(Icon::Logs), "Service logs", "The last 500 lines from the background service.", true, |_| {}).clicked() {
        app.settings.logs_open = true;
        fetch_logs(app, ui.ctx());
    }

    section(ui, "This laptop");
    let v = snap.versions.clone().unwrap_or_default();
    let sys = snap.system.clone().unwrap_or_default();
    group(ui, "", "", |ui| {
        egui::Grid::new("dev").num_columns(2).spacing([24.0, 6.0]).show(ui, |ui| {
            let c = theme::colors(ui.ctx());
            let mut row = |k: &str, val: Option<String>| {
                ui.label(RichText::new(k).color(c.text2));
                ui.label(RichText::new(val.unwrap_or("—".into())).color(c.text));
                ui.end_row();
            };
            row("Mainboard", v.mainboard_type);
            row("BIOS", v.uefi_version.map(|x| format!("{x} ({})", v.uefi_release_date.unwrap_or_default())));
            row("EC", v.ec_build_version);
            row("framework_tool", v.tool_version);
            row("CPU", Some(sys.cpu));
            row("Graphics", sys.dgpu);
            row("Memory", Some(format!("{:.0} GB", sys.memory_total_mb as f64 / 1024.0)));
            row("Windows", Some(sys.os));
        });
    });

    section(ui, "About");
    if card(
        ui,
        Some(Icon::Info),
        &format!("Framework Control {}", env!("CARGO_PKG_VERSION")),
        "A native Windows app built on Framework Control by Kemal Ozturk. Opens this app's page on GitHub.",
        true,
        |_| {},
    )
    .clicked()
    {
        ui.ctx().open_url(egui::OpenUrl::new_tab(format!("https://github.com/{}", version::REPO)));
    }
    if card(
        ui,
        Some(Icon::Heart),
        "Support the original project",
        "The service, fan curves and power logic come from Framework Control. Opens its page on GitHub.",
        true,
        |_| {},
    )
    .clicked()
    {
        ui.ctx().open_url(egui::OpenUrl::new_tab(version::UPSTREAM_URL));
    }
}

fn poll_jobs(app: &mut App, ctx: &egui::Context) {
    let s = &mut app.settings;
    if let Some(r) = finished(&mut s.update_job) {
        s.update_msg = Some(match r {
            Ok(start) => {
                if let UpdateStart::App(pending) = start {
                    // Remember it, so a failed install is reported the next time the app starts.
                    let mut local = crate::launcher::LocalSettings::load();
                    local.pending_update = Some(pending);
                    let _ = local.save();
                }
                // The installer replaces this exe, so get out of its way; a small watcher reopens it.
                app.close_at = Some(Instant::now() + Duration::from_secs(3));
                let reopen = crate::launcher::reopen_after_update();
                let msg = if reopen.is_ok() {
                    "Installing. Framework Control will close now and reopen by itself when the update is done."
                } else {
                    "Installing. Framework Control will close now; open it again from the Start menu when the installer finishes."
                };
                (msg.into(), false)
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
    let busy = app.settings.update_job.is_some() || app.close_at.is_some();
    unfinished_update(app, ui);
    match &snap.update {
        None => {
            card(ui, Some(Icon::Sync), "Framework Control", "Checking GitHub for the newest version…", false, |ui| {
                ui.spinner();
            });
        }
        Some(u) if !u.updates_enabled => {
            card(ui, Some(Icon::Sync), "Framework Control", &format!("Version {}. In-app updates aren't available for this install.", u.current_version), false, |_| {});
        }
        Some(u) => {
            let newer = is_newer(&u.latest_version, &u.current_version);
            let help = if newer {
                format!("Version {} is available (you have {}). Your settings are kept.", u.latest_version, u.current_version)
            } else {
                format!("You have the newest version ({}).", u.current_version)
            };
            card(ui, Some(if newer { Icon::Download } else { Icon::Check }), "Framework Control", &help, false, |ui| {
                if newer {
                    ui.add_enabled_ui(!busy, |ui| {
                        if accent_button(ui, if busy { "Downloading…" } else { "Install update" }).clicked() {
                            let b = app.backend.clone();
                            app.settings.update_job = Some(Task::spawn(ui.ctx(), move || {
                                // Download and run the installer here, visibly; if GitHub can't be reached
                                // from the app, let the background service do it.
                                match crate::updater::download_and_run() {
                                    Ok(p) => Ok(UpdateStart::App(p)),
                                    Err(e) => b
                                        .apply_update()
                                        .map(|_| UpdateStart::Service)
                                        .map_err(|e2| format!("{e} (and the service couldn't either: {e2})")),
                                }
                            }));
                        }
                    });
                } else if button(ui, "Check for updates").clicked() {
                    app.refresh();
                }
            });
            let mut auto = cfg.updates.auto_install;
            card(
                ui,
                Some(Icon::Sync),
                "Keep up to date automatically",
                "The service checks GitHub every 6 hours and installs new versions in the background, with no prompt.",
                false,
                |ui| {
                    if switch(ui, &mut auto) {
                        app.patch(json!({ "updates": { "auto_install": auto } }));
                    }
                },
            );
        }
    }
    if card(ui, Some(Icon::Open), "Release notes and downloads", "What's new in each version, on GitHub.", true, |_| {}).clicked() {
        ui.ctx().open_url(egui::OpenUrl::new_tab(version::RELEASES_URL));
    }
    status_line(ui, &app.settings.update_msg);
}

/// An update the app started that hasn't finished (read once per run from the app's own settings).
pub fn unfinished(app: &mut App) -> Option<crate::updater::PendingUpdate> {
    let current = env!("CARGO_PKG_VERSION");
    app.settings
        .pending
        .get_or_insert_with(|| {
            let mut local = crate::launcher::LocalSettings::load();
            match local.pending_update.clone() {
                // Done: this is that version (or newer). Forget it.
                Some(p) if !is_newer(&p.version, current) => {
                    local.pending_update = None;
                    let _ = local.save();
                    None
                }
                other => other,
            }
        })
        .clone()
}

/// If an update the app started didn't finish, say so and offer to try again or open the log.
fn unfinished_update(app: &mut App, ui: &mut Ui) {
    let Some(p) = unfinished(app) else { return };
    if app.settings.update_job.is_some() || app.close_at.is_some() {
        return;
    }
    let mut clear = false;
    card(
        ui,
        Some(Icon::Warning),
        &format!("The update to {} didn't finish", p.version),
        "The installer was cancelled or stopped. Try again, or open its log to see why.",
        false,
        |ui| {
            if accent_button(ui, "Try again").clicked() {
                if let Err(e) = crate::updater::run_installer(&p.msi, None) {
                    app.settings.update_msg = Some((e, true));
                }
            }
            if p.log.is_file() && button(ui, "Open the log").clicked() {
                let _ = crate::updater::open_log(&p.log);
            }
            if button(ui, "Dismiss").clicked() {
                clear = true;
            }
        },
    );
    if clear {
        let mut local = crate::launcher::LocalSettings::load();
        local.pending_update = None;
        let _ = local.save();
        app.settings.pending = Some(None);
    }
}

fn framework_tool(app: &mut App, ui: &mut Ui, cfg: &Config) {
    let s = &mut app.settings;
    let busy = s.tool_switch.is_some();
    let mut choice: Option<Option<String>> = None;
    let mut help = "The tool the service uses to talk to the laptop. Picking a version installs it.".to_string();
    if let Some(v) = &s.tool_versions {
        let current_tag = v.current_version.as_ref().map(|c| if c.starts_with('v') { c.clone() } else { format!("v{c}") });
        if cfg.framework_tool.latest && v.latest_tag.is_some() && current_tag.is_some() && v.latest_tag != current_tag {
            help = format!("Latest is chosen but {} is running.", current_tag.clone().unwrap_or_default());
        }
    }
    card(ui, Some(Icon::Board), "framework_tool version", &help, false, |ui| match &s.tool_versions {
        None => {
            ui.spinner();
        }
        Some(v) => {
            let current_tag = v.current_version.as_ref().map(|c| if c.starts_with('v') { c.clone() } else { format!("v{c}") });
            let latest_label = format!("Latest{}", v.latest_tag.as_ref().map(|t| format!(" ({t})")).unwrap_or_default());
            let selected = if cfg.framework_tool.latest { latest_label.clone() } else { current_tag.clone().unwrap_or("Unknown".into()) };
            if busy {
                ui.spinner();
            }
            ui.add_enabled_ui(!busy, |ui| {
                egui::ComboBox::from_id_salt("tool").selected_text(selected).width(200.0).show_ui(ui, |ui| {
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
        }
    });
    if let Some(c) = choice {
        let b = app.backend.clone();
        s.tool_msg = None;
        s.tool_switch = Some(Task::spawn(ui.ctx(), move || b.switch_framework_tool(c)));
    }
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
        .frame(egui::Frame::window(&ctx.style()).inner_margin(egui::Margin::same(16)))
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
                    ui.label(RichText::new(format!("Couldn't load logs: {e}")).color(crate::theme::colors(ui.ctx()).error));
                }
            });
        });
    if refresh {
        fetch_logs(app, ctx);
    }
    app.settings.logs_open = open;
}
