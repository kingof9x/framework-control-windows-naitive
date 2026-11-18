use eframe::egui::{self, Ui};

use crate::api::*;
use crate::app::App;
use crate::pages::overview::battery_status;
use crate::plots;
use crate::state::Snapshot;
use crate::widgets::{card, group, section, stat_row, switch, Icon};

pub fn show(app: &mut App, ui: &mut Ui, snap: &Snapshot) {
    let Some(b) = snap.power.as_ref().and_then(|p| p.battery.clone()) else {
        card(ui, Some(Icon::Battery), "No battery information yet", "Waiting for the service.", false, |_| {});
        return;
    };
    let health = match (b.last_full_charge_capacity_mah, b.design_capacity_mah) {
        (Some(f), Some(d)) if d > 0 => format!("{:.0}%", f as f32 * 100.0 / d as f32),
        _ => "—".into(),
    };
    section(ui, "Status");
    stat_row(
        ui,
        &[
            ("Charge", b.percentage.map(|p| format!("{p}%")).unwrap_or("—".into()), battery_status(&b).into()),
            (
                "Health",
                health,
                format!("{} of {} mAh", b.last_full_charge_capacity_mah.unwrap_or(0), b.design_capacity_mah.unwrap_or(0)),
            ),
            ("Cycles", b.cycle_count.map(|c| c.to_string()).unwrap_or("—".into()), String::new()),
            (
                "Power",
                b.watts().map(|w| format!("{:+.1} W", w)).unwrap_or("—".into()),
                b.present_voltage_mv.map(|v| format!("{:.2} V", v as f32 / 1000.0)).unwrap_or_default(),
            ),
        ],
    );

    let draft = &mut app.drafts.battery;
    let Some(cfg) = draft.value.as_mut() else { return };
    let mut dirty = false;

    section(ui, "Charging");
    let s = cfg.charge_limit_max_pct.get_or_insert(SettingU8 { enabled: false, value: 80 });
    let help = format!(
        "Stop charging at this level to help the battery last longer.{}",
        b.charge_limit_max_pct.map(|v| format!(" The laptop is set to {v}%.")).unwrap_or_default()
    );
    card(ui, Some(Icon::BatteryCharge), "Charge limit", &help, false, |ui| {
        ui.add_enabled_ui(s.enabled, |ui| {
            dirty |= ui.add(egui::Slider::new(&mut s.value, 25..=100).suffix(" %")).changed();
        });
        ui.add_space(12.0);
        dirty |= switch(ui, &mut s.enabled);
    });
    let r = cfg.charge_rate_c.get_or_insert(SettingF32 { enabled: false, value: 0.5 });
    card(ui, Some(Icon::Speed), "Charge rate limit", "Charge more gently. 1.0C is about the normal rate.", false, |ui| {
        ui.add_enabled_ui(r.enabled, |ui| {
            dirty |= ui.add(egui::Slider::new(&mut r.value, 0.05..=1.0).step_by(0.05).suffix(" C")).changed();
        });
        ui.add_space(12.0);
        dirty |= switch(ui, &mut r.enabled);
    });
    let enabled = r.enabled;
    let th = cfg.charge_rate_soc_threshold_pct.get_or_insert(0);
    card(ui, Some(Icon::Battery), "Only limit the rate above", "0% limits it all the time.", false, |ui| {
        ui.add_enabled_ui(enabled, |ui| {
            dirty |= ui.add(egui::Slider::new(th, 0..=100).suffix(" %")).changed();
        });
    });

    section(ui, "History");
    card(ui, Some(Icon::Timer), "Sample every", "How often the service records charge and power.", false, |ui| {
        let mut secs = cfg.poll_ms.unwrap_or(15_000) / 1000;
        if ui.add(egui::Slider::new(&mut secs, 5..=60).suffix(" s")).changed() {
            cfg.poll_ms = Some(secs * 1000);
            dirty = true;
        }
    });
    group(ui, "", "", |ui| plots::battery_plot(ui, &snap.battery_history));
    if dirty {
        draft.touch();
    }
}
