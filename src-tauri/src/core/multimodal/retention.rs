use std::time::{Duration, SystemTime};

/// Raw-audio retention policy (Phase 10): bounded disk, foreground kept.
/// Evidence older than `max_age` or beyond `max_bytes` (oldest first)
/// becomes a deletion candidate. Foreground dictation audio is pinned
/// until its transcript is acknowledged on the timeline.
pub struct RetentionPolicy {
    pub max_age: Duration,
    pub max_bytes: u64,
}

pub struct AudioCandidate {
    pub id: String,
    pub bytes: u64,
    pub captured_at: SystemTime,
    pub pinned_foreground: bool,
    pub transcript_acknowledged: bool,
}

impl RetentionPolicy {
    pub fn should_delete(&self, candidate: &AudioCandidate, now: SystemTime) -> bool {
        if candidate.pinned_foreground && !candidate.transcript_acknowledged {
            return false;
        }
        let age = now
            .duration_since(candidate.captured_at)
            .unwrap_or(Duration::ZERO);
        age > self.max_age
    }

    /// Oldest-first deletion order to fit `current_bytes` under budget.
    pub fn over_budget_deletions<'a>(
        &self,
        candidates: &'a [AudioCandidate],
        current_bytes: u64,
    ) -> Vec<&'a str> {
        if current_bytes <= self.max_bytes {
            return Vec::new();
        }
        let mut ordered: Vec<&AudioCandidate> = candidates
            .iter()
            .filter(|candidate| !candidate.pinned_foreground)
            .collect();
        ordered.sort_by_key(|candidate| candidate.captured_at);
        let mut freed = 0;
        let target = current_bytes - self.max_bytes;
        ordered
            .into_iter()
            .take_while(|candidate| {
                let take = freed < target;
                freed += candidate.bytes;
                take
            })
            .map(|candidate| candidate.id.as_str())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: &str, age_secs: u64, bytes: u64) -> AudioCandidate {
        AudioCandidate {
            id: id.to_string(),
            bytes,
            captured_at: SystemTime::now() - Duration::from_secs(age_secs),
            pinned_foreground: false,
            transcript_acknowledged: true,
        }
    }

    #[test]
    fn old_audio_expires() {
        let policy = RetentionPolicy {
            max_age: Duration::from_secs(7 * 24 * 3600),
            max_bytes: u64::MAX,
        };
        assert!(policy.should_delete(&candidate("old", 8 * 24 * 3600, 10), SystemTime::now()));
        assert!(!policy.should_delete(&candidate("new", 3600, 10), SystemTime::now()));
    }

    #[test]
    fn unacknowledged_foreground_is_pinned() {
        let policy = RetentionPolicy {
            max_age: Duration::ZERO,
            max_bytes: u64::MAX,
        };
        let mut pinned = candidate("fg", 999_999, 10);
        pinned.pinned_foreground = true;
        pinned.transcript_acknowledged = false;
        assert!(!policy.should_delete(&pinned, SystemTime::now()));
    }

    #[test]
    fn over_budget_deletes_oldest_first() {
        let policy = RetentionPolicy {
            max_age: Duration::from_secs(7 * 24 * 3600),
            max_bytes: 100,
        };
        let files = vec![
            candidate("new", 10, 60),
            candidate("mid", 100, 60),
            candidate("old", 200, 60),
        ];
        assert_eq!(
            policy.over_budget_deletions(&files, 180),
            vec!["old", "mid"]
        );
    }
}
