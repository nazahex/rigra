//! CLI argument parsing via `clap`.

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "rigra",
    version,
    about = "Rigra v2 (Rust + TOML)",
    long_about = "Rigra — a tiny, fast CLI to lint, format, and sync JSON/TOML-based conventions.\n\nConfiguration precedence: CLI > rigra.toml > defaults.",
    after_help = "Examples:\n  rigra lint --index conventions/hyperedge/ts-base/index.toml\n  rigra format --index conv/index.toml --diff\n  rigra sync --index conv/index.toml --scope repo --check\n  rigra conv install --name myconv@v0.1.0 --source gh:owner/repo@v0.1.0",
    arg_required_else_help = true
)]
/// Top-level CLI options and subcommands.
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Commands,
}

#[derive(Subcommand)]
/// Supported subcommands for linting, formatting, and syncing.
pub enum Commands {
    /// Show version
    #[command(
        about = "Show version",
        long_about = "Print the current rigra version."
    )]
    Version,
    /// Lint configs using TOML policies
    #[command(
        about = "Run lint checks",
        long_about = "Validate files matched by index rules using TOML policies. Severity levels contribute to CI exits.",
        after_help = "Examples:\n  rigra lint --index conv/index.toml\n  rigra lint --index conv/index.toml --output json"
    )]
    Lint {
        #[arg(long, help = "Repository root (default: current dir)")]
        repo_root: Option<String>,
        #[arg(long, help = "Scope token for sync-related lint (e.g. repo, lib)")]
        scope: Option<String>,
        #[arg(long, help = "Output mode: human|json (default: human)")]
        output: Option<String>,
        #[arg(long, help = "Path to index.toml (required)")]
        index: Option<String>,
    },
    /// Format files deterministically
    #[command(
        about = "Apply deterministic formatting",
        long_about = "Reorder keys and adjust line breaks per policy. When --diff or --check is set, write is disabled.",
        after_help = "Examples:\n  rigra format --index conv/index.toml --diff\n  rigra format --index conv/index.toml --write"
    )]
    Format {
        #[arg(long, help = "Repository root (default: current dir)")]
        repo_root: Option<String>,
        #[arg(long, action = clap::ArgAction::SetTrue, help = "Write changes to files")]
        write: bool,
        #[arg(long, action = clap::ArgAction::SetTrue, help = "Show diffs for changed files (implies write=false)")]
        diff: bool,
        #[arg(long, action = clap::ArgAction::SetTrue, help = "Exit non-zero if changes would occur (implies write=false)")]
        check: bool,
        #[arg(long, help = "Output mode: human|json (default: human)")]
        output: Option<String>,
        #[arg(long, help = "Path to index.toml (required)")]
        index: Option<String>,
    },
    /// Sync templates/configs
    #[command(
        about = "Sync templates/configs",
        long_about = "Copy files or perform smart JSON merges according to sync policy. Honors scope filters.",
        after_help = "Examples:\n  rigra sync --index conv/index.toml --scope repo --dry-run\n  rigra sync --index conv/index.toml --scope lib --write"
    )]
    Sync {
        #[arg(long, help = "Repository root (default: current dir)")]
        repo_root: Option<String>,
        #[arg(long, help = "Scope token to select rules (e.g. repo, lib)")]
        scope: Option<String>,
        #[arg(long, help = "Output mode: human|json (default: human)")]
        output: Option<String>,
        #[arg(long, help = "Path to index.toml (required)")]
        index: Option<String>,
        #[arg(long, action = clap::ArgAction::SetTrue, help = "Apply changes to disk (disabled if --diff/--check)")]
        write: bool,
        #[arg(long, action = clap::ArgAction::SetTrue, help = "Preview planned writes without changing files")]
        dry_run: bool,
        #[arg(long, action = clap::ArgAction::SetTrue, help = "Exit non-zero if changes would occur")]
        check: bool,
    },
    /// Convention management (install/list/prune/path)
    Conv {
        #[command(subcommand)]
        cmd: ConvCmd,
    },
}

#[derive(Subcommand)]
/// Subcommands for `rigra conv`
pub enum ConvCmd {
    /// Install a convention into cache
    #[command(
        about = "Install convention",
        long_about = "Install a convention archive into repo cache under .rigra/conv."
    )]
    Install {
        #[arg(long, help = "Repository root (default: current dir)")]
        repo_root: Option<String>,
        /// Optional source override: gh:owner/repo@tag or file:/abs/path
        source: Option<String>,
        /// Optional name@version override for cache key
        #[arg(long, help = "Override name@version used as cache folder key")]
        name: Option<String>,
    },
    /// List installed conventions
    #[command(
        about = "List conventions",
        long_about = "List installed convention cache entries."
    )]
    Ls {
        #[arg(long, help = "Repository root (default: current dir)")]
        repo_root: Option<String>,
    },
    /// Prune all convention cache
    #[command(
        about = "Prune cache",
        long_about = "Remove all convention cache under .rigra/conv."
    )]
    Prune {
        #[arg(long, help = "Repository root (default: current dir)")]
        repo_root: Option<String>,
    },
    /// Resolve a conv path (conv:name@ver[:subpath])
    #[command(
        about = "Resolve path",
        long_about = "Resolve local cache path for a convention reference."
    )]
    Path {
        #[arg(long, help = "Repository root (default: current dir)")]
        repo_root: Option<String>,
        #[arg(help = "Convention ref: conv:name@ver[:subpath]")]
        conv: String,
    },
}
