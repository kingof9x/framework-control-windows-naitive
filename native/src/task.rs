//! One-off background jobs (installs, calibration, log fetches) that must not
//! block the UI thread.

use std::sync::{Arc, Mutex};

use eframe::egui::Context;

pub struct Task<T> {
    slot: Arc<Mutex<Option<Result<T, String>>>>,
}

impl<T: Send + 'static> Task<T> {
    pub fn spawn(ctx: &Context, f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Self {
        let slot = Arc::new(Mutex::new(None));
        let out = slot.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = f();
            *out.lock().unwrap() = Some(r);
            ctx.request_repaint();
        });
        Self { slot }
    }

    /// Takes the result once the job has finished.
    pub fn take(&self) -> Option<Result<T, String>> {
        self.slot.lock().unwrap().take()
    }
}

/// Convenience: poll an optional task, clearing it when done.
pub fn finished<T: Send + 'static>(task: &mut Option<Task<T>>) -> Option<Result<T, String>> {
    let r = task.as_ref()?.take();
    if r.is_some() {
        *task = None;
    }
    r
}
