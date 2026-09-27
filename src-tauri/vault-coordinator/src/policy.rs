//! Retry and alarm policy (spec v0.4 §11.3.2, §11.6, §15):
//!
//! - Backoff 1 → 5 → 15 minutes (then every 15), plus immediate retries at
//!   launch, unlock, network change and "retry now". Security-driven work
//!   (a revocation, a suspected-stolen RK) goes first.
//! - `BACKUP_REVOCATION_FAILED` after 3 failed attempts of a revocation;
//!   retries continue.
//! - `BACKUP_STALE` after 48 h without a successful publication.
//! - `BACKUP_ACCESS_LOST`: this device's own `state_get` gets `401` three
//!   consecutive times at least 5 minutes apart, while locate answers and
//!   the local clock is within ±300 s of the provider's `Date`. A skewed
//!   clock is a clock problem, never lost access. It is never treated as
//!   revocation and never deletes anything.

pub const BACKOFF: [u64; 3] = [60, 300, 900];
pub const STALE_AFTER: u64 = 48 * 3600;
pub const REVOCATION_FAILED_AFTER: u32 = 3;
pub const ACCESS_LOST_STRIKES: u32 = 3;
pub const ACCESS_LOST_SPACING: u64 = 300;
pub const CLOCK_WINDOW: u64 = 300;

/// Seconds to wait before retry number `attempt` (1-based).
pub fn backoff(attempt: u32) -> u64 {
    BACKOFF[(attempt.max(1) as usize - 1).min(BACKOFF.len() - 1)]
}

pub fn stale(last_success: Option<u64>, now: u64) -> bool {
    last_success.is_none_or(|t| now.saturating_sub(t) >= STALE_AFTER)
}

pub fn revocation_failed(attempts: u32) -> bool {
    attempts >= REVOCATION_FAILED_AFTER
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessSignal {
    Ok,
    /// Keep going; not enough evidence yet.
    Suspect,
    /// The device clock disagrees with the provider: report a clock
    /// problem, not lost access.
    ClockSkew,
    AccessLost,
}

/// Tracks this device's own `state_get` results.
#[derive(Debug, Default, Clone)]
pub struct AccessWatch {
    strikes: u32,
    last_strike: Option<u64>,
}

impl AccessWatch {
    pub fn success(&mut self) {
        *self = AccessWatch::default();
    }

    /// A `401` on our own `state_get` at `now`, with the provider's `Date`
    /// and whether an unauthenticated locate answered.
    pub fn unauthorized(&mut self, now: u64, provider_date: Option<u64>, locate_ok: bool) -> AccessSignal {
        if provider_date.is_some_and(|d| d.abs_diff(now) > CLOCK_WINDOW) {
            return AccessSignal::ClockSkew;
        }
        if !locate_ok || provider_date.is_none() {
            return AccessSignal::Suspect; // provider not otherwise reachable: no conclusion
        }
        let spaced = self.last_strike.is_none_or(|t| now.saturating_sub(t) >= ACCESS_LOST_SPACING);
        if spaced {
            self.strikes += 1;
            self.last_strike = Some(now);
        }
        if self.strikes >= ACCESS_LOST_STRIKES {
            AccessSignal::AccessLost
        } else {
            AccessSignal::Suspect
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_schedule() {
        assert_eq!([backoff(1), backoff(2), backoff(3), backoff(9)], [60, 300, 900, 900]);
        assert!(stale(None, 0) && stale(Some(0), STALE_AFTER) && !stale(Some(10), 20));
        assert!(!revocation_failed(2) && revocation_failed(3));
    }

    /// BK-13: three 401s at least 5 minutes apart, provider otherwise
    /// reachable, clock fine → access lost; rapid retries do not count.
    #[test]
    fn access_lost_needs_spaced_strikes() {
        let mut w = AccessWatch::default();
        let t = 1_900_000_000;
        assert_eq!(w.unauthorized(t, Some(t), true), AccessSignal::Suspect);
        assert_eq!(w.unauthorized(t + 10, Some(t + 10), true), AccessSignal::Suspect, "too soon");
        assert_eq!(w.unauthorized(t + 300, Some(t + 300), true), AccessSignal::Suspect);
        assert_eq!(w.unauthorized(t + 600, Some(t + 600), true), AccessSignal::AccessLost);
        let mut w = AccessWatch::default();
        assert_eq!(w.unauthorized(t, Some(t + 3600), true), AccessSignal::ClockSkew);
        assert_eq!(w.unauthorized(t, Some(t), false), AccessSignal::Suspect, "locate down: no conclusion");
        w.success();
        assert_eq!(w.unauthorized(t, Some(t), true), AccessSignal::Suspect);
    }
}
