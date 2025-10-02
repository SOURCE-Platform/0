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
