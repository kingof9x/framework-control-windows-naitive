use crate::utils::github as gh;
use tracing::{error, info};

pub fn updates_enabled() -> bool {
    parse_github_repo_env().is_some()
}

pub fn parse_github_repo_env() -> Option<(String, String)> {
    let repo = std::env::var("FRAMEWORK_CONTROL_UPDATE_REPO")
        .ok()
        .or_else(|| option_env!("FRAMEWORK_CONTROL_UPDATE_REPO").map(String::from))?;
    if repo.contains('/') && !repo.contains("github.com") {
        let mut it = repo.splitn(2, '/');
        Some((it.next().unwrap_or("").to_string(), it.next().unwrap_or("").to_string()))
    } else {
        let parts: Vec<&str> = repo.split('/').collect();
        let owner = parts.get(parts.len().saturating_sub(2)).cloned().unwrap_or("");
        let name = parts.get(parts.len().saturating_sub(1)).cloned().unwrap_or("");
        if owner.is_empty() || name.is_empty() {
            None
        } else {
            Some((owner.to_string(), name.to_string()))
        }
    }
}

/// Version compared against GitHub release tags. Builds that publish under a
/// different release line (e.g. a fork that bundles extra apps) bake their own
/// via `FRAMEWORK_CONTROL_RELEASE_VERSION`; otherwise the crate version is used.
pub fn release_version() -> &'static str {
    option_env!("FRAMEWORK_CONTROL_RELEASE_VERSION")
        .filter(|v| !v.trim().is_empty())
        .unwrap_or(env!("CARGO_PKG_VERSION"))
}

/// Numeric semver-ish comparison ("1.10.0" > "1.9.0"); pre-release suffixes are ignored.
pub fn is_newer(latest: &str, current: &str) -> bool {
    fn parts(v: &str) -> Vec<u64> {
        v.trim()
            .trim_start_matches('v')
            .split(['-', '+'])
            .next()
            .unwrap_or("")
            .split('.')
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    }
    let (a, b) = (parts(latest), parts(current));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

pub async fn get_current_and_latest() -> Result<(String, String), String> {
    let current = release_version().to_string();
    let current_trimmed = current.trim().to_string();
    if current_trimmed.is_empty() {
        return Err("current version missing".into());
    }
    let Some((owner, name)) = parse_github_repo_env() else {
        return Err("FRAMEWORK_CONTROL_UPDATE_REPO not set".into());
    };
    let latest_opt = gh::get_latest_release_version_tag(&owner, &name).await?;
    let latest = latest_opt.ok_or_else(|| "latest version missing".to_string())?;
    Ok((current_trimmed, latest))
}

/// Name of the one-off scheduled task that runs the installer (see `spawn_msiexec_install`).
#[cfg(target_os = "windows")]
const UPDATE_TASK: &str = "FrameworkControlUpdate";

#[cfg(target_os = "windows")]
async fn spawn_msiexec_install(msi_url: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // Somewhere stable without spaces (SYSTEM's temp folder can be cleaned mid-install).
    let dir = std::path::PathBuf::from(std::env::var("PROGRAMDATA").unwrap_or_else(|_| r"C:\ProgramData".into()))
        .join("FrameworkControl");
    std::fs::create_dir_all(&dir).map_err(|e| format!("failed to create {}: {e}", dir.display()))?;
    let msi = dir.join("update.msi");
    let log = dir.join("update-install.log");

    let resp = reqwest::get(msi_url.to_string())
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("failed to download msi: {e}"))?;
    let bytes = resp.bytes().await.map_err(|_| "failed to read msi bytes".to_string())?;
    std::fs::write(&msi, &bytes).map_err(|_| "failed to write msi".to_string())?;

    // ADDLOCAL=Binaries keeps the user's original feature choice; /l*v leaves a log for troubleshooting.
    let msiexec = format!(
        "msiexec.exe /i {} ADDLOCAL=Binaries /qn /norestart /l*v {}",
        msi.display(),
        log.display()
    );

    // The installer must not run as our child process: it stops this service to replace its files, and
    // the service wrapper (WinSW) then ends the service's whole process tree, which would kill msiexec
    // half way and roll the update back. A one-off scheduled task runs it from Task Scheduler instead.
    // schtasks /TR is a single string; keep it simple by only using it when the paths need no quoting.
    let plain_paths = !msi.display().to_string().contains(' ') && !log.display().to_string().contains(' ');
    if plain_paths {
        let created = std::process::Command::new("schtasks")
            .args(["/Create", "/TN", UPDATE_TASK, "/TR", &msiexec, "/SC", "ONCE", "/ST", "00:00", "/RU", "SYSTEM", "/RL", "HIGHEST", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        match created {
            Ok(out) if out.status.success() => {
                let ran = std::process::Command::new("schtasks")
                    .args(["/Run", "/TN", UPDATE_TASK])
                    .creation_flags(CREATE_NO_WINDOW)
                    .output();
                match ran {
                    Ok(out) if out.status.success() => {
                        info!("update: installer started from Task Scheduler (log: {})", log.display());
                        return Ok(());
                    }
                    Ok(out) => error!("update: schtasks /Run failed: {}", String::from_utf8_lossy(&out.stderr).trim()),
                    Err(e) => error!("update: schtasks /Run failed: {e}"),
                }
            }
            Ok(out) => error!("update: schtasks /Create failed: {}", String::from_utf8_lossy(&out.stderr).trim()),
            Err(e) => error!("update: schtasks unavailable: {e}"),
        }
    }

    // Fallback: start msiexec through cmd's `start`, detached and (if allowed) outside our job object, so
    // it is no longer part of the service's process tree once cmd exits.
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    let start = |flags: u32| {
        std::process::Command::new("cmd.exe")
            .raw_arg(format!("/c start \"\" /b {msiexec}"))
            .creation_flags(flags)
            .spawn()
    };
    start(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB)
        .or_else(|_| start(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW))
        .map(|_| info!("update: installer started detached (log: {})", log.display()))
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "linux")]
async fn extract_and_replace_binary(tarball_url: &str) -> Result<(), String> {
    let tmp_dir = std::env::temp_dir().join("framework-control-update");
    std::fs::create_dir_all(&tmp_dir).map_err(|e| format!("failed to create temp dir: {}", e))?;

    // Download and extract tarball (download_to_path handles tar.gz extraction automatically)
    let extracted_dir =
        crate::utils::download::download_to_path(tarball_url, &tmp_dir.to_string_lossy().to_string()).await?;

    // Find the extracted binary
    let extracted_binary = std::path::Path::new(&extracted_dir).join("framework-control");
    if !extracted_binary.exists() {
        return Err("extracted binary not found".to_string());
    }

    // Get the current binary path
    let current_exe = std::env::current_exe().map_err(|e| format!("failed to get current exe path: {}", e))?;

    // Create path for new binary (same directory as current)
    let current_dir = current_exe
        .parent()
        .ok_or_else(|| "failed to get current binary directory".to_string())?;
    let new_binary = current_dir.join("framework-control.new");

    info!("Installing update to {}", new_binary.display());

    // Copy to .new file
    std::fs::copy(&extracted_binary, &new_binary)
        .map_err(|e| format!("failed to copy new binary (may need root/sudo permissions): {}", e))?;

    // Make it executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&new_binary)
            .map_err(|e| format!("failed to get binary metadata: {}", e))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&new_binary, perms).map_err(|e| format!("failed to set binary permissions: {}", e))?;
    }

    // Clean up temp files
    let _ = std::fs::remove_dir_all(&tmp_dir);

    info!("Binary staged successfully, scheduling atomic replacement and restart...");

    // Spawn a background task to atomically replace and restart
    let current_exe_clone = current_exe.clone();
    let new_binary_clone = new_binary.clone();
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Atomic rename (this works even if the old binary is running)
        if let Err(e) = std::fs::rename(&new_binary_clone, &current_exe_clone) {
            error!("Failed to replace binary: {}", e);
            return;
        }

        info!("Binary replaced successfully, restarting service...");

        match tokio::process::Command::new("systemctl")
            .arg("restart")
            .arg("framework-control")
            .status()
            .await
        {
            Ok(status) if status.success() => {
                info!("Service restart initiated");
            }
            Ok(status) => {
                info!(
                    "Service restart failed (exit {}). Manual restart required: \
                     sudo systemctl restart framework-control",
                    status
                );
            }
            Err(e) => {
                info!(
                    "Could not restart service ({}). Manual restart required: \
                     sudo systemctl restart framework-control",
                    e
                );
            }
        }
    });

    Ok(())
}

