//! Launcher for the companion Framework 16 tools (White Rabbit LED Matrix
//! app, Framework 16 Keyboard Control).

use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, Ui};

use crate::app::App;
use crate::launcher::{self, Found, LocalSettings, TOOLS};
use crate::task::{finished, Task};
use crate::theme;
use crate::tool_updates::{self, Latest, Status};
use crate::widgets::{accent_button, button, card, section, status_line, Icon};

/// The newest release of each tool, or why it couldn't be checked.
type Releases = Vec<Result<Option<Latest>, String>>;

const RUNNING_CHECK_EVERY: Duration = Duration::from_secs(3);
/// How long tool locations are trusted before looking on disk again.
const LOCATE_EVERY: Duration = Duration::from_secs(5);

#[derive(Default)]
pub struct ToolsUi {
    /// Sample data mode: made-up release info instead of asking GitHub.
    demo: bool,
    settings: Option<LocalSettings>,
    running: Vec<String>,
    running_job: Option<Task<Vec<String>>>,
    last_check: Option<Instant>,
    msg: Option<(String, bool)>,
    /// Where each tool in TOOLS was found, refreshed every few seconds.
    located: Option<(Instant, Vec<Found>)>,
    /// The newest release of each tool (same order as TOOLS), and when it was checked.
    releases: Option<(Instant, Releases)>,
    release_job: Option<Task<Releases>>,
    install_job: Option<Task<String>>,
    cleaned_up: bool,
}

impl ToolsUi {
    pub fn new(demo: bool) -> Self {
        Self { demo, ..Default::default() }
    }
}

impl ToolsUi {
    fn check_running(&mut self, ctx: &egui::Context) {
        if self.running_job.is_none() {
            let exes: Vec<&'static str> = TOOLS.iter().flat_map(|t| std::iter::once(t.exe).chain(t.old_exes.iter().copied())).collect();
            self.running_job = Some(Task::spawn(ctx, move || Ok(launcher::running(&exes))));
            self.last_check = Some(Instant::now());
        }
    }

    fn is_running(&self, exe: &str) -> bool {
        self.running.iter().any(|r| r.eq_ignore_ascii_case(exe))
    }

    /// Where each tool is (same order as TOOLS), cached for a few seconds.
    fn locate_all(&mut self) -> Vec<Found> {
        if let Some((t, found)) = &self.located {
            if t.elapsed() < LOCATE_EVERY {
                return found.clone();
            }
        }
        let settings = self.settings.get_or_insert_with(LocalSettings::load);
        let found: Vec<Found> = TOOLS.iter().map(|tool| launcher::locate(tool, settings)).collect();
        self.located = Some((Instant::now(), found.clone()));
        found
    }

    /// Check GitHub for new versions now and then (and right away when `now`).
    fn check_releases(&mut self, ctx: &egui::Context, now: bool) {
        if let Some(Ok(r)) = finished(&mut self.release_job) {
            self.releases = Some((Instant::now(), r));
        }
        let due = self.releases.as_ref().is_none_or(|(t, _)| t.elapsed() >= tool_updates::CHECK_EVERY);
        if self.demo {
            if self.releases.is_none() {
                let sample = |v: &str| Latest { version: v.into(), page: String::new(), asset_url: None, published: None };
                self.releases = Some((Instant::now(), vec![Ok(Some(sample("1.6"))), Ok(None)]));
            }
            return;
        }
        if (due || now) && self.release_job.is_none() {
            self.release_job = Some(Task::spawn(ctx, || {
                Ok(TOOLS.iter().map(|t| tool_updates::latest(t.repo, t.update_asset())).collect())
            }));
        }
        if let Some(r) = finished(&mut self.install_job) {
            self.msg = Some(match r {
                Ok(m) => (m, false),
                Err(e) => (e, true),
            });
            self.located = None;
        }
    }

    /// Each tool's update status (same order as TOOLS), once the check has finished.
    fn statuses(&mut self, found: &[Found]) -> Vec<Option<Status>> {
        let Some((_, releases)) = &self.releases else { return vec![None; TOOLS.len()] };
        let settings = LocalSettings::load();
        TOOLS
            .iter()
            .zip(found)
            .zip(releases)
            .map(|((tool, f), r)| Some(tool_updates::status(tool, f.path(), &settings, r.clone())))
            .collect()
    }

