use std::borrow::Cow;

use scribe_core::insert::{Clipboard, ClipboardContent};

/// System clipboard through `arboard` (Windows and macOS).
pub struct SystemClipboard;

impl Clipboard for SystemClipboard {
    fn read(&self) -> ClipboardContent {
        let Ok(mut cb) = arboard::Clipboard::new() else { return ClipboardContent::Unsupported };
        if let Ok(text) = cb.get_text() {
            return ClipboardContent::Text(text);
        }
        if let Ok(img) = cb.get_image() {
            return ClipboardContent::Image { width: img.width, height: img.height, rgba: img.bytes.into_owned() };
        }
        // Empty, files, rich formats…: arboard cannot tell them apart, so never restore.
        ClipboardContent::Unsupported
    }

    fn write_text(&self, text: &str) -> Result<(), String> {
        arboard::Clipboard::new().and_then(|mut cb| cb.set_text(text.to_string())).map_err(|e| e.to_string())
    }

    fn restore(&self, content: &ClipboardContent) -> Result<(), String> {
        let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        match content {
            ClipboardContent::Text(t) => cb.set_text(t.clone()).map_err(|e| e.to_string()),
            ClipboardContent::Image { width, height, rgba } => cb
                .set_image(arboard::ImageData { width: *width, height: *height, bytes: Cow::Owned(rgba.clone()) })
                .map_err(|e| e.to_string()),
            ClipboardContent::Empty => cb.clear().map_err(|e| e.to_string()),
            ClipboardContent::Unsupported => Ok(()),
        }
    }
}
