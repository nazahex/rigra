//! Index schema: lists rules for lint/format targets and sync operations.

use serde::Deserialize;

#[derive(Deserialize)]
/// Top-level index configuration.
pub struct Index {
    #[serde(default)]
    pub rules: Vec<RuleIndex>,
    /// External sync policy file path relative to this index
    #[serde(default, rename = "sync")]
    pub sync_ref: Option<String>,
}

#[derive(Deserialize)]
/// A lint/format rule entry from the index.
pub struct RuleIndex {
    pub id: String,
    pub patterns: Vec<String>,
    pub policy: String,
}

// Sync rules are now defined in external policy files