    fn install_update(&mut self, ctx: &egui::Context, idx: usize, path: &std::path::Path, latest: &Latest) {
        let tool = &TOOLS[idx];
        let running = std::iter::once(tool.exe).chain(tool.old_exes.iter().copied()).any(|e| self.is_running(e));
        let (path, latest) = (path.to_path_buf(), latest.clone());
        self.msg = Some((format!("Downloading {} {}…", tool.name, latest.version), false));
        self.install_job = Some(Task::spawn(ctx, move || tool_updates::install(&TOOLS[idx], &path, &latest, running)));
        // Ask GitHub again afterwards so the status catches up.
        self.releases = None;
    }

    fn launch(&mut self, tool: &launcher::ToolDef, path: &std::path::Path, args: &[&str]) {
        self.msg = Some(match launcher::launch(path, args) {
            Ok(()) => (format!("Started {}.", tool.name), false),
            Err(e) => (e, true),
        });
        // Re-check soon so the status catches up with the new process.
        self.last_check = Some(Instant::now() - RUNNING_CHECK_EVERY + Duration::from_millis(1500));
    }
}

fn tool_icon(tool: &launcher::ToolDef) -> Icon {
    match tool.id {
        "white-rabbit" => Icon::Panels,
        _ => Icon::Keyboard,
    }
}

