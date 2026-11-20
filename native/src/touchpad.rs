//! "Ignore touchpad clicks while typing": clicks (and, optionally, scrolling) are dropped for a short
//! moment after each key press, so a palm brushing the touchpad doesn't move the text cursor.
//! Same idea as TouchFreeze (https://github.com/macleod-ee/touchfreeze), written from scratch here.
//!
//! The work happens in a tiny background process, the *touchpad guard*: this app's own exe started
//! with `--touchpad-guard`. It runs no window or UI, just two low-level input hooks (keyboard and
//! mouse) on a message loop, so it uses a couple of MB of memory. It runs as the signed-in user,
//! because input hooks can't run in the background service (session 0).
//!
//! The guard runs from a per-user copy of the exe (`%LOCALAPPDATA%\FrameworkControl\`), so
//! installing an update never has to close it or ask for a reboot. The app refreshes the copy and
//! restarts the guard when it opens after an update. A `HKCU\...\Run` entry starts it at sign-in.
//! If Framework Control is uninstalled, the guard notices at the next sign-in and removes itself
//! from startup.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_DELAY_MS: u32 = 700;
pub const DELAY_RANGE_MS: std::ops::RangeInclusive<u32> = 200..=2000;

/// The guard's settings, kept in the app's per-user settings file (`touchpad`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TouchpadSettings {
    pub enabled: bool,
    /// How long after the last key press clicks are ignored.
    pub delay_ms: u32,
    /// Also ignore scrolling (two-finger scroll, mouse wheel) in that time.
    pub block_scroll: bool,
}

impl Default for TouchpadSettings {
    fn default() -> Self {
        Self { enabled: false, delay_ms: DEFAULT_DELAY_MS, block_scroll: false }
    }
}

impl TouchpadSettings {
    pub fn delay(&self) -> u32 {
        self.delay_ms.clamp(*DELAY_RANGE_MS.start(), *DELAY_RANGE_MS.end())
    }
}

pub const GUARD_ARG: &str = "--touchpad-guard";
const GUARD_EXE: &str = "FrameworkControl-Touchpad.exe";

/// Whether this build can run the guard at all (Windows only).
pub fn supported() -> bool {
    cfg!(windows)
}

/// Where the per-user copy of the guard lives.
fn guard_path() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(base).join("FrameworkControl").join(GUARD_EXE))
}

/// The guard copy is out of date when its size or timestamp differs from this exe's
/// (`fs::copy` keeps the timestamp on Windows).
fn stale(src: &Path, dst: &Path) -> bool {
    let (Ok(a), Ok(b)) = (std::fs::metadata(src), std::fs::metadata(dst)) else { return true };
    a.len() != b.len() || a.modified().ok() != b.modified().ok()
}

