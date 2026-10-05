use std::sync::OnceLock;
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();

/// Monotonic milliseconds since the first call. Shared by the keyboard hook,
/// the ticker and the session so every timestamp uses the same clock.
pub fn now_ms() -> u64 {
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn is_monotonic_and_follows_elapsed_time() {
        let a = now_ms();
        std::thread::sleep(Duration::from_millis(30));
        let b = now_ms();
        assert!(b >= a + 30, "{a} then {b}");
        assert!(b - a < 5_000, "milliseconds, not microseconds");
        assert!(now_ms() >= b);
    }
}
