use eframe::egui::{self, Ui};

use crate::api::*;
use crate::app::App;
use crate::state::Snapshot;
use crate::task::{finished, Task};
use crate::widgets::{self, card, group, section, segmented, setting_card_choice, setting_card_u32, status_line, Icon};

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
        card(ui, Some(Icon::Power), "Loading…", "Waiting for the service.", false, |_| {});
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
        if windows {
            ryzenadj_install(app, ui);
        } else {
            card(
                ui,
                Some(Icon::Warning),
                "Power controls aren't available",
                "No supported power interface was found (unsupported CPU, or missing AMD P-State/cpufreq support).",
                false,
                |_| {},
            );
        }
        return;
    }

    let Some(draft) = app.drafts.power.value.as_mut() else { return };
    let mut dirty = false;
    section(ui, "Profile");
    card(
        ui,
        Some(Icon::Plug),
        "Settings for",
        "The service applies the matching profile whenever you plug in or unplug.",
        false,
        |ui| {
            if segmented(ui, !app.power.battery_tab, "Plugged in") {
                app.power.battery_tab = false;
            }
            if segmented(ui, app.power.battery_tab, "On battery") {
                app.power.battery_tab = true;
            }
        },
    );
    let prof = if app.power.battery_tab {
        draft.battery.get_or_insert_with(Default::default)
    } else {
        draft.ac.get_or_insert_with(Default::default)
    };
    section(ui, "Limits");
    if caps.supports_tdp {
        let (lo, hi) = (caps.tdp_min_watts.unwrap_or(5), caps.tdp_max_watts.unwrap_or(120));
        let s = prof.tdp_watts.get_or_insert(SettingU32 { enabled: false, value: 30 });
        dirty |= setting_card_u32(ui, Icon::Power, "TDP limit", s, lo..=hi, " W", pc.current_state.tdp_limit_watts);
    }
    if caps.supports_thermal {
        let s = prof.thermal_limit_c.get_or_insert(SettingU32 { enabled: false, value: 95 });
        dirty |= setting_card_u32(ui, Icon::Temperature, "Thermal limit", s, 60..=100, " °C", pc.current_state.thermal_limit_c);
    }
    if caps.supports_epp {
        let opts = caps.available_epp_preferences.clone().unwrap_or_default();
        let s = prof.epp_preference.get_or_insert(SettingString::default());
        dirty |= setting_card_choice(ui, Icon::Sliders, "Energy preference", s, &opts, pc.current_state.epp_preference.clone());
    }
    if caps.supports_governor {
        let opts = caps.available_governors.clone().unwrap_or_default();
        let s = prof.governor.get_or_insert(SettingString::default());
        dirty |= setting_card_choice(ui, Icon::Options, "CPU governor", s, &opts, pc.current_state.governor.clone());
    }
    if caps.supports_frequency_limits {
        let (lo, hi) = (caps.frequency_min_mhz.unwrap_or(400), caps.frequency_max_mhz.unwrap_or(5000));
        let s = prof.min_freq_mhz.get_or_insert(SettingU32 { enabled: false, value: lo });
        dirty |= setting_card_u32(ui, Icon::Gauge, "Minimum frequency", s, lo..=hi, " MHz", pc.current_state.min_freq_mhz);
        let s = prof.max_freq_mhz.get_or_insert(SettingU32 { enabled: false, value: hi });
        dirty |= setting_card_u32(ui, Icon::Speed, "Maximum frequency", s, lo..=hi, " MHz", pc.current_state.max_freq_mhz);
    }
    if dirty {
        app.drafts.power.touch();
    }

    if windows && caps.supports_tdp {
        section(ui, "Helper");
        let busy = app.power.job.is_some();
        card(ui, Some(Icon::Delete), "RyzenAdj", "The helper that sets power limits. You can reinstall it from this page.", false, |ui| {
            if ui.add_enabled(!busy, egui::Button::new(if busy { "Removing…" } else { "Uninstall" }).min_size(egui::vec2(0.0, 32.0))).clicked() {
                let b = app.backend.clone();
                app.power.job = Some(Task::spawn(ui.ctx(), move || b.uninstall_ryzenadj().map(|_| "RyzenAdj removed.")));
            }
        });
        status_line(ui, &app.power.msg);
    }
}

fn ryzenadj_install(app: &mut App, ui: &mut Ui) {
    section(ui, "Enable power controls");
    group(ui, "Install the RyzenAdj helper", "", |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label("Power limits need a small helper,");
            ui.hyperlink_to("RyzenAdj", "https://github.com/FlyGoat/RyzenAdj");
            ui.label(", installed by the service.");
        });
        widgets::hint(ui, "It may trigger antivirus warnings.");
        widgets::hint(
            ui,
            "Changing power settings can cause instability and crashes, and may even (rarely) damage your hardware. We take no responsibility!",
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.checkbox(&mut app.power.agreed, "I agree and understand the risks");
            ui.add_space(12.0);
            let busy = app.power.job.is_some();
            ui.add_enabled_ui(app.power.agreed && !busy, |ui| {
                if widgets::accent_button(ui, if busy { "Installing…" } else { "Install" }).clicked() {
                    let b = app.backend.clone();
                    app.power.job = Some(Task::spawn(ui.ctx(), move || {
                        b.install_ryzenadj()
                            .map(|_| "RyzenAdj installed.")
                            .map_err(|e| format!("Failed to install, check your antivirus settings! ({e})"))
                    }));
                }
            });
        });
        status_line(ui, &app.power.msg);
    });
}
