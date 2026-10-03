use crate::{
    catalog,
    files::{validate_name, validate_version},
    model::{Installed, Manifest},
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
        catalog::seed(&self.manifests)
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
