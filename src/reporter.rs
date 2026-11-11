//! Error reports: when anything goes wrong, a short anonymous report goes to the app's maker so it
//! can be fixed, without the person at the computer having to do or know anything.
//!
//! It starts before anything else (before the window, before the sound), so even a crash while
//! the app is opening gets reported. A report that can't be sent straight away (no internet, or
//! the app crashing) is kept on disk and sent the next time the app starts.
//!
//! Reports never contain names: the home folder and user name are cut out of every line, show
//! titles aren't included, and the computer is identified only by a random id made up on first
//! launch. People can turn reports off in Settings.

use serde::Serialize;
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The server collecting reports (its code is in report-server/ in the app's repository).
pub const ENDPOINT: &str = "https://easy-sound-cues-reports.cmd-chat.workers.dev/report";
/// Enough to see a pattern, not enough to flood anyone if something goes wrong in a loop.
const MAX_PER_RUN: usize = 25;
const LOG_LINES: usize = 80;
const KEEP_UNSENT: Duration = Duration::from_secs(14 * 86_400);

#[derive(Serialize, Clone, Debug)]
pub struct Report {
    app: &'static str,
    version: &'static str,
    install: String,
    os: String,
    arch: &'static str,
    stage: String,
    kind: &'static str,
    message: String,
    detail: String,
    log: Vec<String>,
    uptime_secs: u64,
    time: u64,
}

struct State {
    enabled: AtomicBool,
    install: String,
    os: String,
    started: Instant,
    log: Mutex<VecDeque<String>>,
    seen: Mutex<HashSet<String>>,
    count: AtomicUsize,
    queue: Mutex<Sender<Report>>,
}

static STATE: OnceLock<State> = OnceLock::new();

/// Tests and screenshots never send anything, unless EASY_SOUND_CUES_REPORTS=1 asks for it.
fn may_send() -> bool {
    !cfg!(test)
        && (std::env::var_os("EASY_SOUND_CUES_HOME").is_none()
            || std::env::var_os("EASY_SOUND_CUES_REPORTS").is_some_and(|v| v == "1"))
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn unsent_dir() -> PathBuf {
    crate::paths::data_dir().join("reports-to-send")
}

/// Remove anything that could identify the person: their home folder and their user name.
pub fn scrub(text: &str) -> String {
    let mut out = text.to_string();
    if let Some(home) = dirs::home_dir() {
        let home = home.display().to_string();
        if home.len() > 3 {
            out = out.replace(&home, "~");
            // Paths sometimes show up with the other kind of slash.
            out = out.replace(&home.replace('\\', "/"), "~");
        }
    }
    for var in ["USER", "USERNAME", "LOGNAME"] {
        if let Ok(name) = std::env::var(var)
            && name.chars().count() >= 3
        {
            out = out.replace(&name, "<user>");
        }
    }
    out
}

fn os_name() -> String {
    let info = os_info::get();
    format!("{} {} ({})", info.os_type(), info.version(), info.bitness())
}

/// Start reporting. Call this first thing, before anything that could go wrong.
pub fn start(enabled: bool, install: String) {
    let (tx, rx) = channel::<Report>();
    let state = State {
        enabled: AtomicBool::new(enabled),
        install,
        os: os_name(),
        started: Instant::now(),
        log: Mutex::new(VecDeque::new()),
        seen: Mutex::new(HashSet::new()),
        count: AtomicUsize::new(0),
        queue: Mutex::new(tx),
    };
    if STATE.set(state).is_err() {
        return;
    }
    install_panic_hook();
    log(format!("started {} on {} {}", env!("CARGO_PKG_VERSION"), os_name(), std::env::consts::ARCH));
    // One thread sends everything, in order: first what earlier runs couldn't, then new reports.
    std::thread::Builder::new()
        .name("error reports".into())
        .spawn(move || {
            send_unsent();
            while let Ok(report) = rx.recv() {
                if !post(&report, Duration::from_secs(20)) {
                    keep_for_later(&report);
                }
            }
        })
        .ok();
}

pub fn set_enabled(on: bool) {
    if let Some(state) = STATE.get() {
        state.enabled.store(on, Ordering::Relaxed);
    }
}

fn enabled() -> bool {
    STATE.get().is_some_and(|s| s.enabled.load(Ordering::Relaxed)) && may_send()
}

/// Note something the app did, so a report can show what happened just before a problem.
pub fn log(event: impl AsRef<str>) {
    let Some(state) = STATE.get() else { return };
    let secs = state.started.elapsed().as_secs();
    let line = format!("{:02}:{:02}:{:02} {}", secs / 3600, secs / 60 % 60, secs % 60, scrub(event.as_ref()));
    if let Ok(mut log) = state.log.lock() {
        if log.len() == LOG_LINES {
            log.pop_front();
        }
        log.push_back(line);
    }
}

fn build(stage: &str, kind: &'static str, message: &str, detail: &str) -> Option<Report> {
    let state = STATE.get()?;
    // try_lock: a crash in the middle of logging must not leave the crash report waiting forever.
    let log = state.log.try_lock().map(|l| l.iter().cloned().collect()).unwrap_or_default();
    Some(Report {
        app: "easy-sound-cues",
        version: env!("CARGO_PKG_VERSION"),
        install: state.install.clone(),
        os: state.os.clone(),
        arch: std::env::consts::ARCH,
        stage: scrub(stage),
        kind,
        message: scrub(message).chars().take(2000).collect(),
        detail: scrub(detail).chars().take(20_000).collect(),
        log,
        uptime_secs: state.started.elapsed().as_secs(),
        time: now(),
    })
}

/// Report a problem in the background. The same problem is only reported once per run.
pub fn error(stage: &str, message: &str, detail: &str) {
    log(format!("problem in {stage}: {message}"));
    let Some(state) = STATE.get() else { return };
    if !enabled() {
        return;
    }
    let key = format!("{stage}\n{}", message.chars().take(160).collect::<String>());
    let new = state.seen.lock().map(|mut s| s.insert(key)).unwrap_or(false);
    if !new || state.count.fetch_add(1, Ordering::Relaxed) >= MAX_PER_RUN {
        return;
    }
    if let Some(report) = build(stage, "error", message, detail) {
        let sent = state.queue.lock().map(|q| q.send(report.clone()).is_ok()).unwrap_or(false);
        if !sent {
            keep_for_later(&report);
        }
    }
}

/// Report a problem and wait (up to `wait`) for it to arrive, for when the app is about to stop.
/// Returns whether it arrived.
pub fn error_now(stage: &str, message: &str, detail: &str, wait: Duration) -> bool {
    log(format!("problem in {stage}: {message}"));
    if !enabled() {
        return false;
    }
    let Some(report) = build(stage, "error", message, detail) else { return false };
    keep_for_later(&report);
    let arrived = post(&report, wait);
    if arrived {
        forget(&report);
    }
    arrived
}

/// "Send a test report" in Settings: says plainly whether reports get through from here.
pub fn send_test() -> Result<String, String> {
    if STATE.get().is_none() {
        return Err("error reports didn't start".into());
    }
    let report = build("test", "test", "Test report, sent from Settings.", "").ok_or("error reports didn't start")?;
    match post_with_answer(&report, Duration::from_secs(20)) {
        Ok(_) => Ok("The test report arrived. Error reports work from this computer.".into()),
        Err(e) => Err(format!("The test report didn't get through: {e}")),
    }
}

/// "Tell the developer" in Help: the person's own words, plus what the app did recently.
pub fn send_feedback(words: &str) -> Result<(), String> {
    let report = build("help: tell the developer", "user", words.trim(), "").ok_or("error reports didn't start")?;
    post_with_answer(&report, Duration::from_secs(20)).map(|_| ())
}

fn agent(wait: Duration) -> ureq::Agent {
    use ureq::tls::{RootCerts, TlsConfig};
    ureq::Agent::config_builder()
        .tls_config(TlsConfig::builder().root_certs(RootCerts::PlatformVerifier).build())
        .timeout_global(Some(wait))
        .http_status_as_error(false)
        .user_agent(concat!("EasySoundCues/", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent()
}

fn post_with_answer(report: &Report, wait: Duration) -> Result<String, String> {
    let body = serde_json::to_string(report).map_err(|e| e.to_string())?;
    let mut response = agent(wait)
        .post(ENDPOINT)
        .header("content-type", "application/json")
        .send(body.as_str())
        .map_err(|e| format!("couldn't reach the report server ({e})"))?;
    let status = response.status();
    let text = response.body_mut().read_to_string().unwrap_or_default();
    if status.is_success() { Ok(text) } else { Err(format!("the report server answered {status}: {text}")) }
}

fn post(report: &Report, wait: Duration) -> bool {
    post_with_answer(report, wait).is_ok()
}

fn file_for(report: &Report) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    report.stage.hash(&mut h);
    report.message.hash(&mut h);
    report.time.hash(&mut h);
    unsent_dir().join(format!("{}-{:016x}.json", report.time, h.finish()))
}

fn keep_for_later(report: &Report) {
    let dir = unsent_dir();
    if std::fs::create_dir_all(&dir).is_ok()
        && let Ok(json) = serde_json::to_string(report)
    {
        let _ = std::fs::write(file_for(report), json);
    }
}

fn forget(report: &Report) {
    let _ = std::fs::remove_file(file_for(report));
}

/// Send what earlier runs couldn't (they crashed, or were offline), oldest first.
fn send_unsent() {
    if !enabled() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(unsent_dir()) else { return };
    let mut files: Vec<PathBuf> =
        entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    files.sort();
    for path in files.into_iter().take(30) {
        let old = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > KEEP_UNSENT);
        let body = std::fs::read_to_string(&path).unwrap_or_default();
        if old || body.is_empty() {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        let sent = agent(Duration::from_secs(20))
            .post(ENDPOINT)
            .header("content-type", "application/json")
            .send(body.as_str())
            .is_ok_and(|r| r.status().is_success() || r.status().is_client_error());
        // A server that says the report itself is bad will say so every time: drop it.
        if sent {
            let _ = std::fs::remove_file(&path);
        } else {
            break;
        }
    }
}

/// A crash anywhere (any thread) is written down at once, sent if it can be within a few seconds,
/// and otherwise sent next time the app starts.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "the app crashed".to_string());
        let place = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        let trace = std::backtrace::Backtrace::force_capture().to_string();
        let detail = format!("crashed at {place} on the {thread} thread\n\n{trace}");
        if enabled()
            && let Some(report) = build(&format!("crash ({thread} thread)"), "panic", &message, &detail)
        {
            keep_for_later(&report);
            // Sending on its own thread, so a crash in the network code can't hang the app.
            let copy = report.clone();
            let sender = std::thread::spawn(move || post(&copy, Duration::from_secs(4)));
            let deadline = Instant::now() + Duration::from_secs(5);
            while !sender.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
            if sender.is_finished() && sender.join().unwrap_or(false) {
                forget(&report);
            }
        }
        previous(info);
    }));
}

/// A random id for this installation, so reports from one computer can be told apart from
/// another's without saying whose computer it is.
pub fn new_install_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
    h.write_u32(std::process::id());
    let a = h.finish();
    let mut h2 = std::collections::hash_map::RandomState::new().build_hasher();
    h2.write_u64(a);
    format!("{:016x}{:016x}", a, h2.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_home_folders_are_cut_out() {
        let home = dirs::home_dir().unwrap().display().to_string();
        let text = format!("Couldn't open {home}/Documents/Easy Sound Cues/My Show/show.json");
        let clean = scrub(&text);
        assert!(!clean.contains(&home), "{clean}");
        assert!(clean.contains("~/Documents/Easy Sound Cues"), "{clean}");
        if let Ok(user) = std::env::var("USER")
            && user.len() >= 3
        {
            assert!(!scrub(&format!("hello {user}")).contains(&user));
        }
    }

    #[test]
    fn install_ids_are_random() {
        let (a, b) = (new_install_id(), new_install_id());
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }

    #[test]
    fn tests_never_send_anything() {
        assert!(!may_send());
    }
}
