use assert_cmd::Command;
use serde_json::json;
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

pub(crate) struct Server {
    url: String,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Server {
    pub(crate) fn start(fail_download: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let base = url.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = thread::spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(e) => panic!("{e}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                while !request.windows(4).any(|b| b == b"\r\n\r\n") {
                    let n = stream.read(&mut buffer).unwrap();
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..n]);
                }
                let request = String::from_utf8_lossy(&request);
                let path = request.split_whitespace().nth(1).unwrap_or("");
                let (status, body) = if path.starts_with("/repos/test/demo/releases/") {
                    let version = if path.ends_with("latest") { "v2" } else { "v1" };
                    ("200 OK", json!({
                        "tag_name": version, "draft": false, "prerelease": false,
                        "assets": [{"name": "demo.bin", "browser_download_url": format!("{base}/download/{version}")}]
                    }).to_string().into_bytes())
                } else if path.starts_with("/download/") {
                    if fail_download {
                        ("500 Internal Server Error", b"failure".to_vec())
                    } else {
                        ("200 OK", path.as_bytes().to_vec())
                    }
                } else {
                    ("404 Not Found", Vec::new())
                };
                write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
                stream.write_all(&body).unwrap();
            }
        });
        Self {
            url,
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}

pub(crate) struct Fixture {
    pub(crate) dir: tempfile::TempDir,
    pub(crate) server: Server,
}
impl Fixture {
    pub(crate) fn new(fail_download: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let manifests = dir.path().join("manifests");
        fs::create_dir(&manifests).unwrap();
        let mut yaml =
            "name: demo\nversion: v1\nsource:\n  github: test/demo\nassets:\n".to_string();
        for platform in [
            "linux-amd64-gnu",
            "linux-arm64-gnu",
            "linux-amd64-musl",
            "linux-arm64-musl",
            "darwin-amd64",
            "darwin-arm64",
            "windows-amd64",
            "windows-arm64",
        ] {
            yaml.push_str(&format!(
                "  {platform}:\n    file: demo.bin\n    format: raw\n"
            ));
        }
        yaml.push_str("executables:\n  - demo\n");
        fs::write(manifests.join("demo.yaml"), yaml).unwrap();
        Self {
            dir,
            server: Server::start(fail_download),
        }
    }
    pub(crate) fn cmd(&self) -> Command {
        let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("binpick"));
        cmd.arg("--root")
            .arg(self.dir.path().join("data"))
            .arg("--manifests")
            .arg(self.dir.path().join("manifests"))
            .env("BINPICK_GITHUB_API", &self.server.url)
            .env("NO_PROXY", "*")
            .env("no_proxy", "*")
            .env_remove("GITHUB_TOKEN")
            .env_remove("http_proxy")
            .env_remove("https_proxy")
            .env_remove("all_proxy")
            .env_remove("HTTP_PROXY")
            .env_remove("HTTPS_PROXY")
            .env_remove("ALL_PROXY");
        cmd
    }
    pub(crate) fn version(&self) -> String {
        let text = fs::read_to_string(self.dir.path().join("manifests/demo.yaml")).unwrap();
        let yaml: serde_yaml::Value = serde_yaml::from_str(&text).unwrap();
        yaml["version"].as_str().unwrap().to_owned()
    }
    pub(crate) fn binary(&self) -> std::path::PathBuf {
        self.dir
            .path()
            .join("data/bin")
            .join(if cfg!(windows) { "demo.exe" } else { "demo" })
    }
}
