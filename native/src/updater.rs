//! The app's own "Install update": download the newest installer from this repo's GitHub releases and
//! run it where you can see it (Windows' install progress window and the administrator prompt), instead
//! of asking the background service to install silently. If an update doesn't finish, the app finds
//! out the next time it starts and offers to try again or open the installer's log.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::version::REPO;

/// An update that was started; kept until the app starts at that version (or newer).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct PendingUpdate {
    pub version: String,
    pub msi: PathBuf,
    pub log: PathBuf,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(600))
        .build()
}

/// Download the latest release's installer and start it. Returns what was started.
pub fn download_and_run() -> Result<PendingUpdate, String> {
    let agent = agent();
    let release: Release = agent
        .get(&format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .set("User-Agent", "framework-control-app")
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("couldn't check GitHub: {e}"))?
        .into_json()
        .map_err(|e| format!("unexpected answer from GitHub: {e}"))?;
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.to_ascii_lowercase().ends_with(".msi"))
        .ok_or("the latest release has no installer (.msi)")?;

    let dir = std::env::temp_dir().join("FrameworkControl");
    std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't create {}: {e}", dir.display()))?;
    let msi = dir.join(&asset.name);
    let log = dir.join("update-install.log");
    let mut body = agent
        .get(&asset.browser_download_url)
        .set("User-Agent", "framework-control-app")
        .call()
        .map_err(|e| format!("couldn't download the installer: {e}"))?
        .into_reader();
    let mut file = std::fs::File::create(&msi).map_err(|e| format!("couldn't save the installer: {e}"))?;
    std::io::copy(&mut body, &mut file).map_err(|e| format!("couldn't save the installer: {e}"))?;
    drop(file);

    run_installer(&msi, Some(&log))?;
    Ok(PendingUpdate { version: release.tag_name.trim_start_matches('v').to_string(), msi, log })
}

/// Run an installer. With a log: progress window only (`/passive`) and a verbose log. Without: the full
/// wizard, for "Try again".
pub fn run_installer(msi: &Path, log: Option<&Path>) -> Result<(), String> {
    let mut cmd = Command::new("msiexec.exe");
    cmd.arg("/i").arg(msi);
    if let Some(log) = log {
        cmd.args(["/passive", "/norestart", "/l*v"]).arg(log);
    }
    cmd.spawn().map(|_| ()).map_err(|e| format!("couldn't start the installer: {e}"))
}

/// Open a text file (the installer's log) in Notepad.
pub fn open_log(log: &Path) -> Result<(), String> {
    Command::new("notepad.exe").arg(log).spawn().map(|_| ()).map_err(|e| e.to_string())
}
