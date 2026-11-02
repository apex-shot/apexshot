//! Persistent installation identity for device-bound Studio licenses.
//! This module does not send telemetry.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

#[cfg(test)]
pub(crate) fn ensure_install_id() -> Option<String> {
    Some("12345678-1234-4123-8123-123456789abc".into())
}

#[cfg(not(test))]
pub(crate) fn ensure_install_id() -> Option<String> {
    let path = dirs::config_dir()?.join("apexshot").join("install_id");
    load_or_create(&path)
}

#[cfg_attr(test, allow(dead_code))]
fn load_or_create(path: &Path) -> Option<String> {
    if let Ok(id) = fs::read_to_string(path) {
        return valid_id(id.trim()).then(|| id.trim().to_owned());
    }
    fs::create_dir_all(path.parent()?).ok()?;
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).ok()?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let id = format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    );
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => {
            file.write_all(id.as_bytes()).ok()?;
            file.sync_all().ok()?;
            Some(id)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let existing = fs::read_to_string(path).ok()?;
            valid_id(existing.trim()).then(|| existing.trim().to_owned())
        }
        Err(_) => None,
    }
}

fn valid_id(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_id_is_persistent_and_valid() {
        let path = std::env::temp_dir()
            .join(format!("apexshot-id-test-{}", std::process::id()))
            .join("install_id");
        let first = load_or_create(&path).unwrap();
        assert!(valid_id(&first));
        assert_eq!(load_or_create(&path).as_deref(), Some(first.as_str()));
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(path.parent().unwrap()).unwrap();
    }
}
