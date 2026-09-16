//! `/v1/agent`: the phone's two-way connection for agent sessions.
//!
//! On connect the Mac sends a snapshot, then pushes what changes: the session
//! list (when session files change), the open conversation's messages, and
//! turn events from conversations SOURCE is driving. The phone can open a
//! conversation and send prompts, typed or spoken (see `agent_voice.rs`).

use super::agent_feed::{epoch, AgentServices};
use super::agent_frames::{parse_client_frame, ClientFrame, ServerBody, ServerFrame, TalkState};
use super::agent_send_window::Decision;
use super::agent_voice::{Talks, VoiceServices};
use super::server::MobileState;
use crate::core::agent_bridge::SendError;
use crate::core::agent_sessions::{AgentApp, AgentMessage, AgentSession};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use futures::{SinkExt, StreamExt};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc};

const PING_EVERY: Duration = Duration::from_secs(25);
const DEAD_AFTER: Duration = Duration::from_secs(60);
const OUTBOX: usize = 256;

/// Header auth only: this connection can drive coding agents, so the token must
/// not travel in the URL, where it would end up in logs.
pub(super) async fn agent_ws(
    State(state): State<MobileState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<axum::response::Response, StatusCode> {
    let token = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;
    state.pairing.verify(token).await.ok_or(StatusCode::UNAUTHORIZED)?;
    let agents = state.agents.clone().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let voice = VoiceServices {
        commands: state.commands.clone(),
        db: Some(state.db.clone()),
        session: state.session.clone(),
        audio_dir: None,
    };
    Ok(ws.on_upgrade(move |socket| run(socket, agents, voice)))
}

async fn run(socket: WebSocket, agents: AgentServices, voice: VoiceServices) {
    let (mut sink, mut incoming) = socket.split();
    let (out, mut outbox) = mpsc::channel::<ServerBody>(OUTBOX);
    let writer = tokio::spawn(async move {
        let mut seq = 0u64;
        while let Some(body) = outbox.recv().await {
            seq += 1;
            let frame = ServerFrame { seq, body }.to_json();
            if sink.send(Message::Text(frame.into())).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });

    let mut connection = Connection::new(agents.clone(), Some(voice), out);
    let mut changes = agents.changes.clone();
    changes.borrow_and_update();
    let mut can_send_changes = agents.can_send_changes.clone();
    can_send_changes.borrow_and_update();
    let mut turns = agents.bridge.subscribe();
    let mut ping = tokio::time::interval(PING_EVERY);
    ping.tick().await; // the first tick is immediate
    let mut unanswered_ping: Option<Instant> = None;

    let mut alive = connection.send_snapshot().await;
    while alive {
        alive = tokio::select! {
            frame = incoming.next() => match frame {
                Some(Ok(Message::Text(text))) => match parse_client_frame(&text) {
                    Some(ClientFrame::Pong) => { unanswered_ping = None; true }
                    Some(frame) => connection.handle(frame).await,
                    None => connection.push(ServerBody::Error { message: "Unknown message.".into() }),
                },
                Some(Ok(Message::Binary(bytes))) => { connection.audio(&bytes); true }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => false,
                Some(Ok(_)) => true,
            },
            changed = changes.changed() => changed.is_ok() && connection.refresh().await,
            changed = can_send_changes.changed() => changed.is_ok() && connection.send_can_send(),
            turn = turns.recv() => match turn {
                Ok(event) => connection.push(ServerBody::Turn {
                    session_id: event.session_id,
                    event: event.event,
                    brief: event.brief,
                    still_working: event.still_working,
                }),
                Err(broadcast::error::RecvError::Lagged(_)) => connection.send_snapshot().await,
                Err(broadcast::error::RecvError::Closed) => false,
            },
            _ = ping.tick() => {
                let dead = unanswered_ping.is_some_and(|since| since.elapsed() >= DEAD_AFTER);
                unanswered_ping.get_or_insert_with(Instant::now);
                !dead && connection.push(ServerBody::Ping)
            }
        };
    }
    drop(connection);
    let _ = writer.await;
}

/// One phone connection's state. Pushing never waits: if the phone can't keep
/// up and the outbox fills, the connection closes and the phone reconnects to
/// a fresh snapshot, rather than falling silently behind.
pub(super) struct Connection {
    agents: AgentServices,
    voice: Option<VoiceServices>,
    out: mpsc::Sender<ServerBody>,
    open: Option<(AgentApp, String)>,
    last_sessions: Option<(Vec<AgentSession>, Vec<String>)>,
    last_messages: Option<Vec<AgentMessage>>,
    talks: Talks,
}

impl Connection {
    pub(super) fn new(agents: AgentServices, voice: Option<VoiceServices>, out: mpsc::Sender<ServerBody>) -> Self {
        Self { agents, voice, out, open: None, last_sessions: None, last_messages: None, talks: Talks::default() }
    }

    /// Returns false when the connection should close.
    pub(super) fn push(&self, body: ServerBody) -> bool {
        self.out.try_send(body).is_ok()
    }

    pub(super) async fn send_snapshot(&mut self) -> bool {
        let (sessions, held) = self.agents.sessions().await;
        self.last_sessions = Some((sessions.clone(), held.clone()));
        let can_send = self.agents.phone_may_send();
        self.push(ServerBody::Snapshot { epoch: epoch().to_string(), sessions, held, can_send })
            && self.send_messages(true).await
    }

    pub(super) async fn handle(&mut self, frame: ClientFrame) -> bool {
        match frame {
            ClientFrame::Hello => self.send_snapshot().await,
            ClientFrame::OpenSession { app, session_id } => {
                self.open = Some((app, session_id));
                self.send_messages(true).await
            }
            ClientFrame::CloseSession => {
                self.open = None;
                self.last_messages = None;
                true
            }
            ClientFrame::SendText { request_id, session_id, text } => self.send_text(request_id, session_id, text),
            ClientFrame::TalkStart { talk_id, session_id } => self.start_talk(talk_id, session_id),
            ClientFrame::TalkEnd { talk_id } => {
                if let Some(voice) = &self.voice {
                    self.talks.end(&talk_id, &self.agents, voice, &self.out);
                }
                true
            }
            ClientFrame::TalkCancel { talk_id } => match self.talks.cancel(&talk_id) {
                Some(session_id) => self.push(ServerBody::Talk { talk_id, session_id, state: TalkState::Cancelled }),
                None => true,
            },
            ClientFrame::SendNow { talk_id } => {
                self.talks.decide(&talk_id, Decision::SendNow);
                true
            }
            ClientFrame::CancelSend { talk_id } => {
                self.talks.decide(&talk_id, Decision::Cancel);
                true
            }
            ClientFrame::Pong => true,
        }
    }

    /// Audio for the voice prompt being recorded, if there is one.
    pub(super) fn audio(&mut self, bytes: &[u8]) {
        self.talks.audio(bytes);
    }

    fn start_talk(&mut self, talk_id: String, session_id: String) -> bool {
        let refusal = if self.voice.is_none() {
            Some("Voice prompts aren't available on this Mac.")
        } else if !self.agents.phone_may_send() {
            Some(SENDING_OFF)
        } else {
            None
        };
        if let Some(message) = refusal {
            let state = TalkState::Failed { message: message.into() };
            return self.push(ServerBody::Talk { talk_id, session_id, state });
        }
        self.talks.start(talk_id, session_id);
        true
    }

    /// The setting was switched: tell the phone what it is now.
    pub(super) fn send_can_send(&self) -> bool {
        self.push(ServerBody::CanSend { can_send: self.agents.phone_may_send() })
    }

    /// Session files changed: send whatever actually differs.
    pub(super) async fn refresh(&mut self) -> bool {
        let current = self.agents.sessions().await;
        if self.last_sessions.as_ref() != Some(&current) {
            let (sessions, held) = current.clone();
            self.last_sessions = Some(current);
            if !self.push(ServerBody::Sessions { sessions, held }) {
                return false;
            }
        }
        self.send_messages(false).await
    }

    async fn send_messages(&mut self, force: bool) -> bool {
        let Some((app, session_id)) = self.open.clone() else { return true };
        let Some(messages) = self.agents.messages(app, &session_id).await else { return true };
        if !force && self.last_messages.as_ref() == Some(&messages) {
            return true;
        }
        self.last_messages = Some(messages.clone());
        self.push(ServerBody::Messages { session_id, messages })
    }

    /// Sending can take seconds (handing the conversation over, starting
    /// Claude), so it runs on its own task and answers with `send_result`.
    fn send_text(&self, request_id: String, session_id: String, text: String) -> bool {
        let (agents, out) = (self.agents.clone(), self.out.clone());
        tokio::spawn(async move { deliver_prompt(&agents, &out, request_id, session_id, text).await });
        true
    }
}

const SENDING_OFF: &str =
    "Sending prompts from the phone is turned off. Turn it on in SOURCE on the Mac: Settings → Mobile.";

/// Send a prompt from the phone, typed or spoken, and answer with `send_result`.
/// The setting is checked here, at the moment of sending, so switching it off
/// stops a voice prompt still in its confirm window too.
pub(super) async fn deliver_prompt(
    agents: &AgentServices,
    out: &mpsc::Sender<ServerBody>,
    request_id: String,
    session_id: String,
    text: String,
) {
    let refusal = if text.trim().is_empty() {
        Some("Nothing to send.")
    } else if !agents.phone_may_send() {
        Some(SENDING_OFF)
    } else {
        None
    };
    let body = match refusal {
        Some(message) => {
            let error = SendError::Driver { message: message.into() };
            ServerBody::SendResult { request_id, ok: false, error: Some(error) }
        }
        None => match agents.bridge.send_prompt(&session_id, text.trim()).await {
            Ok(_) => ServerBody::SendResult { request_id, ok: true, error: None },
            Err(error) => ServerBody::SendResult { request_id, ok: false, error: Some(error) },
        },
    };
    let _ = out.try_send(body);
}
