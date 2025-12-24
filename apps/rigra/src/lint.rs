//! Lint runner for policy checks and order validation.
//!
//! Produces a `LintResult` with issues and a summary. Order lint uses
//! `policy.order` with optional `message` and `level` per policy.

use crate::checks::run_checks;
use crate::models::index::{Index, RuleIndex};
use crate::models::policy::Policy;
use crate::models::sync_policy::SyncPolicy;
use crate::models::{Issue, LintResult, Summary};
use crate::sync;
use glob::glob;
use rayon::prelude::*;
use serde_json::Value as Json;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Run lint across files matched by the index.
///
/// - Executes validation checks declared in the policy.
/// - Verifies top-level key order when `order` is present.
///
/// Severity accounting contributes to the final summary; `level = "error"`
/// affects the error count and typical CI exit behavior upstream.
pub fn run_lint(
    repo_root: &str,
    index_path: &str,
    scope: &str,
    patterns_override: &std::collections::HashMap<String, Vec<String>>,
) -> LintResult {
    let root = PathBuf::from(repo_root);
    let idx_path = root.join(index_path);
    let idx_str = match fs::read_to_string(&idx_path) {
        Ok(s) => s,
        Err(_) => {
            return LintResult {
                issues: vec![Issue {
                    file: idx_path.to_string_lossy().to_string(),
                    rule: "load-index".into(),
                    severity: "error".into(),
                    path: "$".into(),
                    message: format!(
                        "Index file not found. Looked at '{}'. Pass --index or add rigra.{{toml,yaml}}.",
                        idx_path.to_string_lossy()
                    ),
                }],
                summary: Summary {
                    errors: 1,
                    warnings: 0,
                    infos: 0,
                    files: 0,
                },
            };
        }
    };
    let index: Index = match toml::from_str(&idx_str) {
        Ok(ix) => ix,
        Err(_) => {
            return LintResult {
                issues: vec![Issue {
                    file: idx_path.to_string_lossy().to_string(),
                    rule: "parse-index".into(),
                    severity: "error".into(),
                    path: "$".into(),
                    message: "Index file is not valid TOML".into(),
                }],
                summary: Summary {
                    errors: 1,
                    warnings: 0,
                    infos: 0,
                    files: 0,
                },
            };
        }
    };

    let mut issues: Vec<Issue> = Vec::new();
    let mut files_count: usize = 0;

    // Cache policies across rules by path to avoid repeated I/O and parse when shared
    let mut policy_cache: HashMap<PathBuf, Policy> = HashMap::new();
    for ri in index.rules {
        lint_rule(
            &root,
            &idx_path,
            ri,
            &mut issues,
            &mut files_count,
            &mut policy_cache,
            patterns_override,
        );
    }

    // Evaluate sync status into lint using external policy
    if let Some(sync_ref) = index.sync_ref.as_ref() {
        let pol_path = idx_path.parent().unwrap().join(sync_ref);
        if let Ok(pol_str) = fs::read_to_string(&pol_path) {
            if let Ok(policy) = toml::from_str::<SyncPolicy>(&pol_str) {
                let defaults = policy.lint.unwrap_or_default();
                for rule in policy.sync {
                    if !is_rule_enabled(&rule.when, scope) {
                        continue;
                    }
                    // src resolved relative to index
                    let src = idx_path.parent().unwrap().join(&rule.source);
                    // apply client target override
                    let client_cfg = crate::config::load_config(&root).unwrap_or_default();
                    let dst_target = client_cfg
                        .sync
                        .as_ref()
                        .and_then(|s| s.config.as_ref())
                        .and_then(|m| m.get(&rule.id))
                        .and_then(|c| c.target.clone())
                        .unwrap_or_else(|| rule.target.clone());
                    let dst = root.join(&dst_target);
                    let (_w, would_write) = sync::apply_sync(
                        &root,
                        &rule,
                        &src,
                        &dst,
                        client_cfg
                            .sync
                            .as_ref()
                            .and_then(|s| s.config.as_ref())
                            .and_then(|m| m.get(&rule.id)),
                        false,
                    );
                    if would_write {
                        let sev = rule
                            .level
                            .clone()
                            .or(defaults.level.clone())
                            .unwrap_or_else(|| "info".to_string());
                        let msg = rule
                            .message
                            .clone()
                            .or(defaults.message.clone())
                            .unwrap_or_else(|| {
                                "Not synced yet. Please run rigra sync.".to_string()
                            });
                        issues.push(Issue {
                            file: dst.to_string_lossy().to_string(),
                            rule: format!("sync:{}", rule.id),
                            severity: sev,
                            path: "$".into(),
                            message: msg,
                        });
                    }
                }
            }
        }
    }

    let mut errs = 0usize;
    let mut warns = 0usize;
    let mut infos = 0usize;
    for is in &issues {
        match is.severity.as_str() {
            "error" => errs += 1,
            "warning" => warns += 1,
            _ => infos += 1,
        }
    }
    LintResult {
        issues,
        summary: Summary {
            errors: errs,
            warnings: warns,
            infos,
            files: files_count,
        },
    }
}
fn is_rule_enabled(when: &str, scope: &str) -> bool {
    let w = when.trim();
    if w.is_empty() || w == "*" || w.eq_ignore_ascii_case("any") || w.eq_ignore_ascii_case("all") {
        return true;
    }
    w.split(|c| c == ',' || c == '|')
        .map(|s| s.trim())
        .any(|tok| !tok.is_empty() && tok.eq_ignore_ascii_case(scope))
}

