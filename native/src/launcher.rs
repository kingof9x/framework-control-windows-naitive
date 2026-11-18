//! Companion Framework 16 tools that this app can start
//! (see https://github.com/kingof9x/my-framework-16-tools).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

pub struct Action {
    pub label: &'static str,
    pub args: &'static [&'static str],
    pub hint: &'static str,
    /// This action starts the long-running instance (blocked while it is running).
    pub starts_instance: bool,
}

pub struct ToolDef {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub exe: &'static str,
    pub actions: &'static [Action],
    /// Where the tool's own installer puts it, relative to a known folder.
    install_dirs: &'static [(&'static str, &'static str)],
    pub download_label: &'static str,
    pub download_url: &'static str,
    pub project_url: &'static str,
    /// The tool refuses to start a second copy (named mutex).
    pub single_instance: bool,
}

pub const TOOLS: &[ToolDef] = &[
    ToolDef {
        id: "led_mon",
        name: "LED Matrix Monitor",
        description: "System monitor, clock, weather and more on the LED Matrix panels, with a layout editor.",
        exe: "led_mon.exe",
        actions: &[
            Action { label: "Start monitor", args: &[], hint: "Runs in the system tray", starts_instance: true },
            Action { label: "Edit layout", args: &["--edit-layout"], hint: "Choose which apps show on each panel", starts_instance: false },
        ],
        install_dirs: &[("LOCALAPPDATA", "Programs\\led_mon")],
        download_label: "Download installer",
        download_url: "https://github.com/kingof9x/My-Framework-16-tools/releases/latest/download/led_mon-setup.exe",
        project_url: "https://github.com/kingof9x/led-matrix",
        single_instance: true,
    },
    ToolDef {
        id: "f16-lights",
        name: "Framework 16 Lights",
        description: "RGB keyboard and macropad lighting: per-device effects, linked effects and hotkey lock.",
        exe: "f16-lights.exe",
        actions: &[Action { label: "Open", args: &[], hint: "", starts_instance: false }],
        install_dirs: &[
            ("LOCALAPPDATA", "Programs\\f16-lights"),
            ("USERPROFILE", "Downloads"),
            ("USERPROFILE", "Downloads\\f16-lights-windows"),
            ("USERPROFILE", "Desktop"),
        ],
        download_label: "Get the latest build",
        download_url: "https://github.com/kingof9x/framework-16-keyboard-customization/releases/latest",
        project_url: "https://github.com/kingof9x/framework-16-keyboard-customization",
        single_instance: false,
    },
];

// ---------------------------------------------------------------- settings ---

/// Per-user preferences of the native app (tool locations). Kept out of the
/// service config, which is shared, system-owned and upstream's schema.
#[derive(Default, Serialize, Deserialize)]
pub struct LocalSettings {
    #[serde(default)]
    pub tool_paths: BTreeMap<String, PathBuf>,
}

fn settings_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("FrameworkControl").join("native-app.json"))
}

impl LocalSettings {
    pub fn load() -> Self {
        settings_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let p = settings_path().ok_or("no settings folder")?;
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(p, json).map_err(|e| e.to_string())
    }
}

// --------------------------------------------------------------- discovery ---

pub enum Found {
    /// Picked by the user with Browse…
    Chosen(PathBuf),
    /// Found in a standard location.
    Detected(PathBuf),
    Missing,
}

impl Found {
    pub fn path(&self) -> Option<&Path> {
        match self {
            Found::Chosen(p) | Found::Detected(p) => Some(p),
            Found::Missing => None,
        }
    }
}

pub fn locate(tool: &ToolDef, settings: &LocalSettings) -> Found {
    if let Some(p) = settings.tool_paths.get(tool.id) {
        if p.is_file() {
            return Found::Chosen(p.clone());
        }
    }
    let mut candidates: Vec<PathBuf> = tool
        .install_dirs
        .iter()
        .filter_map(|(var, rel)| std::env::var_os(var).map(|b| PathBuf::from(b).join(rel).join(tool.exe)))
        .collect();
    // Next to this app (e.g. copied into the Framework Control folder).
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        candidates.push(dir.join(tool.exe));
    }
    // Anywhere on PATH.
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|d| d.join(tool.exe)));
    }
    candidates.into_iter().find(|p| p.is_file()).map(Found::Detected).unwrap_or(Found::Missing)
}

// ----------------------------------------------------------------- running ---

/// Which of the given exe names currently have a running process (Windows only).
pub fn running(exes: &[&str]) -> Vec<String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let Ok(out) = Command::new("tasklist").args(["/FO", "CSV", "/NH"]).creation_flags(CREATE_NO_WINDOW).output() else {
            return Vec::new();
        };
        let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
        exes.iter()
            .filter(|e| text.contains(&format!("\"{}\"", e.to_lowercase())))
            .map(|e| e.to_string())
            .collect()
    }
    #[cfg(not(windows))]
    {
        let _ = exes;
        Vec::new()
    }
}

// ------------------------------------------------------------------ launch ---

/// Start the tool detached from this app, so closing Framework Control leaves it running.
pub fn launch(exe: &Path, args: &[&str]) -> Result<(), String> {
    let mut cmd = Command::new(exe);
    cmd.args(args);
    if let Some(dir) = exe.parent() {
        cmd.current_dir(dir);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    cmd.spawn().map(|_| ()).map_err(|e| format!("Couldn't start {}: {e}", exe.display()))
}

/// Native file picker for an exe (Windows); elsewhere there is no picker.
pub fn browse_for(tool: &ToolDef) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        rfd::FileDialog::new()
            .set_title(format!("Locate {} ({})", tool.name, tool.exe))
            .add_filter("Programs", &["exe"])
            .set_file_name(tool.exe)
            .pick_file()
    }
    #[cfg(not(windows))]
    {
        let _ = tool;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chosen_path_wins_and_missing_is_reported() {
        let dir = std::env::temp_dir().join(format!("fc-launcher-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("f16-lights.exe");
        std::fs::write(&exe, b"x").unwrap();

        let tool = TOOLS.iter().find(|t| t.id == "f16-lights").unwrap();
        let mut s = LocalSettings::default();
        assert!(matches!(locate(tool, &s), Found::Missing | Found::Detected(_)));
        s.tool_paths.insert(tool.id.into(), exe.clone());
        assert!(matches!(locate(tool, &s), Found::Chosen(p) if p == exe));
        // A remembered path that no longer exists falls back to detection.
        std::fs::remove_file(&exe).unwrap();
        assert!(!matches!(locate(tool, &s), Found::Chosen(_)));
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn launch_runs_a_program_with_args() {
        // `true`/`cmd` stand in for a tool on each OS.
        #[cfg(unix)]
        assert!(launch(Path::new("/bin/sh"), &["-c", "exit 0"]).is_ok());
        assert!(launch(Path::new("/definitely/not/here.exe"), &[]).is_err());
    }
}
