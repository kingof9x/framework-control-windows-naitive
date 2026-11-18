//! Native versions of the web UI's DaisyUI themes (web/theme.config.js), so the
//! shared `ui.theme` setting looks the same in both front ends.

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke, Visuals};

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub accent: Color32,
    pub on_accent: Color32,
    /// DaisyUI base-100: page background.
    pub page: Color32,
    /// DaisyUI base-200: raised panels/cards.
    pub panel: Color32,
    pub text: Color32,
    pub border: Color32,
    pub radius: u8,
}

/// (config value, label) in the same order as the web theme picker.
pub const THEMES: [(&str, &str); 8] = [
    ("system", "System"),
    ("light", "Light"),
    ("dark", "Dark"),
    ("extra-dark", "Extra Dark"),
    ("retro", "Retro"),
    ("synthwave", "Synthwave"),
    ("coffee", "Coffee"),
    ("terminal", "Terminal"),
];

const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

#[allow(clippy::too_many_arguments)]
const fn p(dark: bool, accent: u32, on: u32, page: u32, panel: u32, text: u32, border: u32, radius: u8) -> Palette {
    Palette {
        dark,
        accent: hex(accent),
        on_accent: hex(on),
        page: hex(page),
        panel: hex(panel),
        text: hex(text),
        border: hex(border),
        radius,
    }
}

pub fn palette(name: &str) -> Palette {
    match name {
        "light" => p(false, 0xea7148, 0xFFFFFF, 0xebeaea, 0xf5f4f5, 0x2A2422, 0xd1ccc7, 8),
        "extra-dark" => p(true, 0xE89B52, 0x17100A, 0x111318, 0x090B0F, 0xCDD3DA, 0x252b37, 8),
        "retro" => p(false, 0xB36F30, 0xFFFFFF, 0xECE3C8, 0xF4EBD4, 0x1B1A1A, 0xd6c48f, 6),
        "synthwave" => p(true, 0xF8C740, 0x27113F, 0x2E2058, 0x241945, 0xD8C8E6, 0x735CC0, 8),
        "coffee" => p(true, 0xDB924B, 0x21150D, 0x30232C, 0x291F28, 0xE2C48C, 0x141112, 8),
        "terminal" => p(true, 0x4CB66A, 0x08140C, 0x000000, 0x000000, 0xD7DED8, 0x22452d, 0),
        _ => p(true, 0xFF9D57, 0x18202A, 0x2A303C, 0x242933, 0xC4D0D8, 0x353D4A, 8),
    }
}

/// Sensor series colours (web: --sensor-* variables).
pub fn series(dark: bool, i: usize) -> Color32 {
    const DARK: [u32; 12] = [
        0x60A5FA, 0xFB923C, 0x4ADE80, 0xF472B6, 0xFBBF24, 0xA78BFA, 0x22D3EE, 0xFF5C5C, 0x34D399, 0xC084FC,
        0xFACC15, 0x2DD4BF,
    ];
    const LIGHT: [u32; 12] = [
        0x3B82F6, 0xF97316, 0x1F9D4A, 0xDB2777, 0xD4A017, 0x8B5CF6, 0x0891B2, 0xEF4444, 0x10B981, 0xA855F7,
        0xEAB308, 0x14B8A6,
    ];
    hex(if dark { DARK[i % 12] } else { LIGHT[i % 12] })
}

pub fn warning(dark: bool) -> Color32 {
    hex(if dark { 0xEFD057 } else { 0xC06D00 })
}
pub fn error(dark: bool) -> Color32 {
    hex(if dark { 0xFF8F7D } else { 0xCF544B })
}
pub fn success(dark: bool) -> Color32 {
    hex(if dark { 0x62EFBD } else { 0x17B84C })
}

/// The accent colour of the active theme (stored in the selection stroke).
pub fn accent(ui: &egui::Ui) -> Color32 {
    ui.visuals().selection.stroke.color
}

/// Resolve "system" against the OS setting and apply the palette.
/// Returns the concrete theme name that was applied.
pub fn apply(ctx: &egui::Context, preference: &str, last: &mut String) {
    let name = if preference == "system" || !THEMES.iter().any(|(v, _)| *v == preference) {
        match ctx.system_theme() {
            Some(egui::Theme::Light) => "light",
            _ => "dark",
        }
    } else {
        preference
    };
    if last == name {
        return;
    }
    *last = name.to_string();
    let pal = palette(name);
    ctx.set_theme(if pal.dark { egui::Theme::Dark } else { egui::Theme::Light });
    ctx.set_visuals(visuals(&pal));
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(ON_ACCENT), pal.on_accent));
}

const ON_ACCENT: &str = "theme_on_accent";

/// Text colour to use on top of the accent colour (DaisyUI primary-content).
pub fn on_accent(ctx: &egui::Context) -> Color32 {
    ctx.data(|d| d.get_temp(egui::Id::new(ON_ACCENT))).unwrap_or(Color32::WHITE)
}

fn visuals(pal: &Palette) -> Visuals {
    let mut v = if pal.dark { Visuals::dark() } else { Visuals::light() };
    v.widgets.noninteractive.fg_stroke.color = pal.text;
    v.widgets.inactive.fg_stroke.color = pal.text;
    v.panel_fill = pal.panel;
    v.window_fill = pal.panel;
    v.window_stroke = Stroke::new(1.0_f32, pal.border);
    v.extreme_bg_color = pal.page;
    v.faint_bg_color = pal.page;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, pal.border);
    v.selection.bg_fill = pal.accent.gamma_multiply(if pal.dark { 0.35 } else { 0.3 });
    v.selection.stroke = Stroke::new(1.0_f32, pal.accent);
    v.hyperlink_color = pal.accent;
    v.slider_trailing_fill = true;
    let r = CornerRadius::same(pal.radius.min(6));
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = r;
    }
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, pal.border);
    v.window_corner_radius = CornerRadius::same(pal.radius);
    v
}

pub fn install(ctx: &egui::Context) {
    install_system_font(ctx);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 4.0);
        s.spacing.slider_width = 220.0;
        s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(11.5));
        s.text_styles.insert(egui::TextStyle::Monospace, egui::FontId::monospace(12.5));
    });
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
