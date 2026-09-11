use super::ingest::{discard_spool, ingest_clip_spooled, mobile_clip_path};
use super::server::{bearer_device, current_session_id, MobileState};
use super::types::{ClipStatus, ClipStatusResponse, ClipUploadMeta};
use crate::core::multimodal::MOBILE_SOURCE_ID;
use axum::extract::multipart::{Field, MultipartError};
use axum::extract::{Multipart, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Json;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;

/// Largest clip accepted in one upload: 4 GiB. That is about 37 hours of a
/// voice memo (16 kHz, 16-bit) or 8 hours at high quality (48 kHz, 24-bit).
///
/// Left unset, axum caps request bodies at 2 MB — roughly one minute of audio —
/// so every longer recording was refused and the phone retried it forever.
/// Uploads stream to disk, so the size of this limit costs no memory.
pub const MAX_CLIP_BYTES: usize = 4 * 1024 * 1024 * 1024;

type Rejection = (StatusCode, String);

/// Characters of transcript shown under a recording on the phone.
const PREVIEW_CHARS: usize = 160;
/// Most recordings a single status request may ask about.
const MAX_STATUS_IDS: usize = 50;

/// Tell the phone which of its recordings Source has, and which are transcribed.
pub async fn clip_status(
    State(state): State<MobileState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<ClipStatusResponse>, Rejection> {
    let Some(device) = bearer_device(&state, &headers, &query).await else {
        return Err((StatusCode::UNAUTHORIZED, "Unauthorized.".to_string()));
    };
    state.pairing.touch(&device.device_id).await;
    let ids = parse_clip_ids(query.get("ids").map(String::as_str).unwrap_or(""));
    let mut clips = Vec::with_capacity(ids.len());
    for clip_id in ids {
        clips.push(status_for(&state, &clip_id).await?);
    }
    Ok(Json(ClipStatusResponse { clips }))
}

async fn status_for(state: &MobileState, clip_id: &str) -> Result<ClipStatus, Rejection> {
    let received = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM mobile_clips WHERE clip_id = ?")
        .bind(clip_id)
        .fetch_one(state.db.pool())
        .await
        .map_err(database_error)?
        > 0;
    let row: Option<(Option<String>, i64)> = sqlx::query_as(
        "SELECT transcript, is_final FROM asr_segments WHERE asr_segment_id = ? AND source_id = ?",
    )
    .bind(clip_id)
    .bind(MOBILE_SOURCE_ID)
    .fetch_optional(state.db.pool())
    .await
    .map_err(database_error)?;
    let finished = row
        .and_then(|(text, is_final)| text.filter(|text| is_final == 1 && !text.trim().is_empty()));
    Ok(ClipStatus {
        clip_id: clip_id.to_string(),
        received,
        transcribed: finished.is_some(),
        preview: finished.map(|text| preview_of(&text, PREVIEW_CHARS)),
    })
}

/// Clip ids from `?ids=a,b,c`: trimmed, de-duplicated, capped in number, and
/// limited to the characters clip ids are made of.
fn parse_clip_ids(raw: &str) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for id in raw.split(',').map(str::trim) {
        let valid = !id.is_empty()
            && id.len() <= 128
            && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if valid && !ids.iter().any(|seen| seen == id) {
            ids.push(id.to_string());
        }
        if ids.len() == MAX_STATUS_IDS {
            break;
        }
    }
    ids
}

/// The first `limit` characters of a transcript, cut on a character boundary.
/// Cutting by bytes would panic partway through a multi-byte character.
fn preview_of(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    match trimmed.char_indices().nth(limit) {
        Some((cut, _)) => format!("{}…", trimmed[..cut].trim_end()),
        None => trimmed.to_string(),
    }
}

fn database_error(error: sqlx::Error) -> Rejection {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("Couldn't read clip status: {error}"),
    )
}

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

#[cfg(test)]
mod clip_status_tests {
    use super::*;

    #[test]
    fn clip_ids_are_trimmed_deduplicated_and_validated() {
        assert_eq!(
            parse_clip_ids(" a-1 ,b_2,a-1,,bad id,../etc,"),
            vec!["a-1".to_string(), "b_2".to_string()]
        );
    }

    #[test]
    fn clip_id_list_is_capped() {
        let raw = (0..80).map(|n| format!("clip-{n}")).collect::<Vec<_>>().join(",");
        assert_eq!(parse_clip_ids(&raw).len(), MAX_STATUS_IDS);
    }

    #[test]
    fn preview_cuts_on_a_character_boundary() {
        let text = "é".repeat(200);
        let preview = preview_of(&text, 160);
        assert_eq!(preview.chars().count(), 161, "160 characters plus the ellipsis");
        assert!(preview.ends_with('…'));
        assert_eq!(preview_of("  short  ", 160), "short");
    }
}
