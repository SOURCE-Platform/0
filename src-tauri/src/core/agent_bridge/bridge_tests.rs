use super::brief::{brief_reply, first_sentences, plain_text};
use super::bridge::{AgentBridge, SendError};
use super::driver_events::DriverEvent;
use super::handoff::HandoffError;
use super::test_support::{fake_claude, temp_roots, write_transcript};
use std::time::Duration;

const IDLE_END: &str = r#"{"type":"assistant","message":{"stop_reason":"end_turn","content":[{"type":"text","text":"Hello"}]}}"#;
const BUSY_END: &str = r#"{"type":"assistant","message":{"stop_reason":"tool_use","content":[{"type":"tool_use","name":"Bash"}]}}"#;

#[tokio::test]
async fn sends_a_prompt_and_broadcasts_the_brief_reply() {
    let roots = temp_roots("send");
    write_transcript(&roots, "s-1", IDLE_END);
    let bridge = AgentBridge::with_binary(roots, Some(fake_claude("bridge_send")), Duration::from_secs(60));
    let mut events = bridge.subscribe();

    assert_eq!(bridge.send_prompt("s-1", "reply DONE").await.unwrap(), DriverEvent::Working);

    let done = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = events.recv().await.unwrap();
            if matches!(event.event, DriverEvent::TurnDone { .. }) {
                return event;
            }
        }
    })
    .await
    .expect("turn finished");
    assert_eq!(done.session_id, "s-1");
    assert_eq!(done.brief.as_deref(), Some("DONE"));

    // Handed back without being asked.
    tokio::time::timeout(Duration::from_secs(5), async {
        while events.recv().await.unwrap().event != DriverEvent::Released {}
    })
    .await
    .expect("released after the reply");
    assert!(bridge.held_sessions().await.is_empty());
}

#[tokio::test]
async fn refuses_a_conversation_that_is_busy_in_the_claude_app() {
    let roots = temp_roots("busy");
    write_transcript(&roots, "s-1", BUSY_END);
    let mut app = std::process::Command::new("sleep").arg("30").spawn().unwrap();
    std::fs::write(
        roots.claude.join(format!("sessions/{}.json", app.id())),
        format!(r#"{{"pid":{},"sessionId":"s-1","entrypoint":"claude-desktop"}}"#, app.id()),
    )
    .unwrap();
    let bridge = AgentBridge::with_binary(roots, Some(fake_claude("bridge_busy")), Duration::from_secs(60));

    let refused = bridge.send_prompt("s-1", "hello").await;
    assert_eq!(refused, Err(SendError::Handoff(HandoffError::BusyInApp)));
    assert!(app.try_wait().unwrap().is_none(), "the app's process was left alone");
    app.kill().unwrap();
    let _ = app.wait();
}

#[tokio::test]
async fn explains_missing_pieces() {
    let roots = temp_roots("missing");
    write_transcript(&roots, "s-1", IDLE_END);
    let no_program = AgentBridge::with_binary(roots.clone(), None, Duration::from_secs(60));
    assert_eq!(no_program.send_prompt("s-1", "hi").await, Err(SendError::NoClaudeProgram));
    let bridge = AgentBridge::with_binary(roots, Some(fake_claude("bridge_missing")), Duration::from_secs(60));
    assert_eq!(
        bridge.send_prompt("nope", "hi").await,
        Err(SendError::Handoff(HandoffError::NoTranscript))
    );
}

#[test]
fn briefs_are_plain_and_short() {
    let reply = "## Done\n\nThe header now **hides** on scroll down. It comes back when you scroll up. \
                 I also tidied `Header.tsx`.\n\n```tsx\nexport function Header() {}\n```";
    assert_eq!(brief_reply(reply, None), "The header now hides on scroll down. It comes back when you scroll up.");
}

#[test]
fn briefs_fall_back_to_claudes_recap_or_done() {
    assert_eq!(brief_reply("```\ncode only\n```", Some("ran the tests; all passed")), "Ran the tests; all passed.");
    assert_eq!(brief_reply("", None), "Done.");
}

#[test]
fn briefs_keep_links_words_and_version_numbers() {
    assert_eq!(plain_text("See [the docs](https://example.com) for details."), "See the docs for details.");
    assert_eq!(first_sentences("Updated to v2.1.266 today. Tests pass. Deployed.", 2), "Updated to v2.1.266 today. Tests pass.");
    assert_eq!(plain_text("1. First step\n- second step\n| a | b |"), "First step second step");
}

#[test]
fn briefs_are_capped_at_a_word_boundary() {
    let long = format!("{} end", "word ".repeat(100));
    let brief = brief_reply(&long, None);
    assert!(brief.chars().count() <= 280, "{}", brief.len());
    assert!(brief.ends_with("word…"), "{brief}");
}

/// Real end-to-end run against a Claude desktop conversation you name:
/// `SOURCE_LIVE_SESSION=<session id> cargo test --lib bridge_tests::live_bridge -- --ignored --nocapture`
/// Use a throwaway conversation: it is taken over from the Claude app.
#[tokio::test]
#[ignore = "takes over a real Claude conversation and uses plan usage"]
async fn live_bridge() {
    let session_id = std::env::var("SOURCE_LIVE_SESSION").expect("set SOURCE_LIVE_SESSION");
    let bridge = AgentBridge::new(crate::core::agent_sessions::AgentRoots::from_env());
    let mut events = bridge.subscribe();
    let started = std::time::Instant::now();
    let sent = bridge.send_prompt(&session_id, "This is a test from SOURCE. Reply with only the word PONG.").await;
    println!("send: {sent:?} after {:?}", started.elapsed());
    sent.expect("sent");
    let done = tokio::time::timeout(Duration::from_secs(120), async {
        loop {
            let event = events.recv().await.unwrap();
            println!("  {:?} brief={:?}", event.event, event.brief);
            if matches!(event.event, DriverEvent::TurnDone { .. } | DriverEvent::AuthExpired) {
                return event;
            }
        }
    })
    .await
    .expect("turn finished");
    println!("done after {:?}: brief={:?}", started.elapsed(), done.brief);
    bridge.release(&session_id).await;
}
