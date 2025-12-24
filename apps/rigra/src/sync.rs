//! Template synchronization based on index `sync` rules.
//!
//! Applies file/dir copy operations conditionally per `when` scope tokens.
//! Uses simple recursive copying for directories.

use crate::models::index::{Index, SyncRule};
use crate::{config, utils};
use serde_json::Value as Json;
use std::fs;
use std::path::{Path, PathBuf};

pub struct SyncAction {
    pub rule_id: String,
    pub source: String,
    pub target: String,
    pub wrote: bool,
    pub format: Option<String>,
    pub would_write: bool,
}

/// Run sync actions for the given `scope`, producing a list of results.
pub fn run_sync(repo_root: &str, index_path: &str, scope: &str, write: bool) -> Vec<SyncAction> {
    let root = PathBuf::from(repo_root);
    let idx_path = root.join(index_path);
    let idx_str = fs::read_to_string(&idx_path).expect("failed to read index.toml");
    let index: Index = toml::from_str(&idx_str).expect("invalid index.toml");

    // Load client config (rigra.toml/yaml) for sync overrides
    let client_cfg = config::load_config(&root).unwrap_or_default();
    let sync_cfg_map = client_cfg
        .sync
        .as_ref()
        .and_then(|s| s.config.as_ref())
        .cloned()
        .unwrap_or_default();
    let post_hooks = client_cfg
        .sync
        .as_ref()
        .and_then(|s| s.hooks.as_ref().and_then(|h| h.post.as_ref()))
        .cloned()
        .unwrap_or_default();
    let ignore_ids: std::collections::HashSet<String> = client_cfg
        .sync
        .as_ref()
        .and_then(|s| s.ignore.clone())
        .unwrap_or_default()
        .into_iter()
        .collect();

    let mut actions = Vec::new();
    for rule in index.sync {
        if ignore_ids.contains(&rule.id) {
            continue;
        }
        if !is_rule_enabled(&rule.when, scope) {
            continue;
        }
        let src = resolve_path(&idx_path, &rule.source);
        // Allow per-id target override from client config
        let dst_target = sync_cfg_map
            .get(&rule.id)
            .and_then(|c| c.target.clone())
            .unwrap_or_else(|| rule.target.clone());
        let dst = root.join(&dst_target);
        let (wrote, would_write) =
            apply_sync(&root, &rule, &src, &dst, sync_cfg_map.get(&rule.id), write);
        actions.push(SyncAction {
            rule_id: rule.id,
            source: src.to_string_lossy().to_string(),
            target: dst.to_string_lossy().to_string(),
            wrote,
            format: rule.format.clone(),
            would_write,
        });
    }

    // Run post hooks for wrote actions
    for a in &actions {
        if a.wrote {
            if let Some(cmds) = post_hooks.get(&a.rule_id) {
                for cmd in cmds {
                    let _ = std::process::Command::new("sh")
                        .arg("-lc")
                        .arg(cmd)
                        .current_dir(&root)
                        .status();
                }
            }
        }
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
fn copy_rule(rule: &SyncRule, src: &PathBuf, dst: &PathBuf, write: bool) -> (bool, bool) {
    let mut wrote = false;
    let mut would_write = false;
    if src.is_file() {
        would_write = true;
        if let Some(parent) = dst.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if write {
            let _ = fs::copy(src, dst);
        }
        wrote = write;
    } else if src.is_dir() {
        // Copy directory recursively
        if write {
            let _ = fs::create_dir_all(dst);
        }
        if let Ok(entries) = fs::read_dir(src) {
            for entry in entries.flatten() {
                let p = entry.path();
                let t = dst.join(entry.file_name());
                let (_w, _would) = copy_rule(rule, &p, &t, write);
                if _would {
                    would_write = true;
                }
                if _w {
                    wrote = true;
                }
            }
        }
    }
    (wrote, would_write)
}

/// Apply sync for a rule, performing copy or smart merge depending on rule.format and client config.
fn apply_sync(
    _root: &Path,
    rule: &SyncRule,
    src: &PathBuf,
    dst: &PathBuf,
    client: Option<&config::SyncClientCfg>,
    write: bool,
) -> (bool, bool) {
    // Structured merge only when format=json and client merge config is present
    if let Some(ct) = rule.format.as_ref() {
        if ct.as_str().eq_ignore_ascii_case("json") {
            if let Some(mcfg) = client.and_then(|c| c.merge.as_ref()) {
                return apply_json_merge(rule, src, dst, mcfg, write);
            }
        }
    }
    copy_rule(rule, src, dst, write)
}

fn read_to_string(p: &Path) -> Option<String> {
    fs::read_to_string(p).ok()
}

fn fingerprint(s: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    format!("{:016x}-{}", h.finish(), s.len())
}

fn checksum_path(root: &Path, target: &Path) -> PathBuf {
    let rel = utils::rel_to_wd(target).replace('/', "__");
    root.join(".rigra/sync/checksums")
        .join(format!("{}.chk", rel))
}

fn ensure_parent(p: &Path) {
    if let Some(parent) = p.parent() {
        let _ = fs::create_dir_all(parent);
    }
}

fn apply_json_merge(
    rule: &SyncRule,
    src: &PathBuf,
    dst: &PathBuf,
    mcfg: &config::SyncClientMergeCfg,
    write: bool,
) -> (bool, bool) {
    let mut wrote = false;
    // will compute `would_write` only when differing from current
    let src_str = match read_to_string(src) {
        Some(s) => s,
        None => return (wrote, false),
    };
    let src_json: Json = match serde_json::from_str(&src_str) {
        Ok(j) => j,
        Err(_) => {
            let (w, ww) = copy_rule(rule, src, dst, write);
            return (w, ww);
        }
    };
    let dst_json: Json = if let Some(s) = read_to_string(dst) {
        serde_json::from_str(&s).unwrap_or(Json::Null)
    } else {
        Json::Null
    };
    let mut result = src_json.clone();

    // Helper closures to set or remove path (no wildcard support)
    let set_path = |root: &mut Json, path: &str, val: Option<Json>| {
        let p = path.trim().trim_start_matches('$').trim_start_matches('.');
        let mut segs: Vec<&str> = p.split('.').filter(|s| !s.is_empty()).collect();
        if segs.is_empty() {
            if let Some(v) = val {
                *root = v;
            } else {
                *root = Json::Null;
            }
            return;
        }
        let last = segs.pop().unwrap();
        let mut cur = root;
        for s in segs {
            if let Json::Object(map) = cur {
                if !map.contains_key(s) {
                    map.insert(s.to_string(), Json::Object(serde_json::Map::new()));
                }
                cur = map.get_mut(s).unwrap();
            } else {
                // cannot set nested into non-object; abort
                return;
            }
        }
        if let Json::Object(map) = cur {
            if let Some(v) = val {
                map.insert(last.to_string(), v);
            } else {
                map.remove(last);
            }
        }
    };

    // Apply precedence: override > keep > default; noSync wins last
    for p in &mcfg.override_paths {
        if let Some(v) = utils::get_json_path(&src_json, p) {
            set_path(&mut result, p, Some(v.clone()));
        }
    }
    for p in &mcfg.keep_paths {
        if let Some(v) = utils::get_json_path(&dst_json, p) {
            set_path(&mut result, p, Some(v.clone()));
        } else {
            // remove any value from result
            set_path(&mut result, p, None);
        }
    }
    for p in &mcfg.nosync_paths {
        if let Some(v) = utils::get_json_path(&dst_json, p) {
            set_path(&mut result, p, Some(v.clone()));
        } else {
            set_path(&mut result, p, None);
        }
    }

    // Array strategies
    if let Some(arr) = mcfg.array.as_ref() {
        for (path, strat) in arr.iter() {
            if strat == "union" {
                if let Some(Json::Array(sa)) = utils::get_json_path(&src_json, path) {
                    let da = utils::get_json_path(&dst_json, path).and_then(|v| v.as_array());
                    let mut merged = Vec::new();
                    if let Some(darr) = da {
                        merged.extend(darr.iter().cloned());
                    }
                    for it in sa.iter() {
                        if !merged.iter().any(|x| x == it) {
                            merged.push(it.clone());
                        }
                    }
                    set_path(&mut result, path, Some(Json::Array(merged)));
                }
            } else {
                // replace
                if let Some(v) = utils::get_json_path(&src_json, path) {
                    set_path(&mut result, path, Some(v.clone()));
                }
            }
        }
    }

    // Serialize and compare checksums
    let out_str = match serde_json::to_string_pretty(&result) {
        Ok(s) => s,
        Err(_) => src_str,
    };
    let out_fp = fingerprint(&out_str);
    let cur_fp = read_to_string(dst).map(|s| fingerprint(&s));
    if Some(out_fp.clone()) == cur_fp {
        return (false, false);
    }
    let would_write = true;
    if write {
        let cpath = checksum_path(&src.parent().unwrap_or_else(|| Path::new(".")), dst);
        ensure_parent(&cpath);
        let _ = fs::write(&cpath, &out_fp);
        ensure_parent(dst);
        if fs::write(dst, out_str).is_ok() {
            wrote = true;
        }
    }
    (wrote, would_write)
}

/// Check whether a rule is enabled for a given scope value.
fn is_rule_enabled(when: &str, scope: &str) -> bool {
    let w = when.trim();
    if w.is_empty() || w == "*" || w.eq_ignore_ascii_case("any") || w.eq_ignore_ascii_case("all") {
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
            true,
        );
        // only r1 should write; r2 filtered out by `when`
        assert!(actions.iter().any(|a| a.rule_id == "r1" && a.wrote));
        assert!(actions.iter().all(|a| a.rule_id != "r2"));
        assert!(root.join("out/repo.txt").exists());
        assert!(!root.join("out/lib.txt").exists());
    }
}
