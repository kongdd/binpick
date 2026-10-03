use crate::manifest::{Asset, Manifest};
use anyhow::{bail, Context, Result};

pub(crate) fn platform() -> Result<String> {
    let os = match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "darwin",
        "windows" => "windows",
        other => bail!("unsupported OS: {other}"),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => bail!("unsupported architecture: {other}"),
    };
    // On Linux, select explicit libc assets; do not pretend musl and glibc are interchangeable.
    let libc = if os == "linux" {
        if cfg!(target_env = "musl") {
            "-musl"
        } else {
            "-gnu"
        }
    } else {
        ""
    };
    Ok(format!("{os}-{arch}{libc}"))
}

pub(crate) fn glibc_version() -> Option<String> {
    let output = std::process::Command::new("getconf")
        .arg("GNU_LIBC_VERSION")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()?
        .split_whitespace()
        .nth(1)
        .map(str::to_owned)
}

pub(crate) fn numeric_version(version: &str) -> Result<Vec<u32>> {
    let values: Vec<u32> = version
        .split('.')
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()
        .context("invalid numeric glibc version")?;
    if values.len() != 2 {
        bail!("glibc version must be major.minor");
    }
    Ok(values)
}

pub(crate) fn select_asset(m: &Manifest) -> Result<(String, &Asset)> {
    select_asset_for(m, &platform()?, glibc_version().as_deref())
}

pub(crate) fn select_asset_for<'a>(
    m: &'a Manifest,
    platform: &str,
    glibc: Option<&str>,
) -> Result<(String, &'a Asset)> {
    if let Some(asset) = m.assets.get(platform) {
        let compatible = match &asset.min_glibc {
            Some(min) if platform.ends_with("-gnu") => {
                let min = numeric_version(min)?;
                glibc
                    .map(numeric_version)
                    .transpose()?
                    .is_some_and(|current| current >= min)
            }
            _ => true,
        };
        if compatible {
            return Ok((platform.to_owned(), asset));
        }
    }
    if let Some(prefix) = platform.strip_suffix("-gnu") {
        let fallback = format!("{prefix}-musl");
        if let Some(asset) = m.assets.get(&fallback) {
            eprintln!(
                "{}: GNU asset unavailable/incompatible; selecting {fallback}",
                m.name
            );
            return Ok((fallback, asset));
        }
    }
    bail!(
        "{} has no compatible asset for {platform} (glibc: {}); check assets/min_glibc",
        m.name,
        glibc.unwrap_or("unknown")
    )
}

pub(crate) fn binary_name(name: &str) -> String {
    if cfg!(windows) && !name.ends_with(".exe") {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

pub(crate) fn render(template: &str, tag: &str) -> String {
    template
        .replace("{tag}", tag)
        .replace("{version}", tag.strip_prefix('v').unwrap_or(tag))
}
