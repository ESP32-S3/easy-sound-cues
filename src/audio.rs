//! Audio: one output stream, a master bus with a limiter on it, and a voice per playing sound.
//!
//! Sound effects are decoded into memory when a show opens, so GO starts them instantly. Songs run
//! for minutes, so they stream from their file and loop until faded out. Every voice ramps its own
//! gain on the audio thread, which keeps fades and stops click-free and lets the UI read back
//! exactly how far through a sound is.

use rodio::mixer::{self, Mixer, MixerSource};
use rodio::source::{LimitSettings, Source};
use rodio::{ChannelCount, CpalErrorKind, Decoder, DeviceSinkBuilder, DeviceTrait, MixerDeviceSink, SampleRate};
use std::fs::File;
use std::io::{BufReader, Cursor};
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering::Relaxed};
use std::time::Duration;

/// How fast gain follows a level change when nothing is fading: full scale in 30 ms.
const GLIDE: f32 = 1.0 / 0.03;
/// Loudness that "even out" brings sound effects to (EBU R128, like ffmpeg's loudnorm).
const EFFECT_TARGET: f64 = -16.0;
/// Songs sit a little lower, since they play under everything.
const MUSIC_TARGET: f64 = -18.0;
/// Anything quieter than this (about -50 dB) counts as silence at the start of a sound.
const SILENCE: f32 = 0.003;
/// Fade at each end of a looped section, so the loop point can't click.
const EDGE: f32 = 0.01;

fn load(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Relaxed))
}

fn store(a: &AtomicU32, v: f32) {
    a.store(v.to_bits(), Relaxed)
}

fn hint(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn open_error(path: &Path, e: std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => format!("The file {} is missing.", file_name(path)),
        _ => format!("Couldn't open {}: {e}", file_name(path)),
    }
}

fn decode_error(path: &Path) -> String {
    format!("{} isn't a sound file this app can play. MP3, WAV, M4A, OGG and FLAC files work.", file_name(path))
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// Gain that takes a sound from its measured loudness to `target`, without pushing its loudest
/// moment past full scale (the limiter on the bus catches what's left).
fn even_gain(loudness: f64, peak: f32, target: f64) -> f32 {
    let wanted = if loudness.is_finite() {
        10f64.powf((target - loudness) / 20.0) as f32
    } else if peak > 0.0 {
        // Too short to measure loudness (under 0.4 s): bring its peak to -6 dB instead.
        0.5 / peak
    } else {
        1.0
    };
    wanted.min(1.0 / peak.max(1e-4)).clamp(0.05, 8.0)
}

/// Loudness meter fed a chunk at a time, so a long song never has to sit in memory.
struct Meter {
    ebu: Option<ebur128::EbuR128>,
    peak: f32,
}

impl Meter {
    fn new(channels: ChannelCount, rate: SampleRate) -> Self {
        let ebu = ebur128::EbuR128::new(channels.get() as u32, rate.get(), ebur128::Mode::I).ok();
        Self { ebu, peak: 0.0 }
    }

    fn add(&mut self, chunk: &[f32]) {
        self.peak = chunk.iter().fold(self.peak, |m, s| m.max(s.abs()));
        if let Some(ebu) = &mut self.ebu {
            let _ = ebu.add_frames_f32(chunk);
        }
    }

    fn gain(&self, target: f64) -> f32 {
        let loudness = self.ebu.as_ref().and_then(|e| e.loudness_global().ok()).unwrap_or(f64::NEG_INFINITY);
        even_gain(loudness, self.peak, target)
    }
}

/// A sound effect, decoded up front.
pub struct Clip {
    data: Arc<[f32]>,
    channels: ChannelCount,
    rate: SampleRate,
    /// Loudness outline (0..=1) for drawing the waveform.
    pub peaks: Vec<f32>,
    /// Gain that brings it to the same loudness as other sound effects.
    pub even_gain: f32,
    /// Frames of near-silence before the sound really starts.
    pub lead: usize,
}

impl Clip {
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| open_error(path, e))?;
        let len = bytes.len() as u64;
        let decoder = Decoder::builder()
            .with_data(Cursor::new(bytes))
            .with_byte_len(len)
            .with_hint(&hint(path))
            .build()
            .map_err(|_| decode_error(path))?;
        let (channels, rate) = (decoder.channels(), decoder.sample_rate());
        Self::from_samples(decoder.collect(), channels, rate).map_err(|e| format!("{}: {e}", file_name(path)))
    }

    pub fn from_samples(data: Vec<f32>, channels: ChannelCount, rate: SampleRate) -> Result<Self, String> {
        let ch = channels.get() as usize;
        if data.len() < ch {
            return Err("the file has no sound in it".into());
        }
        let mut meter = Meter::new(channels, rate);
        meter.add(&data);
        let lead = data.iter().position(|s| s.abs() > SILENCE).map_or(0, |i| i / ch);
        // Keep 5 ms before the first sound so its attack isn't shaved off.
        let lead = lead.saturating_sub(rate.get() as usize / 200);
        Ok(Self {
            peaks: outline(&data, ch, 160),
            even_gain: meter.gain(EFFECT_TARGET),
            lead,
            data: data.into(),
            channels,
            rate,
        })
    }

    /// A mono clip at the made-up sounds' rate.
    pub fn mono(samples: Vec<f32>) -> Self {
        let rate = NonZero::new(crate::synth::RATE).expect("non-zero rate");
        Self::from_samples(samples, NonZero::new(1).expect("one channel"), rate).expect("made-up sounds aren't empty")
    }

    pub fn frames(&self) -> usize {
        self.data.len() / self.channels.get() as usize
    }

    pub fn rate(&self) -> f32 {
        self.rate.get() as f32
    }

    pub fn seconds(&self) -> f32 {
        self.frames() as f32 / self.rate()
    }

    /// The frames a cue plays, given its trim settings.
    pub fn range(&self, skip_silence: bool, start: f32, end: f32) -> (usize, usize) {
        let frames = self.frames();
        let mut from = (start.max(0.0) * self.rate()) as usize;
        if skip_silence {
            from = from.max(self.lead);
        }
        let to = if end > 0.0 { ((end * self.rate()) as usize).min(frames) } else { frames };
        let from = from.min(to.saturating_sub(1));
        (from, to.max(from + 1))
    }
}