/// Lint a single indexed rule against its targets, collecting issues.
fn lint_rule(
    root: &PathBuf,
    idx_path: &PathBuf,
    ri: RuleIndex,
    issues: &mut Vec<Issue>,
    files_count: &mut usize,
    policy_cache: &mut HashMap<PathBuf, Policy>,
    patterns_override: &std::collections::HashMap<String, Vec<String>>,
) {
    let pol_path = idx_path.parent().unwrap().join(&ri.policy);
    let policy: &Policy = if let Some(p) = policy_cache.get(&pol_path) {
        p
    } else {
        let pol_str = match fs::read_to_string(&pol_path) {
            Ok(s) => s,
            Err(_) => {
                issues.push(Issue {
                    file: pol_path.to_string_lossy().to_string(),
                    rule: ri.id.clone(),
                    severity: "error".into(),
                    path: "$".into(),
                    message: format!(
                        "Policy file not found for rule '{}': {}",
                        ri.id,
                        pol_path.to_string_lossy()
                    ),
                });
                return;
            }
        };
        match toml::from_str::<Policy>(&pol_str) {
            Ok(p) => {
                policy_cache.insert(pol_path.clone(), p);
                policy_cache.get(&pol_path).unwrap()
            }
            Err(_) => {
                issues.push(Issue {
                    file: pol_path.to_string_lossy().to_string(),
                    rule: ri.id.clone(),
                    severity: "error".into(),
                    path: "$".into(),
                    message: "Policy file is not valid TOML".into(),
                });
                return;
            }
        }
    };

    // Choose patterns: override from rigra.toml if available, otherwise index defaults
    let use_patterns: Vec<String> = patterns_override
        .get(&ri.id)
        .cloned()
        .unwrap_or_else(|| ri.patterns.clone());
    let mut targets: Vec<PathBuf> = Vec::new();
    for pat in use_patterns.iter() {
        let abs_glob = root.join(pat);
        let pattern = abs_glob.to_string_lossy().to_string();
        for entry in glob(&pattern).expect("bad glob pattern") {
            if let Ok(p) = entry {
                targets.push(p);
            }
        }
    }

    let mut per_file: Vec<(Vec<Issue>, usize)> = targets
        .par_iter()
        .map(|path| {
            let data = match fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => return (Vec::new(), 0),
            };
            let json: Json = match serde_json::from_str(&data) {
                Ok(v) => v,
                Err(_) => return (Vec::new(), 0),
            };
            let mut file_issues: Vec<Issue> = Vec::new();
            let mut found = run_checks(&policy.checks, &json, path, &ri.id);
            file_issues.append(&mut found);
            if let Some(ord) = policy.order.as_ref() {
                if let Json::Object(obj) = &json {
                    let actual: Vec<String> = obj.keys().cloned().collect();
                    let mut expected: Vec<String> = Vec::new();
                    for group in &ord.top {
                        for key in group {
                            if obj.contains_key(key.as_str()) {
                                expected.push(key.clone());
                            }
                        }
                    }
                    let mut rest: Vec<String> = obj
                        .keys()
                        .filter(|k| !expected.contains(k))
                        .cloned()
                        .collect();
                    rest.sort();
                    expected.extend(rest);
                    if expected != actual {
                        file_issues.push(Issue {
                            file: path.to_string_lossy().to_string(),
                            rule: ri.id.clone(),
                            severity: ord.level.clone().unwrap_or_else(|| "error".to_string()),
                            path: "$".to_string(),
                            message: ord.message.clone().unwrap_or_else(|| {
                                "Object key order does not match policy".to_string()
                            }),
                        });
                    }
                }
            }
            (file_issues, 1)
        })
        .collect();
    // Deterministic ordering of issues by file then message
    let mut combined: Vec<Issue> = per_file.iter_mut().flat_map(|(v, _)| v.drain(..)).collect();
    combined.sort_by(|a, b| a.file.cmp(&b.file).then(a.message.cmp(&b.message)));
    *files_count += per_file.iter().map(|(_, c)| *c).sum::<usize>();
    issues.extend(combined);
}
