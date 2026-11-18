use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Stroke, Ui, Vec2};
use serde_json::{json, Value};

use crate::api::*;
use crate::pages;
use crate::state::{self, Cmd, Shared, Snapshot};
use crate::widgets;

/// Edits are saved automatically this long after the last change (like the web UI).
const AUTOSAVE_AFTER: Duration = Duration::from_millis(600);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Overview,
    Fans,
    Power,
    Battery,
    Tools,
    Settings,
}

impl Page {
    const ALL: [Page; 6] = [Page::Overview, Page::Fans, Page::Power, Page::Battery, Page::Tools, Page::Settings];
    fn label(self) -> &'static str {
        match self {
            Page::Overview => "Overview",
            Page::Fans => "Fans",
            Page::Power => "Power",
            Page::Battery => "Battery",
            Page::Tools => "Tools",
            Page::Settings => "Settings",
        }
    }
    fn icon(self) -> &'static str {
        match self {
            Page::Overview => "⌂",
            Page::Fans => "✳",
            Page::Power => "⚡",
            Page::Battery => "▮",
            Page::Tools => "⚒",
            Page::Settings => "⚙",
        }
    }
    /// The service feature (dashboard panel) a page belongs to.
    pub fn panel(self) -> Option<PanelId> {
        match self {
            Page::Overview => Some(PanelId::Telemetry),
            Page::Fans => Some(PanelId::Fan),
            Page::Power => Some(PanelId::Power),
            Page::Battery => Some(PanelId::Battery),
            Page::Tools | Page::Settings => None,
        }
    }
}

/// A locally edited copy of a config section, saved automatically.
pub struct Draft<T> {
    pub value: Option<T>,
    edited: Option<Instant>,
}

impl<T> Default for Draft<T> {
    fn default() -> Self {
        Self { value: None, edited: None }
    }
}

impl<T> Draft<T> {
    pub fn touch(&mut self) {
        self.edited = Some(Instant::now());
    }
    fn dirty(&self) -> bool {
        self.edited.is_some()
    }
}

#[derive(Default)]
pub struct Drafts {
    rev: u64,
    pub fan: Draft<FanControlConfig>,
    pub power: Draft<PowerConfig>,
    pub battery: Draft<BatteryConfig>,
}

