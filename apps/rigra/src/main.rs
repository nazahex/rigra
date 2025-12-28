//! Rigra CLI binary entry point.
//! Delegates to modules for lint/format/sync and prints results.

mod checks;
mod cli;
mod config;
mod conv;
mod format;
mod lint;
mod models;
mod output;
mod sync;
mod utils;

use crate::models::index::Index;
use clap::Parser;
use cli::{Cli, Commands};
// Colorization centralized in utils; no direct owo_colors usage here
use std::fs;

fn main() {
    // Early help handling to avoid surprises; prints long help and exits
    // Rely on Clap's auto help; no early manual printing
    let cli = Cli::parse();
    match cli.cmd {
        Commands::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
        }
        Commands::Lint {
            repo_root,
            scope,
            output,
            index,
        } => {
            let eff = config::resolve_effective(
                repo_root.as_deref(),
                index.as_deref(),
                scope.as_deref(),
                output.as_deref(),
                None,
                None,
                None,
            );
            // Require index to be configured (no default)
            if !eff.index_configured {
                eprintln!(
                    "{} {}",
                    crate::utils::error_prefix(),
                    "Index is not configured. Pass --index or add rigra.toml."
                );
                std::process::exit(2);
            }
            // Friendly note if no rigra config was found
            if config::load_config(&eff.repo_root).is_none() {
                eprintln!(
                    "{} {}",
                    crate::utils::note_prefix(),
                    "No rigra.toml found; using defaults."
                );
            }
            // Friendly error if index file is missing
            let idx_path = eff.repo_root.join(&eff.index);
            if !idx_path.exists() {
                eprintln!(
                    "{} {}",
                    crate::utils::error_prefix(),
                    format!(
                        "Index file not found: {} (pass --index or configure rigra.toml)",
                        idx_path.to_string_lossy()
                    )
                );
                std::process::exit(2);
            }
            // Emit single top info when default patterns from index are used (no overrides in rigra.toml)
            if eff.output != "json" {
                if let Ok(s) = fs::read_to_string(&idx_path) {
                    if let Ok(ix) = toml::from_str::<Index>(&s) {
                        let mut pat_set: std::collections::BTreeSet<String> =
                            std::collections::BTreeSet::new();
                        for r in ix.rules.iter() {
                            if !eff.pattern_overrides.contains_key(&r.id) {
                                for p in r.patterns.iter() {
                                    pat_set.insert(p.clone());
                                }
                            }
                        }
                        if !pat_set.is_empty() {
                            let joined =
                                format!("[{}]", pat_set.into_iter().collect::<Vec<_>>().join(", "));
                            eprintln!(
                                "{} {}",
                                crate::utils::info_prefix(),
                                format!("Using default patterns: {}", joined)
                            );
                        }
                    }
                }
            }
            let repo_root_str = eff.repo_root.to_string_lossy().to_string();
            let (result, errors) = lint::run_lint(
                &repo_root_str,
                &eff.index,
                &eff.scope,
                &eff.pattern_overrides,
            );
            output::print_lint(&result, &eff.output, &errors);
            if result.summary.errors > 0 {
                std::process::exit(1);
            }
        }
        Commands::Format {
            repo_root,
            write,
            diff,
            check,
            output,
            index,
        } => {
            let eff = config::resolve_effective(
                repo_root.as_deref(),
                index.as_deref(),
                None,
                output.as_deref(),
                if write { Some(true) } else { None },
                if diff { Some(true) } else { None },
                if check { Some(true) } else { None },
            );
            if !eff.index_configured {
                eprintln!(
                    "{} {}",
                    crate::utils::error_prefix(),
                    "Index is not configured. Pass --index or add rigra.toml."
                );
                std::process::exit(2);
            }
            if config::load_config(&eff.repo_root).is_none() {
                eprintln!(
                    "{} {}",
                    crate::utils::note_prefix(),
                    "No rigra.toml found; using defaults."
                );
            }
            let idx_path = eff.repo_root.join(&eff.index);
            if !idx_path.exists() {
                eprintln!(
                    "{} {}",
                    crate::utils::error_prefix(),
                    format!(
                        "Index file not found: {} (pass --index or configure rigra.toml)",
                        idx_path.to_string_lossy()
                    )
                );
                std::process::exit(2);
            }
            // Emit single top info when default patterns from index are used (no overrides in rigra.toml)
            if eff.output != "json" {
                if let Ok(s) = fs::read_to_string(&idx_path) {
                    if let Ok(ix) = toml::from_str::<Index>(&s) {
                        let mut pat_set: std::collections::BTreeSet<String> =
                            std::collections::BTreeSet::new();
                        for r in ix.rules.iter() {
                            if !eff.pattern_overrides.contains_key(&r.id) {
                                for p in r.patterns.iter() {
                                    pat_set.insert(p.clone());
                                }
                            }
                        }
                        if !pat_set.is_empty() {
                            let joined =
                                format!("[{}]", pat_set.into_iter().collect::<Vec<_>>().join(", "));
                            eprintln!(
                                "{} {}",
                                crate::utils::info_prefix(),
                                format!("Using default patterns: {}", joined)
                            );
                        }
                    }
                }
            }
            // CLI/config precedence at runtime:
            // - If diff or check is enabled, force write=false for this run.
            // - Otherwise respect write.
            let eff_diff = eff.diff;
            let eff_check = eff.check;
            let eff_write = if eff_diff || eff_check {
                false
            } else {
                eff.write
            };
            let repo_root_str = eff.repo_root.to_string_lossy().to_string();
            let (results, errors) = format::run_format(
                &repo_root_str,
                &eff.index,
                eff_write,
                eff_diff || eff_check,
                eff.strict_linebreak,
                eff.lb_between_groups,
                &eff.lb_before_fields,
                &eff.lb_in_fields,
                &eff.pattern_overrides,
            );
            output::print_format(&results, &eff.output, eff_write, eff_diff, &errors);
            if eff_check && results.iter().any(|r| r.changed) {
                std::process::exit(1);
            }
        }
        Commands::Sync {
            repo_root,
            scope,
            output,
            index,
            write,
            dry_run,
            check,
        } => {
            let eff = config::resolve_effective(
                repo_root.as_deref(),
                index.as_deref(),
                scope.as_deref(),
                output.as_deref(),
                Some(write),
                Some(dry_run),
                Some(check),
            );
            // Require index to be configured and point to a file
            if !eff.index_configured {
                eprintln!(
                    "{} {}",
                    crate::utils::error_prefix(),
                    "Index is not configured. Pass --index or add rigra.toml."
                );
                std::process::exit(2);
            }
            if config::load_config(&eff.repo_root).is_none() {
                eprintln!(
                    "{} {}",
                    crate::utils::note_prefix(),
                    "No rigra.toml found; using defaults."
                );
            }
            let idx_path = eff.repo_root.join(&eff.index);
            if !idx_path.exists() || !idx_path.is_file() {
                eprintln!(
                    "{} {}",
                    crate::utils::error_prefix(),
                    format!(
                        "Index file not found: {} (pass --index or configure rigra.toml)",
                        idx_path.to_string_lossy()
                    )
                );
                std::process::exit(2);
            }
            let eff_diff = eff.diff;
            let eff_check = eff.check;
            // Default write from config: [sync].write acts as ergonomics fallback
            let cfg_sync = config::load_config(&eff.repo_root).unwrap_or_default().sync;
            let cfg_sync_write = cfg_sync.as_ref().and_then(|s| s.write).unwrap_or(false);
            let eff_write = if eff_diff || eff_check {
                false
            } else {
                // CLI --write takes precedence; otherwise use [sync].write
                write || cfg_sync_write
            };
            let repo_root_str = eff.repo_root.to_string_lossy().to_string();
            let (actions, errors) =
                sync::run_sync(&repo_root_str, &eff.index, &eff.scope, eff_write);
            output::print_sync(&actions, &eff.output, &errors);
            // In check mode, exit non-zero when any action would write
            if eff_check && actions.iter().any(|a| a.would_write) {
                std::process::exit(1);
            }
        }
        Commands::Conv { cmd } => {
            match cmd {
                cli::ConvCmd::Install {
                    repo_root,
                    source,
                    name,
                } => {
                    let eff = config::resolve_effective(
                        repo_root.as_deref(),
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                    );
                    // Prefer CLI overrides; otherwise pull from rigra.toml [conv]
                    let cfg = config::load_config(&eff.repo_root).unwrap_or_default();
                    let cfg_conv = cfg.conv.as_ref();

                    // Determine name@ver
                    let name_ver = if let Some(nv) = name {
                        nv
                    } else if let Some(pkg) = cfg_conv.and_then(|c| c.package.clone()) {
                        if pkg.rsplit_once('@').is_some() {
                            pkg
                        } else {
                            eprintln!("[conv.package] must include @version");
                            std::process::exit(2);
                        }
                    } else if let Some(src) = source.as_ref().and_then(|s| conv::parse_source(s)) {
                        match src {
                            conv::Source::Gh {
                                owner: _,
                                repo,
                                tag,
                            } => format!("{}@{}", repo, tag),
                            _ => {
                                eprintln!(
                                    "{} {}",
                                    crate::utils::error_prefix(),
                                    "--name is required when using file: source without [conv.package]"
                                );
                                std::process::exit(2);
                            }
                        }
                    } else {
                        eprintln!(
                            "{} {}",
                            crate::utils::error_prefix(),
                            "missing install context: set [conv.package] in rigra.toml or pass --name"
                        );
                        std::process::exit(2);
                    };

                    // Determine source string
                    let src_str = if let Some(s) = source {
                        s
                    } else if let Some(s) = cfg_conv.and_then(|c| c.source.clone()) {
                        s
                    } else {
                        eprintln!(
                            "{} {}",
                            crate::utils::error_prefix(),
                            "missing source: set [conv.source] in rigra.toml or pass --source"
                        );
                        std::process::exit(2);
                    };
                    // If shorthand "github" is used, derive gh:owner/repo@tag from package
                    let src_str = if src_str == "github" {
                        if let Some((name, ver)) = crate::config::rsplit_once_at(&name_ver, '@') {
                            if let Some((owner, repo)) = crate::config::package_owner_repo(name) {
                                format!("gh:{}/{}@{}", owner, repo, ver)
                            } else {
                                src_str
                            }
                        } else {
                            src_str
                        }
                    } else {
                        src_str
                    };

                    match conv::install(&eff.repo_root, &name_ver, &src_str) {
                        Ok(path) => println!("installed: {}", path.to_string_lossy()),
                        Err(e) => {
                            eprintln!(
                                "{} {}",
                                crate::utils::error_prefix(),
                                format!("install failed: {}", e)
                            );
                            std::process::exit(2);
                        }
                    }
                }
                cli::ConvCmd::Ls { repo_root } => {
                    let eff = config::resolve_effective(
                        repo_root.as_deref(),
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                    );
                    for it in conv::list(&eff.repo_root) {
                        println!("{}", it);
                    }
                }
                cli::ConvCmd::Prune { repo_root } => {
                    let eff = config::resolve_effective(
                        repo_root.as_deref(),
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                    );
                    if let Err(e) = conv::prune(&eff.repo_root) {
                        eprintln!(
                            "{} {}",
                            crate::utils::error_prefix(),
                            format!("prune failed: {}", e)
                        );
                        std::process::exit(2);
                    } else {
                        println!("pruned");
                    }
                }
                cli::ConvCmd::Path {
                    repo_root,
                    conv: conv_str,
                } => {
                    let eff = config::resolve_effective(
                        repo_root.as_deref(),
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                    );
                    if let Some(cr) = conv::parse_conv_ref(&conv_str) {
                        let p = conv::resolve_path(&eff.repo_root, &cr);
                        println!("{}", p.to_string_lossy());
                    } else {
                        eprintln!("{} {}", crate::utils::error_prefix(), "invalid conv string");
                        std::process::exit(2);
                    }
                }
            }
        }
    }
}