/// Make the running state match the settings: start, restart (for new settings or after an
/// update) or stop the guard, and add or remove its sign-in entry. Blocks for up to a few seconds,
/// so call it off the UI thread.
pub fn apply(settings: &TouchpadSettings, restart: bool) -> Result<(), String> {
    if !supported() {
        return Err("Only available on Windows.".into());
    }
    if !settings.enabled {
        stop_and_wait();
        sys::set_autostart(None)?;
        return Ok(());
    }
    let app = std::env::current_exe().map_err(|e| e.to_string())?;
    let guard = guard_path().ok_or("LOCALAPPDATA isn't set")?;
    let needs_copy = stale(&app, &guard);
    if (needs_copy || restart) && is_running() {
        stop_and_wait();
    }
    if needs_copy {
        if let Some(dir) = guard.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::copy(&app, &guard).map_err(|e| format!("Couldn't copy the touchpad helper: {e}"))?;
    }
    let app_s = app.display().to_string();
    let args = [GUARD_ARG, "--app", app_s.as_str()];
    sys::set_autostart(Some(&format!("\"{}\" {GUARD_ARG} --app \"{app_s}\"", guard.display())))?;
    if !is_running() {
        crate::launcher::launch(&guard, &args)?;
        // Give it a moment to install its hooks, so the page shows the right state straight away.
        for _ in 0..20 {
            if is_running() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if !is_running() {
            return Err("The touchpad helper didn't start.".into());
        }
    }
    Ok(())
}

fn stop_and_wait() {
    if !is_running() {
        return;
    }
    sys::signal_stop();
    for _ in 0..30 {
        if !is_running() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Whether a guard process is running for this user.
pub fn is_running() -> bool {
    sys::is_running()
}

/// Entry point of the guard process (`--touchpad-guard [--app <path>]`). Returns the exit code.
pub fn run_guard(app: Option<PathBuf>) -> i32 {
    // Framework Control was uninstalled (or moved): take ourselves out of startup and stop.
    if app.as_ref().is_some_and(|a| !a.exists()) {
        let _ = sys::set_autostart(None);
        return 0;
    }
    let settings = crate::launcher::LocalSettings::load().touchpad.unwrap_or_default();
    if !settings.enabled {
        let _ = sys::set_autostart(None);
        return 0;
    }
    sys::run(settings.delay(), settings.block_scroll)
}

// ------------------------------------------------------------------ the decision ---

/// Which input to drop. Kept free of Windows types so it can be tested anywhere.
#[cfg_attr(not(windows), allow(dead_code))]
pub mod rules {
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering::Relaxed};

    pub struct Guard {
        delay_ms: AtomicU32,
        block_scroll: AtomicBool,
        typed: AtomicBool,
        last_key_ms: AtomicU32,
        /// Modifier keys held right now (bit per key), so Ctrl/Shift/Alt/Win + click still work.
        modifiers: AtomicU8,
        /// Buttons whose press was dropped, so their release is dropped too.
        swallowed: AtomicU8,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Mouse {
        Down(u8),
        Up(u8),
        Scroll,
        Other,
    }

    impl Guard {
        pub const fn new() -> Self {
            Self {
                delay_ms: AtomicU32::new(super::DEFAULT_DELAY_MS),
                block_scroll: AtomicBool::new(false),
                typed: AtomicBool::new(false),
                last_key_ms: AtomicU32::new(0),
                modifiers: AtomicU8::new(0),
                swallowed: AtomicU8::new(0),
            }
        }

        pub fn configure(&self, delay_ms: u32, block_scroll: bool) {
            self.delay_ms.store(delay_ms, Relaxed);
            self.block_scroll.store(block_scroll, Relaxed);
        }

        /// A key went down or up. `modifier` is Some(bit) for Shift/Ctrl/Alt/Win, which don't
        /// count as typing.
        pub fn key(&self, time_ms: u32, modifier: Option<u8>, down: bool) {
            match modifier {
                Some(bit) if down => {
                    self.modifiers.fetch_or(bit, Relaxed);
                }
                Some(bit) => {
                    self.modifiers.fetch_and(!bit, Relaxed);
                }
                None => {
                    self.last_key_ms.store(time_ms, Relaxed);
                    self.typed.store(true, Relaxed);
                }
            }
        }

        fn typing(&self, time_ms: u32) -> bool {
            if !self.typed.load(Relaxed) || self.modifiers.load(Relaxed) != 0 {
                return false;
            }
            // Event times can arrive slightly out of order; either way round counts.
            let d = time_ms.wrapping_sub(self.last_key_ms.load(Relaxed)) as i32;
            d.unsigned_abs() < self.delay_ms.load(Relaxed)
        }

        /// True to drop this mouse event.
        pub fn mouse(&self, time_ms: u32, ev: Mouse) -> bool {
            match ev {
                Mouse::Down(b) => {
                    if self.typing(time_ms) {
                        self.swallowed.fetch_or(b, Relaxed);
                        true
                    } else {
                        false
                    }
                }
                // Drop the release of a press we dropped, whenever it comes.
                Mouse::Up(b) => self.swallowed.fetch_and(!b, Relaxed) & b != 0,
                Mouse::Scroll => self.block_scroll.load(Relaxed) && self.typing(time_ms),
                Mouse::Other => false,
            }
        }
    }

    impl Default for Guard {
        fn default() -> Self {
            Self::new()
        }
    }
}

// ------------------------------------------------------------------ Windows ---

#[cfg(windows)]
mod sys {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, LPARAM, LRESULT, WAIT_OBJECT_0, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Registry::{RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
    use windows_sys::Win32::System::Threading::{CreateEventW, CreateMutexW, OpenEventW, OpenMutexW, ResetEvent, SetEvent, EVENT_MODIFY_STATE, INFINITE, SYNCHRONIZATION_SYNCHRONIZE};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    use super::rules::{Guard, Mouse};

    // Per user session ("Local\").
    const MUTEX: &str = "Local\\FrameworkControlTouchpadGuard";
    const STOP_EVENT: &str = "Local\\FrameworkControlTouchpadGuardStop";
    const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    const RUN_VALUE: &str = "FrameworkControlTouchpad";

    static GUARD: Guard = Guard::new();

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    pub fn is_running() -> bool {
        let name = wide(MUTEX);
        let h = unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, 0, name.as_ptr()) };
        if h.is_null() {
            return false;
        }
        unsafe { CloseHandle(h) };
        true
    }

    pub fn signal_stop() {
        let name = wide(STOP_EVENT);
        let h = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) };
        if !h.is_null() {
            unsafe {
                SetEvent(h);
                CloseHandle(h);
            }
        }
    }

    /// Add (Some(command)) or remove (None) the guard's sign-in entry.
    pub fn set_autostart(command: Option<&str>) -> Result<(), String> {
        let key = wide(RUN_KEY);
        let value = wide(RUN_VALUE);
        let rc = match command {
            Some(cmd) => {
                let data = wide(cmd);
                unsafe {
                    RegSetKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr(), REG_SZ, data.as_ptr().cast(), (data.len() * 2) as u32)
                }
            }
            None => unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr()) },
        };
        match rc {
            0 => Ok(()),
            ERROR_FILE_NOT_FOUND if command.is_none() => Ok(()),
            e => Err(format!("Couldn't update the sign-in entry (error {e})")),
        }
    }

    fn modifier_bit(vk: u32) -> Option<u8> {
        match vk as u16 {
            VK_SHIFT | VK_LSHIFT | VK_RSHIFT => Some(1),
            VK_CONTROL | VK_LCONTROL | VK_RCONTROL => Some(2),
            VK_MENU | VK_LMENU | VK_RMENU => Some(4),
            VK_LWIN | VK_RWIN => Some(8),
            _ => None,
        }
    }

    unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 {
            let k = &*(lparam as *const KBDLLHOOKSTRUCT);
            // Keys sent by software (macros, on-screen keyboards) aren't typing on this keyboard.
            if k.flags & LLKHF_INJECTED == 0 {
                let down = matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
                GUARD.key(k.time, modifier_bit(k.vkCode), down);
            }
        }
        CallNextHookEx(null_mut(), code, wparam, lparam)
    }

    unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 {
            let m = &*(lparam as *const MSLLHOOKSTRUCT);
            let ev = match wparam as u32 {
                WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => Mouse::Down(1),
                WM_RBUTTONDOWN | WM_RBUTTONDBLCLK => Mouse::Down(2),
                WM_MBUTTONDOWN | WM_MBUTTONDBLCLK => Mouse::Down(4),
                WM_LBUTTONUP => Mouse::Up(1),
                WM_RBUTTONUP => Mouse::Up(2),
                WM_MBUTTONUP => Mouse::Up(4),
                WM_MOUSEWHEEL | WM_MOUSEHWHEEL => Mouse::Scroll,
                _ => Mouse::Other,
            };
            if GUARD.mouse(m.time, ev) {
                return 1;
            }
        }
        CallNextHookEx(null_mut(), code, wparam, lparam)
    }

    pub fn run(delay_ms: u32, block_scroll: bool) -> i32 {
        GUARD.configure(delay_ms, block_scroll);
        unsafe {
            // One guard per signed-in user.
            let mutex_name = wide(MUTEX);
            let mutex = CreateMutexW(null(), 0, mutex_name.as_ptr());
            if mutex.is_null() || GetLastError() == ERROR_ALREADY_EXISTS {
                return 0;
            }
            let stop_name = wide(STOP_EVENT);
            let stop = CreateEventW(null(), 1, 0, stop_name.as_ptr());
            if stop.is_null() {
                return 1;
            }
            ResetEvent(stop);

            let module = GetModuleHandleW(null());
            let kb = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0);
            let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0);
            if kb.is_null() || mouse.is_null() {
                return 1;
            }

            // Low-level hooks are called on this thread, so keep its message queue moving until
            // the app asks us to stop.
            let mut msg: MSG = std::mem::zeroed();
            'outer: loop {
                let r = MsgWaitForMultipleObjects(1, &stop, 0, INFINITE, QS_ALLINPUT);
                if r == WAIT_OBJECT_0 {
                    break;
                }
                while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                    if msg.message == WM_QUIT {
                        break 'outer;
                    }
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            UnhookWindowsHookEx(kb);
            UnhookWindowsHookEx(mouse);
            CloseHandle(stop);
            CloseHandle(mutex);
        }
        0
    }
}

