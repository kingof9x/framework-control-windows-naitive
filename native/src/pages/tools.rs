//! Launcher for the companion Framework 16 tools (White Rabbit LED Matrix
//! app, Framework 16 Keyboard Control).

use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, Ui};

use crate::app::App;
use crate::launcher::{self, Found, LocalSettings, TOOLS};
use crate::task::{finished, Task};
use crate::theme;
use crate::widgets::{accent_button, button, card, section, status_line, Icon};

const RUNNING_CHECK_EVERY: Duration = Duration::from_secs(3);
/// How long tool locations are trusted before looking on disk again.
const LOCATE_EVERY: Duration = Duration::from_secs(5);

#[derive(Default)]
pub struct ToolsUi {
    settings: Option<LocalSettings>,
    running: Vec<String>,
    running_job: Option<Task<Vec<String>>>,
    last_check: Option<Instant>,
    msg: Option<(String, bool)>,
    /// Where each tool in TOOLS was found, refreshed every few seconds.
    located: Option<(Instant, Vec<Found>)>,
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
    let found = t.locate_all();
    let available: Vec<(&launcher::ToolDef, std::path::PathBuf)> = TOOLS
        .iter()
        .zip(found)
        .filter_map(|(tool, f)| f.path().map(|p| (tool, p.to_path_buf())))
        .collect();
    if available.is_empty() {
        return;
    }
    section(ui, "Your tools");
    let mut clicked: Option<usize> = None;
    ui.columns(available.len().max(2), |cols| {
        for (i, (tool, _)) in available.iter().enumerate() {
            let help = match tool.id {
                "white-rabbit" => "Open the LED Matrix app",
                _ => "Open keyboard and macropad lighting",
            };
            if card(&mut cols[i], Some(tool_icon(tool)), tool.name, help, true, |_| {}).clicked() {
                clicked = Some(i);
            }
        }
    });
    if let Some(i) = clicked {
        let (tool, path) = &available[i];
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

    section(ui, "Your Framework 16 tools");
    crate::widgets::hint(ui, "They run on their own, so closing Framework Control leaves them running.");
    ui.add_space(4.0);

    let located = t.locate_all();
    for (tool, found) in TOOLS.iter().zip(located) {
        let running = std::iter::once(tool.exe).chain(tool.old_exes.iter().copied()).any(|e| t.is_running(e));
        let icon = tool_icon(tool);
        let c = theme::colors(ui.ctx());
        let status = match (&found, running) {
            (_, true) => "Running",
            (Found::Missing, _) => "Not found on this PC",
            _ => "Installed",
        };
        let help = format!("{}  ·  {status}", tool.description);
        card(ui, Some(icon), tool.name, &help, false, |ui| match found.path() {
            Some(path) => {
                for (n, action) in tool.actions.iter().enumerate() {
                    let r = if n == 0 { accent_button(ui, action.label) } else { button(ui, action.label) };
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
    status_line(ui, &t.msg);

    section(ui, "On GitHub");
    for tool in TOOLS {
        let help = format!("Source code, releases and instructions for {}.", tool.name);
        if card(ui, Some(Icon::Open), &format!("{} project", tool.name), &help, true, |_| {}).clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(tool.project_url));
        }
    }
}

fn remember(t: &mut ToolsUi, id: &str, path: Option<std::path::PathBuf>) {
    let s = t.settings.get_or_insert_with(LocalSettings::load);
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
    t.located = None;
}
