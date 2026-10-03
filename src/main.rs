mod maintenance;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use directories::BaseDirs;
use fs2::FileExt;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    time::Duration,
};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    name: String,
    /// Exact upstream tag, including a leading v if present
    version: String,
    source: Source,
    assets: BTreeMap<String, Asset>,
    executables: Vec<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pinned: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    github: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Asset {
    file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    format: Option<String>,
    /// Release asset containing sha256sum-style checksums
    #[serde(default, skip_serializing_if = "Option::is_none")]
    checksum: Option<String>,
    /// Minimum glibc version; selects musl fallback when unavailable/too old
    #[serde(default, skip_serializing_if = "Option::is_none")]
    min_glibc: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Installed {
    name: String,
    version: String,
    executables: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    generation: Option<String>,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<ReleaseAsset>,
}
#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

struct App {
    root: PathBuf,
    manifests: PathBuf,
    client: Client,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
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

impl App {
    fn init(&self) -> Result<()> {
        for path in [
            self.root.join("bin"),
            self.root.join("packages"),
            self.root.join("state"),
            self.manifests.clone(),
        ] {
            fs::create_dir_all(path)?;
        }
        // Never overwrite a user-edited manifest.
        for (name, yaml) in [
            ("lazygit", include_str!("../manifests/lazygit.yaml")),
            ("yazi", include_str!("../manifests/yazi.yaml")),
            ("gh", include_str!("../manifests/gh.yaml")),
            ("herdr", include_str!("../manifests/herdr.yaml")),
        ] {
            let path = self.manifests.join(format!("{name}.yaml"));
            if !path.exists() {
                atomic_write(&path, yaml.as_bytes())?;
            }
        }
        Ok(())
    }

    fn names(&self) -> Result<Vec<String>> {
        let mut names = Vec::new();
        for entry in fs::read_dir(&self.manifests)? {
            let path = entry?.path();
            if path.extension().and_then(|s| s.to_str()) == Some("yaml") {
                names.push(
                    path.file_stem()
                        .context("invalid manifest filename")?
                        .to_str()
                        .context("non-UTF8 manifest filename")?
                        .to_owned(),
                );
            }
        }
        names.sort();
        Ok(names)
    }

    fn manifest_path(&self, name: &str) -> Result<PathBuf> {
        validate_name(name)?;
        Ok(self.manifests.join(format!("{name}.yaml")))
    }

    fn manifest(&self, name: &str) -> Result<Manifest> {
        let path = self.manifest_path(name)?;
        let m: Manifest = serde_yaml::from_slice(
            &fs::read(&path).with_context(|| format!("reading {}", path.display()))?,
        )?;
        validate_name(&m.name)?;
        if m.name != name {
            bail!("manifest name must match filename: {name}");
        }
        validate_version(&m.version)?;
        let repo: Vec<_> = m.source.github.split('/').collect();
        if repo.len() != 2 || repo.iter().any(|p| validate_name(p).is_err()) {
            bail!("github must be owner/repository");
        }
        if m.executables.is_empty() {
            bail!("executables cannot be empty");
        }
        let mut seen = std::collections::BTreeSet::new();
        for executable in &m.executables {
            validate_name(executable)?;
            if !seen.insert(executable.to_ascii_lowercase()) {
                bail!("duplicate executable: {executable}");
            }
        }
        Ok(m)
    }

    fn state_path(&self, name: &str) -> Result<PathBuf> {
        validate_name(name)?;
        Ok(self.root.join("state").join(format!("{name}.json")))
    }

    fn installed(&self, name: &str) -> Result<Option<Installed>> {
        let path = self.state_path(name)?;
        match fs::read(path) {
            Ok(bytes) => {
                let state: Installed = serde_json::from_slice(&bytes)?;
                if state.name != name {
                    bail!("invalid state name");
                }
                validate_version(&state.version)?;
                if let Some(id) = &state.generation {
                    validate_name(id)?;
                }
                for exe in &state.executables {
                    validate_name(exe)?;
                }
                Ok(Some(state))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn github(&self, m: &Manifest, latest: bool) -> Result<Release> {
        let mut url = reqwest::Url::parse(
            &std::env::var("BINPICK_GITHUB_API")
                .unwrap_or_else(|_| "https://api.github.com".into()),
        )?;
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| anyhow::anyhow!("invalid API URL"))?;
            segments.pop_if_empty().push("repos");
            for part in m.source.github.split('/') {
                segments.push(part);
            }
            segments.push("releases");
            if latest {
                segments.push("latest");
            } else {
                segments.push("tags").push(&m.version);
            }
        }
        let mut request = self
            .client
            .get(url)
            .header("Accept", "application/vnd.github+json");
        if let Ok(token) = std::env::var("GITHUB_TOKEN") {
            request = request.bearer_auth(token);
        }
        let release: Release = request
            .send()?
            .error_for_status()
            .context("GitHub API request failed (set GITHUB_TOKEN if rate-limited)")?
            .json()?;
        if release.draft || release.prerelease {
            bail!("only stable releases are supported");
        }
        validate_version(&release.tag_name)?;
        Ok(release)
    }

    fn update(&self, name: &str) -> Result<()> {
        let mut m = self.manifest(name)?;
        if m.pinned {
            println!("{name}: pinned ({}), skipped", m.version);
            return Ok(());
        }
        let release = self.github(&m, true)?;
        let old = m.version.clone();
        m.version = release.tag_name.clone();
        if let Some(state) = self.installed(name)? {
            if state.version != m.version {
                self.install(&m, Some(&release))?;
            }
        }
        if old != m.version {
            atomic_write(
                &self.manifest_path(name)?,
                serde_yaml::to_string(&m)?.as_bytes(),
            )?;
            println!("{name}: YAML {old} -> {}", m.version);
        } else {
            println!("{name}: up to date ({})", m.version);
        }
        Ok(())
    }

    fn install(&self, m: &Manifest, release: Option<&Release>) -> Result<()> {
        let (platform, asset) = select_asset(m)?;
        let binaries: Vec<String> = m.executables.iter().map(|e| binary_name(e)).collect();
        let old_state = self.installed(&m.name)?;
        self.check_collisions(&m.name, &binaries, old_state.as_ref())?;
        if old_state.as_ref().is_some_and(|s| s.version == m.version) {
            println!("{}: already installed ({})", m.name, m.version);
            return Ok(());
        }
        let owned_release;
        let release = match release {
            Some(r) => r,
            None => {
                owned_release = self.github(m, false)?;
                &owned_release
            }
        };
        let filename = render(&asset.file, &m.version);
        let url = release
            .assets
            .iter()
            .find(|a| a.name == filename)
            .with_context(|| format!("release {} has no asset {filename}", m.version))?;
        println!("{}: downloading {filename} ({platform})", m.name);
        let temp = tempfile::tempdir_in(self.root.join("packages"))?;
        let archive = temp.path().join("download");
        let mut out = fs::File::create(&archive)?;
        let mut response = self
            .client
            .get(&url.browser_download_url)
            .send()?
            .error_for_status()?;
        io::copy(&mut response, &mut out)?;
        out.sync_all()?;
        drop(out);
        if let Some(checksum) = &asset.checksum {
            let checksum = render(checksum, &m.version);
            let checksum_url = release
                .assets
                .iter()
                .find(|a| a.name == checksum)
                .with_context(|| format!("missing checksum asset {checksum}"))?;
            let text = self
                .client
                .get(&checksum_url.browser_download_url)
                .send()?
                .error_for_status()?
                .text()?;
            verify_checksum(&archive, &filename, &text)?;
        } else {
            eprintln!("warning: {} has no upstream checksum configured", m.name);
        }
        let extracted = temp.path().join("extracted");
        fs::create_dir(&extracted)?;
        let format = asset.format.as_deref().unwrap_or_else(|| {
            if filename.ends_with(".tar.gz") || filename.ends_with(".tgz") {
                "tar.gz"
            } else if filename.ends_with(".zip") {
                "zip"
            } else {
                "raw"
            }
        });
        extract(&archive, format, &binaries, &extracted)?;
        let package_dir = self.root.join("packages").join(&m.name);
        fs::create_dir_all(&package_dir)?;
        // Unique generation directory: never overwrite binaries currently in use.
        let generation = tempfile::Builder::new()
            .prefix(&format!("{}-", m.version))
            .tempdir_in(&package_dir)?;
        for exe in &binaries {
            fs::rename(extracted.join(exe), generation.path().join(exe))?;
        }
        let recorded = self.record_generation(&m.name, &m.version, &binaries, generation.path())?;
        let _destination = generation.keep();
        self.activate(m, &recorded)?;
        println!("{}: installed {}", m.name, m.version);
        Ok(())
    }

    fn remove(&self, name: &str) -> Result<()> {
        if let Some(state) = self.installed(name)? {
            for exe in state.executables {
                remove_file_if_exists(&self.root.join("bin").join(exe))?;
            }
            remove_file_if_exists(&self.state_path(name)?)?;
            let path = self.root.join("packages").join(name);
            if path.exists() {
                fs::remove_dir_all(path)?;
            }
            println!("{name}: removed");
        } else {
            println!("{name}: not installed");
        }
        Ok(())
    }
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        || name == "."
        || name == ".."
        || name.ends_with('.')
    {
        bail!("invalid name: {name}");
    }
    let stem = name.split('.').next().unwrap().to_ascii_uppercase();
    if ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
    {
        bail!("reserved Windows name: {name}");
    }
    Ok(())
}

fn validate_version(version: &str) -> Result<()> {
    validate_name(version).context("version must be a safe single path component")
}

fn platform() -> Result<String> {
    let os = match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "darwin",
        "windows" => "windows",
        other => bail!("unsupported OS: {other}"),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => bail!("unsupported architecture: {other}"),
    };
    // On Linux, select explicit libc assets; do not pretend musl and glibc are interchangeable.
    let libc = if os == "linux" {
        if cfg!(target_env = "musl") {
            "-musl"
        } else {
            "-gnu"
        }
    } else {
        ""
    };
    Ok(format!("{os}-{arch}{libc}"))
}

fn glibc_version() -> Option<String> {
    let output = std::process::Command::new("getconf")
        .arg("GNU_LIBC_VERSION")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()?
        .split_whitespace()
        .nth(1)
        .map(str::to_owned)
}

fn numeric_version(version: &str) -> Result<Vec<u32>> {
    let values: Vec<u32> = version
        .split('.')
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()
        .context("invalid numeric glibc version")?;
    if values.len() != 2 {
        bail!("glibc version must be major.minor");
    }
    Ok(values)
}

fn select_asset(m: &Manifest) -> Result<(String, &Asset)> {
    select_asset_for(m, &platform()?, glibc_version().as_deref())
}

fn select_asset_for<'a>(
    m: &'a Manifest,
    platform: &str,
    glibc: Option<&str>,
) -> Result<(String, &'a Asset)> {
    if let Some(asset) = m.assets.get(platform) {
        let compatible = match &asset.min_glibc {
            Some(min) if platform.ends_with("-gnu") => {
                let min = numeric_version(min)?;
                glibc
                    .map(numeric_version)
                    .transpose()?
                    .is_some_and(|current| current >= min)
            }
            _ => true,
        };
        if compatible {
            return Ok((platform.to_owned(), asset));
        }
    }
    if let Some(prefix) = platform.strip_suffix("-gnu") {
        let fallback = format!("{prefix}-musl");
        if let Some(asset) = m.assets.get(&fallback) {
            eprintln!(
                "{}: GNU asset unavailable/incompatible; selecting {fallback}",
                m.name
            );
            return Ok((fallback, asset));
        }
    }
    bail!(
        "{} has no compatible asset for {platform} (glibc: {}); check assets/min_glibc",
        m.name,
        glibc.unwrap_or("unknown")
    )
}

fn binary_name(name: &str) -> String {
    if cfg!(windows) && !name.ends_with(".exe") {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

fn render(template: &str, tag: &str) -> String {
    template
        .replace("{tag}", tag)
        .replace("{version}", tag.strip_prefix('v').unwrap_or(tag))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().context("missing parent")?)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

fn publish(source: &Path, target: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let temp = tempfile::tempdir_in(target.parent().context("missing parent")?)?;
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(source, &link)?;
        fs::rename(link, target)?;
    }
    #[cfg(not(unix))]
    {
        let mut temp = tempfile::NamedTempFile::new_in(target.parent().context("missing parent")?)?;
        io::copy(&mut fs::File::open(source)?, &mut temp)?;
        temp.as_file().sync_all()?;
        temp.persist(target).map_err(|e| e.error)?;
    }
    Ok(())
}

fn remove_file_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn verify_checksum(path: &Path, filename: &str, text: &str) -> Result<()> {
    let expected = text
        .lines()
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            let hash = fields.next()?;
            let file = fields.next()?.trim_start_matches('*');
            if file == filename && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                Some(hash.to_ascii_lowercase())
            } else {
                None
            }
        })
        .with_context(|| format!("checksum not found for {filename}"))?;
    let mut hash = Sha256::new();
    let mut file = fs::File::open(path)?;
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    if format!("{:x}", hash.finalize()) != expected {
        bail!("SHA-256 mismatch for {filename}");
    }
    Ok(())
}

