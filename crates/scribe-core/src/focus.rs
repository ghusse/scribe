use std::sync::{mpsc, Arc};
use std::time::Duration;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FocusState {
    Editable,
    NotEditable,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FocusSnapshot {
    pub app_name: Option<String>,
    pub window_id: Option<u64>,
    pub state: FocusState,
}

impl FocusSnapshot {
    pub fn unknown() -> Self {
        Self { app_name: None, window_id: None, state: FocusState::Unknown }
    }
}

pub trait FocusDetector: Send + Sync {
    fn snapshot(&self) -> FocusSnapshot;
}

/// Accessibility APIs can block for seconds when the target app is hung: never wait longer than `timeout_ms`.
pub fn snapshot_with_timeout(detector: Arc<dyn FocusDetector>, timeout_ms: u64) -> FocusSnapshot {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(detector.snapshot());
    });
    rx.recv_timeout(Duration::from_millis(timeout_ms)).unwrap_or_else(|_| FocusSnapshot::unknown())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Slow(u64);
    impl FocusDetector for Slow {
        fn snapshot(&self) -> FocusSnapshot {
            std::thread::sleep(Duration::from_millis(self.0));
            FocusSnapshot { app_name: Some("App".into()), window_id: Some(1), state: FocusState::Editable }
        }
    }

    #[test]
    fn fast_detector_result_is_returned() {
        let s = snapshot_with_timeout(Arc::new(Slow(0)), 500);
        assert_eq!(s.state, FocusState::Editable);
    }

    #[test]
    fn hung_detector_yields_unknown() {
        let s = snapshot_with_timeout(Arc::new(Slow(2_000)), 50);
        assert_eq!(s, FocusSnapshot::unknown());
    }
}
