//! YouTube to MP3: runs yt-dlp in the background, follows its progress, and explains problems in
//! plain words.

use crate::paths::{move_file, quiet, unique_file};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

pub struct Request {
    pub url: String,
    /// Only this part of the video, in seconds.
    pub from: Option<f64>,
    pub to: Option<f64>,
    /// The folder the MP3 ends up in.
    pub dest: PathBuf,
    /// The file's name; the video's title when empty.
    pub name: String,
}

pub struct Helpers {
    pub ytdlp: PathBuf,
    pub ffmpeg: PathBuf,
    pub deno: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    LookingUp,
    /// How far through, when yt-dlp knows.
    Downloading(Option<f32>),
    Converting,
    Done {
        path: PathBuf,
        title: String,
    },
    Failed {
        message: String,
        detail: String,
    },
}

impl Step {
    pub fn finished(&self) -> bool {
        matches!(self, Step::Done { .. } | Step::Failed { .. })
    }
}

pub struct Job {
    pub step: Step,
    pub started: Instant,
    rx: Receiver<Step>,
    cancel: Arc<AtomicBool>,
}

impl Job {
    pub fn start(req: Request, helpers: Helpers, repaint: impl Fn() + Send + 'static) -> Job {
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        std::thread::spawn(move || {
            let result = work(&req, &helpers, &tx, &stop, &repaint);
            let _ = tx.send(result);
            repaint();
        });
        Job { step: Step::LookingUp, started: Instant::now(), rx, cancel }
    }

