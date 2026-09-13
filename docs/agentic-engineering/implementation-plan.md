# Implementation plan

The build order for Agentic Engineering, with the steps split across both repos
and a clear "done when" for each.

- Spec and research: [README.md](README.md) and the documents it links.
- Repos:
  - **Mac:** `SOURCE-Platform/0` (this repo).
  - **iPhone:** `SOURCE-Platform/source-mobile` (`~/Documents/source mobile`).

## In plain terms

The first thing to make usable is the core loop, on home Wi-Fi:

1. Open a Claude Code session on your phone.
2. Hold a button and say what you want.
3. Your words land in that session on the Mac.
4. When Claude finishes, the phone speaks a one- or two-sentence summary.

Everything else comes after that loop works reliably. Order:

1. The other three apps.
2. Using it away from home.
3. Hands-free streaming with smart routing.
4. Catch me up.
5. Live view of the app you're building.

Before building anything, one question needs a yes (M0). When another program
sends a message into a Claude session, Claude labels it as coming from another
agent, not from you. We haven't tested whether Claude acts on an instruction
sent that way.

## Status

| Milestone | What | Status |
|---|---|---|
| Research | All four apps reachable from outside; steering proven; router and speech proven | ✅ Done |
| Session hub v1 | Agents tab on the Mac listing every session from all four apps | ✅ Done |
| M0 | Go/no-go spikes for Claude delivery | ⬜ Next |
| M1 | Mac-only loop: send to Claude, watch the reply | ⬜ |
| M2 | Phone ↔ Mac agent channel, sessions on the phone | ⬜ |
| M3 | Push-to-talk with spoken reply (**first usable slice**) | ⬜ |
| M3b | Kokoro voice | ⬜ |
| M4 | Away from home (Tailscale) | ⬜ |
| M5 | Steer/queue + Codex, OpenCode, Factory | ⬜ |
| M6 | Hands-free streaming + local router | ⬜ |
| M7 | Catch me up, global chat, approvals, agent speaks first | ⬜ |
| M8 | Live view + auto-start the app | ⬜ |
| M9 | Mac screen streaming | ⬜ |
| M10 | ChatGPT web | ⬜ |

Update the Status column as tasks land.

## Principles

- **Reading and writing live in separate modules.** `core/agent_sessions` only
  reads the agent apps' records, as its module doc promises. Anything that sends
  goes in `core/agent_bridge`.
- **Event-driven, no polling.** Watch transcript and registry files with
  FSEvents (the `notify` crate, never its polling backend). No `setInterval` or
  sleep loops. A WebSocket ping every 25 s is liveness detection, not data
  polling.
- **Respect the agent apps' permission systems.** SOURCE never tries to make its
  messages count as your approval. Approval prompts stay on the Mac until an
  app's own supported approval path is wired up (M7).
- **A paired phone becomes powerful, so:**
  - add a Mac setting "Allow phone to send prompts to agents", off by default;
  - `/v1/agent` accepts header auth only;
  - spoken prompts stay visible in the timeline.
- **Both repos:**
  - every source file stays at or under 350 lines, split by responsibility;
  - the phone repo regenerates its Xcode project with `xcodegen generate` after
    adding files.

## What the code review found

These findings changed the design:

1. **Messages between Claude sessions arrive labelled as coming from another
   agent,** not from the user. Whether the target acts on them is untested, so it
   becomes the M0 gate.
2. **Delivery can be confirmed exactly.** The relay's SendMessage result carries
   a `msg_id`. The target's transcript then gets an attachment with
   `origin.msg_id` and the verbatim `origin.body`.
3. **Session names change** (`0-e0` later became `0-c1`); session ids don't.
   - Resolve the name from `~/.claude/sessions/*.json` by `sessionId` right
     before every send.
   - Idle desktop sessions lose their process, so only live sessions can
     receive.
4. **Turn end in the transcript is subtle.**
   - One API message is written as several records sharing a `message.id`.
   - `stop_hook_summary` and `last-prompt` records are not end markers.
   - API errors appear as `stop_sequence` text.
5. **No per-request transcript result exists.** The waiter must complete in the
   action loop in `app/dictation_supervision.rs`, on both `PersistForeground`
   and `DuplicateIgnored`. Empty or failed transcripts never reach
   `persist_action`, so a waiter there would hang.
6. **A bundled app can't find `claude` on PATH.** Resolve the binary:
   `SOURCE_CLAUDE_BIN`, then the newest desktop-bundled CLI, then
   `~/.local/bin`, then `/opt/homebrew/bin`.
