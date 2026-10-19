use source_banking::{BankingOverview, BankingService, TransactionPage, TransactionQuery};
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

fn require_main(window: &tauri::WebviewWindow) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("Banking is only available in the main SOURCE window.".into())
    }
}

#[tauri::command]
pub async fn banking_overview(
    window: tauri::WebviewWindow,
    service: State<'_, BankingService>,
) -> Result<BankingOverview, String> {
    require_main(&window)?;
    service.overview().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn banking_configure(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    service: State<'_, BankingService>,
    application_id: String,
) -> Result<BankingOverview, String> {
    require_main(&window)?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Select your Enable Banking RSA private key")
        .add_filter("RSA PEM private key", &["pem", "key"])
        .pick_file(move |file| {
            let _ = tx.send(file);
        });
    let file = rx
        .await
        .map_err(|_| "Private-key selection was cancelled.")?
        .ok_or("Private-key selection was cancelled.")?;
    let path = file
        .into_path()
        .map_err(|_| "Select a local PEM private-key file.")?;
    service
        .configure(application_id, &path)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn banking_connect(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    service: State<'_, BankingService>,
) -> Result<BankingOverview, String> {
    require_main(&window)?;
    // Capture suppression survives tab navigation throughout the external bank login.
    let _guard = crate::core::banking_capture::BankAuthorizationGuard::new();
    service
        .connect(|url| {
            app.opener()
                .open_url(url, None::<&str>)
                .map_err(|_| source_banking::Error::Browser)
        })
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn banking_cancel_connect(
    window: tauri::WebviewWindow,
    service: State<'_, BankingService>,
) -> Result<(), String> {
    require_main(&window)?;
    service.cancel_connect();
    Ok(())
}

#[tauri::command]
pub async fn banking_sync(
    window: tauri::WebviewWindow,
    service: State<'_, BankingService>,
) -> Result<BankingOverview, String> {
    require_main(&window)?;
    service.sync(false).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn banking_set_auto_sync(
    window: tauri::WebviewWindow,
    service: State<'_, BankingService>,
    enabled: bool,
) -> Result<BankingOverview, String> {
    require_main(&window)?;
    service
        .set_auto_sync(enabled)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn banking_disconnect(
    window: tauri::WebviewWindow,
    service: State<'_, BankingService>,
) -> Result<BankingOverview, String> {
    require_main(&window)?;
    service.disconnect().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn banking_transactions(
    window: tauri::WebviewWindow,
    service: State<'_, BankingService>,
    query: TransactionQuery,
) -> Result<TransactionPage, String> {
    require_main(&window)?;
    service.transactions(query).await.map_err(|e| e.to_string())
}

pub fn start_background_sync(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut timer = tokio::time::interval(std::time::Duration::from_secs(3600));
        timer.tick().await;
        loop {
            timer.tick().await;
            let service = app.state::<BankingService>();
            service.background_tick().await;
        }
    });
}
