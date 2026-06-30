async fn collect_attention_detail_payloads(
    db: &Arc<Database>,
    slice: &ContextSlice,
    slice_id: &str,
    state: &mut SliceDetailPayloadState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let snapshots = gaze::get_attention_snapshots(
        db,
        slice.start_timestamp.saturating_sub(60_000),
        slice.end_timestamp.saturating_add(60_000),
        None,
        slice.session_id.clone(),
    )
    .await?;

    if let Some(snapshot) = snapshots.iter().find(|item| item.attention_snapshot_id == slice_id) {
        state.raw_payloads.push(RawPayloadDto {
            label: "Attention snapshot".to_string(),
            raw_json: pretty_json(serde_json::to_value(snapshot)?),
        });
        let episode = gaze::get_attention_at_timestamp(db, snapshot.timestamp).await?;
        state.raw_payloads.push(RawPayloadDto {
            label: "Attention episode".to_string(),
            raw_json: pretty_json(serde_json::to_value(episode)?),
        });
        if let Some(scene) = ocr_agent_context::get_scene_snapshots(
            db,
            snapshot.timestamp.saturating_sub(5_000),
            snapshot.timestamp.saturating_add(5_000),
            snapshot.frontmost_app_name.clone(),
        )
        .await?
        .into_iter()
        .min_by_key(|scene| (scene.timestamp - snapshot.timestamp).abs())
        {
            if let Some(frame_path) = scene.frame_path {
                state.linked_file_paths.push(frame_path);
            }
        }
        return Ok(());
    }

    let spans = gaze::get_attention_spans(
        db,
        slice.start_timestamp.saturating_sub(60_000),
        slice.end_timestamp.saturating_add(60_000),
        None,
    )
    .await?;
    if let Some(span) = spans.into_iter().find(|item| item.attention_span_id == slice_id) {
        state.raw_payloads.push(RawPayloadDto {
            label: "Attention span".to_string(),
            raw_json: pretty_json(serde_json::to_value(&span)?),
        });
        let supporting = snapshots
            .into_iter()
            .filter(|snapshot| {
                span.supporting_attention_snapshot_ids
                    .iter()
                    .any(|id| id == &snapshot.attention_snapshot_id)
            })
            .collect::<Vec<_>>();
        if !supporting.is_empty() {
            state.raw_payloads.push(RawPayloadDto {
                label: "Supporting attention snapshots".to_string(),
                raw_json: pretty_json(serde_json::to_value(&supporting)?),
            });
        }
    }

    Ok(())
}