fn safe_archive_path(path: &Path) -> Result<()> {
    if path.as_os_str().to_string_lossy().contains('\\')
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        bail!("unsafe archive path: {}", path.display());
    }
    Ok(())
}

fn copy_binary(reader: &mut impl Read, name: &str, wanted: &[String], output: &Path) -> Result<()> {
    if !wanted.iter().any(|s| s == name) {
        return Ok(());
    }
    let target = output.join(name);
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&target)
        .with_context(|| format!("duplicate executable in archive: {name}"))?;
    io::copy(reader, &mut file)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn extract(archive: &Path, format: &str, wanted: &[String], output: &Path) -> Result<()> {
    match format {
        "tar.gz" => {
            let decoder = flate2::read::GzDecoder::new(fs::File::open(archive)?);
            let mut tar = tar::Archive::new(decoder);
            for entry in tar.entries()? {
                let mut entry = entry?;
                let path = entry.path()?.into_owned();
                safe_archive_path(&path)?;
                if !entry.header().entry_type().is_file() {
                    continue;
                }
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    copy_binary(&mut entry, name, wanted, output)?;
                }
            }
        }
        "zip" => {
            let mut zip = zip::ZipArchive::new(fs::File::open(archive)?)?;
            for index in 0..zip.len() {
                let mut entry = zip.by_index(index)?;
                let path = PathBuf::from(entry.name());
                safe_archive_path(&path)?;
                if entry.is_dir() || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
                    continue;
                }
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    copy_binary(&mut entry, name, wanted, output)?;
                }
            }
        }
        "raw" => {
            if wanted.len() != 1 {
                bail!("raw downloads must have exactly one executable");
            }
            copy_binary(&mut fs::File::open(archive)?, &wanted[0], wanted, output)?;
        }
        other => bail!("unsupported archive format: {other}; use zip, tar.gz or raw"),
    }
    for exe in wanted {
        if !output.join(exe).is_file() {
            bail!("archive does not contain executable {exe}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn templates() {
        assert_eq!(render("pkg_{version}_{tag}", "v1.2.3"), "pkg_1.2.3_v1.2.3");
    }
    #[test]
    fn invalid_paths() {
        for name in ["", "..", "../x", "a/b", "a\\b", "CON", "NUL.exe", "COM1"] {
            assert!(validate_name(name).is_err());
        }
        for path in ["../evil", "/evil", "a/../../evil", "a\\evil"] {
            assert!(safe_archive_path(Path::new(path)).is_err());
        }
        assert!(safe_archive_path(Path::new("folder/bin/tool")).is_ok());
    }
    #[test]
    fn bundled_manifests() {
        for yaml in [
            include_str!("../manifests/lazygit.yaml"),
            include_str!("../manifests/yazi.yaml"),
            include_str!("../manifests/gh.yaml"),
            include_str!("../manifests/herdr.yaml"),
        ] {
            let m: Manifest = serde_yaml::from_str(yaml).unwrap();
            validate_name(&m.name).unwrap();
            assert!(!m.executables.is_empty());
        }
    }
    #[test]
    fn herdr_uses_static_linux_assets() {
        let m: Manifest = serde_yaml::from_str(include_str!("../manifests/herdr.yaml")).unwrap();
        for arch in ["amd64", "arm64"] {
            let key = format!("linux-{arch}-musl");
            let (selected, asset) =
                select_asset_for(&m, &format!("linux-{arch}-gnu"), Some("2.36")).unwrap();
            assert_eq!(selected, key);
            assert_eq!(asset.format.as_deref(), Some("raw"));
            assert_eq!(select_asset_for(&m, &key, None).unwrap().0, key);
        }
        assert!(select_asset_for(&m, "windows-arm64", None).is_err());
    }
    #[test]
    fn glibc_fallback() {
        let m: Manifest = serde_yaml::from_str(include_str!("../manifests/yazi.yaml")).unwrap();
        assert_eq!(
            select_asset_for(&m, "linux-amd64-gnu", Some("2.36"))
                .unwrap()
                .0,
            "linux-amd64-musl"
        );
        assert_eq!(
            select_asset_for(&m, "linux-amd64-gnu", Some("2.39"))
                .unwrap()
                .0,
            "linux-amd64-gnu"
        );
        assert_eq!(
            select_asset_for(&m, "linux-amd64-gnu", None).unwrap().0,
            "linux-amd64-musl"
        );
        assert!(numeric_version("not-a-version").is_err());
    }
    #[test]
    fn checksum() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("asset");
        fs::write(&path, b"abc").unwrap();
        let sum = format!("{:x}  *asset", Sha256::digest(b"abc"));
        verify_checksum(&path, "asset", &sum).unwrap();
        assert!(verify_checksum(&path, "different", &sum).is_err());
        fs::write(&path, b"tampered").unwrap();
        assert!(verify_checksum(&path, "asset", &sum).is_err());
    }
    #[test]
    fn raw_extraction_and_missing_binary() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("file");
        let out = tmp.path().join("out");
        fs::create_dir(&out).unwrap();
        fs::write(&file, b"binary").unwrap();
        extract(&file, "raw", &["tool".into()], &out).unwrap();
        assert_eq!(fs::read(out.join("tool")).unwrap(), b"binary");
        assert!(extract(&file, "raw", &["a".into(), "b".into()], &out).is_err());
    }
}
