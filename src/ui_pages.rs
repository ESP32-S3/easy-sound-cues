//! The welcome, the computer check, the start page, "YouTube to MP3", the tour, help and settings.

use crate::app::{Act, App, HelperState, Page, PickFor, Purpose, Target, UpdateState};
use crate::style::{self, bold, caps, card, hint, p, strong};
use crate::tools::{self, Tool};
use crate::ytdl::{self, Step};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Layout, Margin, Pos2, Rect, RichText, ScrollArea, Sense,
    Stroke, Ui, pos2, vec2,
};

/// Lay out a column of `width` in the middle of the space.
fn centered(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui)) {
    let w = width.min(ui.available_width());
    let margin = ((ui.available_width() - w) / 2.0).max(0.0);
    ui.horizontal_top(|ui| {
        ui.add_space(margin);
        ui.vertical(|ui| {
            ui.set_width(w);
            add(ui);
        });
    });
}

/// How a line of "Check my computer" stands.
#[derive(Clone, Copy, PartialEq)]
enum Mark {
    Busy,
    Good,
    /// Works, but something could be better.
    Warn,
    Bad,
}

fn mark_icon(ui: &mut Ui, mark: Mark) {
    let pal = p();
    let (text, color) = match mark {
        Mark::Busy => {
            ui.add(egui::Spinner::new().size(20.0));
            return;
        }
        Mark::Good => ("✔", pal.good),
        Mark::Warn => ("⚠", pal.warn),
        Mark::Bad => ("✖", pal.stop),
    };
    ui.label(RichText::new(text).size(20.0).color(color));
}

fn mark(ok: Option<bool>) -> Mark {
    match ok {
        None => Mark::Busy,
        Some(true) => Mark::Good,
        Some(false) => Mark::Bad,
    }
}

fn mb(bytes: u64) -> String {
    format!("{:.0}", bytes as f64 / 1_048_576.0)
}

impl App {
    pub(crate) fn hello(&mut self, ui: &mut Ui, acts: &mut Vec<Act>) {
        ScrollArea::vertical().show(ui, |ui| {
            centered(ui, 640.0, |ui| {
                ui.add_space(30.0);
                style::logo(ui, 96.0);
                ui.add_space(10.0);
                ui.label(strong("Welcome to Easy Sound Cues", 34.0));
                ui.add_space(8.0);
                ui.label(RichText::new("This app plays the sound effects and music for a play, a concert or a presentation: one at a time, in the right order, exactly when you press a button.").size(19.0));
                ui.add_space(6.0);
                ui.label(RichText::new("You don't need to know anything about computers or sound. Everything is explained as you go, and you can't break anything.").size(19.0).color(p().muted));
                ui.add_space(6.0);
                ui.label(RichText::new("First, the app will check that this computer has everything it needs. It takes about a minute.").size(19.0).color(p().muted));
                ui.add_space(24.0);
                let go = style::big_button(ui, "Let's get started  ▶", None, (p().go, p().go_hover, p().go_ink), vec2(320.0, 64.0), 24.0, true);
                if go.clicked() {
                    acts.push(Act::CheckComputer);
                    acts.push(Act::Page(Page::Checkup));
                }
            });
        });
    }

