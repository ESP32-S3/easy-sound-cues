// No black console window behind the app on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() -> eframe::Result {
    // The updater asks a freshly downloaded copy for its version before trusting it.
    if std::env::args().any(|a| a == "--version") {
        println!("easy-sound-cues {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    easy_sound_cues::run()
}
