// Embeds the icon and version info into nimblenote.exe when building for Windows.
fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("FileDescription", "nimblenote — keyboard-driven notes");
        res.set("ProductName", "nimblenote-rs");
        if let Err(e) = res.compile() {
            // Don't fail the build just because the resource compiler is missing.
            println!("cargo:warning=could not embed Windows icon: {e}");
        }
    }
}
