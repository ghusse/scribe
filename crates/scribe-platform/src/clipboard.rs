//! System clipboard: what to read first and how to restore each kind of content. The raw clipboard calls are
//! a [`ClipboardBackend`]; the arboard one is `device::clipboard::ArboardBackend` (excluded from coverage).
use scribe_core::insert::{Clipboard, ClipboardContent};

pub use crate::device::clipboard::ArboardBackend;

/// One raw clipboard operation per method; errors are display strings.
pub trait ClipboardBackend: Send + Sync {
    fn get_text(&self) -> Result<String, String>;
    /// `(width, height, rgba)`.
    fn get_image(&self) -> Result<(usize, usize, Vec<u8>), String>;
    fn set_text(&self, text: &str) -> Result<(), String>;
    fn set_image(&self, width: usize, height: usize, rgba: &[u8]) -> Result<(), String>;
    fn clear(&self) -> Result<(), String>;
}

/// [`Clipboard`] on top of a [`ClipboardBackend`].
#[derive(Debug, Default, Clone, Copy)]
pub struct BackendClipboard<B>(pub B);

/// The system clipboard (Windows and macOS).
pub type SystemClipboard = BackendClipboard<ArboardBackend>;

impl<B: ClipboardBackend> Clipboard for BackendClipboard<B> {
    fn read(&self) -> ClipboardContent {
        if let Ok(text) = self.0.get_text() {
            return ClipboardContent::Text(text);
        }
        if let Ok((width, height, rgba)) = self.0.get_image() {
            return ClipboardContent::Image { width, height, rgba };
        }
        // Empty, files, rich formats, clipboard unavailable…: arboard cannot tell them apart, so never restore.
        ClipboardContent::Unsupported
    }

    fn write_text(&self, text: &str) -> Result<(), String> {
        self.0.set_text(text)
    }

    fn restore(&self, content: &ClipboardContent) -> Result<(), String> {
        match content {
            ClipboardContent::Text(t) => self.0.set_text(t),
            ClipboardContent::Image { width, height, rgba } => self.0.set_image(*width, *height, rgba),
            ClipboardContent::Empty => self.0.clear(),
            ClipboardContent::Unsupported => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// In-memory clipboard; `None` slots make the matching read fail, `fail_writes` every write.
    #[derive(Default)]
    struct Fake {
        text: Mutex<Option<String>>,
        image: Mutex<Option<(usize, usize, Vec<u8>)>>,
        fail_writes: bool,
        log: Mutex<Vec<String>>,
    }

    impl Fake {
        fn write(&self, what: String) -> Result<(), String> {
            self.log.lock().unwrap().push(what);
            if self.fail_writes { Err("clipboard locked".into()) } else { Ok(()) }
        }
        fn log(&self) -> Vec<String> {
            self.log.lock().unwrap().clone()
        }
    }

    impl ClipboardBackend for Fake {
        fn get_text(&self) -> Result<String, String> {
            self.text.lock().unwrap().clone().ok_or_else(|| "no text".into())
        }
        fn get_image(&self) -> Result<(usize, usize, Vec<u8>), String> {
            self.image.lock().unwrap().clone().ok_or_else(|| "no image".into())
        }
        fn set_text(&self, text: &str) -> Result<(), String> {
            self.write(format!("text:{text}"))
        }
        fn set_image(&self, width: usize, height: usize, rgba: &[u8]) -> Result<(), String> {
            self.write(format!("image:{width}x{height}:{}", rgba.len()))
        }
        fn clear(&self) -> Result<(), String> {
            self.write("clear".into())
        }
    }

    fn with(text: Option<&str>, image: Option<(usize, usize, Vec<u8>)>) -> BackendClipboard<Fake> {
        BackendClipboard(Fake { text: Mutex::new(text.map(String::from)), image: Mutex::new(image), ..Fake::default() })
    }

    #[test]
    fn reads_text_first() {
        let cb = with(Some("a\r\nb"), Some((1, 1, vec![0; 4])));
        assert_eq!(cb.read(), ClipboardContent::Text("a\r\nb".into()));
    }

    #[test]
    fn reads_an_image_when_there_is_no_text() {
        let cb = with(None, Some((2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8])));
        assert_eq!(cb.read(), ClipboardContent::Image { width: 2, height: 1, rgba: vec![1, 2, 3, 4, 5, 6, 7, 8] });
    }

    #[test]
    fn anything_else_is_unsupported_never_empty() {
        // An empty clipboard, files and an unavailable clipboard all look the same: never restored.
        assert_eq!(with(None, None).read(), ClipboardContent::Unsupported);
    }

    #[test]
    fn writes_and_restores_through_the_backend() {
        let cb = with(None, None);
        cb.write_text("dictée").unwrap();
        cb.restore(&ClipboardContent::Text("avant".into())).unwrap();
        cb.restore(&ClipboardContent::Image { width: 2, height: 3, rgba: vec![0; 24] }).unwrap();
        cb.restore(&ClipboardContent::Empty).unwrap();
        cb.restore(&ClipboardContent::Unsupported).unwrap();
        assert_eq!(cb.0.log(), vec!["text:dictée", "text:avant", "image:2x3:24", "clear"]);
    }

    #[test]
    fn backend_errors_are_returned() {
        let cb = BackendClipboard(Fake { fail_writes: true, ..Fake::default() });
        assert_eq!(cb.write_text("x"), Err("clipboard locked".into()));
        assert_eq!(cb.restore(&ClipboardContent::Text("x".into())), Err("clipboard locked".into()));
        assert_eq!(cb.restore(&ClipboardContent::Image { width: 1, height: 1, rgba: vec![0; 4] }), Err("clipboard locked".into()));
        assert_eq!(cb.restore(&ClipboardContent::Empty), Err("clipboard locked".into()));
        assert_eq!(cb.restore(&ClipboardContent::Unsupported), Ok(()), "nothing to restore, nothing written");
        assert_eq!(cb.0.log().len(), 4);
    }
}
