/// Single-mic policy: one capture owner, shared clock, foreground priority.
///
/// Background transcription yields while a Right Option session is active;
/// chunks captured during the session buffer and resume afterwards. The
/// foreground transcript becomes the timeline entry for that span — the
/// same speech is never transcribed twice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureMode {
    Background,
    Foreground { session_id: String },
}

pub struct ForegroundCoordinator {
    mode: CaptureMode,
    buffered_background_chunks: u64,
    last_session_started_ms: Option<i64>,
}

impl ForegroundCoordinator {
    pub fn new() -> Self {
        Self {
            mode: CaptureMode::Background,
            buffered_background_chunks: 0,
            last_session_started_ms: None,
        }
    }

    pub fn mode(&self) -> &CaptureMode {
        &self.mode
    }

    pub fn on_session_started(&mut self, session_id: &str, now_ms: i64) {
        self.mode = CaptureMode::Foreground {
            session_id: session_id.to_string(),
        };
        self.last_session_started_ms = Some(now_ms);
    }

    /// Returns the closed session id, if any.
    pub fn on_session_stopped(&mut self, session_id: &str) -> Option<String> {
        match &self.mode {
            CaptureMode::Foreground { session_id: active } if active == session_id => {
                self.mode = CaptureMode::Background;
                Some(session_id.to_string())
            }
            _ => None,
        }
    }

    /// Background chunks arriving mid-dictation buffer instead of running ASR.
    /// Returns true when the chunk should be transcribed immediately.
    pub fn admit_background_chunk(&mut self) -> bool {
        match self.mode {
            CaptureMode::Background => true,
            CaptureMode::Foreground { .. } => {
                self.buffered_background_chunks += 1;
                false
            }
        }
    }

    pub fn buffered_background_chunks(&self) -> u64 {
        self.buffered_background_chunks
    }

    pub fn drain_buffered(&mut self) -> u64 {
        std::mem::take(&mut self.buffered_background_chunks)
    }
}

impl Default for ForegroundCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared clock for mic/desktop/helper timestamps (Unix millis).
pub fn capture_timestamp_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_chunks_flow_when_idle() {
        let mut coordinator = ForegroundCoordinator::new();
        assert!(coordinator.admit_background_chunk());
        assert_eq!(coordinator.buffered_background_chunks(), 0);
    }

    #[test]
    fn foreground_buffers_and_resumes() {
        let mut coordinator = ForegroundCoordinator::new();
        coordinator.on_session_started("sess-1", 1000);
        assert!(!coordinator.admit_background_chunk());
        assert!(!coordinator.admit_background_chunk());
        assert_eq!(coordinator.buffered_background_chunks(), 2);
        assert_eq!(
            coordinator.on_session_stopped("sess-1"),
            Some("sess-1".to_string())
        );
        assert!(coordinator.admit_background_chunk());
        assert_eq!(coordinator.drain_buffered(), 2);
        assert_eq!(coordinator.buffered_background_chunks(), 0);
    }

    #[test]
    fn wrong_session_id_does_not_release() {
        let mut coordinator = ForegroundCoordinator::new();
        coordinator.on_session_started("sess-1", 1000);
        assert_eq!(coordinator.on_session_stopped("sess-2"), None);
        assert!(matches!(
            coordinator.mode(),
            CaptureMode::Foreground { .. }
        ));
    }

    #[test]
    fn timestamps_are_monotonic() {
        assert!(capture_timestamp_ms() <= capture_timestamp_ms());
    }
}
