// Embed the app icon and version info into the Windows executable.
fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/app.ico");
        res.set("ProductName", "Framework Control");
        res.set("FileDescription", "Framework Control");
        if let Err(e) = res.compile() {
            // Cross-checking from another OS has no resource compiler; the exe just lacks an icon.
            println!("cargo:warning=Windows resources not embedded: {e}");
        }
    }
}
