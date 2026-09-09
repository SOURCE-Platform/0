use super::mappers::asr_segment_row_to_dto;
use super::types::{AsrSegmentDto, AsrSegmentRow};
use super::MultimodalQueryResult;
use crate::core::database::Database;
use std::sync::Arc;

pub async fn get_asr_segment(
    db: &Arc<Database>,
    asr_segment_id: &str,
) -> MultimodalQueryResult<Option<AsrSegmentDto>> {
    let row = sqlx::query_as::<_, AsrSegmentRow>(
        "SELECT * FROM asr_segments WHERE asr_segment_id = ? LIMIT 1",
    )
    .bind(asr_segment_id)
    .fetch_optional(db.pool())
    .await?;
    Ok(row.map(asr_segment_row_to_dto))
}
