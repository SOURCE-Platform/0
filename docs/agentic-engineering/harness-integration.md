# Harness integration: how SOURCE talks to each agent app

Every adapter needs the same five abilities:

- `listSessions`
- `readHistory`
- `send(text, queue | steer)`
- `interrupt`
- `subscribe(events)`

This page records what each app offers, what was proven on this Mac on
2026-09-13, and the recommended adapter.

Proof scripts lived in the session scratchpad (`agent-proof/`). They used
throwaway sessions in a scratch folder and harmless prompts ("reply PONG",
"count to sixty"), and never wrote to real conversations.

## Summary

| | Codex | Claude Code | Factory | OpenCode |
|---|---|---|---|---|
| Installed | ChatGPT.app bundles `codex-cli 0.154.0-alpha` (not on PATH) | Desktop app runs bundled CLI 2.1.266; `~/.local/bin/claude` 2.1.252 | Factory.app 0.170, `droid` 0.213 | Desktop 1.18.27, Homebrew CLI 1.18.20 |
| Protocol | `codex app-server` JSON-RPC (stdio / unix / ws) | stream-json (Agent SDK), channels (MCP), session files | `droid exec` stream-jsonrpc, `droid daemon` (ws/ipc) | HTTP + SSE server (`opencode serve`) |
| List sessions | `thread/list` ✅ (shows desktop threads) | `claude agents --json` ✅ (shows desktop sessions) + JSONL files | `droid search --json`, SDK `listSessions()`, JSONL files | `GET /session` ✅ (shows desktop sessions) |
| Send from outside | ✅ `turn/start` | ✅ continue the conversation (`--resume`, stream-json); peer SendMessage not reliable | ✅ `droid exec -s <id>`, `droid.add_user_message` | ✅ `POST /api/session/:id/prompt` |
| Mid-turn message | ✅ `turn/steer` | ✅ picked up between tool calls, changes course | Queued as the next turn ✅ | `delivery: "steer" \| "queue"` ✅ |
| Interrupt | `turn/interrupt` | SDK `interrupt()` | `droid.interrupt_session` | `POST /api/session/:id/interrupt`, `/session/:id/abort` |
| Live events | `item/agentMessage/delta`, `turn/completed` ✅ | stream-json, hooks, JSONL tail | `create_message`, `agent_turn_completed` ✅ | `/event`, `/api/event` SSE ✅ |
| Shows in the app's own window | Not verified | ❌ not displayed; Claude's memory includes it once the app's process restarts | Not tested | Only after "Reload Webview"; v2-API messages don't render |
| Auth used | Existing ChatGPT login ✅ | claude.ai login ✅ (the CLI keeps its own, separate from the desktop app's) | Existing Factory login ✅ | Existing OpenCode providers ✅ |

✅ = proven on this Mac. ⏳ = blocked.

## Codex

**How the app is wired.** ChatGPT.app starts its own private
`codex app-server` over stdio. The shared daemon socket
(`~/.codex/app-server-control/app-server-control.sock`) does not exist until
`codex app-server daemon start` (or `codex remote-control start`) runs. Threads,
queues and history live in `~/.codex` (sqlite + `sessions/` JSONL), which the
app and any other `codex` process share.

**P1 result (pass).** A separate `codex app-server` process:

- `initialize` took 0.15 s.
- `thread/list` returned the desktop app's threads.
- `thread/start` + `turn/start` streamed the first words after 3.5 s.
- `turn/steer` (with `expectedTurnId`) was accepted mid-answer.
- The steer text was applied inside the same turn once the message being
  streamed finished (the reply ended "…sixty" + "STEERED").
- `turn/completed` arrived at 9.3 s.

**`codex queue --thread <id> --message …` (pass, partial).** The message is
stored in the thread's persistent queue (`thread/queue/list` shows it). It
doesn't run until some app-server loads that thread. Not verified: whether an
open ChatGPT.app window drains it.

**Blocked.** A steer during a multi-step command loop (to see steer landing
between tool calls). The account hit its Codex usage limit, which resets
2026-09-14 01:01.

**Useful methods** (from `codex app-server generate-json-schema`):

- Threads: `thread/list`, `thread/read`, `thread/turns/list`, `thread/search`,
  `thread/resume`, `thread/start`.
- Turns: `turn/start`, `turn/steer`, `turn/interrupt`.
- Queue: `thread/queue/add|list|update|delete|reorder|start`.
- Approvals: server requests `item/commandExecution/requestApproval`.
- Built-in voice: `thread/realtime/start|appendAudio|appendText`,
  `thread/realtime/outputAudio/delta`.

**Recommended adapter.** SOURCE keeps one long-lived `codex app-server`
child over stdio.

- Reads: `thread/list` + `thread/turns/list`.
- Sends: `thread/resume` then `turn/start`; `turn/steer` for corrections.
- Safety rule until the open question is answered: if a thread is running
  inside ChatGPT.app, add to its queue (`thread/queue/add`) instead of
  starting a turn from a second process.

## Claude Code

**How the desktop app is wired.**

- Each desktop session is the bundled CLI running as
  `claude --input-format stream-json --output-format stream-json`, the same
  protocol as the Agent SDK.
- Transcripts: `~/.claude/projects/<folder>/<sessionId>.jsonl`. Record types
  seen: `user`, `assistant`, `attachment`, `queue-operation`, `custom-title`,
  `last-prompt`, `mode`, `system`.
- Live registry: `~/.claude/sessions/<pid>.json` holds `sessionId`, `cwd`,
  `kind: interactive`, `entrypoint: claude-desktop`, `name` and
  `messagingSocketPath: /tmp/cc-socks/<pid>.sock`.
- `claude agents --json` lists these desktop sessions.

**Ways in, best first** (after the M0 tests, see
[implementation-plan.md](implementation-plan.md), section M0).

1. **Continue the conversation in a SOURCE-owned process (chosen).**
   `claude -p --resume <sessionId> --input-format stream-json --output-format
   stream-json`, run in the session's working directory.
   - Same session id and transcript file; history kept.
   - Words arrive as the user's own; corrections land between tool calls.
   - Turn end arrives as a `result` line, so no file-based turn tracking.
   - About 7 s to resume and answer a short prompt.
   - **One writer rule:** the Claude app's own process for that session doesn't
     see outside turns, so SOURCE stops it first (only when idle). The app
     starts a fresh process on next use, and that one's memory includes the
     voice turns. The app window doesn't display them.
2. **Relay through SendMessage (not reliable).** Delivery works, including into
   a busy desktop session. But messages arrive labelled as coming from another
   agent, and Claude decides whether to trust them: in M0 it refused a
   mid-task correction and a simple "reply WARM" as unverified requests.
3. **Channels** (documented, research preview). An MCP server declares
   `experimental['claude/channel']` and pushes `notifications/claude/channel`
   events into a running CLI session. Custom channels need
   `claude --dangerously-load-development-channels server:<name>`. Only works
   for sessions started with that flag, not desktop sessions.

**P2a: delivery into a live desktop session (pass).** A headless
`claude -p --model haiku --allowedTools "ListAgents SendMessage ToolSearch"`
process called ListAgents, found the desktop session by name, and delivered a
message with SendMessage. About 10 s end to end, most of it relay startup.

- The message appeared in the target desktop conversation, **while that session
  was mid-turn**, surfaced between its tool calls. So an outside program can
  reach a busy desktop session, not just an idle one.

**P2c: a second message mid-turn (pass).** Driving
`claude -p --input-format stream-json --output-format stream-json` directly:

- Message 1 asked for five `sleep 4; echo stepN` commands, one tool call each.
- Message 2 arrived 7 s later: "skip any remaining commands, reply STEERED".
- Claude finished step 1, dropped steps 2–5 and replied STEERED, all inside the
  same turn.
- This is the most responsive steering of the four apps: Codex applies a steer
  after the message it is streaming, Factory and OpenCode run it as the next
  turn.

**Not tested.** A custom dev channel. Also worth knowing: `claude --bg`
background sessions started in this environment sat in `blocked` / idle with
their prompt unsent, so background sessions need their own look before SOURCE
relies on them.

**Privacy note.** A `--bg` session's banner showed Remote Control connecting
(`/rc active`), which links the session to claude.ai. `/remote-control` turns
it off per session. SOURCE should start any session it owns with remote control
off, since SOURCE is the remote control.

**Recommended adapter.**

- **Reads:** the live registry (`~/.claude/sessions/*.json`) for which sessions
  have a running process, plus the JSONL transcripts for history.
- **Sends:** take the conversation over (stop the Claude app's idle process for
  it, or refuse if it's mid-task), then drive it with a SOURCE-owned stream-json
  process. Release it after a few minutes of quiet so the Claude app can use it
  again.

**Subscription use.** Personal Agent SDK and `claude -p` use on a Pro/Max
plan draws from plan limits. Offering claude.ai login inside a product for
other people is not allowed.

## Factory

**How the app is wired.** Factory.app runs a private
`droid daemon --listen ipc` and spawns
`droid exec --input-format stream-jsonrpc --output-format stream-jsonrpc` for
each session. Transcripts: `~/.factory/sessions/<folder>/<id>.jsonl`.

**P3 results (pass).**

- `droid exec "…" -o stream-json` replied PONG in 9.7 s (5.4 s turn). The
  first event carries `session_id`.
- `droid exec -s <id> "…"` continued that session with its context ("PONG2").
- JSON-RPC (the protocol Factory.app uses): `droid.load_session {sessionId}`,
  then `droid.add_user_message {text}`, streamed `create_message`,
  `droid_working_state_changed` and `agent_turn_completed`.
- A second `add_user_message` sent 5 s into a running turn was accepted and
  ran as its own next turn (queue, not steer).

**Not tested.** Whether Factory.app's window shows an outside process's
turns. The app was mid-task on a real session, so I didn't touch it.
Also not tested: `droid daemon --port` over WebSocket (the SDK example
authenticates with a Factory API key).

**Cost note.** The one-word PONG reported `factory_credits: 13565`. Don't use
Factory for routing or summaries.

**Recommended adapter.** Speak JSON-RPC to a `droid exec` stream-jsonrpc child
per active session:

- `droid.load_session` → `droid.add_user_message`
- `droid.interrupt_session` to stop
- read history from the JSONL

The SDKs (`@factory/droid-sdk`, `droid-sdk-python`) wrap exactly this.

## OpenCode

**How the app is wired.** The desktop app starts its own server on a random
localhost port with a per-launch password (`401` without it). All clients
share `~/.local/share/opencode/opencode.db`.

**P4 results (pass).** `OPENCODE_SERVER_PASSWORD=… opencode serve --port 4196`:

- `GET /session` lists sessions, including desktop ones.
- `POST /session` creates a session.
- The newer API `POST /api/session/:id/prompt` accepts
  `{"prompt":{"text":…},"delivery":"steer"|"queue"}`.
- `GET /api/session/:id/context` and `/history` return the conversation and
  its event log.
- In both modes the second message was delivered after the step in progress
  finished. With a single-step answer the two modes look the same. The
  difference only shows inside multi-step tool loops.

**Desktop window.** Sessions and messages added by the separate server
appeared only after OpenCode › Reload Webview. Messages sent through the
newer `/api/session/:id/prompt` did not render in the desktop app at all. The
older `POST /session/:id/prompt_async` did render after reload.

**Free model latency** (the model you use in OpenCode):

- `muse-spark-1.3-contributor-free`: 2–9 s typical, 45–55 s stalls.
- `nemotron-3-ultra-free`: 80 s for "hello".

**Recommended adapter.** SOURCE runs `opencode serve` on a fixed port with a
password.

- For a live shared UI, open that same server with `opencode attach <url>`
  (TUI) or `opencode web` instead of the desktop app.
- Send with the newer prompt API when steer or queue matters. Use
  `prompt_async` when the message must show in the desktop app.
- Stream with `/api/event`.

## ChatGPT web

There's no API into consumer chat threads. The only way in is browser
automation (Chrome DevTools Protocol or an extension), and it breaks whenever
the page changes. Deferred.

For the ".md specs, brief, plain-language" preferences, use ChatGPT's custom
instructions (or project instructions) instead of adding text to each prompt.

## Global preferences (brief, plain language, copy-friendly)

Put standing preferences in each app's global instruction file, not in every
routed prompt:

| App | File | Present on this Mac |
|---|---|---|
| Codex | `~/.codex/AGENTS.md` | yes |
| Claude Code | `~/.claude/CLAUDE.md` (can import a shared file with an `@AGENTS.md` line) | no |
| Factory | `~/.factory/AGENTS.md` | no |
| OpenCode | `~/.config/opencode/AGENTS.md` | no |
| ChatGPT | Settings › Personalization › Custom instructions | – |
