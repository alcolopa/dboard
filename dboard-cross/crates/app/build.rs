fn main() {
    // Slint's standard widget sources use internal menu types. Allow those types
    // in our local copy so every control can share the app's palette.
    std::env::set_var("SLINT_ENABLE_EXPERIMENTAL_FEATURES", "1");
    let config = slint_build::CompilerConfiguration::new()
        .with_include_paths(vec![
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/styles"),
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/styles/dboard"),
        ])
        .with_style("dboard".into());
    slint_build::compile_with_config("ui/app.slint", config).unwrap();
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
