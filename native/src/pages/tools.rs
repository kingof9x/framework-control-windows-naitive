//! Launcher for the companion Framework 16 tools (White Rabbit LED Matrix
//! app, Framework 16 Keyboard Control).

use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, Ui};

use crate::app::App;
use crate::launcher::{self, Found, LocalSettings, TOOLS};
use crate::task::{finished, Task};
use crate::theme;
use crate::widgets::{card, hint, primary_button, section_title, status_line};

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

    hint(ui, "Your other Framework 16 tools. They run on their own, so closing Framework Control leaves them running.");
    ui.add_space(8.0);

    for tool in TOOLS {
        let found = launcher::locate(tool, t.settings.as_ref().unwrap());
        let running = t.is_running(tool.exe);
        card(ui, |ui| {
            ui.horizontal(|ui| {
                section_title(ui, tool.name);
                let dark = ui.visuals().dark_mode;
                let (text, color) = match (&found, running) {
                    (_, true) => ("● Running", theme::success(dark)),
                    (Found::Missing, _) => ("Not found", theme::warning(dark)),
                    _ => ("Installed", ui.visuals().weak_text_color()),
                };
                ui.label(RichText::new(text).color(color).small());
            });
            ui.label(tool.description);
            ui.add_space(6.0);

            match found.path() {
                Some(path) => {
                    ui.horizontal_wrapped(|ui| {
                        for action in tool.actions {
                            let r = if std::ptr::eq(action, &tool.actions[0]) {
                                primary_button(ui, action.label)
                            } else {
                                ui.button(action.label)
                            };
                            let resp = if action.hint.is_empty() { r } else { r.on_hover_text(action.hint) };
                            if resp.clicked() {
                                t.msg = Some(match launcher::launch(path, action.args) {
                                    Ok(()) => (format!("Started {}.", tool.name), false),
                                    Err(e) => (e, true),
                                });
                                // Re-check soon so the status catches up with the new process.
                                t.last_check = Some(Instant::now() - RUNNING_CHECK_EVERY + Duration::from_millis(1500));
                            }
                        }
                        if cfg!(windows) && ui.button("Change location…").clicked() {
                            if let Some(p) = launcher::browse_for(tool) {
                                remember(t, tool.id, Some(p));
                            }
                        }
                        if matches!(found, Found::Chosen(_)) && ui.button("Use default location").clicked() {
                            remember(t, tool.id, None);
                        }
                    });
                    hint(ui, &path.display().to_string());
                }
                None => {
                    ui.label(format!("{} wasn't found on this PC.", tool.exe));
                    ui.horizontal_wrapped(|ui| {
                        if primary_button(ui, tool.download_label).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(tool.download_url));
                        }
                        if cfg!(windows) && ui.button("I already have it: browse…").clicked() {
                            if let Some(p) = launcher::browse_for(tool) {
                                remember(t, tool.id, Some(p));
                            }
                        }
                        ui.hyperlink_to("About this tool", tool.project_url);
                    });
                }
            }
        });
        ui.add_space(12.0);
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
