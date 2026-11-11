//! The window's frame: the top bar with the three big tabs, the message bar at the bottom, and
//! the dialogs that can appear over anything.

use crate::app::{Act, App, Page, Tab, Tone, UpdateState};
use crate::style::{self, bold, card, p, strong};
use eframe::egui::{self, Align, CornerRadius, Frame, Layout, Margin, RichText, Stroke, vec2};

impl App {
    pub(crate) fn draw(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let pal = p();
        match self.page {
            Page::Hello | Page::Checkup => {
                egui::CentralPanel::default().frame(Frame::new().fill(pal.bg).inner_margin(Margin::same(24))).show(
                    ui,
                    |ui| {
                        if self.page == Page::Hello { self.hello(ui, acts) } else { self.checkup_page(ui, acts) }
                    },
                );
            }
            Page::Start => {
                self.top_bar(ui, acts);
                self.update_banner(ui, acts);
                self.bottom_bar(ui);
                egui::CentralPanel::default()
                    .frame(Frame::new().fill(pal.bg).inner_margin(Margin::same(20)))
                    .show(ui, |ui| self.start_page(ui, acts));
            }
            Page::Main => {
                self.top_bar(ui, acts);
                self.update_banner(ui, acts);
                if self.tab != Tab::YouTube {
                    self.needs_banner(ui);
                }
                self.bottom_bar(ui);
                match self.tab {
                    Tab::Run => self.run_view(ui, acts),
                    Tab::Setup => self.setup_view(ui, acts),
                    Tab::YouTube => {
                        egui::CentralPanel::default()
                            .frame(Frame::new().fill(pal.bg).inner_margin(Margin::same(20)))
                            .show(ui, |ui| self.youtube_view(ui, acts));
                    }
                }
            }
        }
        let ctx = ui.ctx().clone();
        self.dialogs(&ctx, acts);
    }

    fn top_bar(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let pal = p();
        egui::Panel::top("top")
            .frame(
                Frame::new().fill(pal.panel).inner_margin(Margin::symmetric(14, 8)).stroke(Stroke::new(1.0, pal.line)),
            )
            .show(ui, |ui| {
                // Small windows get shorter labels so nothing ends up hidden behind anything else.
                let narrow = ui.available_width() < 1180.0;
                ui.horizontal(|ui| {
                    style::logo(ui, 30.0);
                    if !narrow {
                        ui.label(strong("Easy Sound Cues", 18.0));
                    }
                    ui.add_space(4.0);
                    if let Some(open) = &self.open {
                        let title =
                            if open.show.title.trim().is_empty() { "Untitled show" } else { open.show.title.as_str() };
                        let label = egui::Label::new(RichText::new(format!("{title}  ⏷")).size(15.0).color(pal.muted))
                            .truncate()
                            .sense(egui::Sense::click());
                        let b = ui
                            .scope(|ui| {
                                ui.set_max_width(if narrow { 150.0 } else { 260.0 });
                                ui.add(label)
                            })
                            .inner
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text("Open a different show, or make a new one");
                        if b.clicked() {
                            acts.push(Act::ShowList);
                        }
                    }
                    ui.add_space(8.0);
                    if self.page == Page::Main {
                        for (tab, label, short, tip) in [
                            (Tab::Setup, "✏  Set up", "✏ Set up", "Add sounds and say when each one plays"),
                            (
                                Tab::Run,
                                "▶  Run the show",
                                "▶ Run",
                                "Use this during the performance: one big GO button",
                            ),
                            (
                                Tab::YouTube,
                                "⬇  YouTube to MP3",
                                "⬇ YouTube",
                                "Get a sound or song from a YouTube video",
                            ),
                        ] {
                            if tab_button(ui, if narrow { short } else { label }, self.tab == tab)
                                .on_hover_text(tip)
                                .clicked()
                            {
                                acts.push(Act::Tab(tab));
                            }
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let settings = style::button(ui, if narrow { "⚙" } else { "⚙ Settings" }, None)
                            .on_hover_text("Settings: text size, colours, speakers");
                        if settings.clicked() {
                            self.settings_open = true;
                            self.speakers = crate::audio::speakers();
                        }
                        if style::button(ui, if narrow { "?" } else { "?  Help" }, None)
                            .on_hover_text("Help: how everything works")
                            .clicked()
                        {
                            self.help_open = true;
                        }
                        if self.anything_playing() {
                            let text = if narrow { "■ Stop" } else { "■  Stop all sound" };
                            let stop =
                                egui::Button::new(RichText::new(text).family(bold()).size(16.0).color(pal.stop_ink))
                                    .fill(pal.stop)
                                    .min_size(vec2(0.0, 34.0))
                                    .corner_radius(CornerRadius::same(8));
                            if ui.add(stop).on_hover_text("Silence everything right now (Esc key)").clicked() {
                                acts.push(Act::StopAll);
                            }
                        }
                    });
                });
            });
    }

