//! Update checks for the companion tools (White Rabbit, Framework 16 Keyboard Control): compare the
//! copy on this PC with the newest GitHub release of its project, and install the new one.
//!
//! The installed version comes from, in order: the version Framework Control recorded when it last
//! updated that exe; a `VERSION` file next to the exe (White Rabbit ships one); the exe's own file
//! version. When none of those is known, a copy older than the newest release counts as out of date.

use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::launcher::{LocalSettings, ToolDef, UpdateKind};
use crate::version::is_newer;

/// The newest release of a tool's project.
#[derive(Clone, Debug)]
pub struct Latest {
    pub version: String,
    pub page: String,
    pub asset_url: Option<String>,
    pub published: Option<u64>,
}

/// What the check found for one tool.
#[derive(Clone, Debug)]
pub enum Status {
    /// The project hasn't published a release yet.
    NoReleases,
    UpToDate { installed: Option<String> },
    Available { installed: Option<String>, latest: Latest },
    /// Not installed: just the newest version, for the download button.
    NotInstalled { latest: Latest },
    Failed,
}

impl Status {
    pub fn update_available(&self) -> Option<&Latest> {
        match self {
            Status::Available { latest, .. } => Some(latest),
            _ => None,
        }
    }
}

/// The version Framework Control installed, tied to the exact file it wrote.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Recorded {
    pub version: String,
    pub len: u64,
    pub modified: u64,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).timeout(timeout).build()
}

/// The newest release of `repo` (None when there is none yet).
pub fn latest(repo: &str, asset: &str) -> Result<Option<Latest>, String> {
    let r = agent(Duration::from_secs(20))
        .get(&format!("https://api.github.com/repos/{repo}/releases/latest"))
        .set("User-Agent", "framework-control-app")
        .set("Accept", "application/vnd.github+json")
        .call();
    let release: Release = match r {
        Ok(resp) => resp.into_json().map_err(|e| format!("unexpected answer from GitHub: {e}"))?,
        Err(ureq::Error::Status(404, _)) => return Ok(None),
        Err(ureq::Error::Status(403 | 429, _)) => return Err("GitHub is limiting update checks; try again later".into()),
        Err(e) => return Err(format!("couldn't reach GitHub: {e}")),
    };
    Ok(Some(Latest {
        version: release.tag_name.trim_start_matches('v').to_string(),
        page: release.html_url,
        asset_url: release.assets.into_iter().find(|a| a.name.eq_ignore_ascii_case(asset)).map(|a| a.browser_download_url),
        published: release.published_at.as_deref().and_then(parse_rfc3339),
    }))
}

/// "2026-11-19T01:26:12Z" as seconds since 1970.
fn parse_rfc3339(s: &str) -> Option<u64> {
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (hh, mm, ss) = (n(11..13)?, n(14..16)?, n(17..19)?);
    // Days from 1970-01-01 to the date (Howard Hinnant's days_from_civil).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + hh * 3600 + mm * 60 + ss).ok()
}

fn file_stamp(path: &Path) -> Option<(u64, u64)> {
    let m = std::fs::metadata(path).ok()?;
    let modified = m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some((m.len(), modified))
}

/// The version of the tool at `exe`, when it can be told.
pub fn installed_version(tool: &ToolDef, exe: &Path, settings: &LocalSettings) -> Option<String> {
    if let (Some(rec), Some((len, modified))) = (settings.tool_versions.get(tool.id), file_stamp(exe)) {
        if rec.len == len && rec.modified == modified {
            return Some(rec.version.clone());
        }
    }
    let from_file = exe
        .parent()
        .and_then(|d| std::fs::read_to_string(d.join("VERSION")).ok())
        .map(|v| v.trim().trim_start_matches('v').to_string())
        .filter(|v| !v.is_empty());
    from_file.or_else(|| file_version(exe))
}

/// Compare the copy at `exe` (if any) with the newest release.
pub fn status(tool: &ToolDef, exe: Option<&Path>, settings: &LocalSettings, latest: Result<Option<Latest>, String>) -> Status {
    let latest = match latest {
        Ok(Some(l)) => l,
        Ok(None) => return Status::NoReleases,
        Err(_) => return Status::Failed,
    };
    let Some(exe) = exe else { return Status::NotInstalled { latest } };
    let installed = installed_version(tool, exe, settings);
    let newer = match &installed {
        Some(v) => is_newer(&latest.version, v),
        // Version unknown: out of date if the file is older than the release.
        None => match (file_stamp(exe), latest.published) {
            (Some((_, modified)), Some(published)) => modified < published,
            _ => false,
        },
    };
    if newer {
        Status::Available { installed, latest }
    } else {
        Status::UpToDate { installed }
    }
}

/// White Rabbit's installer leaves an uninstaller next to the exe; a portable copy has none.
fn installed_by_setup(exe: &Path) -> bool {
    exe.parent().is_some_and(|d| d.join("unins000.exe").is_file())
}

