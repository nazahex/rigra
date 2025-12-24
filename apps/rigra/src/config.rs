//! Configuration discovery and effective settings resolution.
//!
//! Rigra reads `rigra.toml|yaml|yml` from the repository root (or closest
//! ancestor) and merges it with CLI flags to produce an `Effective` config.
//! Defaults:
//! - `index`: `convention/index.toml`
//! - `scope`: `repo`
//! - `output`: `human`
//! - `format.write|diff|check`: false
//! - `format.strictLineBreak`: true
//! - `format.linebreak.{between_groups,before_fields,in_fields}`: optional
//!
//! Overrides precedence: CLI > config file > defaults.

use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Deserialize, Clone)]
/// Formatting-related configuration section under `[format]`.
pub struct FormatCfg {
    pub write: Option<bool>,
    pub diff: Option<bool>,
    pub check: Option<bool>,
    #[serde(rename = "strictLineBreak")]
    pub strict_linebreak: Option<bool>,
    pub linebreak: Option<LineBreakCfg>,
}

#[derive(Debug, Default, Deserialize, Clone)]
/// Line break configuration (overrides policy at runtime).
pub struct LineBreakCfg {
    pub between_groups: Option<bool>,
    pub before_fields: Option<std::collections::HashMap<String, String>>, // keep|none
    pub in_fields: Option<std::collections::HashMap<String, String>>,     // keep|none
}

#[derive(Debug, Default, Deserialize, Clone)]
/// Root configuration loaded from `rigra.toml|yaml`.
pub struct RigletConfig {
    pub index: Option<String>,
    pub scope: Option<String>,
    pub output: Option<String>,
    pub format: Option<FormatCfg>,
    #[serde(default)]
    pub rules: Option<std::collections::HashMap<String, RulePatternOverride>>, // [rules.<id>].patterns
    #[serde(default)]
    pub conv: Option<ConvCfg>,
    #[serde(default)]
    pub sync: Option<SyncCfg>,
}