7. **Phone capture always writes a clip** and queues it for upload.
   Push-to-talk needs capture without a file.
8. **The Agents tab re-reads every 10 s.** Watcher events will replace that.
9. **FluidAudio's Kokoro also runs on iOS 17,** so speech could be generated on
   the phone instead of the Mac.
10. **Not audited:** `native-pkg/.../overlay.swift` is 537 lines. The
    file-length audit only scans `src/` and `src-tauri/src/`. Tracked as a
    separate cleanup.

---

## M0: Go/no-go spikes

Throwaway scripts in a scratch folder, against a throwaway Claude desktop
session. Save useful output as test fixtures for M1.

| Spike | Pass when | If it fails |
|---|---|---|
| **P-auth** | The relay delivers "create `voice-proof.txt` containing BANANA, then reply DONE". The target does it and replies in its own transcript. Record any approval prompt. Try with and without a framing prefix (`Adam, by voice via SOURCE:`). | Choose a delivery fallback before M1: a desktop-hosted relay using Claude's own session-send tool, or typed insertion as a last resort. |
| **P-flags** | A long-lived, trimmed relay works: `--model haiku --tools SendMessage --allowedTools SendMessage --system-prompt … --strict-mcp-config --setting-sources "" --no-session-persistence`. `uds:<socket>` addressing works. A warm send takes under 5 s. Save the stdout lines as fixtures. | Keep ListAgents and name addressing; accept slower sends. |
| **P-shape** | Transcript records captured for delivery to an **idle** target and to a **busy** one. | – |
| **P-timing** | Measured delay between an assistant message's blocks landing on disk, which sets the settle window. | Use a longer settle window. |
| **P-live** | Known: how long an idle desktop session stays in the registry; whether opening it restarts a process; whether the relay triggers Keychain prompts. | – |

---

## M1: Mac-only loop

Prove sending and reply capture with a prompt box on the Mac, before any phone
code. Paths are under `src-tauri/src/` unless they start with `src/`.

### 1.1 Split `dictation_helper.rs`

✅ Done.

- **What changed:** the protocol types, `parse_helper_line` and their tests moved
  to `core/multimodal/dictation_helper_protocol.rs` and are re-exported.
- **Done when:** the file-length audit is clean and the 9 helper tests pass.

### 1.2 Claude registry, record parser, history

Read-only.

- **Files:**
  - `core/agent_sessions/claude_registry.rs`: `LiveClaudeSession`,
    `read_registry`, `find_live(session_id)`. Checks the pid is alive and skips
    the relay's own pid and working directory.
  - `core/agent_sessions/claude_records.rs`: `ClaudeRecord` (user prompt, queued
    command, queue operation, assistant block, tool result, other) and `Origin`
    (human, peer with `msg_id` and body, task notification).
  - `core/agent_sessions/claude_history.rs`: `recent_messages(session_id, limit)`
    reading the last 512 KB.
  - `core/agent_sessions/message_types.rs`: `AgentMessage {id, role, text, at_ms}`.
  - Fixtures under `core/agent_sessions/fixtures/`.
  - Tauri command `agent_session_messages`.
- **Tests:**
  - Peer queued command parses with body and `msg_id`.
  - Thinking blocks and tool results are hidden.
  - Human and peer prompts are distinguished.
  - A cut-off first line is ignored.
  - A stale pid is not live.
- **Done when:** the last 40 messages of a 5 MB transcript come back in under
  100 ms.

### 1.3 Tail watcher

- **Files:**
  - `core/agent_sessions/tail_watch.rs`: `notify` on the parent folder,
    starting at a byte offset; a pure `LineAssembler` for partial lines; handles
    the file shrinking.
  - `core/agent_sessions/tail_hub.rs`: one shared watcher per session,
    reference-counted.
- **Tests:** split lines, CRLF, three lines written in two chunks.
- **Done when:** records from a live session arrive within 500 ms of being
  written.

### 1.4 Turn tracker

Pure logic.

- **File:** `core/agent_sessions/turn_tracker.rs`.
- **States:** anchored on our `msg_id`: awaiting anchor → working → settling →
  done.
- **Completion:** a terminal stop reason, on a message with text, with no queued
  follow-up, and the settle window passes quietly. A 20 s fallback covers a
  final message with only thinking.
- **Events:** delivered (new turn or mid-turn), progress, assistant text,
  completed (with API error flag).
- **Tests:** fixtures for idle delivery, mid-turn delivery, queued follow-up,
  thinking-only then text, a 401 error, and records before the anchor being
  ignored.

### 1.5 Relay protocol and CLI lookup

Pure logic.

