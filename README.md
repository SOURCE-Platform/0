# 0

**The digital observation layer for [SOURCE](https://sourceovcourse.com).**
0 is local-first desktop software that records what happens on a computer
while a person uses it: the screen and the text on it, speech, keystrokes,
pointer movement, and gaze. It keeps that record on the person's own machine.

0 is early, working software. Parts of it run every day; other parts are
designed but not built yet. [Where the project is](#where-the-project-is)
lists which is which.

## Why 0 exists

Today's AI knows a great deal about the world and very little about the
person using it. What an agent can see of someone is mostly what that person
types into a chat box or what a connected app happens to expose. Most of the
context that would make an assistant useful, like routines, current work,
what was on screen an hour ago and what changed since last week, is never
written down.

SOURCE is an open-source research project exploring a different basis for
personal AI. It learns about a person from **continuous, consented, verified
observations of their physical and digital life**. That processing runs on
hardware the person owns, and the resulting model stays under their control.

The first SOURCE environment is deliberately small: one person at one
computer. A person working at a desk produces two streams at once:

- **Physical:** who is present, how they move and react, what they say, and
  how they interact with the workspace.
- **Digital:** what appears on screen, which keys are pressed, where the
  pointer goes, and how the session changes over time.

SOURCE captures those streams with three parts:

| Part | Role |
|---|---|
| [**DeepReal**](https://github.com/SOURCE-Platform/deepreal) | A multi-sensor hardware array mounted on the display. It observes the person at the computer and is designed to sign each observation inside a trusted execution environment. |
| **0** (this repo) | Software on the computer. It records the digital stream at the operating-system level. |
| **Continuous Human Model** | The proposed, evolving representation of one person, built from those authenticated streams. It is meant to support both identity continuity and scoped personal context for agents. |

0 serves two purposes:

1. **Evidence.** Digital observations can be checked against DeepReal's
   physical ones. DeepReal sees a hand move toward a key; 0 sees that key
   arrive in the operating system at the same moment. Neither proves much
   alone. Together they are expensive to fake, because an attacker would have
   to fabricate a physical record and a digital record that agree frame by
   frame.
2. **Context.** 0 provides the digital context needed to test the project's
   central hypothesis: that a personal agent becomes meaningfully more useful
   when it can learn from ordinary work over time.

### Why verification is part of the design

A model that learns from observation is only as good as the observations
behind it. Someone else can use the computer. Credentials get stolen.
Activity can be generated remotely. Keyboard, microphone and camera feeds can
be spoofed or replayed, and live video and voice can already be deepfaked. A
personal AI that cannot tell these cases apart will learn from a manufactured
reality, the wrong person, or both. SOURCE therefore treats identity and data
origin as part of the observation system itself, not a check added later.

## Principles

- **Local-first.** Captured data lives on the user's machine by default.
- **Consent-gated.** Capture requires the user's OS-level permission, and
  every source can be switched off.
- **Separate decisions.** Recording an event does not make it permanent
  history. Capture, retention, feature extraction, model updates, identity
  verification and agent access are meant to be separate, user-controlled
  decisions. This is design intent; the architecture is still being built
  toward it.
- **Minimal retention.** Where raw data is not needed, it is not kept. See
  the retention defaults under [Capture](#capture).
- **Open.** A system that observes a person continuously should be
  inspectable by that person. 0 is GPL-3.0 software, and SOURCE is designed
  as open hardware, open software and an open protocol.

## Where the project is

*Last updated September 2026.* SOURCE is early. Nothing below should be read
as a claim that the verification chain works end to end today.

**Platform.** Development happens on macOS on Apple Silicon, and that is the
only platform exercised day to day. The keyboard and mouse capture layer has
Windows and Linux implementations. The dictation helper and the credential
vault are currently macOS-only.

### Built and in daily use

#### Capture

- **Screen and on-screen text.** 0 records screen keyframes and reads their
  text with OCR, titled with the app in front at that moment. Screenshots are
  deleted as soon as their text is read, and screen video is not kept by
  default.
- **Audio and transcription.** 0 records microphone audio and transcribes it
  on-device with a bundled FluidAudio speech engine. By default only
  transcripts are kept; the audio is discarded. It follows microphone changes
  and shows which mic is recording.
- **Dictation.** Hold Right Option to dictate into any text field. A floating
  pill shows a live waveform and preview words. Each transcript goes to the
  focused field and to the timeline. A custom dictionary corrects recurring
  words.
- **Keyboard and mouse.** 0 captures keystrokes and pointer movement at the
  operating-system level. Keyboard capture pauses whenever macOS Secure Input
  is active, for example during password entry. Screen frames and keystrokes
  are also suppressed while one of 0's own sensitive windows, such as the
  vault, is on screen.
- **Gaze.** 0 estimates where on the screen you are looking from the camera,
  after a short calibration. It uses MediaPipe face and iris tracking, head
  pose, and a MobileOne ONNX gaze model.

#### The app

- **Timeline.** A zoomable, pannable timeline of the day. It currently shows
  audio, transcript and OCR rails, with a detail panel for each block.
- **Agents.** A hub that lists coding-agent sessions from Claude Code, Codex,
  Factory and OpenCode. From it you can read a Claude Code conversation and
  send prompts into it, including taking a conversation over from the Claude
  app safely.
- **Phone companion.** A separate iPhone app, not yet public, pairs with 0
  over a pinned local connection using a QR code. It can send recordings for
  transcription, list agent sessions, open a conversation and send prompts
  into it.
- **Settings.** Per-source capture controls, microphone meters, an OCR
  configuration, storage overview, hardware info, gaze calibration, and
  privacy filters with persistent block lists.

#### Hardening

A strict content security policy and a narrowed asset scope for the WebView.
Hardened runtime with minimal entitlements. `cargo audit` and `cargo vet`
supply-chain checks, and a tracked `Cargo.lock`.

### In progress

- **Credential vault** (the Vault tab). This is a password and identity vault
  built to the standard its surroundings demand: 0 itself holds Screen
  Recording, Accessibility and Input Monitoring permissions, so the vault
  cannot trust it.
  - **Design.** Vault keys live in a separate hardened helper process that
    authenticates the app by code signature. Device keys are backed by the
    Secure Enclave. Unlocking works with a master password or an offline
    Recovery Key, and a lost device can be revoked. The provider never holds
    decryption capability.
  - **Progress.** Phases A–E.1 are implemented and verified with recorded gate
    evidence: helper skeleton, cryptographic core, local vault, recovery,
    device enrollment and revocation. Phase F, encrypted backup and Mac ↔
    iPhone sync, is under way; its revision model and merge rules are
    committed.
  - **Limits.** The vault is exercised with synthetic data only and is not yet
    used for real credentials. Design and verification records are in
    [`docs/security/`](docs/security/).
- **Voice control of coding agents** ("Agentic Engineering"). The goal is to
  walk away from the Mac and keep working by voice from the phone. Milestones
  M0–M2 are done. M3, push-to-talk with a spoken reply, is built and awaiting
  on-phone verification. Steering running agents, remote access, hands-free
  streaming with a local router, and live views follow. See
  [`docs/agentic-engineering/`](docs/agentic-engineering/).
- **Identity surface** (the ID tab). A local identity profile that ties the
  record to a person rather than an account: a 3D head scan and personal
  documents. The UI exists, and the underlying identity work is early.

### Designed, not built

- **Tamper-evident record.** Signing captured observations and hashing frames
  and audio into a record that cannot be quietly altered.
- **Pairing with DeepReal.** Cross-checking what the hardware sees against
  what the operating system reports.
- **Continuous Human Model interfaces.** A narrow verification interface
  answers "is this the same person?" without exposing history. A scoped
  context interface gives an agent only what its task permits.

The goal of the current phase is to join these pieces into one working
research system and begin testing the larger thesis with real longitudinal
use.

## Architecture

0 is a [Tauri 2](https://tauri.app) app. It has a Rust backend with native
system access, a React 19 + TypeScript frontend, and native Swift helpers for
work that needs Apple frameworks.

```
src/                        React frontend (Timeline, ID, Agents, Vault, Settings)
src-tauri/
  src/
    app/                    Tauri commands and startup wiring
    core/                   Capture, OCR, transcription, timeline, agents, mobile server
    platform/               OS-specific capture (macOS / Windows / Linux)
    models/                 Shared data types
  migrations/               SQLite schema
  native-pkg/               Swift dictation helper (FluidAudio engine, overlay pill)
  vault-helper/             Isolated credential-vault process
  vault-proto/              Vault wire formats and cryptographic building blocks
  vault-apple-crypto/       Secure Enclave and Keychain bindings
  poc/                      Interoperability proofs (HPKE with the Secure Enclave)
docs/                       Architecture and security documents
scripts/                    Build, signing, audit and phase-gate scripts
```

## Running it

Requirements: macOS on Apple Silicon, Node.js, a stable Rust toolchain,
Xcode (for the Swift helpers), and FFmpeg 8 or later (`brew install ffmpeg`).

```bash
npm install
npm run tauri dev      # Vite on :1420 plus the Rust app, with hot reload
npm run tauri build    # production bundle
```

The first build compiles several hundred Rust crates and takes a few minutes.
On first launch, macOS asks for the permissions each capture source needs:
Screen Recording, Accessibility, Input Monitoring, Microphone and Camera.
Every source can be switched off in Settings.

The credential vault's helper authenticates the app by code signature, so
vault features need a signed development build
(`scripts/dev-run-signed.sh` is wired in as the Cargo runner).

## Documentation

- [Transcription architecture](docs/transcription-architecture.md)
- [Gaze model selection](docs/gaze-model-selection.md)
- [Agentic Engineering](docs/agentic-engineering/README.md): the plan and
  progress for voice control of coding agents
- [Credential vault security architecture](docs/security/credential-vault-security-architecture.md)
  and [implementation specification](docs/security/credential-vault-implementation-spec.md)
- [Testing](TESTING.md)
- Project site: [sourceovcourse.com](https://sourceovcourse.com). See the
  [0 page](https://sourceovcourse.com/0),
  [Continuous Human Model](https://sourceovcourse.com/continuous-human-model)
  and [Vision](https://sourceovcourse.com/vision).

## License

[GPL-3.0](LICENSE).
