//! Keeping the app itself up to date: asks GitHub for the newest release and, when there is a
//! newer one, swaps the program for it and starts it again.
//!
//! The new program is only put in place after it has been downloaded completely and has proved
//! that it runs (by printing its version), so a bad download can't break the app.

use crate::paths::quiet;
use crate::tools::Progress;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const REPO: &str = "ESP32-S3/easy-sound-cues";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The release file holding the bare program for this kind of computer.
pub fn asset_name() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", _) => Some("Easy-Sound-Cues-Windows.exe"),
        ("macos", _) => Some("easy-sound-cues-mac"),
        ("linux", "x86_64") => Some("easy-sound-cues-linux-x64"),
        _ => None,
    }
}

pub fn releases_page() -> String {
    format!("https://github.com/{REPO}/releases/latest")
}

/// "2.10.0" is newer than "2.9.3"; anything unreadable is never newer.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    fn parts(v: &str) -> Option<Vec<u64>> {
        v.trim().trim_start_matches('v').split('.').map(|p| p.parse().ok()).collect()
    }
    match (parts(candidate), parts(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

fn agent(redirects: u32) -> ureq::Agent {
    use ureq::tls::{RootCerts, TlsConfig};
    ureq::Agent::config_builder()
        .tls_config(TlsConfig::builder().root_certs(RootCerts::PlatformVerifier).build())
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .max_redirects(redirects)
        .http_status_as_error(false)
        .user_agent(concat!("EasySoundCues/", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent()
}

/// The newest released version. GitHub answers /releases/latest with a redirect to the tag, which
/// avoids its API and the API's hourly limit (a whole school can share one internet address).
pub fn latest_version() -> Result<String, String> {
    let response = agent(0).get(&releases_page()).call().map_err(|e| format!("couldn't reach GitHub ({e})"))?;
    let location = response
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| format!("GitHub answered {} instead of naming the newest version", response.status()))?;
    let tag = location.rsplit('/').next().unwrap_or("");
    let version = tag.trim_start_matches('v').to_string();
    if is_newer(&version, "0") { Ok(version) } else { Err(format!("couldn't read the version from “{location}”")) }
}

/// The running program, with symlinks followed so the real file is the one replaced.
fn current_exe() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("couldn't find the app's own file ({e})"))?;
    Ok(std::fs::canonicalize(&exe).unwrap_or(exe))
}

fn old_copy(exe: &Path) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    exe.with_file_name(name)
}

/// Remove the copy left behind by the last update (Windows can't delete a program while it runs,
/// so this happens on the next start instead).
pub fn clean_up() {
    if let Ok(exe) = current_exe() {
        let _ = std::fs::remove_file(old_copy(&exe));
    }
}

/// Download version `version`, check it, and put it in place of the running program. Returns the
/// program to start afterwards.
pub fn install(version: &str, report: &mut dyn FnMut(Progress)) -> Result<PathBuf, String> {
    let asset = asset_name().ok_or("there's no automatic update for this kind of computer")?;
    let exe = current_exe()?;
    let dir = exe.parent().ok_or("the app's folder couldn't be found")?;
    let fresh = dir.join(".easy-sound-cues-update");
    let url = format!("https://github.com/{REPO}/releases/download/v{version}/{asset}");
    let result = (|| {
        let mut file = File::create(&fresh).map_err(|e| {
            format!("the app's folder can't be written to ({e}). Download the new version from the website instead")
        })?;
        let response = agent(10).get(&url).call().map_err(|e| format!("the download didn't start ({e})"))?;
        if response.status() != 200 {
            return Err(format!("the download page answered {}", response.status()));
        }
        let total: Option<u64> =
            response.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok());
        let mut body = response.into_body().into_reader();
        let mut buf = vec![0u8; 256 * 1024];
        let (mut done, mut last) = (0u64, Instant::now());
        loop {
            let n = body.read(&mut buf).map_err(|e| format!("the download stopped halfway ({e})"))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).map_err(|e| format!("couldn't save the download ({e})"))?;
            done += n as u64;
            if last.elapsed() > Duration::from_millis(100) {
                report(Progress { stage: "Downloading", done, total });
                last = Instant::now();
            }
        }
        drop(file);
        if total.is_some_and(|t| done < t) || done < 100_000 {
            return Err("the download was incomplete".into());
        }
        report(Progress { stage: "Checking it works", done, total });
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&fresh, std::fs::Permissions::from_mode(0o755));
        }
        let said = version_of(&fresh)?;
        if said != version {
            return Err(format!("the download says it is version {said}, not {version}"));
        }
        Ok(())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&fresh);
        return Err(e);
    }
    // A running program can be renamed on every system, just not overwritten on Windows.
    let old = old_copy(&exe);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| format!("couldn't move the old version aside ({e})"))?;
    if let Err(e) = std::fs::rename(&fresh, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("couldn't put the new version in place ({e})"));
    }
    Ok(exe)
}

/// What a downloaded program says when asked `--version`, which also proves it runs here.
fn version_of(program: &Path) -> Result<String, String> {
    let mut child = quiet(Command::new(program).arg("--version"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("the new version wouldn't start ({e})"))?;
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            break;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            return Err("the new version didn't answer".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut out = String::new();
    if let Some(mut s) = child.stdout.take() {
        let _ = s.read_to_string(&mut out);
    }
    out.split_whitespace()
        .last()
        .map(str::to_string)
        .ok_or_else(|| "the new version didn't say which version it is".into())
}

/// Start the (new) program again. On a Mac that means opening the app bundle it lives in.
pub fn restart(exe: &Path) {
    if cfg!(target_os = "macos")
        && let Some(bundle) = exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app"))
    {
        let _ = Command::new("open").arg("-n").arg(bundle).spawn();
        return;
    }
    let _ = quiet(&mut Command::new(exe)).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number_not_by_letter() {
        assert!(is_newer("2.0.1", "2.0.0"));
        assert!(is_newer("v2.10.0", "2.9.3"));
        assert!(is_newer("3.0", "2.99.99"));
        assert!(!is_newer("2.0.0", "2.0.0"));
        assert!(!is_newer("1.9.9", "2.0.0"));
        assert!(!is_newer("nightly", "2.0.0"));
    }

    #[test]
    fn this_computer_has_an_update_file() {
        if cfg!(any(windows, target_os = "macos", all(target_os = "linux", target_arch = "x86_64"))) {
            assert!(asset_name().is_some());
        }
    }

    #[test]
    fn the_old_copy_sits_next_to_the_program() {
        let old = old_copy(Path::new("/apps/Easy Sound Cues.exe"));
        assert_eq!(old, Path::new("/apps/Easy Sound Cues.exe.old"));
    }
}