fn outline(data: &[f32], channels: usize, bins: usize) -> Vec<f32> {
    let frames = data.len() / channels;
    let mut peaks: Vec<f32> = (0..bins)
        .map(|b| {
            let start = b * frames / bins * channels;
            let end = ((b + 1) * frames / bins * channels).clamp(start + 1, data.len());
            data[start.min(data.len() - 1)..end].iter().fold(0.0f32, |m, s| m.max(s.abs()))
        })
        .collect();
    shape(&mut peaks);
    peaks
}

fn shape(peaks: &mut [f32]) {
    let loudest = peaks.iter().copied().fold(1e-6, f32::max);
    for p in peaks {
        *p = (*p / loudest).powf(0.7);
    }
}

/// A song for the music track. It runs for minutes, so it streams from its file rather than
/// sitting decoded in memory (a decoded three-minute song would take about 60 MB).
pub struct Song {
    path: PathBuf,
    channels: ChannelCount,
    rate: SampleRate,
    frames: usize,
    pub peaks: Vec<f32>,
    pub even_gain: f32,
}

impl Song {
    /// Reads the whole file once, a chunk at a time, to measure its length and loudness.
    pub fn load(path: &Path) -> Result<Self, String> {
        const BINS: usize = 160;
        let mut decoder = Self::open_decoder(path)?;
        let (channels, rate) = (decoder.channels(), decoder.sample_rate());
        let ch = channels.get() as usize;
        let expected = decoder.total_duration().map(|d| (d.as_secs_f64() * rate.get() as f64) as usize);
        let mut meter = Meter::new(channels, rate);
        let mut raw_peaks = vec![0.0f32; BINS];
        let mut chunk = Vec::with_capacity(4096 * ch);
        let mut frames = 0usize;
        loop {
            chunk.clear();
            chunk.extend(decoder.by_ref().take(4096 * ch));
            if chunk.is_empty() {
                break;
            }
            meter.add(&chunk);
            if let Some(total) = expected.filter(|&t| t > 0) {
                for (k, frame) in chunk.chunks(ch).enumerate() {
                    let bin = ((frames + k) * BINS / total).min(BINS - 1);
                    raw_peaks[bin] = frame.iter().fold(raw_peaks[bin], |m, s| m.max(s.abs()));
                }
            }
            frames += chunk.len() / ch;
        }
        if frames == 0 {
            return Err(format!("{} has no sound in it.", file_name(path)));
        }
        let peaks = if expected.is_some() {
            shape(&mut raw_peaks);
            raw_peaks
        } else {
            Vec::new()
        };
        Ok(Self { path: path.to_path_buf(), channels, rate, frames, peaks, even_gain: meter.gain(MUSIC_TARGET) })
    }

