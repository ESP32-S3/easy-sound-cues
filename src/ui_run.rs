//! "Run the show": the next sound with its line, one big GO button, a big STOP button, and the
//! script (or the list of sounds) following along.

use crate::app::{Act, App, Loaded};
use crate::show::{Cue, LineKind, Music};
use crate::style::{self, bold, caps, card, mmss, p, strong, who_color};
use eframe::egui::{
    self, Align, CornerRadius, CursorIcon, Frame, Layout, Margin, RichText, ScrollArea, Sense, Slider, Stroke,
    UiBuilder, vec2,
};

/// How a cue's moment is described: "When NARRATOR says:" and the words.
pub(crate) fn when_text(cue: &Cue) -> (String, Option<String>) {
    let line = cue.line.trim();
    match (cue.who.trim(), line) {
        (_, "") if !cue.note.trim().is_empty() => ("When to press GO is in the note below.".into(), None),
        (_, "") => ("No line has been set for this sound.".into(), None),
        ("", _) => ("Press GO right after:".into(), Some(line.to_string())),
        (who, _) => (format!("Press GO when {who} says:"), Some(line.to_string())),
    }
}

/// What a cue does to the music, in a few words.
pub(crate) fn music_text(app: &App, i: usize) -> Option<String> {
    let open = app.open.as_ref()?;
    let cue = open.show.cues.get(i)?;
    match cue.music {
        Music::Keep => None,
        Music::Stop => Some("♪ The music stops dead".into()),
        Music::Fade => Some("♪ The music fades out".into()),
        Music::Start { song } => Some(match open.show.song_before(i) {
            Some(before) if before != song => {
                format!("♪ {} fades into {}", open.show.song_name(before), open.show.song_name(song))
            }
            _ => format!("♪ {} starts playing", open.show.song_name(song)),
        }),
    }
}

impl App {
    pub(crate) fn run_view(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let pal = p();
        let wide = ui.available_width() > 880.0;
        egui::Panel::left("run controls")
            .resizable(true)
            .default_size(if wide { 440.0 } else { 360.0 })
            .size_range(340.0..=620.0)
            .frame(Frame::new().fill(pal.panel).inner_margin(Margin::same(16)))
            .show(ui, |ui| {
                ScrollArea::vertical().id_salt("run controls").auto_shrink([false, false]).show(ui, |ui| {
                    self.next_card(ui, acts);
                    ui.add_space(10.0);
                    self.playing_now(ui, acts);
                    ui.add_space(10.0);
                    self.music_card(ui, acts);
                    ui.add_space(10.0);
                    self.volume_card(ui, acts);
                    ui.add_space(10.0);
                    keys_card(ui);
                });
            });
        egui::CentralPanel::default()
            .frame(Frame::new().fill(pal.bg).inner_margin(Margin { left: 20, right: 12, top: 12, bottom: 0 }))
            .show(ui, |ui| self.follow_along(ui, acts));
    }

