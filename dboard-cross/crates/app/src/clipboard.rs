//! Clipboard access. On Linux the clipboard owner must stay alive for the text to remain
//! pasteable, so a single instance is kept for the lifetime of the process.

use std::sync::Mutex;

static CLIP: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

pub fn set(text: &str) -> Result<(), String> {
    let mut guard = CLIP.lock().map_err(|_| "clipboard lock poisoned".to_string())?;
    if guard.is_none() {
        *guard = Some(arboard::Clipboard::new().map_err(|e| e.to_string())?);
    }
    guard.as_mut().expect("just set").set_text(text.to_string()).map_err(|e| e.to_string())
}

pub fn get() -> Result<String, String> {
    let mut guard = CLIP.lock().map_err(|_| "clipboard lock poisoned".to_string())?;
    if guard.is_none() {
        *guard = Some(arboard::Clipboard::new().map_err(|e| e.to_string())?);
    }
    guard.as_mut().expect("just set").get_text().map_err(|e| e.to_string())
}
