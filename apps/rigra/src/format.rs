//! JSON formatter for policy-driven ordering and line breaks.
//!
//! This module applies two deterministic passes to JSON objects:
//! - Key ordering based on the policy's `order.top`/`order.sub`.
//! - Line-break adjustments governed by `linebreak` rules when
//!   `strictLineBreak` is enabled (config default: true).
//!
//! Design notes:
//! - Group line breaks are only inserted at object depth 1 (top-level),
//!   and never before the first group. Rules in `before_fields` can
//!   override insertion for the first key of each group.
//! - In-field line breaks use the original source to faithfully preserve
//!   existing blank lines for fields marked `keep`. We compute a map of
//!   child entries that had a preceding blank line and mirror it after
//!   pretty-printing.
//! - `LineBreakRule::Keep` preserves exactly one blank line where it
//!   originally existed (otherwise none). `LineBreakRule::None` forces
//!   no blank line.

use crate::models::index::Index;
use crate::models::policy::{LineBreakRule, Policy};
use serde_json::{Map, Value as Json};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

pub struct FormatResult {
    pub file: String,
    pub changed: bool,
    pub preview: Option<String>,
    pub original: Option<String>,
}

/// Format JSON files matched by the index using the active policy.
///
/// Behavior:
/// - Reorders keys according to `order` rules.
/// - When `strict_linebreak` is true, applies `linebreak` rules:
///   - `between_groups`: blank line before the first key of subsequent groups
///     (top-level only; not before the first group).
///   - `before_fields`: per-field override for group boundaries.
///   - `in_fields`: preserve/remove blank lines between entries inside specific
///     object fields using the original file as reference when `Keep`.
///
/// Returns one `FormatResult` per matched file. When `write` is false and
/// `capture_old` is true, results include a pretty-printed preview and original.
pub fn run_format(
    repo_root: &str,
    index_path: &str,
    write: bool,
    capture_old: bool,
    strict_linebreak: bool,
    lb_between_groups_override: Option<bool>,
    lb_before_fields_override: &std::collections::HashMap<String, String>,
    lb_in_fields_override: &std::collections::HashMap<String, String>,
) -> Vec<FormatResult> {
    let root = PathBuf::from(repo_root);
    let idx_path = root.join(index_path);
    let idx_str = fs::read_to_string(&idx_path).expect("failed to read index.toml");
    let index: Index = toml::from_str(&idx_str).expect("invalid index.toml");

    let mut results = Vec::new();
    for ri in index.rules {
        // Load policy for this rule to discover per-target ordering rules
        let pol_path = idx_path.parent().unwrap().join(&ri.policy);
        let policy: Option<Policy> = fs::read_to_string(&pol_path)
            .ok()
            .and_then(|s| toml::from_str::<Policy>(&s).ok());

        // Collect all target files for this rule
        let mut targets: Vec<PathBuf> = Vec::new();
        for pat in ri.patterns.iter() {
            let abs_glob = root.join(pat);
            let pattern = abs_glob.to_string_lossy().to_string();
            for entry in glob::glob(&pattern).expect("bad glob pattern") {
                if let Ok(path) = entry {
                    targets.push(path);
                }
            }
        }

        // Process targets in parallel for throughput; gather deterministic order by file path
        let ord_opt = policy.as_ref().and_then(|p| p.order.as_ref()).cloned();
        let rule_results: Vec<FormatResult> = targets
            .par_iter()
            .map(|path| {
                let data = match fs::read_to_string(path) {
                    Ok(s) => s,
                    Err(_) => return FormatResult { file: path.to_string_lossy().to_string(), changed: false, preview: None, original: None },
                };
                let mut json: Json = match serde_json::from_str(&data) {
                    Ok(v) => v,
                    Err(_) => return FormatResult { file: path.to_string_lossy().to_string(), changed: false, preview: None, original: None },
                };
                if let Some(ord) = ord_opt.as_ref() {
                    let changed = apply_order_from(&mut json, &ord.top, &ord.sub);
                    if changed {
                        let mut s = serde_json::to_string_pretty(&json).unwrap();
                        if strict_linebreak {
                            let between = lb_between_groups_override
                                .or(policy
                                    .as_ref()
                                    .and_then(|p| p.linebreak.as_ref())
                                    .and_then(|lb| lb.between_groups))
                                .unwrap_or(false);
                            let fields = merge_linebreak_fields(
                                policy
                                    .as_ref()
                                    .and_then(|p| p.linebreak.as_ref())
                                    .map(|lb| &lb.before_fields),
                                lb_before_fields_override,
                            );
                            let in_fields = merge_linebreak_fields(
                                policy
                                    .as_ref()
                                    .and_then(|p| p.linebreak.as_ref())
                                    .map(|lb| &lb.in_fields),
                                lb_in_fields_override,
                            );
                            s = apply_linebreaks(s, &ord.top, between, &fields);
                            let keep_map = compute_in_field_keep_map(&data, &in_fields);
                            s = apply_in_field_linebreaks(s, &in_fields, &keep_map);
                        }
                        if write {
                            let _ = fs::write(path, s.clone());
                            return FormatResult { file: path.to_string_lossy().to_string(), changed: true, preview: None, original: if capture_old { Some(data) } else { None } };
                        } else {
                            return FormatResult { file: path.to_string_lossy().to_string(), changed: true, preview: Some(s), original: if capture_old { Some(data) } else { None } };
                        }
                    } else {
                        return FormatResult { file: path.to_string_lossy().to_string(), changed: false, preview: None, original: if capture_old { Some(data) } else { None } };
                    }
                }
                // No order applies
                FormatResult { file: path.to_string_lossy().to_string(), changed: false, preview: None, original: if capture_old { Some(data) } else { None } }
            })
            .collect();

        let mut rule_results = rule_results;
        rule_results.sort_by(|a, b| a.file.cmp(&b.file));
        results.extend(rule_results);
    }
    results
}

