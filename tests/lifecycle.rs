mod common;
use common::{Fixture, Server};
use std::fs;

#[test]
fn disconnected_clients_do_not_stop_server() {
    use std::{io::Write, net::TcpStream};

    let f = Fixture::new(false);
    // Obtain the fixture URL from the command's environment without exposing
    // additional server internals.
    let cmd = f.cmd();
    let url = cmd
        .get_envs()
        .find(|(key, _)| *key == "BINPICK_GITHUB_API")
        .and_then(|(_, value)| value)
        .unwrap()
        .to_str()
        .unwrap();
    let address = url.strip_prefix("http://").unwrap();
    drop(TcpStream::connect(address).unwrap());
    let mut stream = TcpStream::connect(address).unwrap();
    stream.write_all(b"GET /download/v1 HTTP/1.1\r\n").unwrap();
    drop(stream);
    f.cmd().args(["install", "demo"]).assert().success();
}

#[test]
fn server_waits_for_fragmented_request_headers() {
    use std::{
        io::{Read, Write},
        net::TcpStream,
        thread,
        time::Duration,
    };

    let f = Fixture::new(false);
    let cmd = f.cmd();
    let url = cmd
        .get_envs()
        .find(|(key, _)| *key == "BINPICK_GITHUB_API")
        .and_then(|(_, value)| value)
        .unwrap()
        .to_str()
        .unwrap();
    let mut stream = TcpStream::connect(url.strip_prefix("http://").unwrap()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.write_all(b"GET /download/v1 HTTP/1.1\r\n").unwrap();
    thread::sleep(Duration::from_millis(100));
    stream.write_all(b"Host: localhost\r\n\r\n").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(response.ends_with("/download/v1"));
}

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

fn node_archive(format: &str, version: &str) -> Vec<u8> {
    use std::io::{Cursor, Write};

    let binary = if cfg!(windows) { "node.exe" } else { "node" };
    let path = format!("node-{version}/bin/{binary}");
    let contents = format!("fixture-node-{version}");
    if format == "zip" {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(contents.as_bytes()).unwrap();
        return zip.finish().unwrap().into_inner();
    }
    let mut tar = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(contents.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    tar.append_data(&mut header, path, contents.as_bytes())
        .unwrap();
    let bytes = tar.into_inner().unwrap();
    match format {
        "tar.xz" => {
            let mut encoder = xz2::write::XzEncoder::new(Vec::new(), 1);
            encoder.write_all(&bytes).unwrap();
            encoder.finish().unwrap()
        }
        "tar.gz" => {
            let mut encoder =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            encoder.write_all(&bytes).unwrap();
            encoder.finish().unwrap()
        }
        _ => panic!("unexpected fixture format: {format}"),
    }
}

fn node_fixture(bad_checksum: bool) -> Fixture {
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;

    let mut routes = BTreeMap::new();
    // Serve exact upstream-style paths, independent of the manifest templates.
    // The next version deliberately has a bad checksum to exercise failed updates.
    for version in ["v1.2.3", "v1.2.4"] {
        let mut sums = String::new();
        for (suffix, format) in [
            ("linux-x64", "tar.xz"),
            ("linux-arm64", "tar.xz"),
            ("linux-x64-musl", "tar.xz"),
            ("darwin-x64", "tar.gz"),
            ("darwin-arm64", "tar.gz"),
            ("win-x64", "zip"),
            ("win-arm64", "zip"),
        ] {
            let archive = node_archive(format, version);
            let filename = format!("node-{version}-{suffix}.{format}");
            let sum = if bad_checksum || version == "v1.2.4" {
                "0".repeat(64)
            } else {
                format!("{:x}", Sha256::digest(&archive))
            };
            sums.push_str(&format!("{sum}  {filename}\n"));
            routes.insert(format!("/dist/{version}/{filename}"), archive);
        }
        routes.insert(format!("/dist/{version}/SHASUMS256.txt"), sums.into_bytes());
    }
    let mut f = Fixture::new(false);
    f.server = Server::with_handler(move |path, _| match routes.get(path) {
        Some(body) => ("200 OK", body.clone()),
        None => ("404 Not Found", Vec::new()),
    });
    let yaml = include_str!("../manifests/node.yaml").replace("https://nodejs.org", &f.server.url);
    let mut manifest: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    manifest["version"] = "v1.2.3".into();
    fs::write(
        f.dir.path().join("manifests/node.yaml"),
        serde_yaml::to_string(&manifest).unwrap(),
    )
    .unwrap();
    f
}

fn node_binary(f: &Fixture) -> std::path::PathBuf {
    f.dir
        .path()
        .join("data/bin")
        .join(if cfg!(windows) { "node.exe" } else { "node" })
}

#[test]
fn bundled_node_installs_with_checksum_and_nested_archive() {
    let f = node_fixture(false);
    let result = f.cmd().args(["install", "node"]).assert().success();
    assert!(result.get_output().stderr.is_empty());
    assert_eq!(fs::read(node_binary(&f)).unwrap(), b"fixture-node-v1.2.3");
    f.cmd().args(["doctor"]).assert().success();
}

#[test]
fn node_checksum_mismatch_does_not_publish_installation() {
    let f = node_fixture(true);
    let result = f.cmd().args(["install", "node"]).assert().failure();
    assert!(String::from_utf8_lossy(&result.get_output().stderr).contains("SHA-256 mismatch"));
    assert!(!node_binary(&f).exists());
    assert!(!f.dir.path().join("data/state/node.json").exists());
    assert!(!f.dir.path().join("data/packages/node").exists());
}

#[test]
fn failed_url_update_preserves_installed_node_and_manifest() {
    let f = node_fixture(false);
    f.cmd().args(["install", "node"]).assert().success();
    let state_path = f.dir.path().join("data/state/node.json");
    let old_state = fs::read(&state_path).unwrap();
    let manifest_path = f.dir.path().join("manifests/node.yaml");
    let yaml = fs::read_to_string(&manifest_path)
        .unwrap()
        .replace("v1.2.3", "v1.2.4");
    fs::write(&manifest_path, &yaml).unwrap();
    let result = f.cmd().args(["update", "node"]).assert().failure();
    assert!(String::from_utf8_lossy(&result.get_output().stderr).contains("SHA-256 mismatch"));
    assert_eq!(fs::read(node_binary(&f)).unwrap(), b"fixture-node-v1.2.3");
    assert_eq!(fs::read(&state_path).unwrap(), old_state);
    assert_eq!(fs::read_to_string(&manifest_path).unwrap(), yaml);
}

#[test]
fn url_source_downloads_without_github_or_asset_url_override() {
    let f = Fixture::new(false);
    let manifest_path = f.dir.path().join("manifests/demo.yaml");
    let yaml = fs::read_to_string(&manifest_path).unwrap().replace(
        "github: test/demo",
        &format!("url: {}/download/{{tag}}", f.server.url),
    );
    fs::write(&manifest_path, &yaml).unwrap();
    f.cmd().args(["install", "demo"]).assert().success();
    assert_eq!(fs::read(f.binary()).unwrap(), b"/download/v1");
    fs::write(&manifest_path, yaml.replace("version: v1", "version: v2")).unwrap();
    f.cmd().args(["update", "demo"]).assert().success();
    assert_eq!(fs::read(f.binary()).unwrap(), b"/download/v2");
}
