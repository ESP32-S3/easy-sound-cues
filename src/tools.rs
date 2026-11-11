//! The free helper programs behind "YouTube to MP3", found on the computer or downloaded from
//! their official pages into the app's own folder, so nobody has to install anything by hand.

use crate::paths::{helpers_dir, quiet};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Tool {
    YtDlp,
    Ffmpeg,
    Deno,
}

pub const ALL: [Tool; 3] = [Tool::YtDlp, Tool::Ffmpeg, Tool::Deno];

impl Tool {
    /// What it is, in plain words.
    pub fn title(self) -> &'static str {
        match self {
            Tool::YtDlp => "YouTube downloader",
            Tool::Ffmpeg => "MP3 maker",
            Tool::Deno => "YouTube helper",
        }
    }

    /// The program's real name.
    pub fn program(self) -> &'static str {
        match self {
            Tool::YtDlp => "yt-dlp",
            Tool::Ffmpeg => "FFmpeg",
            Tool::Deno => "Deno",
        }
    }

    pub fn purpose(self) -> &'static str {
        match self {
            Tool::YtDlp => "Fetches the sound from a YouTube video.",
            Tool::Ffmpeg => "Turns what was downloaded into an MP3 file.",
            Tool::Deno => {
                "Lets the downloader understand YouTube's pages. Downloads may still work without it, but less reliably."
            }
        }
    }

    /// Without it the YouTube tool can't work at all.
    pub fn required(self) -> bool {
        self != Tool::Deno
    }

    /// Roughly how much there is to download, in MB.
    pub fn size_mb(self) -> u32 {
        match (self, std::env::consts::OS) {
            (Tool::YtDlp, "windows") => 22,
            (Tool::YtDlp, _) => 38,
            (Tool::Ffmpeg, "windows") => 86,
            (Tool::Ffmpeg, "macos") => 60,
            (Tool::Ffmpeg, _) => 150,
            (Tool::Deno, _) => 40,
        }
    }

    fn folder(self) -> &'static str {
        match self {
            Tool::YtDlp => "yt-dlp",
            Tool::Ffmpeg => "ffmpeg",
            Tool::Deno => "deno",
        }
    }

    fn exe(self) -> String {
        format!("{}{}", self.folder(), std::env::consts::EXE_SUFFIX)
    }

    /// Where the app keeps its own copy.
    pub fn ours(self) -> PathBuf {
        helpers_dir().join(self.folder()).join(self.exe())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub path: PathBuf,
    pub version: String,
    /// The app's own copy, rather than one already on the computer.
    pub ours: bool,
}

impl Found {
    /// Days since this yt-dlp was released (its versions are dates, like 2026.10.26).
    pub fn age_days(&self) -> Option<i64> {
        let mut parts = self.version.split('.').map(|p| p.parse::<i64>().ok());
        let (Some(Some(y)), Some(Some(m)), Some(Some(d))) = (parts.next(), parts.next(), parts.next()) else {
            return None;
        };
        // Only yt-dlp numbers its versions by date; "2.9.6" is not the year 2.
        if y < 2000 {
            return None;
        }
        let today = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs() as i64 / 86_400;
        Some(today - days_from_civil(y, m, d))
    }

    /// YouTube changes often, and a downloader a couple of months old usually stops working.
    pub fn is_old(&self) -> bool {
        self.age_days().is_some_and(|d| d > 60)
    }
}

/// Days since 1970-01-01 for a calendar date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Folders to look in besides PATH. Apps opened from the Dock or Start menu often get a short
/// PATH that leaves out where Homebrew, winget and friends put things.
fn extra_dirs() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut dirs = vec![home.join(".deno").join("bin"), home.join(".local").join("bin")];
    if cfg!(target_os = "macos") {
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"].map(PathBuf::from));
    }
    if cfg!(windows) {
        if let Some(local) = dirs::data_local_dir() {
            dirs.push(local.join("Microsoft").join("WinGet").join("Links"));
        }
        dirs.push(home.join("scoop").join("shims"));
        dirs.push(PathBuf::from(r"C:\ProgramData\chocolatey\bin"));
    }
    dirs
}

fn on_computer(tool: Tool) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path).chain(extra_dirs()).map(|d| d.join(tool.exe())).find(|p| p.is_file())
}

