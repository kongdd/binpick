//! YAML models and automatic discovery of bundled manifests.
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    pub(crate) name: String,
    /// Exact upstream tag, including a leading v if present
    pub(crate) version: String,
    pub(crate) source: Source,
    pub(crate) assets: BTreeMap<String, Asset>,
    pub(crate) executables: Vec<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub(crate) pinned: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Source {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) github: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Asset {
    pub(crate) file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) format: Option<String>,
    /// Release asset containing sha256sum-style checksums
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) checksum: Option<String>,
    /// URL template for the checksum file when source is not a GitHub release
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) checksum_url: Option<String>,
    /// Per-asset URL template override when the source.url pattern needs
    /// platform-specific fragments (e.g. node uses `linux-x64` while binpick
    /// calls the platform `linux-amd64-gnu`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) url: Option<String>,
    /// Minimum glibc version; selects musl fallback when unavailable/too old
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) min_glibc: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Installed {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) executables: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) generation: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct Release {
    pub(crate) tag_name: String,
    pub(crate) draft: bool,
    pub(crate) prerelease: bool,
    pub(crate) assets: Vec<ReleaseAsset>,
}
#[derive(Deserialize)]
pub(crate) struct ReleaseAsset {
    pub(crate) name: String,
    pub(crate) browser_download_url: String,
}
include!(concat!(env!("OUT_DIR"), "/catalog.rs"));

pub(crate) fn seed(directory: &Path) -> Result<()> {
    for &(filename, yaml) in BUNDLED {
        let path = directory.join(filename);
        if !path.exists() {
            crate::storage::atomic_write(&path, yaml.as_bytes())?;
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