    pub(crate) fn checkup_page(&mut self, ui: &mut Ui, acts: &mut Vec<Act>) {
        ScrollArea::vertical().show(ui, |ui| {
            centered(ui, 760.0, |ui| {
                ui.add_space(16.0);
                ui.label(strong("Checking your computer", 30.0));
                hint(ui, "Each line goes green when it's ready. Anything missing can be fixed with one click.");
                ui.add_space(10.0);
                self.checkup_rows(ui, acts);
                ui.add_space(18.0);
                ui.horizontal(|ui| {
                    if style::button(ui, "◀ Back", None).clicked() {
                        acts.push(Act::Page(Page::Hello));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let label =
                            if self.helpers.youtube_ready() { "Next  ▶" } else { "Next (I'll do this later)  ▶" };
                        if style::primary(ui, label, true).clicked() {
                            self.settings.welcomed = true;
                            self.settings.save();
                            acts.push(Act::Page(Page::Start));
                            self.tour = Some(0);
                        }
                    });
                });
            });
        });
    }

    /// The list of checks, with a fix-it button on anything that isn't ready.
    pub(crate) fn checkup_rows(&mut self, ui: &mut Ui, acts: &mut Vec<Act>) {
        let pal = p();
        let row = |ui: &mut Ui, m: Mark, title: &str, detail: &str, add: &mut dyn FnMut(&mut Ui)| {
            card(pal.card, pal.line).inner_margin(Margin::symmetric(14, 10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                // The button goes on the right first, so the words wrap before reaching it.
                egui::Sides::new().shrink_left().show(
                    ui,
                    |ui| {
                        mark_icon(ui, m);
                        ui.vertical(|ui| {
                            ui.label(strong(title, 17.0));
                            ui.add(egui::Label::new(RichText::new(detail).size(14.5).color(pal.muted)).wrap());
                        });
                    },
                    |ui| add(ui),
                );
            });
            ui.add_space(4.0);
        };

        // Speakers.
        let (ok, detail) = match &self.engine.problem {
            Some(problem) => (false, problem.clone()),
            None => (true, format!("Sound will come out of: {}", self.engine.device)),
        };
        row(ui, mark(Some(ok)), "Speakers or headphones", &detail, &mut |ui| {
            if ok {
                if ui.button("▶ Play a test sound").on_hover_text("You should hear two short chimes").clicked() {
                    acts.push(Act::TestSound);
                }
            } else if ui.button("Try again").clicked() {
                acts.push(Act::Reconnect);
            }
        });

        // The shows folder.
        let place = crate::paths::friendly(&crate::paths::shows_dir());
        let (ok, detail) = match &self.helpers.folder {
            None => (None, "Checking…".to_string()),
            Some(Ok(())) => (Some(true), format!("Your shows will be saved in {place}")),
            Some(Err(e)) => (Some(false), format!("Can't save in {place}: {e}")),
        };
        row(ui, mark(ok), "A place to save your shows", &detail, &mut |_| {});

        // The internet, only needed for YouTube.
        let (ok, detail) = match self.helpers.online {
            None => (None, "Checking…"),
            Some(true) => (Some(true), "Connected. (Only needed for YouTube downloads.)"),
            Some(false) => {
                (Some(false), "Can't reach YouTube right now. That's fine unless you want YouTube downloads.")
            }
        };
        let m = if ok == Some(false) { Mark::Warn } else { mark(ok) };
        row(ui, m, "Internet", detail, &mut |_| {});

        ui.add_space(6.0);
        ui.label(caps("For YouTube to MP3", pal.muted));
        for tool in tools::ALL {
            let state = self.helpers.get(tool).clone();
            let title = format!("{} ({})", tool.title(), tool.program());
            let (m, detail) = match &state {
                HelperState::Unknown | HelperState::Checking => (Mark::Busy, "Looking for it…".to_string()),
                HelperState::Ready(found) => {
                    let whose = if found.ours { "the app's own copy" } else { "already on this computer" };
                    if found.is_old() {
                        (
                            Mark::Warn,
                            format!(
                                "Version {} ({whose}) is old: YouTube may not work until it's updated.",
                                found.version
                            ),
                        )
                    } else {
                        (Mark::Good, format!("Ready: version {} ({whose})", found.version))
                    }
                }
                HelperState::Missing if !tool.required() => {
                    (Mark::Warn, format!("Not on this computer yet. Recommended: {}", tool.purpose()))
                }
                HelperState::Missing => (Mark::Bad, format!("Not on this computer yet. Needed: {}", tool.purpose())),
                HelperState::Broken(e) => (Mark::Bad, format!("It's here but won't run ({e}).")),
                HelperState::Installing(pr) => {
                    let amount = match pr.total {
                        Some(t) => format!("{} of {} MB", mb(pr.done), mb(t)),
                        None if pr.done > 0 => format!("{} MB", mb(pr.done)),
                        None => String::new(),
                    };
                    (Mark::Busy, format!("{}… {amount}", pr.stage))
                }
                HelperState::Failed(e) => (Mark::Bad, e.clone()),
            };
            row(ui, m, &title, &detail, &mut |ui| match &state {
                HelperState::Missing | HelperState::Broken(_) | HelperState::Failed(_) => {
                    let label = if matches!(state, HelperState::Missing) {
                        format!("⬇ Get it ({} MB)", tool.size_mb())
                    } else {
                        "⬇ Try again".into()
                    };
                    if ui.add_enabled(!self.helpers.installing, egui::Button::new(label)).clicked() {
                        acts.push(Act::Install(vec![tool]));
                    }
                }
                HelperState::Ready(found) if found.is_old() || (tool == Tool::YtDlp && !found.ours) => {
                    let label = if found.ours { "⬇ Update it" } else { "⬇ Get the app's own copy" };
                    if ui
                        .add_enabled(!self.helpers.installing, egui::Button::new(label))
                        .on_hover_text("Downloads the newest version into the app's own folder")
                        .clicked()
                    {
                        acts.push(Act::Install(vec![tool]));
                    }
                }
                _ => {}
            });
            if let HelperState::Installing(pr) = &state
                && let Some(t) = pr.total.filter(|&t| t > 0)
            {
                style::bar(ui, pr.done as f32 / t as f32, pal.accent, 6.0);
                ui.add_space(4.0);
            }
        }
        let missing = self.helpers.missing();
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if !missing.is_empty() {
                let total: u32 = missing.iter().map(|t| t.size_mb()).sum();
                let label = format!("⬇ Get everything I need (about {total} MB)");
                if style::primary(ui, &label, !self.helpers.installing).clicked() {
                    acts.push(Act::Install(missing.clone()));
                }
            }
            if ui
                .add_enabled(
                    !self.helpers.installing && !self.helpers.checking(),
                    egui::Button::new(RichText::new("↻ Check again").size(16.0)).min_size(vec2(0.0, 34.0)),
                )
                .clicked()
            {
                acts.push(Act::CheckComputer);
            }
        });
        ui.add_space(4.0);
        hint(
            ui,
            "These helpers are free programs, downloaded from their official websites into this app's own folder. Nothing else on your computer is changed. You only need them for YouTube to MP3: everything else works without them.",
        );
    }

    pub(crate) fn start_page(&mut self, ui: &mut Ui, acts: &mut Vec<Act>) {
        let pal = p();
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            centered(ui, 760.0, |ui| {
                ui.add_space(12.0);
                ui.label(strong("What would you like to do?", 30.0));
                ui.add_space(10.0);
                for (title, sub, act) in [
                    (
                        "▶  Try the practice show",
                        "A tiny pretend play with four made-up sounds. The best way to learn.",
                        Act::OpenPractice,
                    ),
                    (
                        "➕  Make a new show",
                        "Start with an empty show and add your own sounds.",
                        Act::NewShow(String::new()),
                    ),
                    (
                        "📂  Open a show someone gave me",
                        "A .zip file or a show folder, for example from a USB stick or an email.",
                        Act::ImportShow,
                    ),
                ] {
                    let colors = (pal.card, pal.card_hi, pal.text);
                    let r =
                        style::big_button(ui, title, Some(sub), colors, vec2(ui.available_width(), 84.0), 22.0, true);
                    if r.clicked() {
                        match act {
                            Act::NewShow(_) => self.new_show = Some(String::new()),
                            other => acts.push(other),
                        }
                    }
                    ui.add_space(6.0);
                }
                if !self.show_list_cache.is_empty() {
                    ui.add_space(14.0);
                    ui.label(caps("Your shows", pal.muted));
                    for (title, dir) in self.show_list_cache.clone() {
                        card(pal.card, pal.line).inner_margin(Margin::symmetric(14, 8)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(strong(&title, 17.0));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if style::button(ui, "Open", None).clicked() {
                                        acts.push(Act::OpenShow(dir.clone()));
                                    }
                                });
                            });
                        });
                        ui.add_space(3.0);
                    }
                }
                ui.add_space(14.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.link(RichText::new("Take the tour again").size(15.0)).clicked() {
                        self.tour = Some(0);
                    }
                    ui.label("·");
                    if ui.link(RichText::new("Check my computer").size(15.0)).clicked() {
                        acts.push(Act::CheckComputer);
                        self.settings_open = true;
                    }
                });
            });
        });
    }

    pub(crate) fn youtube_view(&mut self, ui: &mut Ui, acts: &mut Vec<Act>) {
        let pal = p();
        let ctx = ui.ctx().clone();
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            centered(ui, 760.0, |ui| {
                ui.label(strong("⬇ YouTube to MP3", 28.0));
                hint(ui, "Turn the sound of a YouTube video into an MP3 file, ready to use in your show.");
                ui.add_space(8.0);
                let target = match self.yt.target {
                    Target::ReplaceCue(id) => self.cues().iter().find(|c| c.id == id).map(|c| format!("This download will become the sound for “{}”.", c.name)),
                    Target::ReplaceSong(id) => self.open.as_ref().and_then(|o| o.show.song(id)).map(|s| format!("This download will become the music for “{}”.", s.name)),
                    Target::Purpose => None,
                };
                if let Some(text) = target {
                    card(pal.card_hi, pal.accent).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(text).size(16.0));
                            if ui.button("Don't replace it").clicked() {
                                self.yt.target = Target::Purpose;
                            }
                        });
                    });
                    ui.add_space(8.0);
                }
                if !self.helpers.youtube_ready() {
                    card(pal.card, pal.warn).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(strong("One-time setup needed", 19.0));
                        hint(ui, "Before the first download, the app needs some free helper programs. Press the green button and wait a minute or two.");
                        ui.add_space(4.0);
                        self.checkup_rows(ui, acts);
                    });
                    ui.add_space(10.0);
                }

                let ready = self.helpers.youtube_ready();
                let running = self.yt.job.as_ref().is_some_and(|j| !j.step.finished());
                step(ui, 1, "Find the video on YouTube in your web browser.", |ui| {
                    if ui.button("Open YouTube in my browser").clicked() {
                        ctx.open_url(egui::OpenUrl::new_tab("https://www.youtube.com"));
                    }
                });
                step(ui, 2, "Copy its address: click the address bar at the very top of the browser, then press Ctrl+C (or ⌘+C on a Mac).", |_| {});
                let url_id = egui::Id::new("youtube link");
                step(ui, 3, "Paste it here:", |ui| {
                    ui.horizontal(|ui| {
                        let edit = egui::TextEdit::singleline(&mut self.yt.url)
                            .id(url_id)
                            .hint_text("https://www.youtube.com/watch?v=…")
                            .desired_width((ui.available_width() - 110.0).max(200.0))
                            .font(FontId::proportional(17.0));
                        ui.add(edit);
                        if ui.button(RichText::new("Paste").size(16.0)).on_hover_text("Paste what you copied").clicked() {
                            self.yt.url.clear();
                            ui.memory_mut(|m| m.request_focus(url_id));
                            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                        }
                    });
                });
                step(ui, 4, "Only want part of it? (You can skip this.)", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("From");
                        ui.add(egui::TextEdit::singleline(&mut self.yt.from).hint_text("0:00").desired_width(80.0));
                        ui.label("to");
                        ui.add(egui::TextEdit::singleline(&mut self.yt.to).hint_text("the end").desired_width(80.0));
                    });
                    hint(ui, "Write times like 0:30 (thirty seconds) or 1:15 (one minute fifteen). Leave them empty to keep the whole thing.");
                });
                let replacing = self.yt.target != Target::Purpose;
                step(ui, 5, "What is it for?", |ui| {
                    ui.add_enabled_ui(!replacing && self.open.is_some(), |ui| {
                        ui.radio_value(&mut self.yt.purpose, Purpose::Effect, "A sound for the show (it's added to the list of sounds)");
                        ui.radio_value(&mut self.yt.purpose, Purpose::Music, "Background music for the show");
                    });
                    ui.add_enabled_ui(!replacing, |ui| {
                        ui.radio_value(&mut self.yt.purpose, Purpose::JustSave, "Just save the MP3 file");
                    });
                    if self.open.is_none() {
                        self.yt.purpose = Purpose::JustSave;
                    }
                    ui.horizontal(|ui| {
                        ui.label("Its name (optional):");
                        ui.add(egui::TextEdit::singleline(&mut self.yt.name).hint_text("the video's title").desired_width(300.0));
                    });
                });
                ui.add_space(6.0);
                let go = style::big_button(ui, "⬇  Download as MP3", None, (pal.go, pal.go_hover, pal.go_ink), vec2(ui.available_width(), 60.0), 22.0, ready && !running);
                if go.clicked() {
                    self.start_download(&ctx);
                }
                if let Some(problem) = &self.yt.problem {
                    ui.label(RichText::new(format!("⚠ {problem}")).size(16.0).color(pal.stop));
                }
                let dest = self.download_dir();
                hint(ui, &format!("The MP3 is saved in {}.", crate::paths::friendly(&dest)));
                ui.add_space(8.0);
                self.download_card(ui, acts);
                if !self.yt.done.is_empty() {
                    ui.add_space(10.0);
                    ui.label(caps("Downloaded so far", pal.muted));
                    for (path, what) in self.yt.done.iter().rev() {
                        ui.horizontal_wrapped(|ui| {
                            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                            ui.label(RichText::new(format!("✔ {name}")).color(pal.good));
                            ui.label(RichText::new(what).size(14.0).color(pal.muted));
                            if ui.small_button("Show in folder").clicked()
                                && let Some(dir) = path.parent() {
                                    crate::paths::reveal(dir);
                                }
                        });
                    }
                }
            });
        });
    }

    fn download_dir(&self) -> std::path::PathBuf {
        match &self.open {
            Some(open) if self.yt.purpose != Purpose::JustSave || self.yt.target != Target::Purpose => {
                open.dir.join(crate::show::SOUNDS_DIR)
            }
            _ => crate::paths::downloads_dir(),
        }
    }

    fn start_download(&mut self, ctx: &egui::Context) {
        self.yt.problem = None;
        let url = match ytdl::clean_url(&self.yt.url) {
            Ok(u) => u,
            Err(e) => {
                self.yt.problem = Some(e);
                return;
            }
        };
        let (from, to) = match (ytdl::parse_time(&self.yt.from), ytdl::parse_time(&self.yt.to)) {
            (Ok(f), Ok(t)) => (f, t),
            (Err(e), _) | (_, Err(e)) => {
                self.yt.problem = Some(e);
                return;
            }
        };
        if let (Some(f), Some(t)) = (from, to)
            && t <= f
        {
            self.yt.problem =
                Some(format!("The end ({}) has to be after the start ({}).", ytdl::clock(t), ytdl::clock(f)));
            return;
        }
        let (Some(ytdlp), Some(ffmpeg)) = (self.helpers.found(Tool::YtDlp), self.helpers.found(Tool::Ffmpeg)) else {
            self.yt.problem = Some(
                "The helper programs aren't ready yet. Use the green “Get everything I need” button above.".into(),
            );
            return;
        };
        let helpers = ytdl::Helpers {
            ytdlp: ytdlp.path.clone(),
            ffmpeg: ffmpeg.path.clone(),
            deno: self.helpers.found(Tool::Deno).map(|f| f.path.clone()),
        };
        self.yt.url = url.clone();
        let req = ytdl::Request { url, from, to, dest: self.download_dir(), name: self.yt.name.clone() };
        let repaint = ctx.clone();
        self.yt.job = Some(ytdl::Job::start(req, helpers, move || repaint.request_repaint()));
    }

    fn download_card(&mut self, ui: &mut Ui, acts: &mut Vec<Act>) {
        let pal = p();
        let Some(job) = &self.yt.job else { return };
        let step = job.step.clone();
        let elapsed = job.started.elapsed().as_secs();
        let mut dismiss = false;
        card(pal.card, if matches!(step, Step::Failed { .. }) { pal.stop } else { pal.accent }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let (n, text, frac) = match &step {
                Step::LookingUp => (1, "Looking up the video…".to_string(), None),
                Step::Downloading(f) => (2, "Downloading…".to_string(), *f),
                Step::Converting => (3, "Making the MP3…".to_string(), None),
                Step::Done { .. } => (3, "Done!".to_string(), Some(1.0)),
                Step::Failed { message, .. } => (0, message.clone(), None),
            };
            if n > 0 {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(22.0));
                    ui.label(strong(format!("Step {n} of 3: {text}"), 18.0));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Cancel").clicked()
                            && let Some(job) = &self.yt.job
                        {
                            job.cancel();
                        }
                        ui.label(RichText::new(format!("{elapsed} s")).color(pal.muted));
                    });
                });
                match frac {
                    Some(f) => style::bar(ui, f, pal.accent, 8.0),
                    None => hint(ui, "This can take a little while for long videos."),
                }
            } else {
                ui.label(RichText::new(format!("✖ {text}")).size(17.0).color(pal.stop));
                if let Step::Failed { detail, .. } = &step
                    && !detail.trim().is_empty()
                {
                    egui::CollapsingHeader::new(RichText::new("Details for someone who knows computers").size(14.0))
                        .show(ui, |ui| {
                            ui.label(RichText::new(detail).monospace().size(12.5).color(pal.muted));
                        });
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.button("OK").clicked() {
                        dismiss = true;
                    }
                    if ui
                        .add_enabled(!self.helpers.installing, egui::Button::new("⬇ Update the downloader"))
                        .on_hover_text("Get the newest yt-dlp. YouTube changes often, and this fixes most problems.")
                        .clicked()
                    {
                        acts.push(Act::Install(vec![Tool::YtDlp]));
                        dismiss = true;
                    }
                });
            }
        });
        if dismiss {
            self.yt.job = None;
        }
    }

    pub(crate) fn tour_window(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        let pal = p();
        let Some(page) = self.tour else { return };
        let page = page.min(TOUR.len() - 1);
        let (title, body, picture) = TOUR[page];
        let mut next = Some(page);
        let modal = egui::Modal::new(egui::Id::new("tour"))
            .frame(card(pal.panel, pal.line).inner_margin(Margin::same(24)))
            .show(ctx, |ui| {
                ui.set_width(600.0);
                ui.horizontal(|ui| {
                    ui.label(caps(&format!("Quick tour · {} of {}", page + 1, TOUR.len()), pal.accent));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Skip the tour").clicked() {
                            next = None;
                        }
                    });
                });
                ui.add_space(4.0);
                ui.label(strong(title, 28.0));
                ui.add_space(8.0);
                illustrate(ui, picture);
                ui.add_space(10.0);
                ui.label(RichText::new(body).size(18.0));
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    if page > 0 && style::button(ui, "◀ Back", None).clicked() {
                        next = Some(page - 1);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if page + 1 < TOUR.len() {
                            if style::primary(ui, "Next  ▶", true).clicked() {
                                next = Some(page + 1);
                            }
                        } else {
                            if style::primary(ui, "▶  Try the practice show", true).clicked() {
                                acts.push(Act::OpenPractice);
                                next = None;
                            }
                            if style::button(ui, "Close", None).clicked() {
                                next = None;
                            }
                        }
                    });
                });
            });
        if modal.should_close() {
            next = None;
        }
        self.tour = next;
    }

    pub(crate) fn help_window(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        let mut open = true;
        egui::Window::new(RichText::new("Help").size(20.0).family(bold()))
            .open(&mut open)
            .collapsible(false)
            .default_width(560.0)
            .default_height(560.0)
            .anchor(Align2::RIGHT_TOP, vec2(-16.0, 64.0))
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if style::button(ui, "▶ Watch the tour again", None).clicked() {
                        self.tour = Some(0);
                        self.help_open = false;
                    }
                    if style::button(ui, "✔ Check my computer", None).clicked() {
                        acts.push(Act::CheckComputer);
                        self.settings_open = true;
                    }
                });
                ui.separator();
                ScrollArea::vertical().show(ui, |ui| {
                    for (question, steps) in HELP {
                        egui::CollapsingHeader::new(RichText::new(*question).size(17.0).family(bold())).show(
                            ui,
                            |ui| {
                                for (n, s) in steps.iter().enumerate() {
                                    ui.horizontal_top(|ui| {
                                        ui.label(RichText::new(format!("{}.", n + 1)).family(bold()).color(p().accent));
                                        ui.add(egui::Label::new(RichText::new(*s).size(16.0)).wrap());
                                    });
                                }
                            },
                        );
                    }
                });
            });
        if !open {
            self.help_open = false;
        }
    }

    pub(crate) fn settings_window(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        let pal = p();
        let mut open = true;
        let mut restyle = false;
        egui::Window::new(RichText::new("Settings").size(20.0).family(bold()))
            .open(&mut open)
            .collapsible(false)
            .default_width(640.0)
            .default_height(600.0)
            .anchor(Align2::CENTER_TOP, vec2(0.0, 60.0))
            .show(ctx, |ui| {
                ScrollArea::vertical().show(ui, |ui| {
                    ui.label(strong("Text size", 18.0));
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("A").size(13.0));
                        let r = ui.add(egui::Slider::new(&mut self.settings.text_size, 0.8..=1.8).show_value(false));
                        ui.label(RichText::new("A").size(24.0));
                        if r.drag_stopped() || (r.changed() && !r.dragged()) {
                            restyle = true;
                        }
                        if ui.button("Normal").clicked() {
                            self.settings.text_size = 1.0;
                            restyle = true;
                        }
                    });
                    hint(ui, "Make everything bigger or smaller. (Ctrl and + or - does this too.)");
                    ui.add_space(8.0);
                    ui.label(strong("Colours", 18.0));
                    ui.horizontal(|ui| {
                        restyle |=
                            ui.radio_value(&mut self.settings.light, false, "Dark (best in a dark theatre)").changed();
                        restyle |= ui.radio_value(&mut self.settings.light, true, "Light").changed();
                    });
                    ui.add_space(8.0);
                    ui.label(strong("Speakers", 18.0));
                    let current = self
                        .settings
                        .speaker
                        .clone()
                        .unwrap_or_else(|| "The computer's usual speakers (recommended)".into());
                    let mut chosen = self.settings.speaker.clone();
                    egui::ComboBox::from_id_salt("speakers").selected_text(current).width(420.0).show_ui(ui, |ui| {
                        ui.selectable_value(&mut chosen, None, "The computer's usual speakers (recommended)");
                        for name in &self.speakers {
                            ui.selectable_value(&mut chosen, Some(name.clone()), name);
                        }
                    });
                    if chosen != self.settings.speaker {
                        self.settings.speaker = chosen;
                        acts.push(Act::Reconnect);
                    }
                    ui.horizontal(|ui| {
                        if ui.button("▶ Play a test sound").clicked() {
                            acts.push(Act::TestSound);
                        }
                        if ui
                            .button("↻ Reconnect the speakers")
                            .on_hover_text("Use this after plugging in speakers, or if the sound stops")
                            .clicked()
                        {
                            acts.push(Act::Reconnect);
                        }
                    });
                    ui.add_space(8.0);
                    ui.label(strong("Main volume", 18.0));
                    let mut volume = self.settings.volume;
                    ui.horizontal(|ui| {
                        if ui.add(egui::Slider::new(&mut volume, 0.0..=1.0).show_value(false)).changed() {
                            acts.push(Act::Volume(volume));
                        }
                        ui.label(format!("{}%", (volume * 100.0).round() as i32));
                    });
                    ui.add_space(8.0);
                    ui.label(strong("Your shows", 18.0));
                    let dir = crate::paths::shows_dir();
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(crate::paths::friendly(&dir)).monospace());
                        if ui.button("Show the folder").clicked() {
                            let _ = std::fs::create_dir_all(&dir);
                            crate::paths::reveal(&dir);
                        }
                    });
                    ui.add_space(8.0);
                    ui.separator();
                    ui.label(strong("Updates", 18.0));
                    ui.label(format!("You have version {}.", crate::updater::VERSION));
                    let status = match &self.update {
                        UpdateState::Checking => Some(("Checking…".to_string(), pal.muted)),
                        UpdateState::UpToDate => Some(("✔ You have the newest version.".to_string(), pal.good)),
                        UpdateState::Available(v) => Some((
                            format!("✨ Version {v} is out: use the green “Update now” button at the top."),
                            pal.accent,
                        )),
                        UpdateState::Installing(_) => Some(("Updating…".to_string(), pal.muted)),
                        UpdateState::Ready(_) => {
                            Some(("✔ Updated. Restart the app to use the new version.".to_string(), pal.good))
                        }
                        UpdateState::Failed(e) => Some((e.clone(), pal.stop)),
                        UpdateState::Idle | UpdateState::Dismissed => None,
                    };
                    if let Some((text, color)) = status {
                        ui.label(RichText::new(text).color(color));
                    }
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Check for updates now").clicked() {
                            acts.push(Act::CheckUpdate);
                        }
                        if matches!(self.update, UpdateState::Dismissed) && ui.button("Update now").clicked() {
                            acts.push(Act::CheckUpdate);
                        }
                        if ui.checkbox(&mut self.settings.check_updates, "Check by itself once a day").changed() {
                            self.settings.save();
                        }
                    });
                    hint(ui, "Updates come from the app's page on GitHub. Your shows are never touched by an update.");
                    if ui.link("See what's new on the website").clicked() {
                        ctx.open_url(egui::OpenUrl::new_tab(crate::updater::releases_page()));
                    }
                    ui.add_space(8.0);
                    ui.separator();
                    ui.label(strong("Check my computer", 18.0));
                    self.checkup_rows(ui, acts);
                    ui.add_space(8.0);
                    ui.separator();
                    ui.label(
                        RichText::new(format!("Easy Sound Cues {} · free and open source", env!("CARGO_PKG_VERSION")))
                            .color(pal.muted),
                    );
                    if ui.link("github.com/ESP32-S3/easy-sound-cues").clicked() {
                        ctx.open_url(egui::OpenUrl::new_tab(env!("CARGO_PKG_REPOSITORY")));
                    }
                });
            });
        if restyle {
            style::apply(ctx, self.settings.light, self.settings.text_size);
            self.settings.save();
        }
        if !open {
            self.settings_open = false;
            self.settings.save();
        }
    }

    /// "Sounds you already have": files already on the computer, ready to use without hunting
    /// through folders.
    pub(crate) fn picker_window(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        let pal = p();
        let Some(picker) = &self.picker else { return };
        let purpose = picker.purpose;
        let for_what = match purpose {
            PickFor::NewCue => "a new sound".to_string(),
            PickFor::NewSong => "new background music".to_string(),
            PickFor::Cue(id) => {
                self.cues().iter().find(|c| c.id == id).map_or("the sound".into(), |c| format!("“{}”", c.name))
            }
            PickFor::Song(id) => {
                self.open.as_ref().and_then(|o| o.show.song(id)).map_or("the music".into(), |s| format!("“{}”", s.name))
            }
        };
        let mut open = true;
        let mut close = false;
        egui::Window::new(RichText::new("Sounds you already have").size(20.0).family(bold()))
            .open(&mut open)
            .collapsible(false)
            .default_width(620.0)
            .default_height(560.0)
            .anchor(Align2::CENTER_TOP, vec2(0.0, 60.0))
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(format!("Pick a file to use for {for_what}. Press ▶ to hear one first.")).size(16.0),
                );
                ui.horizontal_wrapped(|ui| {
                    if ui.button("📁 Look somewhere else on this computer…").clicked() {
                        let picked = rfd::FileDialog::new()
                            .set_title("Choose a sound file")
                            .add_filter("Sound files", &crate::app::SOUND_FILES)
                            .pick_file();
                        if let Some(file) = picked {
                            acts.push(Act::UseFile(purpose, file));
                            close = true;
                        }
                    }
                });
                ui.separator();
                ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let Some(picker) = &self.picker else { return };
                    if picker.files.is_empty() {
                        ui.label(style::muted(
                            "No sound files found yet. Use “Look somewhere else”, or get one from YouTube.",
                        ));
                    }
                    let mut last_place = "";
                    for have in &picker.files {
                        if have.place != last_place {
                            ui.add_space(6.0);
                            ui.label(caps(have.place, pal.muted));
                            last_place = have.place;
                        }
                        let name = have.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        card(pal.card, pal.line).inner_margin(Margin::symmetric(10, 6)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            egui::Sides::new().shrink_left().show(
                                ui,
                                |ui| {
                                    ui.vertical(|ui| {
                                        ui.add(egui::Label::new(RichText::new(&name).size(15.0)).truncate());
                                        let mut info = format!("{:.1} MB", have.size as f64 / 1_048_576.0);
                                        if have.in_show {
                                            info += " · already used in this show";
                                        }
                                        ui.label(RichText::new(info).size(13.0).color(pal.muted));
                                    });
                                },
                                |ui| {
                                    if ui.button(RichText::new("Use this").size(15.0)).clicked() {
                                        acts.push(Act::UseFile(purpose, have.path.clone()));
                                    }
                                    if ui.button("▶").on_hover_text("Listen to it").clicked() {
                                        acts.push(Act::PreviewFile(have.path.clone()));
                                    }
                                },
                            );
                        });
                    }
                });
            });
        if !open || close {
            self.picker = None;
        }
    }

    /// Every sound that can't play yet, each with the ways to give it a file.
    pub(crate) fn fix_window(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        let pal = p();
        let needs = self.needs();
        let mut open = true;
        egui::Window::new(RichText::new("Sounds that need a file").size(20.0).family(bold()))
            .open(&mut open)
            .collapsible(false)
            .default_width(640.0)
            .anchor(Align2::CENTER_TOP, vec2(0.0, 70.0))
            .show(ctx, |ui| {
                if needs.is_empty() {
                    ui.label(RichText::new("✔ Every sound has its file now. All set!").size(17.0).color(pal.good));
                    if style::primary(ui, "Close", true).clicked() {
                        self.fix_open = false;
                    }
                    return;
                }
                hint(ui, "For each one, choose where its sound comes from. A file you already have is quickest.");
                ScrollArea::vertical().max_height(460.0).show(ui, |ui| {
                    for need in &needs {
                        card(pal.card, pal.warn.gamma_multiply(0.6)).inner_margin(Margin::symmetric(12, 8)).show(
                            ui,
                            |ui| {
                                ui.set_width(ui.available_width());
                                let title = match need.number {
                                    Some(n) => format!("Sound {n}: {}", need.name),
                                    None => format!("Music: {}", need.name),
                                };
                                ui.label(strong(title, 17.0));
                                ui.label(RichText::new(&need.why).size(14.5).color(pal.muted));
                                ui.horizontal_wrapped(|ui| {
                                    if ui.button("🎵 One I already have").clicked() {
                                        acts.push(Act::Pick(need.pick));
                                    }
                                    let choose = match need.pick {
                                        PickFor::Cue(id) => Act::ChooseFile(id),
                                        PickFor::Song(id) => Act::ChooseSongFile(id),
                                        _ => Act::AddFiles,
                                    };
                                    if ui.button("📁 Choose a file…").clicked() {
                                        acts.push(choose);
                                    }
                                    let (target, purpose) = match need.pick {
                                        PickFor::Song(id) => (Target::ReplaceSong(id), Purpose::Music),
                                        PickFor::Cue(id) => (Target::ReplaceCue(id), Purpose::Effect),
                                        _ => (Target::Purpose, Purpose::Effect),
                                    };
                                    let label = if need.source.starts_with("http") {
                                        "⬇ Download it again from its YouTube link"
                                    } else {
                                        "⬇ From YouTube"
                                    };
                                    if ui.button(label).clicked() {
                                        acts.push(Act::YouTubeFor(target, purpose));
                                        self.fix_open = false;
                                    }
                                });
                            },
                        );
                        ui.add_space(4.0);
                    }
                });
            });
        if !open {
            self.fix_open = false;
        }
    }
}

