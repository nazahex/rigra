//! Index schema: lists rules for lint/format targets and sync operations.

use serde::Deserialize;

#[derive(Deserialize)]
/// Top-level index configuration.
pub struct Index {
    #[serde(default)]
    pub rules: Vec<RuleIndex>,
    #[serde(default)]
    pub sync: Vec<SyncRule>,
}

#[derive(Deserialize)]
/// A lint/format rule entry from the index.
pub struct RuleIndex {
    pub id: String,
    pub patterns: Vec<String>,
    pub policy: String,
}

#[derive(Deserialize)]
/// A sync rule entry from the index.
pub struct SyncRule {
    pub id: String,
    pub source: String,
    pub target: String,
    pub when: String,
    /// Optional format type for structured files: json|yaml|toml
    #[serde(default)]
    pub format: Option<String>,
}
