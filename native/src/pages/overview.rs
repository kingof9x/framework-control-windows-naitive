use eframe::egui::{self, Ui};

use crate::api::BatteryInfo;
use crate::app::App;
use crate::plots;
use crate::state::Snapshot;
use crate::widgets::{group, section, stat_row, warning_banner};

pub fn battery_status(b: &BatteryInfo) -> &'static str {
    if b.charging == Some(true) {
        "Charging"
    } else if b.ac_present == Some(true) {
        "Plugged in"
    } else {
        "On battery"
    }
}

pub fn show(app: &mut App, ui: &mut Ui, snap: &Snapshot) {
    if snap.health.as_ref().is_some_and(|h| !h.cli_present) {
        warning_banner(ui, "framework_tool wasn't found by the service, so live readings are unavailable.");
    }
    let latest = snap.thermal.last();
    let hottest = latest.and_then(|s| s.temps.iter().max_by_key(|(_, v)| **v).map(|(k, v)| (k.clone(), *v)));
    let rpm = latest.map(|s| s.rpms.clone()).unwrap_or_default();
    let bat = snap.power.as_ref().and_then(|p| p.battery.clone());
    let profile = snap.config.as_ref().and_then(|c| c.fan.active());

    crate::pages::tools::quick_launch(app, ui);

    section(ui, "Right now");
    stat_row(
        ui,
        &[
            (
                "Hottest sensor",
                hottest.as_ref().map(|(_, v)| format!("{v} °C")).unwrap_or("—".into()),
                hottest.map(|(k, _)| k).unwrap_or_default(),
            ),
            (
                "Fans",
                if rpm.is_empty() { "—".into() } else { rpm.iter().map(|r| r.to_string()).collect::<Vec<_>>().join(" / ") },
                "RPM".into(),
            ),
            (
                "Battery",
                bat.as_ref().and_then(|b| b.percentage).map(|p| format!("{p}%")).unwrap_or("—".into()),
                bat.as_ref().map(|b| battery_status(b).to_string()).unwrap_or_default(),
            ),
            (
                "Battery power",
                bat.as_ref().and_then(|b| b.watts()).map(|w| format!("{:+.1} W", w)).unwrap_or("—".into()),
                "+ in, − out".into(),
            ),
            (
                "Fan profile",
                profile.map(|p| p.name.clone()).unwrap_or("—".into()),
                profile.map(|p| crate::pages::fans::mode_label(p.mode).to_string()).unwrap_or_default(),
            ),
        ],
    );

    let names: Vec<String> = latest.map(|s| s.temps.keys().cloned().collect()).unwrap_or_default();
    section(ui, "Temperatures");
    group(ui, "", "", |ui| {
        ui.horizontal(|ui| {
            crate::widgets::hint(ui, "The last few minutes, from the service's own samples.");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let sel = &mut app.settings.sensor_filter;
                let label = if sel.is_empty() { "All sensors".to_string() } else { format!("{} sensors", sel.len()) };
                egui::ComboBox::from_id_salt("sensor_filter").selected_text(label).show_ui(ui, |ui| {
                    if ui.selectable_label(sel.is_empty(), "All sensors").clicked() {
                        sel.clear();
                    }
                    for n in &names {
                        let mut on = sel.contains(n);
                        if ui.checkbox(&mut on, n).changed() {
                            if on {
                                sel.push(n.clone());
                            } else {
                                sel.retain(|x| x != n);
                            }
                        }
                    }
                });
            });
        });
        plots::temp_plot(ui, &snap.thermal, &app.settings.sensor_filter, 240.0);
    });

    section(ui, "Fan speed");
    group(ui, "", "", |ui| {
        plots::rpm_plot(ui, &snap.thermal, 160.0);
    });
}
