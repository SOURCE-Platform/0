# Database Testing Guide

## Automated Tests

### Unit Tests (In-Memory Database)
Run the comprehensive test suite with 4 tests covering all CRUD operations:

```bash
cd src-tauri
cargo test --lib core::database::tests
```

**Tests included:**
- `test_create_and_get_session`: Verifies session creation and retrieval
- `test_end_session`: Validates updating session end timestamps
- `test_delete_session`: Confirms session deletion
- `test_list_sessions`: Tests querying multiple sessions

### Integration Test (Real Database)
Run the example program that creates a real database file and performs operations:

```bash
cd src-tauri
cargo run --example test_database
```

**This test:**
1. Creates `~/.observer_data/database/observer.db`
2. Runs migrations (creates tables and indexes)
3. Creates a test session
4. Retrieves and updates the session
5. Lists all sessions
6. Reports database file size and location

## Manual Testing

### 1. Inspect Database Schema
```bash
sqlite3 ~/.observer_data/database/observer.db ".schema"
```

**Expected output:**
- `_sqlx_migrations` table (migration tracking)
- `sessions` table with indexes on start_timestamp and device_id
- `consent_records` table with index on feature_name

### 2. Check Migration History
```bash
sqlite3 ~/.observer_data/database/observer.db "SELECT version, description, success FROM _sqlx_migrations;"
```

**Expected:**
- Migration 20251001000001: create sessions table ✓
- Migration 20251001000002: create consent records table ✓

### 3. Query Sessions Data
```bash
sqlite3 ~/.observer_data/database/observer.db "SELECT * FROM sessions;"
```

### 4. Check Database File
```bash
# View database location and size
ls -lh ~/.observer_data/database/observer.db

# Verify it's a valid SQLite database
file ~/.observer_data/database/observer.db
```

## Testing in Your Application

### From Rust Code
```rust
use zero_lib::core::database::Database;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize database
    let db = Database::init().await?;

    // Create a session
    let session_id = uuid::Uuid::new_v4().to_string();
    let start_time = chrono::Utc::now().timestamp();
    db.create_session(&session_id, start_time, "my-device").await?;

    // Retrieve it
    let session = db.get_session(&session_id).await?;
    println!("Session: {:?}", session);

    Ok(())
}
```

### Future: From Tauri Commands (Frontend)
Once Tauri commands are implemented (Phase 1, Task 1.3), you'll be able to test from the UI:

```typescript
import { invoke } from '@tauri-apps/api/core';

// Create session
await invoke('create_session', { deviceId: 'web-client' });

// List sessions
const sessions = await invoke('list_sessions');
console.log(sessions);
```

## Clean Up Test Data

```bash
# Remove database file
rm ~/.observer_data/database/observer.db

# Remove entire data directory
rm -rf ~/.observer_data
```

## Troubleshooting

### "Database locked" errors
The database uses WAL mode (Write-Ahead Logging). If you see lock errors:
1. Close any open sqlite3 sessions
2. Restart the application
3. Check for stale `.db-shm` and `.db-wal` files

### Migrations not running
If schema changes aren't applied:
1. Delete `~/.observer_data/database/observer.db`
2. Run `cargo run --example test_database` again
3. Migrations will run on fresh database

### Test failures
If unit tests fail:
1. Run with verbose output: `cargo test -- --nocapture`
2. Check for port conflicts (tests use in-memory databases, should not conflict)
3. Verify sqlx dependencies are properly installed: `cargo clean && cargo build`

---

# SOURCE V1 Manual Acceptance Checklist

Use this section as the live user-flow checklist for the SOURCE desktop app.
Mark each item as pass, fail, or needs tuning after testing in the UI.

## 0. Preflight

- [ ] App opens with only one SOURCE instance visible in the Dock.
- [ ] App name is SOURCE, not the marketing/CRM app name.
- [ ] Settings loads without a white screen or startup error.
- [ ] Data Mode switch still toggles Real and Mock modes clearly.
- [ ] Settings saved messages appear as toast overlays, not layout banners.
- [ ] All clickable controls show a hand cursor on hover.
- [ ] Paragraph text stays readable at roughly 50 to 60 chars per line.

## 1. Capture Control

