use std::sync::Arc;

use crate::core::database::Database;

pub(super) async fn init_schema(
    db: &Arc<Database>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let pool = db.pool();

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS keyboard_events (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            event_type TEXT NOT NULL,
            key_code INTEGER NOT NULL,
            key_char TEXT,
            modifiers TEXT NOT NULL,
            app_name TEXT NOT NULL,
            window_title TEXT NOT NULL,
            process_id INTEGER NOT NULL,
            ui_element TEXT,
            FOREIGN KEY (session_id) REFERENCES sessions(id)
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_keyboard_session ON keyboard_events(session_id)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_keyboard_timestamp ON keyboard_events(timestamp)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_keyboard_app ON keyboard_events(app_name)")
        .execute(pool)
        .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS mouse_events (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            event_type TEXT NOT NULL,
            position_x INTEGER NOT NULL,
            position_y INTEGER NOT NULL,
            app_name TEXT NOT NULL,
            window_title TEXT NOT NULL,
            process_id INTEGER NOT NULL,
            ui_element TEXT,
            FOREIGN KEY (session_id) REFERENCES sessions(id)
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_mouse_session ON mouse_events(session_id)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_mouse_timestamp ON mouse_events(timestamp)")
        .execute(pool)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_mouse_position ON mouse_events(position_x, position_y)",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS commands (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            shortcut TEXT NOT NULL,
            command_type TEXT NOT NULL,
            app_name TEXT NOT NULL,
            description TEXT NOT NULL,
            FOREIGN KEY (session_id) REFERENCES sessions(id)
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_commands_session ON commands(session_id)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_commands_timestamp ON commands(timestamp)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_commands_shortcut ON commands(shortcut)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_commands_app ON commands(app_name)")
        .execute(pool)
        .await?;

    Ok(())
}
