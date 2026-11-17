#![cfg(feature = "iris-client")]
use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "iris-cli-profiles-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_iris"))
            .env("XDG_CONFIG_HOME", &self.0)
            .env_remove("IRIS_CONFIG")
            .env_remove("IRIS_ENDPOINT")
            .env_remove("IRIS_CONTROL_ADDRESS")
            .env_remove("IRIS_TOKEN")
            .env_remove("IRIS_TOKEN_FILE")
            .args(args)
            .output()
            .unwrap()
    }
    fn ok(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn configuration_is_saved_offline_and_named_commands_use_the_saved_endpoint_and_token() {
    let dir = Temp::new();
    dir.ok(&["config"]);
    let config_path = dir.0.join("iris/config.toml");
    assert!(config_path.is_file());
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let token_path = dir.0.join("camera.token");
    fs::write(&token_path, "test-credential\n").unwrap();
    dir.ok(&[
        "config",
        "front",
        "--endpoint",
        &endpoint,
        "--token-file",
        token_path.to_str().unwrap(),
    ]);
    dir.ok(&["config", "rear", "--endpoint", "http://127.0.0.1:9"]);
    let saved = dir.ok(&["config", "--show"]);
    assert_eq!(dir.ok(&["show"]), saved);
    assert!(
        saved.contains("[cameras.front]")
            && saved.contains("[cameras.rear]")
            && saved.contains("[cameras.default]")
    );
    assert!(!saved.contains("test-credential"));
    assert!(!dir.run(&["camera", "missing", "start"]).status.success());
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for state in ["Running", "Stopped"] {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut bytes = [0; 4096];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let size = socket.read(&mut bytes).unwrap();
                assert!(size > 0);
                request.extend_from_slice(&bytes[..size]);
            }
            requests.push(String::from_utf8(request).unwrap());
            let status = serde_json::json!({
                "camera": { "camera_state": state, "frame_count": 0, "width": 1280, "height": 720,
                    "pixel_format": "NV12", "latest_frame": null, "last_error": null, "frame_age_ms": null, "frame_stale": true },
                "requested_settings": {"camera_index": 0, "width":1280, "height":720, "fps":30, "exposure_us":59000, "gain":1.5, "jpeg_quality":85},
                "capture_active": state == "Running"
            }).to_string();
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{status}", status.len()).unwrap();
        }
        requests
    });
    assert!(dir.ok(&["camera", "front", "start"]).contains("Running"));
    assert!(dir.ok(&["camera", "front", "stop"]).contains("Stopped"));
    let requests = server.join().unwrap();
    assert!(requests[0].starts_with("POST /camera/start "));
    assert!(requests[1].starts_with("POST /camera/stop "));
    for request in requests {
        assert!(request
            .to_lowercase()
            .contains("authorization: bearer test-credential"));
        assert!(request.to_lowercase().contains("x-iris-control: 1"));
    }
    // Malformed TOML is reported and preserved, rather than replaced with defaults.
    fs::write(&config_path, "not [TOML").unwrap();
    assert!(!dir
        .run(&["config", "front", "--endpoint", &endpoint])
        .status
        .success());
    assert_eq!(fs::read_to_string(config_path).unwrap(), "not [TOML");
}

#[test]
fn ping_uses_default_or_named_profiles_without_writing_config() {
    let dir = Temp::new();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    dir.ok(&["config", "--endpoint", &endpoint]);
    dir.ok(&["config", "front", "--endpoint", &endpoint]);
    let before = fs::read(dir.0.join("iris/config.toml")).unwrap();
    let server = std::thread::spawn(move || {
        for _ in 0..3 {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let size = socket.read(&mut buffer).unwrap();
                assert!(size > 0);
                request.extend_from_slice(&buffer[..size]);
            }
            assert!(request.starts_with(b"GET /ping "));
            socket
                .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                .unwrap();
        }
    });
    for command in [
        &["ping"][..],
        &["ping", "front"][..],
        &["camera", "front", "ping"][..],
    ] {
        let output = dir.ok(command);
        assert!(
            output.starts_with(&format!("pong {endpoint} (")),
            "{output}"
        );
        assert!(output.trim_end().ends_with(" ms)"));
    }
    server.join().unwrap();
    assert_eq!(fs::read(dir.0.join("iris/config.toml")).unwrap(), before);
}
