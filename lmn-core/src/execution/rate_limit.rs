use std::num::NonZeroU32;
use std::sync::Arc;

use governor::{DefaultDirectRateLimiter, Quota, RateLimiter};

// ── RpsLimiter ────────────────────────────────────────────────────────────────

/// Shared request-rate limiter used to cap aggregate throughput across all VUs.
///
/// Wraps a `governor` token-bucket so VUs can `.acquire().await` one permit per
/// request. Permits refill smoothly at the configured rate, so callers see a
/// steady drip rather than a once-per-second burst.
///
/// The limiter is intended to be shared across many VUs behind an `Arc` and is
/// `Send + Sync`.
pub struct RpsLimiter {
    inner: DefaultDirectRateLimiter,
}

impl RpsLimiter {
    /// Constructs a new limiter that permits at most `rps` requests per second.
    ///
    /// Returns `None` if `rps` is zero or does not fit in `u32`. Callers should
    /// interpret `None` as "no rate limit configured" and skip wiring the
    /// limiter into VUs.
    pub fn new(rps: usize) -> Option<Arc<Self>> {
        let rps_u32: u32 = u32::try_from(rps).ok()?;
        let nz = NonZeroU32::new(rps_u32)?;
        Some(Arc::new(Self {
            inner: RateLimiter::direct(Quota::per_second(nz)),
        }))
    }

    /// Awaits until one permit is available, then claims it.
    ///
    /// Callers should typically wrap this in a `tokio::select!` against a
    /// cancellation token so VUs do not block past the end of a run.
    pub async fn acquire(&self) {
        self.inner.until_ready().await;
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rps_limiter_zero_returns_none() {
        assert!(RpsLimiter::new(0).is_none());
    }

    #[test]
    fn rps_limiter_valid_rps_returns_some() {
        assert!(RpsLimiter::new(100).is_some());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn first_acquire_is_immediate() {
        let limiter = RpsLimiter::new(10).expect("rps=10 is valid");
        let start = std::time::Instant::now();
        limiter.acquire().await;
        assert!(
            start.elapsed().as_millis() < 50,
            "first acquire should be near-instant (bucket starts full)"
        );
    }
}
