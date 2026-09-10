#[cfg(test)]
mod snapshot_cost_tests {
    use super::*;

    fn window(app: &str, pid: u32) -> WindowSnapshotDto {
        WindowSnapshotDto {
            app_name: app.to_string(),
            bundle_id: format!("com.example.{}", pid),
            process_id: pid,
            is_frontmost: pid == 1,
            confidence: 0.9,
        }
    }

    fn snapshot(
        id: &str,
        timestamp: i64,
        app: Option<&str>,
        windows: &[WindowSnapshotDto],
    ) -> WindowSnapshotRow {
        WindowSnapshotRow {
            id: id.to_string(),
            session_id: Some("session-1".to_string()),
            timestamp,
            frontmost_app_name: app.map(str::to_string),
            frontmost_bundle_id: app.map(|name| format!("com.example.{}", name.len())),
            visible_windows_json: serde_json::to_string(windows).expect("serialize windows"),
            confidence: 0.85,
            source: "os_monitor".to_string(),
        }
    }

    /// The figure computed the old way, by serializing the row, as the reference.
    fn serialized_reference(row: &WindowSnapshotRow) -> u64 {
        serialized_len(&serde_json::json!({
            "id": row.id,
            "session_id": row.session_id,
            "timestamp": row.timestamp,
            "frontmost_app_name": row.frontmost_app_name,
            "frontmost_bundle_id": row.frontmost_bundle_id,
            "visible_windows_json": row.visible_windows_json,
            "confidence": row.confidence,
            "source": row.source,
        }))
    }

    #[test]
    fn snapshot_size_matches_the_serialized_figure() {
        // Quotes, a backslash, and non-ASCII all change the escaped length.
        let windows = vec![window("Ghostty", 1), window("Café \"Notes\" \\ draft", 2)];
        let row = snapshot("snap-1", 1_789_000_000_000, Some("Ghostty"), &windows);
        assert_eq!(window_snapshot_storage_bytes(&row), serialized_reference(&row));
    }

    #[test]
    fn snapshot_size_matches_when_optional_fields_are_missing() {
        let mut row = snapshot("snap-2", 1_789_000_000_000, None, &[]);
        row.session_id = None;
        row.source = "line\nbreak".to_string();
        assert_eq!(window_snapshot_storage_bytes(&row), serialized_reference(&row));
    }

    #[test]
    fn focus_span_shows_windows_from_its_last_snapshot() {
        let one = vec![window("Ghostty", 1)];
        let two = vec![window("Ghostty", 1), window("Safari", 2)];
        let snapshots = vec![
            snapshot("a1", 1_000, Some("Ghostty"), &one),
            snapshot("a2", 2_000, Some("Ghostty"), &two),
            snapshot("b1", 3_000, Some("Safari"), &one),
        ];
        let sessions: Vec<SessionRow> = Vec::new();
        let rail = build_focus_rail(&snapshots, &sessions, 10_000);

        assert_eq!(rail.slices.len(), 2);
        assert_eq!(rail.slices[0].row_count, 2);
        assert_eq!(rail.slices[0].visible_windows.len(), 2, "must reflect the later snapshot");
        assert_eq!(rail.slices[1].visible_windows.len(), 1);
        assert_eq!(
            rail.slices[0].storage_bytes,
            window_snapshot_storage_bytes(&snapshots[0]) + window_snapshot_storage_bytes(&snapshots[1]),
        );
    }
}