/// Run `program args` and return what it printed, giving up after `limit`.
fn run(program: &Path, args: &[&str], limit: Duration) -> Result<String, String> {
    let mut child = quiet(Command::new(program).args(args))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("it wouldn't start ({e})"))?;
    let mut out = child.stdout.take().expect("piped");
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = out.read_to_string(&mut text);
        text
    });
    let deadline = Instant::now() + limit;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            return Err("it didn't answer".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let text = reader.join().unwrap_or_default();
    if !status.success() {
        let mut err = String::new();
        if let Some(mut e) = child.stderr.take() {
            let _ = e.read_to_string(&mut err);
        }
        let why = err.lines().find(|l| !l.trim().is_empty()).unwrap_or("it stopped with an error").trim().to_string();
        return Err(why);
    }
    Ok(text)
}

/// Ask a helper for its version, which also proves it runs on this computer.
pub fn version(tool: Tool, path: &Path) -> Result<String, String> {
    // A new download's first start can be slow: virus scanners check it, and yt-dlp unpacks itself.
    let limit = Duration::from_secs(90);
    let text = match tool {
        Tool::Ffmpeg => run(path, &["-hide_banner", "-version"], limit)?,
        _ => run(path, &["--version"], limit)?,
    };
    let first = text.lines().next().unwrap_or("").trim();
    let word = match tool {
        Tool::YtDlp => first.split_whitespace().next(),
        Tool::Ffmpeg => first.split_whitespace().nth(2),
        Tool::Deno => first.split_whitespace().nth(1),
    };
    word.map(str::to_string).filter(|w| !w.is_empty()).ok_or_else(|| "it didn't say which version it is".into())
}

#[derive(Clone, Debug, PartialEq)]
pub enum Check {
    Ready(Found),
    Missing,
    Broken(String),
}

/// Look for a working copy: the app's own first, then one already on the computer.
pub fn check(tool: Tool) -> Check {
    let ours = tool.ours();
    let mut broken = None;
    if ours.is_file() {
        match version(tool, &ours) {
            Ok(version) => return Check::Ready(Found { path: ours, version, ours: true }),
            Err(e) => broken = Some(e),
        }
    }
    if let Some(path) = on_computer(tool)
        && let Ok(version) = version(tool, &path)
    {
        return Check::Ready(Found { path, version, ours: false });
    }
    match broken {
        Some(e) => Check::Broken(e),
        None => Check::Missing,
    }
}

/// Can this computer reach YouTube? Any answer at all counts.
pub fn online() -> bool {
    match agent(Some(Duration::from_secs(8))).head("https://www.youtube.com/").call() {
        Ok(_) | Err(ureq::Error::StatusCode(_)) => true,
        Err(_) => false,
    }
}

