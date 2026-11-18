//! Fan calibration wizard: measures RPM at several fixed duties so the curve
//! editor can show the duty each fan is really running at. Same procedure as
//! the web UI's CalibrationModal.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui::{self, RichText};
use serde_json::json;

use crate::api::*;
use crate::widgets;

const DUTIES: [u32; 5] = [100, 80, 60, 40, 20];

#[derive(Default)]
struct Progress {
    pct: f32,
    info: String,
    done: Option<Result<(), String>>,
}

pub struct Calibration {
    started: bool,
    progress: Arc<Mutex<Progress>>,
    cancel: Arc<AtomicBool>,
}

pub enum Outcome {
    Open,
    Closed,
}

impl Calibration {
    pub fn new() -> Self {
        Self { started: false, progress: Default::default(), cancel: Arc::new(AtomicBool::new(false)) }
    }

    pub fn show(&mut self, ctx: &egui::Context, backend: &Arc<dyn Backend>) -> Outcome {
        let mut outcome = Outcome::Open;
        let frame = egui::Frame::popup(&ctx.style()).inner_margin(egui::Margin::same(24)).corner_radius(egui::CornerRadius::same(8));
        egui::Modal::new(egui::Id::new("calibration")).frame(frame).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.spacing_mut().item_spacing.y = 8.0;
            if !self.started {
                ui.label(egui::RichText::new("Calibrate to see live fan speed").font(crate::theme::semibold(20.0)));
                ui.label(
                    "To show the live RPM marker accurately, we measure how each fan's speed (RPM) maps to \
                     duty percentage. This takes about a minute and briefly spins the fans at different speeds.",
                );
                ui.label(RichText::new("• Your current fan settings are restored afterwards.\n• You can cancel at any time.").weak());
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if widgets::accent_button(ui, "Start calibration").clicked() {
                        self.start(ctx, backend.clone());
                    }
                    if widgets::button(ui, "Cancel").clicked() {
                        outcome = Outcome::Closed;
                    }
                });
                return;
            }
            let p = self.progress.lock().unwrap();
            ui.label(egui::RichText::new("Calibrating fans").font(crate::theme::semibold(20.0)));
            ui.label(format!("{} {:.0}%", p.info, p.pct));
            ui.add(egui::ProgressBar::new(p.pct / 100.0).animate(p.done.is_none()));
            match &p.done {
                None => {
                    if ui.button("Cancel").clicked() {
                        self.cancel.store(true, Ordering::Relaxed);
                    }
                }
                Some(r) => {
                    match r {
                        Ok(()) if self.cancel.load(Ordering::Relaxed) => ui.label("Cancelled; fan settings restored."),
                        Ok(()) => ui.label("Done. The curve editor now shows live fan duty."),
                        Err(e) => ui.label(RichText::new(format!("Calibration failed: {e}")).color(crate::theme::colors(ui.ctx()).error)),
                    };
                    if ui.button("Close").clicked() {
                        outcome = Outcome::Closed;
                    }
                }
            }
        });
        outcome
    }

    fn start(&mut self, ctx: &egui::Context, backend: Arc<dyn Backend>) {
        self.started = true;
        let progress = self.progress.clone();
        let cancel = self.cancel.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = run(&*backend, &progress, &cancel, &ctx);
            progress.lock().unwrap().done = Some(r);
            ctx.request_repaint();
        });
    }
}

fn set_progress(progress: &Mutex<Progress>, ctx: &egui::Context, pct: f32, info: &str) {
    let mut p = progress.lock().unwrap();
    p.pct = pct;
    p.info = info.to_string();
    ctx.request_repaint();
}

/// Patch the given profile (by id) on top of the latest server config.
fn update_profile(backend: &dyn Backend, id: &str, f: impl Fn(&mut FanProfile)) -> Result<(), String> {
    let mut profiles = backend.config()?.fan.profiles;
    for p in profiles.iter_mut().filter(|p| p.id == id) {
        f(p);
    }
    backend.patch_config(json!({ "fan": { "profiles": profiles } })).map(|_| ())
}

