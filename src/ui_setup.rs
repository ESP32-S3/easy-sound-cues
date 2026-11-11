//! "Set up": the list of sounds with an editor for the chosen one, the background music, the
//! script, and the show's own settings. Every field says what it's for, right under it.

use crate::app::{Act, App, Loaded, PickFor, Purpose, SetupTab, Target};
use crate::show::Music;
use crate::style::{self, bold, caps, card, hint, length, p, strong};
use crate::ui_run::{music_text, when_text};
use eframe::egui::{
    self, Align, CornerRadius, CursorIcon, DragValue, Frame, Layout, Margin, RichText, ScrollArea, Sense, Slider,
    Stroke, TextEdit, UiBuilder, vec2,
};

const EDITOR_WIDTH: f32 = 680.0;

/// A labelled section of the editor.
fn section(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    let pal = p();
    card(pal.card, pal.line).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(strong(title, 18.0));
        ui.add_space(2.0);
        add(ui);
    });
    ui.add_space(10.0);
}

fn seconds_box(ui: &mut egui::Ui, value: &mut f32, max: f32) -> egui::Response {
    ui.add(DragValue::new(value).range(0.0..=max.max(0.1)).speed(0.05).max_decimals(2).suffix(" s"))
}

fn percent(v: f32) -> String {
    format!("{}%", (v * 100.0).round() as i32)
}