pub struct App {
    pub shared: Shared,
    pub tx: Sender<Cmd>,
    pub backend: Arc<dyn Backend>,
    pub page: Page,
    pub drafts: Drafts,
    writes_sent: u64,
    pub source: String,
    applied_theme: String,
    pub fans: pages::fans::FansUi,
    pub power: pages::power::PowerUi,
    pub settings: pages::settings::SettingsUi,
    pub tools: pages::tools::ToolsUi,
    pub close_at: Option<Instant>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, backend: Arc<dyn Backend>, page: Option<&str>) -> Self {
        crate::theme::install(&cc.egui_ctx);
        let source = backend.describe();
        let (shared, tx) = state::spawn(backend.clone(), cc.egui_ctx.clone());
        let page = match page.unwrap_or("") {
            "fans" => Page::Fans,
            "power" => Page::Power,
            "battery" => Page::Battery,
            "tools" => Page::Tools,
            "settings" => Page::Settings,
            _ => Page::Overview,
        };
        Self {
            shared,
            tx,
            backend,
            page,
            drafts: Drafts::default(),
            writes_sent: 0,
            source,
            applied_theme: String::new(),
            fans: Default::default(),
            power: Default::default(),
            settings: Default::default(),
            tools: Default::default(),
            close_at: None,
        }
    }

    /// Demo-only: open a specific UI state, used to generate README screenshots.
    pub fn scene(&mut self, scene: &str, ctx: &egui::Context) {
        match scene {
            "fan2" => {
                self.page = Page::Fans;
                self.fans.select_tab(Some(1));
            }
            "calibrate" => {
                self.page = Page::Fans;
                self.fans.open_calibration();
            }
            "logs" => {
                self.page = Page::Settings;
                self.settings.open_logs(&self.backend, ctx);
            }
            "light" => self.patch(json!({ "ui": { "theme": "light" } })),
            _ => {}
        }
    }

    /// Send a partial config update through the poller.
    pub fn patch(&mut self, patch: Value) {
        self.writes_sent += 1;
        let _ = self.tx.send(Cmd::Patch(patch));
    }

    pub fn refresh(&self) {
        let _ = self.tx.send(Cmd::RefreshAll);
    }

    fn sync_drafts(&mut self, snap: &Snapshot) {
        let Some(cfg) = &snap.config else { return };
        // Don't let a config fetched before our own write land over the edit.
        if snap.writes_applied < self.writes_sent {
            return;
        }
        if self.drafts.rev == snap.config_rev && self.drafts.fan.value.is_some() {
            return;
        }
        self.drafts.rev = snap.config_rev;
        if !self.drafts.fan.dirty() {
            self.drafts.fan.value = Some(cfg.fan.clone());
        }
        if !self.drafts.power.dirty() {
            self.drafts.power.value = Some(cfg.power.clone());
        }
        if !self.drafts.battery.dirty() {
            self.drafts.battery.value = Some(cfg.battery.clone());
        }
    }

    fn autosave(&mut self, ctx: &egui::Context) {
        // Wait until the user lets go of a slider/curve point.
        if ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        let mut patches = Vec::new();
        let mut wake: Option<Duration> = None;
        let mut due = |edited: &mut Option<Instant>| -> bool {
            let Some(t) = *edited else { return false };
            let age = t.elapsed();
            if age >= AUTOSAVE_AFTER {
                *edited = None;
                true
            } else {
                let left = AUTOSAVE_AFTER - age;
                wake = Some(wake.map_or(left, |w| w.min(left)));
                false
            }
        };
        if due(&mut self.drafts.fan.edited) {
            if let Some(f) = &self.drafts.fan.value {
                patches.push(json!({ "fan": {
                    "profiles": f.profiles,
                    "active_profile": f.active_profile,
                    "auto_switch": f.auto_switch,
                }}));
            }
        }
        if due(&mut self.drafts.power.edited) {
            if let Some(p) = &self.drafts.power.value {
                patches.push(json!({ "power": p }));
            }
        }
        if due(&mut self.drafts.battery.edited) {
            if let Some(b) = &self.drafts.battery.value {
                patches.push(json!({ "battery": b }));
            }
        }
        for p in patches {
            self.patch(p);
        }
        if let Some(w) = wake {
            ctx.request_repaint_after(w);
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let shared = self.shared.clone();
        let snap = shared.lock().unwrap();
        self.sync_drafts(&snap);
        let theme_pref = snap.config.as_ref().and_then(|c| c.ui.theme.clone()).unwrap_or_else(|| "system".into());
        crate::theme::apply(ctx, &theme_pref, &mut self.applied_theme);

        if let Some(t) = self.close_at {
            if Instant::now() >= t {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ctx.request_repaint_after(Duration::from_millis(200));
            }
        }

        egui::TopBottomPanel::top("header")
            .frame(egui::Frame::new().inner_margin(Margin::symmetric(16, 10)).fill(ctx.style().visuals.panel_fill))
            .show(ctx, |ui| self.header(ui, &snap));

        egui::SidePanel::left("nav")
            .exact_width(176.0)
            .resizable(false)
            .frame(egui::Frame::new().inner_margin(Margin::same(10)).fill(ctx.style().visuals.panel_fill))
            .show(ctx, |ui| self.nav(ui, &snap));

        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(Margin::same(16)).fill(ctx.style().visuals.extreme_bg_color))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    // Tools don't need the service, so they stay usable while it's down.
                    if !snap.connected && snap.health.is_none() && self.page != Page::Tools {
                        disconnected(ui, &snap, &self.source);
                        return;
                    }
                    if let Some(panel) = self.page.panel() {
                        let cfg_ui = snap.config.as_ref().map(|c| c.ui.clone()).unwrap_or_default();
                        if !cfg_ui.panel_enabled(panel) {
                            self.feature_off(ui, &cfg_ui, panel);
                            return;
                        }
                    }
                    match self.page {
                        Page::Overview => pages::overview::show(self, ui, &snap),
                        Page::Fans => pages::fans::show(self, ui, &snap),
                        Page::Power => pages::power::show(self, ui, &snap),
                        Page::Battery => pages::battery::show(self, ui, &snap),
                        Page::Tools => pages::tools::show(self, ui),
                        Page::Settings => pages::settings::show(self, ui, &snap),
                    }
                });
            });

        pages::fans::modal(self, ctx);
        pages::settings::windows(self, ctx);
        drop(snap);
        self.autosave(ctx);
    }
}

