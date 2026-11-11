//! Easy Sound Cues: plays the sound effects and music for a play, one press of GO at a time.
//!
//! A show is a list of sounds, each waiting for a line of the script. Whoever runs the sound
//! presses GO when they hear the line; the next sound lines itself up. Background music can loop
//! underneath. Sounds can come from files or from YouTube (through yt-dlp, which the app fetches
//! itself), and everything is explained in plain words for people who don't like computers.

mod app;
mod audio;
mod paths;
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

pub fn run() -> eframe::Result {
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
    eframe::run_native("Easy Sound Cues", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
