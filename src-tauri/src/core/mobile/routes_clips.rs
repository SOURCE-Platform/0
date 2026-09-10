use super::ingest::{discard_spool, ingest_clip_spooled, mobile_clip_path};
use super::server::{bearer_device, current_session_id, MobileState};
use super::types::ClipUploadMeta;
use axum::extract::multipart::{Field, MultipartError};
use axum::extract::{Multipart, Query, State};
use axum::http::{HeaderMap, StatusCode};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;

/// Largest clip accepted in one upload: 1 GiB, about 5.5 hours of 16 kHz mono.
///
/// Left unset, axum caps request bodies at 2 MB — roughly one minute of audio —
/// so every longer recording was refused and the phone retried it forever.
pub const MAX_CLIP_BYTES: usize = 1024 * 1024 * 1024;

type Rejection = (StatusCode, String);

/// Accept a recorded clip from the phone.
///
/// Audio is streamed to disk as it arrives rather than buffered, so a long
/// recording never has to fit in memory.
pub async fn upload_clip(
    State(state): State<MobileState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    mut multipart: Multipart,
) -> Result<StatusCode, Rejection> {
    let Some(device) = bearer_device(&state, &headers, &query).await else {
        return Err((StatusCode::UNAUTHORIZED, "Unauthorized.".to_string()));
    };
    state.pairing.touch(&device.device_id).await;

    let mut meta: Option<ClipUploadMeta> = None;
    let mut spooled: Option<(PathBuf, u64)> = None;
    if let Err(rejection) = read_parts(&mut multipart, &mut meta, &mut spooled).await {
        if let Some((path, _)) = &spooled {
            discard_spool(path);
        }
        eprintln!("Mobile clip upload rejected: {}", rejection.1);
        return Err(rejection);
    }
    let Some((spool_path, bytes)) = spooled else {
        return Err((StatusCode::BAD_REQUEST, "Upload had no audio file.".to_string()));
    };
    let meta = match meta {
        Some(meta) if !meta.clip_id.is_empty() && meta.clip_id.len() <= 128 => meta,
        _ => {
            discard_spool(&spool_path);
            return Err((
                StatusCode::BAD_REQUEST,
                "Upload had no valid clip metadata.".to_string(),
            ));
        }
    };

    let session_id = current_session_id(&state).await;
    let commands = state.commands.lock().await.clone();
    let stored = ingest_clip_spooled(
        &state.db,
        &commands,
        &session_id,
        &device,
        &meta.clip_id,
        meta.started_at_ms,
        meta.ended_at_ms,
        &spool_path,
        bytes,
    )
    .await;
    match stored {
        Ok(_) => {
            println!("Mobile clip {} received ({bytes} bytes)", meta.clip_id);
            Ok(StatusCode::CREATED)
        }
        Err(error) => {
            discard_spool(&spool_path);
            eprintln!("Mobile clip {} could not be stored: {error}", meta.clip_id);
            Err((StatusCode::UNPROCESSABLE_ENTITY, error))
        }
    }
}

/// Walk the multipart body, returning errors rather than swallowing them.
///
/// A failed read used to be treated as "no file attached", which is exactly
/// how the size limit stayed invisible.
async fn read_parts(
    multipart: &mut Multipart,
    meta: &mut Option<ClipUploadMeta>,
    spooled: &mut Option<(PathBuf, u64)>,
) -> Result<(), Rejection> {
    while let Some(field) = multipart.next_field().await.map_err(reject)? {
        let name = field.name().unwrap_or_default().to_string();
        match name.as_str() {
            "meta" => {
                let text = field.text().await.map_err(reject)?;
                let parsed = serde_json::from_str(&text).map_err(|error| {
                    (StatusCode::BAD_REQUEST, format!("Bad clip metadata: {error}"))
                })?;
                *meta = Some(parsed);
            }
            "file" | "audio" => {
                if let Some((previous, _)) = spooled.take() {
                    discard_spool(&previous);
                }
                *spooled = Some(spool_field(field).await?);
            }
            _ => {}
        }
    }
    Ok(())
}

/// Stream one file part to a uniquely named spool file beside the final clip,
/// so moving it into place afterwards is a same-filesystem rename.
async fn spool_field(mut field: Field<'_>) -> Result<(PathBuf, u64), Rejection> {
    let path = mobile_clip_path(&format!("incoming-{}", uuid::Uuid::new_v4()))
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(disk_error)?;
    }
    let mut file = tokio::fs::File::create(&path).await.map_err(disk_error)?;
    let mut written: u64 = 0;
    let copied: Result<(), Rejection> = async {
        while let Some(chunk) = field.chunk().await.map_err(reject)? {
            file.write_all(&chunk).await.map_err(disk_error)?;
            written += chunk.len() as u64;
        }
        file.flush().await.map_err(disk_error)
    }
    .await;
    drop(file);
    match copied {
        Ok(()) => Ok((path, written)),
        Err(rejection) => {
            discard_spool(&path);
            Err(rejection)
        }
    }
}

fn reject(error: MultipartError) -> Rejection {
    (error.status(), error.body_text())
}

fn disk_error(error: std::io::Error) -> Rejection {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("Couldn't write the clip to disk: {error}"),
    )
}
