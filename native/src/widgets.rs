//! Building blocks in the style of the Windows 11 Settings app, matching White Rabbit's
//! `widgets.slint`: cards with an icon, a title and a line of help, switches, accent buttons,
//! section titles and Fluent icons.

use eframe::egui::{self, Align, Color32, CornerRadius, Frame, Image, ImageSource, Layout, Margin, RichText, Sense, Stroke, Ui, Vec2};

use crate::api::{SettingString, SettingU32};
use crate::theme::{self, colors};

// ---------------------------------------------------------------- icons ---

/// Fluent UI System Icons (MIT, Microsoft), the set White Rabbit uses. Drawn white and tinted.
#[derive(Clone, Copy)]
pub enum Icon {
    Home,
    Fan,
    Power,
    Battery,
    Tools,
    Settings,
    ChevronRight,
    Theme,
    Sync,
    Download,
    Board,
    Logs,
    Info,
    Keyboard,
    Panels,
    Curve,
    Gauge,
    Plug,
    BatteryCharge,
    Speed,
    Timer,
    Temperature,
    Target,
    Options,
    Edit,
    Heart,
    Pulse,
    Warning,
    Delete,
    Sort,
    Open,
    Check,
    Sliders,
}

impl Icon {
    pub fn source(self) -> ImageSource<'static> {
        match self {
            Icon::Home => egui::include_image!("../assets/icons/home.svg"),
            Icon::Fan => egui::include_image!("../assets/icons/weather_squalls.svg"),
            Icon::Power => egui::include_image!("../assets/icons/flash.svg"),
            Icon::Battery => egui::include_image!("../assets/icons/battery_6.svg"),
            Icon::Tools => egui::include_image!("../assets/icons/toolbox.svg"),
            Icon::Settings => egui::include_image!("../assets/icons/settings.svg"),
            Icon::ChevronRight => egui::include_image!("../assets/icons/chevron_right.svg"),
            Icon::Theme => egui::include_image!("../assets/icons/dark_theme.svg"),
            Icon::Sync => egui::include_image!("../assets/icons/arrow_sync.svg"),
            Icon::Download => egui::include_image!("../assets/icons/arrow_download.svg"),
            Icon::Board => egui::include_image!("../assets/icons/developer_board.svg"),
            Icon::Logs => egui::include_image!("../assets/icons/document_text.svg"),
            Icon::Info => egui::include_image!("../assets/icons/info.svg"),
            Icon::Keyboard => egui::include_image!("../assets/icons/keyboard.svg"),
            Icon::Panels => egui::include_image!("../assets/icons/grid_dots.svg"),
            Icon::Curve => egui::include_image!("../assets/icons/data_line.svg"),
            Icon::Gauge => egui::include_image!("../assets/icons/gauge.svg"),
            Icon::Plug => egui::include_image!("../assets/icons/plug_connected.svg"),
            Icon::BatteryCharge => egui::include_image!("../assets/icons/battery_charge.svg"),
            Icon::Speed => egui::include_image!("../assets/icons/top_speed.svg"),
            Icon::Timer => egui::include_image!("../assets/icons/timer.svg"),
            Icon::Temperature => egui::include_image!("../assets/icons/temperature.svg"),
            Icon::Target => egui::include_image!("../assets/icons/target_arrow.svg"),
            Icon::Options => egui::include_image!("../assets/icons/options.svg"),
            Icon::Edit => egui::include_image!("../assets/icons/edit.svg"),
            Icon::Heart => egui::include_image!("../assets/icons/heart.svg"),
            Icon::Pulse => egui::include_image!("../assets/icons/pulse.svg"),
            Icon::Warning => egui::include_image!("../assets/icons/warning.svg"),
            Icon::Delete => egui::include_image!("../assets/icons/delete.svg"),
            Icon::Sort => egui::include_image!("../assets/icons/arrow_sort.svg"),
            Icon::Open => egui::include_image!("../assets/icons/open.svg"),
            Icon::Check => egui::include_image!("../assets/icons/checkmark_circle.svg"),
            Icon::Sliders => egui::include_image!("../assets/icons/slide_settings.svg"),
        }
    }

    pub fn image(self, size: f32, tint: Color32) -> Image<'static> {
        Image::new(self.source()).fit_to_exact_size(Vec2::splat(size)).tint(tint)
    }
}

// ----------------------------------------------------------------- text ---

/// The big title at the top of a page (28 px semibold).
pub fn page_title(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).font(theme::semibold(28.0)).color(colors(ui.ctx()).text));
    ui.add_space(10.0);
}

