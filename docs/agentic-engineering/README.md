# Agentic Engineering in SOURCE

Status: research phase (September 2026). Nothing here is built yet.

## The idea, in plain terms

You run coding agents in four apps: Codex (inside the ChatGPT app), Claude Code
(Claude desktop app), Factory, and OpenCode. Today that means sitting at the Mac and
jumping between windows. The goal is to walk away from the Mac and keep working
by voice from the SOURCE phone app:

1. A new **Stream** mode on the phone sends your voice live to the Mac.
2. A **router** on the Mac listens to the transcript. For each thing you say,
   it decides whether it's work or background talk. If it's work, it picks the
   app and conversation it belongs to, and whether it waits its turn (**queue**)
   or changes what the agent is doing right now (**steer**).
3. The prompt goes straight into that agent through its own programming
   interface. No clicking, no pasting.
4. Replies come back to the phone as short text and a spoken summary.
5. On the phone you can see all sessions, open one, look at the app being
   built, and say "catch me up".

## Is it possible? Short answer: yes, for all four apps

Proof tests on this Mac (details in [harness-integration.md](harness-integration.md)):

| App | Send a prompt from outside | Change a running answer | Replies stream back | Seen live in the app's own window |
|---|---|---|---|---|
| Codex | Yes (tested) | Yes, built-in steer (tested) | Yes | Not yet verified |
| Factory | Yes (tested) | Queues it for the next turn (tested) | Yes | Not tested |
| OpenCode | Yes (tested) | Built-in steer/queue switch (tested) | Yes | Only after reloading the window |
| Claude Code | Yes (tested) | Yes, between tool calls (tested) | Yes | Yes: a test message landed in an open desktop session |
| ChatGPT web | Only by automating the browser | – | – | Deferred |

Supporting tests:

- **Router accuracy** on 25 sample sentences. A local model on the Mac got 21/25
  in about 2.3 s each, offline and free. OpenCode's free cloud model got 25/25
  but sometimes took 45+ s. See [voice-router.md](voice-router.md).
- **Spoken replies.** The Kokoro voice already on this Mac starts speaking
  about 1 s after a reply arrives when it speaks one sentence at a time. See
  [phone-experience.md](phone-experience.md).

## Documents

- [harness-integration.md](harness-integration.md): how SOURCE talks to each
  agent app, with test results.
- [voice-router.md](voice-router.md): how speech becomes a routed prompt, with
  the accuracy test.
- [phone-experience.md](phone-experience.md): Stream mode screens, spoken
  replies, preferences, the "catch me up" chat.
- [transport-and-live-view.md](transport-and-live-view.md): phone ↔ Mac
  connection, use away from home, seeing the app you're building, starting dev
  servers.

## Roadmap

1. **Session hub (Mac).** One list of every session across the four apps: title,
   project, app, last activity, busy or idle. Mirrored read-only to the phone.
2. **Stream mode.** Voice → live transcript → router → send. Push-to-talk first,
   then always-on streaming.
3. **Replies.** Brief text and speech back to the phone, "catch me up", approving
   agent permission prompts from the phone.
4. **Live view.** Tap a session and the dev server starts, and the site opens in
   the phone's own browser engine. Tailscale for use away from home Wi-Fi.
5. **Later.** Streaming the Mac screen for native apps, and ChatGPT web through
   browser automation.

## Decision log

| # | Decision | Why |
|---|---|---|
| D1 | Talk to agents through their own protocols, not by typing into their windows | All four expose a protocol; typing into windows is slow and breaks when the UI changes. SOURCE's existing typing path (`TextInserter`) stays a last resort. |
| D2 | Queue by default; steer only on correction phrases ("I forgot…", "no, I meant…", "actually stop") | Matches how you already use Codex. Codex and OpenCode have native steer; Factory queues. |
| D3 | Run the router locally (a ~4B model on MLX) plus simple keyword rules; cloud models only for heavy summaries | Local routing took about 2.3 s, offline and free. The free cloud model was accurate but had 45–55 s stalls. |
| D4 | Speak replies with Kokoro, one sentence at a time; production path is FluidAudio's Kokoro in SOURCE's Swift helper | About 1 s to first audio with the install already on this Mac; FluidAudio is already a SOURCE dependency. |
| D5 | Web live view = the phone's WebKit loading the Mac dev server, not Chromium and not video | iPhone browsers all use WebKit, so it's true mobile rendering, and it's interactive. |
| D6 | Away from home via Tailscale | The existing pairing pins the Mac's certificate fingerprint and ignores the hostname, so the pairing carries over. |

## Open questions

- Does the ChatGPT app pick up a Codex queued message (`codex queue`) or a turn
  started by another process, and show it live?
- Factory: does Factory.app show turns added by an outside `droid` process?
- How messy is real streamed speech compared with the typed test sentences?
