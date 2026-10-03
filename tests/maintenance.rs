mod common;
use common::Fixture;
use std::fs;

fn install_two(f: &Fixture) {
    f.cmd().args(["install", "demo"]).assert().success();
    f.cmd().args(["update", "demo"]).assert().success();
}
fn count(f: &Fixture) -> usize {
    fs::read_dir(f.dir.path().join("data/packages/demo"))
        .unwrap()
        .count()
}

#[test]
fn offline_rollback_and_explicit_version() {
    let f = Fixture::new(false);
    install_two(&f);
    f.cmd().args(["history", "demo"]).assert().success();
    f.cmd()
        .args(["rollback", "demo"])
        .env("BINPICK_GITHUB_API", "http://127.0.0.1:1")
        .assert()
        .success();
    assert_eq!(f.version(), "v1");
    assert_eq!(fs::read(f.binary()).unwrap(), b"/download/v1");
    f.cmd()
        .args(["rollback", "demo", "--version", "v2"])
        .assert()
        .success();
    assert_eq!(f.version(), "v2");
    f.cmd().args(["doctor"]).assert().success();
}

#[test]
fn gc_keeps_active_even_when_it_is_older() {
    let f = Fixture::new(false);
    install_two(&f);
    f.cmd().args(["rollback", "demo"]).assert().success();
    assert_eq!(count(&f), 2);
    f.cmd()
        .args(["gc", "demo", "--keep", "1", "--dry-run"])
        .assert()
        .success();
    assert_eq!(count(&f), 2);
    f.cmd()
        .args(["gc", "demo", "--keep", "0"])
        .assert()
        .failure();
    assert_eq!(count(&f), 2);
    f.cmd()
        .args(["gc", "demo", "--keep", "1"])
        .assert()
        .success();
    assert_eq!(count(&f), 1);
    assert_eq!(fs::read(f.binary()).unwrap(), b"/download/v1");
    f.cmd()
        .args(["rollback", "demo", "--version", "v2"])
        .assert()
        .failure();
    f.cmd().args(["doctor"]).assert().success();
}

#[test]
fn pin_prevents_network_and_version_changes() {
    let f = Fixture::new(false);
    f.cmd().args(["install", "demo"]).assert().success();
    f.cmd().args(["pin", "demo"]).assert().success();
    f.cmd()
        .args(["update", "demo"])
        .env("BINPICK_GITHUB_API", "http://127.0.0.1:1")
        .assert()
        .success();
    assert_eq!(f.version(), "v1");
    f.cmd().args(["unpin", "demo"]).assert().success();
    f.cmd().args(["update", "demo"]).assert().success();
    assert_eq!(f.version(), "v2");
}

#[test]
fn corrupted_retained_binary_cannot_be_activated() {
    let f = Fixture::new(false);
    install_two(&f);
    for entry in fs::read_dir(f.dir.path().join("data/packages/demo")).unwrap() {
        let path = entry.unwrap().path();
        let metadata: serde_json::Value =
            serde_json::from_slice(&fs::read(path.join(".binpick-generation.json")).unwrap())
                .unwrap();
        if metadata["version"] == "v1" {
            let exe = if cfg!(windows) { "demo.exe" } else { "demo" };
            fs::write(path.join(exe), b"tampered").unwrap();
        }
    }
    f.cmd().args(["rollback", "demo"]).assert().failure();
    assert_eq!(f.version(), "v2");
    assert_eq!(fs::read(f.binary()).unwrap(), b"/download/v2");
    f.cmd().args(["doctor"]).assert().success();
}

#[test]
fn doctor_detects_broken_command() {
    let f = Fixture::new(false);
    f.cmd().args(["install", "demo"]).assert().success();
    fs::remove_file(f.binary()).unwrap();
    f.cmd().args(["doctor"]).assert().failure();
}

#[test]
fn legacy_state_migrates_without_reinstall() {
    let f = Fixture::new(false);
    f.cmd().args(["install", "demo"]).assert().success();
    let state_path = f.dir.path().join("data/state/demo.json");
    let mut state: serde_json::Value =
        serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    state.as_object_mut().unwrap().remove("generation");
    fs::write(&state_path, serde_json::to_vec(&state).unwrap()).unwrap();
    for entry in fs::read_dir(f.dir.path().join("data/packages/demo")).unwrap() {
        fs::remove_file(entry.unwrap().path().join(".binpick-generation.json")).unwrap();
    }
    f.cmd()
        .args(["doctor"])
        .env("BINPICK_GITHUB_API", "http://127.0.0.1:1")
        .assert()
        .success();
    let state: serde_json::Value = serde_json::from_slice(&fs::read(state_path).unwrap()).unwrap();
    assert!(state["generation"].is_string());
}

#[test]
fn gc_leaves_untracked_directories_untouched() {
    let f = Fixture::new(false);
    install_two(&f);
    let untracked = f.dir.path().join("data/packages/demo/legacy-unknown");
    fs::create_dir(&untracked).unwrap();
    fs::write(untracked.join("notes"), b"do not delete").unwrap();
    f.cmd()
        .args(["gc", "demo", "--keep", "1"])
        .assert()
        .success();
    assert!(untracked.join("notes").exists());
}

#[test]
fn reinstall_same_version_does_not_create_generation() {
    let f = Fixture::new(false);
    f.cmd().args(["install", "demo"]).assert().success();
    f.cmd()
        .args(["install", "demo"])
        .env("BINPICK_GITHUB_API", "http://127.0.0.1:1")
        .assert()
        .success();
    assert_eq!(count(&f), 1);
}