/// Reorder an object according to top-level groups and sub-field orders.
///
/// Returns true if the order changed. Remaining keys not listed in `top` or
/// `sub` are appended in lexicographic order for determinism.
fn apply_order_from(
    json: &mut Json,
    top: &Vec<Vec<String>>,
    sub: &std::collections::HashMap<String, Vec<String>>,
) -> bool {
    let mut changed = false;
    if let Json::Object(obj) = json {
        let mut new_obj = Map::new();
        for keys in top.iter() {
            for key in keys {
                if let Some(v) = obj.remove(key) {
                    new_obj.insert(key.clone(), v);
                    changed = true;
                }
            }
        }
        for keys in sub.values() {
            for key in keys {
                if let Some(v) = obj.remove(key) {
                    new_obj.insert(key.clone(), v);
                    changed = true;
                }
            }
        }
        let mut rest: Vec<_> = obj.iter().map(|(k, _)| k.clone()).collect();
        rest.sort();
        for key in rest {
            if let Some(v) = obj.remove(&key) {
                new_obj.insert(key.clone(), v);
            }
        }
        *obj = new_obj;
    }
    changed
}

/// Merge policy-provided field rules with CLI/config overrides.
///
/// Override values accept `"keep"` or anything else treated as `None`.
fn merge_linebreak_fields(
    policy: Option<&HashMap<String, LineBreakRule>>,
    override_map: &HashMap<String, String>,
) -> HashMap<String, LineBreakRule> {
    let mut out: HashMap<String, LineBreakRule> = policy.cloned().unwrap_or_default();
    for (k, v) in override_map.iter() {
        let rule = match v.as_str() {
            "keep" => LineBreakRule::Keep,
            _ => LineBreakRule::None,
        };
        out.insert(k.clone(), rule);
    }
    out
}

