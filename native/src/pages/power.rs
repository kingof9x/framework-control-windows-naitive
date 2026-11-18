use eframe::egui::{self, RichText, Ui};

use crate::api::*;
use crate::app::App;
use crate::state::Snapshot;
use crate::task::{finished, Task};
use crate::widgets::{self, card, section_title, segmented, setting_row_choice, setting_row_u32, status_line};

#[derive(Default)]
pub struct PowerUi {
    pub battery_tab: bool,
    agreed: bool,
    job: Option<Task<&'static str>>,
    msg: Option<(String, bool)>,
}

fn is_windows(snap: &Snapshot) -> bool {
    snap.system.as_ref().map(|s| s.os.to_lowercase().contains("windows")).unwrap_or(cfg!(windows))
}

pub fn show(app: &mut App, ui: &mut Ui, snap: &Snapshot) {
    if let Some(r) = finished(&mut app.power.job) {
        app.power.msg = Some(match r {
            Ok(m) => (m.to_string(), false),
            Err(e) => (e, true),
        });
        app.refresh();
    }
    let Some(pc) = snap.power.as_ref().map(|p| p.power_control.clone()) else {
        ui.label("Loading…");
        return;
    };
    let caps = &pc.capabilities;
    let any = caps.supports_tdp
        || caps.supports_thermal
        || caps.supports_epp
        || caps.supports_governor
        || caps.supports_frequency_limits;
    let windows = is_windows(snap);

    if !any {
        card(ui, |ui| {
            if windows {
                ryzenadj_install(app, ui);
            } else {
                section_title(ui, "Power controls not available");
                ui.label("No supported power management interface was detected. This may be due to:");
                ui.label(RichText::new("• Unsupported CPU\n• Missing kernel support (Linux needs AMD P-State or cpufreq)").weak());
            }
        });
        return;
    }

    let Some(draft) = app.drafts.power.value.as_mut() else { return };
    let mut dirty = false;
    card(ui, |ui| {
        ui.horizontal(|ui| {
            section_title(ui, "Power profile");
            ui.add_space(8.0);
            if segmented(ui, !app.power.battery_tab, "Plugged in") {
                app.power.battery_tab = false;
            }
            if segmented(ui, app.power.battery_tab, "On battery") {
                app.power.battery_tab = true;
            }
        });
        widgets::hint(ui, "The service re-applies the matching profile whenever the power source changes.");
        ui.add_space(8.0);
        let prof = if app.power.battery_tab {
            draft.battery.get_or_insert_with(Default::default)
        } else {
            draft.ac.get_or_insert_with(Default::default)
        };
        egui::Grid::new("power_grid").num_columns(3).spacing([16.0, 10.0]).show(ui, |ui| {
            if caps.supports_tdp {
                let (lo, hi) = (caps.tdp_min_watts.unwrap_or(5), caps.tdp_max_watts.unwrap_or(120));
                let s = prof.tdp_watts.get_or_insert(SettingU32 { enabled: false, value: 30 });
                dirty |= setting_row_u32(ui, "TDP limit", s, lo..=hi, " W", pc.current_state.tdp_limit_watts);
            }
            if caps.supports_thermal {
                let s = prof.thermal_limit_c.get_or_insert(SettingU32 { enabled: false, value: 95 });
                dirty |= setting_row_u32(ui, "Thermal limit", s, 60..=100, " °C", pc.current_state.thermal_limit_c);
            }
            if caps.supports_epp {
                let opts = caps.available_epp_preferences.clone().unwrap_or_default();
                let s = prof.epp_preference.get_or_insert(SettingString::default());
                dirty |= setting_row_choice(ui, "Energy preference", s, &opts, pc.current_state.epp_preference.clone());
            }
            if caps.supports_governor {
                let opts = caps.available_governors.clone().unwrap_or_default();
                let s = prof.governor.get_or_insert(SettingString::default());
                dirty |= setting_row_choice(ui, "CPU governor", s, &opts, pc.current_state.governor.clone());
            }
            if caps.supports_frequency_limits {
                let (lo, hi) = (caps.frequency_min_mhz.unwrap_or(400), caps.frequency_max_mhz.unwrap_or(5000));
                let s = prof.min_freq_mhz.get_or_insert(SettingU32 { enabled: false, value: lo });
                dirty |= setting_row_u32(ui, "Min frequency", s, lo..=hi, " MHz", pc.current_state.min_freq_mhz);
                let s = prof.max_freq_mhz.get_or_insert(SettingU32 { enabled: false, value: hi });
                dirty |= setting_row_u32(ui, "Max frequency", s, lo..=hi, " MHz", pc.current_state.max_freq_mhz);
            }
        });
    });
    if dirty {
        app.drafts.power.touch();
    }

    if windows && caps.supports_tdp {
        ui.add_space(12.0);
        card(ui, |ui| {
            section_title(ui, "RyzenAdj helper");
            ui.label("Remove the RyzenAdj helper. You can reinstall it later from this page.");
            ui.horizontal(|ui| {
                let busy = app.power.job.is_some();
                if ui.add_enabled(!busy, egui::Button::new(if busy { "Removing…" } else { "Uninstall RyzenAdj" })).clicked() {
                    let b = app.backend.clone();
                    app.power.job = Some(Task::spawn(ui.ctx(), move || b.uninstall_ryzenadj().map(|_| "RyzenAdj removed.")));
                }
                status_line(ui, &app.power.msg);
            });
        });
    }
}

fn ryzenadj_install(app: &mut App, ui: &mut Ui) {
    section_title(ui, "Enable power controls");
    ui.horizontal_wrapped(|ui| {
        ui.label("• This requires a small helper,");
        ui.hyperlink_to("RyzenAdj", "https://github.com/FlyGoat/RyzenAdj");
        ui.label(", to be installed.");
    });
    ui.label("• It may trigger antivirus warnings on your system.");
    ui.label(
        "• Adjusting power settings can cause instability and crashes and may even (though rarely) damage your \
         hardware. We take no responsibility!",
    );
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.checkbox(&mut app.power.agreed, "I agree to the above and understand the risks");
        let busy = app.power.job.is_some();
        let enabled = app.power.agreed && !busy;
        let label = if busy { "Installing…" } else { "Install" };
        if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
            let b = app.backend.clone();
            app.power.job = Some(Task::spawn(ui.ctx(), move || {
                b.install_ryzenadj()
                    .map(|_| "RyzenAdj installed.")
                    .map_err(|e| format!("Failed to install, check your antivirus settings! ({e})"))
            }));
        }
    });
    status_line(ui, &app.power.msg);
}