/// A group heading between cards ("Appearance", "Updates", …).
pub fn section(ui: &mut Ui, text: &str) {
    ui.add_space(14.0);
    ui.label(RichText::new(text).font(theme::semibold(14.0)).color(colors(ui.ctx()).text));
    ui.add_space(4.0);
}

pub fn hint(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).size(12.0).color(colors(ui.ctx()).text2)).wrap());
}

pub fn status_line(ui: &mut Ui, msg: &Option<(String, bool)>) {
    if let Some((m, err)) = msg {
        let c = colors(ui.ctx());
        ui.label(RichText::new(m).size(12.0).color(if *err { c.error } else { c.text2 }));
    }
}

// ---------------------------------------------------------------- cards ---

fn card_frame(ui: &Ui, fill: Color32) -> Frame {
    let c = colors(ui.ctx());
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, c.card_border))
        .corner_radius(CornerRadius::same(6))
}

/// One setting: icon, title and help on the left, its controls on the right.
/// `link` adds a › and makes the whole card clickable (returns the click).
pub fn card(
    ui: &mut Ui,
    icon: Option<Icon>,
    title: &str,
    help: &str,
    link: bool,
    controls: impl FnOnce(&mut Ui),
) -> egui::Response {
    let c = colors(ui.ctx());
    let mut prepared = card_frame(ui, c.card).inner_margin(Margin::symmetric(16, 10)).begin(ui);
    {
        let ui = &mut prepared.content_ui;
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.set_min_height(46.0);
            ui.spacing_mut().item_spacing.x = 16.0;
            if let Some(i) = icon {
                ui.add(i.image(20.0, c.text));
            }
            // The controls sit at the right, laid out left to right in natural order. Their width is
            // measured each frame and remembered, so the title and help can take the rest.
            let slot = ui.id().with(("card_controls", title, help.len()));
            let ctrl_w: f32 = ui.ctx().data(|d| d.get_temp(slot)).unwrap_or(0.0);
            let chevron_w = if link { 32.0 } else { 0.0 };
            let gap = if ctrl_w > 0.0 { 16.0 } else { 0.0 };
            let text_w = (ui.available_width() - ctrl_w - chevron_w - gap).max(120.0);
            ui.vertical(|ui| {
                ui.set_width(text_w);
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.add(egui::Label::new(RichText::new(title).size(14.0).color(c.text)).wrap());
                if !help.is_empty() {
                    ui.add(egui::Label::new(RichText::new(help).size(12.0).color(c.text2)).wrap());
                }
            });
            let used = ui
                .scope(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    controls(ui)
                })
                .response
                .rect
                .width();
            let used = if used < 1.0 { 0.0 } else { used };
            if (used - ctrl_w).abs() > 0.5 {
                ui.ctx().data_mut(|d| d.insert_temp(slot, used));
                ui.ctx().request_repaint();
            }
            if link {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add(Icon::ChevronRight.image(16.0, c.text2));
                });
            }
        });
    }
    let sense = if link { Sense::click() } else { Sense::hover() };
    let response = prepared.allocate_space(ui).interact(sense);
    if link && response.hovered() {
        prepared.frame.fill = c.card_hover;
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    prepared.paint(ui);
    ui.add_space(4.0);
    response
}

/// A card holding anything: optional title and help, then the content.
pub fn group<R>(ui: &mut Ui, title: &str, help: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let c = colors(ui.ctx());
    let r = card_frame(ui, c.card)
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !title.is_empty() {
                ui.label(RichText::new(title).font(theme::semibold(14.0)).color(c.text));
            }
            if !help.is_empty() {
                hint(ui, help);
            }
            if !title.is_empty() || !help.is_empty() {
                ui.add_space(6.0);
            }
            add(ui)
        })
        .inner;
    ui.add_space(4.0);
    r
}

/// A row of equally sized stat cards that always fits the window width.
pub fn stat_row(ui: &mut Ui, tiles: &[(&str, String, String)]) {
    let c = colors(ui.ctx());
    ui.columns(tiles.len(), |cols| {
        for (col, (title, value, sub)) in cols.iter_mut().zip(tiles) {
            card_frame(col, c.card).inner_margin(Margin::symmetric(16, 12)).show(col, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(*title).size(12.0).color(c.text2));
                ui.label(RichText::new(value).font(theme::semibold(22.0)).color(c.text));
                ui.label(RichText::new(if sub.is_empty() { " " } else { sub }).size(12.0).color(c.text2));
            });
        }
    });
    ui.add_space(4.0);
}

