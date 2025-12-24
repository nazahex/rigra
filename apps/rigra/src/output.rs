//! Output rendering for lint, format, and sync commands.
//!
//! Supports `human` (default) and `json` outputs. The JSON form includes
//! per-item fields and a top-level summary.

use crate::models::LintResult;
use crate::{format::FormatResult, sync::SyncAction};
use owo_colors::OwoColorize;
use serde_json::json;
use serde_json::Value as JsonVal;

fn use_colors(output: &str) -> bool {
    output != "json" && std::env::var_os("NO_COLOR").is_none()
}

/// Print lint results in the requested format.
pub fn print_lint(res: &LintResult, output: &str) {
    match output {
        "json" => println!(
            "{}",
            serde_json::to_string_pretty(&compose_lint_json(res)).unwrap()
        ),
        _ => {
            let color = use_colors(output);
            for is in &res.issues {
                let sev = match is.severity.as_str() {
                    "error" => {
                        if color {
                            "⟦error⟧".red().bold().to_string()
                        } else {
                            "⟦error⟧".to_string()
                        }
                    }
                    "warning" | "warn" => {
                        if color {
                            "⟦warn⟧".yellow().bold().to_string()
                        } else {
                            "⟦warn⟧".to_string()
                        }
                    }
                    _ => {
                        if color {
                            "⟦info⟧".blue().bold().to_string()
                        } else {
                            "⟦info⟧".to_string()
                        }
                    }
                };
                let icon = match is.severity.as_str() {
                    "error" => "✖".red().to_string(),
                    "warning" | "warn" => "▲".yellow().to_string(),
                    _ => "◆".blue().to_string(),
                };
                let file = if color {
                    is.file.clone().bold().to_string()
                } else {
                    is.file.clone()
                };
                println!("{} {} {} ❲{}❳ — {}", icon, sev, file, is.rule, is.message);
            }
            let summary = format!(
                "— Summary — errors={} warnings={} infos={} files={}",
                res.summary.errors, res.summary.warnings, res.summary.infos, res.summary.files
            );
            if color {
                println!("{}", summary.bold());
            } else {
                println!("{}", summary);
            }
        }
    }
}

/// Print formatting results. When `write` is false, previews and diffs
/// can be emitted; otherwise only file statuses are shown.
pub fn print_format(results: &[FormatResult], output: &str, write: bool, diff: bool) {
    match output {
        "json" => {
            let out = compose_format_json(results, write, diff);
            println!("{}", serde_json::to_string_pretty(&out).unwrap());
        }
        _ => {
            let color = use_colors(output);
            for r in results {
                if write {
                    if r.changed {
                        if color {
                            println!("{} {}", "✏️  formatted:".green().bold(), r.file.bold());
                        } else {
                            println!("✏️  formatted: {}", r.file);
                        }
                    }
                } else if r.changed {
                    if diff {
                        if let Some(d) =
                            build_naive_diff(r.original.as_deref(), r.preview.as_deref())
                        {
                            if color {
                                println!("{} {}\n{}", "---".cyan().bold(), r.file.bold(), d);
                            } else {
                                println!("--- {}\n{}", r.file, d);
                            }
                        } else if let Some(prev) = &r.preview {
                            if color {
                                println!("{} {}\n{}", "---".cyan().bold(), r.file.bold(), prev);
                            } else {
                                println!("--- {}\n{}", r.file, prev);
                            }
                        }
                    } else if let Some(prev) = &r.preview {
                        if color {
                            println!("{} {}\n{}", "---".cyan().bold(), r.file.bold(), prev);
                        } else {
                            println!("--- {}\n{}", r.file, prev);
                        }
                    }
                } else {
                    if color {
                        println!("{} {}", "no changes:".bright_black().to_string(), r.file);
                    } else {
                        println!("no changes: {}", r.file);
                    }
                }
            }
        }
    }
}

