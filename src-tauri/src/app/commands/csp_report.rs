//! CSP violation reporting from the WebView.
//!
//! The frontend forwards `securitypolicyviolation` events here so policy
//! regressions surface in the app log instead of only the web inspector
//! console. Fields are truncated: a blocked URI can contain page data and
//! must never become an unbounded log channel.

fn crop(value: &str) -> String {
    const MAX: usize = 160;
    let mut out: String = value.chars().take(MAX).collect();
    if value.chars().count() > MAX {
        out.push('…');
    }
    out
}

#[tauri::command]
pub fn report_csp_violation(
    directive: String,
    blocked_uri: String,
    source_file: Option<String>,
) {
    eprintln!(
        "CSP VIOLATION: directive={} blocked={} source={}",
        crop(&directive),
        crop(&blocked_uri),
        source_file.as_deref().map(crop).unwrap_or_default(),
    );
}

/// Smoke-test beacon: the webview reports that it mounted plus basic render
/// metrics, proving scripts executed under the active CSP. Debug builds log
/// the full payload; release builds only log that the beacon fired.
#[tauri::command]
pub fn webview_smoke_event(kind: String, payload: String) {
    let kind = crop(&kind);
    if cfg!(debug_assertions) {
        eprintln!("SMOKE[{}]: {}", kind, crop_n(&payload, 2000));
    } else {
        eprintln!("SMOKE[{}]", kind);
    }
}

fn crop_n(value: &str, max: usize) -> String {
    let mut out: String = value.chars().take(max).collect();
    if value.chars().count() > max {
        out.push('…');
    }
    out
}

/// Debug builds only: hands the frontend a real file inside the asset scope
/// (recordings) and a probe file inside the future vault directory (outside
/// the scope), so a runtime check can prove the asset protocol allows media
/// and rejects vault paths. The probe PNG is written by us into our own
/// empty vault directory; it contains no user data.
#[tauri::command]
pub fn debug_asset_scope_probe() -> Result<serde_json::Value, String> {
    if !cfg!(debug_assertions) {
        return Err("debug builds only".into());
    }
    let home = std::env::var_os("HOME").ok_or("no HOME set")?;
    let data = std::path::PathBuf::from(home).join(".observer_data");

    let vault_dir = data.join("vault");
    std::fs::create_dir_all(&vault_dir).map_err(|e| e.to_string())?;
    let vault_probe = vault_dir.join("asset_scope_probe.png");
    // 1x1 transparent PNG, valid so a load failure means scope denial.
    const PNG_1X1: [u8; 67] = [
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
        0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
        0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00,
        0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x62, 0x00, 0x01, 0x00, 0x00,
        0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    std::fs::write(&vault_probe, PNG_1X1).map_err(|e| e.to_string())?;

    let mut image: Option<String> = None;
    let mut video: Option<String> = None;
    let mut stack = vec![data.join("recordings")];
    while let Some(dir) = stack.pop() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let s = || path.to_string_lossy().to_string();
                match ext.as_str() {
                    "png" | "jpg" | "jpeg" | "gif" | "webp" if image.is_none() => {
                        image = Some(s())
                    }
                    "mp4" | "mov" | "webm" if video.is_none() => video = Some(s()),
                    _ => {}
                }
            }
        }
        if image.is_some() && video.is_some() {
            break;
        }
    }

    Ok(serde_json::json!({
        "vault_probe": vault_probe.to_string_lossy(),
        "image_sample": image,
        "video_sample": video,
    }))
}
