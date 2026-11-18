//! Background poller: talks to the backend on its own thread so the UI thread
//! never blocks, and wakes the UI only when new data arrives.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::api::*;

#[derive(Default)]
pub struct Snapshot {
    pub connected: bool,
    pub last_error: Option<String>,
    pub health: Option<Health>,
    pub thermal: Vec<TelemetrySample>,
    pub battery_history: Vec<BatterySample>,
    pub power: Option<PowerResponse>,
    pub config: Option<Config>,
    pub versions: Option<Versions>,
    pub system: Option<SystemInfo>,
    pub update: Option<UpdateCheck>,
    /// Bumped every time `config` changes so the UI can reset clean drafts.
    pub config_rev: u64,
    /// Result of the most recent write (message, is_error).
    pub write_status: Option<(String, bool)>,
}

pub enum Cmd {
    Patch(Value),
    RefreshAll,
}

pub type Shared = Arc<Mutex<Snapshot>>;

pub fn spawn(backend: Arc<dyn Backend>, ctx: eframe::egui::Context) -> (Shared, Sender<Cmd>) {
    let shared: Shared = Arc::new(Mutex::new(Snapshot::default()));
    let (tx, rx) = mpsc::channel();
    let s = shared.clone();
    std::thread::Builder::new()
        .name("poller".into())
        .spawn(move || run(backend, s, rx, ctx))
        .expect("spawn poller");
    (shared, tx)
}

fn run(backend: Arc<dyn Backend>, shared: Shared, rx: Receiver<Cmd>, ctx: eframe::egui::Context) {
    let mut last_config = Instant::now() - Duration::from_secs(3600);
    let mut last_battery = last_config;
    let mut last_static = last_config;

    loop {
        let poll = {
            let s = shared.lock().unwrap();
            s.config
                .as_ref()
                .map(|c| c.telemetry.poll_ms)
                .unwrap_or(2000)
                .clamp(500, 10_000)
        };

        // Fast loop: health, live telemetry history, power/battery state.
        let health = backend.health();
        let connected = health.is_ok();
        let thermal = if connected { backend.thermal_history().ok() } else { None };
        let power = if connected { backend.power().ok() } else { None };
        {
            let mut s = shared.lock().unwrap();
            s.connected = connected;
            match health {
                Ok(h) => {
                    s.health = Some(h);
                    s.last_error = None;
                }
                Err(e) => s.last_error = Some(e),
            }
            if let Some(t) = thermal {
                s.thermal = t;
            }
            if power.is_some() {
                s.power = power;
            }
        }

        if connected && last_config.elapsed() > Duration::from_secs(5) {
            if let Ok(cfg) = backend.config() {
                set_config(&shared, cfg);
            }
            last_config = Instant::now();
        }
        if connected && last_battery.elapsed() > Duration::from_secs(15) {
            if let Ok(h) = backend.battery_history() {
                shared.lock().unwrap().battery_history = h;
            }
            last_battery = Instant::now();
        }
        if connected && last_static.elapsed() > Duration::from_secs(600) {
            let v = backend.versions().ok();
            let sys = backend.system().ok();
            let up = backend.check_update().ok();
            let mut s = shared.lock().unwrap();
            s.versions = v.or(s.versions.take());
            s.system = sys.or(s.system.take());
            s.update = up.or(s.update.take());
            last_static = Instant::now();
        }
        ctx.request_repaint();

        // Sleep until the next poll, but handle UI commands immediately.
        let deadline = Instant::now() + Duration::from_millis(poll);
        loop {
            let wait = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(wait) {
                Ok(Cmd::Patch(patch)) => {
                    let res = backend.patch_config(patch);
                    let mut s = shared.lock().unwrap();
                    match res {
                        Ok(cfg) => {
                            s.config = Some(cfg);
                            s.config_rev += 1;
                            s.write_status = Some(("Saved".into(), false));
                        }
                        Err(e) => s.write_status = Some((format!("Save failed: {e}"), true)),
                    }
                    drop(s);
                    ctx.request_repaint();
                }
                Ok(Cmd::RefreshAll) => {
                    last_config = Instant::now() - Duration::from_secs(3600);
                    last_battery = last_config;
                    last_static = last_config;
                    break;
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

fn set_config(shared: &Shared, cfg: Config) {
    let mut s = shared.lock().unwrap();
    if s.config.as_ref() != Some(&cfg) {
        s.config = Some(cfg);
        s.config_rev += 1;
    }
}
