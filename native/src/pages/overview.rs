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

    if let Some(p) = crate::pages::settings::unfinished(app) {
        if crate::widgets::card(
            ui,
            Some(crate::widgets::Icon::Warning),
            &format!("The update to {} didn't finish", p.version),
            "Open Settings to try again or see why.",
            true,
            |_| {},
        )
        .clicked()
        {
            app.page = crate::app::Page::Settings;
        }
    }
    crate::pages::tools::quick_launch(app, ui);

    section(ui, "Right now");
    stat_row(
        ui,
        &[
            (
                "Hottest sensor",
                hottest.as_ref().map(|(_, v)| format!("{v} °C")).unwrap_or("—".into()),
                hottest.map(|(k, _)| plots::sensor_label(&k)).unwrap_or_default(),
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

    // Temperatures: grouped charts or one sensor at a time, the viewer's choice (remembered per user).
    let view = app.settings.chart_view();
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        let c = crate::theme::colors(ui.ctx());
        ui.label(egui::RichText::new("Temperatures").font(crate::theme::semibold(14.0)).color(c.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Right to left: added in reverse.
            if crate::widgets::segmented(ui, view == ChartView::Single, "One sensor") {
                app.settings.set_chart_view(ChartView::Single);
            }
            if crate::widgets::segmented(ui, view == ChartView::Grouped, "Grouped") {
                app.settings.set_chart_view(ChartView::Grouped);
            }
        });
    });
    ui.add_space(4.0);
    match view {
        ChartView::Grouped => grouped(ui, snap),
        ChartView::Single => focus(app, ui, snap),
    }

    section(ui, "Fan speed");
    group(ui, "", "", |ui| plots::fan_chart(ui, "overview_fans", &snap.thermal, 150.0));
}

// ------------------------------------------------------------ chart layouts ---

/// How the Overview shows temperatures.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChartView {
    /// A small chart per group of related sensors.
    Grouped,
    /// Sensor buttons and one large chart of the chosen one.
    Single,
}

/// One sensor at a time: a button per sensor with its current value, and a large chart of the chosen one.
fn focus(app: &mut App, ui: &mut Ui, snap: &Snapshot) {
    let c = crate::theme::colors(ui.ctx());
    let sensors = plots::active_sensors(&snap.thermal);
    let chosen = app
        .settings
        .chart_focus
        .clone()
        .filter(|n| sensors.contains(n))
        .or_else(|| sensors.first().cloned());
    group(ui, "", "", |ui| {
        ui.horizontal_wrapped(|ui| {
            for name in &sensors {
                let now = snap.thermal.last().and_then(|s| s.temps.get(name)).copied().unwrap_or(0);
                let text = format!("{}  {} °C", plots::sensor_label(name), now);
                if crate::widgets::segmented(ui, chosen.as_deref() == Some(name), &text) {
                    app.settings.chart_focus = Some(name.clone());
                }
            }
        });
        ui.add_space(8.0);
        if let Some(name) = &chosen {
            let series = vec![(plots::sensor_label(name), plots::series_of(&snap.thermal, name), c.accent)];
            plots::clean_chart(ui, "focus_temp", &series, 230.0, "°", true);
        }
    });
}

/// A small chart per group of related sensors, a few lines each, labelled at their ends. A group with no
/// readings (e.g. the graphics module when it's off) isn't shown.
fn grouped(ui: &mut Ui, snap: &Snapshot) {
    let c = crate::theme::colors(ui.ctx());
    let sensors = plots::active_sensors(&snap.thermal);
    for group_name in ["Processor", "Memory and case", "Graphics"] {
        let members: Vec<&String> = sensors.iter().filter(|n| plots::sensor_group(n) == group_name).collect();
        if members.is_empty() {
            continue;
        }
        group(ui, group_name, "", |ui| {
            let series: Vec<_> = members
                .iter()
                .enumerate()
                .map(|(i, n)| (plots::sensor_label(n), plots::series_of(&snap.thermal, n), c.series(i)))
                .collect();
            plots::clean_chart(ui, &format!("group_{group_name}"), &series, 130.0, "°", false);
        });
    }
}
