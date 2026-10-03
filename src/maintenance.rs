use crate::{
    app::App,
    files::{atomic_write, publish, remove_file_if_exists, validate_name, validate_version},
    model::{Installed, Manifest},
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

const METADATA: &str = ".binpick-generation.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Generation {
    pub name: String,
    pub version: String,
    pub executables: Vec<String>,
    pub id: String,
    pub created_at: u128,
    pub hashes: BTreeMap<String, String>,
}

// Capture symlinks as links, not their targets. Recovery handles ordinary errors,
// but is not a durable journal for process termination or power loss.
enum Snapshot {
    Missing,
    Link(PathBuf),
    File(Vec<u8>, fs::Permissions),
}
impl Snapshot {
    fn capture(path: &Path) -> Result<Self> {
        match fs::symlink_metadata(path) {
            Ok(m) if m.file_type().is_symlink() => Ok(Self::Link(fs::read_link(path)?)),
            Ok(m) if m.is_file() => Ok(Self::File(fs::read(path)?, m.permissions())),
            Ok(_) => bail!("refusing to replace non-file {}", path.display()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::Missing),
            Err(e) => Err(e.into()),
        }
    }
    fn restore(&self, path: &Path) -> Result<()> {
        match self {
            Self::Missing => remove_file_if_exists(path),
            Self::File(bytes, permissions) => {
                atomic_write(path, bytes)?;
                fs::set_permissions(path, permissions.clone())?;
                Ok(())
            }
            Self::Link(target) => {
                #[cfg(unix)]
                {
                    publish(target, path)
                }
                #[cfg(not(unix))]
                {
                    bail!(
                        "cannot restore symlink {} -> {}",
                        path.display(),
                        target.display()
                    )
                }
            }
        }
    }
}

fn recover(snapshots: &[(PathBuf, Snapshot)], result: Result<()>) -> Result<()> {
    if let Err(error) = result {
        let mut failures = Vec::new();
        for (path, snapshot) in snapshots.iter().rev() {
            if let Err(e) = snapshot.restore(path) {
                failures.push(format!("{}: {e:#}", path.display()));
            }
        }
        if !failures.is_empty() {
            bail!(
                "activation failed: {error:#}; recovery incomplete: {}",
                failures.join("; ")
            );
        }
        return Err(error.context("activation failed; previous state restored"));
    }
    Ok(())
}

