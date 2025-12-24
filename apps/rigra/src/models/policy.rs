//! Policy schema used by lint and format passes.
//!
//! Key components:
//! - `order`: Declares top-level key groups and optional sub-orders, plus
//!   lint `message` and `level` (info|warn|error).
//! - `linebreak`: Controls line breaks between top-level groups and inside
//!   specific object fields via `before_fields` and `in_fields` maps.
//! - `checks`: Validation rules (required/type/const/pattern/enum/length...).
//!
//! All identifiers and comments are documented in English.

use serde::Deserialize;
use serde_json::Value as Json;
use std::collections::HashMap;

#[derive(Deserialize)]
/// Root policy loaded from TOML files referenced by the index.
pub struct Policy {
    #[serde(default)]
    pub checks: Vec<Check>,
    #[serde(default)]
    pub order: Option<OrderSpec>,
    #[serde(default)]
    pub linebreak: Option<LineBreakSpec>,
}

#[derive(Deserialize, Clone)]
/// Controls object key ordering and lint metadata.
pub struct OrderSpec {
    #[serde(default)]
    pub top: Vec<Vec<String>>,
    #[serde(default)]
    pub sub: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub level: Option<String>, // info|warn|error (treated as error for exit code when 'error')
}

#[derive(Deserialize, Clone)]
/// Line-break behavior configuration.
pub struct LineBreakSpec {
    #[serde(default)]
    pub between_groups: Option<bool>,
    #[serde(default)]
    pub before_fields: HashMap<String, LineBreakRule>,
    #[serde(default)]
    pub in_fields: HashMap<String, LineBreakRule>,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
/// Rule applied to line-break handling.
pub enum LineBreakRule {
    Keep,
    None,
}

#[derive(Deserialize, Clone)]
#[serde(tag = "kind")]
/// Lint checks supported by the engine.
pub enum Check {
    #[serde(rename = "required")]
    Required {
        fields: Vec<String>,
        message: Option<String>,
        #[serde(default)]
        level: Option<String>,
    },
    #[serde(rename = "type")]
    Type {
        #[serde(default)]
        /// Map of JSON paths to expected kinds (string|number|integer|boolean|array|object|null)
        fields: HashMap<String, String>,
        message: Option<String>,
        #[serde(default)]
        level: Option<String>,
    },
    #[serde(rename = "const")]
    Const {
        field: String,
        value: Json,
        message: Option<String>,
        #[serde(default)]
        level: Option<String>,
    },
    #[serde(rename = "pattern")]
    Pattern {
        field: String,
        regex: String,
        message: Option<String>,
        #[serde(default)]
        level: Option<String>,
    },
    #[serde(rename = "enum")]
    Enum {
        field: String,
        values: Vec<Json>,
        message: Option<String>,
        #[serde(default)]
        level: Option<String>,
    },
    #[serde(rename = "minLength")]
    MinLength {
        field: String,
        min: usize,
        message: Option<String>,
        #[serde(default)]
        level: Option<String>,
    },
    #[serde(rename = "maxLength")]
    MaxLength {
        field: String,
        max: usize,
        message: Option<String>,
        #[serde(default)]
        level: Option<String>,
    },
}
