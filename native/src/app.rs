use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Ui, Vec2};
use serde_json::{json, Value};

use crate::api::*;
use crate::pages;
use crate::state::{self, Cmd, Shared, Snapshot};
use crate::theme::{self, Colors, WindowsLook};
use crate::widgets::{self, Icon};

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
    fn icon(self) -> Icon {
        match self {
            Page::Overview => Icon::Home,
            Page::Fans => Icon::Fan,
            Page::Power => Icon::Power,
            Page::Battery => Icon::Battery,
            Page::Tools => Icon::Tools,
            Page::Settings => Icon::Settings,
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
    theme_state: Option<(Colors, Instant)>,
    windows_look: Option<WindowsLook>,
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
            theme_state: None,
            windows_look: None,
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
        crate::theme::apply(ctx, &theme_pref, &mut self.theme_state, &mut self.windows_look);
        // Follow changes to Windows' light/dark setting and accent colour.
        ctx.request_repaint_after(Duration::from_secs(2));
        let c = theme::colors(ctx);

        if let Some(t) = self.close_at {
            if Instant::now() >= t {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ctx.request_repaint_after(Duration::from_millis(200));
            }
        }

        egui::SidePanel::left("nav")
            .exact_width(260.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().inner_margin(Margin { left: 8, right: 8, top: 14, bottom: 14 }).fill(c.window_bg))
            .show(ctx, |ui| self.nav(ui, &snap));

        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(Margin { left: 24, right: 8, top: 14, bottom: 0 }).fill(c.window_bg))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    ui.set_max_width(ui.available_width() - 16.0);
                    // Tools don't need the service, so they stay usable while it's down.
                    if !snap.connected && snap.health.is_none() && self.page != Page::Tools {
                        disconnected(ui, &snap, &self.source);
                        return;
                    }
                    widgets::page_title(ui, self.page.label());
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
                    ui.add_space(24.0);
                });
            });

        pages::fans::modal(self, ctx);
        pages::settings::windows(self, ctx);
        drop(snap);
        self.autosave(ctx);
    }
}

impl App {
    /// The menu on the left, laid out like White Rabbit's (and the Windows 11 Settings app).
    fn nav(&mut self, ui: &mut Ui, snap: &Snapshot) {
        let c = theme::colors(ui.ctx());
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.add(egui::Image::new(egui::include_image!("../assets/icon.png")).fit_to_exact_size(Vec2::splat(20.0)));
            ui.add_space(4.0);
            ui.label(RichText::new(format!("Framework Control {}", short_version())).size(12.0).color(c.text));
        });
        ui.add_space(14.0);
        let cfg_ui = snap.config.as_ref().map(|c| c.ui.clone()).unwrap_or_default();
        for p in Page::ALL {
            let off = p.panel().is_some_and(|id| !cfg_ui.panel_enabled(id));
            if nav_item(ui, p.icon(), p.label(), self.page == p, off).clicked() {
                self.page = p;
            }
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add_space(2.0);
            if let Some((msg, true)) = &snap.write_status {
                ui.add(egui::Label::new(RichText::new(msg).size(12.0).color(c.error)).wrap());
            }
            if let Some(u) = &snap.update {
                if u.updates_enabled && crate::version::is_newer(&u.latest_version, &u.current_version) {
                    if ui.link(RichText::new(format!("Version {} is available", u.latest_version)).size(12.0)).clicked() {
                        self.page = Page::Settings;
                    }
                }
            }
            if let Some(v) = snap.versions.as_ref().and_then(|v| v.mainboard_type.clone()) {
                ui.label(RichText::new(v).size(12.0).color(c.text2));
            }
            let (dot, text) = match (&snap.health, snap.connected) {
                (Some(h), true) => (c.ok, format!("Service connected · v{}", h.service_version)),
                _ => (c.error, "Service not running".to_string()),
            };
            ui.horizontal(|ui| {
                let (r, p) = ui.allocate_painter(Vec2::new(10.0, 14.0), egui::Sense::hover());
                p.circle_filled(r.rect.center(), 4.0, dot);
                ui.label(RichText::new(text).size(12.0).color(c.text2));
            });
            ui.label(RichText::new("Background service").font(theme::semibold(14.0)).color(c.text));
        });
    }

    fn feature_off(&mut self, ui: &mut Ui, cfg_ui: &UiConfig, panel: PanelId) {
        let mut on = false;
        widgets::card(
            ui,
            Some(self.page.icon()),
            &format!("{} is turned off", self.page.label()),
            "Turning a feature off also stops the service's background work for it.",
            false,
            |ui| {
                if widgets::switch(ui, &mut on) {
                    let panels = cfg_ui.with_panel(panel, true);
                    self.patch(json!({ "ui": { "panels": panels } }));
                }
            },
        );
    }
}

/// "1.2" from "1.2.0", like White Rabbit's title.
fn short_version() -> &'static str {
    let v = env!("CARGO_PKG_VERSION");
    v.strip_suffix(".0").unwrap_or(v)
}

/// One menu entry: icon and label, a light fill when chosen or hovered, and the accent pill.
fn nav_item(ui: &mut Ui, icon: Icon, label: &str, selected: bool, dimmed: bool) -> egui::Response {
    let c = theme::colors(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 36.0), egui::Sense::click());
    let p = ui.painter();
    let fill = if selected {
        c.selected
    } else if resp.hovered() {
        c.hover
    } else {
        Color32::TRANSPARENT
    };
    p.rect_filled(rect, CornerRadius::same(4), fill);
    if selected {
        let pill = egui::Rect::from_center_size(egui::pos2(rect.left() + 1.5, rect.center().y), Vec2::new(3.0, 16.0));
        p.rect_filled(pill, CornerRadius::same(2), c.accent);
    }
    let text_color = if dimmed { c.text3 } else { c.text };
    let icon_rect = egui::Rect::from_center_size(egui::pos2(rect.left() + 24.0, rect.center().y), Vec2::splat(16.0));
    icon.image(16.0, text_color).paint_at(ui, icon_rect);
    p.text(
        egui::pos2(rect.left() + 46.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(14.0),
        text_color,
    );
    ui.add_space(2.0);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn disconnected(ui: &mut Ui, snap: &Snapshot, source: &str) {
    widgets::page_title(ui, "Can't reach the background service");
    widgets::card(
        ui,
        Some(Icon::Warning),
        "The Framework Control service isn't running",
        &format!("Tried {source}. This window only shows your settings; the background service does the hardware work."),
        false,
        |_| {},
    );
    if let Some(e) = &snap.last_error {
        widgets::card(ui, Some(Icon::Info), "Details", e, false, |_| {});
    }
    if widgets::card(
        ui,
        Some(Icon::Download),
        "Install or repair Framework Control",
        "Download the latest installer from GitHub. Use --port <n> if the service listens on another port.",
        true,
        |_| {},
    )
    .clicked()
    {
        ui.ctx().open_url(egui::OpenUrl::new_tab(crate::version::RELEASES_URL));
    }
}
