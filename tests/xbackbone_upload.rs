//! Integration tests for the XBackBone upload feature.
//!
//! These spawn a mock XBackBone HTTP server (see `tests/support/mock_xbackbone.py`)
//! that mimics both the 3.x and 4.x APIs, then drive ApexShot's REAL
//! `upload_file()` / `test_connection()` code paths over HTTP. This covers the
//! version auto-detection and wire-level behaviour that the unit tests skip.

use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use apexshot::cloud::upload::{is_configured, upload_file, UploadError};
use apexshot::cloud::xbackbone::test_connection;
use apexshot::config::AppConfig;

const VALID_TOKEN: &str = "good-token";

/// Kills the child process when dropped so test servers never leak.
struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Spawn the mock XBackBone server in `mode` and return its port.
fn spawn_mock(mode: &str) -> (u16, ChildGuard) {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("support")
        .join("mock_xbackbone.py");

    let mut child = Command::new("python3")
        .arg(&script)
        .arg(mode)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to spawn mock server ({mode}): {e}"));

    let mut stdout = child.stdout.take().expect("piped stdout");
    // The server prints the port as the first line of stdout (flushed) before
    // serving. A blocking read waits for it.
    let mut buf = [0u8; 64];
    let n = stdout.read(&mut buf).expect("read port from mock server");
    let line = std::str::from_utf8(&buf[..n])
        .unwrap_or("")
        .lines()
        .next()
        .unwrap_or("");
    let port: u16 = line
        .trim()
        .parse()
        .unwrap_or_else(|e| panic!("failed to parse mock server port ({mode}): {e}"));

    // Give the listener a moment to be ready.
    thread::sleep(Duration::from_millis(50));

    (port, ChildGuard(child))
}

/// A minimal config pointing at the mock server with the XBackBone destination.
fn xb_config(port: u16, token: &str) -> AppConfig {
    AppConfig {
        cloud_destination: "xbackbone".to_string(),
        xbackbone_url: format!("http://127.0.0.1:{port}"),
        xbackbone_api_token: token.to_string(),
        ..AppConfig::default()
    }
}

/// Write a small temp file named `shot.png` and return its path.
fn write_temp_png() -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("apexshot-xb-test-{}-shot.png", std::process::id()));
    // A tiny payload; the mock server validates multipart structure, not image
    // contents, and ApexShot derives the content type from the `.png` suffix.
    std::fs::write(&path, b"fake-png-bytes-for-testing").expect("write temp png");
    path
}

// --- 4.x (Laravel + Sanctum) ---

#[test]
fn v4_upload_returns_preview_url() {
    let (port, _guard) = spawn_mock("v4");
    let config = xb_config(port, VALID_TOKEN);
    let path = write_temp_png();

    let result = upload_file(&config, &path).expect("v4 upload should succeed");
    assert_eq!(result.share_url, "http://xb.test/p/abc");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn v4_test_connection_ok() {
    let (port, _guard) = spawn_mock("v4");
    let config = xb_config(port, VALID_TOKEN);
    test_connection(&config).expect("v4 test_connection should succeed");
}

#[test]
fn v4_bad_token_upload_is_auth_expired() {
    let (port, _guard) = spawn_mock("v4_bad_token");
    let config = xb_config(port, "wrong-token");
    let path = write_temp_png();

    let err = upload_file(&config, &path).expect_err("bad token should fail");
    assert!(
        matches!(err, UploadError::AuthExpired(_)),
        "expected AuthExpired, got {err:?}"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn v4_bad_token_test_connection_errors() {
    let (port, _guard) = spawn_mock("v4_bad_token");
    let config = xb_config(port, "wrong-token");
    let err = test_connection(&config).expect_err("bad token test_connection should fail");
    assert!(err.contains("token"), "unexpected error: {err}");
}

#[test]
fn v4_quota_upload_is_server_error() {
    let (port, _guard) = spawn_mock("v4_quota");
    let config = xb_config(port, VALID_TOKEN);
    let path = write_temp_png();

    let err = upload_file(&config, &path).expect_err("quota should fail");
    assert!(
        matches!(err, UploadError::Server(_)),
        "expected Server error, got {err:?}"
    );
    assert!(
        err.to_string().to_lowercase().contains("quota"),
        "unexpected: {err}"
    );

    let _ = std::fs::remove_file(&path);
}

// --- 3.x (Slim PHP, current stable) ---

#[test]
fn v3_test_connection_falls_back_ok() {
    let (port, _guard) = spawn_mock("v3");
    let config = xb_config(port, VALID_TOKEN);
    // test_connection probes v4 first (404), then falls back to v3 (400 probe accepted).
    test_connection(&config).expect("v3 test_connection fallback should succeed");
}

#[test]
fn v3_upload_falls_back_to_v3() {
    let (port, _guard) = spawn_mock("v3");
    let config = xb_config(port, VALID_TOKEN);
    let path = write_temp_png();

    // upload() tries v4 first; on 404 (3.x instance has no /api/v1/upload)
    // it must fall back to the 3.x /upload endpoint.
    let result = upload_file(&config, &path).expect("v3 upload fallback should succeed");
    assert_eq!(result.share_url, "http://xb.test/abc.png");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn v3_bad_token_test_connection_errors() {
    let (port, _guard) = spawn_mock("v3_bad_token");
    let config = xb_config(port, "wrong-token");
    let err = test_connection(&config).expect_err("v3 bad token should fail");
    assert!(
        err.to_lowercase().contains("token"),
        "unexpected error: {err}"
    );
}

// --- configuration ---

#[test]
fn is_configured_requires_url_and_token() {
    let (port, _guard) = spawn_mock("v4");
    let good = xb_config(port, VALID_TOKEN);
    assert!(is_configured(&good));

    let no_token = AppConfig {
        xbackbone_api_token: String::new(),
        ..good.clone()
    };
    assert!(!is_configured(&no_token));

    let no_url = AppConfig {
        xbackbone_url: String::new(),
        ..good
    };
    assert!(!is_configured(&no_url));
}