#[derive(Debug, Clone)]
/// Fully-resolved configuration used by commands after applying precedence.
pub struct Effective {
    pub repo_root: PathBuf,
    pub index: String,
    pub index_configured: bool,
    pub scope: String,
    pub output: String,
    pub write: bool,
    pub diff: bool,
    pub check: bool,
    pub strict_linebreak: bool,
    pub lb_between_groups: Option<bool>,
    pub lb_before_fields: std::collections::HashMap<String, String>,
    pub lb_in_fields: std::collections::HashMap<String, String>,
    pub pattern_overrides: std::collections::HashMap<String, Vec<String>>, // id -> patterns
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct RulePatternOverride {
    pub patterns: Vec<String>,
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct ConvCfg {
    #[serde(rename = "autoInstall")]
    pub auto_install: Option<bool>,
    /// Package identifier with version, e.g. "@nazahex/conv-lib-ts-mono@v0.1.0" or "myconv@v0.1.0"
    pub package: Option<String>,
    /// Single source of truth for installation: "gh:owner/repo@tag" or "file:/abs/path.tar.gz"
    pub source: Option<String>,
    /// Optional default subpath inside archive (defaults to "index.toml")
    pub subpath: Option<String>,
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct SyncCfg {
    #[serde(default)]
    pub config: Option<std::collections::HashMap<String, SyncClientCfg>>, // [sync.config.<id>]
    #[serde(default)]
    pub hooks: Option<SyncHooks>, // [sync.hooks.post]
    /// Default write behavior for `rigra sync` when CLI flags are absent
    pub write: Option<bool>,
    /// Ignore specific sync IDs entirely
    #[serde(default)]
    pub ignore: Option<Vec<String>>, // [sync].ignore = ["id1","id2"]
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct SyncHooks {
    #[serde(default)]
    pub post: Option<std::collections::HashMap<String, Vec<String>>>,
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct SyncClientCfg {
    pub target: Option<String>,
    pub merge: Option<SyncClientMergeCfg>,
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct SyncClientMergeCfg {
    #[serde(default, rename = "keep")]
    pub keep_paths: Vec<String>,
    #[serde(default, rename = "override")]
    pub override_paths: Vec<String>,
    #[serde(default, rename = "noSync")]
    pub nosync_paths: Vec<String>,
    #[serde(default)]
    pub array: Option<std::collections::HashMap<String, String>>, // path -> union|replace
}

/// Walk upward from `start` to detect the repository root.
///
/// Stops when a `rigra.toml|yaml|yml` or a `.git` directory is found.
pub fn detect_repo_root(start: &Path) -> PathBuf {
    // Walk up to find config or .git; else return start
    let mut cur = start;
    loop {
        if cur.join("rigra.toml").exists()
            || cur.join("rigra.yaml").exists()
            || cur.join("rigra.yml").exists()
        {
            return cur.to_path_buf();
        }
        if cur.join(".git").exists() {
            return cur.to_path_buf();
        }
        match cur.parent() {
            Some(p) => cur = p,
            None => return start.to_path_buf(),
        }
    }
}

/// Load `RigletConfig` from `rigra.toml` or `rigra.yaml|yml` if present.
pub fn load_config(root: &Path) -> Option<RigletConfig> {
    let toml_path = root.join("rigra.toml");
    if toml_path.exists() {
        let s = fs::read_to_string(&toml_path).ok()?;
        let cfg: RigletConfig = toml::from_str(&s).ok()?;
        return Some(cfg);
    }
    for yml in ["rigra.yaml", "rigra.yml"] {
        let p = root.join(yml);
        if p.exists() {
            let s = fs::read_to_string(&p).ok()?;
            let cfg: RigletConfig = serde_yaml::from_str(&s).ok()?;
            return Some(cfg);
        }
    }
    None
}

/// Resolve `Effective` by merging CLI flags, discovered config, and defaults.
pub fn resolve_effective(
    cli_repo_root: Option<&str>,
    cli_index: Option<&str>,
    cli_scope: Option<&str>,
    cli_output: Option<&str>,
    cli_write: Option<bool>,
    cli_diff: Option<bool>,
    cli_check: Option<bool>,
) -> Effective {
    let start = PathBuf::from(cli_repo_root.unwrap_or("."));
    let repo_root = detect_repo_root(&start);
    let cfg = load_config(&repo_root).unwrap_or_default();

    let index_src = cli_index.map(|s| s.to_string()).or(cfg.index);
    let (mut index, mut index_configured) = match index_src.clone() {
        Some(s) => (s, true),
        None => (String::new(), false),
    };

    let scope = cli_scope
        .map(|s| s.to_string())
        .or(cfg.scope)
        .unwrap_or_else(|| "repo".to_string());

    let output = cli_output
        .map(|s| s.to_string())
        .or(cfg.output)
        .unwrap_or_else(|| "human".to_string());

    let write = cli_write
        .or_else(|| cfg.format.as_ref().and_then(|f| f.write))
        .unwrap_or(false);
    let diff = cli_diff
        .or_else(|| cfg.format.as_ref().and_then(|f| f.diff))
        .unwrap_or(false);
    let check = cli_check
        .or_else(|| cfg.format.as_ref().and_then(|f| f.check))
        .unwrap_or(false);
    let strict_linebreak = cfg
        .format
        .as_ref()
        .and_then(|f| f.strict_linebreak)
        .unwrap_or(true);
    let lb_between_groups = cfg
        .format
        .as_ref()
        .and_then(|f| f.linebreak.as_ref()?.between_groups);
    let lb_before_fields = cfg
        .format
        .as_ref()
        .and_then(|f| f.linebreak.as_ref()?.before_fields.clone())
        .unwrap_or_default();
    let lb_in_fields = cfg
        .format
        .as_ref()
        .and_then(|f| f.linebreak.as_ref()?.in_fields.clone())
        .unwrap_or_default();

    // rules pattern overrides: support map form [rules.<id>].patterns
    let pattern_overrides = cfg
        .rules
        .unwrap_or_default()
        .into_iter()
        .map(|(id, ov)| (id, ov.patterns))
        .collect::<std::collections::HashMap<_, _>>();

    // Conv config
    let conv_auto_install = cfg
        .conv
        .as_ref()
        .and_then(|c| c.auto_install)
        .unwrap_or(false);
    let conv_source = cfg.conv.as_ref().and_then(|c| c.source.clone());

    // Resolve conv index if specified using Option A: conv:name@ver[:subpath]
    if let Some(ref idx) = index_src {
        if let Some(cr) = crate::conv::parse_conv_ref(idx) {
            let resolved = crate::conv::resolve_path(&repo_root, &cr);
            // If not present, optionally auto-install from sources map
            if !resolved.exists() && conv_auto_install {
                if let Some(src) = conv_source.as_ref() {
                    let name_ver = format!("{}@{}", cr.name, cr.ver);
                    let _ = crate::conv::install(&repo_root, &name_ver, src);
                }
            }
            index = resolved
                .strip_prefix(&repo_root)
                .unwrap_or(resolved.as_path())
                .to_string_lossy()
                .to_string();
            index_configured = true;
        }
    }

    // If index is not set, but [conv.package] is present, derive it.
    if !index_configured {
        if let Some(conv_cfg) = cfg.conv.as_ref() {
            if let Some(pkg) = conv_cfg.package.as_ref() {
                if let Some((name, ver)) = rsplit_once_at(pkg, '@') {
                    let subpath = conv_cfg
                        .subpath
                        .clone()
                        .unwrap_or_else(|| "index.toml".to_string());
                    let cr = crate::conv::ConvRef {
                        name: name.to_string(),
                        ver: ver.to_string(),
                        subpath,
                    };
                    let resolved = crate::conv::resolve_path(&repo_root, &cr);
                    if !resolved.exists() && conv_auto_install {
                        if let Some(src) = conv_cfg.source.as_ref() {
                            let mut src_str = src.clone();
                            if src == "github" {
                                if let Some((owner, repo)) = package_owner_repo(name) {
                                    src_str = format!("gh:{}/{}@{}", owner, repo, ver);
                                }
                            }
                            let _ = crate::conv::install(&repo_root, pkg, &src_str);
                        }
                    }
                    index = resolved
                        .strip_prefix(&repo_root)
                        .unwrap_or(resolved.as_path())
                        .to_string_lossy()
                        .to_string();
                    index_configured = true;
                }
            }
        }
    }

    Effective {
        repo_root,
        index,
        index_configured,
        scope,
        output,
        write,
        diff,
        check,
        strict_linebreak,
        lb_between_groups,
        lb_before_fields,
        lb_in_fields,
        pattern_overrides,
    }
}

pub fn rsplit_once_at(s: &str, ch: char) -> Option<(&str, &str)> {
    let mut iter = s.rsplitn(2, ch);
    let b = iter.next()?;
    let a = iter.next()?;
    Some((a, b))
}

pub fn package_owner_repo(name: &str) -> Option<(String, String)> {
    // Accept forms: @owner/repo, owner/repo, repo
    let s = name.strip_prefix('@').unwrap_or(name);
    let mut parts = s.splitn(2, '/');
    let first = parts.next()?;
    if let Some(second) = parts.next() {
        Some((first.to_string(), second.to_string()))
    } else {
        // No owner provided; use the same for owner and repo
        Some((first.to_string(), first.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_detect_and_load_toml() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let mut f = fs::File::create(root.join("rigra.toml")).unwrap();
        writeln!(
            f,
            "{}",
            r#"
index = "conventions/acme/index.toml"
scope = "repo"
output = "json"
[format]
write = true
    "#
        )
        .unwrap();

        // Resolve using explicit repo_root to avoid global CWD races
        let eff = resolve_effective(root.to_str(), None, None, None, None, None, None);
        assert_eq!(eff.index, "conventions/acme/index.toml");
        assert_eq!(eff.output, "json");
        assert!(eff.write);
    }

    #[test]
    fn test_load_yaml_and_defaults() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let mut f = fs::File::create(root.join("rigra.yaml")).unwrap();
        writeln!(
            f,
            "{}",
            r#"
index: convention/index.toml
scope: repo
output: human
format:
  write: false
  diff: false
  check: false
            "#
        )
        .unwrap();

        let eff = resolve_effective(root.to_str(), None, None, None, None, None, None);
        assert_eq!(eff.index, "convention/index.toml");
        assert_eq!(eff.scope, "repo");
        assert_eq!(eff.output, "human");
        // strict_linebreak defaults to true when unspecified
        assert!(eff.strict_linebreak);
    }

    #[test]
    fn test_precedence_and_linebreak_overrides_loaded() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let mut f = fs::File::create(root.join("rigra.toml")).unwrap();
        writeln!(
            f,
            "{}",
            r#"
index = "conventions/acme/index.toml"
scope = "repo"
output = "json"
[format]
write = true
diff = false
check = false
strictLineBreak = true
[format.linebreak]
between_groups = false
[format.linebreak.before_fields]
license = "keep"
[format.linebreak.in_fields]
scripts = "keep"
            "#
        )
        .unwrap();

        // CLI overrides write=false should take precedence over config write=true
        let eff = resolve_effective(root.to_str(), None, None, None, Some(false), None, None);
        assert!(!eff.write);
        // Linebreak overrides should be loaded from config
        assert_eq!(eff.lb_between_groups, Some(false));
        assert_eq!(
            eff.lb_before_fields.get("license").map(String::as_str),
            Some("keep")
        );
        assert_eq!(
            eff.lb_in_fields.get("scripts").map(String::as_str),
            Some("keep")
        );
    }

    #[test]
    fn test_conv_index_resolution_default_subpath() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let mut f = fs::File::create(root.join("rigra.toml")).unwrap();
        writeln!(
            f,
            "{}",
            r#"
index = "conv:hyperedge@v0.1.0"
scope = "repo"
output = "json"
            "#
        )
        .unwrap();

        let eff = resolve_effective(root.to_str(), None, None, None, None, None, None);
        assert!(eff.index_configured);
        // Should resolve to cache path with default index.toml
        let expected = root
            .join(".rigra/conv/hyperedge@v0.1.0/index.toml")
            .to_string_lossy()
            .to_string();
        assert_eq!(root.join(&eff.index).to_string_lossy(), expected);
    }

    #[test]
    fn test_conv_auto_install_with_file_source() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        // Create a tar.gz for a simple convention with index.toml
        let staged = root.join("staged");
        fs::create_dir_all(&staged).unwrap();
        fs::write(staged.join("index.toml"), "# idx").unwrap();
        let tgz = root.join("archive.tar.gz");
        let status = std::process::Command::new("tar")
            .current_dir(&staged)
            .args(["-czf", tgz.to_str().unwrap(), "."])
            .status()
            .expect("tar exec");
        assert!(status.success());

        // rigra.toml enabling autoInstall and declaring single source
        let mut f = fs::File::create(root.join("rigra.toml")).unwrap();
        writeln!(
            f,
            "{}",
            format!(
                r#"
[conv]
autoInstall = true
package = "myconv@v0.1.0"
source = "file:{}"
                "#,
                tgz.to_string_lossy()
            )
        )
        .unwrap();

        // Resolve; should trigger auto-install and point to cache path
        let eff = resolve_effective(root.to_str(), None, None, None, None, None, None);
        let resolved = root.join(&eff.index);
        assert!(resolved.exists());
    }

    #[test]
    fn test_conv_without_index_uses_package_and_github_shorthand() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let mut f = fs::File::create(root.join("rigra.toml")).unwrap();
        writeln!(
            f,
            "{}",
            r#"
[conv]
autoInstall = false
package = "@nazahex/conv-lib-ts-mono@v0.1.0"
source = "github"
            "#
        )
        .unwrap();

        let eff = resolve_effective(root.to_str(), None, None, None, None, None, None);
        assert!(eff.index_configured);
        let expected = root
            .join(".rigra/conv/@nazahex__conv-lib-ts-mono@v0.1.0/index.toml")
            .to_string_lossy()
            .to_string();
        assert_eq!(root.join(&eff.index).to_string_lossy(), expected);
        // No installation attempted since autoInstall=false; file won't exist.
    }
}