/// Scan the original source to determine which child keys had a blank
/// line before them inside objects configured with `Keep`.
///
/// Returns a map `field -> {child keys}` used to reinsert single blank
/// lines in the pretty-printed output.
fn compute_in_field_keep_map(
    original: &str,
    in_field_rules: &HashMap<String, LineBreakRule>,
) -> HashMap<String, HashSet<String>> {
    let mut result: HashMap<String, HashSet<String>> = HashMap::new();
    // consider only fields configured as Keep
    let targets: HashSet<&String> = in_field_rules
        .iter()
        .filter_map(|(k, v)| {
            if matches!(v, LineBreakRule::Keep) {
                Some(k)
            } else {
                None
            }
        })
        .collect();
    if targets.is_empty() {
        return result;
    }
    let mut active: Option<String> = None;
    let mut depth: i32 = 0;
    let mut prev_blank = false;
    for line in original.lines() {
        let trimmed = line.trim_start();
        if active.is_none() && trimmed.starts_with('"') {
            if let Some(p1) = trimmed.find('"') {
                let rest = &trimmed[p1 + 1..];
                if let Some(p2) = rest.find('"') {
                    let key = &rest[..p2];
                    if targets.contains(&key.to_string()) && trimmed.contains(": {") {
                        active = Some(key.to_string());
                        depth = 0;
                        prev_blank = false;
                    }
                }
            }
        }
        if let Some(ref fld) = active {
            for ch in trimmed.chars() {
                if ch == '{' {
                    depth += 1;
                } else if ch == '}' {
                    depth -= 1;
                }
            }
            if depth == 1 && trimmed.starts_with('"') && !trimmed.contains("\": {") {
                if prev_blank {
                    // record child key for which a blank line preceded it in the original
                    if let Some(p1) = trimmed.find('"') {
                        let rest = &trimmed[p1 + 1..];
                        if let Some(p2) = rest.find('"') {
                            let child = rest[..p2].to_string();
                            result.entry(fld.clone()).or_default().insert(child);
                        }
                    }
                }
            }
            if depth <= 0 && trimmed.contains('}') {
                active = None;
            }
        }
        prev_blank = trimmed.is_empty();
    }
    result
}

/// Apply top-level group line breaks and per-field overrides.
///
/// Notes:
/// - Only affects lines at object depth 1.
/// - Never inserts a blank line before the first group.
/// - `before_fields[key] == None` removes a blank line before that key even
///   when it is the first key of a subsequent group.
fn apply_linebreaks(
    pretty: String,
    groups: &Vec<Vec<String>>,
    between_groups: bool,
    field_rules: &std::collections::HashMap<String, LineBreakRule>,
) -> String {
    if !between_groups || groups.is_empty() {
        return pretty;
    }
    let mut group_first_keys: HashSet<String> = HashSet::new();
    for grp in groups.iter() {
        if let Some(first) = grp.first() {
            group_first_keys.insert(first.clone());
        }
    }
    let mut out: Vec<String> = Vec::new();
    let mut seen_first = false;
    let mut depth: i32 = 0; // track object depth; top-level keys at depth==1
    for line in pretty.lines() {
        let trimmed = line.trim_start();
        if depth == 1 && trimmed.starts_with('"') {
            if let Some(pos) = trimmed.find('"') {
                let rest = &trimmed[pos + 1..];
                if let Some(end) = rest.find('"') {
                    let key = &rest[..end];
                    if group_first_keys.contains(key) {
                        if seen_first {
                            match field_rules.get(key).copied() {
                                Some(LineBreakRule::None) => {
                                    if let Some(last) = out.last() {
                                        if last.is_empty() {
                                            out.pop();
                                        }
                                    }
                                }
                                Some(LineBreakRule::Keep) | None => {
                                    // Ensure exactly one blank line before group-first key
                                    if let Some(last) = out.last() {
                                        if last.is_empty() {
                                            // already one blank; if there are multiple, collapse to one
                                            if out.len() >= 2 && out[out.len() - 2].is_empty() {
                                                out.pop();
                                            }
                                        } else {
                                            out.push(String::new());
                                        }
                                    }
                                }
                            }
                        } else {
                            seen_first = true;
                        }
                    }
                }
            }
        }
        out.push(line.to_string());
        // update depth after processing current line
        for ch in trimmed.chars() {
            if ch == '{' {
                depth += 1;
            } else if ch == '}' {
                depth -= 1;
            }
        }
    }
    out.join("\n")
}

