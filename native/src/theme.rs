use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke, Theme, Visuals};

use crate::app::ACCENT;

pub fn install(ctx: &egui::Context) {
    install_system_font(ctx);
    ctx.options_mut(|o| o.theme_preference = egui::ThemePreference::System);
    ctx.set_visuals_of(Theme::Dark, visuals(true));
    ctx.set_visuals_of(Theme::Light, visuals(false));
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 4.0);
        s.spacing.slider_width = 220.0;
        s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(11.5));
    });
}

fn visuals(dark: bool) -> Visuals {
    let mut v = if dark { Visuals::dark() } else { Visuals::light() };
    let (panel, base, stroke) = if dark {
        (Color32::from_rgb(0x20, 0x22, 0x26), Color32::from_rgb(0x17, 0x18, 0x1b), Color32::from_rgb(0x33, 0x36, 0x3c))
    } else {
        (Color32::from_rgb(0xff, 0xff, 0xff), Color32::from_rgb(0xf3, 0xf4, 0xf6), Color32::from_rgb(0xe1, 0xe3, 0xe8))
    };
    v.panel_fill = panel;
    v.window_fill = panel;
    v.extreme_bg_color = base;
    v.faint_bg_color = base;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, stroke);
    v.selection.bg_fill = ACCENT.gamma_multiply(if dark { 0.35 } else { 0.25 });
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    v.slider_trailing_fill = true;
    let r = CornerRadius::same(6);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = r;
    }
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, stroke);
    v
}

/// Prefer the OS UI font (Segoe UI on Windows) so the app feels native.
fn install_system_font(ctx: &egui::Context) {
    let candidates = [
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ];
    let Some(bytes) = candidates.iter().find_map(|p| std::fs::read(p).ok()) else { return };
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert("system".into(), std::sync::Arc::new(FontData::from_owned(bytes)));
    // System font first; egui's bundled fonts stay as fallback for symbols.
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "system".into());
    ctx.set_fonts(fonts);
}