fn digest(path: &Path) -> Result<String> {
    let mut hash = Sha256::new();
    let mut file = fs::File::open(path)?;
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

impl App {
    pub(crate) fn generation_path(&self, name: &str, id: &str) -> Result<PathBuf> {
        validate_name(name)?;
        validate_name(id)?;
        let path = self.root.join("packages").join(name).join(id);
        let meta = fs::symlink_metadata(&path)?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            bail!("generation must be a real directory");
        }
        let canonical = fs::canonicalize(&path)?;
        if !canonical.starts_with(self.root.join("packages")) {
            bail!("generation escapes packages directory");
        }
        Ok(canonical)
    }

    fn read_generation(&self, name: &str, id: &str) -> Result<Generation> {
        let dir = self.generation_path(name, id)?;
        let g: Generation = serde_json::from_slice(&fs::read(dir.join(METADATA))?)?;
        if g.name != name || g.id != id {
            bail!("generation metadata identity mismatch");
        }
        validate_version(&g.version)?;
        let mut seen = BTreeSet::new();
        if g.executables.is_empty() {
            bail!("empty generation");
        }
        for exe in &g.executables {
            validate_name(exe)?;
            if !seen.insert(exe.to_ascii_lowercase()) || !g.hashes.contains_key(exe) {
                bail!("invalid generation executables/hashes");
            }
        }
        Ok(g)
    }

    pub(crate) fn record_generation(
        &self,
        name: &str,
        version: &str,
        executables: &[String],
        dir: &Path,
    ) -> Result<Generation> {
        let id = dir
            .file_name()
            .and_then(|s| s.to_str())
            .context("invalid generation id")?
            .to_owned();
        validate_name(&id)?;
        let mut hashes = BTreeMap::new();
        for exe in executables {
            hashes.insert(exe.clone(), digest(&dir.join(exe))?);
        }
        let g = Generation {
            name: name.into(),
            version: version.into(),
            executables: executables.to_vec(),
            id,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos(),
            hashes,
        };
        atomic_write(&dir.join(METADATA), &serde_json::to_vec_pretty(&g)?)?;
        Ok(g)
    }

    fn verify_generation(&self, g: &Generation) -> Result<()> {
        let dir = self.generation_path(&g.name, &g.id)?;
        for exe in &g.executables {
            let path = dir.join(exe);
            let meta = fs::symlink_metadata(&path)?;
            if !meta.is_file() || meta.file_type().is_symlink() {
                bail!("invalid executable {}", path.display());
            }
            if digest(&path)? != g.hashes[exe] {
                bail!("stored executable checksum mismatch: {exe}");
            }
        }
        Ok(())
    }

    /// Upgrade state created by the initial MVP. Old inactive directories have no
    /// trustworthy version metadata and remain untouched by GC.
    pub(crate) fn migrate(&self) -> Result<()> {
        for name in self.installed_names()? {
            let Some(mut state) = self.installed(&name)? else {
                continue;
            };
            if state.generation.is_some() {
                continue;
            }
            let parent = self.root.join("packages").join(&name);
            let mut candidates = Vec::new();
            for entry in fs::read_dir(parent)? {
                let path = entry?.path();
                let meta = fs::symlink_metadata(&path)?;
                if !meta.is_dir() || meta.file_type().is_symlink() {
                    continue;
                }
                if !path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(|id| id.starts_with(&format!("{}-", state.version)))
                {
                    continue;
                }
                let matches = state.executables.iter().all(|exe| {
                    let stored = path.join(exe);
                    let live = self.root.join("bin").join(exe);
                    #[cfg(unix)]
                    {
                        fs::canonicalize(live).ok() == fs::canonicalize(&stored).ok()
                            && stored.is_file()
                    }
                    #[cfg(not(unix))]
                    {
                        digest(&live).ok() == digest(&stored).ok() && stored.is_file()
                    }
                });
                if matches {
                    candidates.push(path);
                }
            }
            if candidates.len() != 1 {
                bail!("cannot safely migrate {name}; expected one matching active generation");
            }
            let g =
                self.record_generation(&name, &state.version, &state.executables, &candidates[0])?;
            state.generation = Some(g.id);
            atomic_write(
                &self.state_path(&name)?,
                &serde_json::to_vec_pretty(&state)?,
            )?;
        }
        Ok(())
    }

    pub(crate) fn installed_names(&self) -> Result<Vec<String>> {
        let mut names = Vec::new();
        for entry in fs::read_dir(self.root.join("state"))? {
            let path = entry?.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .context("invalid state filename")?;
                validate_name(name)?;
                names.push(name.to_owned());
            }
        }
        names.sort();
        Ok(names)
    }

    fn generations(&self, name: &str) -> Result<Vec<Generation>> {
        validate_name(name)?;
        let parent = self.root.join("packages").join(name);
        if !parent.exists() {
            return Ok(Vec::new());
        }
        let mut result = Vec::new();
        for entry in fs::read_dir(parent)? {
            let path = entry?.path();
            // Untracked staging/legacy directories are never automatically removed.
            if !path.join(METADATA).exists() {
                continue;
            }
            let id = path
                .file_name()
                .and_then(|s| s.to_str())
                .context("invalid generation filename")?;
            result.push(self.read_generation(name, id)?);
        }
        result.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        Ok(result)
    }

    pub(crate) fn check_collisions(
        &self,
        name: &str,
        binaries: &[String],
        old: Option<&Installed>,
    ) -> Result<()> {
        for owner in self.installed_names()? {
            if owner == name {
                continue;
            }
            if let Some(s) = self.installed(&owner)? {
                for exe in binaries {
                    if s.executables
                        .iter()
                        .any(|other| other.eq_ignore_ascii_case(exe))
                    {
                        bail!("{exe} belongs to {}", s.name);
                    }
                }
            }
        }
        for exe in binaries {
            let path = self.root.join("bin").join(exe);
            if fs::symlink_metadata(path).is_ok()
                && !old.is_some_and(|s| s.executables.contains(exe))
            {
                bail!("refusing to overwrite unrelated executable {exe}");
            }
        }
        Ok(())
    }

    pub(crate) fn activate(&self, m: &Manifest, g: &Generation) -> Result<()> {
        if m.name != g.name || m.version != g.version {
            bail!("activation manifest mismatch");
        }
        self.verify_generation(g)?;
        let old = self.installed(&m.name)?;
        self.check_collisions(&m.name, &g.executables, old.as_ref())?;
        let mut paths: BTreeSet<PathBuf> = g
            .executables
            .iter()
            .map(|e| self.root.join("bin").join(e))
            .collect();
        if let Some(s) = &old {
            paths.extend(s.executables.iter().map(|e| self.root.join("bin").join(e)));
        }
        paths.insert(self.state_path(&m.name)?);
        paths.insert(self.manifest_path(&m.name)?);
        let snapshots = paths
            .into_iter()
            .map(|p| Snapshot::capture(&p).map(|s| (p, s)))
            .collect::<Result<Vec<_>>>()?;
        let dir = self.generation_path(&g.name, &g.id)?;
        let result = (|| -> Result<()> {
            for exe in &g.executables {
                publish(&dir.join(exe), &self.root.join("bin").join(exe))?;
            }
            if let Some(old) = old {
                for exe in old.executables {
                    if !g.executables.contains(&exe) {
                        remove_file_if_exists(&self.root.join("bin").join(exe))?;
                    }
                }
            }
            let state = Installed {
                name: g.name.clone(),
                version: g.version.clone(),
                executables: g.executables.clone(),
                generation: Some(g.id.clone()),
            };
            atomic_write(
                &self.state_path(&m.name)?,
                &serde_json::to_vec_pretty(&state)?,
            )?;
            atomic_write(
                &self.manifest_path(&m.name)?,
                serde_yaml::to_string(m)?.as_bytes(),
            )?;
            Ok(())
        })();
        recover(&snapshots, result)
    }

    pub(crate) fn history(&self, name: &str) -> Result<()> {
        let active = self.installed(name)?.and_then(|s| s.generation);
        for g in self.generations(name)? {
            println!(
                "{} {} {}{}",
                g.name,
                g.version,
                g.id,
                if active.as_deref() == Some(&g.id) {
                    " [active]"
                } else {
                    ""
                }
            );
        }
        Ok(())
    }

    pub(crate) fn rollback(&self, name: &str, version: Option<&str>) -> Result<()> {
        let state = self.installed(name)?.context("package is not installed")?;
        let mut m = self.manifest(name)?;
        let generations = self.generations(name)?;
        let g = generations
            .iter()
            .find(|g| {
                Some(&g.id) != state.generation.as_ref()
                    && version.map_or(g.version != state.version, |v| v == g.version)
            })
            .context("no matching previous version; use history to see retained versions")?;
        m.version = g.version.clone();
        self.activate(&m, g)?;
        println!("{name}: rolled back {} -> {}", state.version, g.version);
        Ok(())
    }

    pub(crate) fn gc(&self, names: &[String], keep: usize, dry_run: bool) -> Result<()> {
        if keep == 0 {
            bail!("--keep must be at least 1 (the active generation)");
        }
        let names = if names.is_empty() {
            self.installed_names()?
        } else {
            names.to_vec()
        };
        // Plan the whole operation before deleting anything.
        let mut plan = Vec::new();
        for name in names.into_iter().collect::<BTreeSet<_>>() {
            let state = self
                .installed(&name)?
                .context("gc only cleans installed packages")?;
            let active = state.generation.context("missing active generation")?;
            let generations = self.generations(&name)?;
            let current = generations
                .iter()
                .find(|g| g.id == active)
                .context("active generation missing; refusing gc")?;
            self.verify_generation(current)?;
            let mut retained = 1;
            for g in generations {
                if g.id == active {
                    continue;
                }
                if retained < keep {
                    retained += 1;
                    continue;
                }
                plan.push(self.generation_path(&name, &g.id)?);
            }
        }
        for path in &plan {
            println!(
                "{} {}",
                if dry_run { "would remove" } else { "removing" },
                path.display()
            );
            if !dry_run {
                fs::remove_dir_all(path)?;
            }
        }
        println!(
            "{} generation(s) {}",
            plan.len(),
            if dry_run {
                "would be removed"
            } else {
                "removed"
            }
        );
        Ok(())
    }

    pub(crate) fn pin(&self, name: &str, pinned: bool) -> Result<()> {
        let mut m = self.manifest(name)?;
        m.pinned = pinned;
        atomic_write(
            &self.manifest_path(name)?,
            serde_yaml::to_string(&m)?.as_bytes(),
        )?;
        println!("{name}: {}", if pinned { "pinned" } else { "unpinned" });
        Ok(())
    }

    pub(crate) fn doctor(&self) -> Result<()> {
        let mut failures = Vec::new();
        for name in self.installed_names()? {
            let check = (|| -> Result<()> {
                let state = self.installed(&name)?.context("missing state")?;
                let id = state.generation.context("missing active generation")?;
                let g = self.read_generation(&name, &id)?;
                if state.version != g.version || state.executables != g.executables {
                    bail!("state/metadata mismatch");
                }
                self.verify_generation(&g)?;
                for exe in &g.executables {
                    if digest(&self.root.join("bin").join(exe))? != g.hashes[exe] {
                        bail!("command entry mismatch: {exe}");
                    }
                }
                let m = self.manifest(&name)?;
                if m.version != state.version {
                    eprintln!("warning: {name} YAML version differs from installed version");
                }
                Ok(())
            })();
            match check {
                Ok(()) => println!("{name}: OK"),
                Err(e) => failures.push(format!("{name}: {e:#}")),
            }
        }
        if !failures.is_empty() {
            bail!("installation problems:\n{}", failures.join("\n"));
        }
        println!("doctor: OK");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_switch_restores_files_and_removes_new_entries() {
        let temp = tempfile::tempdir().unwrap();
        let old = temp.path().join("state.json");
        let new = temp.path().join("new-command");
        fs::write(&old, b"old-state").unwrap();
        let snapshots = vec![
            (old.clone(), Snapshot::capture(&old).unwrap()),
            (new.clone(), Snapshot::capture(&new).unwrap()),
        ];
        fs::write(&old, b"new-state").unwrap();
        fs::write(&new, b"new-binary").unwrap();
        let result = recover(
            &snapshots,
            Err(anyhow::anyhow!("simulated publish failure")),
        );
        assert!(result.is_err());
        assert_eq!(fs::read(old).unwrap(), b"old-state");
        assert!(!new.exists());
    }

    #[cfg(unix)]
    #[test]
    fn failed_switch_restores_old_symlink() {
        let temp = tempfile::tempdir().unwrap();
        let old = temp.path().join("old");
        let new = temp.path().join("new");
        let link = temp.path().join("command");
        fs::write(&old, b"old-binary").unwrap();
        fs::write(&new, b"new-binary").unwrap();
        publish(&old, &link).unwrap();
        let snapshot = Snapshot::capture(&link).unwrap();
        publish(&new, &link).unwrap();
        assert!(recover(&[(link.clone(), snapshot)], Err(anyhow::anyhow!("failure"))).is_err());
        assert_eq!(fs::read_link(&link).unwrap(), old);
        assert_eq!(fs::read(link).unwrap(), b"old-binary");
    }
}
