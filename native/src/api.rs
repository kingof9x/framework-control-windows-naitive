//! Types mirrored from the service's OpenAPI models plus a tiny blocking HTTP
//! client. Unknown fields are kept in `extra` maps so writing a section back
//! never drops data added by a newer service.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

// ---------- Read-only responses ----------

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Health {
    pub cli_present: bool,
    pub service_version: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TelemetrySample {
    pub ts_ms: i64,
    #[serde(default)]
    pub temps: BTreeMap<String, i32>,
    #[serde(default)]
    pub rpms: Vec<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BatterySample {
    pub ts_ms: i64,
    pub charge_pct: Option<f32>,
    pub watts: Option<f32>,
    #[allow(dead_code)]
    pub ac_present: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct FanReading {
    pub name: String,
    pub rpm: u32,
}

/// Live `/thermal` reading (bypasses the service's history buffer).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ThermalNow {
    pub temps: BTreeMap<String, i32>,
    pub fans: Vec<FanReading>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct FrameworkToolVersions {
    pub current_version: Option<String>,
    pub latest_tag: Option<String>,
    pub available_tags: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct BatteryInfo {
    pub ac_present: Option<bool>,
    pub battery_present: Option<bool>,
    pub last_full_charge_capacity_mah: Option<u32>,
    pub remaining_capacity_mah: Option<u32>,
    pub percentage: Option<u32>,
    pub present_voltage_mv: Option<u32>,
    pub present_rate_ma: Option<u32>,
    pub design_capacity_mah: Option<u32>,
    pub cycle_count: Option<u32>,
    pub charging: Option<bool>,
    pub discharging: Option<bool>,
    pub charge_limit_min_pct: Option<u8>,
    pub charge_limit_max_pct: Option<u8>,
}

impl BatteryInfo {
    /// Signed pack power in watts (positive = charging).
    pub fn watts(&self) -> Option<f32> {
        let v = self.present_voltage_mv? as f32 / 1000.0;
        let a = self.present_rate_ma? as f32 / 1000.0;
        let sign = if self.discharging == Some(true) { -1.0 } else { 1.0 };
        Some(v * a * sign)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PowerCapabilities {
    pub supports_tdp: bool,
    pub supports_thermal: bool,
    pub supports_epp: bool,
    pub supports_governor: bool,
    pub supports_frequency_limits: bool,
    pub available_epp_preferences: Option<Vec<String>>,
    pub available_governors: Option<Vec<String>>,
    pub frequency_min_mhz: Option<u32>,
    pub frequency_max_mhz: Option<u32>,
    pub tdp_min_watts: Option<u32>,
    pub tdp_max_watts: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PowerState {
    pub tdp_limit_watts: Option<u32>,
    pub thermal_limit_c: Option<u32>,
    pub epp_preference: Option<String>,
    pub governor: Option<String>,
    pub min_freq_mhz: Option<u32>,
    pub max_freq_mhz: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PowerControlInfo {
    pub capabilities: PowerCapabilities,
    pub current_state: PowerState,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PowerResponse {
    pub battery: Option<BatteryInfo>,
    pub power_control: PowerControlInfo,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Versions {
    pub tool_version: Option<String>,
    pub mainboard_type: Option<String>,
    pub mainboard_revision: Option<String>,
    pub uefi_version: Option<String>,
    pub uefi_release_date: Option<String>,
    pub ec_build_version: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SystemInfo {
    pub cpu: String,
    pub memory_total_mb: u64,
    pub os: String,
    pub dgpu: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateCheck {
    pub current_version: String,
    pub latest_version: String,
    pub updates_enabled: bool,
}

// ---------- Config (read + write) ----------

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Config {
    #[serde(default)]
    pub fan: FanControlConfig,
    #[serde(default)]
    pub power: PowerConfig,
    #[serde(default)]
    pub battery: BatteryConfig,
    #[serde(default)]
    pub updates: UpdatesConfig,
    #[serde(default)]
    pub telemetry: TelemetryConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub framework_tool: FrameworkToolConfig,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FrameworkToolConfig {
    #[serde(default = "yes")]
    pub latest: bool,
}

impl Default for FrameworkToolConfig {
    fn default() -> Self {
        Self { latest: true }
    }
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct UiConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panels: Option<Vec<DashboardPanel>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PanelId {
    Telemetry,
    Fan,
    Power,
    Battery,
}

impl PanelId {
    pub const ALL: [PanelId; 4] = [PanelId::Telemetry, PanelId::Fan, PanelId::Power, PanelId::Battery];
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DashboardPanel {
    pub id: PanelId,
    pub enabled: bool,
    #[serde(default = "half")]
    pub size: String,
}

fn half() -> String {
    "half".into()
}

impl UiConfig {
    /// Absent layout or a missing id means that panel is enabled (same rule as the service).
    pub fn panel_enabled(&self, id: PanelId) -> bool {
        self.panels
            .as_ref()
            .and_then(|ps| ps.iter().find(|p| p.id == id))
            .map(|p| p.enabled)
            .unwrap_or(true)
    }

    /// Full panel list with `id` toggled, preserving order and size used by the web UI.
    pub fn with_panel(&self, id: PanelId, enabled: bool) -> Vec<DashboardPanel> {
        let mut panels = self.panels.clone().unwrap_or_default();
        for pid in PanelId::ALL {
            if !panels.iter().any(|p| p.id == pid) {
                panels.push(DashboardPanel { id: pid, enabled: true, size: half() });
            }
        }
        for p in &mut panels {
            if p.id == id {
                p.enabled = enabled;
            }
        }
        panels
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FanControlConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibration: Option<FanCalibration>,
    #[serde(default)]
    pub profiles: Vec<FanProfile>,
    #[serde(default)]
    pub active_profile: String,
    #[serde(default)]
    pub auto_switch: AutoSwitch,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AutoSwitch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ac: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battery: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FanCalibration {
    pub updated_at: i64,
    #[serde(default)]
    pub fans: Vec<PerFanCalibration>,
}

/// `points` are `[duty_pct, rpm]`, sorted by duty.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PerFanCalibration {
    pub index: u32,
    pub points: Vec<[u32; 2]>,
}

impl FanCalibration {
    /// Estimate the duty % a fan is running at from its RPM.
    pub fn duty_for_rpm(&self, fan: usize, rpm: u32) -> Option<f64> {
        let pts = &self.fans.iter().find(|f| f.index as usize == fan)?.points;
        if pts.len() < 2 {
            return None;
        }
        let mut sorted: Vec<(f64, f64)> = pts.iter().map(|p| (p[1] as f64, p[0] as f64)).collect();
        sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
        let r = rpm as f64;
        if r <= sorted[0].0 {
            return Some(sorted[0].1);
        }
        for w in sorted.windows(2) {
            let ((r0, d0), (r1, d1)) = (w[0], w[1]);
            if r <= r1 {
                let t = if r1 > r0 { (r - r0) / (r1 - r0) } else { 0.0 };
                return Some((d0 + t * (d1 - d0)).clamp(0.0, 100.0));
            }
        }
        Some(sorted.last()?.1)
    }
}

impl FanControlConfig {
    pub fn active(&self) -> Option<&FanProfile> {
        self.profiles.iter().find(|p| p.id == self.active_profile)
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FanMode {
    #[default]
    Disabled,
    Manual,
    Curve,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FanProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub mode: FanMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual: Option<ManualConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<GlobalCurveConfig>,
    #[serde(default)]
    pub overrides: Vec<FanOverride>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Per-fan override of the profile's manual duty and/or curve.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FanOverride {
    pub index: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual: Option<ManualConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<CurveConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManualConfig {
    pub duty_pct: u32,
}

/// The profile-wide curve: a [`CurveConfig`] plus the shared control-loop interval.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GlobalCurveConfig {
    #[serde(flatten)]
    pub curve: CurveConfig,
    #[serde(default = "default_poll")]
    pub poll_ms: u64,
}

impl Default for GlobalCurveConfig {
    fn default() -> Self {
        Self { curve: CurveConfig::default(), poll_ms: default_poll() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CurveConfig {
    #[serde(default)]
    pub sensors: Vec<String>,
    #[serde(default = "default_points")]
    pub points: Vec<[u32; 2]>,
    #[serde(default = "default_hysteresis")]
    pub hysteresis_c: u32,
    #[serde(default = "default_rate")]
    pub rate_limit_pct_per_step: u32,
    /// Separate limit for slowing down; None = same as `rate_limit_pct_per_step`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit_down_pct_per_step: Option<u32>,
}

impl Default for CurveConfig {
    fn default() -> Self {
        Self {
            sensors: Vec::new(),
            points: default_points(),
            hysteresis_c: default_hysteresis(),
            rate_limit_pct_per_step: default_rate(),
            rate_limit_down_pct_per_step: None,
        }
    }
}

fn default_points() -> Vec<[u32; 2]> {
    vec![[40, 0], [60, 40], [75, 80], [85, 100]]
}
fn default_hysteresis() -> u32 {
    2
}
fn default_rate() -> u32 {
    100
}
fn default_poll() -> u64 {
    2000
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SettingU32 {
    pub enabled: bool,
    pub value: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SettingString {
    pub enabled: bool,
    pub value: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SettingU8 {
    pub enabled: bool,
    pub value: u8,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SettingF32 {
    pub enabled: bool,
    pub value: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PowerProfile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tdp_watts: Option<SettingU32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thermal_limit_c: Option<SettingU32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epp_preference: Option<SettingString>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governor: Option<SettingString>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_freq_mhz: Option<SettingU32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_freq_mhz: Option<SettingU32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PowerConfig {
    #[serde(default)]
    pub ac: Option<PowerProfile>,
    #[serde(default)]
    pub battery: Option<PowerProfile>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct BatteryConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub charge_limit_max_pct: Option<SettingU8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub charge_rate_c: Option<SettingF32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub charge_rate_soc_threshold_pct: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct UpdatesConfig {
    #[serde(default)]
    pub auto_install: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TelemetryConfig {
    #[serde(default = "default_poll")]
    pub poll_ms: u64,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self { poll_ms: default_poll() }
    }
}

// ---------- Backend abstraction ----------

pub type ApiResult<T> = Result<T, String>;

/// Everything the UI needs from the service. Implemented over HTTP and by the
/// in-memory demo backend.
pub trait Backend: Send + Sync {
    fn health(&self) -> ApiResult<Health>;
    fn thermal_history(&self) -> ApiResult<Vec<TelemetrySample>>;
    fn battery_history(&self) -> ApiResult<Vec<BatterySample>>;
    fn power(&self) -> ApiResult<PowerResponse>;
    fn config(&self) -> ApiResult<Config>;
    fn versions(&self) -> ApiResult<Versions>;
    fn system(&self) -> ApiResult<SystemInfo>;
    fn check_update(&self) -> ApiResult<UpdateCheck>;
    /// POST a partial config (e.g. `{"battery": {...}}`); returns the merged config.
    fn patch_config(&self, patch: Value) -> ApiResult<Config>;
    fn thermal_now(&self) -> ApiResult<ThermalNow>;
    fn apply_update(&self) -> ApiResult<()>;
    fn install_ryzenadj(&self) -> ApiResult<()>;
    fn uninstall_ryzenadj(&self) -> ApiResult<()>;
    fn framework_tool_versions(&self) -> ApiResult<FrameworkToolVersions>;
    /// `None` switches to (and follows) the latest release.
    fn switch_framework_tool(&self, version: Option<String>) -> ApiResult<()>;
    fn logs(&self) -> ApiResult<String>;
    fn describe(&self) -> String;
}

pub struct HttpBackend {
    base: String,
    agent: ureq::Agent,
}

impl HttpBackend {
    pub fn new(port: u16) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(2))
            .timeout(Duration::from_secs(15))
            .build();
        Self {
            base: format!("http://127.0.0.1:{port}/api"),
            agent,
        }
    }

    fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> ApiResult<T> {
        let resp = self
            .agent
            .get(&format!("{}{}", self.base, path))
            .call()
            .map_err(err_str)?;
        resp.into_json::<T>().map_err(|e| e.to_string())
    }

    fn post(&self, path: &str, body: Value) -> ApiResult<ureq::Response> {
        self.agent
            .post(&format!("{}{}", self.base, path))
            .timeout(Duration::from_secs(300))
            .send_json(body)
            .map_err(err_str)
    }
}

fn err_str(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            // Service errors are `{code, message}` envelopes.
            serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(String::from))
                .unwrap_or_else(|| format!("HTTP {code}"))
        }
        ureq::Error::Transport(t) => format!("service unreachable ({})", t.kind()),
    }
}

impl Backend for HttpBackend {
    fn health(&self) -> ApiResult<Health> {
        self.get("/health")
    }
    fn thermal_history(&self) -> ApiResult<Vec<TelemetrySample>> {
        self.get("/thermal/history")
    }
    fn battery_history(&self) -> ApiResult<Vec<BatterySample>> {
        self.get("/battery/history")
    }
    fn power(&self) -> ApiResult<PowerResponse> {
        self.get("/power")
    }
    fn config(&self) -> ApiResult<Config> {
        self.get("/config")
    }
    fn versions(&self) -> ApiResult<Versions> {
        self.get("/versions")
    }
    fn system(&self) -> ApiResult<SystemInfo> {
        self.get("/system")
    }
    fn check_update(&self) -> ApiResult<UpdateCheck> {
        self.get("/update/check")
    }
    fn patch_config(&self, patch: Value) -> ApiResult<Config> {
        let resp = self
            .agent
            .post(&format!("{}/config?client_id=native", self.base))
            .send_json(patch)
            .map_err(err_str)?;
        resp.into_json::<Config>().map_err(|e| e.to_string())
    }
    fn thermal_now(&self) -> ApiResult<ThermalNow> {
        self.get("/thermal")
    }
    fn apply_update(&self) -> ApiResult<()> {
        self.post("/update/apply", json!({})).map(|_| ())
    }
    fn install_ryzenadj(&self) -> ApiResult<()> {
        self.post("/ryzenadj/install", json!({})).map(|_| ())
    }
    fn uninstall_ryzenadj(&self) -> ApiResult<()> {
        self.post("/ryzenadj/uninstall", json!({})).map(|_| ())
    }
    fn framework_tool_versions(&self) -> ApiResult<FrameworkToolVersions> {
        self.get("/framework_tool/versions")
    }
    fn switch_framework_tool(&self, version: Option<String>) -> ApiResult<()> {
        self.post("/framework_tool/switch", json!({ "version": version })).map(|_| ())
    }
    fn logs(&self) -> ApiResult<String> {
        self.agent
            .get(&format!("{}/logs", self.base))
            .call()
            .map_err(err_str)?
            .into_string()
            .map_err(|e| e.to_string())
    }
    fn describe(&self) -> String {
        self.base.trim_end_matches("/api").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Run against a live service: `FC_TEST_PORT=8090 cargo test -- --ignored`
    #[test]
    #[ignore]
    fn live_round_trip() {
        let port: u16 = std::env::var("FC_TEST_PORT").unwrap().parse().unwrap();
        let api = HttpBackend::new(port);
        let mut cfg = api.config().unwrap();
        let p = &mut cfg.fan.profiles[0];
        p.mode = FanMode::Curve;
        p.curve = Some(GlobalCurveConfig {
            curve: CurveConfig { points: vec![[45, 10], [70, 50], [90, 100]], ..Default::default() },
            poll_ms: 1500,
        });
        p.overrides = vec![FanOverride { index: 1, manual: Some(ManualConfig { duty_pct: 30 }), curve: None }];
        let out = api
            .patch_config(json!({ "fan": { "profiles": cfg.fan.profiles, "active_profile": cfg.fan.active_profile } }))
            .unwrap();
        assert_eq!(out.fan.profiles[0].mode, FanMode::Curve);
        assert_eq!(out.fan.profiles[0].curve.as_ref().unwrap().curve.points[1], [70, 50]);
        assert_eq!(out.fan.profiles[0].curve.as_ref().unwrap().poll_ms, 1500);
        assert_eq!(out.fan.profiles[0].overrides[0].manual.as_ref().unwrap().duty_pct, 30);

        // Calibration + auto-switch + panels
        let cal = FanCalibration { updated_at: 1, fans: vec![PerFanCalibration { index: 0, points: vec![[0, 0], [100, 5000]] }] };
        let out = api
            .patch_config(json!({ "fan": { "calibration": cal, "auto_switch": { "ac": "default" } } }))
            .unwrap();
        assert_eq!(out.fan.auto_switch.ac.as_deref(), Some("default"));
        assert_eq!(out.fan.calibration.unwrap().duty_for_rpm(0, 2500).unwrap().round(), 50.0);
        let panels = out.ui.with_panel(PanelId::Power, false);
        let out = api.patch_config(json!({ "ui": { "panels": panels, "theme": "dark" } })).unwrap();
        assert!(!out.ui.panel_enabled(PanelId::Power));
        assert!(out.ui.panel_enabled(PanelId::Fan));
        // Restore
        let panels = out.ui.with_panel(PanelId::Power, true);
        api.patch_config(json!({ "ui": { "panels": panels } })).unwrap();

        // Read-only endpoints deserialize
        api.framework_tool_versions().unwrap();
        let _ = api.logs();

        let power = PowerConfig {
            ac: Some(PowerProfile { tdp_watts: Some(SettingU32 { enabled: true, value: 40 }), ..Default::default() }),
            battery: None,
        };
        let out = api.patch_config(json!({ "power": power })).unwrap();
        assert_eq!(out.power.ac.unwrap().tdp_watts.unwrap().value, 40);

        let bat = BatteryConfig {
            charge_limit_max_pct: Some(SettingU8 { enabled: true, value: 75 }),
            ..Default::default()
        };
        let out = api.patch_config(json!({ "battery": bat })).unwrap();
        assert_eq!(out.battery.charge_limit_max_pct.unwrap().value, 75);

        let out = api.patch_config(json!({ "telemetry": { "poll_ms": 1500 } })).unwrap();
        assert_eq!(out.telemetry.poll_ms, 1500);
        // Unknown sections (e.g. `version`) survive in `extra`.
        assert!(out.extra.contains_key("version"));
    }

    #[test]
    fn tolerates_service_nulls() {
        // Error envelope parsing is exercised by the live test; here just make
        // sure the types tolerate the service's explicit nulls.
        let raw = r#"{"battery":{"charge_limit_max_pct":null,"charge_rate_c":null},"fan":{"active_profile":"default","profiles":[{"curve":null,"id":"default","manual":null,"mode":"disabled","name":"Default","overrides":[]}]},"power":{"ac":null,"battery":null},"telemetry":{"poll_ms":2000},"updates":{"auto_install":false},"version":1}"#;
        let cfg: Config = serde_json::from_str(raw).unwrap();
        let back = serde_json::to_value(&cfg.fan.profiles[0]).unwrap();
        assert_eq!(back["overrides"], json!([]));
        assert!(back.get("curve").is_none());
    }
}
