use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
    pub(crate) github: String,
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
