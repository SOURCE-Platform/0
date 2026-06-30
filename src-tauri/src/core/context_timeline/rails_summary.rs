fn build_summary(
    focus_rail: &TimelineRailDto,
    visible_rail: &TimelineRailDto,
    interaction_rail: &TimelineRailDto,
    ocr_rail: &TimelineRailDto,
    evidence_rail: &TimelineRailDto,
) -> TimelineSummaryDto {
    let mut focused_apps = HashMap::new();
    let mut visible_apps = HashMap::new();
    let mut interaction = InteractionSummaryDto::default();

    let total_focus_time_ms = focus_rail
        .slices
        .iter()
        .map(|slice| {
            if let Some(app) = slice.app_name.as_ref() {
                focused_apps.insert(app.clone(), true);
            }
            slice.end_timestamp - slice.start_timestamp
        })
        .sum();

    let total_visible_time_ms = visible_rail
        .slices
        .iter()
        .map(|slice| {
            for window in &slice.visible_windows {
                visible_apps.insert(window.app_name.clone(), true);
            }
            slice.end_timestamp - slice.start_timestamp
        })
        .sum();

    for slice in &interaction_rail.slices {
        let duration = slice.end_timestamp - slice.start_timestamp;
        match slice.interaction_state.as_deref() {
            Some("active_typing") => interaction.active_typing_ms += duration,
            Some("active_pointer") => interaction.active_pointer_ms += duration,
            Some("voice_input_inferred") => interaction.voice_input_inferred_ms += duration,
            Some("mixed") => interaction.mixed_ms += duration,
            _ => interaction.passive_viewing_ms += duration,
        }
    }

    let total_interaction_time_ms = interaction.active_typing_ms
        + interaction.active_pointer_ms
        + interaction.voice_input_inferred_ms
        + interaction.mixed_ms
        + interaction.passive_viewing_ms;

    TimelineSummaryDto {
        ocr_block_count: ocr_rail.slices.len(),
        evidence_frame_count: evidence_rail.slices.len(),
        interaction,
        app_metrics: AppMetricSummaryDto {
            focused_app_count: focused_apps.len(),
            visible_app_count: visible_apps.len(),
            total_focus_time_ms,
            total_visible_time_ms,
            total_interaction_time_ms,
        },
    }
}