    fn open_decoder(path: &Path) -> Result<Decoder<BufReader<File>>, String> {
        let file = File::open(path).map_err(|e| open_error(path, e))?;
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        Decoder::builder()
            .with_data(BufReader::new(file))
            .with_byte_len(len)
            .with_seekable(true)
            .with_hint(&hint(path))
            .build()
            .map_err(|_| decode_error(path))
    }

    pub fn rate(&self) -> f32 {
        self.rate.get() as f32
    }

    pub fn seconds(&self) -> f32 {
        self.frames as f32 / self.rate()
    }

    /// The frames to play for a song with these settings.
    pub fn range(&self, start: f32, end: f32) -> (usize, usize) {
        let to = if end > 0.0 { ((end * self.rate()) as usize).min(self.frames) } else { self.frames };
        let from = ((start.max(0.0) * self.rate()) as usize).min(to.saturating_sub(1));
        (from, to.max(from + 1))
    }
}

/// Plays one stretch of a song file, going back to its start each time it reaches the end.
struct Section {
    decoder: Decoder<BufReader<File>>,
    path: PathBuf,
    channels: usize,
    rate: f32,
    start: usize,
    end: usize,
    repeat: bool,
    /// Frame of the file the next sample belongs to.
    pos: usize,
    sub: usize,
    edge: usize,
}

impl Section {
    fn new(song: &Song, (start, end): (usize, usize), repeat: bool) -> Result<Self, String> {
        let decoder = Song::open_decoder(&song.path)?;
        let mut section = Self {
            decoder,
            path: song.path.clone(),
            channels: song.channels.get() as usize,
            rate: song.rate(),
            start,
            end,
            repeat,
            pos: 0,
            sub: 0,
            edge: ((EDGE * song.rate()) as usize).max(1),
        };
        if start > 0 {
            section.rewind();
        }
        Ok(section)
    }

    /// Back to the section's start: a seek if the decoder can, or a fresh read up to it if not.
    fn rewind(&mut self) -> bool {
        let at = Duration::from_secs_f64(self.start as f64 / self.rate as f64);
        if self.decoder.try_seek(at).is_err() {
            let Ok(mut fresh) = Song::open_decoder(&self.path) else { return false };
            if fresh.by_ref().take(self.start * self.channels).count() < self.start * self.channels {
                return false;
            }
            self.decoder = fresh;
        }
        self.pos = self.start;
        self.sub = 0;
        true
    }
}

impl Iterator for Section {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        for _ in 0..2 {
            if self.pos >= self.end && (!self.repeat || !self.rewind()) {
                return None;
            }
            if let Some(sample) = self.decoder.next() {
                let into = self.pos - self.start;
                let left = self.end - self.pos;
                let gain = (into.min(left) as f32 / self.edge as f32).min(1.0);
                self.sub += 1;
                if self.sub == self.channels {
                    self.sub = 0;
                    self.pos += 1;
                }
                return Some(sample * gain);
            }
            // The file ended before we thought it would: that's the real end.
            if self.pos == self.start {
                return None;
            }
            self.end = self.pos;
        }
        None
    }
}

