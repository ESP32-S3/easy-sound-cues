//! Sounds made from scratch for the practice show and the speaker test, so the app has something
//! to play before anyone has added sounds of their own (and nothing copyrighted ships with it).

use crate::show::{Cue, Music, SOUNDS_DIR, Show, Song};
use std::f32::consts::TAU;
use std::path::Path;

pub const RATE: u32 = 44_100;

fn seconds(s: f32) -> usize {
    (s * RATE as f32) as usize
}

/// A bell-like note: a few partials with a fast attack and an exponential decay.
fn bell(out: &mut [f32], at: f32, freq: f32, length: f32, level: f32) {
    let start = seconds(at);
    for n in 0..seconds(length).min(out.len().saturating_sub(start)) {
        let t = n as f32 / RATE as f32;
        let env = (t / 0.004).min(1.0) * (-t * 4.0 / length).exp();
        let tone = (TAU * freq * t).sin() + 0.35 * (TAU * 2.0 * freq * t).sin() + 0.12 * (TAU * 3.01 * freq * t).sin();
        out[start + n] += level * env * tone / 1.47;
    }
}

/// Small deterministic noise source, so the thunder sounds the same every time.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

pub fn doorbell() -> Vec<f32> {
    let mut out = vec![0.0; seconds(2.0)];
    bell(&mut out, 0.0, 659.25, 1.1, 0.6);
    bell(&mut out, 0.55, 523.25, 1.4, 0.6);
    out
}

pub fn sparkle() -> Vec<f32> {
    let mut out = vec![0.0; seconds(1.6)];
    for (k, f) in [1046.5, 1174.7, 1318.5, 1568.0, 1760.0, 2093.0, 2349.3, 2637.0].into_iter().enumerate() {
        bell(&mut out, k as f32 * 0.07, f, 0.6, 0.32);
    }
    out
}

pub fn thunder() -> Vec<f32> {
    let mut out = vec![0.0; seconds(3.5)];
    let mut noise = Noise(0x9E37_79B9);
    let (mut low, mut lower) = (0.0f32, 0.0f32);
    for (n, s) in out.iter_mut().enumerate() {
        let t = n as f32 / RATE as f32;
        low += (noise.next() - low) * 0.06;
        lower += (low - lower) * 0.03;
        let crack = (-t * 9.0).exp() * noise.next() * 0.25;
        let rumble = (t / 0.05).min(1.0) * (-t * 1.1).exp() * (1.0 + 0.5 * (TAU * 1.7 * t).sin());
        *s = (lower * 9.0 * rumble + crack).clamp(-1.0, 1.0) * 0.8;
    }
    out
}

pub fn tada() -> Vec<f32> {
    let mut out = vec![0.0; seconds(2.2)];
    for f in [523.25, 659.25, 783.99, 1046.5] {
        bell(&mut out, 0.0, f, 0.18, 0.22);
        bell(&mut out, 0.2, f, 1.9, 0.22);
    }
    out
}

/// Four soft chords, two seconds each, that loop without a seam.
pub fn calm_music() -> Vec<f32> {
    const CHORDS: [[f32; 3]; 4] =
        [[261.63, 329.63, 392.0], [220.0, 261.63, 329.63], [174.61, 220.0, 261.63], [196.0, 246.94, 293.66]];
    let chord_len = seconds(2.0);
    let mut out = vec![0.0; chord_len * CHORDS.len()];
    for (k, chord) in CHORDS.iter().enumerate() {
        for n in 0..chord_len {
            let t = n as f32 / RATE as f32;
            // Swells in and out within its own two seconds, so the loop point is silent too.
            let env = (std::f32::consts::PI * n as f32 / chord_len as f32).sin().powf(0.6);
            let pad: f32 = chord.iter().map(|f| (TAU * f * t).sin() + 0.2 * (TAU * 2.0 * f * t).sin()).sum();
            out[k * chord_len + n] = pad * env * 0.11;
        }
        bell(&mut out, k as f32 * 2.0 + 0.25, chord[2] * 2.0, 1.5, 0.12);
    }
    out
}

/// Two quick chimes for "Play a test sound".
pub fn test_chime() -> Vec<f32> {
    let mut out = vec![0.0; seconds(1.2)];
    bell(&mut out, 0.0, 880.0, 0.5, 0.5);
    bell(&mut out, 0.18, 1318.5, 0.9, 0.5);
    out
}

