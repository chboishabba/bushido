//! Debug-only, opt-in, process-wide aggregate. No origin, request payload,
//! view ID, time, user data, logging, or network access.
use std::sync::{OnceLock, atomic::{AtomicU64, Ordering}};

static ENABLED: OnceLock<bool> = OnceLock::new();
static DENIALS: AtomicU64 = AtomicU64::new(0);

fn enabled() -> bool {
    *ENABLED.get_or_init(|| std::env::var("BUSHIDO_LINUX_PERMISSION_OBSERVATION").as_deref() == Ok("1"))
}

fn increment(counter: &AtomicU64) {
    // Saturation avoids reporting a wrapped counter as a new baseline.
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| Some(n.saturating_add(1)));
}

pub(super) fn observe_denial() {
    if enabled() { increment(&DENIALS); }
}

/// Counts native deny calls, not browser-observed permission outcomes.
/// Global across views; not attributed to an origin or workspace.
pub fn permission_denial_count() -> Option<u64> {
    enabled().then(|| DENIALS.load(Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saturates_without_wrapping() {
        let n = AtomicU64::new(u64::MAX - 1);
        increment(&n);
        increment(&n);
        assert_eq!(n.load(Ordering::Relaxed), u64::MAX);
    }
}
