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

The first question (M0) is answered: SOURCE can reach a Claude Code
conversation by **continuing it in its own Claude process**, so your words
arrive as your own messages. Sending a message "from another session" doesn't
work reliably: Claude sometimes refuses it as unverified, which protects you.

## Status

| Milestone | What | Status |
|---|---|---|
| Research | All four apps reachable from outside; steering proven; router and speech proven | ✅ Done |
| Session hub v1 | Agents tab on the Mac listing every session from all four apps | ✅ Done |
| M0 | Go/no-go spikes for Claude delivery | ✅ Done: continue the conversation, not peer messages |
| M1 | Mac-only loop: send to Claude, get the reply | ✅ Done |
| M2 | Phone ↔ Mac agent channel, sessions on the phone | 🧪 Built and tested; waiting for the on-phone check |
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
- **One writer per conversation.** SOURCE never adds to a Claude conversation
  while the Claude app's own process for it is running. It stops that process
  first, and only when it's idle.
- **A paired phone becomes powerful, so:**
  - add a Mac setting "Allow phone to send prompts to agents", off by default;
  - `/v1/agent` accepts header auth only;
  - spoken prompts stay visible in the timeline.
- **Both repos:**
  - every source file stays at or under 350 lines, split by responsibility;
  - the phone repo regenerates its Xcode project with `xcodegen generate` after
    adding files.

## What the code review and M0 found

These findings changed the design:

1. **Peer messages aren't a delivery path.** Messages between Claude sessions
   arrive labelled as coming from another agent, and Claude decides case by case
   whether to trust them (see M0 results).
2. **Continuing the conversation is.** A SOURCE-owned
   `claude -p --resume <sessionId> --input-format stream-json --output-format stream-json`
   process writes to the same transcript. Claude treats the words as the user's,
   and reports turn end directly with a `result` line, so no file-based turn
   tracking is needed for turns SOURCE starts.