/// Shared between the UI and one playing sound.
struct Control {
    target: AtomicU32,
    /// Gain change per second while heading for `target`.
    speed: AtomicU32,
    /// The gain the audio thread is actually applying right now.
    level: AtomicU32,
    /// End the sound once the gain reaches zero.
    kill: AtomicBool,
    frame: AtomicUsize,
    ended: AtomicBool,
}

impl Control {
    /// A voice starting at `gain` and heading for `target` at `speed` per second.
    fn new(gain: f32, target: f32, speed: f32) -> Arc<Self> {
        Arc::new(Self {
            target: AtomicU32::new(target.to_bits()),
            speed: AtomicU32::new(speed.to_bits()),
            level: AtomicU32::new(gain.to_bits()),
            kill: AtomicBool::new(false),
            frame: AtomicUsize::new(0),
            ended: AtomicBool::new(false),
        })
    }
}

/// The UI's handle on a playing sound.
pub struct Voice {
    /// The cue for a sound effect, or the song for music.
    pub id: u32,
    pub fading: bool,
    ctl: Arc<Control>,
    frames: usize,
    rate: f32,
    looping: bool,
}

impl Voice {
    fn played(&self) -> usize {
        self.ctl.frame.load(Relaxed)
    }

    fn length_frames(&self) -> usize {
        self.frames.max(1)
    }

    pub fn progress(&self) -> f32 {
        let (played, length) = (self.played(), self.length_frames());
        if self.looping { (played % length) as f32 / length as f32 } else { (played as f32 / length as f32).min(1.0) }
    }

    pub fn remaining(&self) -> f32 {
        let (played, length) = (self.played(), self.length_frames());
        let left = if self.looping { length - played % length } else { length.saturating_sub(played) };
        left as f32 / self.rate
    }

    /// Seconds into the current pass, and how many times it has already looped.
    pub fn position(&self) -> (f32, usize) {
        let (played, length) = (self.played(), self.length_frames());
        ((played % length) as f32 / self.rate, played / length)
    }

    pub fn seconds(&self) -> f32 {
        self.frames as f32 / self.rate
    }

    pub fn ended(&self) -> bool {
        self.ctl.ended.load(Relaxed)
    }

    pub fn set_level(&self, gain: f32) {
        if !self.fading {
            store(&self.ctl.speed, GLIDE);
            store(&self.ctl.target, gain);
        }
    }

    /// Ramp to silence over `secs`, then end the sound.
    pub fn fade_out(&mut self, secs: f32) {
        let from = load(&self.ctl.level).max(0.01);
        store(&self.ctl.speed, from / secs.max(0.005));
        store(&self.ctl.target, 0.0);
        self.ctl.kill.store(true, Relaxed);
        self.fading = true;
    }
}

/// Hands out part of a decoded clip's shared samples, fading the last stretch if asked to.
struct ClipSamples {
    data: Arc<[f32]>,
    pos: usize,
    end: usize,
    channels: usize,
    fade_from: usize,
}

impl ClipSamples {
    fn new(data: Arc<[f32]>, channels: usize, (from, to): (usize, usize), fade_frames: usize) -> Self {
        let end = (to * channels).min(data.len());
        let pos = (from * channels).min(end);
        let fade_from = end.saturating_sub(fade_frames * channels).max(pos);
        Self { data, pos, end, channels, fade_from }
    }
}

impl Iterator for ClipSamples {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.pos >= self.end {
            return None;
        }
        let mut sample = self.data[self.pos];
        if self.pos >= self.fade_from {
            let frame = (self.pos - self.fade_from) / self.channels;
            let frames = ((self.end - self.fade_from) / self.channels).max(1);
            sample *= 1.0 - frame as f32 / frames as f32;
        }
        self.pos += 1;
        Some(sample)
    }
}

/// Applies a voice's gain ramp to a stream of samples, ends it once a fade reaches silence, and
/// publishes how far through it is.
struct VoiceSource<S> {
    inner: S,
    channels: ChannelCount,
    rate: SampleRate,
    played: usize,
    gain: f32,
    ctl: Arc<Control>,
}

