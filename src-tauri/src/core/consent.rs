use super::database::Database;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

/// Features that require user consent
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    ScreenRecording,
    OsActivity,
    KeyboardRecording,
    MouseRecording,
    CameraRecording,
    MicrophoneRecording,
}

impl Feature {
    /// Get all available features
    pub fn all() -> Vec<Feature> {
        vec![
            Feature::ScreenRecording,
            Feature::OsActivity,
            Feature::KeyboardRecording,
            Feature::MouseRecording,
            Feature::CameraRecording,
            Feature::MicrophoneRecording,
        ]
    }

    /// Convert feature to database-friendly string
    pub fn to_db_string(&self) -> &'static str {
        match self {
            Feature::ScreenRecording => "screen_recording",
            Feature::OsActivity => "os_activity",
            Feature::KeyboardRecording => "keyboard_recording",
            Feature::MouseRecording => "mouse_recording",
            Feature::CameraRecording => "camera_recording",
            Feature::MicrophoneRecording => "microphone_recording",
        }
    }

    /// Parse feature from string
    pub fn from_string(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "screen_recording" => Ok(Feature::ScreenRecording),
            "os_activity" => Ok(Feature::OsActivity),
            "keyboard_recording" => Ok(Feature::KeyboardRecording),
            "mouse_recording" => Ok(Feature::MouseRecording),
            "camera_recording" => Ok(Feature::CameraRecording),
            "microphone_recording" => Ok(Feature::MicrophoneRecording),
            _ => Err(format!("Unknown feature: {}", s)),
        }
    }
}

impl fmt::Display for Feature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_db_string())
    }
}

/// Manages user consent for various features
#[derive(Clone)]
pub struct ConsentManager {
    db: Arc<Database>,
}

impl ConsentManager {
    /// Create a new ConsentManager
    pub async fn new(db: Arc<Database>) -> Result<Self, Box<dyn std::error::Error>> {
        let manager = Self { db };

        // Initialize all features with default false consent if not already present
        for feature in Feature::all() {
            if manager.check_consent(feature).await?.is_none() {
                manager.initialize_feature(feature).await?;
            }
        }

        Ok(manager)
    }

    /// Initialize a feature with default false consent
    async fn initialize_feature(&self, feature: Feature) -> Result<(), Box<dyn std::error::Error>> {
        let id = uuid::Uuid::new_v4().to_string();
        let feature_name = feature.to_db_string();
        let timestamp = chrono::Utc::now().timestamp();

        sqlx::query(
            "INSERT INTO consent_records (id, feature_name, consent_given, timestamp, last_updated)
             VALUES (?, ?, 0, ?, ?)"
        )
        .bind(&id)
        .bind(feature_name)
        .bind(timestamp)
        .bind(timestamp)
        .execute(self.db.pool())
        .await?;

        Ok(())
    }

    /// Check if consent is granted for a feature
    /// Returns None if feature not initialized, Some(true/false) otherwise
    async fn check_consent(&self, feature: Feature) -> Result<Option<bool>, Box<dyn std::error::Error>> {
        let feature_name = feature.to_db_string();

        let result = sqlx::query_as::<_, (i64,)>(
            "SELECT consent_given FROM consent_records WHERE feature_name = ?"
        )
        .bind(feature_name)
        .fetch_optional(self.db.pool())
        .await?;

        Ok(result.map(|(consent,)| consent != 0))
    }

