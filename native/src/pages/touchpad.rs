//! Touchpad: ignore touchpad clicks while typing (see touchpad.rs). Works without the service.

use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, Ui};

use crate::app::App;
use crate::launcher::LocalSettings;
use crate::task::{finished, Task};
use crate::theme;
use crate::touchpad::{self, TouchpadSettings, DELAY_RANGE_MS};
use crate::widgets::{button, card, group, hint, section, status_line, switch_enabled, Icon};

const CHECK_EVERY: Duration = Duration::from_secs(1);
/// Slider and switch changes are applied this long after the last one.
const APPLY_AFTER: Duration = Duration::from_millis(600);

#[derive(Default)]
pub struct TouchpadUi {
    /// Sample data mode: pretend the helper works, without touching this PC.
    demo: bool,
    settings: Option<TouchpadSettings>,
    changed: Option<Instant>,
    job: Option<Task<()>>,
    msg: Option<(String, bool)>,
    running: bool,
    last_check: Option<Instant>,
    test_text: String,
    test_clicks: u32,
}

impl TouchpadUi {
    pub fn new(demo: bool) -> Self {
        Self { demo, ..Default::default() }
    }
}

/// When the app opens: start the helper if it's turned on but not running, and refresh its copy
/// after an update. Runs on its own thread.
pub fn on_app_start() {
    if !touchpad::supported() {
        return;
    }
    std::thread::spawn(|| {
        if let Some(s) = LocalSettings::load().touchpad.filter(|s| s.enabled) {
            let _ = touchpad::apply(&s, false);
        }
    });
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let t = &mut app.touchpad;
    let ctx = ui.ctx().clone();
    let c = theme::colors(&ctx);
    let supported = touchpad::supported() || t.demo;
    let mut s = t.settings.get_or_insert_with(|| LocalSettings::load().touchpad.unwrap_or_default()).clone();

    if let Some(r) = finished(&mut t.job) {
        t.msg = r.err().map(|e| (e, true));
        t.last_check = None;
    }
    if t.last_check.is_none_or(|c| c.elapsed() >= CHECK_EVERY) {
        t.running = if t.demo { s.enabled } else { touchpad::is_running() };
        t.last_check = Some(Instant::now());
    }
    ctx.request_repaint_after(CHECK_EVERY);
    let busy = t.job.is_some() || t.changed.is_some();

    let status = if !supported {
        "Only available on Windows".to_string()
    } else if busy {
        "Applying…".to_string()
    } else if s.enabled && t.running {
        "On, running in the background".to_string()
    } else if s.enabled {
        "On, but the helper isn't running. Turn it off and on again to restart it.".to_string()
    } else {
        "Off".to_string()
    };

    let mut changed = false;
    let mut toggled = false;
    card(
        ui,
        Some(Icon::Tap),
        "Ignore touchpad clicks while typing",
        "Stops a palm resting on the touchpad from clicking and moving the text cursor while you type. \
         Starts with Windows and keeps working when this window is closed.",
        false,
        |ui| {
            toggled |= switch_enabled(ui, &mut s.enabled, supported);
        },
    );
    ui.horizontal(|ui| {
        ui.add_space(52.0);
        let color = if s.enabled && !t.running && !busy && supported { c.error } else { c.text3 };
        ui.label(RichText::new(status).size(12.0).color(color));
    });
    ui.add_space(6.0);

    ui.add_enabled_ui(s.enabled, |ui| {
        card(
            ui,
            Some(Icon::Timer),
            "Delay after typing",
            "How long clicks are ignored after each key press. 0.5 to 1 second suits most people.",
            false,
            |ui| {
                let r = ui.add(
                    egui::Slider::new(&mut s.delay_ms, DELAY_RANGE_MS)
                        .step_by(50.0)
                        .custom_formatter(|v, _| format!("{:.2} s", v / 1000.0))
                        .custom_parser(|text| {
                            let v: f64 = text.trim().trim_end_matches('s').trim().parse().ok()?;
                            Some(if v < 10.0 { v * 1000.0 } else { v })
                        }),
                );
                changed |= r.changed();
            },
        );
        card(
            ui,
            Some(Icon::CursorOff),
            "Also ignore scrolling",
            "Ignore two-finger scrolling and the mouse wheel for the same moment too.",
            false,
            |ui| {
                changed |= switch_enabled(ui, &mut s.block_scroll, supported && s.enabled);
            },
        );
    });
    hint(
        ui,
        "Moving the pointer always works, and so do clicks with Ctrl, Shift, Alt or the Windows key held down. \
         This applies to every mouse and touchpad, so a click on an external mouse right after typing waits too.",
    );
    status_line(ui, &t.msg);

    if toggled {
        // Switching on or off takes effect straight away.
        t.changed = Some(Instant::now() - APPLY_AFTER);
    } else if changed {
        t.changed = Some(Instant::now());
    }
    t.settings = Some(s.clone());

    // Apply once the user has let go of the slider and stopped changing things.
    if let Some(at) = t.changed {
        let waiting = at.elapsed() < APPLY_AFTER || ctx.input(|i| i.pointer.any_down()) || t.job.is_some();
        if waiting {
            ctx.request_repaint_after(Duration::from_millis(100));
        } else {
            t.changed = None;
            let mut local = LocalSettings::load();
            local.touchpad = Some(s.clone());
            if !t.demo {
                t.msg = local.save().err().map(|e| (format!("Couldn't save the setting: {e}"), true));
            }
            if supported && !t.demo {
                t.job = Some(Task::spawn(&ctx, move || touchpad::apply(&s, true)));
            }
        }
    }

    section(ui, "Try it");
    group(ui, "", "Type a few letters, then tap the touchpad straight away: the tap is ignored. Wait a moment and it works.", |ui| {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut t.test_text).hint_text("Type here").desired_width(260.0));
            if button(ui, "Tap or click me").clicked() {
                t.test_clicks += 1;
            }
            let text = match t.test_clicks {
                0 => "No clicks yet".to_string(),
                1 => "1 click received".to_string(),
                n => format!("{n} clicks received"),
            };
            ui.label(RichText::new(text).size(12.0).color(c.text2));
        });
    });

    section(ui, "About");
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.label(RichText::new("The same idea as ").size(12.0).color(c.text2));
        ui.hyperlink_to(RichText::new("TouchFreeze").size(12.0), "https://github.com/macleod-ee/touchfreeze");
        ui.label(
            RichText::new(" by Ivan Zhakov, built into Framework Control, so there's nothing else to install.")
                .size(12.0)
                .color(c.text2),
        );
    });
}
