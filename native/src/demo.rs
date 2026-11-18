//! In-memory backend with synthetic data, for trying the UI without a
//! Framework laptop or the service (`--demo`).

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::api::*;

pub struct DemoBackend {
    config: Mutex<Config>,
    start_ms: i64,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Cheap deterministic pseudo-noise in [-1, 1].
fn noise(seed: f64) -> f64 {
    let x = (seed * 12.9898).sin() * 43758.5453;
    (x - x.floor()) * 2.0 - 1.0
}

impl DemoBackend {
    pub fn new() -> Self {
        let curve = GlobalCurveConfig {
            curve: CurveConfig {
                sensors: vec!["APU".into(), "CPU".into()],
                ..Default::default()
            },
            poll_ms: 2000,
        };
        let config = Config {
            fan: FanControlConfig {
                profiles: vec![
                    FanProfile {
                        id: "default".into(),
                        name: "Balanced".into(),
                        mode: FanMode::Curve,
                        manual: Some(ManualConfig { duty_pct: 35 }),
                        curve: Some(curve.clone()),
                        ..Default::default()
                    },
                    FanProfile {
                        id: "quiet".into(),
                        name: "Quiet".into(),
                        mode: FanMode::Curve,
                        curve: Some(GlobalCurveConfig {
                            curve: CurveConfig {
                                points: vec![[50, 0], [70, 25], [82, 60], [92, 100]],
                                ..curve.curve
                            },
                            poll_ms: 2000,
                        }),
                        ..Default::default()
                    },
                ],
                active_profile: "default".into(),
                auto_switch: AutoSwitch { ac: Some("default".into()), battery: Some("quiet".into()) },
                calibration: Some(FanCalibration {
                    updated_at: 0,
                    fans: (0..2)
                        .map(|i| PerFanCalibration {
                            index: i,
                            points: vec![[0, 0], [20, 1100], [40, 2150], [60, 3200], [80, 4200], [100, 5200]],
                        })
                        .collect(),
                }),
                ..Default::default()
            },
            power: PowerConfig {
                ac: Some(PowerProfile {
                    tdp_watts: Some(SettingU32 { enabled: true, value: 45 }),
                    thermal_limit_c: Some(SettingU32 { enabled: false, value: 95 }),
                    ..Default::default()
                }),
                battery: Some(PowerProfile {
                    tdp_watts: Some(SettingU32 { enabled: true, value: 18 }),
                    thermal_limit_c: Some(SettingU32 { enabled: true, value: 85 }),
                    ..Default::default()
                }),
            },
            battery: BatteryConfig {
                charge_limit_max_pct: Some(SettingU8 { enabled: true, value: 80 }),
                charge_rate_c: Some(SettingF32 { enabled: false, value: 0.5 }),
                charge_rate_soc_threshold_pct: Some(60),
                poll_ms: None,
            },
            ..Default::default()
        };
        Self {
            config: Mutex::new(config),
            start_ms: now_ms(),
        }
    }

    fn sample_at(&self, ts: i64) -> TelemetrySample {
        let t = ts as f64 / 1000.0;
        let load = 0.5 + 0.35 * (t / 90.0).sin() + 0.15 * (t / 17.0).sin();
        let apu = 48.0 + 30.0 * load + 1.5 * noise(t);
        let mut temps = BTreeMap::new();
        temps.insert("APU".into(), apu.round() as i32);
        temps.insert("CPU".into(), (apu - 3.0 + noise(t + 1.0)).round() as i32);
        temps.insert("Battery".into(), (33.0 + 2.0 * (t / 600.0).sin()).round() as i32);
        temps.insert("Ambient".into(), (29.0 + 0.5 * noise(t / 50.0)).round() as i32);
        temps.insert("dGPU".into(), (44.0 + 18.0 * load + noise(t + 3.0)).round() as i32);
        let duty = ((apu - 40.0) / 45.0).clamp(0.0, 1.0);
        let rpm = |off: f64| ((duty * (5200.0 - off * 60.0) + 80.0 * noise(t + off)).max(0.0)) as u32;
        TelemetrySample {
            ts_ms: ts,
            temps,
            rpms: vec![rpm(5.0), rpm(7.0)],
        }
    }
}

impl Backend for DemoBackend {
    fn health(&self) -> ApiResult<Health> {
        Ok(Health {
            cli_present: true,
            service_version: "0.5.5 (demo)".into(),
        })
    }

    fn thermal_history(&self) -> ApiResult<Vec<TelemetrySample>> {
        let now = now_ms();
        // Pretend the service has been sampling for 10 minutes before we started.
        let first = (self.start_ms - 10 * 60_000).max(now - 30 * 60_000);
        Ok((first..=now)
            .step_by(2000)
            .map(|ts| self.sample_at(ts))
            .collect())
    }

