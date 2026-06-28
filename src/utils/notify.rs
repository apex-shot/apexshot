pub fn desktop_notification(summary: &str, body: &str) {
    let mut cmd = std::process::Command::new("notify-send");
    cmd.arg("-a").arg("ApexShot").arg(summary);
    if !body.is_empty() {
        cmd.arg(body);
    }

    if let Err(e) = cmd.spawn() {
        eprintln!("[notify] Failed to send desktop notification: {e}");
    }
}