#[cfg(not(windows))]
mod sys {
    pub fn is_running() -> bool {
        false
    }
    pub fn signal_stop() {}
    pub fn set_autostart(_command: Option<&str>) -> Result<(), String> {
        Ok(())
    }
    pub fn run(_delay_ms: u32, _block_scroll: bool) -> i32 {
        eprintln!("The touchpad guard only runs on Windows.");
        1
    }
}

#[cfg(test)]
mod tests {
    use super::rules::{Guard, Mouse};

    #[test]
    fn clicks_right_after_typing_are_dropped() {
        let g = Guard::new();
        g.configure(700, false);
        // Nothing typed yet: clicks pass.
        assert!(!g.mouse(1000, Mouse::Down(1)));
        assert!(!g.mouse(1010, Mouse::Up(1)));
        g.key(5000, None, true);
        g.key(5080, None, false);
        // A palm tap 300 ms later is dropped, press and release.
        assert!(g.mouse(5380, Mouse::Down(1)));
        assert!(g.mouse(5390, Mouse::Up(1)));
        // Its release is dropped even if it comes after the delay.
        assert!(g.mouse(5500, Mouse::Down(2)));
        assert!(g.mouse(9000, Mouse::Up(2)));
        // Later clicks work.
        assert!(!g.mouse(5900, Mouse::Down(1)));
        assert!(!g.mouse(5950, Mouse::Up(1)));
    }

    #[test]
    fn modifier_clicks_and_scrolling() {
        let g = Guard::new();
        g.configure(700, false);
        g.key(100, None, true);
        // Ctrl alone isn't typing, and Ctrl+click right after typing still works.
        g.key(150, Some(2), true);
        assert!(!g.mouse(200, Mouse::Down(1)));
        assert!(!g.mouse(210, Mouse::Up(1)));
        g.key(250, Some(2), false);
        assert!(g.mouse(300, Mouse::Down(1)));
        // Scrolling is only dropped when asked.
        assert!(!g.mouse(300, Mouse::Scroll));
        g.configure(700, true);
        assert!(g.mouse(300, Mouse::Scroll));
        assert!(!g.mouse(2000, Mouse::Scroll));
        // The millisecond clock wraps after 49 days.
        g.key(u32::MAX - 100, None, true);
        assert!(g.mouse(200, Mouse::Down(4)));
    }
}
