use eframe::egui::{self, RichText, Ui};

use crate::api::*;
use crate::app::App;
use crate::pages::overview::battery_status;
use crate::plots;
use crate::state::Snapshot;
use crate::widgets::{card, section_title, stat_row};

pub fn show(app: &mut App, ui: &mut Ui, snap: &Snapshot) {
    let Some(b) = snap.power.as_ref().and_then(|p| p.battery.clone()) else {
        ui.label("No battery information yet.");
        return;
    };
    let health = match (b.last_full_charge_capacity_mah, b.design_capacity_mah) {
        (Some(f), Some(d)) if d > 0 => format!("{:.0}%", f as f32 * 100.0 / d as f32),
        _ => "—".into(),
    };
    stat_row(
        ui,
        &[
            ("Charge", b.percentage.map(|p| format!("{p}%")).unwrap_or("—".into()), battery_status(&b).into()),
            (
                "Health",
                health,
                format!("{} / {} mAh", b.last_full_charge_capacity_mah.unwrap_or(0), b.design_capacity_mah.unwrap_or(0)),
            ),
            ("Cycles", b.cycle_count.map(|c| c.to_string()).unwrap_or("—".into()), String::new()),
            (
                "Power",
                b.watts().map(|w| format!("{:+.1} W", w)).unwrap_or("—".into()),
                b.present_voltage_mv.map(|v| format!("{:.2} V", v as f32 / 1000.0)).unwrap_or_default(),
            ),
        ],
    );
    ui.add_space(12.0);

    let draft = &mut app.drafts.battery;
    let Some(cfg) = draft.value.as_mut() else { return };
    let mut dirty = false;
    card(ui, |ui| {
        section_title(ui, "Charging");
        egui::Grid::new("bat_grid").num_columns(3).spacing([16.0, 10.0]).show(ui, |ui| {
            let s = cfg.charge_limit_max_pct.get_or_insert(SettingU8 { enabled: false, value: 80 });
            dirty |= ui.checkbox(&mut s.enabled, "Charge limit").changed();
            ui.add_enabled_ui(s.enabled, |ui| {
                dirty |= ui.add(egui::Slider::new(&mut s.value, 25..=100).suffix(" %")).changed();
            });
            ui.label(RichText::new(b.charge_limit_max_pct.map(|v| format!("EC: {v}%")).unwrap_or_default()).weak());
            ui.end_row();

            let r = cfg.charge_rate_c.get_or_insert(SettingF32 { enabled: false, value: 0.5 });
            dirty |= ui.checkbox(&mut r.enabled, "Charge rate limit").changed();
            ui.add_enabled_ui(r.enabled, |ui| {
                dirty |= ui.add(egui::Slider::new(&mut r.value, 0.05..=1.0).step_by(0.05).suffix(" C")).changed();
            });
            ui.label(RichText::new("1.0C ≈ no limit").weak());
            ui.end_row();

            let enabled = r.enabled;
            let th = cfg.charge_rate_soc_threshold_pct.get_or_insert(0);
            ui.label("    …only above");
            ui.add_enabled_ui(enabled, |ui| {
                dirty |= ui.add(egui::Slider::new(th, 0..=100).suffix(" %")).changed();
            });
            ui.label(RichText::new("0 = always").weak());
            ui.end_row();
        });
    });
    ui.add_space(12.0);
    card(ui, |ui| {
        ui.horizontal(|ui| {
            section_title(ui, "History");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut secs = cfg.poll_ms.unwrap_or(15_000) / 1000;
                if ui.add(egui::Slider::new(&mut secs, 5..=60).suffix(" s")).changed() {
                    cfg.poll_ms = Some(secs * 1000);
                    dirty = true;
                }
                ui.label(RichText::new("Sample every").weak());
            });
        });
        plots::battery_plot(ui, &snap.battery_history);
    });
    if dirty {
        draft.touch();
    }
}