fn run(backend: &dyn Backend, progress: &Mutex<Progress>, cancel: &AtomicBool, ctx: &egui::Context) -> Result<(), String> {
    set_progress(progress, ctx, 5.0, "Starting calibration…");
    let cfg = backend.config()?;
    let prev = cfg.fan.active().cloned().ok_or("no active fan profile")?;
    let id = prev.id.clone();

    let result = (|| {
        update_profile(backend, &id, |p| {
            p.mode = FanMode::Manual;
            p.overrides.clear();
        })?;
        let mut per_fan: Vec<Vec<[u32; 2]>> = Vec::new();
        for (i, duty) in DUTIES.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            set_progress(progress, ctx, 10.0 + i as f32 * 18.0, &format!("Measuring at {duty}% duty…"));
            update_profile(backend, &id, |p| p.manual = Some(ManualConfig { duty_pct: *duty }))?;
            let rpms = read_stable_rpms(backend, cancel);
            if per_fan.len() < rpms.len() {
                per_fan.resize(rpms.len(), Vec::new());
            }
            for (f, r) in rpms.iter().enumerate() {
                per_fan[f].push([*duty, *r]);
            }
        }
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        set_progress(progress, ctx, 98.0, "Saving…");
        let fans: Vec<PerFanCalibration> = per_fan
            .into_iter()
            .enumerate()
            .map(|(i, mut pts)| {
                pts.push([0, 0]);
                pts.sort_by_key(|p| p[0]);
                PerFanCalibration { index: i as u32, points: pts }
            })
            .collect();
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        backend
            .patch_config(json!({ "fan": { "calibration": FanCalibration { updated_at: now, fans } } }))
            .map(|_| ())
    })();

    // Always put the user's profile back exactly as it was.
    let restored = update_profile(backend, &id, |p| *p = prev.clone());
    set_progress(progress, ctx, 100.0, if cancel.load(Ordering::Relaxed) { "Cancelled." } else { "Finished." });
    result.and(restored)
}

fn stdev(v: &[u32]) -> f64 {
    if v.is_empty() {
        return f64::INFINITY;
    }
    let mean = v.iter().map(|&x| x as f64).sum::<f64>() / v.len() as f64;
    (v.iter().map(|&x| (x as f64 - mean).powi(2)).sum::<f64>() / v.len() as f64).sqrt()
}

fn median(v: &[u32]) -> u32 {
    let mut s = v.to_vec();
    s.sort();
    s.get(s.len() / 2).copied().unwrap_or(0)
}

/// Wait for every fan's RPM to settle (low spread over a window), then return medians.
fn read_stable_rpms(backend: &dyn Backend, cancel: &AtomicBool) -> Vec<u32> {
    const SETTLE: Duration = Duration::from_millis(1000);
    const WINDOW: usize = 8;
    const STDEV_MAX: f64 = 30.0;
    const POLL: Duration = Duration::from_millis(500);
    const TIMEOUT: Duration = Duration::from_secs(15);

    let started = Instant::now();
    let mut bufs: Vec<Vec<u32>> = Vec::new();
    let mut pending_accept = false;
    std::thread::sleep(SETTLE);
    while started.elapsed() < TIMEOUT {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if let Ok(t) = backend.thermal_now() {
            if bufs.len() < t.fans.len() {
                bufs.resize(t.fans.len(), Vec::new());
            }
            for (i, f) in t.fans.iter().enumerate() {
                bufs[i].push(f.rpm);
                if bufs[i].len() > WINDOW {
                    bufs[i].remove(0);
                }
            }
        }
        let ready = !bufs.is_empty() && bufs.iter().all(|b| b.len() >= WINDOW && stdev(b) <= STDEV_MAX);
        if ready && pending_accept {
            break;
        }
        pending_accept = ready;
        std::thread::sleep(if pending_accept { SETTLE } else { POLL });
    }
    bufs.iter().map(|b| median(b)).collect()
}
