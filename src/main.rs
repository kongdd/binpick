mod maintenance;
mod manifest;
mod package;
mod platform;
mod storage;

#[cfg(test)]
mod unit_tests;

use crate::package::App;
use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use directories::BaseDirs;
use fs2::FileExt;
use reqwest::blocking::Client;
use std::{fs, path::PathBuf, time::Duration};

#[derive(Parser)]
#[command(version, about = "Install prebuilt executables, not system packages")]
struct Cli {
    /// Data directory (contains bin, packages, state and default manifests)
    #[arg(long, env = "BINPICK_ROOT", global = true)]
    root: Option<PathBuf>,
    /// Use and update YAML files in this directory instead of the default
    #[arg(long, env = "BINPICK_MANIFESTS", global = true)]
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
    /// Refresh YAML versions from GitHub; upgrade packages that are installed
    Update { names: Vec<String> },
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
    /// Prevent update from advancing this package
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
            .join(".binpick"),
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
        .context("another binpick process is running")?;
    let manifests = cli.manifests.unwrap_or_else(|| root.join("manifests"));
    let app = App {
        root,
        manifests,
        client: Client::builder()
            .user_agent(concat!("binpick/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(300))
            .build()?,
    };
    app.init()?;
    app.migrate()?;
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
                app.install(&m, None)?;
            }
        }
        Command::Update { names } => {
            let names = if names.is_empty() {
                app.names()?
            } else {
                names
            };
            let mut failed = false;
            for name in names {
                if let Err(e) = app.update(&name) {
                    eprintln!("{name}: {e:#}");
                    failed = true;
                }
            }
            if failed {
                bail!("some packages failed to update; their manifests were not advanced");
            }
        }
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