impl App {
    pub(crate) fn setup_view(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let pal = p();
        egui::Panel::top("setup tabs")
            .frame(Frame::new().fill(pal.bg).inner_margin(Margin { left: 16, right: 16, top: 10, bottom: 6 }))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (tab, label) in [
                        (SetupTab::Sounds, "🔊 Sounds"),
                        (SetupTab::Music, "♪ Background music"),
                        (SetupTab::Script, "📝 Script"),
                        (SetupTab::Show, "⚙ Show settings"),
                    ] {
                        ui.selectable_value(&mut self.setup_tab, tab, RichText::new(label).size(17.0));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let undo = ui
                            .add_enabled(self.can_undo(), egui::Button::new(RichText::new("⟲ Undo").size(15.0)))
                            .on_hover_text(
                                "Take back the last change to the list (adding, removing or moving a sound). Ctrl+Z",
                            );
                        if undo.clicked() {
                            acts.push(Act::Undo);
                        }
                    });
                });
            });
        match self.setup_tab {
            SetupTab::Sounds => {
                egui::Panel::left("cue list")
                    .resizable(true)
                    .default_size(380.0)
                    .size_range(300.0..=560.0)
                    .frame(Frame::new().fill(pal.panel).inner_margin(Margin::same(14)))
                    .show(ui, |ui| self.cue_list(ui, acts));
                egui::CentralPanel::default().frame(Frame::new().fill(pal.bg).inner_margin(Margin::same(18))).show(
                    ui,
                    |ui| {
                        ScrollArea::vertical().id_salt("cue editor").auto_shrink([false, false]).show(ui, |ui| {
                            ui.set_max_width(EDITOR_WIDTH);
                            self.cue_editor(ui, acts);
                        });
                    },
                );
            }
            SetupTab::Music => {
                egui::Panel::left("song list")
                    .resizable(true)
                    .default_size(360.0)
                    .size_range(300.0..=520.0)
                    .frame(Frame::new().fill(pal.panel).inner_margin(Margin::same(14)))
                    .show(ui, |ui| self.song_list(ui, acts));
                egui::CentralPanel::default().frame(Frame::new().fill(pal.bg).inner_margin(Margin::same(18))).show(
                    ui,
                    |ui| {
                        ScrollArea::vertical().id_salt("song editor").auto_shrink([false, false]).show(ui, |ui| {
                            ui.set_max_width(EDITOR_WIDTH);
                            self.song_editor(ui, acts);
                        });
                    },
                );
            }
            SetupTab::Script => self.script_editor(ui),
            SetupTab::Show => {
                egui::CentralPanel::default().frame(Frame::new().fill(pal.bg).inner_margin(Margin::same(18))).show(
                    ui,
                    |ui| {
                        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                            ui.set_max_width(EDITOR_WIDTH);
                            self.show_settings(ui, acts);
                        });
                    },
                );
            }
        }
    }

    fn cue_list(&self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let pal = p();
        ui.label(caps("Add a sound", pal.accent));
        let width = ui.available_width();
        if ui
            .add(
                egui::Button::new(RichText::new("📁  From a file on this computer").size(16.0))
                    .min_size(vec2(width, 38.0)),
            )
            .on_hover_text("Pick MP3, WAV, M4A, OGG or FLAC files. You can pick several at once.")
            .clicked()
        {
            acts.push(Act::AddFiles);
        }
        if ui
            .add(
                egui::Button::new(RichText::new("🎵  One I already downloaded").size(16.0)).min_size(vec2(width, 38.0)),
            )
            .on_hover_text("A list of sound files already on this computer: your downloads, and sounds in your shows")
            .clicked()
        {
            acts.push(Act::Pick(PickFor::NewCue));
        }
        if ui
            .add(egui::Button::new(RichText::new("⬇  From a YouTube video").size(16.0)).min_size(vec2(width, 38.0)))
            .on_hover_text("Paste a YouTube link and the app makes an MP3 of it")
            .clicked()
        {
            acts.push(Act::YouTubeFor(Target::Purpose, Purpose::Effect));
        }
        if ui
            .add(
                egui::Button::new(RichText::new("➕  An empty one (fill in later)").size(16.0))
                    .min_size(vec2(width, 38.0)),
            )
            .on_hover_text("A sound with no file yet. Also useful for a moment that only changes the music.")
            .clicked()
        {
            acts.push(Act::AddEmpty);
        }
        ui.add_space(8.0);
        ui.separator();
        let cues = self.cues();
        ui.label(caps(&format!("The show's sounds, in order ({})", cues.len()), pal.muted));
        ScrollArea::vertical().id_salt("cue list").auto_shrink([false, false]).show(ui, |ui| {
            if cues.is_empty() {
                ui.add_space(10.0);
                ui.label(
                    RichText::new("No sounds yet. Use the buttons above to add the first one.")
                        .size(15.0)
                        .color(pal.muted),
                );
            }
            for (i, cue) in cues.iter().enumerate() {
                let selected = self.selected == Some(cue.id);
                let problem = match self.clip_of(cue) {
                    Some(Loaded::Failed(_)) => Some("⚠ Its file is missing or broken"),
                    None if cue.music == Music::Keep => Some("⚠ No sound file yet"),
                    _ => None,
                };
                let no_line = cue.line.trim().is_empty();
                let (fill, stroke) = if selected { (pal.card_hi, pal.accent) } else { (pal.card, pal.line) };
                let row = ui.scope_builder(UiBuilder::new().sense(Sense::click()), |ui| {
                    Frame::new()
                        .fill(fill)
                        .stroke(Stroke::new(if selected { 2.0 } else { 1.0 }, stroke))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(Margin::symmetric(10, 7))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal_top(|ui| {
                                ui.label(
                                    RichText::new(format!("{}", i + 1)).family(bold()).size(18.0).color(pal.muted),
                                );
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 1.0;
                                    ui.add(egui::Label::new(strong(&cue.name, 16.0)).truncate());
                                    let (when, line) = when_text(cue);
                                    let noted = no_line && !cue.note.trim().is_empty();
                                    let preview = match line {
                                        Some(l) => format!("“{}”", l.lines().last().unwrap_or("").trim()),
                                        None if noted => cue.note.trim().to_string(),
                                        None => when,
                                    };
                                    let color = if no_line && !noted { pal.warn } else { pal.muted };
                                    ui.add(egui::Label::new(RichText::new(preview).size(13.5).color(color)).truncate());
                                    if let Some(m) = music_text(self, i) {
                                        ui.label(RichText::new(m).size(13.5).color(pal.music));
                                    }
                                    if let Some(problem) = problem {
                                        ui.label(RichText::new(problem).size(13.5).color(pal.stop));
                                    }
                                    if selected {
                                        ui.add_space(4.0);
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new("Move it:").size(14.0).color(pal.muted));
                                            let up = ui
                                                .add_enabled(i > 0, egui::Button::new("⏶ Earlier"))
                                                .on_hover_text("Move it earlier in the show");
                                            if up.clicked() {
                                                acts.push(Act::MoveCue(cue.id, -1));
                                            }
                                            let down = ui
                                                .add_enabled(i + 1 < cues.len(), egui::Button::new("⏷ Later"))
                                                .on_hover_text("Move it later in the show");
                                            if down.clicked() {
                                                acts.push(Act::MoveCue(cue.id, 1));
                                            }
                                        });
                                    }
                                });
                            });
                        });
                });
                if row.response.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    acts.push(Act::Select(cue.id));
                }
                ui.add_space(4.0);
            }
        });
    }

    fn cue_editor(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let pal = p();
        let Some(open) = &self.open else { return };
        let Some(i) = self.selected.and_then(|id| open.show.cue_index(id)) else {
            ui.add_space(30.0);
            ui.label(strong("Add your first sound", 24.0));
            hint(ui, "Use the buttons on the left: pick a sound file from this computer, or get one from YouTube.");
            hint(ui, "Then, for each sound, type the line from the script that it comes right after.");
            return;
        };
        // Everything the editor needs to know about the rest of the show, gathered before the cue
        // itself is borrowed for editing.
        let cue = &open.show.cues[i];
        let id = cue.id;
        let clip = self.clip_of(cue);
        let playhead = self.voice(id).map(|v| v.progress());
        let songs: Vec<(u32, String)> = open.show.music.iter().map(|s| (s.id, s.name.clone())).collect();
        let has_script = !open.show.script.trim().is_empty();
        let found = open.places.get(i).and_then(|p| p.lines).is_some();
        let count = open.show.cues.len();
        let open = self.open.as_mut().expect("checked above");
        let cue = &mut open.show.cues[i];

        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Sound {} of {count}", i + 1)).size(15.0).color(pal.muted));
        });
        section(ui, "Name", |ui| {
            ui.add(
                TextEdit::singleline(&mut cue.name).desired_width(f32::INFINITY).font(egui::FontId::proportional(18.0)),
            );
            hint(ui, "A short name so you know which sound it is, like “Doorbell”.");
        });

        section(ui, "The sound", |ui| {
            match &clip {
                Some(Loaded::Ready(c)) => {
                    let file = std::path::Path::new(&cue.file)
                        .file_name()
                        .map(|f| f.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    ui.label(RichText::new(format!("✔ {file}  ·  {}", length(c.seconds()))).size(15.0).color(pal.good));
                    let (from, to) = c.range(cue.skip_silence, cue.start, cue.end);
                    let frames = c.frames().max(1) as f32;
                    let plays = (from as f32 / frames, to as f32 / frames);
                    let head = playhead.map(|ph| plays.0 + ph * (plays.1 - plays.0));
                    style::waveform(ui, &c.peaks, vec2(ui.available_width(), 64.0), plays, head);
                    if (plays.0, plays.1) != (0.0, 1.0) {
                        hint(ui, &format!("The bright part plays: {:.1} s of it.", (to - from) as f32 / c.rate()));
                    }
                }
                Some(Loaded::Loading) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Loading the sound…");
                    });
                }
                Some(Loaded::Failed(e)) => {
                    ui.label(RichText::new(format!("⚠ {e}")).size(15.0).color(pal.stop));
                    hint(ui, "Choose the file again, or get it again from YouTube.");
                }
                None => {
                    ui.label(RichText::new("No sound file yet.").size(15.0).color(pal.warn));
                    hint(
                        ui,
                        "That's fine for a moment that only changes the music. Otherwise, choose a file or get one from YouTube.",
                    );
                }
            }
            ui.horizontal_wrapped(|ui| {
                if playhead.is_some() {
                    if style::button(ui, "■ Stop", None).clicked() {
                        acts.push(Act::StopCue(id));
                    }
                } else if ui
                    .add_enabled(
                        matches!(clip, Some(Loaded::Ready(_))),
                        egui::Button::new(RichText::new("▶ Listen").size(16.0)).min_size(vec2(0.0, 34.0)),
                    )
                    .on_hover_text("Hear it, with its volume and start/end settings. The show doesn't move on.")
                    .clicked()
                {
                    acts.push(Act::Listen(id));
                }
                if style::button(ui, "🎵 One I already have", None)
                    .on_hover_text("Pick from sound files already on this computer")
                    .clicked()
                {
                    acts.push(Act::Pick(PickFor::Cue(id)));
                }
                if style::button(ui, "📁 Choose a file…", None).clicked() {
                    acts.push(Act::ChooseFile(id));
                }
                if style::button(ui, "⬇ Get it from YouTube", None).clicked() {
                    acts.push(Act::YouTubeFor(Target::ReplaceCue(id), Purpose::Effect));
                }
            });
        });

        section(ui, "When should it play?", |ui| {
            hint(
                ui,
                "Copy the words from the script that come just before the sound. When the actor says them, whoever runs the sound presses GO.",
            );
            ui.horizontal(|ui| {
                ui.label("Who says them:");
                ui.add(TextEdit::singleline(&mut cue.who).hint_text("e.g. Narrator (optional)").desired_width(240.0));
            });
            ui.label("The words:");
            ui.add(
                TextEdit::multiline(&mut cue.line)
                    .hint_text("e.g. …and then someone rang the doorbell.")
                    .desired_rows(2)
                    .desired_width(f32::INFINITY)
                    .font(egui::FontId::proportional(17.0)),
            );
            if has_script && !cue.line.trim().is_empty() {
                if found {
                    ui.label(
                        RichText::new("✔ Found in your script. It will show up right there when you run the show.")
                            .size(14.0)
                            .color(pal.good),
                    );
                } else {
                    ui.label(RichText::new("⚠ These words weren't found in your script (after the sound before this one). Check the spelling, or copy them from the Script tab.").size(14.0).color(pal.warn));
                }
            }
            ui.label("Notes for whoever runs the sound (optional):");
            ui.add(
                TextEdit::multiline(&mut cue.note)
                    .hint_text("e.g. Wait for the door to open first.")
                    .desired_rows(1)
                    .desired_width(f32::INFINITY),
            );
        });

        section(ui, "Background music", |ui| {
            hint(ui, "What should happen to the background music when this sound plays?");
            let label = |m: Music| match m {
                Music::Keep => "Leave the music as it is".to_string(),
                Music::Start { song } => {
                    format!(
                        "Start “{}”",
                        songs.iter().find(|s| s.0 == song).map_or("(removed music)", |s| s.1.as_str())
                    )
                }
                Music::Fade => "Fade the music out gently".to_string(),
                Music::Stop => "Stop the music right away".to_string(),
            };
            egui::ComboBox::from_id_salt(("music", id))
                .selected_text(RichText::new(label(cue.music)).size(16.0))
                .width(380.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut cue.music, Music::Keep, label(Music::Keep));
                    for (song, _) in &songs {
                        ui.selectable_value(
                            &mut cue.music,
                            Music::Start { song: *song },
                            label(Music::Start { song: *song }),
                        );
                    }
                    ui.selectable_value(&mut cue.music, Music::Fade, label(Music::Fade));
                    ui.selectable_value(&mut cue.music, Music::Stop, label(Music::Stop));
                });
            if songs.is_empty() {
                hint(ui, "This show has no background music yet. Add some under “♪ Background music” at the top.");
            }
        });

        section(ui, "Volume", |ui| {
            ui.horizontal(|ui| {
                ui.add(Slider::new(&mut cue.volume, 0.0..=2.0).show_value(false));
                ui.label(RichText::new(percent(cue.volume)).size(15.0));
                if ui.small_button("Reset to 100%").clicked() {
                    cue.volume = 1.0;
                }
            });
            ui.checkbox(&mut cue.even_out, "Make it about as loud as the other sounds (recommended)");
            hint(
                ui,
                "Sounds from the internet are all different volumes. With this ticked, the app evens them out first, and the slider adjusts from there.",
            );
        });

        let max = match &clip {
            Some(Loaded::Ready(c)) => c.seconds(),
            _ => 600.0,
        };
        section(ui, "Start and end", |ui| {
            ui.checkbox(&mut cue.skip_silence, "Skip any silence at the start, so it plays the instant GO is pressed");
            egui::Grid::new(("trim", id)).num_columns(3).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label("Start at");
                seconds_box(ui, &mut cue.start, max);
                ui.label(RichText::new("seconds in (0 = the beginning)").color(pal.muted));
                ui.end_row();
                ui.label("Stop at");
                seconds_box(ui, &mut cue.end, max);
                ui.label(RichText::new("seconds in (0 = play to the end)").color(pal.muted));
                ui.end_row();
                ui.label("Fade out over");
                seconds_box(ui, &mut cue.fade_out, 30.0);
                ui.label(RichText::new("seconds at the end (0 = no fade)").color(pal.muted));
                ui.end_row();
            });
            hint(ui, "Drag the numbers left and right, or click and type.");
        });

        ui.add_space(4.0);
        let name = cue.name.clone();
        if style::danger(ui, "🗑  Remove this sound from the show").clicked() {
            acts.push(Act::Confirm(
                format!(
                    "Remove “{name}” from the show? (Its file stays in the show's folder, and Undo brings it back.)"
                ),
                "Yes, remove it".into(),
                Box::new(Act::RemoveCue(id)),
            ));
        }
        ui.add_space(40.0);
    }

    fn song_list(&self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let pal = p();
        let Some(open) = &self.open else { return };
        hint(
            ui,
            "Background music plays quietly under the scene and repeats until a sound stops it. Pick which sound starts each song in the Sounds list.",
        );
        ui.label(caps("Add music", pal.music));
        let width = ui.available_width();
        if ui
            .add(
                egui::Button::new(RichText::new("📁  From a file on this computer").size(16.0))
                    .min_size(vec2(width, 38.0)),
            )
            .clicked()
        {
            acts.push(Act::AddMusicFiles);
        }
        if ui
            .add(
                egui::Button::new(RichText::new("🎵  One I already downloaded").size(16.0)).min_size(vec2(width, 38.0)),
            )
            .clicked()
        {
            acts.push(Act::Pick(PickFor::NewSong));
        }
        if ui
            .add(egui::Button::new(RichText::new("⬇  From a YouTube video").size(16.0)).min_size(vec2(width, 38.0)))
            .clicked()
        {
            acts.push(Act::YouTubeFor(Target::Purpose, Purpose::Music));
        }
        ui.add_space(8.0);
        ui.separator();
        ScrollArea::vertical().id_salt("song list").auto_shrink([false, false]).show(ui, |ui| {
            if open.show.music.is_empty() {
                ui.label(RichText::new("No music yet.").size(15.0).color(pal.muted));
            }
            for song in &open.show.music {
                let selected = self.selected_song == Some(song.id);
                let starts: Vec<String> = open
                    .show
                    .cues
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.music == (Music::Start { song: song.id }))
                    .map(|(i, _)| format!("{}", i + 1))
                    .collect();
                let (fill, stroke) = if selected { (pal.card_hi, pal.music) } else { (pal.card, pal.line) };
                let row = ui.scope_builder(UiBuilder::new().sense(Sense::click()), |ui| {
                    Frame::new()
                        .fill(fill)
                        .stroke(Stroke::new(if selected { 2.0 } else { 1.0 }, stroke))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(Margin::symmetric(10, 7))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(strong(format!("♪ {}", song.name), 16.0));
                            let text = if starts.is_empty() {
                                "No sound starts it yet".to_string()
                            } else {
                                format!("Started by sound {}", starts.join(", "))
                            };
                            ui.label(RichText::new(text).size(13.5).color(if starts.is_empty() {
                                pal.warn
                            } else {
                                pal.muted
                            }));
                        });
                });
                if row.response.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    acts.push(Act::SelectSong(song.id));
                }
                ui.add_space(4.0);
            }
        });
    }

    fn song_editor(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let pal = p();
        let Some(open) = &self.open else { return };
        let Some(i) = self.selected_song.and_then(|id| open.show.music.iter().position(|s| s.id == id)) else {
            ui.add_space(30.0);
            ui.label(strong("No background music yet", 24.0));
            hint(ui, "Music is optional. If you want some, add a song with the buttons on the left.");
            return;
        };
        let song = &open.show.music[i];
        let id = song.id;
        let loaded = self.song_of(song);
        let playing = self.music.iter().find(|v| v.id == id && !v.fading).map(|v| v.progress());
        let open = self.open.as_mut().expect("checked above");
        let song = &mut open.show.music[i];

        section(ui, "Name", |ui| {
            ui.add(
                TextEdit::singleline(&mut song.name)
                    .desired_width(f32::INFINITY)
                    .font(egui::FontId::proportional(18.0)),
            );
        });
        section(ui, "The music", |ui| {
            match &loaded {
                Some(Loaded::Ready(s)) => {
                    let file = std::path::Path::new(&song.file)
                        .file_name()
                        .map(|f| f.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    ui.label(RichText::new(format!("✔ {file}  ·  {}", length(s.seconds()))).size(15.0).color(pal.good));
                    if !s.peaks.is_empty() {
                        let (from, to) = s.range(song.start, song.end);
                        let total = (s.seconds() * s.rate()).max(1.0);
                        let plays = (from as f32 / total, to as f32 / total);
                        let head = playing.map(|ph| plays.0 + ph * (plays.1 - plays.0));
                        style::waveform(ui, &s.peaks, vec2(ui.available_width(), 64.0), plays, head);
                    }
                }
                Some(Loaded::Loading) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Loading the music…");
                    });
                }
                Some(Loaded::Failed(e)) => {
                    ui.label(RichText::new(format!("⚠ {e}")).size(15.0).color(pal.stop));
                }
                None => {
                    ui.label(RichText::new("No music file yet.").size(15.0).color(pal.warn));
                }
            }
            ui.horizontal_wrapped(|ui| {
                if playing.is_some() {
                    if style::button(ui, "■ Stop", None).clicked() {
                        acts.push(Act::StopCue(id));
                    }
                } else if ui
                    .add_enabled(
                        matches!(loaded, Some(Loaded::Ready(_))),
                        egui::Button::new(RichText::new("▶ Listen").size(16.0)).min_size(vec2(0.0, 34.0)),
                    )
                    .clicked()
                {
                    acts.push(Act::ListenSong(id));
                }
                if style::button(ui, "🎵 One I already have", None).clicked() {
                    acts.push(Act::Pick(PickFor::Song(id)));
                }
                if style::button(ui, "📁 Choose a file…", None).clicked() {
                    acts.push(Act::ChooseSongFile(id));
                }
                if style::button(ui, "⬇ Get it from YouTube", None).clicked() {
                    acts.push(Act::YouTubeFor(Target::ReplaceSong(id), Purpose::Music));
                }
            });
        });
        section(ui, "Volume", |ui| {
            ui.horizontal(|ui| {
                ui.add(Slider::new(&mut song.volume, 0.0..=2.0).show_value(false));
                ui.label(RichText::new(percent(song.volume)).size(15.0));
            });
            ui.checkbox(&mut song.even_out, "Make it about as loud as the other music (recommended)");
            hint(ui, "The show's overall music volume is on the “Run the show” page and in Show settings.");
        });
        let max = match &loaded {
            Some(Loaded::Ready(s)) => s.seconds(),
            _ => 3600.0,
        };
        section(ui, "Which part plays", |ui| {
            ui.checkbox(&mut song.repeat, "Start again from the beginning when it ends (keep going until stopped)");
            egui::Grid::new(("song trim", id)).num_columns(3).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label("From");
                seconds_box(ui, &mut song.start, max);
                ui.label(RichText::new("seconds in (0 = the beginning)").color(pal.muted));
                ui.end_row();
                ui.label("To");
                seconds_box(ui, &mut song.end, max);
                ui.label(RichText::new("seconds in (0 = the end)").color(pal.muted));
                ui.end_row();
            });
            hint(ui, "Handy when only part of a song fits the mood. Leave both at 0 to use the whole song.");
        });
        let name = song.name.clone();
        if style::danger(ui, "🗑  Remove this music from the show").clicked() {
            acts.push(Act::Confirm(
                format!(
                    "Remove “{name}”? Sounds that started it will leave the music alone instead. Undo brings it back."
                ),
                "Yes, remove it".into(),
                Box::new(Act::RemoveSong(id)),
            ));
        }
        ui.add_space(40.0);
    }

    fn script_editor(&mut self, ui: &mut egui::Ui) {
        let pal = p();
        let Some(open) = &mut self.open else { return };
        egui::Panel::right("script report")
            .resizable(false)
            .exact_size(330.0)
            .frame(Frame::new().fill(pal.panel).inner_margin(Margin::same(16)))
            .show(ui, |ui| {
                ScrollArea::vertical().id_salt("script report").show(ui, |ui| {
                    ui.label(strong("How to write it", 18.0));
                    for (example, meaning) in [
                        ("NARRATOR: Once upon a time…", "Someone speaking: their name, a colon, then the words."),
                        ("(She opens the door.)", "Stage directions go in brackets."),
                        ("# Scene 2", "A line starting with # is a heading."),
                    ] {
                        ui.label(RichText::new(example).monospace().color(pal.text));
                        hint(ui, meaning);
                        ui.add_space(2.0);
                    }
                    ui.add_space(6.0);
                    ui.separator();
                    ui.label(strong("Sounds in the script", 18.0));
                    if open.show.script.trim().is_empty() {
                        hint(ui, "Paste a script and this list shows which sounds were found in it.");
                    } else {
                        for (i, cue) in open.show.cues.iter().enumerate() {
                            let found = open.places.get(i).and_then(|p| p.lines).is_some();
                            let (mark, color, why) = if cue.line.trim().is_empty() {
                                ("•", pal.muted, "no line set: it goes right after the one before")
                            } else if found {
                                ("✔", pal.good, "found")
                            } else {
                                ("⚠", pal.warn, "its words weren't found")
                            };
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(format!("{mark} {}. {}", i + 1, cue.name)).color(color));
                                ui.label(RichText::new(why).size(13.0).color(pal.muted));
                            });
                        }
                    }
                });
            });
        egui::CentralPanel::default()
            .frame(Frame::new().fill(pal.bg).inner_margin(Margin::same(18)))
            .show(ui, |ui| {
                ui.label(strong("The script (optional)", 22.0));
                hint(ui, "Paste your play's script here. The app finds each sound's words in it, and when you run the show it shows each sound right where it happens, so whoever runs the sound can follow along.");
                ui.add_space(6.0);
                ScrollArea::vertical().id_salt("script text").auto_shrink([false, false]).show(ui, |ui| {
                    ui.add(
                        TextEdit::multiline(&mut open.show.script)
                            .hint_text("Paste the script here (Ctrl+V, or ⌘+V on a Mac)")
                            .font(egui::FontId::proportional(16.0))
                            .desired_rows(28)
                            .desired_width(f32::INFINITY),
                    );
                });
            });
    }

    fn show_settings(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let pal = p();
        let Some(open) = &mut self.open else { return };
        section(ui, "The show's name", |ui| {
            ui.add(
                TextEdit::singleline(&mut open.show.title)
                    .desired_width(f32::INFINITY)
                    .font(egui::FontId::proportional(18.0)),
            );
        });
        section(ui, "Fades", |ui| {
            egui::Grid::new("fades").num_columns(3).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label("One song fades into the next over");
                seconds_box(ui, &mut open.show.crossfade, 20.0);
                ui.label(RichText::new("seconds").color(pal.muted));
                ui.end_row();
                ui.label("“Fade everything out” takes");
                seconds_box(ui, &mut open.show.fade_all, 20.0);
                ui.label(RichText::new("seconds").color(pal.muted));
                ui.end_row();
            });
        });
        section(ui, "Music volume", |ui| {
            ui.horizontal(|ui| {
                let r = ui.add(Slider::new(&mut open.show.music_volume, 0.0..=1.0).show_value(false));
                ui.label(RichText::new(percent(open.show.music_volume)).size(15.0));
                if r.changed() {
                    acts.push(Act::MusicVolume(open.show.music_volume));
                }
            });
            hint(
                ui,
                "How loud background music plays under everything. Start low (about 40%) so the actors can be heard.",
            );
        });
        section(ui, "Where it's saved", |ui| {
            ui.label(RichText::new(crate::paths::friendly(&open.dir)).monospace());
            hint(
                ui,
                "Everything is saved automatically as you go: the show and copies of all its sounds live in this folder.",
            );
            ui.horizontal_wrapped(|ui| {
                if style::button(ui, "🗁 Show the folder", None).clicked() {
                    acts.push(Act::RevealShow);
                }
                if style::button(ui, "📦 Save a copy for another computer…", None)
                    .on_hover_text("Makes one .zip file with everything inside. Open it on the other computer with “Open a show someone gave me”.")
                    .clicked()
                {
                    acts.push(Act::ExportShow);
                }
            });
        });
    }
}
