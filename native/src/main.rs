//! Framework Control native desktop app.
//!
//! A lightweight replacement for the browser UI: it talks to the same local
//! service API on 127.0.0.1, but renders with egui instead of a Chromium
//! window.

// Don't open a console window next to the app on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod api;
mod app;
mod calibration;
mod demo;
mod launcher;
mod pages;
mod plots;
mod state;
mod task;
mod theme;
mod tool_updates;
mod touchpad;
mod updater;
mod version;
mod widgets;

use std::sync::Arc;

const DEFAULT_PORT: u16 = 8090;

fn usage() -> ! {
    eprintln!(
        "framework-control-app [--port <n>] [--demo] [--page overview|fans|power|battery|touchpad|tools|settings]\n\n\
         --port   Port of the local Framework Control service (default: $FRAMEWORK_CONTROL_PORT or {DEFAULT_PORT})\n\
         --demo   Use built-in sample data instead of the service"
    );
    std::process::exit(2)
}

/// When installed next to the service, read the port it was configured with
/// from its WinSW config (`<env name="FRAMEWORK_CONTROL_PORT" value="..."/>`).
fn installed_service_port() -> Option<u16> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    port_from_winsw_xml(&std::fs::read_to_string(dir.join("FrameworkControlService.xml")).ok()?)
}

fn port_from_winsw_xml(xml: &str) -> Option<u16> {
    let after = &xml[xml.find("\"FRAMEWORK_CONTROL_PORT\"")?..];
    let value = &after[after.find("value=\"")? + 7..];
    value[..value.find('"')?].trim().parse().ok()
}

fn main() -> eframe::Result<()> {
    // The background touchpad helper: no window, just input hooks (see touchpad.rs).
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.first().map(String::as_str) == Some(touchpad::GUARD_ARG) {
        let app = raw.iter().position(|a| a == "--app").and_then(|i| raw.get(i + 1)).map(std::path::PathBuf::from);
        std::process::exit(touchpad::run_guard(app));
    }
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
        .or_else(installed_service_port)
        .unwrap_or(DEFAULT_PORT);

    if !demo {
        pages::touchpad::on_app_start();
    }

    let backend: Arc<dyn api::Backend> = if demo {
        Arc::new(demo::DemoBackend::new())
    } else {
        Arc::new(api::HttpBackend::new(port))
    };

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Framework Control")
            .with_inner_size([1240.0, 860.0])
            .with_min_inner_size([760.0, 520.0])
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).unwrap_or_default(),
            ),
        ..Default::default()
    };
    eframe::run_native(
        "Framework Control",
        options,
        Box::new(move |cc| {
            let mut app = app::App::new(cc, backend, page.as_deref());
            if demo {
                if let Ok(scene) = std::env::var("FC_DEMO_SCENE") {
                    app.scene(&scene, &cc.egui_ctx);
                }
            }
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_port_from_service_config() {
        let xml = r#"<env name="FRAMEWORK_CONTROL_ALLOWED_ORIGINS" value="" />
  <env name="FRAMEWORK_CONTROL_PORT" value="8123" />"#;
        assert_eq!(super::port_from_winsw_xml(xml), Some(8123));
        assert_eq!(super::port_from_winsw_xml("<service/>"), None);
    }
}
