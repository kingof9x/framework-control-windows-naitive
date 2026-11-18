//! Framework Control native desktop app.
//!
//! A lightweight replacement for the browser UI: it talks to the same local
//! service API on 127.0.0.1, but renders with egui instead of a Chromium
//! window.

// Don't open a console window next to the app on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod api;
mod app;
mod demo;
mod state;
mod theme;

use std::sync::Arc;

const DEFAULT_PORT: u16 = 8090;

fn usage() -> ! {
    eprintln!(
        "framework-control-app [--port <n>] [--demo] [--page overview|fans|power|battery|settings]\n\n\
         --port   Port of the local Framework Control service (default: $FRAMEWORK_CONTROL_PORT or {DEFAULT_PORT})\n\
         --demo   Use built-in sample data instead of the service"
    );
    std::process::exit(2)
}

fn main() -> eframe::Result<()> {
    let mut port: Option<u16> = None;
    let mut demo = false;
    let mut page: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--port" => port = args.next().and_then(|v| v.parse().ok()).or_else(|| usage()),
            "--demo" => demo = true,
            "--page" => page = args.next(),
            "-h" | "--help" => usage(),
            _ => usage(),
        }
    }
    let port = port
        .or_else(|| std::env::var("FRAMEWORK_CONTROL_PORT").ok().and_then(|v| v.parse().ok()))
        .unwrap_or(DEFAULT_PORT);

    let backend: Arc<dyn api::Backend> = if demo {
        Arc::new(demo::DemoBackend::new())
    } else {
        Arc::new(api::HttpBackend::new(port))
    };

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Framework Control")
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([760.0, 520.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Framework Control",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, backend, page.as_deref())))),
    )
}