    fn next_card(&self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let pal = p();
        let cues = self.cues();
        let total = cues.len();
        let next = cues.get(self.next);
        card(pal.card, if next.is_some() { pal.warn } else { pal.line })
            .stroke(Stroke::new(2.0, if next.is_some() { pal.warn } else { pal.line }))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                match next {
                    Some(cue) => {
                        ui.horizontal(|ui| {
                            ui.label(caps("Next sound", pal.warn));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(
                                    RichText::new(format!("{} of {total}", self.next + 1)).size(14.0).color(pal.muted),
                                );
                            });
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(format!("{}", self.next + 1)).family(bold()).size(30.0).color(pal.warn),
                            );
                            ui.label(strong(&cue.name, 26.0));
                        });
                        let (when, line) = when_text(cue);
                        ui.label(RichText::new(when).size(15.0).color(pal.muted));
                        if let Some(line) = line {
                            quote(ui, &line);
                        }
                        if let Some(m) = music_text(self, self.next) {
                            ui.label(RichText::new(m).family(bold()).size(15.0).color(pal.music));
                        }
                        if !cue.note.trim().is_empty() {
                            ui.label(RichText::new(format!("Note: {}", cue.note.trim())).size(15.0).color(pal.text));
                        }
                        if ui
                            .small_button("✏ Change this sound")
                            .on_hover_text("Open it in Set up: its file, its line, its volume")
                            .clicked()
                        {
                            acts.push(Act::EditCue(cue.id));
                        }
                        match self.clip_of(cue) {
                            Some(Loaded::Failed(e)) => {
                                ui.label(RichText::new(format!("⚠ {e}")).size(14.0).color(pal.stop));
                            }
                            None if cue.music == Music::Keep => {
                                ui.label(
                                    RichText::new("⚠ This one has no sound file yet. Add one in “Set up”.")
                                        .size(14.0)
                                        .color(pal.stop),
                                );
                            }
                            _ => {}
                        }
                    }
                    None if total == 0 => {
                        ui.label(strong("This show has no sounds yet", 22.0));
                        ui.label(style::muted("Go to “Set up” and press “Add a sound”."));
                        if style::primary(ui, "✏  Go to Set up", true).clicked() {
                            acts.push(Act::Tab(crate::app::Tab::Setup));
                        }
                    }
                    None => {
                        ui.label(caps("All done", pal.good));
                        ui.label(strong("That was the last sound!", 24.0));
                        ui.label(style::muted("To run the show again from sound 1, press “Start again”."));
                        if style::primary(ui, "↺  Start again", true).clicked() {
                            acts.push(Act::Restart);
                        }
                    }
                }
                ui.add_space(8.0);
                let can_go = next.is_some();
                let sub = if can_go { "Space bar" } else { "no more sounds" };
                let go = style::big_button(
                    ui,
                    "▶  GO",
                    Some(sub),
                    (pal.go, pal.go_hover, pal.go_ink),
                    vec2(ui.available_width(), 96.0),
                    38.0,
                    can_go,
                )
                .on_hover_text("Play the next sound. The one after it gets ready by itself.");
                if go.clicked() {
                    acts.push(Act::Go);
                }
                ui.add_space(4.0);
                ui.columns(2, |cols| {
                    let back = cols[0]
                        .add_enabled(
                            self.next > 0,
                            egui::Button::new(RichText::new("◀  Back one").size(15.0))
                                .min_size(vec2(cols[0].available_width(), 32.0)),
                        )
                        .on_hover_text(
                            "Make the sound before this one the next sound, without playing anything (Left arrow)",
                        );
                    if back.clicked() {
                        acts.push(Act::Back);
                    }
                    let skip = cols[1]
                        .add_enabled(
                            self.next < total,
                            egui::Button::new(RichText::new("Skip one  ▶").size(15.0))
                                .min_size(vec2(cols[1].available_width(), 32.0)),
                        )
                        .on_hover_text("Move on to the sound after this one without playing it (Right arrow)");
                    if skip.clicked() {
                        acts.push(Act::Skip);
                    }
                });
                ui.add_space(4.0);
                let stop = style::big_button(
                    ui,
                    "■  STOP ALL SOUND",
                    Some("Esc key"),
                    (pal.stop, pal.stop_hover, pal.stop_ink),
                    vec2(ui.available_width(), 58.0),
                    22.0,
                    true,
                )
                .on_hover_text("Silence everything at once: sounds and music");
                if stop.clicked() {
                    acts.push(Act::StopAll);
                }
                ui.add_space(2.0);
                let fade = ui
                    .add(
                        egui::Button::new(RichText::new("Fade everything out gently  (F)").size(15.0))
                            .min_size(vec2(ui.available_width(), 32.0)),
                    )
                    .on_hover_text("Turn everything down smoothly over a couple of seconds, music too");
                if fade.clicked() {
                    acts.push(Act::FadeAll);
                }
            });
    }

    fn playing_now(&self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let pal = p();
        let playing: Vec<_> = self.voices.iter().filter(|v| v.id != 0).collect();
        card(pal.card, pal.line).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(caps("Playing now", pal.accent));
            if playing.is_empty() {
                ui.label(style::muted("Nothing."));
            }
            for v in playing {
                let name = self.cues().iter().find(|c| c.id == v.id).map_or("Sound", |c| c.name.as_str());
                let color = if v.fading { pal.muted } else { pal.accent };
                ui.horizontal(|ui| {
                    let verb = if v.fading { "fading" } else { "▶" };
                    ui.label(RichText::new(format!("{verb}  {name}")).size(16.0).color(color));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if !v.fading && ui.small_button("■ Stop").on_hover_text("Stop just this sound").clicked() {
                            acts.push(Act::StopCue(v.id));
                        }
                        ui.label(
                            RichText::new(format!("{:.1} s left", v.remaining())).monospace().size(13.0).color(color),
                        );
                    });
                });
                style::bar(ui, v.progress(), color, 5.0);
            }
        });
    }

    fn music_card(&self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let pal = p();
        let Some(open) = &self.open else { return };
        let on = self.current_song().is_some();
        card(pal.card, if on { pal.music } else { pal.line }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(caps("♪ Background music", pal.music));
            if open.show.music.is_empty() {
                ui.label(style::muted("This show has no background music. You can add some in “Set up”, under Music."));
                if ui.button("✏ Add background music").clicked() {
                    acts.push(Act::Tab(crate::app::Tab::Setup));
                    acts.push(Act::EditSong(0));
                }
                return;
            }
            if self.music.is_empty() {
                let upcoming = (self.next..open.show.cues.len()).find_map(|i| match open.show.cues[i].music {
                    Music::Start { song } => Some((i, song)),
                    _ => None,
                });
                let text = match upcoming {
                    Some((i, song)) => {
                        format!("Quiet for now. “{}” starts with sound {}.", open.show.song_name(song), i + 1)
                    }
                    None => "Quiet.".to_string(),
                };
                ui.label(style::muted(text));
            }
            for v in &self.music {
                let color = if v.fading { pal.muted } else { pal.music };
                let (at, loops) = v.position();
                ui.horizontal(|ui| {
                    let verb = if v.fading { "fading" } else { "▶" };
                    ui.label(RichText::new(format!("{verb}  {}", open.show.song_name(v.id))).size(16.0).color(color));
                    if loops > 0 {
                        ui.label(RichText::new(format!("round {}", loops + 1)).size(13.0).color(pal.muted));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{} / {}", mmss(at), mmss(v.seconds())))
                                .monospace()
                                .size(13.0)
                                .color(color),
                        );
                    });
                });
                style::bar(ui, v.progress(), color, 5.0);
            }
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(on, egui::Button::new(RichText::new("Fade the music out  (M)").size(15.0))).clicked()
                {
                    acts.push(Act::FadeMusic);
                }
                ui.menu_button(RichText::new("▶ Play some music now…").size(15.0), |ui| {
                    ui.label(style::muted("Start one now (the next sound stays the same):"));
                    for song in &open.show.music {
                        if ui.button(RichText::new(format!("▶  {}", song.name)).size(15.0)).clicked() {
                            acts.push(Act::StartSong(song.id));
                            ui.close();
                        }
                    }
                });
            });
            let mut level = open.show.music_volume;
            ui.horizontal(|ui| {
                ui.label(RichText::new("Music volume").size(15.0));
                let r = ui
                    .add(Slider::new(&mut level, 0.0..=1.0).show_value(false))
                    .on_hover_text("Keep it low enough that the actors can still be heard");
                ui.label(RichText::new(format!("{}%", (level * 100.0).round() as i32)).size(14.0).color(pal.muted));
                if r.changed() {
                    acts.push(Act::MusicVolume(level));
                }
            });
        });
    }

    fn volume_card(&self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let pal = p();
        card(pal.card, pal.line).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let mut level = self.settings.volume;
            ui.horizontal(|ui| {
                ui.label(RichText::new("🔊 Main volume").size(15.0));
                let r = ui
                    .add(Slider::new(&mut level, 0.0..=1.0).show_value(false))
                    .on_hover_text("Everything, sounds and music together");
                ui.label(RichText::new(format!("{}%", (level * 100.0).round() as i32)).size(14.0).color(pal.muted));
                if r.changed() {
                    acts.push(Act::Volume(level));
                }
            });
            ui.horizontal(|ui| {
                if ui
                    .button("↺ Start again from sound 1")
                    .on_hover_text("Stop everything and make sound 1 the next sound")
                    .clicked()
                {
                    acts.push(Act::Confirm(
                        "Start the show again from sound 1? Anything playing will stop.".into(),
                        "Yes, start again".into(),
                        Box::new(Act::Restart),
                    ));
                }
            });
        });
    }

    /// The script with the sounds placed in it, or the plain list when there's no script.
    fn follow_along(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let pal = p();
        let has_script = self.open.as_ref().is_some_and(|o| !o.show.script.trim().is_empty());
        ui.horizontal_wrapped(|ui| {
            if has_script {
                ui.selectable_value(&mut self.show_list, false, RichText::new("📝 Script").size(16.0));
                ui.selectable_value(&mut self.show_list, true, RichText::new("☰ List of sounds").size(16.0));
                ui.add_space(12.0);
            } else {
                ui.label(caps("All the sounds, in order", pal.muted));
            }
            ui.checkbox(&mut self.follow, RichText::new("Keep the next sound in view").size(15.0));
            if ui.button("Find the next sound").clicked() {
                self.scroll_to_next = true;
            }
        });
        ui.add_space(4.0);
        ui.label(RichText::new("Click any sound to make it the next one.").size(14.0).color(pal.faint));
        ui.separator();
        ScrollArea::vertical().id_salt("follow").auto_shrink([false, false]).show(ui, |ui| {
            ui.set_max_width(ui.available_width().min(820.0));
            if has_script && !self.show_list {
                self.script_view(ui, acts);
            } else {
                for i in 0..self.cues().len() {
                    self.cue_row(ui, i, acts);
                }
            }
            ui.add_space(260.0);
        });
        self.scroll_to_next = false;
    }

    fn script_view(&self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let pal = p();
        let Some(open) = &self.open else { return };
        // Which cues come before each script line.
        let mut before: Vec<Vec<usize>> = vec![Vec::new(); open.lines.len() + 1];
        for (i, place) in open.places.iter().enumerate() {
            before[place.slot.min(open.lines.len())].push(i);
        }
        let next_lines = open.places.get(self.next).and_then(|p| p.lines);
        let done_lines: Vec<(usize, usize)> =
            open.places[..self.next.min(open.places.len())].iter().filter_map(|p| p.lines).collect();
        for (k, line) in open.lines.iter().enumerate() {
            for &i in &before[k] {
                self.cue_row(ui, i, acts);
            }
            let lit = next_lines.is_some_and(|(a, b)| (a..=b).contains(&k));
            let done = done_lines.iter().any(|&(a, b)| (a..=b).contains(&k));
            match line.kind {
                LineKind::Blank => {
                    ui.add_space(6.0);
                }
                LineKind::Heading => {
                    ui.add_space(8.0);
                    ui.label(strong(&line.text, 20.0).color(pal.accent));
                    ui.add_space(2.0);
                }
                LineKind::Direction => {
                    ui.label(RichText::new(&line.text).italics().size(15.0).color(pal.muted));
                }
                LineKind::Speech | LineKind::More | LineKind::Text => {
                    ui.horizontal_top(|ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(110.0, 20.0), Sense::hover());
                        if line.kind == LineKind::Speech {
                            ui.painter().text(
                                rect.right_top() + vec2(0.0, 3.0),
                                egui::Align2::RIGHT_TOP,
                                line.who.to_uppercase(),
                                egui::FontId::new(13.0, bold()),
                                who_color(&line.who),
                            );
                        }
                        let mut text = RichText::new(&line.text).size(17.0).color(pal.text);
                        if lit {
                            text = text.family(bold()).background_color(pal.warn).color(pal.warn_ink);
                        } else if done {
                            text = text.color(pal.muted);
                        }
                        ui.add(egui::Label::new(text).wrap());
                        if lit {
                            ui.label(RichText::new("◀ GO").family(bold()).size(14.0).color(pal.warn));
                        }
                    });
                }
            }
        }
        for &i in &before[open.lines.len()] {
            self.cue_row(ui, i, acts);
        }
    }

    /// One sound, as a clickable card. Clicking it makes it the next sound.
    fn cue_row(&self, ui: &mut egui::Ui, i: usize, acts: &mut Vec<Act>) {
        let pal = p();
        let Some(cue) = self.cues().get(i) else { return };
        let voice = self.voice(cue.id);
        let is_next = self.next == i;
        let played = self.played.contains(&cue.id);
        let (stroke, fill) = if voice.is_some() {
            (pal.accent, pal.accent.gamma_multiply(0.12))
        } else if is_next {
            (pal.warn, pal.warn.gamma_multiply(0.12))
        } else {
            (pal.line, pal.card)
        };
        ui.add_space(3.0);
        let row = ui.scope_builder(UiBuilder::new().sense(Sense::click()), |ui| {
            Frame::new()
                .fill(fill)
                .stroke(Stroke::new(if is_next { 2.0 } else { 1.0 }, stroke))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    // The buttons on the right go in first, so the words wrap before reaching them.
                    egui::Sides::new().shrink_left().show(
                        ui,
                        |ui| {
                            let num_color = if is_next { pal.warn } else { pal.muted };
                            ui.label(RichText::new(format!("{}", i + 1)).family(bold()).size(20.0).color(num_color));
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(strong(&cue.name, 17.0));
                                    if let Some(m) = music_text(self, i) {
                                        ui.label(RichText::new(m).size(14.0).color(pal.music));
                                    }
                                });
                                let (when, line) = when_text(cue);
                                let short = match line {
                                    Some(l) => {
                                        format!("{} “{}”", cue.who.trim(), l.lines().last().unwrap_or("").trim())
                                    }
                                    None if !cue.note.trim().is_empty() => cue.note.trim().to_string(),
                                    None => when,
                                };
                                ui.add(
                                    egui::Label::new(RichText::new(short.trim()).size(14.0).color(pal.muted)).wrap(),
                                );
                            });
                        },
                        |ui| {
                            if let Some(v) = voice {
                                if ui.button(RichText::new("■").size(16.0)).on_hover_text("Stop this sound").clicked()
                                {
                                    acts.push(Act::StopCue(cue.id));
                                }
                                ui.label(
                                    RichText::new(format!("▶ {:.1} s left", v.remaining()))
                                        .family(bold())
                                        .size(14.0)
                                        .color(pal.accent),
                                );
                            } else {
                                if ui
                                    .button(RichText::new("✏").size(15.0))
                                    .on_hover_text("Change this sound (opens it in Set up)")
                                    .clicked()
                                {
                                    acts.push(Act::EditCue(cue.id));
                                }
                                let play = ui.button(RichText::new("▶").size(16.0)).on_hover_text(
                                    "Play this sound right now, out of order. The next sound stays the same.",
                                );
                                if play.clicked() {
                                    acts.push(Act::PlayNow(cue.id));
                                }
                                let (tag, color) = if is_next {
                                    ("◀ NEXT", pal.warn)
                                } else if played {
                                    ("✔ played", pal.good)
                                } else {
                                    ("", pal.muted)
                                };
                                ui.label(RichText::new(tag).family(bold()).size(14.0).color(color));
                            }
                        },
                    );
                });
        });
        let row =
            row.response.on_hover_cursor(CursorIcon::PointingHand).on_hover_text("Click to make this the next sound");
        if row.clicked() {
            acts.push(Act::SetNext(i));
        }
        if is_next && self.scroll_to_next {
            row.scroll_to_me(Some(Align::Center));
        }
        ui.add_space(3.0);
    }
}

