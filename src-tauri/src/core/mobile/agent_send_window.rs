//! The few seconds between hearing a voice prompt and sending it.
//!
//! The phone shows what was heard with a countdown, Cancel and Send now. The
//! Mac owns the countdown, not the phone: if the phone drops out after showing
//! the transcript, the prompt still goes when the window ends instead of being
//! stranded half-sent.

use std::time::Duration;
use tokio::sync::mpsc;

/// Long enough to read a sentence and cancel it, short enough not to wait on.
pub const CONFIRM_WINDOW: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    SendNow,
    Cancel,
}

/// Wait out the window. True to send.
pub async fn resolve(window: Duration, mut decisions: mpsc::Receiver<Decision>) -> bool {
    let deadline = tokio::time::sleep(window);
    tokio::pin!(deadline);
    tokio::select! {
        _ = &mut deadline => true,
        decision = decisions.recv() => match decision {
            Some(Decision::SendNow) => true,
            Some(Decision::Cancel) => false,
            // The phone went away: the window still runs its course, then sends.
            None => {
                deadline.await;
                true
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    const WINDOW: Duration = Duration::from_millis(300);

    #[tokio::test]
    async fn sends_when_the_window_ends() {
        let (_decide, decisions) = mpsc::channel(1);
        let started = Instant::now();
        assert!(resolve(WINDOW, decisions).await);
        assert!(started.elapsed() >= WINDOW);
    }

    #[tokio::test]
    async fn send_now_skips_the_rest_of_the_window() {
        let (decide, decisions) = mpsc::channel(1);
        decide.send(Decision::SendNow).await.unwrap();
        let started = Instant::now();
        assert!(resolve(WINDOW, decisions).await);
        assert!(started.elapsed() < WINDOW);
    }

    #[tokio::test]
    async fn cancel_stops_the_send() {
        let (decide, decisions) = mpsc::channel(1);
        decide.send(Decision::Cancel).await.unwrap();
        assert!(!resolve(WINDOW, decisions).await);
    }

    #[tokio::test]
    async fn a_phone_that_drops_out_still_gets_its_prompt_sent_on_time() {
        let (decide, decisions) = mpsc::channel(1);
        drop(decide);
        let started = Instant::now();
        assert!(resolve(WINDOW, decisions).await);
        assert!(started.elapsed() >= WINDOW, "not early");
    }
}