    /// Take in whatever happened since last frame.
    pub fn poll(&mut self) {
        while let Ok(step) = self.rx.try_recv() {
            self.step = step;
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

fn scratch_dir() -> PathBuf {
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    let n = COUNT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("easy-sound-cues-{}-{n}", std::process::id()))
}

fn clock_arg(secs: f64) -> String {
    format!("{secs:.2}")
}

fn args(req: &Request, helpers: &Helpers, scratch: &Path) -> Vec<String> {
    let mut a: Vec<String> = [
        "--ignore-config",
        "--no-playlist",
        "--newline",
        "--color",
        "never",
        "--progress",
        "--progress-template",
        "download:@@dl %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s",
        "--progress-template",
        "postprocess:@@pp %(progress.status)s %(progress.postprocessor)s",
        "--format",
        "bestaudio/best",
        "--extract-audio",
        "--audio-format",
        "mp3",
        "--audio-quality",
        "2",
        "--no-mtime",
        "--retries",
        "3",
    ]
    .map(String::from)
    .to_vec();
    let ffmpeg_dir = helpers.ffmpeg.parent().unwrap_or(Path::new("."));
    a.extend(["--ffmpeg-location".into(), ffmpeg_dir.display().to_string()]);
    if let Some(deno) = &helpers.deno {
        a.extend(["--js-runtimes".into(), format!("deno:{}", deno.display())]);
    }
    if req.from.is_some() || req.to.is_some() {
        let from = clock_arg(req.from.unwrap_or(0.0));
        let to = req.to.map_or_else(|| "inf".to_string(), clock_arg);
        a.extend(["--download-sections".into(), format!("*{from}-{to}")]);
    }
    // A plain ASCII name for the download itself; the real name is applied afterwards, which
    // avoids any trouble with odd characters in titles on Windows.
    a.extend(["--print-to-file".into(), "%(title)s".into(), scratch.join("title.txt").display().to_string()]);
    a.extend(["--output".into(), scratch.join("audio.%(ext)s").display().to_string()]);
    a.extend(["--".into(), req.url.clone()]);
    a
}

/// Seconds from an ffmpeg progress line such as `size= 17KiB time=00:00:01.50 bitrate=…`.
fn ffmpeg_time(line: &str) -> Option<f64> {
    let t = line.split("time=").nth(1)?.split_whitespace().next()?;
    let mut secs = 0.0;
    for part in t.split(':') {
        secs = secs * 60.0 + part.parse::<f64>().ok()?;
    }
    Some(secs)
}

/// What a line of yt-dlp output means for the progress bar.
fn read_line(line: &str, section: Option<f64>) -> Option<Step> {
    if let Some(rest) = line.strip_prefix("@@dl ") {
        let nums: Vec<Option<f64>> = rest.split_whitespace().map(|w| w.parse().ok()).collect();
        let done = nums.first().copied().flatten();
        let total = nums.get(1).copied().flatten().or_else(|| nums.get(2).copied().flatten());
        let frac = match (done, total) {
            (Some(d), Some(t)) if t > 0.0 => Some((d / t).clamp(0.0, 1.0) as f32),
            _ => None,
        };
        return Some(Step::Downloading(frac));
    }
    if line.starts_with("@@pp started ExtractAudio") || line.starts_with("[ExtractAudio]") {
        return Some(Step::Converting);
    }
    if let (Some(len), Some(at)) = (section, ffmpeg_time(line)) {
        return Some(Step::Downloading(Some((at / len.max(0.1)).clamp(0.0, 1.0) as f32)));
    }
    if line.starts_with("[download]") {
        return Some(Step::Downloading(None));
    }
    None
}

fn work(req: &Request, helpers: &Helpers, tx: &Sender<Step>, cancel: &AtomicBool, repaint: &dyn Fn()) -> Step {
    let scratch = scratch_dir();
    let step = (|| {
        std::fs::create_dir_all(&scratch).map_err(|e| fail("Couldn't make a temporary folder.", &e.to_string()))?;
        let mut child = quiet(Command::new(&helpers.ytdlp).args(args(req, helpers, &scratch)))
            .env("PYTHONUTF8", "1")
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                fail("The YouTube downloader wouldn't start. Open “Check my computer” to fix it.", &e.to_string())
            })?;
        let (line_tx, lines) = channel::<(bool, String)>();
        let out = child.stdout.take().expect("piped");
        let err = child.stderr.take().expect("piped");
        for (is_err, stream) in [(false, Box::new(out) as Box<dyn std::io::Read + Send>), (true, Box::new(err))] {
            let line_tx = line_tx.clone();
            std::thread::spawn(move || {
                // ffmpeg ends its progress lines with \r rather than \n.
                let mut reader = BufReader::new(stream);
                let mut buf = Vec::new();
                while reader.read_until(b'\n', &mut buf).is_ok_and(|n| n > 0) {
                    for piece in String::from_utf8_lossy(&buf).split('\r') {
                        if !piece.trim().is_empty() && line_tx.send((is_err, piece.trim().to_string())).is_err() {
                            return;
                        }
                    }
                    buf.clear();
                }
            });
        }
        drop(line_tx);
        let section = match (req.from, req.to) {
            (_, Some(to)) => Some(to - req.from.unwrap_or(0.0)),
            _ => None,
        };
        let mut errors = Vec::new();
        let mut last = Step::LookingUp;
        loop {
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Step::Failed { message: "Download cancelled.".into(), detail: String::new() });
            }
            match lines.recv_timeout(Duration::from_millis(150)) {
                Ok((is_err, line)) => {
                    if line.starts_with("ERROR:") || (is_err && line.contains("ERROR")) {
                        errors.push(line.trim_start_matches("ERROR:").trim().to_string());
                    }
                    if let Some(step) = read_line(&line, section)
                        && step != last
                    {
                        last = step.clone();
                        let _ = tx.send(step);
                        repaint();
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        let status = child.wait().map_err(|e| fail("The downloader stopped unexpectedly.", &e.to_string()))?;
        let mp3 = scratch.join("audio.mp3");
        if !status.success() || !mp3.is_file() {
            let detail = errors.join("\n");
            return Err(Step::Failed { message: friendly(&detail), detail });
        }
        let title = std::fs::read_to_string(scratch.join("title.txt")).unwrap_or_default();
        let title = title.lines().next().unwrap_or("").trim().to_string();
        let name = if req.name.trim().is_empty() { title.clone() } else { req.name.trim().to_string() };
        std::fs::create_dir_all(&req.dest)
            .map_err(|e| fail("Couldn't make the folder for the sound.", &e.to_string()))?;
        let path = unique_file(&req.dest, &name, "mp3");
        move_file(&mp3, &path).map_err(|e| fail("Couldn't save the MP3.", &e.to_string()))?;
        Ok(Step::Done { path, title })
    })();
    let _ = std::fs::remove_dir_all(&scratch);
    step.unwrap_or_else(|e| e)
}

fn fail(message: &str, detail: &str) -> Step {
    Step::Failed { message: message.into(), detail: detail.into() }
}

/// yt-dlp's error, explained for someone who has never seen a terminal.
pub fn friendly(error: &str) -> String {
    let e = error.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| e.contains(w));
    if e.trim().is_empty() {
        "The download didn't work, and the downloader didn't say why. Try again, or try a different video.".into()
    } else if has(&["is not a valid url", "unsupported url"]) {
        "That doesn't look like a video link. Open the video in your web browser, copy the address from the bar at the top, and paste it here.".into()
    } else if has(&["private video"]) {
        "This video is private, so it can't be downloaded. Try a different video.".into()
    } else if has(&["confirm your age", "age-restricted", "inappropriate for some users"]) {
        "YouTube only shows this video to signed-in adults, so it can't be downloaded. Try a different video.".into()
    } else if has(&["not a bot", "sign in to confirm"]) {
        "YouTube is blocking downloads from this internet connection right now. Wait a few minutes and try again, or try another network (like a phone hotspot).".into()
    } else if has(&["live event", "is live", "premieres in", "premiere will begin"]) {
        "This is a live stream, or it hasn't started yet, so it can't be downloaded.".into()
    } else if has(&[
        "video unavailable",
        "this video is unavailable",
        "has been removed",
        "not available in your country",
        "no longer available",
    ]) {
        "This video isn't available. It may have been deleted, or blocked where you are.".into()
    } else if has(&[
        "getaddrinfo",
        "name or service not known",
        "failed to resolve",
        "nodename nor servname",
        "network is unreachable",
        "timed out",
        "connection refused",
        "no route to host",
    ]) {
        "Couldn't reach YouTube. Check that this computer is connected to the internet. (Some school and office networks block YouTube.)".into()
    } else if has(&["ffmpeg", "ffprobe"]) {
        "The MP3 maker (FFmpeg) is missing or broken. Open “Check my computer” and press “Get it”.".into()
    } else if has(&[
        "http error 403",
        "forbidden",
        "requested format is not available",
        "no video formats",
        "signature",
        "nsig",
    ]) {
        "YouTube refused the download. This usually means the downloader needs updating: press “Update the downloader”, then try again.".into()
    } else if has(&["no space left", "disk full"]) {
        "This computer's disk is full. Free up some space and try again.".into()
    } else {
        "The download didn't work. Try again, or try a different video. (The details below may help someone who knows computers.)".into()
    }
}

/// A link someone pasted, tidied up, or why it isn't one.
pub fn clean_url(input: &str) -> Result<String, String> {
    let t = input.trim().trim_matches(['"', '\'', '<', '>']);
    if t.is_empty() {
        return Err("Paste the video's link into the box first.".into());
    }
    // Pasted text with a link somewhere in it: use the link.
    let word = t.split_whitespace().find(|w| w.starts_with("http://") || w.starts_with("https://")).unwrap_or(t);
    if word.starts_with("http://") || word.starts_with("https://") {
        return Ok(word.to_string());
    }
    if !word.contains(char::is_whitespace) && word.contains('.') && word.len() > 4 {
        return Ok(format!("https://{word}"));
    }
    Err("That doesn't look like a link. It should start with https:// — copy it from the address bar at the top of your web browser.".into())
}

/// "1:30", "90", "1:02:03" or "1:30.5", into seconds. Empty means "not set".
pub fn parse_time(s: &str) -> Result<Option<f64>, String> {
    let t = s.trim();
    if t.is_empty() {
        return Ok(None);
    }
    let bad = || format!("“{t}” isn't a time. Write times like 1:30 (one minute and thirty seconds) or 90.");
    let parts: Vec<&str> = t.split(':').collect();
    if parts.len() > 3 {
        return Err(bad());
    }
    let mut secs = 0.0;
    for (i, part) in parts.iter().enumerate() {
        let v: f64 = part.trim().replace(',', ".").parse().map_err(|_| bad())?;
        if v < 0.0 || !v.is_finite() || (i > 0 && v >= 60.0) {
            return Err(bad());
        }
        secs = secs * 60.0 + v;
    }
    Ok(Some(secs))
}

/// Seconds as m:ss (with tenths when they matter).
pub fn clock(secs: f64) -> String {
    let whole = secs.max(0.0);
    let m = (whole / 60.0).floor();
    let s = whole - m * 60.0;
    if (s - s.round()).abs() < 0.05 {
        format!("{}:{:02}", m as u64, s.round() as u64)
    } else {
        format!("{}:{:04.1}", m as u64, s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_read_the_way_people_write_them() {
        assert_eq!(parse_time(""), Ok(None));
        assert_eq!(parse_time("90"), Ok(Some(90.0)));
        assert_eq!(parse_time("1:30"), Ok(Some(90.0)));
        assert_eq!(parse_time(" 1:02:03 "), Ok(Some(3723.0)));
        assert_eq!(parse_time("0:12,5"), Ok(Some(12.5)));
        assert!(parse_time("1:75").is_err());
        assert!(parse_time("soon").is_err());
        assert_eq!(clock(73.0), "1:13");
        assert_eq!(clock(12.5), "0:12.5");
    }

    #[test]
    fn links_are_tidied() {
        assert_eq!(clean_url("  https://youtu.be/abc  ").unwrap(), "https://youtu.be/abc");
        assert_eq!(clean_url("youtu.be/abc").unwrap(), "https://youtu.be/abc");
        assert_eq!(clean_url("watch this https://youtu.be/abc lol").unwrap(), "https://youtu.be/abc");
        assert!(clean_url("").is_err());
        assert!(clean_url("thunder sound effect").is_err());
    }

    #[test]
    fn progress_lines_move_the_bar() {
        assert_eq!(read_line("@@dl 1024 4096 NA", None), Some(Step::Downloading(Some(0.25))));
        assert_eq!(read_line("@@dl 1024 NA 2048", None), Some(Step::Downloading(Some(0.5))));
        assert_eq!(read_line("@@dl 1024 NA NA", None), Some(Step::Downloading(None)));
        assert_eq!(read_line("@@pp started ExtractAudio", None), Some(Step::Converting));
        assert_eq!(
            read_line("size= 17KiB time=00:00:01.50 bitrate=94.7kbits/s", Some(3.0)),
            Some(Step::Downloading(Some(0.5)))
        );
        assert_eq!(read_line("[youtube] abc: Downloading webpage", None), None);
    }

    #[test]
    fn errors_are_explained() {
        assert!(friendly("[generic] 'not a url' is not a valid URL").contains("doesn't look like a video link"));
        assert!(friendly("[youtube] xxxxxxxxxxx: This video is unavailable").contains("isn't available"));
        assert!(friendly("[youtube] abc: Sign in to confirm you’re not a bot").contains("blocking downloads"));
        assert!(friendly("Unable to download webpage: <urlopen error [Errno -3] Temporary failure in name resolution> (caused by getaddrinfo failed)").contains("internet"));
        assert!(friendly("Postprocessing: ffprobe and ffmpeg not found").contains("MP3 maker"));
        assert!(friendly("HTTP Error 403: Forbidden").contains("Update the downloader"));
    }

    /// Run by hand (`cargo test -- --ignored`): downloads two seconds of a YouTube video.
    #[test]
    #[ignore = "downloads from YouTube"]
    fn a_real_download_makes_an_mp3() {
        use crate::tools::{Check, Tool, check};
        let found = |tool| match check(tool) {
            Check::Ready(f) => f.path,
            other => panic!("{tool:?} isn't ready: {other:?}"),
        };
        let deno = match check(Tool::Deno) {
            Check::Ready(f) => Some(f.path),
            _ => None,
        };
        let dest = std::env::temp_dir().join(format!("esc-ytdl-{}", std::process::id()));
        let req = Request {
            url: "https://youtu.be/6mAKhZLh0JE".into(),
            from: Some(0.5),
            to: Some(2.5),
            dest: dest.clone(),
            name: "Bubble test".into(),
        };
        let helpers = Helpers { ytdlp: found(Tool::YtDlp), ffmpeg: found(Tool::Ffmpeg), deno };
        let mut job = Job::start(req, helpers, || {});
        let start = Instant::now();
        let mut seen = Vec::new();
        while !job.step.finished() && start.elapsed() < Duration::from_secs(180) {
            job.poll();
            if seen.last() != Some(&job.step) {
                seen.push(job.step.clone());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let Step::Done { path, title } = &job.step else { panic!("the download failed: {:?}", job.step) };
        assert_eq!(path, &dest.join("Bubble test.mp3"));
        assert!(title.to_lowercase().contains("bubble"), "title was {title}");
        let clip = crate::audio::Clip::load(path).unwrap();
        assert!((clip.seconds() - 2.0).abs() < 0.3, "asked for 2 s, got {:.2} s", clip.seconds());
        println!("steps: {seen:?}");
        std::fs::remove_dir_all(&dest).unwrap();
    }

    #[test]
    fn arguments_ask_for_an_mp3_of_just_the_part_wanted() {
        let req = Request {
            url: "https://youtu.be/x".into(),
            from: Some(30.0),
            to: None,
            dest: PathBuf::from("out"),
            name: String::new(),
        };
        let helpers =
            Helpers { ytdlp: "yt-dlp".into(), ffmpeg: PathBuf::from("/opt/ff/ffmpeg"), deno: Some("/opt/deno".into()) };
        let a = args(&req, &helpers, Path::new("/tmp/x"));
        let after = |flag: &str| a.iter().position(|x| x == flag).map(|i| a[i + 1].clone());
        assert_eq!(after("--audio-format").as_deref(), Some("mp3"));
        assert_eq!(after("--download-sections").as_deref(), Some("*30.00-inf"));
        assert_eq!(after("--ffmpeg-location").as_deref(), Some("/opt/ff"));
        assert_eq!(after("--js-runtimes").as_deref(), Some("deno:/opt/deno"));
        assert_eq!(a.last().unwrap(), "https://youtu.be/x");
        assert_eq!(a[a.len() - 2], "--", "the link must come after -- so it can never be read as an option");
    }
}