/// The words to listen for, set off in a bar so they can't be missed.
pub(crate) fn quote(ui: &mut egui::Ui, text: &str) {
    let pal = p();
    let r = Frame::new()
        .fill(pal.warn.gamma_multiply(0.12))
        .corner_radius(CornerRadius { nw: 0, sw: 0, ne: 8, se: 8 })
        .inner_margin(Margin { left: 12, right: 10, top: 6, bottom: 6 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(format!("“{}”", text.trim())).family(bold()).size(18.0).color(pal.text));
        })
        .response;
    let bar = egui::Rect::from_min_size(r.rect.min, vec2(4.0, r.rect.height()));
    ui.painter().rect_filled(bar, CornerRadius::ZERO, pal.warn);
}

fn keys_card(ui: &mut egui::Ui) {
    let pal = p();
    card(pal.panel, pal.line).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(caps("Keyboard keys (this window must be in front)", pal.muted));
        for (key, what) in [
            ("Space", "GO: play the next sound"),
            ("Enter or Page Down", "GO as well (a presentation clicker works too)"),
            ("Esc", "stop all sound"),
            ("F", "fade everything out"),
            ("M", "fade the music out"),
            ("◀ and ▶ arrows", "back one, or skip one"),
        ] {
            // A wrapping line rather than a table, so a narrow panel can't be forced wider.
            let mut job = egui::text::LayoutJob::default();
            job.append(
                key,
                0.0,
                egui::TextFormat { font_id: egui::FontId::new(14.0, bold()), color: pal.text, ..Default::default() },
            );
            job.append(
                &format!("  {what}"),
                0.0,
                egui::TextFormat { font_id: egui::FontId::proportional(14.0), color: pal.muted, ..Default::default() },
            );
            ui.add(egui::Label::new(job).wrap());
        }
    });
}
