# Transport, reach, and live view

## What SOURCE already has

**The Mac server.** `src-tauri/src/core/mobile/`:

- An axum HTTPS + WebSocket server on port 8787 with a self-signed certificate.
- Bonjour discovery `_source-mobile._tcp`.
- QR pairing or an Allow dialog with a 4-character check code.
- Bearer tokens (`pairing.rs`).

**Routes today.**

| Route | Purpose |
|---|---|
| `GET /v1/health` | Health check |
| `POST /v1/pair/start`, `GET /v1/pair/poll` | Pairing |
| `POST /v1/clips` | Clip upload |
| `GET /v1/clips/status` | Transcription status |
| `GET /v1/stream` | WebSocket: phone → Mac only; 16 kHz Int16 PCM between `open`/`close` frames |

**The phone side.**

- `Net/StreamClient.swift` sends PCM live but never reads from the socket and
  doesn't reconnect.
- Uploads are Wi-Fi only (`allowsCellularAccess = false`) and ATS allows only
  local networking.

## Changes Stream mode needs

1. **A two-way agent socket.** Add `GET /v1/agent` (WebSocket) instead of
   overloading the clip stream.
   - Phone → Mac:
     - `audio` frames
     - `focus {session_id, view}`
     - `send_text` (typed fallback)
     - `confirm_route`, `undo`
     - `approval {id, allow}`
   - Mac → Phone:
     - `transcript_partial`, `route_decision`
     - `sessions` (list + status)
     - `message_delta`, `turn_completed`
     - `speech` (audio chunks, sentence by sentence)
     - `approval_request`
     - `live_view {url}`
   - Resume by `last_event_seq` on reconnect, so a dropped connection on a walk
     loses nothing.
2. **Live transcription while streaming**, not after close (see
   [voice-router.md](voice-router.md)).
3. **Keep ambient behaviour.** Streamed speech still lands in the timeline as
   `source-mobile` transcripts; the router just also reads it.

## Away from home

- **Tailscale** is the simplest option.
  - Install it on the Mac and phone, and add the Mac's Tailscale name/IP as a
    second host in the phone's saved pairing.
  - The phone already pins the Mac's certificate by SHA-256 fingerprint and
    doesn't check the hostname (`PinnedSessionDelegate`), so the same pairing
    works.
  - Phone changes: allow cellular for the agent socket, add a manual host
    field, and add an ATS exception for the Tailscale domain.
- **No port forwarding** and no cloud relay of your audio or code.
- **Background.** The phone already declares background `audio`, so a
  hands-free stream keeps running with the screen locked. The Live Activity
  can show "Streaming to Mac · Marketing site".

## Live view

### Web apps: load the real site on the phone

- The Mac dev server must listen on the network: Vite `--host`, Next
  `-H 0.0.0.0`.
- SOURCE hands the phone `http://<mac-lan-or-tailscale-ip>:<port>`, and the
  session's **Live** tab shows it in a `WKWebView`.
- This is the real mobile rendering, interactive, with no video. Every iPhone
  browser uses WebKit (outside the EU), so installing Chromium isn't needed or
  possible.
- For the desktop layout: rotate to landscape and request desktop mode
  (`WKWebpagePreferences.preferredContentMode = .desktop`).

### Native Mac apps and everything else: watch the Mac screen

- **SOURCE today** records stills (`CGDisplay::image`) into ffmpeg file
  segments (`core/screen_recorder`). That's not live, so a new path is needed:
  1. ScreenCaptureKit `SCStream` of one window or display.
  2. VideoToolbox H.264 (hardware).
  3. Low-latency transport to the phone: WebRTC, or fragmented MP4 over the
     agent socket.
- **View-only** (no tap → click), as requested. Pinch-zoom and a "fit app
  window" crop, since each session knows its app's window.
- Needs the existing Screen Recording permission.
- **Later phase.** Web live view covers most front-end work first.

## "Start the app"

Each project gets a small run profile that SOURCE discovers once and you can
edit.

**Where it comes from:**

- `.claude/launch.json` if present
- else `package.json` scripts (`dev`, `start`)
- else `npm run tauri dev`, `xcodebuild`, etc.

**What it holds:**

| Field | Meaning |
|---|---|
| `command` | How to start the app |
| `cwd` | Folder to run it in |
| `port` / `url` | Where it serves |
| `kind` | `web` or `native` |

**When a session opens** (if the preference is on), SOURCE:

1. Starts the command if the port isn't already in use.
2. Waits for the port.
3. Sends `live_view {url}` to the phone.

It never runs anything new without the profile existing; the first run
asks on the phone.

## Security notes

- Everything stays between your phone and your Mac. Agents keep running
  against real files on the Mac; only text, audio and the live view travel.
- Agent sockets and servers bind to localhost only:
  - `codex app-server` over stdio
  - `opencode serve --hostname 127.0.0.1` with a password
  - `droid` over stdio

  SOURCE is the only thing exposed to the phone, behind existing pairing.
- **Sending prompts is an explicit choice.** Pairing today lets a phone upload
  recordings. With `/v1/agent`, a paired phone can also drive coding agents that
  act on real files, so:
  - A Mac setting "Allow phone to send prompts to agents" is **off by default**.
    With it off, the phone can still browse sessions and read conversations.
  - `/v1/agent` accepts the token in the `Authorization` header only. The
    existing `?token=` query fallback isn't allowed, because URLs end up in logs.
  - Every spoken prompt is also saved to the timeline as a `source-mobile`
    transcript, so there's a record of what was sent and when.
- SOURCE never tries to make its messages count as your approval inside an
  agent app. Approvals from the phone only go through each app's own supported
  approval mechanism. Only the paired device can answer them, the same allowlist
  idea as Claude Code channels.