/// Checks for a newer release and, if found, downloads and starts installation.
/// Returns Ok(true) if an update was initiated, Ok(false) if no update needed.
pub async fn check_and_apply_now() -> Result<bool, String> {
    let Some((owner, name)) = parse_github_repo_env() else {
        return Err("FRAMEWORK_CONTROL_UPDATE_REPO not set".into());
    };
    let (current, latest) = get_current_and_latest().await?;
    if !is_newer(&latest, &current) {
        return Ok(false);
    }
    #[cfg(target_os = "windows")]
    let preferred_exts: &[&str] = &[".msi"];
    #[cfg(target_os = "linux")]
    let preferred_exts: &[&str] = &[".tar.gz"];
    let Some(installer_url) = gh::get_release(&owner, &name, None, preferred_exts)
        .await
        .map_err(|e| {
            error!("update: fetch assets failed: {}", e);
            e
        })?
    else {
        error!("update: no installer asset in latest release");
        return Err("installer asset not found".into());
    };

    #[cfg(target_os = "windows")]
    {
        match spawn_msiexec_install(&installer_url).await {
            Ok(_) => {
                info!("msiexec started for update");
                Ok(true)
            }
            Err(e) => {
                error!("failed to start msiexec: {}", e);
                Err(e)
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        match extract_and_replace_binary(&installer_url).await {
            Ok(_) => {
                info!("Linux binary updated successfully");
                Ok(true)
            }
            Err(e) => {
                error!("failed to update Linux binary: {}", e);
                Err(e)
            }
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        Err("update apply unsupported on this OS".into())
    }
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn compares_numerically() {
        assert!(is_newer("1.10.0", "1.9.0"));
        assert!(is_newer("v1.0.1", "1.0.0"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("0.5.5", "1.0.0"));
        assert!(is_newer("1.1", "1.0.9"));
    }
}
