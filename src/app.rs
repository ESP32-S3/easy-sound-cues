//! The app's state, and everything it does in answer to a button or a key.
//!
//! Drawing code never changes the show directly when the change needs the whole app (playing a
//! sound, adding one, opening a show): it pushes an [`Act`], and [`App::apply`] carries those out
//! once the frame is drawn.

use crate::audio::{Clip, Engine, Song, Voice};
use crate::paths::{self, Settings};
use crate::show::{self, Cue, Music, Placement, ScriptLine, Show};
use crate::tools::{self, Check, Found, Progress, Tool};
use crate::{reporter, updater, ytdl};
use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

/// A second GO this soon after the last one is a double tap, so it is ignored.
const GO_LOCKOUT: Duration = Duration::from_millis(450);
const TOAST_TIME: Duration = Duration::from_secs(5);
pub(crate) const SOUND_FILES: [&str; 9] = ["mp3", "wav", "m4a", "aac", "ogg", "oga", "flac", "mp4", "m4b"];
/// Voice id for previews of files that aren't in the show (cue and song ids start at 1).
pub(crate) const PREVIEW: u32 = u32::MAX;

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Tab {
    Setup,
    Run,
    YouTube,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum SetupTab {
    Sounds,
    Music,
    Script,
    Show,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Page {
    Hello,
    Checkup,
    /// Nothing is open: pick or make a show.
    Start,
    Main,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Tone {
    Info,
    Good,
    Bad,
}

/// What a YouTube download is for.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Purpose {
    Effect,
    Music,
    JustSave,
}

/// Where a finished download goes.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Target {
    /// Decided by the purpose: a new sound, new music, or just a file.
    Purpose,
    ReplaceCue(u32),
    ReplaceSong(u32),
}

#[derive(Clone, Debug)]
pub(crate) enum Act {
    Go,
    Back,
    Skip,
    SetNext(usize),
    PlayNow(u32),
    StopCue(u32),
    StopAll,
    FadeAll,
    FadeMusic,
    StartSong(u32),
    Restart,
    Volume(f32),
    MusicVolume(f32),
    Select(u32),
    SelectSong(u32),
    AddFiles,
    AddEmpty,
    MoveCue(u32, i32),
    RemoveCue(u32),
    ChooseFile(u32),
    Listen(u32),
    AddMusicFiles,
    RemoveSong(u32),
    ChooseSongFile(u32),
    ListenSong(u32),
    YouTubeFor(Target, Purpose),
    Undo,
    NewShow(String),
    OpenShow(PathBuf),
    OpenPractice,
    ImportShow,
    ExportShow,
    RevealShow,
    ShowList,
    CheckComputer,
    Install(Vec<Tool>),
    Reconnect,
    TestSound,
    Tab(Tab),
    Page(Page),
    Confirm(String, String, Box<Act>),
    EditCue(u32),
    EditSong(u32),
    Pick(PickFor),
    UseFile(PickFor, PathBuf),
    PreviewFile(PathBuf),
    CheckUpdate,
    UpdateNow,
    RestartApp,
    TestReport,
    Feedback,
}

/// What a file chosen from "Sounds you already have" is for.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum PickFor {
    NewCue,
    NewSong,
    Cue(u32),
    Song(u32),
}

/// One file in "Sounds you already have".
pub(crate) struct Have {
    pub path: PathBuf,
    pub place: &'static str,
    pub size: u64,
    pub in_show: bool,
}

pub(crate) struct Picker {
    pub purpose: PickFor,
    pub files: Vec<Have>,
}

/// A sound or song that can't play because its file is missing.
pub(crate) struct Need {
    pub number: Option<usize>,
    pub pick: PickFor,
    pub name: String,
    pub why: String,
    pub source: String,
}

#[derive(Clone, Debug)]
pub(crate) enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    Available(String),
    Dismissed,
    Installing(Progress),
    Ready(PathBuf),
    Failed(String),
}

enum UpdateMsg {
    Latest(Result<String, String>, bool),
    Progress(Progress),
    Installed(Result<PathBuf, String>),
}

pub(crate) enum Loaded<T> {
    Loading,
    Ready(Arc<T>),
    Failed(String),
}

// By hand, because a derive would wrongly require the sound itself to be Clone.
impl<T> Clone for Loaded<T> {
    fn clone(&self) -> Self {
        match self {
            Loaded::Loading => Loaded::Loading,
            Loaded::Ready(x) => Loaded::Ready(x.clone()),
            Loaded::Failed(e) => Loaded::Failed(e.clone()),
        }
    }
}

/// Decoded sound effects and measured songs, loaded in the background.
pub(crate) struct Sounds {
    clips: HashMap<PathBuf, Loaded<Clip>>,
    songs: HashMap<PathBuf, Loaded<Song>>,
    tx: Sender<(PathBuf, Result<Clip, String>)>,
    rx: Receiver<(PathBuf, Result<Clip, String>)>,
    song_tx: Sender<(PathBuf, Result<Song, String>)>,
    song_rx: Receiver<(PathBuf, Result<Song, String>)>,
}

impl Sounds {
    fn new() -> Self {
        let (tx, rx) = channel();
        let (song_tx, song_rx) = channel();
        Self { clips: HashMap::new(), songs: HashMap::new(), tx, rx, song_tx, song_rx }
    }

    pub fn clip(&self, path: &Path) -> Loaded<Clip> {
        self.clips.get(path).cloned().unwrap_or(Loaded::Loading)
    }

    pub fn song(&self, path: &Path) -> Loaded<Song> {
        self.songs.get(path).cloned().unwrap_or(Loaded::Loading)
    }

    fn want_clip(&mut self, path: PathBuf, ctx: &egui::Context) {
        if self.clips.contains_key(&path) {
            return;
        }
        self.clips.insert(path.clone(), Loaded::Loading);
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let clip = Clip::load(&path);
            let _ = tx.send((path, clip));
            ctx.request_repaint();
        });
    }

    fn want_song(&mut self, path: PathBuf, ctx: &egui::Context) {
        if self.songs.contains_key(&path) {
            return;
        }
        self.songs.insert(path.clone(), Loaded::Loading);
        let (tx, ctx) = (self.song_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let song = Song::load(&path);
            let _ = tx.send((path, song));
            ctx.request_repaint();
        });
    }

    fn poll(&mut self) {
        while let Ok((path, clip)) = self.rx.try_recv() {
            if let Err(e) = &clip {
                reporter::error("loading a sound", e, "");
            }
            self.clips.insert(path, clip.map(Arc::new).map_or_else(Loaded::Failed, Loaded::Ready));
        }
        while let Ok((path, song)) = self.song_rx.try_recv() {
            if let Err(e) = &song {
                reporter::error("loading music", e, "");
            }
            self.songs.insert(path, song.map(Arc::new).map_or_else(Loaded::Failed, Loaded::Ready));
        }
    }

    /// Load it again next time (the file was replaced).
    fn forget(&mut self, path: &Path) {
        self.clips.remove(path);
        self.songs.remove(path);
    }
}

#[derive(Clone, Debug)]
pub(crate) enum HelperState {
    Unknown,
    Checking,
    Ready(Found),
    Missing,
    Broken(String),
    Installing(Progress),
    Failed(String),
}

enum HelperMsg {
    Checked(Tool, Check),
    Progress(Tool, Progress),
    Installed(Tool, Result<Found, String>),
    Online(bool),
    Folder(Result<(), String>),
}

/// The helper programs for YouTube downloads, and the other things "Check my computer" looks at.
pub(crate) struct Helpers {
    pub state: [HelperState; 3],
    pub online: Option<bool>,
    pub folder: Option<Result<(), String>>,
    pub installing: bool,
    /// Tools being updated without anyone asking, which shouldn't complain if it fails.
    quiet: HashSet<Tool>,
    auto_updated: bool,
    tx: Sender<HelperMsg>,
    rx: Receiver<HelperMsg>,
}

fn slot(tool: Tool) -> usize {
    tools::ALL.iter().position(|&t| t == tool).expect("every tool is listed")
}

