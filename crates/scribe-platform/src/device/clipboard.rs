//! `ClipboardBackend` on the system clipboard through `arboard` (Windows and macOS). One arboard call per
//! method; what to read first and how to restore is decided in `crate::clipboard`.
use std::borrow::Cow;

use crate::clipboard::ClipboardBackend;

#[derive(Debug, Default, Clone, Copy)]
pub struct ArboardBackend;

fn open() -> Result<arboard::Clipboard, String> {
    arboard::Clipboard::new().map_err(|e| e.to_string())
}

impl ClipboardBackend for ArboardBackend {
    fn get_text(&self) -> Result<String, String> {
        open()?.get_text().map_err(|e| e.to_string())
    }

    fn get_image(&self) -> Result<(usize, usize, Vec<u8>), String> {
        let img = open()?.get_image().map_err(|e| e.to_string())?;
        Ok((img.width, img.height, img.bytes.into_owned()))
    }

    fn set_text(&self, text: &str) -> Result<(), String> {
        open()?.set_text(text.to_string()).map_err(|e| e.to_string())
    }

    fn set_image(&self, width: usize, height: usize, rgba: &[u8]) -> Result<(), String> {
        open()?
            .set_image(arboard::ImageData { width, height, bytes: Cow::Owned(rgba.to_vec()) })
            .map_err(|e| e.to_string())
    }

    fn clear(&self) -> Result<(), String> {
        open()?.clear().map_err(|e| e.to_string())
    }
}
