//! Gives the Windows .exe its icon and name in Explorer. Other systems need nothing here.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/icon.ico").set("ProductName", "Easy Sound Cues").set("FileDescription", "Easy Sound Cues");
    // A missing resource compiler (checking Windows code from another system) only costs the icon.
    if let Err(e) = res.compile() {
        println!("cargo:warning=the Windows icon was not embedded: {e}");
    }
}
