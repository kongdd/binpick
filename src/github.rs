use crate::{
    app::App,
    files::validate_version,
    model::{Manifest, Release},
};
use anyhow::{bail, Context, Result};

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
}
