//! End-to-end tests against a REAL XBackBone 3.x instance.
//!
//! These only run when `APEXSHOT_XB_E2E_URL` is set (pointing at a live
//! XBackBone instance) and `APEXSHOT_XB_E2E_TOKEN` is set (a valid upload
//! token for that instance). In normal CI/development they are skipped.
//!
//! Quick local setup (the ApexShot maintainer's testing recipe):
//!
//!   docker run -d --name apexshot-xb-test --rm -p 127.0.0.1:8080:80 \
//!     -e PUID=1000 -e PGID=1000 -v /tmp/xb/config:/config -v /tmp/xb/data:/data \
//!     linuxserver/xbackbone:3.8.2
//!   # complete the web installer at http://127.0.0.1:8080/install/ ,
//!   # create an admin user with an upload token, then:
//!   APEXSHOT_XB_E2E_URL=http://127.0.0.1:8080 \
//!   APEXSHOT_XB_E2E_TOKEN=<token> \
//!   cargo test --test xbackbone_e2e -- --nocapture

use std::path::Path;

use apexshot::cloud::upload::{upload_file, UploadError, UploadResult};
use apexshot::cloud::xbackbone::test_connection;
use apexshot::config::AppConfig;

fn env_or_skip(name: &str) -> String {
    match std::env::var(name) {
        Ok(v) if !v.is_empty() => v,
        _ => {
            eprintln!(
                "[skipped] set {name} (and APEXSHOT_XB_E2E_TOKEN) to run this end-to-end test"
            );
            // Use a sentinel that makes the test trivially pass when no
            // instance is configured, so `cargo test` stays green in CI.
            String::new()
        }
    }
}

fn live_config() -> Option<AppConfig> {
    let url = env_or_skip("APEXSHOT_XB_E2E_URL");
    let token = env_or_skip("APEXSHOT_XB_E2E_TOKEN");
    if url.is_empty() || token.is_empty() {
        return None;
    }
    Some(AppConfig {
        cloud_destination: "xbackbone".to_string(),
        xbackbone_url: url,
        xbackbone_api_token: token,
        ..AppConfig::default()
    })
}

fn write_temp_png() -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "apexshot-xb-e2e-{}-{}.png",
        std::process::id(),
        std::time::SystemTime::now()
            .elapsed()
            .unwrap_or_default()
            .as_nanos()
    ));
    // A minimal valid 1x1 PNG so a strict server doesn't reject the bytes.
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // signature
        0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // 1x1
        0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xDE, // rgb, crc
        0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, // IDAT
        0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0xE5, 0x27, 0xDE,
        0xFC, // crc
        0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, // IEND
        0xAE, 0x42, 0x60, 0x82,
    ];
    std::fs::write(&path, PNG).expect("write temp png");
    path
}

#[test]
fn e2e_test_connection_succeeds() {
    let Some(config) = live_config() else { return };
    test_connection(&config)
        .expect("test_connection should succeed against the live XBackBone instance");
}

#[test]
fn e2e_upload_returns_share_url() {
    let Some(config) = live_config() else { return };
    let path = write_temp_png();

    let result: UploadResult =
        upload_file(&config, &path).expect("upload to the live XBackBone instance should succeed");

    // The share URL must be a non-empty http(s) URL. The exact path depends
    // on the instance config (3.x returns the preview page, 4.x the
    // preview_ext_url), so we only assert the shape here.
    assert!(
        result.share_url.starts_with("http"),
        "expected an http(s) share URL, got: {}",
        result.share_url
    );
    assert!(!result.share_url.trim().is_empty());

    // The uploaded file should now be reachable at the returned URL.
    let status = ureq::get(&result.share_url)
        .call()
        .map(|r| r.status())
        .unwrap_or(0);
    assert!(
        (200..400).contains(&status),
        "share URL should be reachable, got HTTP {status}"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn e2e_bad_token_is_rejected() {
    let Some(mut config) = live_config() else {
        return;
    };
    config.xbackbone_api_token = "definitely-not-a-real-token".to_string();

    let path = write_temp_png();
    let err = upload_file(&config, &path).expect_err("a bad token must be rejected");
    assert!(
        matches!(err, UploadError::AuthExpired(_)),
        "expected AuthExpired for a bad token, got {err:?}"
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn e2e_not_configured_errors_cleanly() {
    let Some(mut config) = live_config() else {
        return;
    };
    config.xbackbone_url = String::new();
    let path = Path::new("/tmp/does-not-matter.png");
    let err = upload_file(&config, path).expect_err("empty URL should fail as NotConfigured");
    assert!(matches!(err, UploadError::NotConfigured(_)));
}
