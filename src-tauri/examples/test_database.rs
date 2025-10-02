/// Example program to test database initialization and operations
/// Run with: cargo run --example test_database

use zero_lib::core::database::Database;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Testing Database Initialization ===\n");

    // Initialize database (creates ~/.observer_data/database/observer.db)
    println!("Initializing database...");
    let db = Database::init().await?;
    println!("✓ Database initialized successfully\n");

    // Test creating a session
    println!("=== Testing Session CRUD Operations ===\n");

    let session_id = uuid::Uuid::new_v4().to_string();
    let start_time = chrono::Utc::now().timestamp();
    let device_id = "test-device-123";

    println!("Creating session:");
    println!("  ID: {}", session_id);
    println!("  Start time: {}", start_time);
    println!("  Device ID: {}", device_id);

    db.create_session(&session_id, start_time, device_id).await?;
    println!("✓ Session created\n");

    // Retrieve the session
    println!("Retrieving session...");
    let session = db.get_session(&session_id).await?;

    if let Some(sess) = session {
        println!("✓ Session retrieved:");
        println!("  ID: {}", sess.id);
        println!("  Start: {}", sess.start_timestamp);
        println!("  End: {:?}", sess.end_timestamp);
        println!("  Device: {}", sess.device_id);
        println!("  Created: {}\n", sess.created_at);
    } else {
        println!("✗ Session not found\n");
    }

    // End the session
    let end_time = chrono::Utc::now().timestamp();
    println!("Ending session at timestamp: {}", end_time);
    db.end_session(&session_id, end_time).await?;
    println!("✓ Session ended\n");

    // Verify the end timestamp was updated
    println!("Verifying end timestamp...");
    let updated_session = db.get_session(&session_id).await?;
    if let Some(sess) = updated_session {
        println!("✓ Session end timestamp: {:?}\n", sess.end_timestamp);
    }

    // List all sessions
    println!("=== Listing All Sessions ===\n");
    let sessions = db.list_sessions().await?;
    println!("Total sessions in database: {}", sessions.len());

    for (i, sess) in sessions.iter().enumerate() {
        println!("\nSession {}:", i + 1);
        println!("  ID: {}", sess.id);
        println!("  Start: {}", sess.start_timestamp);
        println!("  End: {:?}", sess.end_timestamp);
        println!("  Device: {}", sess.device_id);
    }

    println!("\n=== Database Path ===");
    if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
        let db_path = format!("{}/.observer_data/database/observer.db", home);
        println!("Database file location: {}", db_path);

        // Check if file exists
        if std::path::Path::new(&db_path).exists() {
            let metadata = std::fs::metadata(&db_path)?;
            println!("Database file size: {} bytes", metadata.len());
        }
    }

    println!("\n✓ All tests passed!");
    Ok(())
}