/// A numbered step with its instructions, for the YouTube page.
fn step(ui: &mut Ui, n: u32, text: &str, add: impl FnOnce(&mut Ui)) {
    let pal = p();
    ui.horizontal_top(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
        ui.painter().circle_filled(rect.center(), 15.0, pal.accent);
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, n.to_string(), FontId::new(17.0, bold()), pal.go_ink);
        ui.vertical(|ui| {
            ui.add_space(4.0);
            ui.add(egui::Label::new(RichText::new(text).size(17.0)).wrap());
            add(ui);
        });
    });
    ui.add_space(8.0);
}

#[derive(Clone, Copy)]
enum Picture {
    List,
    Line,
    Go,
    Stop,
    Music,
    Sources,
    Tabs,
    Ready,
}

const TOUR: [(&str, &str, Picture); 8] = [
    (
        "A show is a list of sounds",
        "Each sound plays at one moment in your show: a doorbell, a crash of thunder, a song. The app keeps them in order, so you never have to hunt for the right one.",
        Picture::List,
    ),
    (
        "Each sound waits for its line",
        "For every sound, you type the words that come just before it. During the show those words are shown big and bright, so you know exactly when to press.",
        Picture::Line,
    ),
    (
        "Press GO to play the next sound",
        "There's one big green button. Press it (or the Space bar) when you hear the line. The sound plays, and the next one gets ready by itself.",
        Picture::Go,
    ),
    (
        "STOP makes everything quiet",
        "If anything goes wrong, press the red STOP button or the Esc key, and everything goes silent at once. Nothing is lost: press GO when you're ready to carry on.",
        Picture::Stop,
    ),
    (
        "Music can play underneath",
        "Background music can play quietly under a scene and repeat until it's stopped. A sound can start the music, switch to another song, or fade it out.",
        Picture::Music,
    ),
    (
        "Getting your sounds",
        "Add sound files that are already on your computer, or paste a YouTube link and the app makes an MP3 for you. Each sound's volume is evened out automatically.",
        Picture::Sources,
    ),
    (
        "Set up first, then run the show",
        "“Set up” is where you add and change things. “Run the show” is for the performance itself: big buttons, and nothing you can change by accident. Everything saves by itself.",
        Picture::Tabs,
    ),
    (
        "You're ready!",
        "The best way to learn is to try it. The practice show has four made-up sounds and a tiny script. Press GO when each line is highlighted. You can come back to this tour any time from Help.",
        Picture::Ready,
    ),
];

