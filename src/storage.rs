//! Filesystem safety, atomic publication, archive extraction and checksums.
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
};

pub(crate) fn validate_name(name: &str) -> Result<()> {
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

pub(crate) fn validate_version(version: &str) -> Result<()> {
    validate_name(version).context("version must be a safe single path component")
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().context("missing parent")?)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

pub(crate) fn publish(source: &Path, target: &Path) -> Result<()> {
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

pub(crate) fn remove_file_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
pub(crate) fn verify_checksum(path: &Path, filename: &str, text: &str) -> Result<()> {
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

pub(crate) fn safe_archive_path(path: &Path) -> Result<()> {
    if path.as_os_str().to_string_lossy().contains('\\')
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        bail!("unsafe archive path: {}", path.display());
    }
    Ok(())
}

pub(crate) fn copy_binary(
    reader: &mut impl Read,
    path: &str,
    wanted: &[String],
    output: &Path,
) -> Result<()> {
    let Some(name) = wanted.iter().find(|w| *w == path || path_matches(w, path)) else {
        return Ok(());
    };
    let basename = name.rsplit(['/', '\\']).next().unwrap_or(name.as_str());
    let target = output.join(basename);
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&target)
        .with_context(|| format!("duplicate executable in archive: {basename}"))?;
    io::copy(reader, &mut file)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn path_matches(want: &str, path: &str) -> bool {
    let Some(basename) = path.rsplit(['/', '\\']).next() else {
        return false;
    };
    let want_basename = want.rsplit(['/', '\\']).next().unwrap_or(want);
    basename == want_basename
}

pub(crate) fn extract(
    archive: &Path,
    format: &str,
    wanted: &[String],
    output: &Path,
) -> Result<()> {
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
        "tar.xz" => {
            let decoder = xz2::read::XzDecoder::new(fs::File::open(archive)?);
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
        other => bail!("unsupported archive format: {other}; use zip, tar.gz, tar.xz or raw"),
    }
    for exe in wanted {
        if !output.join(exe).is_file() {
            bail!("archive does not contain executable {exe}");
        }
    }
    Ok(())
}