    fn battery_history(&self) -> ApiResult<Vec<BatterySample>> {
        let now = now_ms();
        let span = 6 * 3600_000;
        Ok((now - span..=now)
            .step_by(60_000)
            .map(|ts| {
                let h = (now - ts) as f64 / 3600_000.0;
                // Discharged from 80% to ~35%, then plugged in for the last hour.
                let (pct, w) = if h > 1.0 {
                    (35.0 + (h - 1.0) * 9.0, -9.0 - 4.0 * noise(ts as f64).abs())
                } else {
                    (35.0 + (1.0 - h) * 30.0, 38.0 + 3.0 * noise(ts as f64))
                };
                BatterySample {
                    ts_ms: ts,
                    charge_pct: Some(pct.min(80.0) as f32),
                    watts: Some(w as f32),
                    ac_present: Some(h <= 1.0),
                }
            })
            .collect())
    }

    fn power(&self) -> ApiResult<PowerResponse> {
        Ok(PowerResponse {
            battery: Some(BatteryInfo {
                ac_present: Some(true),
                battery_present: Some(true),
                last_full_charge_capacity_mah: Some(3683),
                remaining_capacity_mah: Some(2394),
                percentage: Some(65),
                present_voltage_mv: Some(16_420),
                present_rate_ma: Some(2_310),
                design_capacity_mah: Some(3915),
                cycle_count: Some(142),
                charging: Some(true),
                discharging: Some(false),
                charge_limit_min_pct: Some(0),
                charge_limit_max_pct: Some(80),
            }),
            power_control: PowerControlInfo {
                capabilities: PowerCapabilities {
                    supports_tdp: true,
                    supports_thermal: true,
                    tdp_min_watts: Some(5),
                    tdp_max_watts: Some(120),
                    ..Default::default()
                },
                current_state: PowerState {
                    tdp_limit_watts: Some(45),
                    thermal_limit_c: Some(100),
                    ..Default::default()
                },
            },
        })
    }

    fn config(&self) -> ApiResult<Config> {
        Ok(self.config.lock().unwrap().clone())
    }

    fn versions(&self) -> ApiResult<Versions> {
        Ok(Versions {
            tool_version: Some("0.4.5".into()),
            mainboard_type: Some("Laptop 16 (AMD Ryzen 7040)".into()),
            mainboard_revision: Some("MassProduction".into()),
            uefi_version: Some("3.07".into()),
            uefi_release_date: Some("2025-06-12".into()),
            ec_build_version: Some("lotus-3.0.7".into()),
        })
    }

    fn system(&self) -> ApiResult<SystemInfo> {
        Ok(SystemInfo {
            cpu: "AMD Ryzen 9 7940HS w/ Radeon 780M Graphics".into(),
            memory_total_mb: 32_768,
            os: "Windows 11".into(),
            dgpu: Some("AMD Radeon RX 7700S".into()),
        })
    }

    fn check_update(&self) -> ApiResult<UpdateCheck> {
        Ok(UpdateCheck {
            current_version: "0.5.5".into(),
            latest_version: "0.5.5".into(),
            updates_enabled: true,
        })
    }

    fn patch_config(&self, patch: Value) -> ApiResult<Config> {
        let mut cfg = self.config.lock().unwrap();
        // Shallow-merge each top-level section, like the service does per field.
        let mut current = serde_json::to_value(&*cfg).map_err(|e| e.to_string())?;
        if let (Some(cur), Some(p)) = (current.as_object_mut(), patch.as_object()) {
            for (section, val) in p {
                let entry = cur.entry(section.clone()).or_insert(Value::Null);
                match (entry.as_object_mut(), val.as_object()) {
                    (Some(e), Some(v)) => {
                        for (k, vv) in v {
                            e.insert(k.clone(), vv.clone());
                        }
                    }
                    _ => *entry = val.clone(),
                }
            }
        }
        *cfg = serde_json::from_value(current).map_err(|e| e.to_string())?;
        Ok(cfg.clone())
    }

    fn thermal_now(&self) -> ApiResult<ThermalNow> {
        let s = self.sample_at(now_ms());
        Ok(ThermalNow {
            temps: s.temps,
            fans: s
                .rpms
                .iter()
                .enumerate()
                .map(|(i, r)| FanReading { name: format!("Fan {}", i + 1), rpm: *r })
                .collect(),
        })
    }

    fn apply_update(&self) -> ApiResult<()> {
        Err("demo mode: no updates to install".into())
    }

    fn install_ryzenadj(&self) -> ApiResult<()> {
        std::thread::sleep(std::time::Duration::from_millis(800));
        Ok(())
    }

    fn uninstall_ryzenadj(&self) -> ApiResult<()> {
        Ok(())
    }

    fn framework_tool_versions(&self) -> ApiResult<FrameworkToolVersions> {
        Ok(FrameworkToolVersions {
            current_version: Some("0.4.5".into()),
            latest_tag: Some("v0.4.5".into()),
            available_tags: vec!["v0.4.5".into(), "v0.4.4".into(), "v0.4.3".into()],
        })
    }

    fn switch_framework_tool(&self, _version: Option<String>) -> ApiResult<()> {
        std::thread::sleep(std::time::Duration::from_millis(800));
        Ok(())
    }

    fn logs(&self) -> ApiResult<String> {
        Ok((0..40)
            .map(|i| format!("INFO framework_control_service::tasks::telemetry: sample {i} collected"))
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn describe(&self) -> String {
        "demo data".into()
    }
}