const HELP: &[(&str, &[&str])] = &[
    (
        "How do I run my show?",
        &[
            "Click “▶ Run the show” at the top.",
            "The box on the left shows the next sound and the words to listen for.",
            "When an actor says those words, press the big green GO button (or the Space bar).",
            "The sound plays and the next one is ready. Keep going until the end.",
        ],
    ),
    (
        "Something went wrong! How do I stop the sound?",
        &[
            "Press the red “STOP ALL SOUND” button, or the Esc key. Everything goes quiet at once.",
            "To turn it down smoothly instead, press “Fade everything out gently” (or the F key).",
            "If GO played the wrong sound, use “Back one” to step back, or click the right sound in the list to make it next.",
        ],
    ),
    (
        "How do I add a sound?",
        &[
            "Click “✏ Set up” at the top, then “🔊 Sounds”.",
            "Press “From a file on this computer” and pick one or more sound files (MP3, WAV, M4A, OGG or FLAC).",
            "Or press “From a YouTube video” and paste the video's link.",
            "The new sound goes after the one that's selected. Use ▲ and ▼ to move it.",
        ],
    ),
    (
        "How do I say when a sound plays?",
        &[
            "In “✏ Set up”, click the sound in the list.",
            "Under “When should it play?”, type who says the line and the words they say just before the sound.",
            "If you pasted your script in the Script tab, the app finds those words and shows the sound right there in the script.",
        ],
    ),
    (
        "How do I add background music?",
        &[
            "In “✏ Set up”, click “♪ Background music” and add a song from a file or from YouTube.",
            "Then go back to “🔊 Sounds”, click the sound that should start the music, and under “Background music” choose “Start …”.",
            "Another sound can later fade the music out, stop it, or start a different song (the two songs blend into each other).",
        ],
    ),
    (
        "A sound is too loud or too quiet",
        &[
            "In “✏ Set up”, click the sound and move its Volume slider. Press “▶ Listen” to check it.",
            "For everything at once, use “Main volume” on the “Run the show” page, or the speakers' own volume knob.",
            "Background music has its own “Music volume” slider. Keep it low so the actors can be heard.",
        ],
    ),
    (
        "There's a gap before the sound starts, or it goes on too long",
        &[
            "In “✏ Set up”, click the sound and find “Start and end”.",
            "Tick “Skip any silence at the start”.",
            "Use “Start at” and “Stop at” to play just part of it, and “Fade out over” to end it gently.",
        ],
    ),
    (
        "How do I put my show on another computer?",
        &[
            "Install Easy Sound Cues on the other computer too.",
            "On this computer, click the show's name at the top, then “📦 Save a copy for another computer”. Save the .zip file on a USB stick, or email it.",
            "On the other computer, choose “Open a show someone gave me” and pick that .zip file. Everything comes with it, sounds and all.",
        ],
    ),
    (
        "I can't hear anything",
        &[
            "Check the speakers are plugged in and switched on, and the computer's own volume is up.",
            "Open ⚙ Settings, press “Reconnect the speakers”, then “Play a test sound”.",
            "If you have more than one set of speakers, choose the right ones under “Speakers” in ⚙ Settings.",
            "Check “Main volume” on the “Run the show” page isn't at 0%.",
        ],
    ),
    (
        "A YouTube download doesn't work",
        &[
            "Check the computer is connected to the internet. Some school and office networks block YouTube; a phone hotspot may work instead.",
            "Open ⚙ Settings and look at “Check my computer”. Fix anything that has a red cross.",
            "Press “Update the downloader”: YouTube changes often and the newest version usually fixes it.",
            "Some videos can't be downloaded at all (private, age-restricted or live videos). Try a different one.",
        ],
    ),
    (
        "Do I need to save?",
        &[
            "No. Every change is saved by itself, straight away.",
            "If you change your mind about adding, removing or moving a sound, press “↶ Undo” in “Set up” (or Ctrl+Z).",
            "Each show lives in its own folder in Documents › Easy Sound Cues, with copies of all its sounds.",
        ],
    ),
    (
        "Keyboard keys",
        &[
            "Space, Enter or Page Down: GO. A wireless presentation clicker works as a GO button too.",
            "Esc: stop all sound.  F: fade everything out.  M: fade the music out.",
            "Left and right arrows: back one sound, or skip one.",
            "The keys only work on the “Run the show” page, while this window is in front.",
        ],
    ),
];

