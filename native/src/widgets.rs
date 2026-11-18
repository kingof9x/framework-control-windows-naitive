use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Ui, Vec2};

use crate::api::{SettingString, SettingU32};
use crate::theme;

pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(ui.visuals().panel_fill)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(ui.visuals().window_corner_radius)
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

pub fn section_title(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(15.0).strong());
}

pub fn hint(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).small().weak());
}

/// A row of equally sized stat tiles that always fits the window width.
pub fn stat_row(ui: &mut Ui, tiles: &[(&str, String, String)]) {
    ui.columns(tiles.len(), |cols| {
        for (col, (title, value, sub)) in cols.iter_mut().zip(tiles) {
            stat_tile(col, title, value, sub);
        }
    });
}

fn stat_tile(ui: &mut Ui, title: &str, value: &str, sub: &str) {
    egui::Frame::new()
        .fill(ui.visuals().panel_fill)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(ui.visuals().window_corner_radius)
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).small().weak());
            ui.label(RichText::new(value).size(22.0).strong());
            ui.label(RichText::new(if sub.is_empty() { " " } else { sub }).small().weak());
        });
}

/// A segmented-control style button. Returns true when clicked.
pub fn segmented(ui: &mut Ui, selected: bool, label: &str) -> bool {
    let (fill, text) = if selected {
        let on = ui.visuals().selection.stroke.color;
        (on, RichText::new(label).color(on_accent(ui)))
    } else {
        (ui.visuals().widgets.inactive.weak_bg_fill, RichText::new(label))
    };
    ui.add(
        egui::Button::new(text)
            .fill(fill)
            .corner_radius(CornerRadius::same(6))
            .min_size(Vec2::new(84.0, 28.0)),
    )
    .clicked()
}

pub fn on_accent(ui: &Ui) -> Color32 {
    theme::on_accent(ui.ctx())
}

pub fn primary_button(ui: &mut Ui, label: &str) -> egui::Response {
    let fill = theme::accent(ui);
    let text = RichText::new(label).color(on_accent(ui));
    ui.add(egui::Button::new(text).fill(fill).min_size(Vec2::new(80.0, 28.0)))
}

pub fn status_line(ui: &mut Ui, msg: &Option<(String, bool)>) {
    if let Some((m, err)) = msg {
        let dark = ui.visuals().dark_mode;
        let c = if *err { theme::error(dark) } else { theme::success(dark) };
        ui.label(RichText::new(m).color(c));
    }
}

pub fn warning_banner(ui: &mut Ui, text: &str) {
    let c = theme::warning(ui.visuals().dark_mode);
    card(ui, |ui| {
        ui.label(RichText::new(format!("⚠ {text}")).color(c));
    });
}

pub fn setting_row_u32(
    ui: &mut Ui,
    label: &str,
    s: &mut SettingU32,
    range: std::ops::RangeInclusive<u32>,
    suffix: &str,
    current: Option<u32>,
) -> bool {
    let mut dirty = ui.checkbox(&mut s.enabled, label).changed();
    ui.add_enabled_ui(s.enabled, |ui| {
        dirty |= ui.add(egui::Slider::new(&mut s.value, range).suffix(suffix)).changed();
    });
    ui.label(RichText::new(current.map(|c| format!("now {c}{suffix}")).unwrap_or_default()).weak());
    ui.end_row();
    dirty
}

pub fn setting_row_choice(
    ui: &mut Ui,
    label: &str,
    s: &mut SettingString,
    opts: &[String],
    current: Option<String>,
) -> bool {
    let mut dirty = ui.checkbox(&mut s.enabled, label).changed();
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
    ui.label(RichText::new(current.map(|c| format!("now {c}")).unwrap_or_default()).weak());
    ui.end_row();
    dirty
}
