// Embeds the application icon and version information into the Windows executable.
// Only runs when building *on* Windows for Windows (rc.exe from the Windows SDK is
// required); cross-checks from Linux skip it.
fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    let target_windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    if target_windows && cfg!(windows) {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico")
            .set("ProductName", "Messenger")
            .set("FileDescription", "Messenger")
            .set("CompanyName", "monoreaz")
            .set("LegalCopyright", "© monoreaz");
        if let Err(error) = res.compile() {
            panic!("failed to embed Windows resources: {error}");
        }
    }
}
