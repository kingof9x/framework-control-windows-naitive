//! The Windows 11 Settings look shared with White Rabbit
//! (https://github.com/kingof9x/white-rabbit-framework-matrix, `rust/wr-window/ui/theme.slint`
//! and `src/theme.rs`): light or dark following Windows, in the Windows accent colour.
//!
//! The choice is the service's shared `ui.theme` setting: "system", "light" or "dark". Any other value
//! (e.g. an old web UI theme name) is treated as "system".
//!
//! Away from Windows (screenshots, tests): FC_APPEARANCE=light|dark and FC_ACCENT=#light,#dark.

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, Visuals};

/// (config value, label) for the theme picker, as in White Rabbit.
pub const THEMES: [(&str, &str); 3] = [("system", "Use my Windows setting"), ("light", "Light"), ("dark", "Dark")];

/// Windows' default blue accent: (light mode, dark mode).
pub const DEFAULT_ACCENT: (u32, u32) = (0x005fb8, 0x60cdff);

/// The palette (White Rabbit's `Theme` global).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colors {
    pub dark: bool,
    pub accent: Color32,
    pub on_accent: Color32,
    pub window_bg: Color32,
    pub card: Color32,
    pub card_hover: Color32,
    pub card_border: Color32,
    pub control: Color32,
    pub control_hover: Color32,
    pub control_border: Color32,
    pub text: Color32,
    pub text2: Color32,
    pub text3: Color32,
    pub hover: Color32,
    pub selected: Color32,
    pub ok: Color32,
    pub warn: Color32,
    pub error: Color32,
}

pub const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

impl Colors {
    pub fn new(dark: bool, accent: u32) -> Self {
        let pick = |d: u32, l: u32| hex(if dark { d } else { l });
        Colors {
            dark,
            accent: hex(accent),
            on_accent: hex(on_accent(dark, accent)),
            window_bg: pick(0x202020, 0xf3f3f3),
            card: pick(0x2b2b2b, 0xfbfbfb),
            card_hover: pick(0x323232, 0xf5f5f5),
            card_border: pick(0x1d1d1d, 0xe5e5e5),
            control: pick(0x373737, 0xffffff),
            control_hover: pick(0x3c3c3c, 0xf9f9f9),
            control_border: pick(0x454545, 0xd0d0d0),
            text: pick(0xffffff, 0x1b1b1b),
            text2: pick(0xc5c5c5, 0x5f5f5f),
            text3: pick(0x9a9a9a, 0x8a8a8a),
            hover: if dark { Color32::from_white_alpha(0x0f) } else { Color32::from_black_alpha(0x0a) },
            selected: if dark { Color32::from_white_alpha(0x15) } else { Color32::from_black_alpha(0x0f) },
            ok: pick(0x6ccb5f, 0x0f7b0f),
            warn: pick(0xfce100, 0x9d5d00),
            error: pick(0xff99a4, 0xc42b1c),
        }
    }

    /// Graph series colours: the accent first, then distinct hues that read on both backgrounds.
    pub fn series(&self, i: usize) -> Color32 {
        const DARK: [u32; 8] = [0x60cdff, 0xffb86c, 0x6ccb5f, 0xf472b6, 0xfce100, 0xb4a0ff, 0xff99a4, 0x4fd1c5];
        const LIGHT: [u32; 8] = [0x005fb8, 0xc25e00, 0x0f7b0f, 0xbf0077, 0x9d5d00, 0x6b3fd8, 0xc42b1c, 0x00777a];
        if i == 0 {
            return self.accent;
        }
        hex(if self.dark { DARK[i % 8] } else { LIGHT[i % 8] })
    }
}

/// Text on the accent colour: dark on a light accent, white on a dark one.
pub fn on_accent(dark: bool, accent: u32) -> u32 {
    if dark {
        return 0x000000;
    }
    let (r, g, b) = ((accent >> 16) & 0xff, (accent >> 8) & 0xff, accent & 0xff);
    let luma = 0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64;
    if luma > 160.0 {
        0x000000
    } else {
        0xffffff
    }
}

// ------------------------------------------------------------- Windows ---

/// Windows' look settings: dark mode and the accent shades (light mode, dark mode).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowsLook {
    pub dark: bool,
    pub accent: (u32, u32),
}

