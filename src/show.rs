//! A show: its sounds in order, its background music, and (if someone pasted it in) the script.
//!
//! Each show is a folder holding `show.json` and a `sounds` folder with copies of its audio, so a
//! whole show can be copied to a USB stick or zipped up and opened on another computer.

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const SHOW_FILE: &str = "show.json";
pub const SOUNDS_DIR: &str = "sounds";
const BACKUP_FILE: &str = "show.backup.json";

/// What a sound does to the background music when it plays.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum Music {
    /// Leave the music as it is.
    #[default]
    Keep,
    /// Start this song, crossfading out whatever is playing.
    Start { song: u32 },
    /// Fade the music out gently.
    Fade,
    /// Cut the music dead.
    Stop,
}

/// One sound in the show, played when someone presses GO.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Cue {
    pub id: u32,
    pub name: String,
    /// The audio file, relative to the show folder. Empty for a cue that only changes the music.
    pub file: String,
    /// Who says the line that the sound follows.
    pub who: String,
    /// The words to listen for: GO is pressed when they are said.
    pub line: String,
    /// Anything else the person running the sound should know.
    pub note: String,
    /// 1.0 is normal.
    pub volume: f32,
    /// Bring it to the same loudness as the other sounds before `volume` applies.
    pub even_out: bool,
    pub skip_silence: bool,
    /// Seconds to cut from the start.
    pub start: f32,
    /// Where to stop, in seconds from the start of the file; 0 plays to the end.
    pub end: f32,
    /// Seconds of fade at the end.
    pub fade_out: f32,
    pub music: Music,
    /// Where it came from (a web address or the original file name), for credits.
    pub source: String,
}

impl Default for Cue {
    fn default() -> Self {
        Self {
            id: 0,
            name: String::new(),
            file: String::new(),
            who: String::new(),
            line: String::new(),
            note: String::new(),
            volume: 1.0,
            even_out: true,
            skip_silence: true,
            start: 0.0,
            end: 0.0,
            fade_out: 0.0,
            music: Music::Keep,
            source: String::new(),
        }
    }
}

/// A piece of background music. It plays under the sounds and repeats until something stops it.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Song {
    pub id: u32,
    pub name: String,
    pub file: String,
    pub volume: f32,
    pub even_out: bool,
    /// The part of the file to play, in seconds; `end` 0 means the end of the file.
    pub start: f32,
    pub end: f32,
    /// Go back to `start` at the end instead of stopping.
    pub repeat: bool,
    pub source: String,
}