/// Apply in-field line break rules for object fields listed in `in_field_rules`.
///
/// When a field is `Keep`, we ensure one blank line before the child key if and
/// only if the original source had one (from `keep_map`). For `None` we remove
/// blank lines between entries.
fn apply_in_field_linebreaks(
    pretty: String,
    in_field_rules: &HashMap<String, LineBreakRule>,
    keep_map: &HashMap<String, HashSet<String>>, // field -> set of child keys with a blank line before in original
) -> String {
    if in_field_rules.is_empty() {
        return pretty;
    }
    let mut out: Vec<String> = Vec::new();
    let mut active_field: Option<(String, bool)> = None; // (field, seen_first_entry)
    let mut brace_depth: i32 = 0;
    for line in pretty.lines() {
        let trimmed = line.trim_start();

        if active_field.is_none() && trimmed.starts_with('"') {
            if let Some(pos) = trimmed.find('"') {
                let rest = &trimmed[pos + 1..];
                if let Some(end) = rest.find('"') {
                    let key = &rest[..end];
                    if in_field_rules.contains_key(key) && trimmed.contains(": {") {
                        active_field = Some((key.to_string(), false));
                        brace_depth = 0;
                    }
                }
            }
        }

        if let Some((ref fld, ref mut seen_first)) = active_field {
            // Update depth with this line's braces
            for ch in trimmed.chars() {
                if ch == '{' {
                    brace_depth += 1;
                } else if ch == '}' {
                    brace_depth -= 1;
                }
            }
            if brace_depth == 1 && trimmed.starts_with('"') && !trimmed.contains("\": {") {
                if !*seen_first {
                    // first entry: just mark seen, no blank line
                    *seen_first = true;
                } else {
                    let rule = in_field_rules
                        .get(fld)
                        .copied()
                        .unwrap_or(LineBreakRule::Keep);
                    // Determine current child key
                    let mut child_key: Option<String> = None;
                    if let Some(p1) = trimmed.find('"') {
                        let rest = &trimmed[p1 + 1..];
                        if let Some(p2) = rest.find('"') {
                            child_key = Some(rest[..p2].to_string());
                        }
                    }
                    match rule {
                        LineBreakRule::Keep => {
                            let should_have_blank = child_key
                                .as_ref()
                                .and_then(|ck| keep_map.get(fld).map(|set| set.contains(ck)))
                                .unwrap_or(false);
                            if should_have_blank {
                                // ensure exactly one blank line
                                if let Some(last) = out.last() {
                                    if last.is_empty() {
                                        if out.len() >= 2 && out[out.len() - 2].is_empty() {
                                            out.pop();
                                        }
                                    } else {
                                        out.push(String::new());
                                    }
                                }
                            } else {
                                // ensure none
                                if let Some(last) = out.last() {
                                    if last.is_empty() {
                                        out.pop();
                                    }
                                }
                            }
                        }
                        LineBreakRule::None => {
                            if let Some(last) = out.last() {
                                if last.is_empty() {
                                    out.pop();
                                }
                            }
                        }
                    }
                }
            }
            // If we've closed the object, reset state
            if brace_depth <= 0 && trimmed.contains('}') {
                // reset after pushing the current line below
            }
        }

        out.push(line.to_string());

        if let Some(_) = active_field.as_ref() {
            if brace_depth <= 0 && (trimmed == "}" || trimmed == "}," || trimmed.ends_with('}')) {
                active_field = None;
            }
        }
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::policy::OrderSpec;
    use serde_json::json;
    use std::collections::{HashMap, HashSet};

    #[test]
    fn test_apply_order_top_then_sub_then_rest() {
        let mut json = json!({
            "z": 1,
            "b": 2,
            "a": 3,
            "name": "n",
            "version": "v"
        });
        let mut sub = HashMap::new();
        sub.insert("meta".to_string(), vec!["version".to_string()]);
        let order = OrderSpec {
            top: vec![vec!["name".into()]],
            sub,
            message: None,
            level: None,
        };
        let changed = apply_order_from(&mut json, &order.top, &order.sub);
        assert!(changed);
        let keys: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, vec!["name", "version", "a", "b", "z"]);
    }

    #[test]
    fn test_apply_linebreaks_between_groups_inserts_blank_line() {
        // pretty JSON with two groups: first key is name, second group's first key is scripts
        let pretty = r#"{
  "name": "x",
  "version": "1.0.0",
  "scripts": {},
  "dependencies": {}
}"#
        .to_string();
        let groups = vec![
            vec!["name".to_string(), "version".to_string()],
            vec!["scripts".to_string(), "dependencies".to_string()],
        ];
        let field_rules: HashMap<String, LineBreakRule> = HashMap::new();
        let out = apply_linebreaks(pretty.clone(), &groups, true, &field_rules);
        // Expect a blank line before scripts because it's the first key of second group
        assert!(out.contains("\n\n  \"scripts\""));
    }

    #[test]
    fn test_apply_linebreaks_before_fields_respects_rules() {
        // Construct pretty with keys so that 'license' occurs after a previous line
        let pretty = r#"{
  "name": "x",
  "license": "MIT",
  "scripts": {}
}"#
        .to_string();
        let groups = vec![
            vec!["name".to_string(), "license".to_string()],
            vec!["scripts".to_string()],
        ];
        let mut rules: HashMap<String, LineBreakRule> = HashMap::new();
        rules.insert("license".to_string(), LineBreakRule::None);
        // do not set rule for scripts so default group insertion applies
        let out_none = apply_linebreaks(pretty.clone(), &groups, true, &rules);
        // No blank line should be before license
        assert!(out_none.contains("\n  \"license\""));
        // For scripts (first of second group) ensure one blank line by default
        assert!(out_none.contains("\n\n  \"scripts\""));
    }

    #[test]
    fn test_apply_in_field_linebreaks_keep_does_not_insert() {
        let pretty = r#"{
    "scripts": {
        "build": "echo build",
        "test": "echo test"
    }
}"#
        .to_string();
        let mut rules: HashMap<String, LineBreakRule> = HashMap::new();
        rules.insert("scripts".to_string(), LineBreakRule::Keep);
        let keep_map: HashMap<String, HashSet<String>> = HashMap::new();
        let out = apply_in_field_linebreaks(pretty, &rules, &keep_map);
        assert!(!out.contains("\n\n"));
    }

    #[test]
    fn test_apply_in_field_linebreaks_keep_preserves_existing_single_blank() {
        // original contains a blank line before 'test'
        let original = r#"{
    "scripts": {
        "build": "echo build",

        "test": "echo test"
    }
}"#;
        // pretty emitted by serde (no blanks)
        let pretty = r#"{
  "scripts": {
    "build": "echo build",
    "test": "echo test"
  }
}"#
        .to_string();
        let mut rules: HashMap<String, LineBreakRule> = HashMap::new();
        rules.insert("scripts".to_string(), LineBreakRule::Keep);
        let keep_map = compute_in_field_keep_map(original, &rules);
        let out = apply_in_field_linebreaks(pretty, &rules, &keep_map);
        assert!(out.contains("\"build\": \"echo build\",\n\n    \"test\""));
    }
}
