mod common;
use common::{Fixture, Server};
use std::fs;

#[test]
fn install_update_remove() {
    let f = Fixture::new(false);
    f.cmd().args(["install", "demo"]).assert().success();
    assert_eq!(fs::read(f.binary()).unwrap(), b"/download/v1");
    assert_eq!(f.version(), "v1");
    f.cmd().args(["update", "demo"]).assert().success();
    assert_eq!(f.version(), "v2");
    assert_eq!(fs::read(f.binary()).unwrap(), b"/download/v2");
    let state: serde_json::Value =
        serde_json::from_slice(&fs::read(f.dir.path().join("data/state/demo.json")).unwrap())
            .unwrap();
    assert_eq!(state["version"], "v2");
    f.cmd().args(["list"]).assert().success();
    f.cmd().args(["update", "demo"]).assert().success();
    f.cmd().args(["remove", "demo"]).assert().success();
    assert!(!f.binary().exists());
    assert_eq!(f.version(), "v2");
}

#[test]
fn uninstalled_update_only_changes_yaml() {
    let f = Fixture::new(false);
    f.cmd().args(["update", "demo"]).assert().success();
    assert_eq!(f.version(), "v2");
    assert!(!f.binary().exists());
}

#[test]
fn failed_update_keeps_yaml_and_installed_binary() {
    let mut f = Fixture::new(false);
    f.cmd().args(["install", "demo"]).assert().success();
    f.server = Server::start(true);
    f.cmd().args(["update", "demo"]).assert().failure();
    assert_eq!(f.version(), "v1");
    assert_eq!(fs::read(f.binary()).unwrap(), b"/download/v1");
}

#[test]
fn rejects_unmanaged_binary_collision() {
    let f = Fixture::new(false);
    f.cmd().arg("init").assert().success();
    fs::write(f.binary(), b"unmanaged").unwrap();
    f.cmd().args(["install", "demo"]).assert().failure();
    assert_eq!(fs::read(f.binary()).unwrap(), b"unmanaged");
}

#[test]
fn runtime_manifest_needs_no_code_registration() {
    let f = Fixture::new(false);
    let manifests = f.dir.path().join("manifests");
    let yaml = fs::read_to_string(manifests.join("demo.yaml")).unwrap();
    fs::write(
        manifests.join("new-tool.yaml"),
        yaml.replacen("name: demo", "name: new-tool", 1),
    )
    .unwrap();
    f.cmd().args(["install", "new-tool"]).assert().success();
    assert_eq!(fs::read(f.binary()).unwrap(), b"/download/v1");
    f.cmd().args(["update", "new-tool"]).assert().success();
    let state: serde_json::Value =
        serde_json::from_slice(&fs::read(f.dir.path().join("data/state/new-tool.json")).unwrap())
            .unwrap();
    assert_eq!(state["name"], "new-tool");
    assert_eq!(state["version"], "v2");
}
