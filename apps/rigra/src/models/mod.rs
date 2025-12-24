//! Shared data models for lint/format outputs and index/policy modules.

pub mod index;
pub mod policy;
pub mod sync_policy;

use serde::Serialize;

#[derive(Serialize)]
/// A single lint issue with severity and location.
pub struct Issue {
    pub file: String,
    pub rule: String,
    pub severity: String,
    pub path: String,
    pub message: String,
}

#[derive(Serialize)]
/// Aggregated lint summary used by printers.
pub struct Summary {
    pub errors: usize,
    pub warnings: usize,
    pub infos: usize,
    pub files: usize,
}

#[derive(Serialize)]
/// Lint results container.
pub struct LintResult {
    pub issues: Vec<Issue>,
    pub summary: Summary,
}