// ------------------------------------------------------------- controls ---

/// Windows toggle switch with its "On"/"Off" label to the left (as in White Rabbit).
/// Returns true when toggled.
pub fn switch(ui: &mut Ui, on: &mut bool) -> bool {
    switch_enabled(ui, on, true)
}

pub fn switch_enabled(ui: &mut Ui, on: &mut bool, enabled: bool) -> bool {
    let c = colors(ui.ctx());
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        ui.label(RichText::new(if *on { "On" } else { "Off" }).size(14.0).color(if enabled { c.text } else { c.text3 }));
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(40.0, 20.0), if enabled { Sense::click() } else { Sense::hover() });
        if resp.clicked() {
            *on = !*on;
            changed = true;
        }
        let t = ui.ctx().animate_bool(resp.id, *on);
        let p = ui.painter();
        let lit = *on && enabled;
        if lit {
            p.rect_filled(rect, CornerRadius::same(10), c.accent);
        } else {
            let fill = if resp.hovered() && enabled { c.hover } else { Color32::TRANSPARENT };
            p.rect(rect, CornerRadius::same(10), fill, Stroke::new(1.0_f32, c.text2), egui::StrokeKind::Inside);
        }
        let x = egui::lerp(rect.left() + 10.0..=rect.right() - 10.0, t);
        let r = if resp.hovered() && enabled { 7.0 } else { 6.0 };
        p.circle_filled(egui::pos2(x, rect.center().y), r, if lit { c.on_accent } else { c.text2 });
        if enabled {
            resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        }
    });
    changed
}

/// The main button of a page, in the accent colour.
pub fn accent_button(ui: &mut Ui, label: &str) -> egui::Response {
    let c = colors(ui.ctx());
    ui.add(
        egui::Button::new(RichText::new(label).color(c.on_accent))
            .fill(c.accent)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(4))
            .min_size(Vec2::new(0.0, 32.0)),
    )
}

/// An ordinary Windows button.
pub fn button(ui: &mut Ui, label: &str) -> egui::Response {
    ui.add(egui::Button::new(label).min_size(Vec2::new(0.0, 32.0)))
}

/// A segmented choice (like Windows' selector bar): the chosen option in the accent colour.
pub fn segmented(ui: &mut Ui, selected: bool, label: &str) -> bool {
    let c = colors(ui.ctx());
    let (fill, text, stroke) = if selected {
        (c.accent, RichText::new(label).color(c.on_accent), Stroke::NONE)
    } else {
        (c.control, RichText::new(label).color(c.text), Stroke::new(1.0_f32, c.control_border))
    };
    ui.add(egui::Button::new(text).fill(fill).stroke(stroke).corner_radius(CornerRadius::same(4)).min_size(Vec2::new(64.0, 32.0)))
        .clicked()
}

pub fn warning_banner(ui: &mut Ui, text: &str) {
    card(ui, Some(Icon::Warning), text, "", false, |_| {});
}

/// A setting that can be switched on/off, with a slider for its value (power limits etc.).
pub fn setting_card_u32(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    s: &mut SettingU32,
    range: std::ops::RangeInclusive<u32>,
    suffix: &str,
    current: Option<u32>,
) -> bool {
    let help = current.map(|c| format!("Now {c}{suffix}")).unwrap_or_default();
    let mut dirty = false;
    card(ui, Some(icon), label, &help, false, |ui| {
        ui.add_enabled_ui(s.enabled, |ui| {
            dirty |= ui.add(egui::Slider::new(&mut s.value, range).suffix(suffix)).changed();
        });
        ui.add_space(12.0);
        dirty |= switch(ui, &mut s.enabled);
    });
    dirty
}

pub fn setting_card_choice(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    s: &mut SettingString,
    opts: &[String],
    current: Option<String>,
) -> bool {
    let help = current.map(|c| format!("Now {c}")).unwrap_or_default();
    let mut dirty = false;
    card(ui, Some(icon), label, &help, false, |ui| {
        ui.add_enabled_ui(s.enabled, |ui| {
            egui::ComboBox::from_id_salt(label).selected_text(s.value.clone()).show_ui(ui, |ui| {
                for o in opts {
                    if ui.selectable_label(&s.value == o, o).clicked() {
                        s.value = o.clone();
                        dirty = true;
                    }
                }
            });
        });
        ui.add_space(12.0);
        dirty |= switch(ui, &mut s.enabled);
    });
    dirty
}
