// Full-text search engine for OCR results using FTS5

use crate::core::database::Database;
use crate::models::ocr::BoundingBox;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

// ==============================================================================
// Errors
// ==============================================================================

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Invalid UUID: {0}")]
    InvalidUuid(#[from] uuid::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid query: {0}")]
    InvalidQuery(String),
}

type Result<T> = std::result::Result<T, SearchError>;

// ==============================================================================
// Search Query
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    pub query: String,
    #[serde(default)]
    pub filters: SearchFilters,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

fn default_limit() -> u32 {
    50
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchFilters {
    pub session_ids: Option<Vec<Uuid>>,
    pub date_range: Option<TimeRange>,
    pub min_confidence: Option<f32>,
    pub app_names: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: i64,
    pub end: i64,
}

// ==============================================================================
// Search Results
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: String,
    pub session_id: Uuid,
    pub timestamp: i64,
    pub text_snippet: String,
    pub full_text: String,
    pub confidence: f32,
    pub bounding_box: BoundingBox,
    pub frame_path: Option<PathBuf>,
    pub app_context: Option<String>,
    pub relevance_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    pub results: Vec<SearchResult>,
    pub total_count: u32,
    pub query_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultWithContext {
    pub result: SearchResult,
    pub before_text: String,
    pub after_text: String,
}

// ==============================================================================
// Database Row Types
// ==============================================================================

#[derive(Debug, sqlx::FromRow)]
struct SearchResultRow {
    id: String,
    session_id: String,
    timestamp: i64,
    text: String,
    confidence: f64,
    bounding_box: String,
    app_context: Option<String>,
    rank: f64,
}

// ==============================================================================
// Search Engine
// ==============================================================================

pub struct SearchEngine {
    db: Arc<Database>,
}

impl SearchEngine {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Search OCR results using full-text search
    pub async fn search(&self, query: SearchQuery) -> Result<SearchResults> {
        let start_time = std::time::Instant::now();

        // Build FTS5 query
        let fts_query = self.build_fts_query(&query.query)?;

        // Build filter clauses
        let filter_clause = self.build_filter_clause(&query.filters)?;

        // Execute search
        let sql = format!(
            r#"
            SELECT
                o.id,
                o.session_id,
                o.timestamp,
                o.text,
                o.confidence,
                o.bounding_box,
                NULL as app_context,
                rank as rank
            FROM ocr_fts fts
            JOIN ocr_results o ON fts.rowid = o.rowid
            WHERE fts.text MATCH ?
            {}
            ORDER BY rank
            LIMIT ? OFFSET ?
            "#,
            filter_clause
        );

        let rows = sqlx::query_as::<_, SearchResultRow>(&sql)
            .bind(&fts_query)
            .bind(query.limit as i64)
            .bind(query.offset as i64)
            .fetch_all(self.db.pool())
            .await?;

        // Get total count
        let count_sql = format!(
            r#"
            SELECT COUNT(*) as count
            FROM ocr_fts fts
            JOIN ocr_results o ON fts.rowid = o.rowid
            WHERE fts.text MATCH ?
            {}
            "#,
            filter_clause
        );

        let total_count: i64 = sqlx::query_scalar(&count_sql)
            .bind(&fts_query)
            .fetch_one(self.db.pool())
            .await?;

        // Convert to SearchResult
        let search_results: Vec<SearchResult> = rows
            .into_iter()
            .map(|row| self.row_to_search_result(row, &query.query))
            .collect::<Result<Vec<_>>>()?;

        let query_time = start_time.elapsed();

        Ok(SearchResults {
            results: search_results,
            total_count: total_count as u32,
            query_time_ms: query_time.as_millis() as u64,
        })
    }

    /// Search with surrounding context
    pub async fn search_with_context(
        &self,
        query: SearchQuery,
        context_before_ms: i64,
        context_after_ms: i64,
    ) -> Result<Vec<SearchResultWithContext>> {
        let results = self.search(query).await?;
        let mut results_with_context = Vec::new();

        for result in results.results {
            // Get OCR results before and after this result
            let before = self
                .get_ocr_in_range(
                    result.session_id,
                    result.timestamp - context_before_ms,
                    result.timestamp,
                )
                .await?;

            let after = self
                .get_ocr_in_range(
                    result.session_id,
                    result.timestamp,
                    result.timestamp + context_after_ms,
                )
                .await?;

            results_with_context.push(SearchResultWithContext {
                result,
                before_text: before,
                after_text: after,
            });
        }

        Ok(results_with_context)
    }

    /// Get autocomplete suggestions
    pub async fn suggest_queries(&self, partial: &str) -> Result<Vec<String>> {
        if partial.len() < 2 {
            return Ok(vec![]);
        }

        let pattern = format!("{}%", partial);
        let suggestions = sqlx::query_scalar::<_, String>(
            r#"
            SELECT DISTINCT substr(text, 1, 100) as snippet
            FROM ocr_results
            WHERE text LIKE ?
            ORDER BY confidence DESC
            LIMIT 10
            "#,
        )
        .bind(pattern)
        .fetch_all(self.db.pool())
        .await?;

        Ok(suggestions)
    }

    /// Build FTS5 query string
    fn build_fts_query(&self, query: &str) -> Result<String> {
        if query.trim().is_empty() {
            return Err(SearchError::InvalidQuery("Query cannot be empty".to_string()));
        }

        // Sanitize query
        let sanitized = query
            .replace('"', "")
            .replace('\'', "")
            .trim()
            .to_string();

        // If query has multiple words, make it a phrase search
        if sanitized.contains(' ') {
            Ok(format!("\"{}\"", sanitized))
        } else {
            // Prefix search for single words
            Ok(format!("{}*", sanitized))
        }
    }

    /// Build SQL filter clause from filters
    fn build_filter_clause(&self, filters: &SearchFilters) -> Result<String> {
        let mut clauses = Vec::new();

        if let Some(ref session_ids) = filters.session_ids {
            let ids: Vec<String> = session_ids
                .iter()
                .map(|id| format!("'{}'", id))
                .collect();
            clauses.push(format!("o.session_id IN ({})", ids.join(", ")));
        }

        if let Some(ref range) = filters.date_range {
            clauses.push(format!(
                "o.timestamp BETWEEN {} AND {}",
                range.start, range.end
            ));
        }

        if let Some(min_conf) = filters.min_confidence {
            clauses.push(format!("o.confidence >= {}", min_conf));
        }

        if clauses.is_empty() {
            Ok(String::new())
        } else {
            Ok(format!("AND {}", clauses.join(" AND ")))
        }
    }

    /// Convert database row to SearchResult
    fn row_to_search_result(&self, row: SearchResultRow, query: &str) -> Result<SearchResult> {
        let snippet = self.generate_snippet(&row.text, query, 100);
        let bounding_box: BoundingBox = serde_json::from_str(&row.bounding_box)?;

        Ok(SearchResult {
            id: row.id,
            session_id: Uuid::parse_str(&row.session_id)?,
            timestamp: row.timestamp,
            text_snippet: snippet,
            full_text: row.text,
            confidence: row.confidence as f32,
            bounding_box,
            frame_path: None,
            app_context: row.app_context,
            relevance_score: -row.rank as f32, // FTS5 rank is negative
        })
    }

    /// Generate text snippet highlighting the query
    fn generate_snippet(&self, text: &str, query: &str, max_length: usize) -> String {
        // Remove quotes from phrase queries
        let query_clean = query.replace('"', "").replace('*', "");
        let query_lower = query_clean.to_lowercase();
        let text_lower = text.to_lowercase();

        if let Some(pos) = text_lower.find(&query_lower) {
            // Extract snippet around query
            let start = pos.saturating_sub(max_length / 2);
            let end = (pos + query_clean.len() + max_length / 2).min(text.len());
            let mut snippet = text[start..end].to_string();

            // Add ellipsis if truncated
            if start > 0 {
                snippet = format!("...{}", snippet);
            }
            if end < text.len() {
                snippet = format!("{}...", snippet);
            }

            snippet
        } else {
            // Query not found (shouldn't happen), return start of text
            let chars: String = text.chars().take(max_length).collect();
            if text.len() > max_length {
                format!("{}...", chars)
            } else {
                chars
            }
        }
    }

    /// Get OCR text in a time range
    async fn get_ocr_in_range(
        &self,
        session_id: Uuid,
        start: i64,
        end: i64,
    ) -> Result<String> {
        let texts = sqlx::query_scalar::<_, String>(
            r#"
            SELECT text FROM ocr_results
            WHERE session_id = ?
              AND timestamp >= ?
              AND timestamp <= ?
            ORDER BY timestamp ASC
            "#,
        )
        .bind(session_id.to_string())
        .bind(start)
        .bind(end)
        .fetch_all(self.db.pool())
        .await?;

        Ok(texts.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_query_default() {
        let query = SearchQuery {
            query: "test".to_string(),
            filters: SearchFilters::default(),
            limit: default_limit(),
            offset: 0,
        };

        assert_eq!(query.limit, 50);
        assert_eq!(query.offset, 0);
    }

    #[test]
    fn test_time_range() {
        let range = TimeRange {
            start: 1000,
            end: 2000,
        };

        assert_eq!(range.start, 1000);
        assert_eq!(range.end, 2000);
    }
}