    fn bottom_bar(&mut self, ui: &mut egui::Ui) {
        let pal = p();
        egui::Panel::bottom("bottom")
            .frame(
                Frame::new().fill(pal.panel).inner_margin(Margin::symmetric(14, 6)).stroke(Stroke::new(1.0, pal.line)),
            )
            .show(ui, |ui| {
                let mut reconnect = false;
                // The speakers on the left always show in full; messages on the right get cut short.
                egui::Sides::new().shrink_right().show(
                    ui,
                    |ui| {
                        let (color, text) = match &self.engine.problem {
                            Some(problem) => (pal.stop, format!("🔈 {problem}")),
                            None => (pal.muted, format!("🔊 Sound comes out of: {}", self.engine.device)),
                        };
                        ui.label(RichText::new(text).size(14.0).color(color));
                        if self.engine.problem.is_some() && ui.button("Try again").clicked() {
                            reconnect = true;
                        }
                    },
                    |ui| {
                        if self.toast_showing() {
                            if let Some((_, text, tone)) = &self.toast {
                                let color = match tone {
                                    Tone::Info => pal.text,
                                    Tone::Good => pal.good,
                                    Tone::Bad => pal.stop,
                                };
                                ui.add(egui::Label::new(RichText::new(text).size(15.0).color(color)).truncate());
                            }
                        } else if let Some(open) = &self.open {
                            let text = format!("Saved automatically in {}", crate::paths::friendly(&open.dir));
                            ui.add(egui::Label::new(RichText::new(text).size(13.0).color(pal.faint)).truncate());
                        }
                    },
                );
                if reconnect {
                    self.apply(Act::Reconnect, &ui.ctx().clone());
                }
            });
    }