- [ ] Go to Settings -> Capture. Click Start Capture.
      Expect the button to become Stop Capture.
- [ ] Stay in Settings -> Capture. Click Stop Capture.
      Expect the status to return to Capture stopped.
- [ ] Go to Timeline. Click Start Capture near the top right.
      Expect the live status pill to say capturing/running.
- [ ] Stay on Timeline. Click Stop Capture.
      Expect the live status pill to say Capture stopped.
- [ ] While capture is running, watch the timeline for 30 seconds.
      Expect new blocks or sample counts on enabled rails.
- [ ] After stopping, wait 30 seconds without changing settings.
      Expect no new capture blocks to appear.
- [ ] Turn on a channel that lacks permission, then start capture.
      Expect a degraded/missing permission message, not silence.

## 2. Device Context Timeline

- [ ] Timeline opens as Device Context Timeline in Real mode.
- [ ] Timeline uses full available page width.
- [ ] The now/live line stays pinned to the right while live.
- [ ] Time moves continuously even if capture is stopped.
- [ ] Blocks appear in near real time while capture is running.
- [ ] Blocks stop appearing after capture stops.
- [ ] Back and Forward shift the visible time window correctly.
- [ ] Command plus scroll zooms the timeline only, not the page.
- [ ] Middle mouse drag pans the timeline left and right.
- [ ] Rail grid lines scale with the top time ruler.
- [ ] Clicking a block opens the right Block Detail panel.
- [ ] Narrow blocks use tooltip/popover detail instead of clipped text.
- [ ] Raw JSON remains available in the detail panel.

## 3. Core Desktop Rails

- [ ] System rail shows capture lifecycle/system events.
- [ ] Focus rail shows frontmost app changes.
- [ ] Visible Windows rail shows visible app/window snapshots.
- [ ] Interaction rail shows keyboard, mouse, idle, or inferred states.
- [ ] Rail info icons explain what each rail means.
- [ ] Disabled channels show empty rails with a clear reason.
- [ ] Enabled channels show sample counts and last sample times.

## 4. OCR Capture

- [ ] OCR channel can be enabled in Settings.
- [ ] Required companion channels are explained clearly.
- [ ] Starting capture with OCR enabled creates OCR blocks.
- [ ] App switching triggers an OCR capture.
- [ ] Major scene change triggers an OCR capture.
- [ ] Scrolling triggers OCR after the page settles.
- [ ] Static fallback creates periodic OCR captures.
- [ ] OCR failures show a clear reason.
- [ ] OCR blocks open a detail view with text and metadata.
- [ ] OCR detail includes app/window attribution when available.
- [ ] OCR detail includes raw structured JSON.
- [ ] PII detections appear inside OCR detail when present.

## 5. OCR Agent Read Layer

- [ ] OCR scene snapshots are created from raw OCR rows.
- [ ] Scene snapshots include trigger reason.
- [ ] Text spans merge repeated visible text across nearby snapshots.
- [ ] Context entities summarize longer app/page/document views.
- [ ] Agent query APIs return JSON without direct SQL access.
- [ ] Search can find captured OCR text by time range.
- [ ] Missing frames do not break OCR text retrieval.

## 6. Vision, Pose, Face, And Iris

- [ ] Camera source selector lists available cameras.
- [ ] Selected camera is saved.
- [ ] Settings shows a live camera preview under the selector.
- [ ] Vision channel degrades clearly if no camera is available.
- [ ] Capture writes pose/body landmark samples.
- [ ] Capture writes face landmark samples.
- [ ] Capture writes iris/eye geometry samples when face is visible.
- [ ] Vision / Scene rail shows visual blocks or spans.
- [ ] Vision detail shows posture, motion, face, iris, and raw JSON.
- [ ] Pose changes like sitting and standing create distinct spans.

## 7. Gaze Calibration

- [ ] Calibration launches on the selected monitor, not inside app bounds.
- [ ] Calibration respects menu bar and Dock safe areas.
- [ ] The selected display name is shown before starting.
- [ ] The selected camera is shown before starting.
- [ ] Calibration warns clearly if camera is unavailable.
- [ ] Moving-dot calibration starts with a short get-ready countdown.
- [ ] Dot moves smoothly between points.
- [ ] Dot pauses briefly before auto-capturing each sample.
- [ ] User can cancel calibration safely.
- [ ] Calibration completes all points without crashing SOURCE.
- [ ] Validation computes error/quality.
- [ ] Active calibration is stored per display.
- [ ] Attention promotion stays off if calibration fails.

