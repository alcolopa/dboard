fn main() {
    slint_build::compile("ui/app.slint").unwrap();
    embed_windows_icon();
}

// Embed the application icon in the Windows .exe (Explorer, taskbar, installer shortcuts).
#[cfg(windows)]
fn embed_windows_icon() {
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../assets/icon.ico");
    res.compile().expect("embed Windows icon");
}

#[cfg(not(windows))]
fn embed_windows_icon() {}