impl Default for Song {
    fn default() -> Self {
        Self {
            id: 0,
            name: String::new(),
            file: String::new(),
            volume: 1.0,
            even_out: true,
            start: 0.0,
            end: 0.0,
            repeat: true,
            source: String::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Show {
    pub format: u32,
    pub title: String,
    pub cues: Vec<Cue>,
    pub music: Vec<Song>,
    pub script: String,
    /// Level of the background music under everything else, 0..=1.
    pub music_volume: f32,
    /// Seconds one song takes to fade into the next.
    pub crossfade: f32,
    /// Seconds for "Fade out everything".
    pub fade_all: f32,
    pub next_id: u32,
}

impl Default for Show {
    fn default() -> Self {
        Self {
            format: 1,
            title: "My Show".into(),
            cues: Vec::new(),
            music: Vec::new(),
            script: String::new(),
            music_volume: 0.4,
            crossfade: 2.0,
            fade_all: 2.0,
            next_id: 1,
        }
    }
}

impl Show {
    pub fn new(title: &str) -> Self {
        Self { title: title.trim().to_string(), ..Self::default() }
    }

    pub fn new_id(&mut self) -> u32 {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        id
    }

    pub fn cue_index(&self, id: u32) -> Option<usize> {
        self.cues.iter().position(|c| c.id == id)
    }

    pub fn song(&self, id: u32) -> Option<&Song> {
        self.music.iter().find(|s| s.id == id)
    }

    pub fn song_name(&self, id: u32) -> String {
        self.song(id).map_or_else(|| "(music that was removed)".into(), |s| s.name.clone())
    }

    /// The song playing just before cue `i`, going by the cues before it.
    pub fn song_before(&self, i: usize) -> Option<u32> {
        self.cues[..i.min(self.cues.len())]
            .iter()
            .rev()
            .find_map(|c| match c.music {
                Music::Start { song } => Some(Some(song)),
                Music::Stop | Music::Fade => Some(None),
                Music::Keep => None,
            })
            .flatten()
    }

    /// Ids must be unique and below `next_id`; a hand-edited file might break that.
    fn repair(&mut self) {
        let mut seen = std::collections::HashSet::new();
        let mut max = 0;
        for id in self.cues.iter().map(|c| c.id).chain(self.music.iter().map(|s| s.id)) {
            max = max.max(id);
        }
        self.next_id = self.next_id.max(max + 1).max(1);
        for i in 0..self.cues.len() {
            if self.cues[i].id == 0 || !seen.insert(self.cues[i].id) {
                let id = self.new_id();
                self.cues[i].id = id;
                seen.insert(id);
            }
        }
        for i in 0..self.music.len() {
            if self.music[i].id == 0 || !seen.insert(self.music[i].id) {
                let id = self.new_id();
                // Cues pointing at the old id meant the first song that had it.
                self.music[i].id = id;
                seen.insert(id);
            }
        }
        for c in &mut self.cues {
            c.volume = c.volume.clamp(0.0, 4.0);
            c.start = c.start.max(0.0);
            c.end = c.end.max(0.0);
            c.fade_out = c.fade_out.max(0.0);
        }
        for s in &mut self.music {
            s.volume = s.volume.clamp(0.0, 4.0);
            s.start = s.start.max(0.0);
            s.end = s.end.max(0.0);
        }
        self.music_volume = self.music_volume.clamp(0.0, 1.0);
        self.crossfade = self.crossfade.clamp(0.0, 20.0);
        self.fade_all = self.fade_all.clamp(0.1, 20.0);
    }

    pub fn load(dir: &Path) -> Result<Self, String> {
        let path = dir.join(SHOW_FILE);
        let text = std::fs::read_to_string(&path).map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
        let mut show: Show = serde_json::from_str(&text)
            .map_err(|e| format!("The show file is damaged ({e}). Your previous version may be in {BACKUP_FILE}."))?;
        show.repair();
        // Keep the version from before this session, in case the edits go badly.
        let _ = std::fs::write(dir.join(BACKUP_FILE), &text);
        Ok(show)
    }

    /// Write `show.json` so that a crash halfway through can't leave a half-written file.
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir.join(SOUNDS_DIR)).map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let tmp = dir.join("show.json.saving");
        std::fs::write(&tmp, json).map_err(|e| format!("Couldn't save the show: {e}"))?;
        std::fs::rename(&tmp, dir.join(SHOW_FILE)).map_err(|e| format!("Couldn't save the show: {e}"))
    }

    /// The full path of a sound file named in the show.
    pub fn path_of(dir: &Path, file: &str) -> PathBuf {
        let p = Path::new(file);
        if p.is_absolute() { p.to_path_buf() } else { dir.join(p) }
    }
}

/// A file path as stored in show.json: relative to the show, with forward slashes on every system.
pub fn stored_path(dir: &Path, file: &Path) -> String {
    match file.strip_prefix(dir) {
        Ok(rel) => rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"),
        Err(_) => file.display().to_string(),
    }
}

pub fn is_show_dir(dir: &Path) -> bool {
    dir.join(SHOW_FILE).is_file()
}

/// Every show in the shows folder, most recently changed first.
pub fn list_shows(root: &Path) -> Vec<(String, PathBuf)> {
    let mut shows: Vec<(std::time::SystemTime, String, PathBuf)> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| is_show_dir(p))
        .map(|p| {
            let changed =
                std::fs::metadata(p.join(SHOW_FILE)).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            let title = std::fs::read_to_string(p.join(SHOW_FILE))
                .ok()
                .and_then(|t| serde_json::from_str::<Show>(&t).ok())
                .map(|s| s.title)
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| p.file_name().unwrap_or_default().to_string_lossy().into_owned());
            (changed, title, p)
        })
        .collect();
    shows.sort_by_key(|s| std::cmp::Reverse(s.0));
    shows.into_iter().map(|(_, t, p)| (t, p)).collect()
}