- **Files:**
  - `core/agent_bridge/mod.rs`
  - `core/agent_bridge/relay_protocol.rs`: encode the request, parse relay
    output, relay system prompt.
  - `core/agent_bridge/claude_cli.rs`: binary lookup (finding 6), relay
    arguments, relay folder `~/.observer_data/agent-relay/source-voice`.
- **Tests:** the M0 fixtures, version sorting, quotes and newlines survive
  encoding.

### 1.6 Relay process

- **File:** `core/agent_bridge/claude_relay.rs`: `ClaudeRelay::send(to, message)`
  returning a receipt, or a `RelayError` (auth expired, no such peer,
  paraphrased, timeout, spawn failed).
- **Behaviour:**
  - Lazy start; one send at a time.
  - Restart if it exits; recycle after 30 sends or 30 min idle.
  - Log to `~/.observer_data/helpers/agent-relay.log`.
  - Match results by `msg_id`, not by order.
- **Done when:** an ignored live test gets a `msg_id`, and the relay never
  appears as a row in the Agents list.

### 1.7 Delivery and brief reply

- **Files:**
  - `core/agent_bridge/delivery.rs`: `AgentBridge::send_prompt(session_id, text)`.
    Steps:
    1. Find the live session.
    2. Note the transcript offset and subscribe to the tail.
    3. Send through the relay.
    4. Wait up to 10 s for the anchor; otherwise report "unconfirmed".
    5. Check the body is verbatim.
    6. Run the turn tracker.
    7. Broadcast turn events; send "still working" once after 90 s of quiet.
  - `core/agent_bridge/brief.rs`: strip markdown and code, keep 2 sentences, at
    most 280 characters. No model.
  - `core/agent_bridge/types.rs`
  - A `RelaySender` trait so tests can use a fake relay.
- **Tests:** a completed turn with its brief; not live; unconfirmed; markdown
  stripping.

### 1.8 Mac session detail with a prompt box

- **Files:**
  - `app/commands/agent_bridge.rs`: `agent_send_prompt`, `agent_watch_session`,
    `agent_unwatch_session`; events `agent-turn-event`, `agent-session-appended`.
  - `app/setup.rs` manages `Arc<AgentBridge>`.
  - `src/components/agents/AgentSessionDetail.tsx`, `AgentPromptBox.tsx`,
    `useAgentSessionDetail.ts`.
  - Rows become clickable.
  - A watcher-driven `agent-sessions-changed` event replaces the 10 s interval in
    `useAgentSessions.ts`, plus a refresh when the window regains focus.
- **Done when:**
  - In `tauri dev` with a live desktop session, typing "reply with the word PONG"
    shows Sent in under 5 s while the relay is warm.
  - The message appears in the Claude window.
  - SOURCE shows the reply and its brief within 2 s of the turn ending.
  - A session that isn't live shows why it can't receive.
  - `npm run build` and the file-length audit pass; no `setInterval` remains in
    `src/components/agents/`.

---

## M2: Phone ↔ Mac agent channel

### 2.1 Frame schema

- **Files:**
  - Mac: `core/mobile/agent_frames.rs`.
  - Phone: `Net/AgentFrames.swift`, a Codable mirror.
- **Client → Mac frames:** `hello`, `open_session`, `close_session`,
  `send_text`, `pong`. M3 adds push-to-talk start, end and cancel, plus confirm
  and cancel send.
- **Mac → client frames:** `snapshot`, `sessions`, `messages_appended`, `turn`,
  `transcript`, `error`, `ping`. Every frame carries a `seq`, and snapshots carry
  an `epoch`.
- **Tests:** a golden JSON file both sides agree on; a Rust test checks every
  variant against it.

### 2.2 Two-way socket

