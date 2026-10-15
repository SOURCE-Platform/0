use super::sampler::persist_context_snapshot;
use crate::core::context_timeline;
use crate::core::database::Database;
use crate::models::activity::AppInfo;
use sqlx::sqlite::SqlitePoolOptions;
use std::collections::HashSet;
use std::sync::Arc;

async fn test_database() -> Arc<Database> {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("test database should open");
    let database = Arc::new(Database { pool });
    context_timeline::init_schema(&database)
        .await
        .expect("context timeline schema should initialize");
    database
}

fn example_app() -> AppInfo {
    AppInfo::new("Example".to_string(), "com.example.app".to_string(), 42)
}

#[tokio::test]
async fn audio_only_capture_does_not_persist_desktop_context() {
    let database = test_database().await;
    let app = example_app();
    let mut previous_frontmost = None;
    let mut previous_running = HashSet::new();

    persist_context_snapshot(
        database.clone(),
        Some("audio-only-session".to_string()),
        Some(app.clone()),
        vec![app],
        &mut previous_frontmost,
        &mut previous_running,
        false,
        false,
        false,
    )
    .await
    .expect("disabled desktop channels should not fail");

    let event_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM context_events")
        .fetch_one(database.pool())
        .await
        .expect("context event count should load");
    let snapshot_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM window_snapshots")
        .fetch_one(database.pool())
        .await
        .expect("window snapshot count should load");

    assert_eq!(event_count, 0);
    assert_eq!(snapshot_count, 0);
}

#[tokio::test]
async fn enabled_context_rails_only_persist_their_own_data() {
    let database = test_database().await;
    let app = example_app();
    let mut previous_frontmost = None;
    let mut previous_running = HashSet::new();

    persist_context_snapshot(
        database.clone(),
        Some("visible-only-session".to_string()),
        Some(app.clone()),
        vec![app],
        &mut previous_frontmost,
        &mut previous_running,
        false,
        false,
        true,
    )
    .await
    .expect("visible windows sampling should succeed");

    let event_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM context_events")
        .fetch_one(database.pool())
        .await
        .expect("context event count should load");
    let snapshot_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM window_snapshots")
        .fetch_one(database.pool())
        .await
        .expect("window snapshot count should load");

    assert_eq!(event_count, 0);
    assert_eq!(snapshot_count, 1);
}

#[tokio::test]
async fn unchanged_running_apps_extend_one_snapshot_until_state_changes() {
    let database = test_database().await;
    let app = example_app();
    let mut previous_frontmost = None;
    let mut previous_running = HashSet::new();
    for running in [
        vec![app.clone()],
        vec![app.clone()],
        vec![
            app.clone(),
            AppInfo::new("Other".into(), "com.example.other".into(), 43),
        ],
    ] {
        persist_context_snapshot(
            database.clone(),
            Some("session".into()),
            Some(app.clone()),
            running,
            &mut previous_frontmost,
            &mut previous_running,
            false,
            false,
            true,
        )
        .await
        .expect("snapshot should persist");
    }
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM window_snapshots")
        .fetch_one(database.pool())
        .await
        .unwrap();
    assert_eq!(count, 2);
    let observed: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM window_snapshots WHERE last_seen_at >= timestamp")
            .fetch_one(database.pool())
            .await
            .unwrap();
    assert_eq!(observed, 2);
}
