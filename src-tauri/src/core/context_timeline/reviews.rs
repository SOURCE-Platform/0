pub async fn get_app_usage_overview(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<AppUsageOverviewDto, Box<dyn std::error::Error + Send + Sync>> {
    let sessions = get_sessions(db, start_timestamp, end_timestamp).await?;
    let snapshots = get_window_snapshots(db, start_timestamp, end_timestamp).await?;
    let keyboard = get_keyboard_events(db, start_timestamp, end_timestamp).await?;
    let mouse = get_mouse_events(db, start_timestamp, end_timestamp).await?;
    let ocr_rows = get_ocr_rows(db, start_timestamp, end_timestamp).await?;

    let mut items: HashMap<String, AppUsageOverviewItemDto> = HashMap::new();
    let focus_rail = build_focus_rail(&snapshots, &sessions, end_timestamp);
    for slice in &focus_rail.slices {
        let app_name = slice
            .app_name
            .clone()
            .unwrap_or_else(|| "Unknown".to_string());
        let entry = items
            .entry(app_name.clone())
            .or_insert(AppUsageOverviewItemDto {
                app_name: app_name.clone(),
                bundle_id: String::new(),
                focused_time_ms: 0,
                visible_time_ms: 0,
                interaction_time_ms: 0,
                ocr_hit_count: 0,
                recent_segment_count: 0,
            });
        entry.focused_time_ms += slice.end_timestamp - slice.start_timestamp;
        entry.recent_segment_count += 1;
    }

    for (index, snapshot) in snapshots.iter().enumerate() {
        let visible = parse_visible_windows(snapshot)?;
        let span = snapshots
            .get(index + 1)
            .map(|next| (next.timestamp - snapshot.timestamp).max(1))
            .unwrap_or(DEFAULT_SNAPSHOT_SPAN_MS);
        for window in visible {
            let entry = items
                .entry(window.app_name.clone())
                .or_insert(AppUsageOverviewItemDto {
                    app_name: window.app_name.clone(),
                    bundle_id: window.bundle_id.clone(),
                    focused_time_ms: 0,
                    visible_time_ms: 0,
                    interaction_time_ms: 0,
                    ocr_hit_count: 0,
                    recent_segment_count: 0,
                });
            entry.visible_time_ms += span;
            if entry.bundle_id.is_empty() {
                entry.bundle_id = window.bundle_id;
            }
        }
    }

    for row in keyboard {
        let entry = items
            .entry(row.app_name.clone())
            .or_insert(AppUsageOverviewItemDto {
                app_name: row.app_name.clone(),
                bundle_id: String::new(),
                focused_time_ms: 0,
                visible_time_ms: 0,
                interaction_time_ms: 0,
                ocr_hit_count: 0,
                recent_segment_count: 0,
            });
        entry.interaction_time_ms += 1_000;
    }
    for row in mouse {
        let entry = items
            .entry(row.app_name.clone())
            .or_insert(AppUsageOverviewItemDto {
                app_name: row.app_name.clone(),
                bundle_id: String::new(),
                focused_time_ms: 0,
                visible_time_ms: 0,
                interaction_time_ms: 0,
                ocr_hit_count: 0,
                recent_segment_count: 0,
            });
        entry.interaction_time_ms += 1_000;
    }

    for row in ocr_rows {
        let context = infer_app_context(db, row.timestamp, Some(&row.session_id)).await?;
        if let Some(app_name) = context.app_name {
            let entry = items
                .entry(app_name.clone())
                .or_insert(AppUsageOverviewItemDto {
                    app_name,
                    bundle_id: context.bundle_id.unwrap_or_default(),
                    focused_time_ms: 0,
                    visible_time_ms: 0,
                    interaction_time_ms: 0,
                    ocr_hit_count: 0,
                    recent_segment_count: 0,
                });
            entry.ocr_hit_count += 1;
        }
    }

    let mut items = items.into_values().collect::<Vec<_>>();
    items.sort_by(|a, b| {
        (b.focused_time_ms + b.visible_time_ms + b.interaction_time_ms)
            .cmp(&(a.focused_time_ms + a.visible_time_ms + a.interaction_time_ms))
    });

    Ok(AppUsageOverviewDto { items })
}

pub async fn get_pii_review(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
    app_filter: Option<String>,
    entity_type_filter: Option<String>,
    confidence_threshold: Option<f32>,
) -> Result<Vec<PiiEntityDto>, Box<dyn std::error::Error + Send + Sync>> {
    let ocr_rows = get_ocr_rows(db, start_timestamp, end_timestamp).await?;
    let mut items = Vec::new();

    for row in ocr_rows {
        let context = infer_app_context(db, row.timestamp, Some(&row.session_id)).await?;
        let app_name = context.app_name.clone();
        let entities = detect_pii_entities(&row, &context);
        for entity in entities {
            let matches_app = app_filter
                .as_ref()
                .map(|filter| {
                    app_name
                        .as_ref()
                        .map(|name| name == filter)
                        .unwrap_or(false)
                })
                .unwrap_or(true);
            let matches_entity_type = entity_type_filter
                .as_ref()
                .map(|filter| entity.entity_type == *filter)
                .unwrap_or(true);
            let matches_confidence = confidence_threshold
                .map(|threshold| entity.confidence >= threshold)
                .unwrap_or(true);
            if matches_app && matches_entity_type && matches_confidence {
                items.push(entity);
            }
        }
    }

    items.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(items)
}

pub async fn get_ocr_review(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
    app_filter: Option<String>,
    query: Option<String>,
    pii_only: bool,
) -> Result<Vec<OcrReviewItemDto>, Box<dyn std::error::Error + Send + Sync>> {
    let ocr_rows = get_ocr_rows(db, start_timestamp, end_timestamp).await?;
    let mut items = Vec::new();

    for row in ocr_rows {
        let context = infer_app_context(db, row.timestamp, Some(&row.session_id)).await?;
        let pii_entities = detect_pii_entities(&row, &context);
        if pii_only && pii_entities.is_empty() {
            continue;
        }

        if let Some(filter) = app_filter.as_ref() {
            if context.app_name.as_ref() != Some(filter) {
                continue;
            }
        }

        if let Some(search) = query.as_ref() {
            if !row.text.to_lowercase().contains(&search.to_lowercase()) {
                continue;
            }
        }

        items.push(OcrReviewItemDto {
            id: row.id.clone(),
            timestamp: row.timestamp,
            app_name: context.app_name,
            window_title: context.window_title,
            text: row.text,
            confidence: row.confidence as f32,
            frame_path: row.frame_path,
            pii_entities,
        });
    }

    items.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(items)
}

pub async fn get_last_event_time_for_table(
    db: &Arc<Database>,
    query: &str,
) -> Result<Option<i64>, Box<dyn std::error::Error + Send + Sync>> {
    let value = sqlx::query_scalar::<_, Option<i64>>(query)
        .fetch_one(db.pool())
        .await?;
    Ok(value)
}

pub async fn get_count_for_query(
    db: &Arc<Database>,
    query: &str,
) -> Result<i64, Box<dyn std::error::Error + Send + Sync>> {
    let value = sqlx::query_scalar::<_, i64>(query)
        .fetch_one(db.pool())
        .await?;
    Ok(value)
}