/// Windows' AccentPalette (8 RGBA colours, lightest first) -> (light mode accent, dark mode accent).
/// Windows apps use the "Dark 1" shade on light backgrounds and "Light 2" on dark ones.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn accent_from_palette(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 32 {
        return None;
    }
    let color = |i: usize| (data[i * 4] as u32) << 16 | (data[i * 4 + 1] as u32) << 8 | data[i * 4 + 2] as u32;
    Some((color(4), color(1)))
}

#[cfg(windows)]
mod win {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_BINARY, RRF_RT_REG_DWORD};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn dword(key: &str, name: &str) -> Option<u32> {
        let (k, n) = (wide(key), wide(name));
        let mut value = 0u32;
        let mut size = 4u32;
        // SAFETY: the strings are null-terminated and value/size describe a 4-byte buffer.
        let r = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                k.as_ptr(),
                n.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&mut value as *mut u32).cast(),
                &mut size,
            )
        };
        (r == ERROR_SUCCESS).then_some(value)
    }

    pub fn binary(key: &str, name: &str) -> Option<Vec<u8>> {
        let (k, n) = (wide(key), wide(name));
        let mut buf = vec![0u8; 64];
        let mut size = buf.len() as u32;
        // SAFETY: the strings are null-terminated and buf/size describe the buffer.
        let r = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                k.as_ptr(),
                n.as_ptr(),
                RRF_RT_REG_BINARY,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut size,
            )
        };
        (r == ERROR_SUCCESS).then(|| {
            buf.truncate(size as usize);
            buf
        })
    }
}

fn parse_hex(s: &str) -> Option<u32> {
    u32::from_str_radix(s.trim().trim_start_matches('#'), 16).ok()
}

pub fn read_windows(ctx: &egui::Context) -> WindowsLook {
    let mut look = WindowsLook {
        dark: ctx.system_theme().map(|t| t == egui::Theme::Dark).unwrap_or(true),
        accent: DEFAULT_ACCENT,
    };
    #[cfg(windows)]
    {
        if let Some(v) = win::dword(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "AppsUseLightTheme") {
            look.dark = v == 0;
        }
        if let Some(a) = win::binary(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent", "AccentPalette")
            .and_then(|d| accent_from_palette(&d))
        {
            look.accent = a;
        }
    }
    if let Ok(v) = std::env::var("FC_APPEARANCE") {
        if v == "light" || v == "dark" {
            look.dark = v == "dark";
        }
    }
    if let Some((a, b)) = std::env::var("FC_ACCENT").ok().as_deref().and_then(|v| v.split_once(',')) {
        if let (Some(a), Some(b)) = (parse_hex(a), parse_hex(b)) {
            look.accent = (a, b);
        }
    }
    look
}

/// The palette for a theme choice and Windows' current look.
pub fn resolve(choice: &str, look: WindowsLook) -> Colors {
    let dark = match choice {
        "light" => false,
        "dark" => true,
        _ => look.dark,
    };
    Colors::new(dark, if dark { look.accent.1 } else { look.accent.0 })
}

// --------------------------------------------------------------- apply ---

const COLORS_ID: &str = "fc_theme_colors";

/// The current palette (set by [`apply`] every frame).
pub fn colors(ctx: &egui::Context) -> Colors {
    ctx.data(|d| d.get_temp(egui::Id::new(COLORS_ID))).unwrap_or_else(|| Colors::new(true, DEFAULT_ACCENT.1))
}

/// Apply the palette when it changes (Windows' setting is re-read every couple of seconds).
pub fn apply(ctx: &egui::Context, choice: &str, last: &mut Option<(Colors, std::time::Instant)>, look: &mut Option<WindowsLook>) {
    let stale = last.as_ref().is_none_or(|(_, t)| t.elapsed().as_secs() >= 2);
    if stale || look.is_none() {
        *look = Some(read_windows(ctx));
    }
    let colors = resolve(choice, look.unwrap());
    if last.as_ref().map(|(c, _)| *c) != Some(colors) {
        ctx.set_theme(if colors.dark { egui::Theme::Dark } else { egui::Theme::Light });
        ctx.set_visuals(visuals(&colors));
        ctx.data_mut(|d| d.insert_temp(egui::Id::new(COLORS_ID), colors));
        *last = Some((colors, std::time::Instant::now()));
    } else if stale {
        *last = Some((colors, std::time::Instant::now()));
    }
}

fn visuals(c: &Colors) -> Visuals {
    let mut v = if c.dark { Visuals::dark() } else { Visuals::light() };
    let small = CornerRadius::same(4);
    v.override_text_color = None;
    v.panel_fill = c.window_bg;
    v.window_fill = c.card;
    v.window_stroke = Stroke::new(1.0_f32, c.card_border);
    v.window_corner_radius = CornerRadius::same(8);
    v.extreme_bg_color = c.control;
    v.faint_bg_color = c.card;
    v.code_bg_color = c.control;
    v.hyperlink_color = c.accent;
    v.selection.bg_fill = c.accent;
    v.selection.stroke = Stroke::new(1.0_f32, c.on_accent);
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;
    v.warn_fg_color = c.warn;
    v.error_fg_color = c.error;
    v.popup_shadow.color = Color32::from_black_alpha(if c.dark { 90 } else { 40 });

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = c.card;
    w.noninteractive.weak_bg_fill = c.card;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, c.card_border);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, c.text);
    for (st, fill, border) in [
        (&mut w.inactive, c.control, c.control_border),
        (&mut w.hovered, c.control_hover, c.control_border),
        (&mut w.active, c.control_hover, c.text3),
        (&mut w.open, c.control, c.control_border),
    ] {
        // Buttons and combo boxes fill with weak_bg_fill; egui uses bg_fill for slider rails and
        // check boxes, which need to stand out from the card like White Rabbit's grey track.
        st.bg_fill = c.control_border;
        st.weak_bg_fill = fill;
        st.bg_stroke = Stroke::new(1.0_f32, border);
        st.fg_stroke = Stroke::new(1.0_f32, c.text);
        st.corner_radius = small;
        st.expansion = 0.0;
    }
    w.noninteractive.corner_radius = small;
    v
}

