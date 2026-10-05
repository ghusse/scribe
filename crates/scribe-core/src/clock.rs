use std::sync::OnceLock;
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();

/// Monotonic milliseconds since the first call. Shared by the keyboard hook,
/// the ticker and the session so every timestamp uses the same clock.
pub fn now_ms() -> u64 {
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}
