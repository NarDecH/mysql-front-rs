fn main() {
    // Embed the application icon into the Windows executable (taskbar,
    // title bar, Explorer). No-op on other platforms.
    #[cfg(windows)]
    {
        use std::env;
        if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("assets/app.ico");
            res.compile().expect("failed to embed app icon");
        }
    }
}
