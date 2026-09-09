use std::time::Duration;

/// Quota recovery delay without a provider hint, shared with offline clients.
/// Consecutive rejections use 15–20, 30–40, then 45–60 seconds.
#[must_use]
pub fn rate_limit_backoff(streak: u8) -> Duration {
    let step = u64::from(streak.clamp(1, 3));
    let mut random = [0_u8; 8];
    let value = if getrandom::fill(&mut random).is_ok() {
        u64::from_le_bytes(random)
    } else {
        0
    };
    Duration::from_millis(15_000 * step + value % (5_000 * step + 1))
}
