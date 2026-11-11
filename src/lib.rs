//! Easy Sound Cues: plays the sound effects and music for a play, one press of GO at a time.
//!
//! A show is a list of sounds, each waiting for a line of the script. Whoever runs the sound
//! presses GO when they hear the line; the next sound lines itself up. Background music can loop
//! underneath. Sounds can come from files or from YouTube (through yt-dlp, which the app fetches
//! itself), and everything is explained in plain words for people who don't like computers.

mod app;
mod audio;
mod paths;
mod reporter;
mod show;
mod style;
mod synth;
mod tools;
mod ui;
mod ui_pages;
mod ui_run;
mod ui_setup;
mod updater;
mod ytdl;

pub use app::App;
pub use audio::Engine;
pub use paths::Settings;

/// The window icon: the same play-and-waves logo as the app's icon file, 64 pixels square.
fn icon() -> eframe::egui::IconData {
    eframe::egui::IconData { rgba: include_bytes!("../assets/icon-64.rgba").to_vec(), width: 64, height: 64 }
}

/// Start error reports, before anything else can go wrong. Returns the settings it loaded.
fn start_reporting() -> Settings {
    let mut settings = Settings::load();
    if settings.install_id.is_empty() {
        settings.install_id = reporter::new_install_id();
        settings.save();
    }
    reporter::start(settings.send_reports, settings.install_id.clone());
    settings
}

fn update_from_command_line() {
    println!("Easy Sound Cues {} is looking for a newer version…", updater::VERSION);
    let latest = match updater::latest_version() {
        Ok(v) => v,
        Err(e) => {
            println!("Couldn't check: {e}");
            reporter::error_now("update from the command line", &e, "", std::time::Duration::from_secs(8));
            std::process::exit(1);
        }
    };
    if !updater::is_newer(&latest, updater::VERSION) {
        if latest == updater::VERSION {
            println!("This is the newest version.");
        } else {
            println!("This copy ({}) is newer than the newest release ({latest}).", updater::VERSION);
        }
        return;
    }
    let mut last = String::new();
    let result = updater::install(&latest, &mut |p| {
        let line = match p.total {
            Some(t) => format!("{}: {} of {} MB", p.stage, p.done / 1_048_576, t / 1_048_576),
            None => p.stage.to_string(),
        };
        if line != last {
            println!("{line}");
            last = line;
        }
    });
    match result {
        Ok(exe) => println!("Updated to {latest}: {}", exe.display()),
        Err(e) => {
            println!("The update didn't work: {e}");
            reporter::error_now("update from the command line", &e, "", std::time::Duration::from_secs(8));
            std::process::exit(1);
        }
    }
}

pub fn run() -> eframe::Result {
    let settings = start_reporting();
    reporter::log(format!(
        "settings: text size {:.1}, {} colours, speakers {}",
        settings.text_size,
        if settings.light { "light" } else { "dark" },
        if settings.speaker.is_some() { "chosen" } else { "default" }
    ));
    // Update without opening the window: `easy-sound-cues --update` (for testing, or IT staff).
    if std::env::args().any(|a| a == "--update") {
        update_from_command_line();
        return Ok(());
    }
    // For checking crash reports work on a particular computer: `easy-sound-cues --crash-test`.
    if std::env::args().any(|a| a == "--crash-test") {
        panic!("crash test: this crash was caused on purpose with --crash-test");
    }
    updater::clean_up();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Easy Sound Cues")
            .with_app_id("easy-sound-cues")
            .with_icon(icon())
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([760.0, 520.0]),
        ..Default::default()
    };
    let result = eframe::run_native("Easy Sound Cues", options, Box::new(|cc| Ok(Box::new(App::new(cc)))));
    if let Err(e) = &result {
        // The window never opened (usually graphics drivers). Without this, nothing would happen
        // at all when someone double-clicks the app.
        let sent = reporter::error_now(
            "start: opening the window",
            &e.to_string(),
            &format!("{e:?}"),
            std::time::Duration::from_secs(8),
        );
        let what = if sent { "A report about it has been sent to the app's maker." } else { "" };
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("Easy Sound Cues couldn't start")
            .set_description(format!(
                "The app couldn't open its window on this computer.\n\n{what}\n\nUpdating the computer (Windows Update, or Software Update on a Mac) often fixes this.\n\nWhat went wrong: {e}"
            ))
            .show();
    }
    result
}
