mod maintenance;
mod manifest;
mod package;
mod platform;
mod storage;

#[cfg(test)]
mod unit_tests;

use crate::package::App;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use directories::BaseDirs;
use fs2::FileExt;
use reqwest::blocking::Client;
use std::{fs, path::PathBuf, time::Duration};

#[derive(Parser)]
#[command(version, about = "Install prebuilt executables, not system packages")]
struct Cli {
    /// Data directory (contains bin, packages, state and default manifests)
    #[arg(long, env = "PREX_ROOT", global = true)]
    root: Option<PathBuf>,
    /// Use and update YAML files in this directory instead of the default
    #[arg(long, env = "PREX_MANIFESTS", global = true)]
    manifests: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create directories and bundled YAML manifests; print PATH instructions
    Init,
    /// Install the version recorded in each YAML (use update to discover latest)
    Install {
        #[arg(required = true)]
        names: Vec<String>,
    },
    /// Discover upstream versions and update YAML only, without installing packages
    Update {
        names: Vec<String>,
        /// Show version changes without writing YAML
        #[arg(long)]
        dry_run: bool,
    },
    /// Upgrade installed packages to the versions recorded in YAML
    Upgrade { names: Vec<String> },
    /// Remove installed executables; keep YAML manifests
    Remove {
        #[arg(required = true)]
        names: Vec<String>,
    },
    /// Show available and installed versions
    List,
    /// List retained local generations
    History { name: String },
    /// Switch to a retained version offline, updating YAML as well
    Rollback {
        name: String,
        #[arg(long)]
        version: Option<String>,
    },
    /// Remove old generations, always preserving the active one
    Gc {
        names: Vec<String>,
        #[arg(long, default_value_t = 2)]
        keep: usize,
        #[arg(long)]
        dry_run: bool,
    },
    /// Prevent update and upgrade from advancing this package
    Pin { name: String },
    /// Allow this package to update again
    Unpin { name: String },
    /// Verify installed binaries, command entries and local metadata
    Doctor,
}

pub(crate) fn run() -> Result<()> {
    let cli = Cli::parse();
    let root = match cli.root {
        Some(root) => root,
        None => BaseDirs::new()
            .context("cannot locate home directory; set --root")?
            .home_dir()
            .join(".prex"),
    };
    fs::create_dir_all(&root)?;
    let root = fs::canonicalize(root)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join(".lock"))?;
    lock.try_lock_exclusive()
        .context("another prex process is running")?;
    let external_manifests = cli.manifests.is_some();
    let manifests = cli.manifests.unwrap_or_else(|| root.join("manifests"));
    let app = App {
        root,
        manifests,
        client: Client::builder()
            .user_agent(concat!("prex/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(300))
            .build()?,
    };
    app.init()?;
    // An explicitly selected directory is authoritative (e.g. an independent repo).
    if !external_manifests {
        manifest::seed(&app.manifests)?;
    }
    // YAML maintenance must not migrate or rewrite installed package state.
    if !matches!(&cli.command, Command::Update { .. }) {
        app.migrate()?;
    }
    match cli.command {
        Command::Init => {
            println!("Manifests: {}", app.manifests.display());
            println!("Add to PATH: {}", app.root.join("bin").display());
        }
        Command::List => {
            for name in app.names()? {
                let m = app.manifest(&name)?;
                let installed = app
                    .installed(&name)?
                    .map(|s| s.version)
                    .unwrap_or_else(|| "-".into());
                println!(
                    "{:<16} available={:<16} installed={}",
                    m.name, m.version, installed
                );
            }
        }
        Command::Install { names } => {
            for name in names {
                let m = app.manifest(&name)?;
                app.install(&m)?;
            }
        }
        Command::Update { names, dry_run } => app.update_many(names, dry_run)?,
        Command::Upgrade { names } => app.upgrade_many(names)?,
        Command::Remove { names } => {
            for name in names {
                app.remove(&name)?;
            }
        }
        Command::History { name } => app.history(&name)?,
        Command::Rollback { name, version } => app.rollback(&name, version.as_deref())?,
        Command::Gc {
            names,
            keep,
            dry_run,
        } => app.gc(&names, keep, dry_run)?,
        Command::Pin { name } => app.pin(&name, true)?,
        Command::Unpin { name } => app.pin(&name, false)?,
        Command::Doctor => app.doctor()?,
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