impl<S> VoiceSource<S> {
    fn new(inner: S, channels: ChannelCount, rate: SampleRate, ctl: Arc<Control>) -> Self {
        Self { inner, channels, rate, played: 0, gain: load(&ctl.level), ctl }
    }

    fn finish(&self) -> Option<f32> {
        self.ctl.frame.store(self.played / self.channels.get() as usize, Relaxed);
        self.ctl.ended.store(true, Relaxed);
        None
    }
}

impl<S: Iterator<Item = f32>> Iterator for VoiceSource<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let channels = self.channels.get() as usize;
        if self.played.is_multiple_of(channels) {
            // Gain only moves between frames so both speakers always get the same level.
            let step = load(&self.ctl.speed) / self.rate.get() as f32;
            self.gain += (load(&self.ctl.target) - self.gain).clamp(-step, step);
            store(&self.ctl.level, self.gain);
            if self.gain <= 0.0 && self.ctl.kill.load(Relaxed) {
                return self.finish();
            }
            self.ctl.frame.store(self.played / channels, Relaxed);
        }
        let Some(sample) = self.inner.next() else {
            return self.finish();
        };
        self.played += 1;
        Some(sample * self.gain)
    }
}

impl<S: Iterator<Item = f32>> Source for VoiceSource<S> {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> SampleRate {
        self.rate
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

impl<S> Drop for VoiceSource<S> {
    fn drop(&mut self) {
        self.ctl.ended.store(true, Relaxed);
    }
}

/// Sums every voice and applies the master level. It never ends (silence when idle), so the
/// limiter after it stays in the chain for the whole show.
struct Bus {
    voices: MixerSource,
    master: Arc<AtomicU32>,
    gain: f32,
    step: f32,
    channels: usize,
    channel: usize,
}

impl Iterator for Bus {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.channel == 0 {
            self.gain += (load(&self.master) - self.gain).clamp(-self.step, self.step);
        }
        self.channel = (self.channel + 1) % self.channels;
        Some(self.voices.next().unwrap_or(0.0) * self.gain)
    }
}

impl Source for Bus {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        self.voices.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.voices.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// The speakers the computer offers, by name.
pub fn speakers() -> Vec<String> {
    use rodio::cpal::traits::HostTrait;
    let Ok(devices) = rodio::cpal::default_host().output_devices() else { return Vec::new() };
    let mut names: Vec<String> = devices.filter_map(|d| d.description().ok().map(|d| d.name().to_string())).collect();
    names.dedup();
    names
}

pub struct Engine {
    // Field order matters: the mixer handle goes before the device it feeds.
    voices: Option<Mixer>,
    sink: Option<MixerDeviceSink>,
    master: Arc<AtomicU32>,
    broken: Arc<AtomicBool>,
    /// Why there is no sound, when there isn't, in words for the person at the computer.
    pub problem: Option<String>,
    /// Name of the speakers in use.
    pub device: String,
    pub sample_rate: u32,
}

impl Engine {
    pub fn open(master: f32, speaker: Option<&str>) -> Self {
        let mut engine = Self {
            voices: None,
            sink: None,
            master: Arc::new(AtomicU32::new(master.to_bits())),
            broken: Arc::new(AtomicBool::new(false)),
            problem: None,
            device: String::new(),
            sample_rate: 0,
        };
        engine.connect(speaker);
        engine
    }

    /// An engine with no speakers at all, for tests and screenshots.
    pub fn silent() -> Self {
        Self {
            voices: None,
            sink: None,
            master: Arc::new(AtomicU32::new(1f32.to_bits())),
            broken: Arc::new(AtomicBool::new(false)),
            problem: None,
            device: "Test speakers".into(),
            sample_rate: 48_000,
        }
    }