impl Helpers {
    fn new() -> Self {
        let (tx, rx) = channel();
        Self {
            state: [HelperState::Unknown, HelperState::Unknown, HelperState::Unknown],
            online: None,
            folder: None,
            installing: false,
            quiet: HashSet::new(),
            auto_updated: false,
            tx,
            rx,
        }
    }

    pub fn get(&self, tool: Tool) -> &HelperState {
        &self.state[slot(tool)]
    }

    pub fn found(&self, tool: Tool) -> Option<&Found> {
        match self.get(tool) {
            HelperState::Ready(f) => Some(f),
            _ => None,
        }
    }

    /// yt-dlp and FFmpeg both work, which is all a download needs.
    pub fn youtube_ready(&self) -> bool {
        self.found(Tool::YtDlp).is_some() && self.found(Tool::Ffmpeg).is_some()
    }

    pub fn checking(&self) -> bool {
        self.state.iter().any(|s| matches!(s, HelperState::Checking)) || self.online.is_none() || self.folder.is_none()
    }

    /// Helpers that aren't working, needed ones first.
    pub fn missing(&self) -> Vec<Tool> {
        tools::ALL
            .into_iter()
            .filter(|&t| matches!(self.get(t), HelperState::Missing | HelperState::Broken(_) | HelperState::Failed(_)))
            .collect()
    }

    pub fn check(&mut self, ctx: &egui::Context) {
        if self.installing {
            return;
        }
        for tool in tools::ALL {
            self.state[slot(tool)] = HelperState::Checking;
            let (tx, ctx) = (self.tx.clone(), ctx.clone());
            std::thread::spawn(move || {
                let _ = tx.send(HelperMsg::Checked(tool, tools::check(tool)));
                ctx.request_repaint();
            });
        }
        self.online = None;
        self.folder = None;
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(HelperMsg::Folder(folder_works()));
            ctx.request_repaint();
            let _ = tx.send(HelperMsg::Online(tools::online()));
            ctx.request_repaint();
        });
    }

    pub fn install(&mut self, list: Vec<Tool>, ctx: &egui::Context) {
        if self.installing || list.is_empty() {
            return;
        }
        self.installing = true;
        for &tool in &list {
            self.state[slot(tool)] = HelperState::Installing(Progress { stage: "Waiting", done: 0, total: None });
        }
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            for tool in list {
                let progress_tx = tx.clone();
                let progress_ctx = ctx.clone();
                let mut report = move |p: Progress| {
                    let _ = progress_tx.send(HelperMsg::Progress(tool, p));
                    progress_ctx.request_repaint();
                };
                let result = tools::install(tool, &mut report);
                let _ = tx.send(HelperMsg::Installed(tool, result));
                ctx.request_repaint();
            }
        });
    }

    /// Returns messages worth telling the person about.
    fn poll(&mut self, ctx: &egui::Context) -> Vec<(String, Tone)> {
        let mut news = Vec::new();
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                HelperMsg::Checked(tool, check) => {
                    match &check {
                        Check::Ready(f) => reporter::log(format!(
                            "{}: version {} ({})",
                            tool.program(),
                            f.version,
                            if f.ours { "ours" } else { "on the computer" }
                        )),
                        Check::Missing => reporter::log(format!("{}: missing", tool.program())),
                        Check::Broken(e) => reporter::error(&format!("check computer: {}", tool.program()), e, ""),
                    }
                    self.state[slot(tool)] = match check {
                        Check::Ready(found) => HelperState::Ready(found),
                        Check::Missing => HelperState::Missing,
                        Check::Broken(e) => HelperState::Broken(e),
                    };
                }
                HelperMsg::Progress(tool, p) => {
                    if !self.quiet.contains(&tool) {
                        self.state[slot(tool)] = HelperState::Installing(p);
                    }
                }
                HelperMsg::Installed(tool, result) => {
                    let quiet = self.quiet.remove(&tool);
                    match &result {
                        Ok(found) => reporter::log(format!("{}: installed version {}", tool.program(), found.version)),
                        Err(e) => {
                            let how = if quiet { "an automatic update" } else { "asked for by the person" };
                            reporter::error(&format!("downloading a helper: {}", tool.program()), e, how);
                        }
                    }
                    match result {
                        Ok(found) => {
                            if quiet {
                                news.push((format!("Updated the {} to the newest version.", tool.title()), Tone::Info));
                            } else {
                                news.push((format!("The {} is ready.", tool.title()), Tone::Good));
                            }
                            self.state[slot(tool)] = HelperState::Ready(found);
                        }
                        Err(e) if !quiet => {
                            self.state[slot(tool)] = HelperState::Failed(format!("Couldn't get it: {e}"));
                        }
                        Err(_) => {}
                    }
                    let busy = self.state.iter().any(|s| matches!(s, HelperState::Installing(_)));
                    self.installing = busy || !self.quiet.is_empty();
                }
                HelperMsg::Online(online) => {
                    reporter::log(if online {
                        "internet: YouTube reachable"
                    } else {
                        "internet: YouTube not reachable"
                    });
                    self.online = Some(online);
                }
                HelperMsg::Folder(result) => {
                    if let Err(e) = &result {
                        reporter::error("check computer: the shows folder", e, "");
                    }
                    self.folder = Some(result);
                }
            }
        }
        // Keep the app's own yt-dlp fresh: YouTube changes often and old copies stop working.
        if !self.auto_updated
            && !self.installing
            && self.online == Some(true)
            && self.found(Tool::YtDlp).is_some_and(|f| f.ours && f.age_days().is_some_and(|d| d > 21))
        {
            self.auto_updated = true;
            self.quiet.insert(Tool::YtDlp);
            self.installing = true;
            let (tx, ctx) = (self.tx.clone(), ctx.clone());
            std::thread::spawn(move || {
                let result = tools::install(Tool::YtDlp, &mut |_| {});
                let _ = tx.send(HelperMsg::Installed(Tool::YtDlp, result));
                ctx.request_repaint();
            });
        }
        news
    }
}

fn folder_works() -> Result<(), String> {
    let dir = paths::shows_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let probe = dir.join(".write-test");
    std::fs::write(&probe, b"ok").map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(probe);
    Ok(())
}

/// The YouTube page's form and its download.
pub(crate) struct YouTube {
    pub url: String,
    pub from: String,
    pub to: String,
    pub name: String,
    pub purpose: Purpose,
    pub target: Target,
    pub job: Option<ytdl::Job>,
    pub problem: Option<String>,
    /// The current download's failure has been reported.
    pub reported: bool,
    /// Downloads finished this session: where they went and what became of them.
    pub done: Vec<(PathBuf, String)>,
}

/// The show that's open, with what was last saved so changes can be saved automatically.
pub(crate) struct Open {
    pub dir: PathBuf,
    pub show: Show,
    saved: Show,
    changed_at: Option<Instant>,
    /// The script, read into lines, and where each cue sits in it.
    pub lines: Vec<ScriptLine>,
    pub places: Vec<Placement>,
    parsed: u64,
}

impl Open {
    fn new(dir: PathBuf, show: Show) -> Self {
        let mut open =
            Self { dir, saved: show.clone(), show, changed_at: None, lines: Vec::new(), places: Vec::new(), parsed: 0 };
        open.reparse();
        open
    }

    fn reparse(&mut self) {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.show.script.hash(&mut h);
        for c in &self.show.cues {
            c.line.hash(&mut h);
        }
        let key = h.finish();
        if key != self.parsed {
            self.lines = show::parse_script(&self.show.script);
            self.places = show::place_cues(&self.lines, &self.show.cues);
            self.parsed = key;
        }
    }

    pub fn path(&self, file: &str) -> PathBuf {
        Show::path_of(&self.dir, file)
    }

    fn save(&mut self) -> Result<(), String> {
        self.show.save(&self.dir)?;
        self.saved = self.show.clone();
        self.changed_at = None;
        Ok(())
    }
}

