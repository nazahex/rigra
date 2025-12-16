//! Template synchronization based on index `sync` rules.
//!
//! Applies file/dir copy operations conditionally per `when` scope tokens.
//! Uses simple recursive copying for directories.

use crate::models::index::{Index, SyncRule};
use std::fs;
use std::path::{Path, PathBuf};

pub struct SyncAction {
    pub rule_id: String,
    pub source: String,
    pub target: String,
    pub wrote: bool,
    pub skipped: bool,
}

/// Run sync actions for the given `scope`, producing a list of results.
pub fn run_sync(repo_root: &str, index_path: &str, scope: &str) -> Vec<SyncAction> {
    let root = PathBuf::from(repo_root);
    let idx_path = root.join(index_path);
    let idx_str = fs::read_to_string(&idx_path).expect("failed to read index.toml");
    let index: Index = toml::from_str(&idx_str).expect("invalid index.toml");

    let mut actions = Vec::new();
    for rule in index.sync {
        if !is_rule_enabled(&rule.when, scope) {
            continue;
        }
        let src = resolve_path(&idx_path, &rule.source);
        let dst = root.join(&rule.target);
        let (wrote, skipped) = copy_rule(&rule, &src, &dst);
        actions.push(SyncAction {
            rule_id: rule.id,
            source: src.to_string_lossy().to_string(),
            target: dst.to_string_lossy().to_string(),
            wrote,
            skipped,
        });
    }
    actions
}

/// Resolve a path relative to the index file location.
fn resolve_path(idx_path: &Path, rel: &str) -> PathBuf {
    let base = idx_path.parent().unwrap_or_else(|| Path::new("."));
    base.join(rel)
}

/// Copy one rule's source to target. Honors `overwrite` for files and
/// performs recursive copies for directories.
fn copy_rule(rule: &SyncRule, src: &PathBuf, dst: &PathBuf) -> (bool, bool) {
    let mut wrote = false;
    let mut skipped = false;
    if src.is_file() {
        if dst.exists() && !rule.overwrite {
            skipped = true;
        } else {
            if let Some(parent) = dst.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::copy(src, dst);
            wrote = true;
        }
    } else if src.is_dir() {
        // Copy directory recursively
        let _ = fs::create_dir_all(dst);
        if let Ok(entries) = fs::read_dir(src) {
            for entry in entries.flatten() {
                let p = entry.path();
                let t = dst.join(entry.file_name());
                let (_w, _s) = copy_rule(rule, &p, &t);
                wrote = true;
            }
        }
    }
    (wrote, skipped)
}

/// Check whether a rule is enabled for a given scope value.
fn is_rule_enabled(when: &str, scope: &str) -> bool {
    let w = when.trim();
    if w.is_empty() || w == "*" || w.eq_ignore_ascii_case("any") {
        return true;
    }
    // support comma or pipe separated tokens
    w.split(|c| c == ',' || c == '|')
        .map(|s| s.trim())
        .any(|tok| !tok.is_empty() && tok.eq_ignore_ascii_case(scope))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_sync_when_filters_rules() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        // conventions dir with index + template file
        let conv = root.join("conv");
        std::fs::create_dir_all(conv.join("templates")).unwrap();
        std::fs::write(conv.join("templates/a.txt"), b"hello").unwrap();
        // index.toml with two rules: one for repo, one for lib
        let index = r#"
[[sync]]
id = "r1"
source = "templates/a.txt"
target = "out/repo.txt"
when = "repo|app"

[[sync]]
id = "r2"
source = "templates/a.txt"
target = "out/lib.txt"
when = "lib"
"#;
        std::fs::write(conv.join("index.toml"), index).unwrap();

        // run with scope=repo
        let actions = run_sync(
            root.to_str().unwrap(),
            &format!("{}/index.toml", conv.file_name().unwrap().to_string_lossy()),
            "repo",
        );
        // only r1 should write; r2 filtered out by `when`
        assert!(actions.iter().any(|a| a.rule_id == "r1" && a.wrote));
        assert!(actions.iter().all(|a| a.rule_id != "r2"));
        assert!(root.join("out/repo.txt").exists());
        assert!(!root.join("out/lib.txt").exists());
    }
}
