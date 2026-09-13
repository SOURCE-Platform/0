# Phone experience: Stream mode

## Where it lives in the phone app

The phone app (`~/Documents/source mobile`, SwiftUI, iOS 17+) has two pages:
Record and Recordings. Its "Dictation / Podcast" picker is a recording-quality
setting (`RecordingQuality`) that gets saved with every clip, so Stream
shouldn't be a third case of it.

Instead:

- Add a mode switch (`dictation | podcast | stream`) that maps the first two to
  `RecordingQuality`.
- **Stream** opens a full-screen agent experience: a `fullScreenCover` holding
  a `NavigationStack`.
- New code goes in `UI/Stream/`, `Net/Agent…` and `Audio/Speech…`, matching
  the app's folder layout.

## Screens

1. **Sessions.** Big cards, one per session:
   - app logo (Codex / Claude / Factory / OpenCode)
   - title and project
   - status: working / waiting for you / done
   - last line of the agent's reply

   A "global" card at the top is the cross-session chat ("catch me up on
   everything").
2. **Session view.** Three views of the same session, switched with a segmented
   control or swipe:
   - **Live**: the app being built (web view or Mac screen, see
     [transport-and-live-view.md](transport-and-live-view.md)). Rotate to
     landscape for desktop layouts.
   - **Chat**: the conversation. Agent messages have a brief summary on top
     and the full text folded below, with a **Copy** button on every message
     and plan.
   - **Listen**: large captions of the latest spoken summary, with
     replay/skip. Made for walking, dishes, the gym.
3. **Always-visible voice bar.**
   - Live partial transcript and where it will go ("→ Marketing site · queue").
   - Undo toast after each send.
   - Push-to-talk / hands-free toggle.

## Preferences

| Setting | Options | Default |
|---|---|---|
| Voice input | Hands-free stream · tap to talk · hold to talk | Hands-free |
| When I open a session | Show live view · show chat · just listen | Live view |
| Start the dev server when I open a session | on / off | on |
| Spoken replies | Summary only · summary + key points · off | Summary only |
| Confirm before sending | Only when unsure · always · never | Only when unsure |

## Spoken replies

**Style.**

- Reply shaping happens on the Mac, not by prompting the agents differently.
- The router turns each completed agent turn into a 1–3 sentence plain-language
  summary: what changed, what's needed from you, whether it's done.
- The full text stays in Chat.

**P6: speed of the Kokoro voice already on this Mac.** Test setup:

- Install: `~/Documents/source-founder-os/.venv-kokoro` (kokoro 0.9.4, PyTorch).
- Weights: `hexgrad/Kokoro-82M`, run offline.
- Reply: two sentences, about 9.7 s of speech.

| Mode | Time to first sound | Whole reply |
|---|---|---|
| Whole reply as one chunk, CPU | 5.1 s | 5.1 s |
| One sentence at a time, CPU | **1.0 s** | 4.8 s |
| One sentence at a time, Apple GPU (after warm-up) | 1.0 s | 3.2 s |
| Cold start (imports + model load) | +15 s once | – |

**Takeaways.**

- Speak sentence by sentence.
- Keep the voice process warm.
- The first GPU run was slow (14 s), so warm it up at launch.

**Production path.** FluidAudio, already linked into SOURCE's Swift helper
(`src-tauri/native-pkg`), ships Kokoro as CoreML (`KokoroAneManager`, one
English voice `af_heart`) plus a multi-voice option (`PocketTtsSynthesizer`).

- Its docs claim 3–11× faster than real time on Apple Silicon.
- It would add TTS to the helper that already does speech-to-text, with no
  Python.
- Not benchmarked yet: it downloads its CoreML models on first use.

**Where audio plays.**

- The Mac makes the audio and streams it to the phone over the same socket.
- Fallback: iOS `AVSpeechSynthesizer` on the phone when the Mac is unreachable
  or for instant short cues ("Sent.").

**Echo.** The phone records with plain `AVAudioEngine`, with no voice
processing. While it plays replies and streams the mic at the same time, it
would hear itself. Stream mode must enable voice processing on the input node
(`setVoiceProcessingEnabled(true)`) and duck or pause routing during playback.

## The global chat ("catch me up")

One conversation above all sessions, backed by the router rather than any
single agent:

- **"What should I work on?" / "Catch me up on everything."** Merges per-session
  summaries by last activity.
- **"What did we do on the marketing site yesterday, and in which app?"** Uses
  the session index and SOURCE's own timeline (`core/context_timeline`).
- **"Start the SAMI app and open it."** Router commands.

This also fixes the rate-limit problem ("my Codex window ran out, which app did
I continue in?"). The session index already knows that, because it spans all
four apps.

## The agent speaks first

Sometimes you won't look at the screen at all: you open Stream mode on a walk
or while doing the dishes. So the router can start the conversation:

- **On entering Stream mode** (a preference, off by default), it speaks a short
  opener built from the session index: "Two sessions are waiting for you: the
  marketing site finished, and the Mac app is asking a question. What do you
  want to work on?"
- **When a watched session finishes or needs you,** it says so once, briefly:
  "The Mac app is done. Want the summary?"
- **It never interrupts you mid-sentence** and never repeats an announcement
  you've already heard.

Built in M7, on top of the per-session summaries.

## Approvals from the phone

Agents sometimes stop and ask permission to run something. Each protocol
forwards those requests:

- Codex: `item/commandExecution/requestApproval`
- Factory: `droid.request_permission`
- OpenCode: `/api/session/:id/permission`
- Claude: channel permission relay, or SDK `canUseTool`

Show them as a card with **Allow / Deny** plus a spoken prompt ("Codex wants to
run npm install. Allow?"), and accept "yes" or "no" by voice.
