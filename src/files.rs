use anyhow::{bail, Context, Result};
use std::{
    fs,
    io::{self, Write},
    path::Path,
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
