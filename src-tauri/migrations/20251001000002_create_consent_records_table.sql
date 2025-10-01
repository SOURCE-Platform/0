-- Create consent_records table
CREATE TABLE IF NOT EXISTS consent_records (
    id TEXT PRIMARY KEY NOT NULL,
    feature_name TEXT UNIQUE NOT NULL,
    consent_given INTEGER NOT NULL,
    timestamp INTEGER NOT NULL,
    last_updated INTEGER NOT NULL
);

-- Create index on feature_name for faster lookups
CREATE INDEX IF NOT EXISTS idx_consent_records_feature_name ON consent_records(feature_name);