/// Install `latest` over the copy at `exe`. Returns a message for the page.
pub fn install(tool: &ToolDef, exe: &Path, latest: &Latest, running: bool) -> Result<String, String> {
    match tool.update {
        UpdateKind::Installer { .. } if !installed_by_setup(exe) => {
            open_url(&latest.page)?;
            Ok(format!("This copy of {} is portable, so its release page is open: download {} there.", tool.name, latest.version))
        }
        UpdateKind::Installer { asset } => {
            let url = latest.asset_url.as_deref().ok_or_else(|| format!("The newest release has no {asset}."))?;
            let dir = std::env::temp_dir().join("FrameworkControl");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let setup = dir.join(asset);
            download(url, &setup)?;
            std::process::Command::new(&setup).spawn().map_err(|e| format!("Couldn't start the installer: {e}"))?;
            Ok(format!("The {} {} installer is open.", tool.name, latest.version))
        }
        UpdateKind::Exe { asset } => {
            let url = latest.asset_url.as_deref().ok_or_else(|| format!("The newest release has no {asset}."))?;
            replace_exe(exe, url)?;
            let mut s = LocalSettings::load();
            if let Some((len, modified)) = file_stamp(exe) {
                s.tool_versions.insert(tool.id.into(), Recorded { version: latest.version.clone(), len, modified });
                let _ = s.save();
            }
            Ok(if running {
                format!("Updated {} to {}. Close and reopen it to use the new version.", tool.name, latest.version)
            } else {
                format!("Updated {} to {}.", tool.name, latest.version)
            })
        }
    }
}

/// Swap in a new exe. A running exe can't be overwritten on Windows but can be renamed, so the old one
/// moves aside (`.old`, removed next time) and the new one takes its name.
fn replace_exe(exe: &Path, url: &str) -> Result<(), String> {
    let new = with_suffix(exe, ".download");
    let old = with_suffix(exe, ".old");
    download(url, &new)?;
    let _ = std::fs::remove_file(&old);
    let denied = |e: std::io::Error| {
        let _ = std::fs::remove_file(&new);
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            format!("Windows didn't allow replacing {} (it's in a protected folder). Download it from the release page instead.", exe.display())
        } else {
            format!("Couldn't replace {}: {e}", exe.display())
        }
    };
    std::fs::rename(exe, &old).map_err(denied)?;
    if let Err(e) = std::fs::rename(&new, exe) {
        let _ = std::fs::rename(&old, exe);
        return Err(format!("Couldn't put the new version in place: {e}"));
    }
    let _ = std::fs::remove_file(&old);
    Ok(())
}

/// Remove an `.old` exe left from an update made while the tool was running.
pub fn clean_up(exe: &Path) {
    let old = with_suffix(exe, ".old");
    if old.is_file() {
        let _ = std::fs::remove_file(old);
    }
}

fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

fn download(url: &str, to: &Path) -> Result<(), String> {
    let mut body = agent(Duration::from_secs(600))
        .get(url)
        .set("User-Agent", "framework-control-app")
        .call()
        .map_err(|e| format!("Couldn't download the new version: {e}"))?
        .into_reader();
    let mut file = std::fs::File::create(to).map_err(|e| format!("Couldn't save the download: {e}"))?;
    std::io::copy(&mut body, &mut file).map_err(|e| format!("Couldn't save the download: {e}"))?;
    Ok(())
}

fn open_url(url: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        std::process::Command::new("explorer.exe").arg(url).spawn().map(|_| ()).map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open").arg(url).spawn().map(|_| ()).map_err(|e| e.to_string())
    }
}

/// The exe's product version from its file details (Windows), e.g. "1.5.0.0" -> "1.5.0".
#[cfg(windows)]
fn file_version(exe: &Path) -> Option<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO};
    let path: Vec<u16> = exe.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    unsafe {
        let size = GetFileVersionInfoSizeW(path.as_ptr(), std::ptr::null_mut());
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        if GetFileVersionInfoW(path.as_ptr(), 0, size, buf.as_mut_ptr().cast()) == 0 {
            return None;
        }
        let mut info: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let root = [b'\\' as u16, 0];
        if VerQueryValueW(buf.as_ptr().cast(), root.as_ptr(), &mut info, &mut len) == 0 || info.is_null() {
            return None;
        }
        let f = &*(info as *const VS_FIXEDFILEINFO);
        let parts = [f.dwProductVersionMS >> 16, f.dwProductVersionMS & 0xFFFF, f.dwProductVersionLS >> 16];
        // Builds without real file details often say 0.0.0 or 0.1.0; those don't tell us anything.
        if parts[0] == 0 {
            return None;
        }
        Some(format!("{}.{}.{}", parts[0], parts[1], parts[2]))
    }
}

#[cfg(not(windows))]
fn file_version(_exe: &Path) -> Option<String> {
    None
}

/// How often the companion tools are checked while Framework Control is open.
pub const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_github_dates() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339("2026-11-19T01:26:12Z"), Some(1_795_051_572));
        assert_eq!(parse_rfc3339("nope"), None);
    }

    #[test]
    fn unknown_version_compares_file_age() {
        let dir = std::env::temp_dir().join(format!("fc-toolupd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("framework-16-keyboard-control.exe");
        std::fs::write(&exe, b"x").unwrap();
        let tool = crate::launcher::TOOLS.iter().find(|t| t.id == "f16-lights").unwrap();
        let s = LocalSettings::default();
        let latest = |published| Latest { version: "1.0".into(), page: String::new(), asset_url: None, published: Some(published) };
        let old_release = latest(1);
        assert!(matches!(status(tool, Some(&exe), &s, Ok(Some(old_release))), Status::UpToDate { installed: None, .. }));
        let future = std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + 3600;
        assert!(matches!(status(tool, Some(&exe), &s, Ok(Some(latest(future)))), Status::Available { .. }));
        // A VERSION file wins over the file's age.
        std::fs::write(dir.join("VERSION"), "1.0").unwrap();
        assert!(matches!(status(tool, Some(&exe), &s, Ok(Some(latest(future)))), Status::UpToDate { .. }));
        assert!(matches!(status(tool, Some(&exe), &s, Ok(None)), Status::NoReleases));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn replaced_exe_keeps_a_rollback_until_done() {
        let p = Path::new("C:/x/tool.exe");
        assert_eq!(with_suffix(p, ".old"), PathBuf::from("C:/x/tool.exe.old"));
    }
}