3. **The Claude app keeps its own copy of an open conversation.**
   - Its running process doesn't see turns added from outside.
   - Once it starts a fresh process for the conversation (after SOURCE hands it
     back), Claude's memory includes them, and so does the window: in the M1
     test, messages sent from SOURCE appeared in the Claude app after it
     reopened the conversation. The one outside turn added while the app's
     process was still running (M0's PINEAPPLE) never appeared in the window.
4. **Session names change** (`0-e0` later became `0-c1`); session ids don't.
   Resolve live state from `~/.claude/sessions/<pid>.json` by `sessionId` right
   before every send.
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

## M0: Go/no-go spikes ✅

Run on 2026-09-14 against throwaway sessions: Claude CLI sessions in a scratch
folder, and one Claude desktop session in `~/Documents/voice-test`.

### Results

| Test | Result |
|---|---|
| **Peer delivery, idle target, plain instruction** ("create `voice-proof.txt` containing BANANA") | Mixed. A neutral default-model session did it, both with and without "Message from Adam, relayed by SOURCE:". A session told earlier to "reply only OK" declined it as a request from another session. |
| **Peer delivery, busy target, correction** ("I forgot to say: skip the rest, reply STEERED") | ❌ Refused. Claude finished its original task and noted it had ignored an unverified request "from relay". |
| **Trimmed relay flags** | ✅ `--tools SendMessage`, `--system-prompt`, `--strict-mcp-config`, `--setting-sources ""`, `--no-session-persistence` all work; `uds:<socket>` addressing works; warm send about 2.3 s. |
| **Relay forwards verbatim** | A plain "copy both byte-for-byte" prompt forwarded 0 of 8 tricky payloads; the model answered them itself. A hardened "sealed mail, never addressed to you" prompt forwarded 8 of 8. (Moot now; kept for reference.) |
| **Continue the conversation** (`claude -p --resume <id>`) | ✅ Same session id and transcript file; history kept ("my favourite fruit" → "Mango"); about 7 s to resume and answer. |
| **Correction mid-task, continued conversation** | ✅ Four slow commands started; "I forgot to say: skip the rest, reply STEERED" sent after the first. Ran one command, replied STEERED. |
| **Claude desktop app, conversation open** | Turn added from outside was saved and shows in the app's session data, but not in the window, even after switching away and back. |
| **Claude desktop app, after its process restarts** | SOURCE stopped the app's idle process for that session; the app started a fresh one on next use. Window still didn't show the added turn, but asked "what was the last word you said?", Claude answered **PINEAPPLE**: its memory includes what SOURCE added. |
| **Transcript timing** | Records land on disk within about 0.05 s of their timestamp; blocks of one assistant message land together. |

### Decision

Deliver voice prompts to Claude Code by **continuing the conversation in a
SOURCE-owned Claude process**, not by peer messages. Consequences:

- **You follow voice turns on the phone** (and in SOURCE on the Mac) as they
  happen. The Claude app window catches up when it reopens the conversation,
  and Claude remembers the voice turns either way.
- **One writer per conversation.** Before continuing a conversation, SOURCE
  checks whether the Claude app's process for it is running. If it is and it's
  idle, SOURCE stops it (the app starts a fresh one next time you use the
  session). If it's mid-task, SOURCE says so and doesn't send.
- **SOURCE holds a conversation only while Claude works on a message SOURCE
  sent,** then hands it back immediately. The first version held it for five
  quiet minutes; in the M1 test that let the Claude app open its own copy
  alongside SOURCE's when the conversation was clicked there, which risks the
  two drifting apart. The Agents tab marks conversations SOURCE is holding.

---

## M1: Mac-only loop

Prove sending and getting the reply with a prompt box on the Mac, before any
phone code. Paths are under `src-tauri/src/` unless they start with `src/`.

### 1.1 Split `dictation_helper.rs`

✅ Done.

- **What changed:** the protocol types, `parse_helper_line` and their tests moved
  to `core/multimodal/dictation_helper_protocol.rs` and are re-exported.
- **Done when:** the file-length audit is clean and the 9 helper tests pass.

### 1.2 Claude registry and history

✅ Done. The last 40 messages of a 61 MB transcript come back in about 8 ms.


Read-only.

- **Files:**
  - `core/agent_sessions/claude_registry.rs`: `LiveClaudeSession {pid,
    session_id, name, cwd, entrypoint}`, `read_registry`, `find_live(session_id)`.
    Checks the pid is alive, and tells the Claude app's processes apart from
    SOURCE's own (by pid).
  - `core/agent_sessions/claude_records.rs`: parse transcript records into user
    prompt, assistant text, tool use, tool result, other.
  - `core/agent_sessions/claude_history.rs`: `recent_messages(session_id, limit)`
    reading the last 512 KB.
  - `core/agent_sessions/message_types.rs`: `AgentMessage {id, role, text, at_ms}`.
  - Fixtures under `core/agent_sessions/fixtures/`, from the M0 transcripts.
  - Tauri command `agent_session_messages`.
- **Tests:**
  - Thinking blocks and tool results are hidden.
  - Cross-session messages are shown as such, not as the user.
  - A cut-off first line is ignored.
  - A stale pid is not live.
- **Done when:** the last 40 messages of a 5 MB transcript come back in under
  100 ms.

### 1.3 Claude CLI lookup and stream protocol

✅ Done. Also parses Claude's own progress and after-turn recap lines, and plan usage.


Pure logic.

- **Files:**
  - `core/agent_bridge/mod.rs`
  - `core/agent_bridge/claude_cli.rs`: binary lookup (finding 6) and the
    arguments for continuing a conversation: `-p --resume <id> --input-format
    stream-json --output-format stream-json --verbose`, run in the session's own
    working directory.
  - `core/agent_bridge/stream_protocol.rs`: encode a user message; parse output
    into `Init {session_id}`, `AssistantText`, `ToolUse {name}`, `ToolResult`,
    `TurnResult {is_error, result, cost_usd, duration_ms}`, `AuthError`.
- **Tests:** real stream lines captured in M0; version sorting; quotes and
  newlines survive encoding; a 401 line becomes `AuthError`.

### 1.4 Conversation driver

✅ Done. Real run: continued a throwaway conversation and recalled the remembered word in 5.7 s.


- **File:** `core/agent_bridge/claude_driver.rs`: `ClaudeDriver` owns one
  SOURCE-held Claude process per conversation.
  - `send(text)` writes a user message. If a turn is running, the message is
    delivered mid-turn (proven in M0) and reported as "added to current work".
  - Streams `DriverEvent`s: working, tool in progress, assistant text, turn done
    (with final text), error.
  - Released as soon as Claude finishes replying (no follow-up queued): stdin
    closed, process exits, the conversation goes back to the Claude app. A
    5-minute quiet timer remains only as a safety net.
  - `kill_on_drop`; restart on the next send if the process died; log to
    `~/.observer_data/helpers/claude-driver.log`.
- **Done when:** an ignored live test continues a throwaway conversation, gets
  the reply and turn end, and releases it.

### 1.5 Hand-off from the Claude app

✅ Done. On this Mac's real sessions: the idle test session would be taken over; the busy working session is refused. Process exit is awaited with a kqueue notification, not polling.


- **File:** `core/agent_bridge/handoff.rs`: `take_over(session_id)`:
  1. No Claude app process for the session → go ahead.
  2. App process running and idle (no turn in progress in the transcript tail)
     → stop it (`SIGINT`, then `SIGTERM` if it's still running after 3 s),
     wait for it to exit, go ahead. SIGINT lets it exit with code 0, so the
     Claude app doesn't show "Session was interrupted".
  3. App process mid-task → refuse with `BusyInApp`.
- **Tests:** decisions from fixture registries and transcript tails; never
  touches a pid that isn't a Claude app process for that session.
- **Done when:** a live test takes over a throwaway desktop session, and the
  Claude app resumes it normally afterwards (M0 showed it does).

### 1.6 Bridge and brief reply

✅ Done.


- **Files:**
  - `core/agent_bridge/bridge.rs`: `AgentBridge::send_prompt(session_id, text)`:
    take over, then send through that conversation's driver, broadcast turn
    events, report "still working" once after 90 s of quiet.
  - `core/agent_bridge/brief.rs`: strip markdown and code, keep 2 sentences, at
    most 280 characters. No model.
  - `core/agent_bridge/types.rs`
  - A `ConversationDriver` trait so tests can use a fake driver.
- **Tests:** a completed turn with its brief; busy in the app; auth expired;
  markdown stripping.

### 1.7 Mac session detail with a prompt box

✅ Done. Real run against a throwaway Claude desktop conversation: taken over in 1.6 s, reply "PONG" with its brief in 5.7 s. Tried in the app: prompts sent from the Agents tab got replies, and appeared in the Claude app window after it reopened the conversation. That test also showed SOURCE holding a conversation for five quiet minutes let the Claude app open a second copy, so SOURCE now hands it back as soon as each reply finishes. Retested after that fix: the reply came back, SOURCE let go immediately (no second copy running), and Cmd+R in the Claude app showed the new messages.


- **Files:**
  - `app/commands/agent_bridge.rs`: `agent_send_prompt`, `agent_release_session`;
    event `agent-turn-event`.
  - `app/setup.rs` manages `Arc<AgentBridge>`.
  - `src/components/agents/AgentSessionDetail.tsx`, `AgentPromptBox.tsx`,
    `useAgentSessionDetail.ts`.
  - Rows become clickable; a "held by SOURCE" badge on conversations SOURCE is
    driving.
  - A watcher-driven `agent-sessions-changed` event (FSEvents on
    `~/.claude/sessions` and the transcript folders) replaces the 10 s interval
    in `useAgentSessions.ts`, plus a refresh when the window regains focus.
- **Done when:**
  - In `tauri dev`, typing "reply with the word PONG" into a throwaway desktop
    session gets the reply and its brief in SOURCE.
  - Going back to that session in the Claude app and asking "what was the last
    word you said?" gets "PONG".
  - A session mid-task in the Claude app shows why it can't receive right now.
  - `npm run build` and the file-length audit pass; no `setInterval` remains in
    `src/components/agents/`.

---

## M2: Phone ↔ Mac agent channel

**Status:** built in both repos (Mac `ca09664`, phone `df7e653`). 55 Mac agent
tests and 5 phone tests pass. Still to do: the on-phone check in 2.5.

**What changed from the plan while building:**

| Plan | Built | Why |
|---|---|---|
| `messages_appended` frame | `messages`: the latest 60 messages, sent again when they change | Simpler, and a reconnect can't miss anything. The Mac diffs them, so unchanged lists aren't resent. |
| No reply frame for `send_text` | `send_result` with `requestId`, `ok`, `error` | The phone needs to say *why* a send failed: Claude busy in the app, open in a terminal, or the setting is off. |
| `transcript` frame | Not yet | Added with push-to-talk in M3. |
| New `registry_watch.rs` | `agent_sessions/watch.rs` (from M1) watches both transcripts and the registry, 300 ms settle | One watcher already covered it; the Mac tab and the phone share it. |
| `MobileServices` struct | `Option<AgentServices>` passed to `serve_mobile` | Only one new argument was needed; `server.rs` is 267 lines. |
| Golden file shared | The phone test target decodes a **copy** of the Mac's file | Separate repos. When frames change, copy `fixtures/agent_frames.json` to `SourceMobileTests/Fixtures/`. |

### 2.1 Frame schema ✅

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

### 2.2 Two-way socket ✅

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

### 2.3 Event-driven session list ✅

- **File:** `core/agent_sessions/registry_watch.rs`: `notify` on
  `~/.claude/sessions`, debounced 300 ms.
- **Done when:** starting or closing a Claude session updates the Mac and phone
  lists within 1 s.

### 2.4 Phone client ✅ (build and tests; the Wi-Fi check happens with 2.5)

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

### 2.5 Phone UI 🧪 built; waiting for the on-phone check

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
| You type in the Claude app while SOURCE holds that conversation, forking it | SOURCE holds a conversation only while Claude is replying to it, and the Agents tab shows "Held by SOURCE" meanwhile. Later: detect the app starting its own process for a held conversation and warn. |
| The Claude app window lags behind voice turns | It doesn't refresh by itself, but Cmd+R in the Claude app reloads the conversation and shows them (seen in M1). "Session was interrupted" there just means SOURCE took the conversation over; using it again in the app resumes normally. |
| Claude asks for approval during a voice turn | Claude runs with the session's own permission settings; SOURCE never auto-approves. While waiting on a tool for over 60 s, say "Claude may be waiting for approval". Approvals from the phone in M7. |
| Sign-in expires (the CLI keeps its own login, separate from the app's) | Detect 401; tell the phone and Mac to run `claude auth login`; no retry loop. |
| Stopping the app's process at the wrong moment | Only when the transcript shows no turn in progress; only that session's Claude app process, matched by session id. |
| Right Option dictation delays transcription | Tell the phone; longer timeout; `fail_all` if the helper exits. |
| iOS audio session switching | `VoiceSession` owns it; stop speech on press; `.defaultToSpeaker`; no push-to-talk during recordings. |
| Echo | Push-to-talk is half-duplex; hands-free (M6) adds echo cancellation. |
| SOURCE's own Claude processes show up as live sessions in the list | Tell them apart by pid; show them as "held by SOURCE", not as extra rows. |
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