pub struct App {
    pub(crate) settings: Settings,
    pub(crate) engine: Engine,
    pub(crate) sounds: Sounds,
    pub(crate) open: Option<Open>,
    pub(crate) page: Page,
    pub(crate) tab: Tab,
    pub(crate) setup_tab: SetupTab,
    /// Index of the cue GO plays next; the number of cues once all have gone.
    pub(crate) next: usize,
    pub(crate) played: HashSet<u32>,
    pub(crate) voices: Vec<Voice>,
    pub(crate) music: Vec<Voice>,
    last_go: Option<Instant>,
    pub(crate) follow: bool,
    pub(crate) scroll_to_next: bool,
    pub(crate) show_list: bool,
    pub(crate) selected: Option<u32>,
    pub(crate) selected_song: Option<u32>,
    undo: Vec<Show>,
    pub(crate) helpers: Helpers,
    pub(crate) yt: YouTube,
    pub(crate) tour: Option<usize>,
    pub(crate) help_open: bool,
    pub(crate) settings_open: bool,
    pub(crate) shows_open: bool,
    pub(crate) new_show: Option<String>,
    pub(crate) confirm: Option<(String, String, Act)>,
    pub(crate) toast: Option<(Instant, String, Tone)>,
    pub(crate) show_list_cache: Vec<(String, PathBuf)>,
    pub(crate) speakers: Vec<String>,
    test_chime: Arc<Clip>,
    pub(crate) picker: Option<Picker>,
    pending_preview: Option<PathBuf>,
    pub(crate) fix_open: bool,
    pub(crate) update: UpdateState,
    /// The speaker problem last reported, so each new one is reported once.
    audio_reported: Option<String>,
    /// What the person typed in Help's "Tell the app's maker".
    pub(crate) feedback: String,
    /// Results of sending a test report or feedback, from the thread that sent it.
    sent_tx: Sender<(String, Tone)>,
    sent_rx: Receiver<(String, Tone)>,
    update_tx: Sender<UpdateMsg>,
    update_rx: Receiver<UpdateMsg>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let settings = Settings::load();
        let engine = Engine::open(settings.volume, settings.speaker.as_deref());
        Self::with_engine(&cc.egui_ctx, settings, engine)
    }

    /// The app around a given audio engine; tests and screenshots pass a silent one.
    pub fn with_engine(ctx: &egui::Context, settings: Settings, engine: Engine) -> Self {
        let (update_tx, update_rx) = channel();
        let (sent_tx, sent_rx) = channel();
        crate::style::load_fonts(ctx);
        crate::style::apply(ctx, settings.light, settings.text_size);
        let mut app = Self {
            page: if settings.welcomed { Page::Start } else { Page::Hello },
            settings,
            engine,
            sounds: Sounds::new(),
            open: None,
            tab: Tab::Setup,
            setup_tab: SetupTab::Sounds,
            next: 0,
            played: HashSet::new(),
            voices: Vec::new(),
            music: Vec::new(),
            last_go: None,
            follow: true,
            scroll_to_next: false,
            show_list: false,
            selected: None,
            selected_song: None,
            undo: Vec::new(),
            helpers: Helpers::new(),
            yt: YouTube {
                url: String::new(),
                from: String::new(),
                to: String::new(),
                name: String::new(),
                purpose: Purpose::Effect,
                target: Target::Purpose,
                job: None,
                problem: None,
                reported: false,
                done: Vec::new(),
            },
            tour: None,
            help_open: false,
            settings_open: false,
            shows_open: false,
            new_show: None,
            confirm: None,
            toast: None,
            show_list_cache: Vec::new(),
            speakers: Vec::new(),
            test_chime: Arc::new(Clip::mono(crate::synth::test_chime())),
            picker: None,
            pending_preview: None,
            fix_open: false,
            update: UpdateState::Idle,
            audio_reported: None,
            feedback: String::new(),
            sent_tx,
            sent_rx,
            update_tx,
            update_rx,
        };
        // Once a day at most, and never from tests or screenshots.
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let due = now.saturating_sub(app.settings.last_update_check) > 20 * 3600;
        if app.settings.check_updates && due && std::env::var_os("EASY_SOUND_CUES_HOME").is_none() {
            app.check_update(false, ctx);
        }
        match &app.engine.problem {
            Some(problem) => {
                let detail = format!(
                    "{} speakers listed; {} chosen in Settings",
                    crate::audio::speakers().len(),
                    if app.settings.speaker.is_some() { "specific speakers" } else { "the default speakers" }
                );
                reporter::error("start: opening the speakers", problem, &detail);
                app.audio_reported = Some(problem.clone());
            }
            None => reporter::log(format!("speakers open at {} Hz", app.engine.sample_rate)),
        }
        app.helpers.check(ctx);
        if app.settings.welcomed
            && let Some(dir) = app.settings.last_show.clone().filter(|d| show::is_show_dir(d))
        {
            app.open_show(dir, ctx);
        }
        app.refresh_show_list();
        app
    }

    pub(crate) fn notify(&mut self, text: impl Into<String>, tone: Tone) {
        let text = text.into();
        // Every problem the person is shown also goes to the app's maker.
        if tone == Tone::Bad {
            reporter::error(&self.where_now(), &text, "");
        }
        self.toast = Some((Instant::now(), text, tone));
    }

    /// Which part of the app the person is in, for reports.
    fn where_now(&self) -> String {
        match (self.page, self.tab) {
            (Page::Hello, _) => "welcome".into(),
            (Page::Checkup, _) => "check computer".into(),
            (Page::Start, _) => "start page".into(),
            (Page::Main, Tab::Run) => "run the show".into(),
            (Page::Main, Tab::Setup) => format!("set up: {:?}", self.setup_tab).to_lowercase(),
            (Page::Main, Tab::YouTube) => "youtube to mp3".into(),
        }
    }

    pub(crate) fn toast_showing(&self) -> bool {
        self.toast
            .as_ref()
            .is_some_and(|(at, _, tone)| at.elapsed() < if *tone == Tone::Bad { TOAST_TIME * 2 } else { TOAST_TIME })
    }

    pub(crate) fn refresh_show_list(&mut self) {
        self.show_list_cache = show::list_shows(&paths::shows_dir());
    }

    pub(crate) fn cues(&self) -> &[Cue] {
        self.open.as_ref().map_or(&[], |o| &o.show.cues)
    }

    pub(crate) fn clip_of(&self, cue: &Cue) -> Option<Loaded<Clip>> {
        let open = self.open.as_ref()?;
        (!cue.file.is_empty()).then(|| self.sounds.clip(&open.path(&cue.file)))
    }

    pub(crate) fn song_of(&self, song: &show::Song) -> Option<Loaded<Song>> {
        let open = self.open.as_ref()?;
        (!song.file.is_empty()).then(|| self.sounds.song(&open.path(&song.file)))
    }

    /// The sound effect sounding for cue `id`, ignoring ones on their way out.
    pub(crate) fn voice(&self, id: u32) -> Option<&Voice> {
        self.voices.iter().rev().find(|v| v.id == id && !v.fading)
    }

    /// The song playing now (not the one fading out under a crossfade).
    pub(crate) fn current_song(&self) -> Option<&Voice> {
        self.music.iter().rev().find(|v| !v.fading)
    }

    pub(crate) fn anything_playing(&self) -> bool {
        self.voices.iter().any(|v| !v.fading) || self.music.iter().any(|v| !v.fading)
    }

    fn checkpoint(&mut self) {
        if let Some(open) = &self.open {
            self.undo.push(open.show.clone());
            if self.undo.len() > 40 {
                self.undo.remove(0);
            }
        }
    }

    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    // ── opening and saving shows ────────────────────────────────────────────────────────────

    pub(crate) fn open_show(&mut self, dir: PathBuf, ctx: &egui::Context) {
        match Show::load(&dir) {
            Ok(show) => {
                self.save_now();
                self.stop_everything(0.05);
                self.selected = show.cues.first().map(|c| c.id);
                self.selected_song = show.music.first().map(|s| s.id);
                let title = show.title.clone();
                let has_sounds = !show.cues.is_empty();
                self.open = Some(Open::new(dir.clone(), show));
                self.undo.clear();
                self.reset_run();
                self.settings.last_show = Some(dir);
                self.settings.save();
                self.page = Page::Main;
                self.tab = if has_sounds { Tab::Run } else { Tab::Setup };
                self.setup_tab = SetupTab::Sounds;
                self.load_sounds(ctx);
                let open = self
                    .open
                    .as_ref()
                    .map(|o| (o.show.cues.len(), o.show.music.len(), !o.show.script.trim().is_empty()));
                if let Some((cues, songs, script)) = open {
                    reporter::log(format!("opened a show: {cues} sounds, {songs} songs, script: {script}"));
                }
                self.notify(format!("Opened “{title}”."), Tone::Info);
            }
            Err(e) => self.notify(e, Tone::Bad),
        }
    }

    /// Make sure every sound in the show is loaded or loading.
    fn load_sounds(&mut self, ctx: &egui::Context) {
        let Some(open) = &self.open else { return };
        let clips: Vec<PathBuf> =
            open.show.cues.iter().filter(|c| !c.file.is_empty()).map(|c| open.path(&c.file)).collect();
        let songs: Vec<PathBuf> =
            open.show.music.iter().filter(|s| !s.file.is_empty()).map(|s| open.path(&s.file)).collect();
        for path in clips {
            self.sounds.want_clip(path, ctx);
        }
        for path in songs {
            self.sounds.want_song(path, ctx);
        }
    }

    pub(crate) fn save_now(&mut self) {
        let Some(open) = &mut self.open else { return };
        if open.show != open.saved
            && let Err(e) = open.save()
        {
            self.notify(e, Tone::Bad);
        }
    }

    fn autosave(&mut self) {
        let Some(open) = &mut self.open else { return };
        if open.show == open.saved {
            open.changed_at = None;
            return;
        }
        let since = *open.changed_at.get_or_insert_with(Instant::now);
        if since.elapsed() > Duration::from_millis(700)
            && let Err(e) = open.save()
        {
            open.changed_at = Some(Instant::now() + Duration::from_secs(5));
            self.notify(e, Tone::Bad);
        }
    }

    fn new_show(&mut self, title: &str, ctx: &egui::Context) {
        let title = if title.trim().is_empty() { "My Show" } else { title.trim() };
        let dir = paths::unique_dir(&paths::shows_dir(), title);
        match Show::new(title).save(&dir) {
            Ok(()) => {
                self.open_show(dir, ctx);
                self.tab = Tab::Setup;
                self.notify(format!("Made a new show called “{title}”. Now add its sounds."), Tone::Good);
            }
            Err(e) => self.notify(e, Tone::Bad),
        }
    }

    fn open_practice(&mut self, ctx: &egui::Context) {
        let dir = paths::shows_dir().join(crate::synth::PRACTICE_TITLE);
        if (!show::is_show_dir(&dir) || !dir.join(show::SOUNDS_DIR).join("doorbell.wav").is_file())
            && let Err(e) = crate::synth::practice_show(&dir)
        {
            self.notify(format!("Couldn't make the practice show: {e}"), Tone::Bad);
            return;
        }
        self.open_show(dir, ctx);
        self.tab = Tab::Run;
    }

    /// Bring in a show from a folder or a .zip someone gave you.
    fn import_show(&mut self, ctx: &egui::Context) {
        let Some(picked) = rfd::FileDialog::new()
            .set_title("Choose the show: its .zip file, or the show.json file inside its folder")
            .add_filter("Show (.zip, or show.json in a show's folder)", &["zip", "json"])
            .pick_file()
        else {
            return;
        };
        let root = paths::shows_dir();
        let _ = std::fs::create_dir_all(&root);
        let picked_dir = if picked.file_name().is_some_and(|n| n == show::SHOW_FILE) {
            picked.parent().map(Path::to_path_buf)
        } else {
            None
        };
        let result = match picked_dir {
            Some(dir) if dir.starts_with(&root) => Ok(dir),
            Some(dir) => {
                // A show on a USB stick would break when the stick comes out, so keep a copy.
                let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Show".into());
                let dest = paths::unique_dir(&root, &name);
                show::copy_dir(&dir, &dest).map(|_| dest).map_err(|e| format!("Couldn't copy the show: {e}"))
            }
            None => show::import_zip(&picked, &root),
        };
        match result {
            Ok(dir) => {
                self.open_show(dir, ctx);
                self.refresh_show_list();
            }
            Err(e) => self.notify(e, Tone::Bad),
        }
    }

    fn export_show(&mut self) {
        self.save_now();
        let Some(open) = &self.open else { return };
        let name = format!("{}.zip", paths::safe_name(&open.show.title));
        let Some(dest) = rfd::FileDialog::new()
            .set_title("Save a copy of the show")
            .set_file_name(name)
            .add_filter("Show", &["zip"])
            .save_file()
        else {
            return;
        };
        let dest = if dest.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")) {
            dest
        } else {
            dest.with_extension("zip")
        };
        match show::export_zip(&open.dir, &dest) {
            Ok(()) => self.notify(
                format!(
                    "Saved “{}”. On the other computer, open Easy Sound Cues and choose “Open a show someone gave me”.",
                    paths::friendly(&dest)
                ),
                Tone::Good,
            ),
            Err(e) => self.notify(e, Tone::Bad),
        }
    }

    // ── running the show ────────────────────────────────────────────────────────────────────

    pub(crate) fn reset_run(&mut self) {
        self.next = 0;
        self.played.clear();
        self.last_go = None;
        self.scroll_to_next = true;
    }

    fn set_next(&mut self, i: usize) {
        self.next = i.min(self.cues().len());
        self.scroll_to_next = self.follow;
    }

    fn go(&mut self) {
        reporter::log(format!("GO: sound {} of {}", self.next + 1, self.cues().len()));
        if self.last_go.is_some_and(|t| t.elapsed() < GO_LOCKOUT) {
            self.notify(
                "GO was pressed twice quickly, so the second press was ignored (no sound was skipped).",
                Tone::Info,
            );
            return;
        }
        let Some(cue) = self.cues().get(self.next).cloned() else {
            self.notify("That was the last sound. Press “Start again” to run the show from the top.", Tone::Info);
            return;
        };
        self.last_go = Some(Instant::now());
        self.fire(&cue);
        let next = self.next + 1;
        self.set_next(next);
    }

    /// Play a cue's sound and do its music change.
    fn fire(&mut self, cue: &Cue) {
        self.played.insert(cue.id);
        if !cue.file.is_empty() {
            for v in self.voices.iter_mut().filter(|v| v.id == cue.id) {
                v.fade_out(0.03);
            }
            match self.start_clip(cue) {
                Ok(voice) => self.voices.push(voice),
                Err(e) => self.notify(e, Tone::Bad),
            }
        }
        match cue.music {
            Music::Keep => {}
            Music::Start { song } => self.start_song(song),
            Music::Fade => {
                let secs = self.open.as_ref().map_or(2.0, |o| o.show.crossfade.max(0.5));
                self.stop_music(secs);
            }
            Music::Stop => self.stop_music(0.05),
        }
    }

    fn start_clip(&self, cue: &Cue) -> Result<Voice, String> {
        let label = format!("“{}”", cue.name);
        match self.clip_of(cue) {
            Some(Loaded::Ready(clip)) => {
                let range = clip.range(cue.skip_silence, cue.start, cue.end);
                let gain = cue.volume * if cue.even_out { clip.even_gain } else { 1.0 };
                self.engine.play(cue.id, &clip, range, gain, cue.fade_out)
            }
            Some(Loaded::Loading) => Err(format!("{label} is still loading. Try again in a moment.")),
            Some(Loaded::Failed(e)) => Err(format!("{label} couldn't play: {e}")),
            None => Err(format!("{label} has no sound file yet.")),
        }
    }

    pub(crate) fn song_gain(&self, song: &show::Song, loaded: &Song) -> f32 {
        let music = self.open.as_ref().map_or(0.4, |o| o.show.music_volume);
        music * song.volume * if song.even_out { loaded.even_gain } else { 1.0 }
    }

    /// Start a song, crossfading out whatever is already playing.
    fn start_song(&mut self, id: u32) {
        let Some(open) = &self.open else { return };
        let Some(song) = open.show.song(id).cloned() else {
            self.notify("That music was removed from the show.", Tone::Bad);
            return;
        };
        let crossfade = open.show.crossfade;
        if self.current_song().is_some_and(|v| v.id == id) {
            return;
        }
        let fading_in = self.current_song().is_some();
        let result = match self.song_of(&song) {
            Some(Loaded::Ready(loaded)) => {
                let gain = self.song_gain(&song, &loaded);
                let range = loaded.range(song.start, song.end);
                let fade = if fading_in { crossfade } else { 0.0 };
                self.engine.play_song(song.id, &loaded, range, song.repeat, gain, fade)
            }
            Some(Loaded::Loading) => Err(format!("“{}” is still loading. Try again in a moment.", song.name)),
            Some(Loaded::Failed(e)) => Err(format!("“{}” couldn't play: {e}", song.name)),
            None => Err(format!("“{}” has no music file yet.", song.name)),
        };
        match result {
            Ok(voice) => {
                self.stop_music(crossfade.max(0.05));
                self.music.push(voice);
            }
            Err(e) => self.notify(e, Tone::Bad),
        }
    }

    fn stop_music(&mut self, secs: f32) {
        for v in &mut self.music {
            v.fade_out(secs);
        }
    }

    fn stop_everything(&mut self, secs: f32) {
        for v in &mut self.voices {
            v.fade_out(secs);
        }
        self.stop_music(secs);
    }

    // ── changing the show ───────────────────────────────────────────────────────────────────

    /// Copy picked files into the show's sounds folder, so the show still works if the originals
    /// are moved, and so the whole show can be copied to another computer.
    fn bring_in(&mut self, files: &[PathBuf]) -> Vec<(String, String, String)> {
        let Some(dir) = self.open.as_ref().map(|o| o.dir.clone()) else { return Vec::new() };
        let sounds = dir.join(show::SOUNDS_DIR);
        let _ = std::fs::create_dir_all(&sounds);
        let mut out = Vec::new();
        for file in files {
            let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let ext = file.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| "mp3".into());
            let stored = if file.starts_with(&dir) {
                show::stored_path(&dir, file)
            } else {
                let dest = paths::unique_file(&sounds, &stem, &ext);
                if let Err(e) = std::fs::copy(file, &dest) {
                    self.notify(format!("Couldn't copy {}: {e}", file.display()), Tone::Bad);
                    continue;
                }
                show::stored_path(&dir, &dest)
            };
            let name = stem.replace(['_', '-'], " ").split_whitespace().collect::<Vec<_>>().join(" ");
            let name = if name.is_empty() { "New sound".to_string() } else { name };
            out.push((name, stored, file.display().to_string()));
        }
        out
    }

    fn pick_sound_files(&self, title: &str, many: bool) -> Vec<PathBuf> {
        let dialog = rfd::FileDialog::new().set_title(title).add_filter("Sound files", &SOUND_FILES);
        if many { dialog.pick_files().unwrap_or_default() } else { dialog.pick_file().into_iter().collect() }
    }

    /// Where a new cue goes: after the selected one, or at the end.
    fn insert_at(&self) -> usize {
        let cues = self.cues();
        self.selected.and_then(|id| cues.iter().position(|c| c.id == id)).map_or(cues.len(), |i| i + 1)
    }

    pub(crate) fn add_cue(&mut self, name: String, file: String, source: String) -> Option<u32> {
        let at = self.insert_at();
        self.checkpoint();
        let open = self.open.as_mut()?;
        let id = open.show.new_id();
        open.show.cues.insert(at.min(open.show.cues.len()), Cue { id, name, file, source, ..Cue::default() });
        self.selected = Some(id);
        Some(id)
    }

    pub(crate) fn add_song(&mut self, name: String, file: String, source: String) -> Option<u32> {
        self.checkpoint();
        let open = self.open.as_mut()?;
        let id = open.show.new_id();
        open.show.music.push(show::Song { id, name, file, source, ..show::Song::default() });
        self.selected_song = Some(id);
        Some(id)
    }

    /// A finished YouTube download, put where it was meant to go.
    pub(crate) fn use_download(&mut self, path: PathBuf, title: String, ctx: &egui::Context) {
        let source = self.yt.url.trim().to_string();
        let stored = self.open.as_ref().map(|o| show::stored_path(&o.dir, &path));
        let name = if self.yt.name.trim().is_empty() { tidy_title(&title) } else { self.yt.name.trim().to_string() };
        let what = match (self.yt.target, self.yt.purpose, stored) {
            (Target::ReplaceCue(id), _, Some(stored)) => {
                self.checkpoint();
                if let Some(c) = self.open.as_mut().and_then(|o| o.show.cues.iter_mut().find(|c| c.id == id)) {
                    c.file = stored;
                    c.source = source;
                    c.start = 0.0;
                    c.end = 0.0;
                }
                self.selected = Some(id);
                "It's now the sound for that cue.".to_string()
            }
            (Target::ReplaceSong(id), _, Some(stored)) => {
                self.checkpoint();
                if let Some(s) = self.open.as_mut().and_then(|o| o.show.music.iter_mut().find(|s| s.id == id)) {
                    s.file = stored;
                    s.source = source;
                    s.start = 0.0;
                    s.end = 0.0;
                }
                self.selected_song = Some(id);
                "It's now that music's file.".to_string()
            }
            (_, Purpose::Effect, Some(stored)) => {
                self.add_cue(name.clone(), stored, source);
                format!("Added “{name}” to your show's sounds. Go to “Set up” to say when it plays.")
            }
            (_, Purpose::Music, Some(stored)) => {
                self.add_song(name.clone(), stored, source);
                format!("Added “{name}” to your show's music.")
            }
            _ => "Saved the MP3.".to_string(),
        };
        self.sounds.forget(&path);
        self.load_sounds(ctx);
        self.yt.done.push((path, what.clone()));
        self.yt.target = Target::Purpose;
        self.yt.name.clear();
        self.notify(format!("Downloaded “{}”. {what}", tidy_title(&title)), Tone::Good);
    }

    // ── sounds you already have, and sounds that are missing ────────────────────────────────

    /// Audio files worth offering: this show's own, ones downloaded with the app, the computer's
    /// Downloads folder, and other shows' sounds.
    fn scan_files(&self) -> Vec<Have> {
        let in_show: HashSet<PathBuf> = self
            .open
            .as_ref()
            .map(|o| {
                let files = o.show.cues.iter().map(|c| &c.file).chain(o.show.music.iter().map(|s| &s.file));
                files.filter(|f| !f.is_empty()).map(|f| o.path(f)).collect()
            })
            .unwrap_or_default();
        let this_show = self.open.as_ref().map(|o| o.dir.clone());
        let mut places: Vec<(&'static str, PathBuf)> = Vec::new();
        if let Some(dir) = &this_show {
            places.push(("In this show's folder", dir.join(show::SOUNDS_DIR)));
        }
        places.push(("Downloaded with Easy Sound Cues", paths::downloads_dir()));
        if let Some(dl) = dirs::download_dir() {
            places.push(("In your Downloads folder", dl));
        }
        for (_, dir) in show::list_shows(&paths::shows_dir()) {
            if Some(&dir) != this_show.as_ref() {
                places.push(("In your other shows", dir.join(show::SOUNDS_DIR)));
            }
        }
        let mut out = Vec::new();
        for (place, dir) in places {
            let mut found: Vec<(std::time::SystemTime, Have)> = std::fs::read_dir(&dir)
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|e| {
                    let path = e.path();
                    // Audio only: .mp4 files in a Downloads folder are nearly always videos.
                    let ext = path.extension()?.to_string_lossy().to_lowercase();
                    if !["mp3", "wav", "m4a", "aac", "ogg", "oga", "flac"].contains(&ext.as_str()) {
                        return None;
                    }
                    let meta = e.metadata().ok()?;
                    let used = in_show.contains(&path);
                    let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                    Some((modified, Have { path, place, size: meta.len(), in_show: used }))
                })
                .collect();
            found.sort_by_key(|f| std::cmp::Reverse(f.0));
            out.extend(found.into_iter().take(150).map(|f| f.1));
        }
        out
    }

    /// Put a file the person picked where it was meant to go.
    fn use_file(&mut self, purpose: PickFor, file: PathBuf, ctx: &egui::Context) {
        let Some((name, stored, source)) = self.bring_in(std::slice::from_ref(&file)).into_iter().next() else {
            return;
        };
        match purpose {
            PickFor::NewCue => {
                self.add_cue(name.clone(), stored, source);
                self.notify(format!("Added “{name}”. Now type the words it waits for."), Tone::Good);
            }
            PickFor::NewSong => {
                self.add_song(name.clone(), stored, source);
                self.notify(
                    format!("Added “{name}” to the music. Pick which sound starts it in the Sounds list."),
                    Tone::Good,
                );
            }
            PickFor::Cue(id) => {
                self.checkpoint();
                let named = self.open.as_mut().and_then(|o| o.show.cues.iter_mut().find(|c| c.id == id)).map(|c| {
                    c.file = stored;
                    c.source = source;
                    c.start = 0.0;
                    c.end = 0.0;
                    c.name.clone()
                });
                if let Some(name) = named {
                    self.notify(format!("“{name}” has its sound now."), Tone::Good);
                }
            }
            PickFor::Song(id) => {
                self.checkpoint();
                let named = self.open.as_mut().and_then(|o| o.show.music.iter_mut().find(|s| s.id == id)).map(|s| {
                    s.file = stored;
                    s.source = source;
                    s.start = 0.0;
                    s.end = 0.0;
                    s.name.clone()
                });
                if let Some(name) = named {
                    self.notify(format!("“{name}” has its music file now."), Tone::Good);
                }
            }
        }
        self.load_sounds(ctx);
    }

    /// Everything in the show that can't play yet because its file is missing or broken.
    pub(crate) fn needs(&self) -> Vec<Need> {
        let Some(open) = &self.open else { return Vec::new() };
        let mut out = Vec::new();
        for (i, cue) in open.show.cues.iter().enumerate() {
            let why = match self.clip_of(cue) {
                None if cue.music == Music::Keep => "It doesn't have a sound file yet.".to_string(),
                Some(Loaded::Failed(e)) => e,
                _ => continue,
            };
            out.push(Need {
                number: Some(i + 1),
                pick: PickFor::Cue(cue.id),
                name: cue.name.clone(),
                why,
                source: cue.source.clone(),
            });
        }
        for song in &open.show.music {
            let why = match self.song_of(song) {
                None => "It doesn't have a music file yet.".to_string(),
                Some(Loaded::Failed(e)) => e,
                _ => continue,
            };
            out.push(Need {
                number: None,
                pick: PickFor::Song(song.id),
                name: song.name.clone(),
                why,
                source: song.source.clone(),
            });
        }
        out
    }

    // ── updates ─────────────────────────────────────────────────────────────────────────────

    pub(crate) fn check_update(&mut self, asked: bool, ctx: &egui::Context) {
        if matches!(self.update, UpdateState::Checking | UpdateState::Installing(_) | UpdateState::Ready(_)) {
            return;
        }
        self.update = UpdateState::Checking;
        let (tx, ctx) = (self.update_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(UpdateMsg::Latest(updater::latest_version(), asked));
            ctx.request_repaint();
        });
    }

    fn install_update(&mut self, ctx: &egui::Context) {
        let UpdateState::Available(version) = self.update.clone() else { return };
        self.update = UpdateState::Installing(Progress { stage: "Starting", done: 0, total: None });
        let (tx, ctx) = (self.update_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let progress_tx = tx.clone();
            let progress_ctx = ctx.clone();
            let mut report = move |p: Progress| {
                let _ = progress_tx.send(UpdateMsg::Progress(p));
                progress_ctx.request_repaint();
            };
            let _ = tx.send(UpdateMsg::Installed(updater::install(&version, &mut report)));
            ctx.request_repaint();
        });
    }

    fn poll_update(&mut self) {
        while let Ok(msg) = self.update_rx.try_recv() {
            match msg {
                UpdateMsg::Latest(Ok(version), asked) => {
                    self.settings.last_update_check =
                        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
                    self.settings.save();
                    self.update = if updater::is_newer(&version, updater::VERSION) {
                        UpdateState::Available(version)
                    } else if asked {
                        UpdateState::UpToDate
                    } else {
                        UpdateState::Idle
                    };
                }
                UpdateMsg::Latest(Err(e), asked) => {
                    reporter::error("checking for an update", &e, if asked { "asked for" } else { "automatic" });
                    self.update = if asked {
                        UpdateState::Failed(format!("Couldn't check for updates: {e}."))
                    } else {
                        UpdateState::Idle
                    };
                }
                UpdateMsg::Progress(p) => self.update = UpdateState::Installing(p),
                UpdateMsg::Installed(Ok(exe)) => self.update = UpdateState::Ready(exe),
                UpdateMsg::Installed(Err(e)) => {
                    reporter::error("installing an update", &e, "");
                    self.update = UpdateState::Failed(format!("The update didn't work: {e}."))
                }
            }
        }
    }

    // ── doing what was asked ────────────────────────────────────────────────────────────────

    pub(crate) fn apply(&mut self, act: Act, ctx: &egui::Context) {
        match act {
            Act::Go => self.go(),
            Act::Back => self.set_next(self.next.saturating_sub(1)),
            Act::Skip => self.set_next(self.next + 1),
            Act::SetNext(i) => self.set_next(i),
            Act::PlayNow(id) => {
                if let Some(cue) = self.cues().iter().find(|c| c.id == id).cloned() {
                    self.fire(&cue);
                }
            }
            Act::Listen(id) => {
                // Just a preview: the music and the show's place stay as they are.
                if let Some(cue) = self.cues().iter().find(|c| c.id == id).cloned() {
                    for v in self.voices.iter_mut().filter(|v| v.id == id) {
                        v.fade_out(0.03);
                    }
                    match self.start_clip(&cue) {
                        Ok(voice) => self.voices.push(voice),
                        Err(e) => self.notify(e, Tone::Bad),
                    }
                }
            }
            Act::StopCue(id) => {
                for v in self.voices.iter_mut().filter(|v| v.id == id) {
                    v.fade_out(0.2);
                }
                for v in self.music.iter_mut().filter(|v| v.id == id) {
                    v.fade_out(0.5);
                }
            }
            Act::StopAll => self.stop_everything(0.05),
            Act::FadeAll => {
                let secs = self.open.as_ref().map_or(2.0, |o| o.show.fade_all);
                self.stop_everything(secs);
            }
            Act::FadeMusic => {
                let secs = self.open.as_ref().map_or(2.0, |o| o.show.crossfade.max(0.5));
                self.stop_music(secs);
            }
            Act::StartSong(id) | Act::ListenSong(id) => self.start_song(id),
            Act::Restart => {
                self.stop_everything(0.05);
                self.reset_run();
                self.notify("Back to the start. Sound 1 is next.", Tone::Info);
            }
            Act::Volume(v) => {
                self.settings.volume = v;
                self.engine.set_master(v);
            }
            Act::MusicVolume(v) => {
                if let Some(open) = &mut self.open {
                    open.show.music_volume = v;
                }
                let gains: Vec<(usize, f32)> = self
                    .music
                    .iter()
                    .enumerate()
                    .filter_map(|(i, voice)| {
                        let song = self.open.as_ref()?.show.song(voice.id)?.clone();
                        let Some(Loaded::Ready(loaded)) = self.song_of(&song) else { return None };
                        Some((i, self.song_gain(&song, &loaded)))
                    })
                    .collect();
                for (i, gain) in gains {
                    self.music[i].set_level(gain);
                }
            }
            Act::Select(id) => self.selected = Some(id),
            Act::SelectSong(id) => self.selected_song = Some(id),
            Act::AddFiles => {
                let files = self.pick_sound_files("Choose sound files to add", true);
                let added = self.bring_in(&files);
                let count = added.len();
                for (name, stored, source) in added {
                    self.add_cue(name, stored, source);
                }
                if count > 0 {
                    self.load_sounds(ctx);
                    let what = if count == 1 { "1 sound".to_string() } else { format!("{count} sounds") };
                    self.notify(format!("Added {what}. Now type the line that each one waits for."), Tone::Good);
                }
            }
            Act::AddEmpty => {
                self.add_cue("New sound".into(), String::new(), String::new());
            }
            Act::MoveCue(id, by) => {
                self.checkpoint();
                if let Some(open) = &mut self.open
                    && let Some(i) = open.show.cue_index(id)
                {
                    let j = (i as i32 + by).clamp(0, open.show.cues.len() as i32 - 1) as usize;
                    let cue = open.show.cues.remove(i);
                    open.show.cues.insert(j, cue);
                }
            }
            Act::RemoveCue(id) => {
                self.checkpoint();
                if let Some(open) = &mut self.open
                    && let Some(i) = open.show.cue_index(id)
                {
                    let cue = open.show.cues.remove(i);
                    self.selected = open.show.cues.get(i.min(open.show.cues.len().saturating_sub(1))).map(|c| c.id);
                    if self.next > i {
                        self.next -= 1;
                    }
                    self.notify(format!("Removed “{}”. Changed your mind? Press Undo.", cue.name), Tone::Info);
                }
            }
            Act::ChooseFile(id) => {
                let files = self.pick_sound_files("Choose the sound file", false);
                if let Some((_, stored, source)) = self.bring_in(&files).into_iter().next() {
                    self.checkpoint();
                    if let Some(c) = self.open.as_mut().and_then(|o| o.show.cues.iter_mut().find(|c| c.id == id)) {
                        c.file = stored;
                        c.source = source;
                        c.start = 0.0;
                        c.end = 0.0;
                    }
                    self.load_sounds(ctx);
                }
            }
            Act::AddMusicFiles => {
                let files = self.pick_sound_files("Choose music files to add", true);
                let added = self.bring_in(&files);
                let count = added.len();
                for (name, stored, source) in added {
                    self.add_song(name, stored, source);
                }
                if count > 0 {
                    self.load_sounds(ctx);
                    self.notify("Added the music. Now pick which sound starts it, in the Sounds list.", Tone::Good);
                }
            }
            Act::RemoveSong(id) => {
                self.checkpoint();
                if let Some(open) = &mut self.open
                    && let Some(i) = open.show.music.iter().position(|s| s.id == id)
                {
                    let song = open.show.music.remove(i);
                    for c in &mut open.show.cues {
                        if c.music == (Music::Start { song: id }) {
                            c.music = Music::Keep;
                        }
                    }
                    self.selected_song = open.show.music.first().map(|s| s.id);
                    self.notify(format!("Removed “{}”. Changed your mind? Press Undo.", song.name), Tone::Info);
                }
                for v in self.music.iter_mut().filter(|v| v.id == id) {
                    v.fade_out(0.5);
                }
            }
            Act::ChooseSongFile(id) => {
                let files = self.pick_sound_files("Choose the music file", false);
                if let Some((_, stored, source)) = self.bring_in(&files).into_iter().next() {
                    self.checkpoint();
                    if let Some(s) = self.open.as_mut().and_then(|o| o.show.music.iter_mut().find(|s| s.id == id)) {
                        s.file = stored;
                        s.source = source;
                        s.start = 0.0;
                        s.end = 0.0;
                    }
                    self.load_sounds(ctx);
                }
            }
            Act::YouTubeFor(target, purpose) => {
                self.yt.target = target;
                self.yt.purpose = purpose;
                let existing = match target {
                    Target::ReplaceCue(id) => self.cues().iter().find(|c| c.id == id).map(|c| c.source.clone()),
                    Target::ReplaceSong(id) => {
                        self.open.as_ref().and_then(|o| o.show.song(id)).map(|s| s.source.clone())
                    }
                    Target::Purpose => None,
                };
                if let Some(src) = existing.filter(|s| s.starts_with("http")) {
                    self.yt.url = src;
                }
                self.tab = Tab::YouTube;
            }
            Act::Undo => {
                if let (Some(prev), Some(open)) = (self.undo.pop(), self.open.as_mut()) {
                    open.show = prev;
                    self.load_sounds(ctx);
                    self.notify("Undone.", Tone::Info);
                }
            }
            Act::NewShow(title) => self.new_show(&title, ctx),
            Act::OpenShow(dir) => self.open_show(dir, ctx),
            Act::OpenPractice => self.open_practice(ctx),
            Act::ImportShow => self.import_show(ctx),
            Act::ExportShow => self.export_show(),
            Act::RevealShow => {
                if let Some(open) = &self.open {
                    paths::reveal(&open.dir);
                }
            }
            Act::ShowList => {
                self.refresh_show_list();
                self.shows_open = true;
            }
            Act::CheckComputer => {
                self.speakers = crate::audio::speakers();
                self.helpers.check(ctx);
            }
            Act::Install(list) => self.helpers.install(list, ctx),
            Act::Reconnect => {
                self.voices.clear();
                self.music.clear();
                self.engine.connect(self.settings.speaker.as_deref());
                self.speakers = crate::audio::speakers();
                match &self.engine.problem {
                    Some(p) => self.notify(p.clone(), Tone::Bad),
                    None => self.notify(format!("Sound is working, through “{}”.", self.engine.device), Tone::Good),
                }
            }
            Act::TestSound => {
                let clip = self.test_chime.clone();
                match self.engine.play(0, &clip, (0, clip.frames()), clip.even_gain, 0.0) {
                    Ok(v) => self.voices.push(v),
                    Err(e) => self.notify(e, Tone::Bad),
                }
            }
            Act::Tab(tab) => {
                reporter::log(format!("page: {tab:?}"));
                self.tab = tab;
                if tab == Tab::Run {
                    self.scroll_to_next = true;
                }
            }
            Act::Page(page) => {
                if page == Page::Start {
                    self.refresh_show_list();
                }
                self.page = page;
            }
            Act::Confirm(question, yes, act) => self.confirm = Some((question, yes, *act)),
            Act::EditCue(id) => {
                self.page = Page::Main;
                self.tab = Tab::Setup;
                self.setup_tab = SetupTab::Sounds;
                self.selected = Some(id);
            }
            Act::EditSong(id) => {
                self.page = Page::Main;
                self.tab = Tab::Setup;
                self.setup_tab = SetupTab::Music;
                self.selected_song = Some(id);
            }
            Act::Pick(purpose) => {
                let files = self.scan_files();
                self.picker = Some(Picker { purpose, files });
            }
            Act::UseFile(purpose, file) => {
                self.picker = None;
                self.use_file(purpose, file, ctx);
            }
            Act::PreviewFile(path) => {
                for v in self.voices.iter_mut().filter(|v| v.id == PREVIEW) {
                    v.fade_out(0.05);
                }
                self.sounds.want_clip(path.clone(), ctx);
                self.pending_preview = Some(path);
            }
            Act::CheckUpdate => {
                self.update = UpdateState::Idle;
                self.check_update(true, ctx);
            }
            Act::UpdateNow => self.install_update(ctx),
            Act::TestReport => {
                self.notify("Sending a test report…", Tone::Info);
                let (tx, ctx) = (self.sent_tx.clone(), ctx.clone());
                std::thread::spawn(move || {
                    // Shown as plain news either way: a failed test is the answer, not a new problem.
                    let text = reporter::send_test().unwrap_or_else(|e| e);
                    let _ = tx.send((text, Tone::Info));
                    ctx.request_repaint();
                });
            }
            Act::Feedback => {
                let words = std::mem::take(&mut self.feedback);
                let (tx, ctx) = (self.sent_tx.clone(), ctx.clone());
                std::thread::spawn(move || {
                    let answer = match reporter::send_feedback(&words) {
                        Ok(()) => ("Thanks! Your message was sent to the app's maker.".to_string(), Tone::Good),
                        Err(e) => (format!("Your message couldn't be sent: {e}"), Tone::Info),
                    };
                    let _ = tx.send(answer);
                    ctx.request_repaint();
                });
            }
            Act::RestartApp => {
                if let UpdateState::Ready(exe) = self.update.clone() {
                    self.save_now();
                    self.settings.save();
                    updater::restart(&exe);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    /// Everything that happens between frames: background work landing, sounds ending, saving.
    fn tick(&mut self, ctx: &egui::Context) {
        while let Ok((text, tone)) = self.sent_rx.try_recv() {
            self.notify(text, tone);
        }
        self.sounds.poll();
        self.poll_update();
        // A preview asked for before its file had loaded plays as soon as it's ready.
        if let Some(path) = self.pending_preview.clone() {
            match self.sounds.clip(&path) {
                Loaded::Ready(clip) => {
                    self.pending_preview = None;
                    match self.engine.play(PREVIEW, &clip, clip.range(true, 0.0, 0.0), clip.even_gain, 0.0) {
                        Ok(v) => self.voices.push(v),
                        Err(e) => self.notify(e, Tone::Bad),
                    }
                }
                Loaded::Failed(e) => {
                    self.pending_preview = None;
                    self.notify(e, Tone::Bad);
                }
                Loaded::Loading => {}
            }
        }
        for (msg, tone) in self.helpers.poll(ctx) {
            self.notify(msg, tone);
        }
        self.voices.retain(|v| !v.ended());
        self.music.retain(|v| !v.ended());
        self.engine.poll();
        if self.engine.problem != self.audio_reported {
            if let Some(problem) = &self.engine.problem {
                reporter::error("speakers", problem, &format!("{} speakers listed", crate::audio::speakers().len()));
            }
            self.audio_reported = self.engine.problem.clone();
        }
        if let Some(job) = &mut self.yt.job {
            job.poll();
            match job.step.clone() {
                ytdl::Step::Done { path, title } => {
                    reporter::log("youtube download finished");
                    self.yt.job = None;
                    self.use_download(path, title, ctx);
                }
                ytdl::Step::Failed { message, detail } if !self.yt.reported => {
                    self.yt.reported = true;
                    if message != "Download cancelled." {
                        let what = format!(
                            "link: {}\nfrom: {:?} to: {:?}\n\nyt-dlp said:\n{detail}",
                            self.yt.url, self.yt.from, self.yt.to
                        );
                        reporter::error("youtube to mp3", &message, &what);
                    }
                }
                _ => {}
            }
        }
        if let Some(open) = &mut self.open {
            open.reparse();
            self.next = self.next.min(open.show.cues.len());
        }
        self.autosave();
    }

    fn keys(&self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        use egui::Key;
        if ctx.egui_wants_keyboard_input() || self.confirm.is_some() || self.tour.is_some() || self.new_show.is_some() {
            return;
        }
        ctx.input_mut(|i| {
            if self.page == Page::Main && self.tab == Tab::Run {
                // Enter and PageDown back up Space; presentation clickers send PageDown and PageUp.
                if take_key(i, Key::Space) | take_key(i, Key::Enter) | take_key(i, Key::PageDown) {
                    acts.push(Act::Go);
                }
                if take_key(i, Key::PageUp) | take_key(i, Key::ArrowLeft) {
                    acts.push(Act::Back);
                }
                if take_key(i, Key::ArrowRight) {
                    acts.push(Act::Skip);
                }
                if take_key(i, Key::F) {
                    acts.push(Act::FadeAll);
                }
                if take_key(i, Key::M) {
                    acts.push(Act::FadeMusic);
                }
            }
            if take_key(i, Key::Escape) {
                acts.push(Act::StopAll);
            }
            if i.consume_key(egui::Modifiers::COMMAND, Key::Z) {
                acts.push(Act::Undo);
            }
        });
    }

    /// Set the app up for one of the README screenshots.
    #[doc(hidden)]
    pub fn stage(&mut self, scene: &str, ctx: &egui::Context) {
        match scene {
            "hello" => self.page = Page::Hello,
            "checkup" => self.page = Page::Checkup,
            "start" => {
                self.page = Page::Start;
                self.refresh_show_list();
            }
            "tour" => {
                self.page = Page::Start;
                self.tour = Some(2);
            }
            "practice" => self.open_practice(ctx),
            "go-twice" => {
                self.next = 2;
                let ids: Vec<u32> = self.cues().iter().take(2).map(|c| c.id).collect();
                self.played.extend(ids);
            }
            "setup" => {
                self.tab = Tab::Setup;
                self.selected = self.cues().get(1).map(|c| c.id);
            }
            "music" => {
                self.tab = Tab::Setup;
                self.setup_tab = SetupTab::Music;
            }
            "script" => {
                self.tab = Tab::Setup;
                self.setup_tab = SetupTab::Script;
            }
            "youtube" => {
                self.tab = Tab::YouTube;
                self.yt.url = "https://www.youtube.com/watch?v=…".into();
            }
            "help" => self.help_open = true,
            "settings" => self.settings_open = true,
            "picker" => self.apply(Act::Pick(PickFor::NewCue), ctx),
            "missing" => {
                self.add_cue("Applause".into(), "sounds/applause.mp3".into(), "https://youtu.be/…".into());
                self.load_sounds(ctx);
                self.settle();
                self.fix_open = true;
            }
            "update" => self.update = UpdateState::Available("2.1.0".into()),
            other if other.starts_with("open:") => self.open_show(PathBuf::from(&other[5..]), ctx),
            _ => {}
        }
        self.sounds.poll();
    }

    /// Wait for background loading to finish (for screenshots).
    #[doc(hidden)]
    pub fn settle(&mut self) {
        for _ in 0..300 {
            self.sounds.poll();
            let loading = self.sounds.clips.values().any(|l| matches!(l, Loaded::Loading))
                || self.sounds.songs.values().any(|l| matches!(l, Loaded::Loading));
            if !loading {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// A video title cut down to something that fits on a button.
pub(crate) fn tidy_title(title: &str) -> String {
    let name = paths::safe_name(title);
    if name.chars().count() > 48 { name.chars().take(48).collect::<String>().trim_end().to_string() } else { name }
}

/// True if `key` was just pressed (not auto-repeat) with no modifiers. Its events are removed so a
/// focused button or checkbox can't also react to it.
fn take_key(input: &mut egui::InputState, key: egui::Key) -> bool {
    let mut hit = false;
    input.events.retain(|e| match e {
        egui::Event::Key { key: k, pressed, repeat, modifiers, .. } if *k == key && modifiers.is_none() => {
            hit |= *pressed && !*repeat;
            false
        }
        _ => true,
    });
    hit
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.tick(&ctx);
        let mut acts = Vec::new();
        self.keys(&ctx, &mut acts);
        for act in acts.drain(..) {
            self.apply(act, &ctx);
        }

        self.draw(ui, &mut acts);

        for act in acts {
            self.apply(act, &ctx);
        }
        // Full frame rate only while something moves fast; a looping song just needs its clock.
        let busy = self.voices.iter().any(|v| !v.ended())
            || self.music.iter().any(|v| v.fading)
            || self.toast_showing()
            || self.yt.job.is_some()
            || self.helpers.installing;
        if busy {
            ctx.request_repaint();
        } else if !self.music.is_empty() || self.helpers.checking() {
            ctx.request_repaint_after(Duration::from_millis(100));
        } else {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_now();
        self.settings.save();
        if let Some(job) = &self.yt.job {
            job.cancel();
        }
    }
}