/// Little drawings for the tour, made from the same shapes as the real buttons.
fn illustrate(ui: &mut Ui, picture: Picture) {
    let pal = p();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 150.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(12), pal.bg);
    let c = rect.center();
    let text = |pos: Pos2, s: &str, size: f32, color: Color32| {
        painter.text(pos, Align2::CENTER_CENTER, s, FontId::new(size, bold()), color);
    };
    let pill = |r: Rect, fill: Color32, stroke: Color32| {
        painter.rect(r, CornerRadius::same(10), fill, Stroke::new(1.5, stroke), egui::StrokeKind::Inside);
    };
    match picture {
        Picture::List => {
            for (k, (name, color)) in
                [("1  Doorbell", pal.warn), ("2  Magic sparkle", pal.line_hi), ("3  Thunder", pal.line_hi)]
                    .into_iter()
                    .enumerate()
            {
                let r = Rect::from_center_size(pos2(c.x, rect.top() + 32.0 + k as f32 * 42.0), vec2(300.0, 34.0));
                pill(r, pal.card, color);
                painter.text(
                    r.left_center() + vec2(14.0, 0.0),
                    Align2::LEFT_CENTER,
                    name,
                    FontId::new(17.0, bold()),
                    pal.text,
                );
            }
        }
        Picture::Line => {
            let r = Rect::from_center_size(c - vec2(70.0, 0.0), vec2(330.0, 46.0));
            painter.rect_filled(r, CornerRadius::same(6), pal.warn);
            text(r.center(), "…someone rang the doorbell.", 18.0, pal.warn_ink);
            painter.arrow(pos2(r.right() + 10.0, c.y), vec2(46.0, 0.0), Stroke::new(3.0, pal.text));
            let b = Rect::from_center_size(pos2(r.right() + 120.0, c.y), vec2(130.0, 46.0));
            pill(b, pal.card, pal.accent);
            text(b.center(), "Doorbell", 18.0, pal.text);
        }
        Picture::Go => {
            let r = Rect::from_center_size(c, vec2(330.0, 96.0));
            painter.rect_filled(r, CornerRadius::same(14), pal.go);
            text(c - vec2(0.0, 12.0), "▶  GO", 38.0, pal.go_ink);
            text(c + vec2(0.0, 26.0), "Space bar", 15.0, pal.go_ink);
        }
        Picture::Stop => {
            let r = Rect::from_center_size(c, vec2(330.0, 70.0));
            painter.rect_filled(r, CornerRadius::same(14), pal.stop);
            text(c - vec2(0.0, 8.0), "■  STOP ALL SOUND", 24.0, pal.stop_ink);
            text(c + vec2(0.0, 20.0), "Esc key", 14.0, pal.stop_ink);
        }
        Picture::Music => {
            let w = rect.width().min(460.0);
            let left = c.x - w / 2.0;
            let pts: Vec<Pos2> = (0..=80)
                .map(|k| {
                    let x = left + w * k as f32 / 80.0;
                    let y = c.y + 18.0 * (k as f32 * 0.42).sin() * (0.4 + 0.6 * (k as f32 * 0.07).sin().abs());
                    pos2(x, y)
                })
                .collect();
            painter.add(egui::Shape::line(pts, Stroke::new(3.0, pal.music)));
            text(pos2(c.x, rect.top() + 24.0), "♪  Background music, round and round", 17.0, pal.music);
            text(pos2(c.x, rect.bottom() - 22.0), "sound 2 starts it · sound 3 fades it out", 15.0, pal.muted);
        }
        Picture::Sources => {
            for (k, label) in ["📁  From a file", "⬇  From YouTube"].into_iter().enumerate() {
                let r = Rect::from_center_size(pos2(c.x + (k as f32 - 0.5) * 220.0, c.y), vec2(200.0, 64.0));
                pill(r, pal.card, pal.accent);
                text(r.center(), label, 18.0, pal.text);
            }
        }
        Picture::Tabs => {
            for (k, (label, on)) in
                [("✏  Set up", false), ("▶  Run the show", true), ("⬇  YouTube to MP3", false)].into_iter().enumerate()
            {
                let r = Rect::from_center_size(pos2(c.x + (k as f32 - 1.0) * 190.0, c.y), vec2(176.0, 44.0));
                painter.rect(
                    r,
                    CornerRadius::same(22),
                    if on { pal.accent } else { pal.card },
                    Stroke::new(1.5, if on { pal.accent } else { pal.line }),
                    egui::StrokeKind::Inside,
                );
                text(r.center(), label, 16.0, if on { pal.go_ink } else { pal.text });
            }
        }
        Picture::Ready => {
            text(c - vec2(0.0, 10.0), "🎭", 64.0, pal.text);
            text(c + vec2(0.0, 52.0), "Break a leg!", 18.0, pal.muted);
        }
    }
}
