# Zero + Fluid Voice Transcription Architecture

## Status

This is the proposed transcription architecture. Zero already has audio
capture and transcription work in progress, but the Fluid Voice takeover and
transcript handoff described here still need to be implemented.

## Data flow

```mermaid
flowchart TD
    capture["Zero continuously captures microphone + desktop audio"]
    queue["Zero timestamps and queues audio chunks"]
    active{"Is a Right Option prompt active?"}

    background["Fluid Voice model transcribes the next background chunk"]
    backgroundResult["Send timestamped transcript to Zero timeline"]

    trigger["User presses Right Option"]
    priority["Fluid Voice claims Priority 1 for the shared speech model"]
    yield["Background model job yields"]
    buffer["Zero keeps capturing and buffers new background chunks"]
    live["Fluid Voice transcribes the live prompt"]
    finish{"Right Option pressed again?"}
    finalize["Fluid Voice finalizes the prompt transcript"]
    zero["Send completed transcript event to Zero"]
    field["Type the same transcript into the focused text field"]
    release["Fluid Voice releases model priority"]
    resume["Zero resumes the oldest queued background job"]

    capture --> queue --> active
    active -- "No" --> background --> backgroundResult --> active

    trigger --> priority --> yield
    active -- "Yes" --> yield
    yield --> buffer --> live --> finish
    finish -- "No: keep dictating" --> live
    finish -- "Yes" --> finalize
    finalize --> zero
    finalize --> field
    zero --> release
    field --> release
    release --> resume --> active
```

## What “takeover” means

Fluid Voice takes over use of the shared transcription model, not Zero's
audio capture pipeline. While the prompt is active:

- Zero continues recording microphone and desktop audio.
- Background audio chunks remain safely queued in Zero.
- Fluid Voice uses the speech model for the live prompt so dictation stays
  responsive.
- The second Right Option press finalizes the prompt.
- Fluid Voice sends the completed transcript to Zero and also types it into
  the currently focused application.
- The shared model then returns to Zero's queued background work.

## Required integration contract

Fluid Voice needs to publish a local transcript-completed event or call a
local Zero endpoint with at least:

- transcript text;
- prompt start and end timestamps;
- a stable transcript or prompt identifier;
- source set to `fluid-voice-prompt`;
- finality set to `final`.

Zero should acknowledge receipt, deduplicate by identifier, and place the
prompt transcript on the unified timeline. Audio capture must not depend on
that handoff succeeding; failed deliveries should be retried without losing
the background recording.