## 8. Gaze And Attention Capture

- [ ] Gaze samples are written after calibration is active.
- [ ] Gaze samples include model name/version.
- [ ] Gaze samples include yaw/pitch or vector data.
- [ ] Gaze samples include projected screen point.
- [ ] Gaze samples include confidence and accuracy radius.
- [ ] Gaze / Attention rail appears in Timeline.
- [ ] Attention blocks resolve to OCR/context targets when possible.
- [ ] Attention detail shows projected point and linked target.
- [ ] No active calibration shows a clear degraded reason.
- [ ] Missing gaze model does not silently fall back.

## 9. Audio Source Settings

- [ ] Settings shows Record microphone toggle.
- [ ] Settings shows microphone source dropdown.
- [ ] Dropdown lists MacBook, iPhone, Camo, or other available inputs.
- [ ] Selected microphone is saved.
- [ ] Settings shows Record desktop audio toggle.
- [ ] Desktop audio explains that it is mixed system/app output.
- [ ] Desktop audio does not claim per-app separation.
- [ ] Missing permission shows desktop audio as degraded.

## 10. Audio Capture Pipeline

- [ ] Mic-only capture creates `microphone:*` audio records.
- [ ] Desktop-only capture creates `desktop_output:system` records.
- [ ] Both enabled records mic and desktop at the same time.
- [ ] Speaking creates Speech blocks.
- [ ] Playing YouTube/Spotify creates desktop audio blocks.
- [ ] Silence does not create noisy clutter.
- [ ] ASR transcript appears when speech is detected.
- [ ] If Whisper is missing, speech spans still work without transcript.
- [ ] Audio detail shows source, transcript, raw JSON, and storage.

## 11. Audio Timeline Hierarchy

- [ ] Timeline shows one top-level Audio group.
- [ ] Expanding Audio reveals Speech.
- [ ] Expanding Audio reveals Emotion Summary.
- [ ] Expanding Audio reveals Sound Events.
- [ ] Expanding Emotion Summary reveals emotion detail lanes.
- [ ] Speech blocks appear only in Speech.
- [ ] Transcript markers appear in Speech.
- [ ] Sound events appear only in Sound Events.
- [ ] Emotion segments appear in Emotion Summary.
- [ ] Positive emotions render above center.
- [ ] Negative emotions render below center.
- [ ] Detail lanes show happy, sad, angry, fearful, surprised, neutral, uncertain.

## 12. Emotion And Sound Understanding

- [ ] Emo2Vec/emotion model runs when audio chunks exist.
- [ ] Emotion labels include confidence.
- [ ] Emotion detail links back to speech/audio chunk.
- [ ] Sound-event detector identifies obvious events.
- [ ] Sound-event labels include confidence.
- [ ] Sound events do not overwrite speech detection.
- [ ] Model unavailable states degrade clearly.

## 13. Storage And Data Access

- [ ] Storage page shows database path.
- [ ] Storage page shows recordings folder path.
- [ ] Storage page shows SOURCE total footprint.
- [ ] Storage page shows disk free space.
- [ ] Low disk space warning appears when needed.
- [ ] User can open the storage location.
- [ ] User can delete data per channel.
- [ ] User can delete all captured data.
- [ ] Delete actions ask for confirmation inside the app UI.
- [ ] Deleting data updates storage totals.

## 14. Mock Mode Separation

- [ ] Mock mode shows demo data.
- [ ] Real mode shows only captured real data.
- [ ] Mock data does not leak into real mode.
- [ ] Real data does not leak into mock mode.

## 15. Regression Checks

- [ ] App survives restart after capture.
- [ ] App survives restart after calibration attempt.
- [ ] App survives restart after deleting data.
- [ ] No blank white window appears.
- [ ] No macOS crash dialog appears during normal flows.
- [ ] `npm run build` passes.
- [ ] `cargo check` passes.
- [ ] `npm run check:file-lengths` passes.
