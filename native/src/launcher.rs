//! Companion Framework 16 tools (White Rabbit, Framework 16 Keyboard Control) that this app can start
//! (see https://github.com/kingof9x/my-framework-16-tools).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

pub struct Action {
    pub label: &'static str,
    pub args: &'static [&'static str],
    pub hint: &'static str,
}

/// How a new version of a tool is installed.
pub enum UpdateKind {
    /// Run the release's setup program (White Rabbit). Portable copies get the release page instead.
    Installer { asset: &'static str },
    /// The release is the exe itself (Framework 16 Keyboard Control): swap it in place.
    Exe { asset: &'static str },
}

pub struct ToolDef {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub exe: &'static str,
    /// Earlier names of the exe, still found so older downloads keep working.
    pub old_exes: &'static [&'static str],
    pub actions: &'static [Action],
    /// Where the tool's own installer puts it, relative to a known folder.
    install_dirs: &'static [(&'static str, &'static str)],
    pub download_label: &'static str,
    pub download_url: &'static str,
    pub project_url: &'static str,
    /// GitHub "owner/name", for update checks.
    pub repo: &'static str,
    pub update: UpdateKind,
}

pub const TOOLS: &[ToolDef] = &[
    ToolDef {
        id: "white-rabbit",
        name: "White Rabbit",
        description: "The LED Matrix app: a system monitor you design, games and patterns, startup animation and firmware updates.",
        exe: "WhiteRabbit.exe",
        old_exes: &[],
        actions: &[
            // A plain start runs the monitor and opens the window; if the monitor is already running, it just opens the window.
            Action { label: "Open", args: &[], hint: "Starts the LED monitor if needed and opens the window" },
            Action { label: "Edit panel layout", args: &["--edit-layout"], hint: "Choose what shows on each panel" },
            Action { label: "Games & patterns", args: &["--window", "--tab", "play"], hint: "" },
        ],
        install_dirs: &[
            ("LOCALAPPDATA", "Programs\\WhiteRabbit"),
            ("USERPROFILE", "Downloads\\WhiteRabbit-portable"),
            ("USERPROFILE", "Downloads"),
            ("USERPROFILE", "Desktop"),
        ],
        download_label: "Download installer",
        download_url: "https://github.com/kingof9x/white-rabbit-framework-matrix/releases/latest/download/WhiteRabbit-setup.exe",
        project_url: "https://github.com/kingof9x/white-rabbit-framework-matrix",
        repo: "kingof9x/white-rabbit-framework-matrix",
        update: UpdateKind::Installer { asset: "WhiteRabbit-setup.exe" },
    },
    ToolDef {
        id: "f16-lights",
        name: "Framework 16 Keyboard Control",
        description: "RGB keyboard and macropad lighting and setup: per-device effects, linked effects, hotkey lock and macropad layers.",
        exe: "framework-16-keyboard-control.exe",
        old_exes: &["f16-lights.exe"],
        actions: &[Action { label: "Open", args: &[], hint: "" }],
        install_dirs: &[
            ("LOCALAPPDATA", "Programs\\framework-16-keyboard-control"),
            ("LOCALAPPDATA", "Programs\\Framework 16 Keyboard Control"),
            ("USERPROFILE", "Downloads"),
            ("USERPROFILE", "Downloads\\framework-16-keyboard-control-windows"),
            ("USERPROFILE", "Desktop"),
            ("LOCALAPPDATA", "Programs\\f16-lights"),
            ("USERPROFILE", "Downloads\\f16-lights-windows"),
        ],
        download_label: "Get the latest build",
        download_url: "https://github.com/kingof9x/framework-16-keyboard-customization/releases/latest",
        project_url: "https://github.com/kingof9x/framework-16-keyboard-customization",
        repo: "kingof9x/framework-16-keyboard-customization",
        update: UpdateKind::Exe { asset: "framework-16-keyboard-control.exe" },
    },
];

// ---------------------------------------------------------------- settings ---

/// Per-user preferences of the native app (tool locations). Kept out of the
/// service config, which is shared, system-owned and upstream's schema.
#[derive(Default, Serialize, Deserialize)]
pub struct LocalSettings {
    #[serde(default)]
    pub tool_paths: BTreeMap<String, PathBuf>,
    /// The Overview's temperature view: "grouped" (default) or "single".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart_view: Option<String>,
    /// An update the app started and hasn't seen finish yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_update: Option<crate::updater::PendingUpdate>,
    /// "Ignore touchpad clicks while typing".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub touchpad: Option<crate::touchpad::TouchpadSettings>,
    /// Versions of the tools that Framework Control updated itself (by tool id).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tool_versions: BTreeMap<String, crate::tool_updates::Recorded>,
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

#[derive(Clone)]
pub enum Found {
    /// Picked by the user with Browse…
    Chosen(PathBuf),
    /// Found in a standard location.
    Detected(PathBuf),
    Missing,
}

impl ToolDef {
    /// The release file an update downloads.
    pub fn update_asset(&self) -> &'static str {
        match self.update {
            UpdateKind::Installer { asset } | UpdateKind::Exe { asset } => asset,
        }
    }
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
    let mut dirs: Vec<PathBuf> = tool
        .install_dirs
        .iter()
        .filter_map(|(var, rel)| std::env::var_os(var).map(|b| PathBuf::from(b).join(rel)))
        .collect();
    // Next to this app (e.g. copied into the Framework Control folder).
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        dirs.push(dir);
    }
    // Anywhere on PATH.
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    // The current name everywhere first, then earlier names.
    let names: Vec<&str> = std::iter::once(tool.exe).chain(tool.old_exes.iter().copied()).collect();
    let candidates = names.iter().flat_map(|n| dirs.iter().map(move |d| d.join(n)));
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

/// After an update has started: reopen this app once the installer has replaced its exe.
///
/// A hidden PowerShell watcher waits (up to 10 minutes) for the exe's timestamp to change, then
/// starts it. It's PowerShell rather than a copy of this exe so nothing holds the file the installer
/// needs to replace.
pub fn reopen_after_update() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        let path = exe.display().to_string().replace('\'', "''");
        let script = format!(
            "$exe='{path}'; $old=(Get-Item -LiteralPath $exe).LastWriteTimeUtc; $end=(Get-Date).AddMinutes(10); \
             while ((Get-Date) -lt $end) {{ Start-Sleep -Seconds 2; \
               try {{ if ((Get-Item -LiteralPath $exe -ErrorAction Stop).LastWriteTimeUtc -ne $old) \
                 {{ Start-Sleep -Seconds 5; Start-Process -FilePath $exe; break }} }} catch {{ }} }}"
        );
        Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-ExecutionPolicy", "Bypass", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = exe;
        Err("only supported on Windows".into())
    }
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
        let exe = dir.join("framework-16-keyboard-control.exe");
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