    /// Check if consent is granted for a feature (public API)
    /// Returns false if not initialized or not granted
    pub async fn is_consent_granted(&self, feature: Feature) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(self.check_consent(feature).await?.unwrap_or(false))
    }

    /// Grant consent for a feature
    pub async fn grant_consent(&self, feature: Feature) -> Result<(), Box<dyn std::error::Error>> {
        let feature_name = feature.to_db_string();
        let timestamp = chrono::Utc::now().timestamp();

        sqlx::query(
            "UPDATE consent_records SET consent_given = 1, last_updated = ? WHERE feature_name = ?"
        )
        .bind(timestamp)
        .bind(feature_name)
        .execute(self.db.pool())
        .await?;

        Ok(())
    }

    /// Revoke consent for a feature
    pub async fn revoke_consent(&self, feature: Feature) -> Result<(), Box<dyn std::error::Error>> {
        let feature_name = feature.to_db_string();
        let timestamp = chrono::Utc::now().timestamp();

        sqlx::query(
            "UPDATE consent_records SET consent_given = 0, last_updated = ? WHERE feature_name = ?"
        )
        .bind(timestamp)
        .bind(feature_name)
        .execute(self.db.pool())
        .await?;

        Ok(())
    }

    /// Get all consents as a HashMap
    pub async fn get_all_consents(&self) -> Result<HashMap<Feature, bool>, Box<dyn std::error::Error>> {
        let mut consents = HashMap::new();

        for feature in Feature::all() {
            let granted = self.is_consent_granted(feature).await?;
            consents.insert(feature, granted);
        }

        Ok(consents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn setup_test_db() -> Database {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("Failed to create in-memory database");

        let db = Database { pool: pool.clone() };

        // Run migrations
        db.run_migrations().await.expect("Failed to run migrations");

        // Make the pool accessible for testing
        Database { pool }
    }

    #[tokio::test]
    async fn test_feature_parsing() {
        assert_eq!(
            Feature::from_string("screen_recording").unwrap(),
            Feature::ScreenRecording
        );
        assert_eq!(
            Feature::from_string("os_activity").unwrap(),
            Feature::OsActivity
        );
        assert!(Feature::from_string("invalid_feature").is_err());
    }

    #[tokio::test]
    async fn test_consent_manager_initialization() {
        let db = setup_test_db().await;
        let manager = ConsentManager::new(db).await.expect("Failed to create manager");

        // All features should be initialized with false consent
        for feature in Feature::all() {
            let consent = manager.is_consent_granted(feature).await.expect("Failed to check consent");
            assert!(!consent, "Feature {:?} should default to false", feature);
        }
    }

    #[tokio::test]
    async fn test_grant_consent() {
        let db = setup_test_db().await;
        let manager = ConsentManager::new(db).await.expect("Failed to create manager");

        // Grant consent for screen recording
        manager.grant_consent(Feature::ScreenRecording)
            .await
            .expect("Failed to grant consent");

        // Check consent is granted
        let granted = manager.is_consent_granted(Feature::ScreenRecording)
            .await
            .expect("Failed to check consent");
        assert!(granted, "Consent should be granted");

        // Other features should still be false
        let os_activity = manager.is_consent_granted(Feature::OsActivity)
            .await
            .expect("Failed to check consent");
        assert!(!os_activity, "Other features should remain false");
    }

    #[tokio::test]
    async fn test_revoke_consent() {
        let db = setup_test_db().await;
        let manager = ConsentManager::new(db).await.expect("Failed to create manager");

        // Grant and then revoke consent
        manager.grant_consent(Feature::KeyboardRecording)
            .await
            .expect("Failed to grant consent");

        manager.revoke_consent(Feature::KeyboardRecording)
            .await
            .expect("Failed to revoke consent");

        // Check consent is revoked
        let granted = manager.is_consent_granted(Feature::KeyboardRecording)
            .await
            .expect("Failed to check consent");
        assert!(!granted, "Consent should be revoked");
    }

    #[tokio::test]
    async fn test_get_all_consents() {
        let db = setup_test_db().await;
        let manager = ConsentManager::new(db).await.expect("Failed to create manager");

        // Grant consent for a few features
        manager.grant_consent(Feature::ScreenRecording).await.unwrap();
        manager.grant_consent(Feature::MouseRecording).await.unwrap();

        // Get all consents
        let consents = manager.get_all_consents().await.expect("Failed to get consents");

        assert_eq!(consents.len(), 6, "Should have 6 features");
        assert_eq!(consents.get(&Feature::ScreenRecording), Some(&true));
        assert_eq!(consents.get(&Feature::MouseRecording), Some(&true));
        assert_eq!(consents.get(&Feature::OsActivity), Some(&false));
        assert_eq!(consents.get(&Feature::KeyboardRecording), Some(&false));
        assert_eq!(consents.get(&Feature::CameraRecording), Some(&false));
        assert_eq!(consents.get(&Feature::MicrophoneRecording), Some(&false));
    }

    #[tokio::test]
    async fn test_consent_persistence() {
        let db = setup_test_db().await;

        // Create manager and grant consent
        {
            let manager = ConsentManager::new(db.clone()).await.expect("Failed to create manager");
            manager.grant_consent(Feature::ScreenRecording).await.unwrap();
        }

        // Create new manager instance and verify consent persisted
        {
            let manager = ConsentManager::new(db).await.expect("Failed to create manager");
            let granted = manager.is_consent_granted(Feature::ScreenRecording)
                .await
                .expect("Failed to check consent");
            assert!(granted, "Consent should persist across manager instances");
        }
    }
}