    fn dialogs(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        let pal = p();
        if let Some((question, yes, act)) = self.confirm.clone() {
            let modal = egui::Modal::new(egui::Id::new("confirm"))
                .frame(card(pal.panel, pal.line).inner_margin(Margin::same(22)))
                .show(ctx, |ui| {
                    ui.set_max_width(420.0);
                    ui.label(RichText::new(question).size(17.0).color(pal.text));
                    ui.add_space(12.0);
                    let mut decided = None;
                    ui.horizontal(|ui| {
                        let yes_button = egui::Button::new(RichText::new(yes).size(16.0).color(pal.stop_ink))
                            .fill(pal.stop)
                            .min_size(vec2(0.0, 36.0));
                        if ui.add(yes_button).clicked() {
                            decided = Some(true);
                        }
                        if style::button(ui, "Cancel", None).clicked() {
                            decided = Some(false);
                        }
                    });
                    decided
                });
            match modal.inner {
                Some(true) => {
                    self.confirm = None;
                    acts.push(act);
                }
                Some(false) => self.confirm = None,
                None if modal.should_close() => self.confirm = None,
                None => {}
            }
        }
        if let Some(mut name) = self.new_show.take() {
            let mut keep = true;
            egui::Modal::new(egui::Id::new("new show"))
                .frame(card(pal.panel, pal.line).inner_margin(Margin::same(22)))
                .show(ctx, |ui| {
                    ui.set_max_width(440.0);
                    ui.label(strong("Make a new show", 20.0));
                    ui.add_space(4.0);
                    ui.label(style::muted("What's the show called? You can change this later."));
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut name)
                            .hint_text("e.g. The Winter Play")
                            .desired_width(380.0)
                            .font(egui::FontId::proportional(17.0)),
                    );
                    edit.request_focus();
                    let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if style::primary(ui, "Make the show", true).clicked() || enter {
                            acts.push(Act::NewShow(name.clone()));
                            keep = false;
                        }
                        if style::button(ui, "Cancel", None).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape))
                        {
                            keep = false;
                        }
                    });
                });
            if keep {
                self.new_show = Some(name);
            }
        }
        if self.shows_open {
            self.shows_window(ctx, acts);
        }
        if self.help_open {
            self.help_window(ctx, acts);
        }
        if self.settings_open {
            self.settings_window(ctx, acts);
        }
        if self.picker.is_some() {
            self.picker_window(ctx, acts);
        }
        if self.fix_open {
            self.fix_window(ctx, acts);
        }
        if self.tour.is_some() {
            self.tour_window(ctx, acts);
        }
    }

    /// "A new version is out", with one button to get it.
    fn update_banner(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let pal = p();
        let (text, color) = match &self.update {
            UpdateState::Available(v) => (
                format!("✨ A new version of Easy Sound Cues is out ({v}). Updating takes about a minute."),
                pal.accent,
            ),
            UpdateState::Installing(pr) => {
                let amount =
                    pr.total.map_or(String::new(), |t| format!(" ({} of {} MB)", pr.done / 1_048_576, t / 1_048_576));
                (format!("{}{amount}… Keep the app open.", pr.stage), pal.accent)
            }
            UpdateState::Ready(_) => (
                "✔ The update is ready. Restart the app to start using it. (Your show is saved.)".to_string(),
                pal.good,
            ),
            _ => return,
        };
        egui::Panel::top("update banner")
            .frame(Frame::new().fill(pal.bg.lerp_to_gamma(color, 0.2)).inner_margin(Margin::symmetric(14, 6)))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(text).size(15.0).color(pal.text));
                    match &self.update {
                        UpdateState::Available(_) => {
                            if style::primary(ui, "Update now", true).clicked() {
                                acts.push(Act::UpdateNow);
                            }
                            if ui.button("Not now").clicked() {
                                self.update = UpdateState::Dismissed;
                            }
                        }
                        UpdateState::Ready(_) => {
                            if style::primary(ui, "Restart now", true).clicked() {
                                acts.push(Act::RestartApp);
                            }
                        }
                        _ => {
                            ui.spinner();
                        }
                    }
                });
            });
    }

    /// "2 sounds still need a file", with a button to sort them out.
    fn needs_banner(&mut self, ui: &mut egui::Ui) {
        let pal = p();
        let count = self.needs().len();
        if count == 0 {
            return;
        }
        egui::Panel::top("needs banner")
            .frame(Frame::new().fill(pal.bg.lerp_to_gamma(pal.warn, 0.2)).inner_margin(Margin::symmetric(14, 6)))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let what = if count == 1 {
                        "1 sound still needs its file, so it can't play yet.".to_string()
                    } else {
                        format!("{count} sounds still need their files, so they can't play yet.")
                    };
                    ui.label(RichText::new(format!("⚠ {what}")).size(15.0).color(pal.text));
                    let fix =
                        egui::Button::new(RichText::new("Fix it now").size(15.0).family(bold()).color(pal.warn_ink))
                            .fill(pal.warn);
                    if ui.add(fix).on_hover_text("Shows each one with buttons to give it a file").clicked() {
                        self.fix_open = true;
                    }
                });
            });
    }

    fn shows_window(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        let pal = p();
        let mut open = true;
        egui::Window::new(RichText::new("Your shows").size(18.0).family(bold()))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .anchor(egui::Align2::CENTER_TOP, vec2(0.0, 70.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if style::button(ui, "➕ New show", None).clicked() {
                        self.new_show = Some(String::new());
                        self.shows_open = false;
                    }
                    if style::button(ui, "📂 Open a show someone gave me", None).clicked() {
                        acts.push(Act::ImportShow);
                        self.shows_open = false;
                    }
                });
                if self.open.is_some() {
                    ui.horizontal(|ui| {
                        if style::button(ui, "📦 Save a copy for another computer", None)
                            .on_hover_text("Makes one .zip file with the whole show inside, sounds and all")
                            .clicked()
                        {
                            acts.push(Act::ExportShow);
                        }
                        if style::button(ui, "🗁 Show this show's folder", None).clicked() {
                            acts.push(Act::RevealShow);
                        }
                    });
                }
                ui.separator();
                let current = self.open.as_ref().map(|o| o.dir.clone());
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    for (title, dir) in self.show_list_cache.clone() {
                        let is_open = current.as_ref() == Some(&dir);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&title).size(16.0).color(if is_open {
                                pal.accent
                            } else {
                                pal.text
                            }));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if is_open {
                                    ui.label(style::muted("open now"));
                                } else if ui.button("Open").clicked() {
                                    acts.push(Act::OpenShow(dir.clone()));
                                    self.shows_open = false;
                                }
                            });
                        });
                    }
                    if self.show_list_cache.is_empty() {
                        ui.label(style::muted("No shows yet."));
                    }
                });
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!("Shows are kept in {}", crate::paths::friendly(&crate::paths::shows_dir())))
                        .size(13.0)
                        .color(pal.faint),
                );
            });
        if !open {
            self.shows_open = false;
        }
    }
}

fn tab_button(ui: &mut egui::Ui, text: &str, selected: bool) -> egui::Response {
    let pal = p();
    let (fill, ink, stroke) =
        if selected { (pal.accent, pal.go_ink, pal.accent) } else { (pal.card, pal.text, pal.line) };
    let label = RichText::new(text).size(16.0).family(bold()).color(if selected { ink } else { pal.text });
    ui.add(
        egui::Button::new(label)
            .fill(fill)
            .stroke(Stroke::new(1.0, stroke))
            .min_size(vec2(0.0, 36.0))
            .corner_radius(CornerRadius::same(18)),
    )
}
