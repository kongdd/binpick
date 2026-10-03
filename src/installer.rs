use crate::{
    app::App,
    artifacts::{extract, verify_checksum},
    files::{atomic_write, remove_file_if_exists},
    model::{Manifest, Release},
    platform::{binary_name, render, select_asset},
};
use anyhow::{Context, Result};
use std::{fs, io};

impl App {
    pub(crate) fn update(&self, name: &str) -> Result<()> {
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