/// Print sync actions summarizing writes and skips.
pub fn print_sync(actions: &[SyncAction], output: &str) {
    match output {
        "json" => {
            let items: Vec<_> = actions
                .iter()
                .map(|a| {
                    json!({
                        "rule": a.rule_id,
                        "source": a.source,
                        "target": a.target,
                        "wrote": a.wrote,
                        "skipped": a.skipped,
                    })
                })
                .collect();
            let summary = json!({
                "wrote": actions.iter().filter(|a| a.wrote).count(),
                "skipped": actions.iter().filter(|a| a.skipped).count(),
                "total": actions.len(),
            });
            let out = json!({"results": items, "summary": summary});
            println!("{}", serde_json::to_string_pretty(&out).unwrap());
        }
        _ => {
            let color = use_colors(output);
            for a in actions {
                if a.skipped {
                    if color {
                        println!(
                            "{} {} -> {} (rule={})",
                            "⏭️  skipped (exists):".yellow().bold(),
                            a.source,
                            a.target,
                            a.rule_id
                        );
                    } else {
                        println!(
                            "⏭️  skipped (exists): {} -> {} (rule={})",
                            a.source, a.target, a.rule_id
                        );
                    }
                } else if a.wrote {
                    if color {
                        println!(
                            "{} {} -> {} (rule={})",
                            "📥 synced:".green().bold(),
                            a.source,
                            a.target,
                            a.rule_id
                        );
                    } else {
                        println!(
                            "📥 synced: {} -> {} (rule={})",
                            a.source, a.target, a.rule_id
                        );
                    }
                }
            }
        }
    }
}

fn build_naive_diff(old: Option<&str>, new: Option<&str>) -> Option<String> {
    let old = old?;
    let new = new?;
    let mut out = String::new();
    out.push_str("+++ new\n");
    out.push_str(new);
    out.push('\n');
    out.push_str("--- old\n");
    out.push_str(old);
    Some(out)
}

/// Compose lint JSON object (pure) for testing/snapshot purposes.
pub fn compose_lint_json(res: &LintResult) -> JsonVal {
    // Directly serialize LintResult as JSON, keeping stable shape
    serde_json::to_value(res).unwrap()
}

/// Compose format JSON object (pure) for testing/snapshot purposes.
pub fn compose_format_json(results: &[FormatResult], write: bool, diff: bool) -> JsonVal {
    let items: Vec<_> = results
        .iter()
        .map(|r| {
            json!({
                "file": r.file,
                "changed": r.changed,
                "wrote": write && r.changed,
                "preview": if !write { r.preview.as_ref() } else { None },
                "diff": if diff && !write { build_naive_diff(r.original.as_deref(), r.preview.as_deref()) } else { None }
            })
        })
        .collect();
    let summary = json!({
        "changed": results.iter().filter(|r| r.changed).count(),
        "total": results.len(),
        "wrote": if write { results.iter().filter(|r| r.changed).count() } else { 0 },
    });
    json!({"results": items, "summary": summary})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compose_format_json_write_and_preview_diff() {
        let results = vec![
            FormatResult {
                file: "a.json".into(),
                changed: true,
                preview: Some("{\n  \"x\": 1\n}".into()),
                original: Some("{\n  \"x\":1\n}".into()),
            },
            FormatResult {
                file: "b.json".into(),
                changed: false,
                preview: None,
                original: Some("{\n  \"y\":2\n}".into()),
            },
        ];
        // Case: write=false, diff=true ⇒ previews and diffs present for changed item
        let out = compose_format_json(&results, false, true);
        assert_eq!(out["summary"]["changed"], 1);
        assert_eq!(out["summary"]["wrote"], 0);
        assert!(out["results"][0]["preview"].is_string());
        assert!(out["results"][0]["diff"].is_string());
        // Case: write=true ⇒ no preview/diff, wrote equals changed
        let out2 = compose_format_json(&results, true, false);
        assert_eq!(out2["summary"]["wrote"], 1);
        assert!(out2["results"][0]["preview"].is_null());
        assert!(out2["results"][0]["diff"].is_null());
    }

    #[test]
    fn test_compose_lint_json_shape() {
        let res = crate::models::LintResult {
            issues: vec![crate::models::Issue {
                file: "p.json".into(),
                rule: "r".into(),
                severity: "warn".into(),
                path: "$.x".into(),
                message: "msg".into(),
            }],
            summary: crate::models::Summary {
                errors: 0,
                warnings: 1,
                infos: 0,
                files: 1,
            },
        };
        let out = compose_lint_json(&res);
        assert_eq!(out["summary"]["warnings"], 1);
        assert_eq!(out["issues"][0]["path"], "$.x");
    }
}
