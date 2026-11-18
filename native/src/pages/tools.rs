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

#[derive(Default)]
pub struct ToolsUi {
    settings: Option<LocalSettings>,
    running: Vec<String>,
    running_job: Option<Task<Vec<String>>>,
    last_check: Option<Instant>,
    msg: Option<(String, bool)>,
}

impl ToolsUi {
    fn check_running(&mut self, ctx: &egui::Context) {
        if self.running_job.is_none() {
            let exes: Vec<&'static str> = TOOLS.iter().map(|t| t.exe).collect();
            self.running_job = Some(Task::spawn(ctx, move || Ok(launcher::running(&exes))));
            self.last_check = Some(Instant::now());
        }
    }

    fn is_running(&self, exe: &str) -> bool {
        self.running.iter().any(|r| r.eq_ignore_ascii_case(exe))
    }
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

    for tool in TOOLS {
        let found = launcher::locate(tool, t.settings.as_ref().unwrap());
        let running = t.is_running(tool.exe);
        let icon = match tool.id {
            "white-rabbit" => Icon::Panels,
            _ => Icon::Keyboard,
        };
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
                        t.msg = Some(match launcher::launch(path, action.args) {
                            Ok(()) => (format!("Started {}.", tool.name), false),
                            Err(e) => (e, true),
                        });
                        // Re-check soon so the status catches up with the new process.
                        t.last_check = Some(Instant::now() - RUNNING_CHECK_EVERY + Duration::from_millis(1500));
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
            ui.hyperlink_to(RichText::new("About").size(12.0), tool.project_url);
        });
        ui.add_space(8.0);
    }
    status_line(ui, &t.msg);
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
}