/// 16-bit mono WAV, which every system and every audio program can open.
pub fn write_wav(path: &Path, samples: &[f32]) -> std::io::Result<()> {
    let data_len = (samples.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&RATE.to_le_bytes());
    bytes.extend_from_slice(&(RATE * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        bytes.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, bytes)
}

pub const PRACTICE_TITLE: &str = "Practice Show";

const PRACTICE_SCRIPT: &str = "\
# The Practice Play
(A tiny pretend play for practising. When a highlighted line is said, press GO.)

NARRATOR: Once upon a time, in a quiet little house, there lived a girl called Sam.
NARRATOR: One morning, someone rang the doorbell.
SAM: Who could that be? I'll go and see!
(Sam opens the door. A wizard is standing there.)
WIZARD: Good morning! Watch this. Abracadabra!
NARRATOR: The wizard waved his wand, and soft music filled the air.
SAM: Wow! That's amazing!
NARRATOR: But then dark clouds rolled in, and there was a crash of thunder.
SAM: Oh no! Make it stop!
WIZARD: Don't worry. One more wave of my wand...
NARRATOR: And the sun came out again. The end!
(Everyone bows.)
";

/// Build the practice show in `dir`: its made-up sounds as WAV files and a short script.
pub fn practice_show(dir: &Path) -> Result<Show, String> {
    let sounds = dir.join(SOUNDS_DIR);
    std::fs::create_dir_all(&sounds).map_err(|e| e.to_string())?;
    let files = [
        ("doorbell.wav", doorbell()),
        ("magic sparkle.wav", sparkle()),
        ("thunder.wav", thunder()),
        ("ta-da.wav", tada()),
        ("calm music.wav", calm_music()),
    ];
    for (name, samples) in &files {
        write_wav(&sounds.join(name), samples).map_err(|e| format!("Couldn't write {name}: {e}"))?;
    }
    let mut show = Show::new(PRACTICE_TITLE);
    show.script = PRACTICE_SCRIPT.to_string();
    let calm = show.new_id();
    show.music.push(Song {
        id: calm,
        name: "Calm music".into(),
        file: "sounds/calm music.wav".into(),
        source: "Made by Easy Sound Cues".into(),
        ..Song::default()
    });
    let cues = [
        ("Doorbell", "doorbell.wav", "NARRATOR", "someone rang the doorbell.", Music::Keep, ""),
        (
            "Magic sparkle",
            "magic sparkle.wav",
            "WIZARD",
            "Abracadabra!",
            Music::Start { song: calm },
            "The calm music starts too, and keeps going round and round.",
        ),
        (
            "Thunder",
            "thunder.wav",
            "NARRATOR",
            "there was a crash of thunder.",
            Music::Fade,
            "The music fades away under the thunder.",
        ),
        ("Ta-da!", "ta-da.wav", "NARRATOR", "The end!", Music::Keep, "The last sound. Well done!"),
    ];
    for (name, file, who, line, music, note) in cues {
        let id = show.new_id();
        show.cues.push(Cue {
            id,
            name: name.into(),
            file: format!("sounds/{file}"),
            who: who.into(),
            line: line.into(),
            note: note.into(),
            music,
            source: "Made by Easy Sound Cues".into(),
            ..Cue::default()
        });
    }
    show.save(dir)?;
    Ok(show)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn made_up_sounds_are_audible_and_never_clip() {
        for (name, s) in [
            ("doorbell", doorbell()),
            ("sparkle", sparkle()),
            ("thunder", thunder()),
            ("tada", tada()),
            ("calm", calm_music()),
            ("chime", test_chime()),
        ] {
            let peak = s.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            assert!(peak > 0.1, "{name} is too quiet: {peak}");
            assert!(peak <= 1.0, "{name} clips: {peak}");
        }
        let calm = calm_music();
        assert!(calm[0].abs() < 1e-3 && calm[calm.len() - 1].abs() < 0.01, "the music loop has a click at its seam");
    }

    #[test]
    fn practice_show_is_complete() {
        let dir = std::env::temp_dir().join(format!("esc-practice-{}", std::process::id()));
        let show = practice_show(&dir).unwrap();
        assert_eq!(show.cues.len(), 4);
        for cue in &show.cues {
            assert!(Show::path_of(&dir, &cue.file).is_file(), "{} has no file", cue.name);
        }
        let lines = crate::show::parse_script(&show.script);
        let placed = crate::show::place_cues(&lines, &show.cues);
        assert!(
            placed.iter().all(|p| p.lines.is_some()),
            "every practice cue should be found in its script: {placed:?}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
