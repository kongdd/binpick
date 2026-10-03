use crate::{
    app::App,
    artifacts::{extract, safe_archive_path, verify_checksum},
    catalog,
    files::validate_name,
    model::Manifest,
    platform::{numeric_version, render, select_asset_for},
};
use reqwest::blocking::Client;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
#[test]
fn templates() {
    assert_eq!(render("pkg_{version}_{tag}", "v1.2.3"), "pkg_1.2.3_v1.2.3");
}
#[test]
fn invalid_paths() {
    for name in ["", "..", "../x", "a/b", "a\\b", "CON", "NUL.exe", "COM1"] {
        assert!(validate_name(name).is_err());
    }
    for path in ["../evil", "/evil", "a/../../evil", "a\\evil"] {
        assert!(safe_archive_path(Path::new(path)).is_err());
    }
    assert!(safe_archive_path(Path::new("folder/bin/tool")).is_ok());
}
#[test]
fn bundled_manifests() {
    let directory = tempfile::tempdir().unwrap();
    let app = App {
        root: directory.path().join("data"),
        manifests: directory.path().join("manifests"),
        client: Client::new(),
    };
    app.init().unwrap();
    assert_eq!(app.names().unwrap().len(), catalog::BUNDLED.len());
    for &(filename, _) in catalog::BUNDLED {
        let name = Path::new(filename).file_stem().unwrap().to_str().unwrap();
        let m = app.manifest(name).unwrap();
        assert!(!m.assets.is_empty(), "{filename}: assets cannot be empty");
        for asset in m.assets.values() {
            assert!(
                !asset.file.is_empty(),
                "{filename}: asset file cannot be empty"
            );
            if let Some(min) = &asset.min_glibc {
                numeric_version(min).unwrap();
            }
        }
    }
}
#[test]
fn static_linux_assets() {
    let m: Manifest = serde_yaml::from_str("name: demo\nversion: v1\nsource:\n  github: test/demo\nexecutables: [demo]\nassets:\n  linux-amd64-musl:\n    file: demo-linux-x86_64\n    format: raw\n  linux-arm64-musl:\n    file: demo-linux-aarch64\n    format: raw\n").unwrap();
    for arch in ["amd64", "arm64"] {
        let key = format!("linux-{arch}-musl");
        let (selected, asset) =
            select_asset_for(&m, &format!("linux-{arch}-gnu"), Some("2.36")).unwrap();
        assert_eq!(selected, key);
        assert_eq!(asset.format.as_deref(), Some("raw"));
        assert_eq!(select_asset_for(&m, &key, None).unwrap().0, key);
    }
    assert!(select_asset_for(&m, "windows-arm64", None).is_err());
}
#[test]
fn glibc_fallback() {
    let m: Manifest = serde_yaml::from_str("name: demo\nversion: v1\nsource:\n  github: test/demo\nexecutables: [demo]\nassets:\n  linux-amd64-gnu:\n    file: demo-gnu.zip\n    min_glibc: '2.39'\n  linux-amd64-musl:\n    file: demo-musl.zip\n").unwrap();
    assert_eq!(
        select_asset_for(&m, "linux-amd64-gnu", Some("2.36"))
            .unwrap()
            .0,
        "linux-amd64-musl"
    );
    assert_eq!(
        select_asset_for(&m, "linux-amd64-gnu", Some("2.39"))
            .unwrap()
            .0,
        "linux-amd64-gnu"
    );
    assert_eq!(
        select_asset_for(&m, "linux-amd64-gnu", None).unwrap().0,
        "linux-amd64-musl"
    );
    assert!(numeric_version("not-a-version").is_err());
}
#[test]
fn checksum() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("asset");
    fs::write(&path, b"abc").unwrap();
    let sum = format!("{:x}  *asset", Sha256::digest(b"abc"));
    verify_checksum(&path, "asset", &sum).unwrap();
    assert!(verify_checksum(&path, "different", &sum).is_err());
    fs::write(&path, b"tampered").unwrap();
    assert!(verify_checksum(&path, "asset", &sum).is_err());
}
#[test]
fn raw_extraction_and_missing_binary() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("file");
    let out = tmp.path().join("out");
    fs::create_dir(&out).unwrap();
    fs::write(&file, b"binary").unwrap();
    extract(&file, "raw", &["tool".into()], &out).unwrap();
    assert_eq!(fs::read(out.join("tool")).unwrap(), b"binary");
    assert!(extract(&file, "raw", &["a".into(), "b".into()], &out).is_err());
}
