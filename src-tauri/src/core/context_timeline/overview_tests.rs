#[cfg(test)]
mod overview_tests {
    use super::*;

    async fn database() -> Arc<Database> {
        let pool = sqlx::sqlite::SqlitePoolOptions::new().max_connections(1)
            .connect("sqlite::memory:").await.unwrap();
        let db = Arc::new(Database { pool });
        db.run_migrations().await.unwrap();
        init_schema(&db).await.unwrap();
        for table in ["keyboard_events", "mouse_events"] {
            sqlx::query(&format!("CREATE TABLE {table} (
                timestamp INTEGER, app_name TEXT, window_title TEXT, key_char TEXT)"))
                .execute(db.pool()).await.unwrap();
        }
        db
    }

    #[tokio::test]
    async fn every_capture_source_has_an_overview_track() {
        let db = database().await;
        let timeline = build_context_timeline_overview(&db, 0, 60_000).await.unwrap();
        for id in ["audio_dictation", "audio_ambient_speech", "audio_sound_events", "audio_mobile",
            "ocr", "keyboard", "mouse", "evidence", "vision", "attention", "focus", "visible_windows", "system"] {
            assert!(timeline.rails.iter().any(|rail| rail.id == id), "Missing {id}");
        }
        assert_eq!(timeline.rails[0].label, "Dictation");
    }

    #[tokio::test]
    async fn input_overview_counts_all_records_without_loading_typed_text() {
        let db = database().await;
        for timestamp in [1_000, 1_001, 2_000, 70_000] {
            sqlx::query("INSERT INTO keyboard_events VALUES (?, 'Editor', 'Work', 'secret-key-text')")
                .bind(timestamp).execute(db.pool()).await.unwrap();
        }
        let rail = activity_overview(&db, 0, 120_000, "keyboard_events", "keyboard", "Keyboard Activity")
            .await.unwrap();
        assert_eq!(rail.slices.len(), 2, "Empty buckets must remain gaps");
        assert_eq!(rail.slices.iter().map(|s| s.row_count).sum::<u64>(), 4);
        assert_eq!((rail.slices[0].start_timestamp, rail.slices[0].end_timestamp), (1_000, 2_001));
        assert!(!serde_json::to_string(&rail).unwrap().contains("secret-key-text"));
        let detail = get_context_slice_detail(&db, rail.slices[0].clone()).await.unwrap();
        assert_eq!(detail.row_count, 3);
        let json = serde_json::to_string(&detail).unwrap();
        assert!(json.contains("Editor"));
        assert!(!json.contains("secret-key-text"));
        assert!(overview_bucket_ms(0, 86_400_000) >= 86_400_000 / 600);
    }

    #[tokio::test]
    async fn compact_app_spans_do_not_send_window_payloads_until_selection() {
        let db = database().await;
        let windows = vec![WindowSnapshotDto { app_name: "Private Window App".to_string(),
            bundle_id: "example.private".to_string(), process_id: 1, is_frontmost: true, confidence: 1.0 }];
        for timestamp in [1_000, 6_000] {
            insert_window_snapshot(&db, None, timestamp, Some("Editor"), Some("example.editor"),
                &windows, "test", 0.9).await.unwrap();
        }
        let rails = build_desktop_overviews(&db, 5_000, 20_000).await.unwrap();
        let running = rails.iter().find(|rail| rail.id == "visible_windows").unwrap();
        assert_eq!(running.slices.len(), 1);
        assert!(running.slices[0].start_timestamp < 5_000, "Spans crossing the viewport edge are retained");
        assert!(!serde_json::to_string(running).unwrap().contains("Private Window App"));
        let detail = get_context_slice_detail(&db, running.slices[0].clone()).await.unwrap();
        assert_eq!(detail.visible_windows.len(), 1);
        assert_eq!(detail.visible_windows[0].app_name, "Private Window App");
    }

    #[tokio::test]
    async fn focus_recording_is_independent_of_running_app_capture_and_respects_session_gaps() {
        let db = database().await;
        sqlx::query("INSERT INTO sessions (id, start_timestamp, end_timestamp, device_id, created_at)
            VALUES ('session', 0, 40_000, 'device', 0)")
            .execute(db.pool()).await.unwrap();
        for (timestamp, app) in [(1_000, "Editor"), (20_000, "Browser")] {
            insert_context_event(&db, Some("session"), timestamp, "focus", "frontmost_changed",
                "test", 1.0, Some(serde_json::json!({"title":app,"app_name":app}).to_string()))
                .await.unwrap();
        }
        let rails = build_desktop_overviews(&db, 10_000, 60_000).await.unwrap();
        let focus = rails.iter().find(|rail| rail.id == "focus").unwrap();
        assert_eq!(focus.slices.len(), 2);
        assert_eq!(focus.slices[0].title, "Editor");
        assert_eq!(focus.slices[1].end_timestamp, 40_000);
        let later = build_desktop_overviews(&db, 50_000, 70_000).await.unwrap();
        assert!(later.iter().find(|rail| rail.id == "focus").unwrap().slices.is_empty());
    }
}
