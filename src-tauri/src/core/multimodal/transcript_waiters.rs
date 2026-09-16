//! Wait for the transcript of one specific file.
//!
//! Transcription results come back through the dictation action loop with no
//! reply channel of their own. Code that needs one particular result (a voice
//! prompt from the phone) registers its id first, sends `TranscribeFile`, and
//! awaits the receiver. The action loop completes it.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tokio::sync::oneshot;

#[derive(Default)]
pub struct TranscriptWaiters {
    waiting: Mutex<HashMap<String, oneshot::Sender<String>>>,
}

impl TranscriptWaiters {
    /// Start waiting for `id`. Register before dispatching the transcription,
    /// so a very short clip can't finish before anyone is listening.
    pub fn register(&self, id: &str) -> oneshot::Receiver<String> {
        let (sender, receiver) = oneshot::channel();
        if let Ok(mut map) = self.waiting.lock() {
            map.insert(id.to_string(), sender);
        }
        receiver
    }

    /// The transcript for `id` is ready. Empty text means no speech was heard.
    /// Unknown ids, and waiters that stopped listening, are ignored.
    pub fn complete(&self, id: &str, text: &str) {
        let sender = self.waiting.lock().ok().and_then(|mut map| map.remove(id));
        if let Some(sender) = sender {
            let _ = sender.send(text.to_string());
        }
    }

    /// Stop waiting for `id` without a result, as when the waiter gives up.
    pub fn forget(&self, id: &str) {
        if let Ok(mut map) = self.waiting.lock() {
            map.remove(id);
        }
    }

    /// The speech engine went away: every receiver reports an error instead of
    /// hanging until its timeout.
    pub fn fail_all(&self) {
        if let Ok(mut map) = self.waiting.lock() {
            map.clear();
        }
    }
}

/// The app's one set of waiters, shared by the action loop and the phone server.
pub fn transcript_waiters() -> &'static TranscriptWaiters {
    static WAITERS: OnceLock<TranscriptWaiters> = OnceLock::new();
    WAITERS.get_or_init(TranscriptWaiters::default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn completes_whether_the_result_lands_before_or_after_awaiting() {
        let waiters = TranscriptWaiters::default();
        let early = waiters.register("early");
        waiters.complete("early", "hello");
        assert_eq!(early.await.unwrap(), "hello");

        let late = tokio::spawn(waiters.register("late"));
        tokio::task::yield_now().await;
        waiters.complete("late", "there");
        assert_eq!(late.await.unwrap().unwrap(), "there");
    }

    #[test]
    fn ignores_unknown_ids_and_waiters_that_left() {
        let waiters = TranscriptWaiters::default();
        waiters.complete("nobody", "ignored");
        drop(waiters.register("left"));
        waiters.complete("left", "ignored");
    }

    #[tokio::test]
    async fn forgetting_or_failing_all_ends_the_wait_without_a_result() {
        let waiters = TranscriptWaiters::default();
        let forgotten = waiters.register("forgotten");
        waiters.forget("forgotten");
        assert!(forgotten.await.is_err());

        let failed = waiters.register("failed");
        waiters.fail_all();
        assert!(failed.await.is_err());
    }
}