/// "Your tools" at the top of the Overview: one card per tool that is installed or was picked with
/// Browse. Nothing is shown when neither is found.
pub fn quick_launch(app: &mut App, ui: &mut Ui) {
    let t = &mut app.tools;
    let ctx = ui.ctx().clone();
    t.check_releases(&ctx, false);
    let found = t.locate_all();
    let statuses = t.statuses(&found);
    let available: Vec<(&launcher::ToolDef, std::path::PathBuf, Option<Status>)> = TOOLS
        .iter()
        .zip(found)
        .zip(statuses)
        .filter_map(|((tool, f), s)| f.path().map(|p| (tool, p.to_path_buf(), s)))
        .collect();
    if available.is_empty() {
        return;
    }
    section(ui, "Your tools");
    let mut clicked: Option<usize> = None;
    ui.columns(available.len().max(2), |cols| {
        for (i, (tool, _, status)) in available.iter().enumerate() {
            let help = match tool.id {
                "white-rabbit" => "Open the LED Matrix app",
                _ => "Open keyboard and macropad lighting",
            };
            let help = match status.as_ref().and_then(Status::update_available) {
                Some(l) => format!("{help}  ·  Version {} is available on the Tools page", l.version),
                None => help.to_string(),
            };
            if card(&mut cols[i], Some(tool_icon(tool)), tool.name, &help, true, |_| {}).clicked() {
                clicked = Some(i);
            }
        }
    });
    if let Some(i) = clicked {
        let (tool, path, _) = &available[i];
        t.launch(tool, path, tool.actions[0].args);
    }
    status_line(ui, &t.msg);
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let t = &mut app.tools;
    let ctx = ui.ctx().clone();
    t.settings.get_or_insert_with(LocalSettings::load);
    if let Some(Ok(r)) = finished(&mut t.running_job) {
        t.running = r;
    }
    if t.last_check.is_none_or(|c| c.elapsed() >= RUNNING_CHECK_EVERY) {
        t.check_running(&ctx);
    }
    ctx.request_repaint_after(RUNNING_CHECK_EVERY);
    t.check_releases(&ctx, false);

    section(ui, "Your Framework 16 tools");
    crate::widgets::hint(ui, "They run on their own, so closing Framework Control leaves them running.");
    ui.add_space(4.0);

    let located = t.locate_all();
    if !t.cleaned_up {
        t.cleaned_up = true;
        located.iter().filter_map(Found::path).for_each(tool_updates::clean_up);
    }
    let statuses = t.statuses(&located);
    let installing = t.install_job.is_some();
    let mut start_update: Option<(usize, std::path::PathBuf, Latest)> = None;
    for (idx, ((tool, found), upd)) in TOOLS.iter().zip(located).zip(statuses).enumerate() {
        let running = std::iter::once(tool.exe).chain(tool.old_exes.iter().copied()).any(|e| t.is_running(e));
        let icon = tool_icon(tool);
        let c = theme::colors(ui.ctx());
        let status = match (&found, running) {
            (_, true) => "Running",
            (Found::Missing, _) => "Not found on this PC",
            _ => "Installed",
        };
        let version = match &upd {
            Some(Status::UpToDate { installed: Some(v), .. }) => format!(" (version {v}), up to date"),
            Some(Status::UpToDate { installed: None, .. }) => ", up to date".to_string(),
            Some(Status::Available { installed: Some(v), latest }) => format!(" (version {v}). Version {} is available", latest.version),
            Some(Status::Available { installed: None, latest }) => format!(". Version {} is available", latest.version),
            Some(Status::NotInstalled { latest }) => format!(". Newest version: {}", latest.version),
            Some(Status::NoReleases) => ". No releases published yet".to_string(),
            Some(Status::Failed) => ". Couldn't check for updates".to_string(),
            None => String::new(),
        };
        let help = format!("{}  ·  {status}{version}", tool.description);
        let available = upd.as_ref().and_then(Status::update_available).cloned();
        card(ui, Some(icon), tool.name, &help, false, |ui| match found.path() {
            Some(path) => {
                if let Some(latest) = &available {
                    let r = ui.add_enabled_ui(!installing, |ui| accent_button(ui, &format!("Update to {}", latest.version))).inner;
                    if r.clicked() {
                        start_update = Some((idx, path.to_path_buf(), latest.clone()));
                    }
                }
                for (n, action) in tool.actions.iter().enumerate() {
                    let r = if n == 0 && available.is_none() { accent_button(ui, action.label) } else { button(ui, action.label) };
                    let r = if action.hint.is_empty() { r } else { r.on_hover_text(action.hint) };
                    if r.clicked() {
                        t.launch(tool, path, action.args);
                    }
                }
            }
            None => {
                if accent_button(ui, tool.download_label).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(tool.download_url));
                }
            }
        });
        // Where it is, and how to point at a different copy.
        ui.horizontal(|ui| {
            ui.add_space(52.0);
            match found.path() {
                Some(p) => {
                    ui.label(RichText::new(p.display().to_string()).size(12.0).color(c.text3));
                }
                None => {
                    ui.label(RichText::new(format!("Looked for {}.", tool.exe)).size(12.0).color(c.text3));
                }
            }
            if cfg!(windows) && ui.link(RichText::new(if found.path().is_some() { "Change" } else { "I already have it" }).size(12.0)).clicked() {
                if let Some(p) = launcher::browse_for(tool) {
                    remember(t, tool.id, Some(p));
                }
            }
            if matches!(found, Found::Chosen(_)) && ui.link(RichText::new("Use default location").size(12.0)).clicked() {
                remember(t, tool.id, None);
            }
            ui.hyperlink_to(RichText::new("GitHub").size(12.0), tool.project_url);
        });
        ui.add_space(8.0);
    }
    if let Some((idx, path, latest)) = start_update {
        t.install_update(&ctx, idx, &path, &latest);
    }
    status_line(ui, &t.msg);

    section(ui, "Updates");
    let checking = t.release_job.is_some();
    let help = match (&t.releases, checking) {
        (_, true) => "Checking GitHub…".to_string(),
        (Some((at, r)), _) => {
            let mins = at.elapsed().as_secs() / 60;
            let when = if mins < 1 { "just now".to_string() } else if mins < 120 { format!("{mins} min ago") } else { format!("{} hours ago", mins / 60) };
            match r.iter().find_map(|x| x.as_ref().err()) {
                Some(e) => format!("Checked {when}. {e}."),
                None => format!("Checked {when}. Framework Control checks each tool's GitHub releases every 6 hours while it's open."),
            }
        }
        (None, false) => "Not checked yet.".to_string(),
    };
    card(ui, Some(Icon::Sync), "Check for tool updates", &help, false, |ui| {
        if ui.add_enabled_ui(!checking, |ui| button(ui, "Check now")).inner.clicked() {
            t.check_releases(&ctx, true);
        }
    });

    section(ui, "On GitHub");
    for tool in TOOLS {
        let help = format!("Source code, releases and instructions for {}.", tool.name);
        if card(ui, Some(Icon::Open), &format!("{} project", tool.name), &help, true, |_| {}).clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(tool.project_url));
        }
    }
}

fn remember(t: &mut ToolsUi, id: &str, path: Option<std::path::PathBuf>) {
    // Start from the file, not the cached copy, so settings saved elsewhere since aren't lost.
    let mut s = LocalSettings::load();
    match path {
        Some(p) => {
            s.tool_paths.insert(id.to_string(), p);
        }
        None => {
            s.tool_paths.remove(id);
        }
    }
    if let Err(e) = s.save() {
        t.msg = Some((format!("Couldn't save the tool location: {e}"), true));
    }
    t.settings = Some(s);
    t.located = None;
}