    /// (Re)open the chosen speakers, or the computer's default ones.
    pub fn connect(&mut self, speaker: Option<&str>) {
        use rodio::cpal::traits::HostTrait;
        self.voices = None;
        self.sink = None;
        self.broken.store(false, Relaxed);
        let host = rodio::cpal::default_host();
        let chosen = speaker.and_then(|want| {
            host.output_devices().ok()?.find(|d| d.description().is_ok_and(|desc| desc.name() == want))
        });
        let device = chosen.or_else(|| host.default_output_device());
        let Some(device) = device else {
            self.problem = Some("No speakers or headphones were found. Plug some in, then press “Try again”.".into());
            return;
        };
        let name = device.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "Speakers".into());
        let broken = self.broken.clone();
        let opened = DeviceSinkBuilder::from_device(device).and_then(|builder| {
            builder
                .with_error_callback(move |err| {
                    eprintln!("audio device: {err}");
                    if !matches!(err.kind(), CpalErrorKind::Xrun) {
                        broken.store(true, Relaxed);
                    }
                })
                .open_sink_or_fallback()
        });
        match opened {
            Ok(mut sink) => {
                sink.log_on_drop(false);
                let (channels, rate) = (sink.config().channel_count(), sink.config().sample_rate());
                let (voices, mix) = mixer::mixer(channels, rate);
                let bus = Bus {
                    voices: mix,
                    master: self.master.clone(),
                    gain: load(&self.master),
                    step: GLIDE / rate.get() as f32,
                    channels: channels.get() as usize,
                    channel: 0,
                };
                let limiter = LimitSettings::default().with_threshold(-1.0).with_knee_width(2.0);
                sink.mixer().add(bus.limit(limiter));
                self.sample_rate = rate.get();
                self.voices = Some(voices);
                self.sink = Some(sink);
                self.device = name;
                self.problem = None;
            }
            Err(err) => {
                self.problem = Some(format!(
                    "The speakers “{name}” wouldn't start ({err}). Check they're plugged in, then press “Try again”."
                ))
            }
        }
    }

    pub fn ready(&self) -> bool {
        self.voices.is_some()
    }

    /// Turn a device failure reported by the audio thread into a visible problem.
    pub fn poll(&mut self) {
        if self.broken.swap(false, Relaxed) && self.problem.is_none() {
            self.problem = Some("The speakers stopped working (maybe they were unplugged). Press “Try again”.".into());
        }
    }

    pub fn set_master(&self, level: f32) {
        store(&self.master, level);
    }

    fn no_speakers(&self) -> String {
        self.problem.clone().unwrap_or_else(|| "No speakers are connected.".into())
    }

    /// Play frames `range` of a clip at `gain`, fading over the last `fade_out` seconds.
    pub fn play(&self, id: u32, clip: &Clip, range: (usize, usize), gain: f32, fade_out: f32) -> Result<Voice, String> {
        let voices = self.voices.as_ref().ok_or_else(|| self.no_speakers())?;
        let ctl = Control::new(gain, gain, GLIDE);
        let fade = (fade_out.max(0.0) * clip.rate()) as usize;
        let samples = ClipSamples::new(clip.data.clone(), clip.channels.get() as usize, range, fade);
        voices.add(VoiceSource::new(samples, clip.channels, clip.rate, ctl.clone()));
        Ok(Voice { id, fading: false, ctl, frames: range.1 - range.0, rate: clip.rate(), looping: false })
    }