// --------------------------------------------------------------- fonts ---

pub const SEMIBOLD: &str = "semibold";

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

/// Segoe UI (and Semibold for headings) like White Rabbit; Inter when testing away from Windows.
pub fn install(ctx: &egui::Context) {
    egui_extras::install_image_loaders(ctx);
    let regular = ["C:\\Windows\\Fonts\\segoeui.ttf", "/usr/share/fonts/opentype/inter/Inter-Regular.otf"];
    let bold = ["C:\\Windows\\Fonts\\seguisb.ttf", "/usr/share/fonts/opentype/inter/Inter-SemiBold.otf"];
    let mut fonts = FontDefinitions::default();
    if let Some(bytes) = regular.iter().find_map(|p| std::fs::read(p).ok()) {
        fonts.font_data.insert("system".into(), std::sync::Arc::new(FontData::from_owned(bytes)));
        fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "system".into());
    }
    let mut semi = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    if let Some(bytes) = bold.iter().find_map(|p| std::fs::read(p).ok()) {
        fonts.font_data.insert("system-semibold".into(), std::sync::Arc::new(FontData::from_owned(bytes)));
        semi.insert(0, "system-semibold".into());
    }
    fonts.families.insert(FontFamily::Name(SEMIBOLD.into()), semi);
    ctx.set_fonts(fonts);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 4.0);
        s.spacing.button_padding = egui::vec2(12.0, 5.0);
        s.spacing.interact_size.y = 32.0;
        s.spacing.slider_width = 200.0;
        s.spacing.combo_width = 200.0;
        s.spacing.icon_width = 20.0;
        s.spacing.icon_spacing = 8.0;
        s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Button, FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Small, FontId::proportional(12.0));
        s.text_styles.insert(egui::TextStyle::Heading, semibold(20.0));
        s.text_styles.insert(egui::TextStyle::Monospace, FontId::monospace(12.5));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_palette_matches_white_rabbit() {
        // Same fixture as White Rabbit's theme.rs test
        let mut data = vec![0u8; 32];
        data[4..8].copy_from_slice(&[0x99, 0xeb, 0xff, 0]);
        data[16..20].copy_from_slice(&[0x00, 0x5a, 0x9e, 0]);
        assert_eq!(accent_from_palette(&data), Some((0x005a9e, 0x99ebff)));
        assert_eq!(accent_from_palette(&[1, 2]), None);
    }

    #[test]
    fn theme_choice_resolves() {
        let look = WindowsLook { dark: true, accent: (0x005fb8, 0x60cdff) };
        assert!(resolve("system", look).dark);
        assert!(!resolve("light", look).dark);
        assert_eq!(resolve("light", look).accent, hex(0x005fb8));
        assert_eq!(resolve("synthwave", look).accent, hex(0x60cdff));
    }
}
