//! Bundled manifests are generated from manifests/*.yaml, never manually listed.
//! Runtime manifests remain ordinary YAML files and can be added without rebuilding.
use anyhow::Result;
use std::path::Path;

include!(concat!(env!("OUT_DIR"), "/catalog.rs"));

pub(crate) fn seed(directory: &Path) -> Result<()> {
    for &(filename, yaml) in BUNDLED {
        let path = directory.join(filename);
        if !path.exists() {
            crate::files::atomic_write(&path, yaml.as_bytes())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_matches_directory() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("manifests");
        let mut expected: Vec<_> = std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "yaml"))
            .map(|path| path.file_name().unwrap().to_str().unwrap().to_owned())
            .collect();
        expected.sort();
        let actual: Vec<_> = BUNDLED
            .iter()
            .map(|(filename, _)| filename.to_string())
            .collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn seed_preserves_existing_manifests() {
        let directory = tempfile::tempdir().unwrap();
        seed(directory.path()).unwrap();
        for &(filename, _) in BUNDLED {
            std::fs::write(directory.path().join(filename), "user configuration").unwrap();
        }
        seed(directory.path()).unwrap();
        for &(filename, _) in BUNDLED {
            assert_eq!(
                std::fs::read_to_string(directory.path().join(filename)).unwrap(),
                "user configuration"
            );
        }
    }
}