/// Copy a whole folder, e.g. a show from a USB stick into the shows folder.
pub fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Pack a show folder into one .zip file, ready to email or put on a USB stick.
pub fn export_zip(dir: &Path, zip_path: &Path) -> Result<(), String> {
    let file = std::fs::File::create(zip_path).map_err(|e| format!("Couldn't create the file: {e}"))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut stack = vec![dir.to_path_buf()];
    while let Some(folder) = stack.pop() {
        for entry in std::fs::read_dir(&folder).map_err(|e| e.to_string())?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = stored_path(dir, &path);
            if name.ends_with(".saving") {
                continue;
            }
            let mut data = Vec::new();
            std::fs::File::open(&path).and_then(|mut f| f.read_to_end(&mut data)).map_err(|e| e.to_string())?;
            zip.start_file(name, options).map_err(|e| e.to_string())?;
            zip.write_all(&data).map_err(|e| e.to_string())?;
        }
    }
    zip.finish().map_err(|e| format!("Couldn't finish the file: {e}"))?;
    Ok(())
}

/// Unpack a show .zip into a new folder inside `parent`; returns that folder.
pub fn import_zip(zip_path: &Path, parent: &Path) -> Result<PathBuf, String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("Couldn't open the file: {e}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|_| "That file isn't a show (it's not a .zip file).".to_string())?;
    // Shows zipped by hand often have everything inside one top folder.
    let names: Vec<String> = archive.file_names().filter_map(|n| n.ok().map(|n| n.into_owned())).collect();
    let show_entry = names
        .iter()
        .filter(|n| n.rsplit('/').next() == Some(SHOW_FILE))
        .min_by_key(|n| n.matches('/').count())
        .ok_or("That .zip file doesn't have a show inside it (no show.json).")?;
    let prefix = show_entry.trim_end_matches(SHOW_FILE).to_string();
    let stem = zip_path.file_stem().unwrap_or_default().to_string_lossy();
    let dest = crate::paths::unique_dir(parent, &stem);
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(inner) = entry.enclosed_name() else { continue };
        let Ok(rel) = inner.strip_prefix(&prefix) else { continue };
        if entry.is_dir() || rel.as_os_str().is_empty() {
            continue;
        }
        let out = dest.join(rel);
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        let mut f = std::fs::File::create(&out).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut f).map_err(|e| e.to_string())?;
    }
    Ok(dest)
}

// ── the script ──────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum LineKind {
    Blank,
    /// `# Scene 2`
    Heading,
    /// `NAME: words`
    Speech,
    /// A line straight under a speech, carrying it on.
    More,
    /// A line in brackets: `(She waves.)`
    Direction,
    Text,
}

#[derive(Clone, Debug)]
pub struct ScriptLine {
    pub kind: LineKind,
    pub who: String,
    pub text: String,
    norm: String,
}

/// `NAME: words`, where NAME is one to four capitalised words, like "Narrator" or "Mr. Fish".
fn speaker(line: &str) -> Option<(&str, &str)> {
    let (name, rest) = line.split_once(':')?;
    let name = name.trim();
    let words: Vec<&str> = name.split_whitespace().collect();
    let ok = !words.is_empty()
        && words.len() <= 4
        && name.chars().count() <= 30
        && name.chars().any(char::is_alphabetic)
        && name.chars().all(|c| c.is_alphanumeric() || " .'-&".contains(c))
        && words.iter().all(|w| w.chars().next().is_some_and(|c| c.is_uppercase() || c.is_ascii_digit()));
    ok.then(|| (name, rest.trim()))
}

/// Lowercase words separated by single spaces, so punctuation and curly quotes don't matter.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for word in s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
        if !out.is_empty() {
            out.push(' ');
        }
        out.extend(word.chars().flat_map(char::to_lowercase));
    }
    out
}