impl App {
    fn header(&mut self, ui: &mut Ui, snap: &Snapshot) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Framework Control").size(18.0).strong());
            if let Some(v) = snap.versions.as_ref().and_then(|v| v.mainboard_type.clone()) {
                ui.label(RichText::new(v).weak());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let dark = ui.visuals().dark_mode;
                let (color, text) = if snap.connected {
                    (crate::theme::success(dark), "Service connected")
                } else {
                    (crate::theme::error(dark), "Service offline")
                };
                ui.label(RichText::new(text).weak());
                let (r, p) = ui.allocate_painter(Vec2::splat(10.0), egui::Sense::hover());
                p.circle_filled(r.rect.center(), 4.0, color);
                if let Some(u) = &snap.update {
                    if u.updates_enabled && crate::version::is_newer(&u.latest_version, &u.current_version) {
                        ui.add_space(8.0);
                        if ui.link(format!("Update {} available", u.latest_version)).clicked() {
                            self.page = Page::Settings;
                        }
                    }
                }
                if let Some((msg, err)) = &snap.write_status {
                    if *err {
                        ui.add_space(12.0);
                        ui.label(RichText::new(msg).color(crate::theme::error(dark)));
                    }
                }
            });
        });
    }

    fn nav(&mut self, ui: &mut Ui, snap: &Snapshot) {
        ui.add_space(4.0);
        let cfg_ui = snap.config.as_ref().map(|c| c.ui.clone()).unwrap_or_default();
        for p in Page::ALL {
            let selected = self.page == p;
            let off = p.panel().is_some_and(|id| !cfg_ui.panel_enabled(id));
            let mut text = RichText::new(format!("{}   {}", p.icon(), p.label())).size(14.5);
            if off {
                text = text.weak();
            }
            let btn = egui::Button::new(text)
                .min_size(Vec2::new(ui.available_width(), 34.0))
                .corner_radius(CornerRadius::same(6))
                .fill(if selected { ui.visuals().selection.bg_fill } else { Color32::TRANSPARENT })
                .stroke(Stroke::NONE);
            if ui.add(btn).clicked() {
                self.page = p;
            }
            ui.add_space(2.0);
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.label(RichText::new(format!("App v{}", env!("CARGO_PKG_VERSION"))).small().weak());
            if let Some(h) = &snap.health {
                ui.label(RichText::new(format!("Service v{}", h.service_version)).small().weak());
            }
            ui.label(RichText::new(&self.source).small().weak());
        });
    }

    fn feature_off(&mut self, ui: &mut Ui, cfg_ui: &UiConfig, panel: PanelId) {
        widgets::card(ui, |ui| {
            widgets::section_title(ui, &format!("{} is turned off", self.page.label()));
            ui.label("Turning a feature off also stops the service's background work for it.");
            ui.add_space(6.0);
            if widgets::primary_button(ui, "Turn on").clicked() {
                let panels = cfg_ui.with_panel(panel, true);
                self.patch(json!({ "ui": { "panels": panels } }));
            }
        });
    }
}

fn disconnected(ui: &mut Ui, snap: &Snapshot, source: &str) {
    widgets::card(ui, |ui| {
        ui.heading("Can't reach the Framework Control service");
        ui.add_space(6.0);
        ui.label(format!("Tried {source}. Make sure the service is installed and running."));
        ui.label("This app is only the window; the background service does the hardware work.");
        if let Some(e) = &snap.last_error {
            ui.add_space(6.0);
            ui.label(RichText::new(e).weak());
        }
        ui.add_space(6.0);
        ui.hyperlink_to("Download the latest installer", crate::version::RELEASES_URL);
        ui.label(RichText::new("Use --port <n> if the service listens on a non-default port.").weak());
    });
}