- **Files:**
  - `core/mobile/agent_socket.rs`:
    - route `/v1/agent`;
    - header auth plus the Mac toggle;
    - split socket with an outbound queue (close when full, so the phone
      resyncs);
    - 25 s ping; close after 60 s without a pong.
  - `core/mobile/agent_feed.rs`: builds the snapshot on every connect (the
    transcripts are the source of truth, so there's no replay buffer), then
    streams changes.
  - A `MobileServices` struct passed to `serve_mobile` and `spawn_mobile_server`,
    so their signatures stop growing.
- **Done when:** tests pass and `server.rs` stays at or under 350 lines.

### 2.3 Event-driven session list

- **File:** `core/agent_sessions/registry_watch.rs`: `notify` on
  `~/.claude/sessions`, debounced 300 ms.
- **Done when:** starting or closing a Claude session updates the Mac and phone
  lists within 1 s.

### 2.4 Phone client

- **Files:**
  - `Net/AgentClient.swift`:
    - one pinned `URLSession` and a receive loop;
    - reconnect with backoff (1 → 30 s), triggered by errors, the Mac becoming
      reachable, or the app becoming active.
  - `Agent/AgentStore.swift`: applies frames, de-duplicates by epoch and seq.
  - `Agent/AgentModel.swift`.
  - About 8 lines in `AppModel.swift`.
- **Done when:**
  - `xcodegen generate && xcodebuild -scheme SourceMobile -destination 'generic/platform=iOS Simulator' build`
    succeeds.
  - On a device, turning the Mac's Wi-Fi off and on reconnects and resyncs.

### 2.5 Phone UI

- **Files:**
  - `UI/Agents/AgentSessionsView.swift`: live sessions first, app badge, title,
    project, preview.
  - `UI/Agents/AgentSessionView.swift`: conversation, status line, typed send.
  - `UI/Agents/AgentMessageRow.swift`: brief first, full text expandable,
    **Copy** button.
  - An entry icon on `RecordView` opens a full-screen `NavigationStack`.
    `RecordingQuality` is untouched.
- **Done when:** a prompt typed on the phone lands in the Claude window, and the
  reply appears on the phone within about 1 s of Claude finishing.

---

## M3: Push-to-talk (first usable slice)

### 3.1 Transcript waiters

- **File:** `core/multimodal/transcript_waiters.rs`: `register(id)`,
  `complete(id, text)`, `fail_all(reason)`.
- **Hook:** completed in the action loop in `app/dictation_supervision.rs` on
  `PersistForeground` and `DuplicateIgnored`; `fail_all` when the helper exits.
- **Tests:** complete before or after awaiting, unknown id, `fail_all`, dropped
  receiver.

### 3.2 Mac utterance handling

- **Files:**
  - `core/mobile/wav.rs`: header code moved out of `ingest.rs`.
  - `core/mobile/agent_voice.rs`:
    - write PCM to `recordings/mobile/agent-<uuid>.wav`; reject clips under
      0.3 s;
    - register the waiter **before** `TranscribeFile`, with the mobile source so
      it lands in the timeline;
    - tell the phone if Right Option dictation is holding the speech engine;
    - timeout = 30 s + 2 × audio length (120 s when delayed);
    - empty text returns "no speech".
  - `core/mobile/agent_send_window.rs`: the Mac owns a 2 s confirm/cancel
    window, so a dropped phone can't strand a prompt.
- **Tests:** WAV header sizes; the window state machine with paused tokio time.

### 3.3 Phone capture

- **Files:**
  - `Audio/AudioSink.swift`: the file becomes optional.
  - `Audio/VoiceSession.swift`: owns the audio session.
    - Capture: `.playAndRecord` with `.defaultToSpeaker` and Bluetooth.
    - Speech: `.playback` / `.spokenAudio`.
    - Idle.
    - Interruptions cancel.
  - `Audio/PushToTalk.swift`: refuses during a Dictation or Podcast recording;
    stops speech; haptic once audio is live.
  - `UI/Agents/HoldToTalkButton.swift`: hold to talk, slide up to cancel.
    States: listening, transcribing, confirm (countdown, Cancel, Send now), sent,
    working, done.
- **Done when:** a 3 s hold sends about 96 KB, the Mac writes a WAV of matching
  length, and the transcript appears on the phone.

### 3.4 Spoken reply

- **Files:**
  - `Audio/SpeechPlayer.swift`: `AVSpeechSynthesizer`, enhanced voice when
    installed.
  - `AgentStore` speaks the brief when a turn completes, plus short lines for
    "not live", "sign in to Claude on the Mac", and "didn't catch that".
  - Pressing talk stops speech instantly.
- **Slice acceptance:** on home Wi-Fi with a live Claude session:
  - Hold, say "reply with the word pong", release.
  - The transcript shows in under 3 s, the confirm window runs, and the message
    lands in Claude.
  - The phone says "Pong" within 3 s of the turn ending.
  - 10 runs in a row with no lost or duplicated prompts.
  - Using Right Option on the Mac mid-flow delays the prompt but never loses it
    (test with the bundled app).

### M3b: Kokoro voice

1. **Spike:** FluidAudio Kokoro on your iPhone. Measure warm per-sentence
   latency and memory.
2. **If it's good:** `Audio/KokoroSpeaker.swift` via Swift Package Manager.
   Synthesize sentence by sentence; download models on Wi-Fi; keep
   `AVSpeechSynthesizer` as the fallback.
3. **Otherwise:** a separate Mac `speak-serve` process, never inside the
   dictation helper, sending WAV frames over `/v1/agent`.

---

## Later milestones (outline)

### M4 Away from home

- Tailscale on the Mac and phone.
- Save a second host in the pairing (the certificate pin ignores hostnames).
- Allow cellular for the agent socket, with an ATS exception.
- Live Activity: "Talking to <session>".

### M5 Steer, queue, and the other three apps

An `AgentAdapter` trait: `list`, `history`, `send(queue | steer)`, `interrupt`,
`subscribe`.

- **Codex:** a long-lived `codex app-server`. Use `turn/start` and `turn/steer`;
  use `thread/queue/add` when the thread is open in ChatGPT.
- **OpenCode:** `opencode serve` on localhost with a password.
  `/api/session/:id/prompt` with `delivery`, or `prompt_async` when it must show
  in the desktop app. Events over SSE.
- **Factory:** `droid exec` JSON-RPC (`load_session`, `add_user_message`).
  Queue only; watch credit use.

Voice correction phrases choose steer.

### M6 Hands-free streaming and the router

- Endpointing with the Silero VAD already on the Mac, plus streaming partials.
- Echo cancellation on the phone.
- Keyword rules plus local Qwen3-4B (already downloaded) in a warm `mlx_lm`
  process in the helper venv.
- Route decisions with confidence-gated confirm and undo.

### M7 Catch me up, global chat, approvals, agent speaks first

- Per-turn summaries from the local model; a global chat above all sessions.
- Proactive speech when a session finishes or needs you, including "what do you
  want to work on?".
- Approvals only through each app's supported mechanism:
  - Codex: `requestApproval`.
  - OpenCode: permission events.
  - Factory: `request_permission`.
  - Claude: research a supported, opt-in route. It likely edits
    `~/.claude/settings.json`, so ask first.

### M8 Live view

- Per-project run profiles: command, port, working directory.
- Auto-start on opening a session, with consent on first run.
- WKWebView through SOURCE's TLS server.
- Landscape for that screen.

### M9 Mac screen streaming

For native apps: ScreenCaptureKit → H.264, view-only, pinch zoom.

### M10 ChatGPT web

Browser automation. Deferred.

---

## Risks

| Risk | Mitigation |
|---|---|
| Claude doesn't act on peer-delivered prompts, or asks for approval | M0 P-auth gate. Fallbacks: desktop-hosted relay, or typed insertion. While waiting on a tool for over 60 s, say "Claude may be waiting for approval on the Mac". |
| The relay paraphrases the prompt, or is steered by what was said | Pass the text as data; verify tool input, `msg_id` and `origin.body`; report "paraphrased". An optional pre-tool hook can enforce verbatim sends (ask first; edits Claude settings). |
| Relay cost and latency | One warm Haiku process with trimmed flags; lazy start; recycle; log cost and latency per send. |
| Relay sign-in expires | Detect 401; tell the phone and Mac to run `claude login`; no retry loop. |
| Turn end detected wrong | Turn tracker, fixtures from real transcripts, measured settle window, 90 s "still working". |
| Target session not live, or renamed | Resolve by `sessionId` before each send; only live sessions are sendable; "open it in Claude on the Mac". |
| Right Option dictation delays transcription | Tell the phone; longer timeout; `fail_all` if the helper exits. |
| iOS audio session switching | `VoiceSession` owns it; stop speech on press; `.defaultToSpeaker`; no push-to-talk during recordings. |
| Echo | Push-to-talk is half-duplex; hands-free (M6) adds echo cancellation. Our own relayed messages show as "You (voice)". |
| Relay sessions clutter the list | `--no-session-persistence`, a dedicated folder, filter by pid and folder. |
| A paired phone can drive agents | Mac toggle off by default; header-only auth; prompts visible in the timeline. |
| Existing bug: the dictation supervisor loop exits on a broadcast `Lagged` error, silently stopping transcripts | Small separate fix before M3. |

## Testing conventions

- **Mac:**
  - `cargo test --manifest-path src-tauri/Cargo.toml --lib <module>`
  - `npm run build`
  - `npm run check:file-lengths`
  - Live tests against real agent apps are `#[ignore]` and run by hand.
- **Phone:** `xcodegen generate`, then `xcodebuild` with the `SourceMobile`
  scheme. There's no test target yet; add `SourceMobileTests` when the frame
  schema lands (2.1).
- **Dictation:** anything involving Right Option must be tested with the bundled
  app, not `tauri dev` (Accessibility is granted per binary location).
  Transcribing files works in `tauri dev`.
