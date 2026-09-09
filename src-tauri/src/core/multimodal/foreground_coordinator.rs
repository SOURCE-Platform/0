use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use tokio::sync::watch;

static BACKGROUND_TRANSCRIPTION_PAUSED: OnceLock<watch::Sender<bool>> = OnceLock::new();
static BACKGROUND_CAPTURE_EPOCH: AtomicU64 = AtomicU64::new(0);

fn background_transcription_gate() -> &'static watch::Sender<bool> {
    BACKGROUND_TRANSCRIPTION_PAUSED.get_or_init(|| watch::channel(false).0)
}

/// Share Right Option state with background capture tasks. Dictation runs in
/// a separate helper process, so this small gate is the cross-process policy
/// point that prevents ambient inference from competing with it.
/// Non-blocking read of the gate.
///
/// Callers driving an event loop must use this rather than awaiting the gate:
/// awaiting inside the loop blocks the very branch that would later open it.
pub fn is_background_transcription_paused() -> bool {
    *background_transcription_gate().borrow()
}

pub fn set_background_transcription_paused(paused: bool) {
    let was_paused = background_transcription_gate().send_replace(paused);
    if paused && !was_paused {
        BACKGROUND_CAPTURE_EPOCH.fetch_add(1, Ordering::SeqCst);
    }
}

pub fn background_transcription_is_paused() -> bool {
    *background_transcription_gate().borrow()
}

pub fn background_capture_epoch() -> u64 {
    BACKGROUND_CAPTURE_EPOCH.load(Ordering::SeqCst)
}

pub async fn wait_for_background_transcription() {
    let mut paused = background_transcription_gate().subscribe();
    while *paused.borrow() {
        if paused.changed().await.is_err() {
            return;
        }
    }
}

/// Single-mic policy: one capture owner, shared clock, foreground priority.
///
/// Background work yields while a Right Option session is active. The
/// foreground transcript becomes the timeline entry for that span.
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
        assert!(matches!(coordinator.mode(), CaptureMode::Foreground { .. }));
    }

    #[test]
    fn timestamps_are_monotonic() {
        assert!(capture_timestamp_ms() <= capture_timestamp_ms());
    }

    #[tokio::test]
    async fn background_gate_waits_for_foreground_release() {
        use tokio::time::{timeout, Duration};

        set_background_transcription_paused(true);
        let mut waiter = tokio::spawn(wait_for_background_transcription());
        let stayed_paused = timeout(Duration::from_millis(20), &mut waiter)
            .await
            .is_err();
        set_background_transcription_paused(false);

        assert!(stayed_paused);
        assert!(timeout(Duration::from_millis(200), waiter).await.is_ok());
    }
}

#[cfg(test)]
mod gate_tests {
    use super::*;

    #[test]
    fn gate_read_reflects_pause_state() {
        set_background_transcription_paused(false);
        assert!(!is_background_transcription_paused());
        set_background_transcription_paused(true);
        assert!(is_background_transcription_paused());
        set_background_transcription_paused(false);
        assert!(!is_background_transcription_paused());
    }

    #[tokio::test]
    async fn gate_read_is_non_blocking_while_paused() {
        // Awaiting the gate here is what deadlocked the supervisor loop:
        // the read must return immediately even while dictation holds it.
        set_background_transcription_paused(true);
        let paused = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            async { is_background_transcription_paused() },
        )
        .await
        .expect("gate read must not block");
        assert!(paused);
        set_background_transcription_paused(false);
    }
}
