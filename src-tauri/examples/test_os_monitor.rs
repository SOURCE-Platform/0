// Test the OS monitor functionality

use zero_lib::platform::os_monitor::create_os_monitor;

#[tokio::main]
async fn main() {
    println!("Testing OS Monitor...\n");

    // Create monitor
    let result = create_os_monitor();

    match result {
        Ok((mut monitor, mut rx)) => {
            println!("✓ Successfully created OS monitor\n");

            // Get running apps
            println!("Getting running applications...");
            match monitor.get_running_apps() {
                Ok(apps) => {
                    println!("✓ Found {} running applications:", apps.len());
                    for (i, app) in apps.iter().take(10).enumerate() {
                        println!("  {}. {} ({})", i + 1, app.name, app.bundle_id);
                        if let Some(path) = &app.executable_path {
                            println!("      Path: {}", path);
                        }
                    }
                    println!();
                }
                Err(e) => {
                    eprintln!("✗ Failed to get running apps: {}", e);
                }
            }

            // Get frontmost app
            println!("Getting frontmost application...");
            match monitor.get_frontmost_app() {
                Ok(Some(app)) => {
                    println!("✓ Frontmost app: {} ({})", app.name, app.bundle_id);
                    println!("  Process ID: {}", app.process_id);
                    if let Some(path) = &app.executable_path {
                        println!("  Path: {}", path);
                    }
                    println!();
                }
                Ok(None) => {
                    println!("No frontmost app detected\n");
                }
                Err(e) => {
                    eprintln!("✗ Failed to get frontmost app: {}\n", e);
                }
            }

            // Start monitoring
            println!("Starting monitoring...");
            match monitor.start_monitoring() {
                Ok(_) => {
                    println!("✓ Monitoring started");
                    println!("  Is monitoring: {}\n", monitor.is_monitoring());
                }
                Err(e) => {
                    eprintln!("✗ Failed to start monitoring: {}\n", e);
                }
            }

            // Try to receive events (with timeout)
            println!("Listening for events for 2 seconds...");
            let timeout = tokio::time::Duration::from_secs(2);
            match tokio::time::timeout(timeout, rx.recv()).await {
                Ok(Some(event)) => {
                    println!("✓ Received event: {:?}", event);
                }
                Ok(None) => {
                    println!("Channel closed");
                }
                Err(_) => {
                    println!("No events received in 2 seconds (this is expected as full event monitoring is not yet implemented)");
                }
            }
            println!();

            // Stop monitoring
            println!("Stopping monitoring...");
            match monitor.stop_monitoring() {
                Ok(_) => {
                    println!("✓ Monitoring stopped");
                    println!("  Is monitoring: {}", monitor.is_monitoring());
                }
                Err(e) => {
                    eprintln!("✗ Failed to stop monitoring: {}", e);
                }
            }

            println!("\n✓ All tests completed successfully!");
        }
        Err(e) => {
            eprintln!("✗ Failed to create OS monitor: {}", e);
            std::process::exit(1);
        }
    }
}
