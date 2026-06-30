use crate::app::state::{AppState, AppUsageSegment, DateRange, TimelineData, TimelineSession};
use crate::core::database::Database;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub async fn get_timeline_data(
    start_timestamp: i64,
    end_timestamp: i64,
    state: State<'_, AppState>,
) -> Result<TimelineData, String> {
    let manager = state
        .session_manager
        .as_ref()
        .ok_or("Session manager not initialized")?;
    let sessions = manager
        .get_sessions_in_range(start_timestamp, end_timestamp)
        .await
        .map_err(|e| format!("Failed to get sessions: {}", e))?;

    let mut timeline_sessions = Vec::new();
    for session in &sessions {
        let apps = get_app_usage_for_session(&state.db, &session.id).await?;
        let app_segments: Vec<AppUsageSegment> = apps
            .into_iter()
            .map(|app| AppUsageSegment {
                app_name: app.app_name.clone(),
                bundle_id: app.bundle_id.clone(),
                start_timestamp: app.start_timestamp,
                end_timestamp: app
                    .end_timestamp
                    .unwrap_or(chrono::Utc::now().timestamp_millis()),
                focus_duration: app.focus_duration_ms,
                color: app_color(&app.app_name),
            })
            .collect();

        timeline_sessions.push(TimelineSession {
            id: session.id.clone(),
            start_timestamp: session.start_timestamp,
            end_timestamp: session.end_timestamp,
            session_type: session.session_type.clone(),
            activity_intensity: calculate_activity_intensity(&app_segments),
            has_screen_recording: check_has_screen_recording(&state.db, &session.id).await?,
            has_input_recording: check_has_input_recording(&state.db, &session.id).await?,
            applications: app_segments,
        });
    }

    let total_duration: u64 = timeline_sessions
        .iter()
        .map(|session| {
            let end = session
                .end_timestamp
                .unwrap_or(chrono::Utc::now().timestamp_millis());
            (end - session.start_timestamp) as u64
        })
        .sum();

    Ok(TimelineData {
        sessions: timeline_sessions,
        total_duration,
        date_range: DateRange {
            start: start_timestamp,
            end: end_timestamp,
        },
    })
}

async fn get_app_usage_for_session(
    db: &Arc<Database>,
    session_id: &str,
) -> Result<Vec<crate::core::os_activity::AppUsage>, String> {
    sqlx::query_as::<_, crate::core::os_activity::AppUsage>(
        r#"
        SELECT id, session_id, app_name, bundle_id, process_id,
               start_timestamp, end_timestamp, focus_duration_ms, background_duration_ms
        FROM app_usage
        WHERE session_id = ?
        ORDER BY start_timestamp ASC
        "#,
    )
    .bind(session_id)
    .fetch_all(&db.pool)
    .await
    .map_err(|e| format!("Failed to get app usage: {}", e))
}

async fn check_has_screen_recording(db: &Arc<Database>, session_id: &str) -> Result<bool, String> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM screen_recordings WHERE session_id = ?")
            .bind(session_id)
            .fetch_one(&db.pool)
            .await
            .map_err(|e| format!("Failed to check screen recording: {}", e))?;
    Ok(count > 0)
}

async fn check_has_input_recording(db: &Arc<Database>, session_id: &str) -> Result<bool, String> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM keyboard_events WHERE session_id = ? LIMIT 1")
            .bind(session_id)
            .fetch_one(&db.pool)
            .await
            .map_err(|e| format!("Failed to check input recording: {}", e))?;
    Ok(count > 0)
}

fn app_color(app_name: &str) -> String {
    let mut hasher = DefaultHasher::new();
    app_name.hash(&mut hasher);
    let hue = (hasher.finish() % 360) as f32;
    format!("hsl({}, 70%, 60%)", hue)
}

fn calculate_activity_intensity(apps: &[AppUsageSegment]) -> f32 {
    (apps.len() as f32 / 20.0).min(1.0)
}
