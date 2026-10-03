use crate::{
    manifest::{self, Installed, Manifest, Release},
    platform::{binary_name, render, render_with, select_asset},
    storage::{
        atomic_write, extract, remove_file_if_exists, validate_name, validate_version,
        verify_checksum,
    },
};
use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use std::{fs, io, path::PathBuf};

pub(crate) struct App {
    pub(crate) root: PathBuf,
    pub(crate) manifests: PathBuf,
    pub(crate) client: Client,
}

impl App {
    pub(crate) fn init(&self) -> Result<()> {
        for path in [
            self.root.join("bin"),
            self.root.join("packages"),
            self.root.join("state"),
            self.manifests.clone(),
        ] {
            fs::create_dir_all(path)?;
        }
        manifest::seed(&self.manifests)
    }

    pub(crate) fn names(&self) -> Result<Vec<String>> {
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

    pub(crate) fn manifest_path(&self, name: &str) -> Result<PathBuf> {
        validate_name(name)?;
        Ok(self.manifests.join(format!("{name}.yaml")))
    }

    pub(crate) fn manifest(&self, name: &str) -> Result<Manifest> {
        let path = self.manifest_path(name)?;
        let m: Manifest = serde_yaml::from_slice(
            &fs::read(&path).with_context(|| format!("reading {}", path.display()))?,
        )?;
        validate_name(&m.name)?;
        if m.name != name {
            bail!("manifest name must match filename: {name}");
        }
        validate_version(&m.version)?;
        match (&m.source.github, &m.source.url) {
            (Some(github), None) => {
                let repo: Vec<_> = github.split('/').collect();
                if repo.len() != 2 || repo.iter().any(|p| validate_name(p).is_err()) {
                    bail!("github must be owner/repository");
                }
            }
            (None, Some(url)) => {
                let parsed = reqwest::Url::parse(url)
                    .with_context(|| format!("invalid url source: {url}"))?;
                if !matches!(parsed.scheme(), "http" | "https") {
                    bail!("url source must be http(s): {url}");
                }
            }
            (None, None) => bail!("manifest must declare source.github or source.url"),
            (Some(_), Some(_)) => bail!("source.github and source.url are mutually exclusive"),
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

    pub(crate) fn state_path(&self, name: &str) -> Result<PathBuf> {
        validate_name(name)?;
        Ok(self.root.join("state").join(format!("{name}.json")))
    }

    pub(crate) fn installed(&self, name: &str) -> Result<Option<Installed>> {
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
}
impl App {
    pub(crate) fn github(&self, m: &Manifest, latest: bool) -> Result<Release> {
        let mut url = reqwest::Url::parse(
            &std::env::var("BINPICK_GITHUB_API")
                .unwrap_or_else(|_| "https://api.github.com".into()),
        )?;
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| anyhow::anyhow!("invalid API URL"))?;
            segments.pop_if_empty().push("repos");
            for part in m.source.github.as_deref().unwrap().split('/') {
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
}
impl App {
    pub(crate) fn update(&self, name: &str) -> Result<()> {
        let mut m = self.manifest(name)?;
        if m.pinned {
            println!("{name}: pinned ({}), skipped", m.version);
            return Ok(());
        }
        if m.source.github.is_some() {
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
            return Ok(());
        }
        // URL sources use the version already recorded in the manifest;
        // discovery requires the host's own latest-version endpoint, which
        // is out of scope for this MVP.
        if let Some(state) = self.installed(name)? {
            if state.version != m.version {
                self.install(&m, None)?;
            } else {
                println!("{name}: up to date ({})", m.version);
            }
        }
        Ok(())
    }

    pub(crate) fn install(&self, m: &Manifest, release: Option<&Release>) -> Result<()> {
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
            None => match &m.source.github {
                Some(_) => {
                    owned_release = self.github(m, false)?;
                    &owned_release
                }
                None => {
                    owned_release = Release {
                        tag_name: m.version.clone(),
                        draft: false,
                        prerelease: false,
                        assets: Vec::new(),
                    };
                    &owned_release
                }
            },
        };
        let filename = render(&asset.file, &m.version);
        let download_url =
            if let Some(release_asset) = release.assets.iter().find(|a| a.name == filename) {
                release_asset.browser_download_url.clone()
            } else {
                let template = asset
                    .url
                    .as_ref()
                    .or(m.source.url.as_ref())
                    .with_context(|| format!("release {} has no asset {filename}", m.version))?;
                render_with(
                    template,
                    &m.version,
                    &platform,
                    asset.format.as_deref().unwrap_or("raw"),
                )
            };
        let checksum_url = if asset.checksum.is_some() {
            if let Some(release_asset) = asset.checksum.as_ref().and_then(|name| {
                let name = render(name, &m.version);
                release.assets.iter().find(|a| a.name == name)
            }) {
                Some(release_asset.browser_download_url.clone())
            } else {
            asset
                .checksum_url
                .as_ref()
                .map(|template| render(template, &m.version))
        }
        } else {
            None
        };
        println!("{}: downloading {filename} ({platform})", m.name);
        let temp = tempfile::tempdir_in(self.root.join("packages"))?;
        let archive = temp.path().join("download");
        let mut out = fs::File::create(&archive)?;
        let mut response = self.client.get(&download_url).send()?.error_for_status()?;
        io::copy(&mut response, &mut out)?;
        out.sync_all()?;
        drop(out);
        if asset.checksum.is_some() {
            let url = checksum_url
                .context("checksum URL could not be resolved from GitHub assets or url template")?;
            let text = self.client.get(&url).send()?.error_for_status()?.text()?;
            verify_checksum(&archive, &filename, &text)?;
        } else {
            eprintln!("warning: {} has no upstream checksum configured", m.name);
        }
        let extracted = temp.path().join("extracted");
        fs::create_dir(&extracted)?;
        let format_name = asset.format.as_deref().unwrap_or_else(|| {
            if filename.ends_with(".tar.gz") || filename.ends_with(".tgz") {
                "tar.gz"
            } else if filename.ends_with(".tar.xz") || filename.ends_with(".txz") {
                "tar.xz"
            } else if filename.ends_with(".zip") {
                "zip"
            } else {
                "raw"
            }
        });
        extract(&archive, format_name, &binaries, &extracted)?;
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

    pub(crate) fn remove(&self, name: &str) -> Result<()> {
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