pub fn parse_script(script: &str) -> Vec<ScriptLine> {
    let mut lines: Vec<ScriptLine> = Vec::new();
    for raw in script.lines() {
        let t = raw.trim();
        let (kind, who, text) = if t.is_empty() {
            (LineKind::Blank, "", t)
        } else if let Some(h) = t.strip_prefix('#') {
            (LineKind::Heading, "", h.trim_start_matches('#').trim())
        } else if t.starts_with('(') || t.starts_with('[') {
            (LineKind::Direction, "", t)
        } else if let Some((who, text)) = speaker(t) {
            (LineKind::Speech, who, text)
        } else if matches!(lines.last().map(|l| l.kind), Some(LineKind::Speech | LineKind::More)) {
            (LineKind::More, "", t)
        } else {
            (LineKind::Text, "", t)
        };
        let norm = if kind == LineKind::Blank { String::new() } else { normalize(text) };
        lines.push(ScriptLine { kind, who: who.to_string(), text: text.to_string(), norm });
    }
    lines
}

/// Where a cue sits in the script.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Placement {
    /// The cue is drawn just before script line `slot` (so `lines.len()` means after the last line).
    pub slot: usize,
    /// The script lines holding its words, if they were found.
    pub lines: Option<(usize, usize)>,
}

/// Find each cue's line in the script, in order. A cue whose words aren't found (or that has none)
/// sits straight after the cue before it.
pub fn place_cues(lines: &[ScriptLine], cues: &[Cue]) -> Vec<Placement> {
    const WINDOW: usize = 4;
    let mut out = Vec::with_capacity(cues.len());
    let (mut cursor, mut slot) = (0usize, 0usize);
    for cue in cues {
        // What GO waits for is the last line of the cue's words; all of them get highlighted when
        // they can be found together.
        let last = normalize(cue.line.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or(""));
        let (last, whole) = (format!(" {last} "), format!(" {} ", normalize(&cue.line)));
        let mut found = None;
        if last.trim().is_empty() {
            out.push(Placement { slot, lines: None });
            continue;
        }
        for end in cursor..lines.len() {
            if lines[end].kind == LineKind::Blank {
                continue;
            }
            // The words may run across a few script lines, so try short runs ending here.
            let mut joined = String::new();
            let mut last_from = None;
            for start in (cursor.max(end.saturating_sub(WINDOW - 1))..=end).rev() {
                if lines[start].kind == LineKind::Blank {
                    continue;
                }
                joined = if joined.is_empty() {
                    lines[start].norm.clone()
                } else {
                    format!("{} {joined}", lines[start].norm)
                };
                let hay = format!(" {joined} ");
                if hay.contains(&whole) {
                    found = Some((start, end));
                    break;
                }
                if last_from.is_none() && hay.contains(&last) {
                    last_from = Some(start);
                }
            }
            if found.is_none() {
                found = last_from.map(|start| (start, end));
            }
            if found.is_some() {
                break;
            }
        }
        if let Some((_, end)) = found {
            cursor = end + 1;
            slot = end + 1;
        }
        out.push(Placement { slot, lines: found });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(line: &str) -> Cue {
        Cue { line: line.into(), ..Cue::default() }
    }

    const SCRIPT: &str = "# Scene 1\n\
        (The stage is dark.)\n\
        NARRATOR: Once upon a time,\n\
        in a quiet little house,\n\
        someone rang the doorbell.\n\
        \n\
        Mr. Fish: Who could that be?\n\
        WIZARD: Abracadabra!\n\
        NARRATOR: Blub.\n\
        NARRATOR: Blub.\n\
        \n\
        10:30 is when the bell rings";

    #[test]
    fn script_lines_are_recognised() {
        let lines = parse_script(SCRIPT);
        let kinds: Vec<LineKind> = lines.iter().map(|l| l.kind).collect();
        use LineKind::*;
        assert_eq!(kinds, [Heading, Direction, Speech, More, More, Blank, Speech, Speech, Speech, Speech, Blank, Text]);
        assert_eq!(lines[0].text, "Scene 1");
        assert_eq!(lines[2].who, "NARRATOR");
        assert_eq!(lines[2].text, "Once upon a time,");
        assert_eq!(lines[6].who, "Mr. Fish");
    }

    #[test]
    fn cues_find_their_lines_in_order() {
        let lines = parse_script(SCRIPT);
        let cues = [
            cue(""),                           // no line: top of the show
            cue("…someone rang the doorbell"), // punctuation and case don't matter
            cue("abracadabra"),
            cue("Not in the script at all"), // follows the cue before
            cue("Blub."),                    // the first Blub after Abracadabra
            cue("Blub"),                     // and the next one, not the same one again
        ];
        let placed = place_cues(&lines, &cues);
        assert_eq!(placed[0], Placement { slot: 0, lines: None });
        assert_eq!(placed[1], Placement { slot: 5, lines: Some((4, 4)) });
        assert_eq!(placed[2], Placement { slot: 8, lines: Some((7, 7)) });
        assert_eq!(placed[3], Placement { slot: 8, lines: None });
        assert_eq!(placed[4], Placement { slot: 9, lines: Some((8, 8)) });
        assert_eq!(placed[5], Placement { slot: 10, lines: Some((9, 9)) });
    }

    #[test]
    fn words_can_run_across_lines() {
        let lines = parse_script(SCRIPT);
        let placed = place_cues(&lines, &[cue("in a quiet little house, someone rang the doorbell")]);
        assert_eq!(placed[0].lines, Some((3, 4)));
        let placed = place_cues(&lines, &[cue("Once upon a time,\nin a quiet little house")]);
        assert_eq!(placed[0].lines, Some((2, 3)), "all of a multi-line cue's words light up");
        let placed = place_cues(&lines, &[cue("Once upon a SOMETHING,\nin a quiet little house")]);
        assert_eq!(placed[0].lines, Some((3, 3)), "the last line is enough to find it");
    }

    #[test]
    fn a_show_survives_saving_and_loading() {
        let dir = std::env::temp_dir().join(format!("esc-show-{}", std::process::id()));
        let mut show = Show::new("Test");
        let song = show.new_id();
        show.music.push(Song { id: song, name: "Calm".into(), file: "sounds/calm.wav".into(), ..Song::default() });
        let id = show.new_id();
        show.cues.push(Cue { id, name: "Bell".into(), music: Music::Start { song }, ..Cue::default() });
        show.save(&dir).unwrap();
        let back = Show::load(&dir).unwrap();
        assert_eq!(back, show);
        let json = std::fs::read_to_string(dir.join(SHOW_FILE)).unwrap();
        assert!(json.contains("\"do\": \"start\""), "music actions should read plainly: {json}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn old_or_hand_edited_files_still_load() {
        let mut show: Show = serde_json::from_str(
            r#"{"title":"Hand made","cues":[{"name":"A"},{"name":"B","id":7},{"name":"C","id":7}]}"#,
        )
        .unwrap();
        show.repair();
        let ids: Vec<u32> = show.cues.iter().map(|c| c.id).collect();
        assert_eq!(ids.len(), 3);
        assert!(ids.iter().all(|&id| id > 0));
        assert_ne!(ids[1], ids[2]);
        assert!(show.next_id > *ids.iter().max().unwrap());
        assert_eq!(show.cues[0].volume, 1.0);
        assert!(show.cues[0].even_out);
    }

    #[test]
    fn shows_zip_up_and_come_back() {
        let base = std::env::temp_dir().join(format!("esc-zip-{}", std::process::id()));
        let dir = base.join("Original");
        let mut show = Show::new("Zip me");
        let id = show.new_id();
        show.cues.push(Cue { id, name: "Bell".into(), file: "sounds/bell.wav".into(), ..Cue::default() });
        show.save(&dir).unwrap();
        std::fs::write(dir.join("sounds/bell.wav"), b"RIFF fake").unwrap();
        let zip = base.join("Zip me.zip");
        export_zip(&dir, &zip).unwrap();
        let back = import_zip(&zip, &base).unwrap();
        assert_eq!(back.file_name().unwrap(), "Zip me");
        assert_eq!(Show::load(&back).unwrap().cues[0].name, "Bell");
        assert_eq!(std::fs::read(back.join("sounds/bell.wav")).unwrap(), b"RIFF fake");
        std::fs::remove_dir_all(&base).unwrap();
    }
}
