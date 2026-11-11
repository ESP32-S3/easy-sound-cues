//! Where things live on disk, and the app's own settings.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

const APP_DIR: &str = if cfg!(target_os = "linux") { "easy-sound-cues" } else { "Easy Sound Cues" };

/// Tests and screenshots point everything at a scratch folder with this variable.
fn override_root() -> Option<PathBuf> {
    std::env::var_os("EASY_SOUND_CUES_HOME").map(PathBuf::from)
}

/// The app's private folder: settings and the helper programs.
pub fn data_dir() -> PathBuf {
    if let Some(root) = override_root() {
        return root.join("data");
    }
    dirs::data_local_dir().or_else(dirs::home_dir).unwrap_or_else(std::env::temp_dir).join(APP_DIR)
}

pub fn helpers_dir() -> PathBuf {
    data_dir().join("helpers")
}

/// Where shows are kept: a plainly named folder in Documents, so people can find and back it up.
pub fn shows_dir() -> PathBuf {
    if let Some(root) = override_root() {
        return root.join("Easy Sound Cues");
    }
    dirs::document_dir().or_else(dirs::home_dir).unwrap_or_else(data_dir).join("Easy Sound Cues")
}

/// Downloads made while no show is open.
pub fn downloads_dir() -> PathBuf {
    shows_dir().join("Downloads")
}

/// A path the way people think of it: "Documents/Easy Sound Cues/…" rather than the full path.
pub fn friendly(path: &Path) -> String {
    // A scratch folder for tests and screenshots stands in for Documents.
    if let Some(rest) = override_root().and_then(|root| path.strip_prefix(root).ok().map(Path::to_path_buf)) {
        return Path::new("Documents").join(rest).display().to_string();
    }
    if let Some(docs) = dirs::document_dir()
        && let Ok(rest) = path.strip_prefix(&docs)
    {
        return Path::new("Documents").join(rest).display().to_string();
    }
    if let Some(home) = dirs::home_dir()
        && let Ok(rest) = path.strip_prefix(&home)
    {
        return Path::new("~").join(rest).display().to_string();
    }
    path.display().to_string()
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// The welcome tour has been through once.
    pub welcomed: bool,
    pub last_show: Option<PathBuf>,
    /// Overall volume, 0..=1.
    pub volume: f32,
    /// Multiplies every size in the window.
    pub text_size: f32,
    pub light: bool,
    /// The speakers to use, by name; `None` follows the computer's default.
    pub speaker: Option<String>,
    /// The person said they won't use YouTube, so missing helpers aren't worth mentioning.
    pub no_youtube: bool,
    /// Look for a newer version of the app once a day.
    pub check_updates: bool,
    /// When that last happened, in seconds since 1970.
    pub last_update_check: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            welcomed: false,
            last_show: None,
            volume: 1.0,
            text_size: 1.0,
            light: false,
            speaker: None,
            no_youtube: false,
            check_updates: true,
            last_update_check: 0,
        }
    }
}

impl Settings {
    fn path() -> PathBuf {
        data_dir().join("settings.json")
    }

    pub fn load() -> Self {
        let text = std::fs::read_to_string(Self::path()).unwrap_or_default();
        let mut s: Settings = serde_json::from_str(&text).unwrap_or_default();
        s.volume = s.volume.clamp(0.0, 1.0);
        s.text_size = s.text_size.clamp(0.7, 2.0);
        s
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }
}

/// Open a folder in Explorer, Finder or the Linux file manager.
pub fn reveal(path: &Path) {
    let program = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = quiet(Command::new(program).arg(path)).spawn();
}

/// Run child programs without flashing a black console window on Windows.
pub fn quiet(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// A file or folder name that works on Windows, macOS and Linux alike.
pub fn safe_name(name: &str) -> String {
    let kept: String =
        name.chars().map(|c| if c.is_alphanumeric() || " -_.,'()&!+#".contains(c) { c } else { ' ' }).collect();
    let words = kept.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut name: String = words.trim_matches(|c: char| c == '.' || c == ' ').chars().take(60).collect();
    name = name.trim_end_matches(['.', ' ']).to_string();
    if name.is_empty() {
        name = "sound".into();
    }
    let upper = name.to_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&upper.as_str())
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.as_bytes()[3].is_ascii_digit());
    if reserved {
        name.push('_');
    }
    name
}

/// `dir/stem.ext`, or `dir/stem (2).ext` and so on if that is taken.
pub fn unique_file(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let base = safe_name(stem);
    let mut path = dir.join(format!("{base}.{ext}"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{base} ({n}).{ext}"));
        n += 1;
    }
    path
}

/// Like [`unique_file`] for folders.
pub fn unique_dir(parent: &Path, name: &str) -> PathBuf {
    let base = safe_name(name);
    let mut path = parent.join(&base);
    let mut n = 2;
    while path.exists() {
        path = parent.join(format!("{base} ({n})"));
        n += 1;
    }
    path
}

/// Move a file, copying when it has to cross from one drive to another.
pub fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to)?;
    let _ = std::fs::remove_file(from);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_safe_everywhere() {
        assert_eq!(safe_name("Spongebob Bubble 🔊👍🏻 No Copyright: 100% 🆓"), "Spongebob Bubble No Copyright 100");
        assert_eq!(safe_name("a/b\\c?d*e"), "a b c d e");
        assert_eq!(safe_name("  ...  "), "sound");
        assert_eq!(safe_name("con"), "con_");
        assert_eq!(safe_name("COM1"), "COM1_");
        assert_eq!(safe_name("Камин (EMIN feat. JONY)"), "Камин (EMIN feat. JONY)");
        assert!(safe_name(&"x".repeat(200)).len() <= 60);
    }

    #[test]
    fn unique_names_count_up() {
        let dir = std::env::temp_dir().join(format!("esc-unique-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = unique_file(&dir, "Thunder", "mp3");
        assert_eq!(first.file_name().unwrap(), "Thunder.mp3");
        std::fs::write(&first, b"x").unwrap();
        assert_eq!(unique_file(&dir, "Thunder", "mp3").file_name().unwrap(), "Thunder (2).mp3");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