    /// Start a song. With a `fade_in` it rises from silence over that many seconds, which is how
    /// a crossfade comes in under the song it replaces.
    pub fn play_song(
        &self,
        id: u32,
        song: &Song,
        range: (usize, usize),
        repeat: bool,
        gain: f32,
        fade_in: f32,
    ) -> Result<Voice, String> {
        let voices = self.voices.as_ref().ok_or_else(|| self.no_speakers())?;
        let stream = Section::new(song, range, repeat)?;
        let ctl = if fade_in > 0.0 { Control::new(0.0, gain, gain / fade_in) } else { Control::new(gain, gain, GLIDE) };
        voices.add(VoiceSource::new(stream, song.channels, song.rate, ctl.clone()));
        Ok(Voice { id, fading: false, ctl, frames: range.1 - range.0, rate: song.rate(), looping: repeat })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stereo voice of `frames` full-scale frames at 1 kHz, so 1 frame = 1 ms.
    fn voice(frames: usize) -> (VoiceSource<ClipSamples>, Voice) {
        let ctl = Control::new(1.0, 1.0, GLIDE);
        let samples = ClipSamples::new(vec![1.0; frames * 2].into(), 2, (0, frames), 0);
        let source = VoiceSource::new(samples, NonZero::new(2).unwrap(), NonZero::new(1000).unwrap(), ctl.clone());
        (source, Voice { id: 0, fading: false, ctl, frames, rate: 1000.0, looping: false })
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("esc-audio-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn wav_files_load_with_their_length() {
        let dir = scratch("wav");
        let path = dir.join("bell.wav");
        crate::synth::write_wav(&path, &crate::synth::doorbell()).unwrap();
        let clip = Clip::load(&path).unwrap();
        assert!((clip.seconds() - 2.0).abs() < 0.01, "{}", clip.seconds());
        assert_eq!(clip.peaks.len(), 160);
        let song = Song::load(&path).unwrap();
        assert!((song.seconds() - 2.0).abs() < 0.01, "{}", song.seconds());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn bad_files_explain_themselves() {
        let dir = scratch("bad");
        let path = dir.join("notes.mp3");
        std::fs::write(&path, b"this is not audio").unwrap();
        let err = Clip::load(&path).err().unwrap();
        assert!(err.contains("isn't a sound file"), "{err}");
        let err = Clip::load(&dir.join("gone.mp3")).err().unwrap();
        assert!(err.contains("missing"), "{err}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn evening_out_matches_loud_and_quiet_sounds() {
        let loud = Clip::mono(crate::synth::thunder());
        let quiet = Clip::mono(crate::synth::thunder().iter().map(|s| s * 0.3).collect());
        let ratio = quiet.even_gain / loud.even_gain;
        assert!(
            (ratio - 1.0 / 0.3).abs() < 0.1,
            "a sound 10.5 dB quieter should get 10.5 dB more gain, got x{ratio:.2} ({} vs {})",
            quiet.even_gain,
            loud.even_gain
        );
        // A short blip can't be measured for loudness, so its peak goes to -6 dB.
        let blip = Clip::mono(vec![0.25; 2000]);
        assert!((blip.even_gain - 2.0).abs() < 0.01, "{}", blip.even_gain);
    }

    #[test]
    fn silence_at_the_start_can_be_skipped() {
        let mut samples = vec![0.0; 22_050];
        samples.extend(crate::synth::doorbell());
        let clip = Clip::mono(samples);
        let (from, to) = clip.range(true, 0.0, 0.0);
        let skipped = from as f32 / clip.rate();
        assert!((0.49..0.5).contains(&skipped), "skipped {skipped} s of a 0.5 s silence");
        assert_eq!(to, clip.frames());
        assert_eq!(clip.range(false, 0.0, 0.0).0, 0);
        let (from, to) = clip.range(false, 1.0, 1.5);
        assert_eq!((from, to), (44_100, 66_150));
        // Nonsense trims still leave something to play.
        let (from, to) = clip.range(false, 99.0, 1.0);
        assert!(to > from);
    }

    #[test]
    fn a_fade_at_the_end_ramps_to_nothing() {
        let samples = ClipSamples::new(vec![1.0; 2000].into(), 2, (0, 1000), 100);
        let out: Vec<f32> = samples.collect();
        assert_eq!(out.len(), 2000);
        assert_eq!(out[1799], 1.0);
        assert!(out[1999] < 0.02);
        assert!(out[1800..].windows(2).all(|w| w[1] <= w[0]));
    }

    #[test]
    fn a_song_section_loops_without_clicks() {
        let dir = scratch("loop");
        let path = dir.join("calm.wav");
        crate::synth::write_wav(&path, &vec![0.5; 44_100]).unwrap();
        let song = Song::load(&path).unwrap();
        let range = song.range(0.25, 0.75);
        assert_eq!(range, (11_025, 33_075));
        let section = Section::new(&song, range, true).unwrap();
        let out: Vec<f32> = section.take(22_050 * 3).collect();
        assert_eq!(out.len(), 22_050 * 3, "a repeating section must keep going");
        let biggest_jump = out.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(biggest_jump < 0.01, "the loop point jumps by {biggest_jump}");
        assert!(out[11_000] > 0.49, "the middle of the section is at full level");
        let once = Section::new(&song, range, false).unwrap();
        assert_eq!(once.count(), 22_050, "a section that doesn't repeat plays once");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn plays_to_the_end() {
        let (source, voice) = voice(500);
        assert_eq!(source.count(), 1000);
        assert!(voice.ended());
        assert_eq!(voice.progress(), 1.0);
    }

    #[test]
    fn fade_ramps_down_and_ends_early() {
        let (mut source, mut voice) = voice(10_000);
        for _ in 0..200 {
            source.next();
        }
        voice.fade_out(0.5);
        let rest: Vec<f32> = source.collect();
        let frames = rest.len() / 2;
        assert!((497..=502).contains(&frames), "fade took {frames} ms instead of 500");
        assert!(rest.windows(2).all(|w| w[1] <= w[0] + 1e-6), "gain went back up while fading");
        assert!(rest[0] > 0.99 && *rest.last().unwrap() < 0.01);
        assert!(voice.ended());
    }

    #[test]
    fn fade_in_rises_from_silence() {
        let ctl = Control::new(0.0, 0.8, 0.8 / 0.5);
        let samples = ClipSamples::new(vec![1.0; 4000].into(), 2, (0, 2000), 0);
        let mut source = VoiceSource::new(samples, NonZero::new(2).unwrap(), NonZero::new(1000).unwrap(), ctl);
        let first = source.next().unwrap();
        assert!(first < 0.01, "a fade-in started at {first}");
        let halfway = source.by_ref().nth(499).unwrap();
        assert!((halfway - 0.4).abs() < 0.01, "a quarter second in it was at {halfway}");
        let settled = source.by_ref().nth(999).unwrap();
        assert!((settled - 0.8).abs() < 1e-3, "it settled at {settled}");
    }

    #[test]
    fn looping_progress_wraps() {
        let (_, mut voice) = voice(100);
        voice.looping = true;
        voice.ctl.frame.store(250, Relaxed);
        assert_eq!(voice.progress(), 0.5);
        assert_eq!(voice.position(), (0.05, 2));
        assert!((voice.remaining() - 0.05).abs() < 1e-6);
    }

    #[test]
    fn stop_is_fast_but_not_a_click() {
        let (mut source, mut voice) = voice(10_000);
        source.next();
        voice.fade_out(0.05);
        let frames = source.count() / 2;
        assert!((45..=52).contains(&frames), "stop took {frames} ms");
    }

    #[test]
    fn level_changes_glide() {
        let (mut source, voice) = voice(10_000);
        voice.set_level(0.5);
        let first = source.next().unwrap();
        assert!(first > 0.95, "level jumped straight to {first}");
        let settled = source.by_ref().nth(200).unwrap();
        assert!((settled - 0.5).abs() < 1e-3, "level settled at {settled}");
    }

    #[test]
    fn bus_outlives_its_voices_and_applies_master() {
        let (voices, mix) = mixer::mixer(NonZero::new(2).unwrap(), NonZero::new(1000).unwrap());
        let master = Arc::new(AtomicU32::new(0.5f32.to_bits()));
        let mut bus = Bus { voices: mix, master, gain: 0.5, step: 1.0, channels: 2, channel: 0 };
        assert_eq!(bus.next(), Some(0.0), "an idle bus must keep producing silence");
        voices.add(voice(100).0);
        let loud = bus.by_ref().take(400).fold(0.0f32, |m, s| m.max(s));
        assert!((loud - 0.5).abs() < 1e-3, "master 50% gave a peak of {loud}");
        assert!(bus.take(1000).all(|s| s == 0.0), "bus should be silent after the voice ends");
    }

    #[test]
    fn a_silent_engine_says_why() {
        let mut engine = Engine::silent();
        engine.problem = Some("No speakers".into());
        let clip = Clip::mono(crate::synth::test_chime());
        let err = engine.play(1, &clip, (0, 10), 1.0, 0.0).err().unwrap();
        assert_eq!(err, "No speakers");
    }
}