fn agent(timeout: Option<Duration>) -> ureq::Agent {
    use ureq::tls::{RootCerts, TlsConfig};
    ureq::Agent::config_builder()
        // The computer's own certificates, so school and office networks that check traffic work.
        .tls_config(TlsConfig::builder().root_certs(RootCerts::PlatformVerifier).build())
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_global(timeout)
        .user_agent(concat!("EasySoundCues/", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent()
}

/// How an install is going.
#[derive(Clone, Debug, PartialEq)]
pub struct Progress {
    pub stage: &'static str,
    pub done: u64,
    pub total: Option<u64>,
}

enum Pack {
    /// The download is the program itself.
    Plain(&'static str),
    /// Take every file in the archive whose path contains `inside` and ends with one of `names`
    /// (or every file there, if `names` is empty).
    Zip {
        inside: &'static str,
        names: &'static [&'static str],
    },
    TarXz {
        names: &'static [&'static str],
    },
}

struct Part {
    url: String,
    pack: Pack,
}

fn unsupported() -> String {
    format!(
        "This kind of computer ({} {}) isn't supported for automatic downloads.",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

/// What to fetch for each helper on this kind of computer.
fn parts(tool: Tool) -> Result<Vec<Part>, String> {
    let (os, arm) = (std::env::consts::OS, std::env::consts::ARCH == "aarch64");
    let gh = |repo: &str, name: &str| format!("https://github.com/{repo}/releases/latest/download/{name}");
    Ok(match tool {
        Tool::YtDlp => {
            let (name, save) = match os {
                "windows" => ("yt-dlp.exe", "yt-dlp.exe"),
                "macos" => ("yt-dlp_macos", "yt-dlp"),
                "linux" if arm => ("yt-dlp_linux_aarch64", "yt-dlp"),
                "linux" => ("yt-dlp_linux", "yt-dlp"),
                _ => return Err(unsupported()),
            };
            vec![Part { url: gh("yt-dlp/yt-dlp", name), pack: Pack::Plain(save) }]
        }
        Tool::Deno => {
            let triple = match (os, arm) {
                ("windows", true) => "aarch64-pc-windows-msvc",
                ("windows", false) => "x86_64-pc-windows-msvc",
                ("macos", true) => "aarch64-apple-darwin",
                ("macos", false) => "x86_64-apple-darwin",
                ("linux", true) => "aarch64-unknown-linux-gnu",
                ("linux", false) => "x86_64-unknown-linux-gnu",
                _ => return Err(unsupported()),
            };
            let pack = Pack::Zip { inside: "", names: &["deno", "deno.exe"] };
            vec![Part { url: gh("denoland/deno", &format!("deno-{triple}.zip")), pack }]
        }
        Tool::Ffmpeg => {
            let builds =
                |name: &str| format!("https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/{name}");
            match os {
                // The "shared" build is about half the size: small programs plus their DLLs.
                "windows" => {
                    let name = if arm { "winarm64" } else { "win64" };
                    let url = builds(&format!("ffmpeg-master-latest-{name}-gpl-shared.zip"));
                    vec![Part { url, pack: Pack::Zip { inside: "/bin/", names: &[] } }]
                }
                "macos" => {
                    let cpu = if arm { "arm64" } else { "amd64" };
                    ["ffmpeg", "ffprobe"]
                        .into_iter()
                        .map(|p| Part {
                            url: format!("https://ffmpeg.martin-riedl.de/redirect/latest/macos/{cpu}/release/{p}.zip"),
                            pack: Pack::Zip { inside: "", names: &["ffmpeg", "ffprobe"] },
                        })
                        .collect()
                }
                "linux" => {
                    let name = if arm { "linuxarm64" } else { "linux64" };
                    let url = builds(&format!("ffmpeg-master-latest-{name}-gpl.tar.xz"));
                    vec![Part { url, pack: Pack::TarXz { names: &["ffmpeg", "ffprobe"] } }]
                }
                _ => return Err(unsupported()),
            }
        }
    })
}

fn net_error(e: ureq::Error) -> String {
    match e {
        ureq::Error::StatusCode(code) => format!("the download page answered with error {code}. Try again later."),
        ureq::Error::Io(e) => format!("the internet connection dropped ({e})."),
        ureq::Error::HostNotFound => "this computer couldn't reach the internet. Check it's connected.".into(),
        ureq::Error::Timeout(_) => "the internet connection is too slow or stopped answering.".into(),
        other => format!("the download didn't work ({other})."),
    }
}

fn download(url: &str, to: &Path, report: &mut dyn FnMut(Progress)) -> Result<(), String> {
    let response = agent(None).get(url).call().map_err(net_error)?;
    let total = response.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok());
    let mut body = response.into_body().into_reader();
    let mut file = File::create(to).map_err(|e| format!("couldn't save it ({e})."))?;
    let mut buf = vec![0u8; 256 * 1024];
    let (mut done, mut last) = (0u64, Instant::now());
    report(Progress { stage: "Downloading", done, total });
    loop {
        let n = body.read(&mut buf).map_err(|e| format!("the download stopped halfway ({e})."))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| format!("couldn't save it. Is the disk full? ({e})"))?;
        done += n as u64;
        if last.elapsed() > Duration::from_millis(100) {
            report(Progress { stage: "Downloading", done, total });
            last = Instant::now();
        }
    }
    if total.is_some_and(|t| done < t) {
        return Err("the download stopped halfway. Try again.".into());
    }
    report(Progress { stage: "Downloading", done, total });
    Ok(())
}

fn unzip(archive: &Path, into: &Path, inside: &str, names: &[&str]) -> Result<(), String> {
    let file = File::open(archive).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| "the download was damaged. Try again.".to_string())?;
    let mut took = 0;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let Some(path) = entry.enclosed_name() else { continue };
        let inner = path.to_string_lossy().replace('\\', "/");
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let wanted = !entry.is_dir()
            && (inside.is_empty() || inner.contains(inside))
            && (names.is_empty() || names.contains(&name.as_str()))
            && !name.starts_with("ffplay");
        if !wanted {
            continue;
        }
        let mut out = File::create(into.join(&name)).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("couldn't unpack {name} ({e})"))?;
        took += 1;
    }
    if took == 0 {
        return Err("the download didn't contain the program. The website may have changed; try again later.".into());
    }
    Ok(())
}

fn untar(archive: &Path, into: &Path, names: &[&str]) -> Result<(), String> {
    let patterns: Vec<String> = names.iter().map(|n| format!("*/bin/{n}")).collect();
    let mut cmd = Command::new("tar");
    cmd.arg("-xJf").arg(archive).arg("-C").arg(into).arg("--wildcards").args(&patterns);
    let status = quiet(&mut cmd).status().map_err(|e| format!("couldn't unpack it (no tar: {e})"))?;
    if !status.success() {
        return Err("couldn't unpack the download. Try again.".into());
    }
    // Lift the programs out of the archive's own folders.
    let mut stack = vec![into.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if dir != into {
                let _ = std::fs::rename(&path, into.join(entry.file_name()));
            }
        }
    }
    Ok(())
}

