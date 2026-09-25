# Agentic Engineering in SOURCE

Status (September 2026):

- **Done:** research and proof tests; the Agents tab on the Mac; the M0
  go/no-go tests; M1 (send prompts to Claude Code from the Agents tab and get
  the reply back).
- **Built, waiting for an on-phone check:** M2, the Agents screen on the phone
  (see sessions, read a Claude Code conversation, type a prompt into it).
- **Next:** push-to-talk with a spoken reply (M3).
- **Build order and progress:** [implementation-plan.md](implementation-plan.md).

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
| Claude Code | Yes, by continuing the conversation (tested) | Yes, between tool calls (tested) | Yes | Not live; Cmd+R in the Claude app shows them |
| ChatGPT web | Only by automating the browser | – | – | Deferred |

Supporting tests:

- **Router accuracy** on 25 sample sentences. A local model on the Mac got 21/25
  in about 2.3 s each, offline and free. OpenCode's free cloud model got 25/25
  but sometimes took 45+ s. See [voice-router.md](voice-router.md).
- **Spoken replies.** The Kokoro voice already on this Mac starts speaking
  about 1 s after a reply arrives when it speaks one sentence at a time. See
  [phone-experience.md](phone-experience.md).

## Documents

- [implementation-plan.md](implementation-plan.md): the build order, step by
  step, across the Mac and iPhone repos, with status.
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

Summary only; the steps and "done when" checks are in
[implementation-plan.md](implementation-plan.md).

1. **M0 Go/no-go.** ✅ Done: continue the conversation in SOURCE's own Claude
   process, not peer messages.
2. **M1–M3 First usable slice.** Send to Claude from the Mac, then from the phone,
   then by holding a button and speaking; the phone speaks a short reply.
3. **M4 Away from home.** Tailscale.
4. **M5 Steer and queue for all four apps.** Codex, OpenCode, Factory.
5. **M6 Hands-free streaming with the router.**
6. **M7 Catch me up.** Global chat, approvals, the agent speaking first.
7. **M8 Live view.** The app you're building on your phone, started for you.
8. **M9–M10.** Mac screen streaming, then ChatGPT web.

## Decision log

| # | Decision | Why |
|---|---|---|
| D1 | Talk to agents through their own protocols, not by typing into their windows | All four expose a protocol; typing into windows is slow and breaks when the UI changes. SOURCE's existing typing path (`TextInserter`) stays a last resort. |
| D2 | Queue by default; steer only on correction phrases ("I forgot…", "no, I meant…", "actually stop") | Matches how you already use Codex. Codex and OpenCode have native steer; Factory queues. |
| D3 | Run the router locally (a ~4B model on MLX) plus simple keyword rules; cloud models only for heavy summaries | Local routing took about 2.3 s, offline and free. The free cloud model was accurate but had 45–55 s stalls. |
| D4 | Speak replies with Kokoro, one sentence at a time; production path is FluidAudio's Kokoro in SOURCE's Swift helper | About 1 s to first audio with the install already on this Mac; FluidAudio is already a SOURCE dependency. |
| D5 | Web live view = the phone's WebKit loading the Mac dev server, not Chromium and not video | iPhone browsers all use WebKit, so it's true mobile rendering, and it's interactive. |
| D6 | Away from home via Tailscale | The existing pairing pins the Mac's certificate fingerprint and ignores the hostname, so the pairing carries over. |
| D7 | First usable slice: push-to-talk into Claude Code, with a spoken reply | The core loop proves the product; smart routing and live view build on it. Claude Code is the app used most, and steering works best there. |
| D8 | Home Wi-Fi first; Tailscale in M4 | Keeps the first slice small; the pairing already supports adding a second host later. |
| D9 | Reading (`core/agent_sessions`) and sending (`core/agent_bridge`) are separate modules | The session hub promises it never writes to an agent app; keeping that true makes it safe to open anytime. |
| D10 | Watch files for changes (FSEvents); no polling loops | Reacts the moment something changes and does nothing otherwise. |
| D11 | Phone sending is off until enabled on the Mac; `/v1/agent` accepts header auth only | A paired phone becomes able to drive coding agents, so that ability must be an explicit choice. |
| D12 | Reach Claude Code by continuing the conversation in a SOURCE-owned Claude process (`--resume`), taking it over from the Claude app when idle | Tested in M0: words arrive as yours, corrections land mid-task, and Claude's memory includes them when you return to the app. Peer messages were sometimes refused as unverified. Trade-off: the Claude app window only catches up when it reopens the conversation, and SOURCE must hold the conversation only while Claude is replying. |

## Open questions

- Does the ChatGPT app pick up a Codex queued message (`codex queue`) or a turn
  started by another process, and show it live?
- Factory: does Factory.app show turns added by an outside `droid` process?
- How messy is real streamed speech compared with the typed test sentences?
