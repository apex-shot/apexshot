use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Output},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "apx-perm-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(
            path.join("helper"),
            r#"#!/bin/sh
printf 'prepare\n' >> "$TEST_EVENTS"
sleep 0.3
if [ "$TEST_DENY" = 1 ]; then
    printf '%s\n' '{"error":"cancelled","cancelled":true}'
    exit 0
fi
printf 'ready\n' >> "$TEST_EVENTS"
printf '%s\n' '{"ready":true,"position":null,"size":null}'
while IFS= read -r command; do
    if [ "$command" = capture ]; then
        printf 'capture\n' >> "$TEST_EVENTS"
        printf '{"path":"%s","width":8,"height":6}\n' "$TEST_FRAME"
    fi
done
printf 'closed\n' >> "$TEST_EVENTS"
"#,
        )
        .unwrap();
        fs::set_permissions(path.join("helper"), fs::Permissions::from_mode(0o700)).unwrap();
        let mut frame = b"P6\n8 6\n255\n".to_vec();
        frame.extend_from_slice(&[0, 255, 0].repeat(8 * 6));
        fs::write(path.join("frame.ppm"), frame).unwrap();
        Self(path)
    }

    fn run(&self, seconds: u32, cancelled: bool) -> (Output, Duration) {
        let binary = option_env!("APEXSHOT_CAPTURE_BIN_DIR")
            .map(|path| PathBuf::from(path).join("apexshot-capture"))
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_BIN_EXE_apexshot")).with_file_name("apexshot-capture")
            });
        let start = Instant::now();
        let output = Command::new(binary)
            .args([
                "--capture-screen",
                "--show-timer",
                &format!("--timer-seconds={seconds}"),
                "--screenshot-cursor=0",
            ])
            .env("QT_QPA_PLATFORM", "offscreen")
            .env("QT_QPA_PLATFORMTHEME", "")
            .env("FLATPAK_ID", "org.apexshot.ApexShot")
            .env("APEXSHOT_PORTAL_HELPER", self.0.join("helper"))
            .env("TEST_EVENTS", self.0.join("events"))
            .env("TEST_FRAME", self.0.join("frame.ppm"))
            .env("TEST_DENY", if cancelled { "1" } else { "0" })
            .env("XDG_RUNTIME_DIR", &self.0)
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path={}", self.0.join("missing-bus").display()),
            )
            .output()
            .unwrap();
        (output, start.elapsed())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn fullscreen_authorizes_before_countdown_and_captures_from_that_session() {
    let fixture = Fixture::new();
    let (output, elapsed) = fixture.run(1, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        elapsed >= Duration::from_millis(1200),
        "The timer must start after the ready reply: {elapsed:?}"
    );
    assert_eq!(
        fs::read_to_string(fixture.0.join("events")).unwrap(),
        "prepare\nready\ncapture\nclosed\n"
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let path = PathBuf::from(response["path"].as_str().unwrap());
    assert!(path.exists());
    assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o077, 0);
    fs::remove_file(path).unwrap();
}

#[test]
fn cancelling_consent_never_runs_the_countdown_or_takes_a_frame() {
    let fixture = Fixture::new();
    let (output, elapsed) = fixture.run(30, true);
    assert_eq!(output.status.code(), Some(1));
    assert!(elapsed < Duration::from_secs(15));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(fixture.0.join("events")).unwrap(),
        "prepare\n"
    );
}

#[test]
fn disabled_timer_still_authorizes_before_capture() {
    let fixture = Fixture::new();
    let (output, _) = fixture.run(0, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(fixture.0.join("events")).unwrap(),
        "prepare\nready\ncapture\nclosed\n"
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    fs::remove_file(response["path"].as_str().unwrap()).unwrap();
}