/// Allow a downloaded program to run: executable on macOS and Linux, and not held back by
/// macOS's "downloaded from the internet" check.
fn allow_running(dir: &Path) {
    #[cfg(unix)]
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(entry.path(), std::fs::Permissions::from_mode(0o755));
    }
    if cfg!(target_os = "macos") {
        let _ = quiet(Command::new("xattr").args(["-dr", "com.apple.quarantine"]).arg(dir)).status();
    }
}

/// Download a helper into the app's own folder and check that it runs. An older copy is only
/// replaced once the new one has proved itself.
pub fn install(tool: Tool, report: &mut dyn FnMut(Progress)) -> Result<Found, String> {
    let home = tool.ours().parent().expect("helper folder").to_path_buf();
    let fresh = home.with_extension("new");
    let _ = std::fs::remove_dir_all(&fresh);
    std::fs::create_dir_all(&fresh).map_err(|e| format!("couldn't make a folder for it ({e})."))?;
    let result = (|| {
        for part in parts(tool)? {
            let archive = fresh.join("download.part");
            download(&part.url, &archive, report)?;
            report(Progress { stage: "Unpacking", done: 0, total: None });
            match part.pack {
                Pack::Plain(name) => std::fs::rename(&archive, fresh.join(name)).map_err(|e| e.to_string())?,
                Pack::Zip { inside, names } => unzip(&archive, &fresh, inside, names)?,
                Pack::TarXz { names } => untar(&archive, &fresh, names)?,
            }
            let _ = std::fs::remove_file(&archive);
        }
        allow_running(&fresh);
        report(Progress { stage: "Checking it works", done: 0, total: None });
        let exe = fresh.join(tool.exe());
        version(tool, &exe).map_err(|e| format!("it downloaded, but won't run on this computer: {e}"))
    })();
    let version = match result {
        Ok(v) => v,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&fresh);
            return Err(e);
        }
    };
    let old = home.with_extension("old");
    let _ = std::fs::remove_dir_all(&old);
    if home.exists() {
        std::fs::rename(&home, &old)
            .map_err(|_| "the old copy is still in use. Wait for downloads to finish, then try again.".to_string())?;
    }
    std::fs::rename(&fresh, &home).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&old);
    Ok(Found { path: tool.ours(), version, ours: true })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_dates_count_days() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(days_from_civil(2026, 11, 11) - days_from_civil(2026, 10, 26), 16);
        let found = Found { path: PathBuf::new(), version: "2001.01.01".into(), ours: true };
        assert!(found.is_old());
        for version in ["7.1.1-1ubuntu1", "2.9.6"] {
            let other = Found { path: PathBuf::new(), version: version.into(), ours: false };
            assert_eq!(other.age_days(), None, "{version} isn't a date");
            assert!(!other.is_old());
        }
    }

    #[test]
    fn every_supported_computer_has_a_download() {
        for tool in ALL {
            let parts = parts(tool).unwrap();
            assert!(!parts.is_empty());
            assert!(parts.iter().all(|p| p.url.starts_with("https://")));
        }
    }

    /// Run by hand (`cargo test -- --ignored`), with EASY_SOUND_CUES_HOME pointing at a scratch
    /// folder: it downloads about 200 MB.
    #[test]
    #[ignore = "downloads from the internet"]
    fn every_helper_downloads_and_runs() {
        assert!(
            std::env::var_os("EASY_SOUND_CUES_HOME").is_some(),
            "point EASY_SOUND_CUES_HOME at a scratch folder first"
        );
        for tool in ALL {
            let mut stages = Vec::new();
            let found = install(tool, &mut |p| stages.push(p.stage)).unwrap_or_else(|e| panic!("{tool:?}: {e}"));
            assert!(found.ours && found.path.is_file(), "{tool:?} wasn't put in the app's folder");
            assert!(stages.contains(&"Checking it works"), "{tool:?} skipped its check");
            assert_eq!(check(tool), Check::Ready(found.clone()), "{tool:?} isn't found again afterwards");
            println!("{tool:?}: {}", found.version);
        }
        // A second install replaces the first cleanly.
        let again = install(Tool::YtDlp, &mut |_| {}).unwrap();
        assert!(again.path.is_file());
        assert!(!again.path.parent().unwrap().with_extension("old").exists());
    }

    #[test]
    fn a_missing_program_is_reported_not_fatal() {
        let err = run(Path::new("/definitely/not/here"), &["--version"], Duration::from_secs(5)).err().unwrap();
        assert!(err.contains("wouldn't start"), "{err}");
    }
}
