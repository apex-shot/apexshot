use super::{CaptureData, DisplayError, DisplayResult, PixelFormat};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::os::fd::{AsFd, FromRawFd, OwnedFd};
use zbus::zvariant::{Fd, OwnedValue, Value};

const KWIN_SERVICE: &str = "org.kde.KWin.ScreenShot2";
const KWIN_PATH: &str = "/org/kde/KWin/ScreenShot2";
const KWIN_INTERFACE: &str = "org.kde.KWin.ScreenShot2";
const KWIN_NO_AUTHORIZED_ERROR: &str = "org.kde.KWin.ScreenShot2.Error.NoAuthorized";

const QIMAGE_FORMAT_RGB32: u32 = 4;
const QIMAGE_FORMAT_ARGB32: u32 = 5;
const QIMAGE_FORMAT_ARGB32_PREMULTIPLIED: u32 = 6;
const QIMAGE_FORMAT_RGB888: u32 = 13;
const QIMAGE_FORMAT_RGBX8888: u32 = 16;
const QIMAGE_FORMAT_RGBA8888: u32 = 17;
const QIMAGE_FORMAT_RGBA8888_PREMULTIPLIED: u32 = 18;
const QIMAGE_FORMAT_BGR888: u32 = 29;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractiveKind {
    Window = 0,
    Screen = 1,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct KWinImageMetadata {
    format: u32,
    width: u32,
    height: u32,
    stride: u32,
    scale: f64,
}

pub fn is_kde_wayland_session_from_env(
    session_type: Option<&str>,
    current_desktop: Option<&str>,
) -> bool {
    let is_wayland = session_type
        .map(|value| value.eq_ignore_ascii_case("wayland"))
        .unwrap_or(false);
    if !is_wayland {
        return false;
    }

    current_desktop
        .unwrap_or_default()
        .split([':', ';', ','])
        .map(|part| part.trim().to_ascii_lowercase())
        .any(|part| part.contains("kde") || part.contains("plasma"))
}

pub fn is_kde_wayland_session() -> bool {
    is_kde_wayland_session_from_env(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref(),
    )
}

pub fn is_kwin_screenshot_available() -> bool {
    let Ok(conn) = zbus::blocking::Connection::session() else {
        return false;
    };

    let Ok(reply) = conn.call_method(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus"),
        "NameHasOwner",
        &(KWIN_SERVICE),
    ) else {
        return false;
    };

    reply.body().deserialize::<bool>().unwrap_or(false)
}

pub fn capture_workspace() -> DisplayResult<CaptureData> {
    let options = default_options();
    let conn = zbus::blocking::Connection::session().map_err(map_zbus_err)?;
    let (read_fd, write_fd) = create_pipe()?;

    let reply = conn
        .call_method(
            Some(KWIN_SERVICE),
            KWIN_PATH,
            Some(KWIN_INTERFACE),
            "CaptureWorkspace",
            &(options, Fd::from(write_fd.as_fd())),
        )
        .map_err(map_zbus_err)?;

    drop(write_fd);

    let metadata = parse_metadata(reply.body().deserialize().map_err(map_zbus_err)?)?;
    read_capture_data(read_fd, metadata)
}

pub fn capture_area(x: i32, y: i32, width: u32, height: u32) -> DisplayResult<CaptureData> {
    let options = default_options();
    let conn = zbus::blocking::Connection::session().map_err(map_zbus_err)?;
    let (read_fd, write_fd) = create_pipe()?;

    let reply = conn
        .call_method(
            Some(KWIN_SERVICE),
            KWIN_PATH,
            Some(KWIN_INTERFACE),
            "CaptureArea",
            &(x, y, width, height, options, Fd::from(write_fd.as_fd())),
        )
        .map_err(map_zbus_err)?;

    drop(write_fd);

    let metadata = parse_metadata(reply.body().deserialize().map_err(map_zbus_err)?)?;
    read_capture_data(read_fd, metadata)
}

pub fn capture_interactive(kind: InteractiveKind) -> DisplayResult<CaptureData> {
    let options = default_options();
    let conn = zbus::blocking::Connection::session().map_err(map_zbus_err)?;
    let (read_fd, write_fd) = create_pipe()?;

    let reply = conn
        .call_method(
            Some(KWIN_SERVICE),
            KWIN_PATH,
            Some(KWIN_INTERFACE),
            "CaptureInteractive",
            &(kind as u32, options, Fd::from(write_fd.as_fd())),
        )
        .map_err(map_zbus_err)?;

    drop(write_fd);

    let metadata = parse_metadata(reply.body().deserialize().map_err(map_zbus_err)?)?;
    read_capture_data(read_fd, metadata)
}

fn default_options() -> HashMap<&'static str, Value<'static>> {
    HashMap::from([
        ("include-cursor", Value::from(false)),
        ("include-shadow", Value::from(true)),
        ("native-resolution", Value::from(true)),
        ("hide-caller-windows", Value::from(true)),
    ])
}

fn create_pipe() -> DisplayResult<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
    if rc != 0 {
        return Err(DisplayError::IoError(std::io::Error::last_os_error()));
    }

    let read_fd = unsafe { OwnedFd::from_raw_fd(fds[0]) };
    let write_fd = unsafe { OwnedFd::from_raw_fd(fds[1]) };
    Ok((read_fd, write_fd))
}

fn parse_metadata(results: HashMap<String, OwnedValue>) -> DisplayResult<KWinImageMetadata> {
    let image_type = string_value(&results, "type")?;
    if image_type != "raw" {
        return Err(DisplayError::CaptureError(format!(
            "Unsupported KWin screenshot type: {image_type}"
        )));
    }

    Ok(KWinImageMetadata {
        format: u32_value(&results, "format")?,
        width: u32_value(&results, "width")?,
        height: u32_value(&results, "height")?,
        stride: u32_value(&results, "stride")?,
        scale: f64_value(&results, "scale").unwrap_or(1.0),
    })
}

fn string_value(results: &HashMap<String, OwnedValue>, key: &str) -> DisplayResult<String> {
    let value = results.get(key).ok_or_else(|| {
        DisplayError::CaptureError(format!("KWin screenshot reply missing '{key}'"))
    })?;
    String::try_from(value.clone()).map_err(|e| {
        DisplayError::CaptureError(format!("KWin screenshot reply field '{key}' invalid: {e}"))
    })
}

fn u32_value(results: &HashMap<String, OwnedValue>, key: &str) -> DisplayResult<u32> {
    let value = results.get(key).ok_or_else(|| {
        DisplayError::CaptureError(format!("KWin screenshot reply missing '{key}'"))
    })?;
    u32::try_from(value.clone()).map_err(|e| {
        DisplayError::CaptureError(format!("KWin screenshot reply field '{key}' invalid: {e}"))
    })
}

fn f64_value(results: &HashMap<String, OwnedValue>, key: &str) -> DisplayResult<f64> {
    let value = results.get(key).ok_or_else(|| {
        DisplayError::CaptureError(format!("KWin screenshot reply missing '{key}'"))
    })?;
    f64::try_from(value.clone()).map_err(|e| {
        DisplayError::CaptureError(format!("KWin screenshot reply field '{key}' invalid: {e}"))
    })
}

fn read_capture_data(read_fd: OwnedFd, metadata: KWinImageMetadata) -> DisplayResult<CaptureData> {
    if metadata.width == 0 || metadata.height == 0 {
        return Err(DisplayError::CaptureError(
            "KWin screenshot returned empty image dimensions".into(),
        ));
    }

    let mut file = File::from(read_fd);
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(DisplayError::IoError)?;

    let pixels = convert_qimage_to_rgba(&bytes, metadata)?;
    let mut capture =
        CaptureData::new(pixels, metadata.width, metadata.height, PixelFormat::RGBA32);
    capture.output_scale = metadata.scale.round().max(1.0) as i32;
    Ok(capture)
}

fn convert_qimage_to_rgba(bytes: &[u8], metadata: KWinImageMetadata) -> DisplayResult<Vec<u8>> {
    let stride = metadata.stride as usize;
    let height = metadata.height as usize;
    let expected_min = stride
        .checked_mul(height)
        .ok_or_else(|| DisplayError::CaptureError("KWin screenshot dimensions overflow".into()))?;

    if bytes.len() < expected_min {
        return Err(DisplayError::CaptureError(format!(
            "KWin screenshot pipe returned {} bytes, expected at least {}",
            bytes.len(),
            expected_min
        )));
    }

    let width = metadata.width as usize;
    let mut out = vec![0u8; width * height * 4];

    for row in 0..height {
        let src_row = &bytes[row * stride..(row + 1) * stride];
        let dst_row = &mut out[row * width * 4..(row + 1) * width * 4];

        match metadata.format {
            QIMAGE_FORMAT_RGB32 => {
                for (src, dst) in src_row
                    .chunks_exact(4)
                    .take(width)
                    .zip(dst_row.chunks_exact_mut(4))
                {
                    dst[0] = src[2];
                    dst[1] = src[1];
                    dst[2] = src[0];
                    dst[3] = 255;
                }
            }
            QIMAGE_FORMAT_ARGB32 | QIMAGE_FORMAT_ARGB32_PREMULTIPLIED => {
                for (src, dst) in src_row
                    .chunks_exact(4)
                    .take(width)
                    .zip(dst_row.chunks_exact_mut(4))
                {
                    dst[0] = src[2];
                    dst[1] = src[1];
                    dst[2] = src[0];
                    dst[3] = src[3];
                }
            }
            QIMAGE_FORMAT_RGB888 => {
                for (src, dst) in src_row
                    .chunks_exact(3)
                    .take(width)
                    .zip(dst_row.chunks_exact_mut(4))
                {
                    dst[0] = src[0];
                    dst[1] = src[1];
                    dst[2] = src[2];
                    dst[3] = 255;
                }
            }
            QIMAGE_FORMAT_BGR888 => {
                for (src, dst) in src_row
                    .chunks_exact(3)
                    .take(width)
                    .zip(dst_row.chunks_exact_mut(4))
                {
                    dst[0] = src[2];
                    dst[1] = src[1];
                    dst[2] = src[0];
                    dst[3] = 255;
                }
            }
            QIMAGE_FORMAT_RGBX8888 => {
                for (src, dst) in src_row
                    .chunks_exact(4)
                    .take(width)
                    .zip(dst_row.chunks_exact_mut(4))
                {
                    dst[0] = src[0];
                    dst[1] = src[1];
                    dst[2] = src[2];
                    dst[3] = 255;
                }
            }
            QIMAGE_FORMAT_RGBA8888 | QIMAGE_FORMAT_RGBA8888_PREMULTIPLIED => {
                for (src, dst) in src_row
                    .chunks_exact(4)
                    .take(width)
                    .zip(dst_row.chunks_exact_mut(4))
                {
                    dst.copy_from_slice(src);
                }
            }
            other => {
                return Err(DisplayError::CaptureError(format!(
                    "Unsupported KWin QImage format: {other}"
                )));
            }
        }
    }

    Ok(out)
}

pub fn is_authorization_error(err: &DisplayError) -> bool {
    matches!(err, DisplayError::CaptureError(message) if message.contains(KWIN_NO_AUTHORIZED_ERROR))
}

fn map_zbus_err(err: zbus::Error) -> DisplayError {
    DisplayError::CaptureError(format!("KWin screenshot D-Bus call failed: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::OwnedValue;

    #[test]
    fn kde_wayland_detection_requires_wayland_and_kde() {
        assert!(is_kde_wayland_session_from_env(
            Some("wayland"),
            Some("KDE")
        ));
        assert!(is_kde_wayland_session_from_env(
            Some("wayland"),
            Some("plasma:foo")
        ));
        assert!(!is_kde_wayland_session_from_env(Some("x11"), Some("KDE")));
        assert!(!is_kde_wayland_session_from_env(
            Some("wayland"),
            Some("GNOME")
        ));
    }

    #[test]
    fn parse_metadata_reads_kwin_reply() {
        let mut map = HashMap::new();
        map.insert(
            "type".into(),
            Value::from("raw")
                .try_into()
                .expect("string owned value should convert"),
        );
        map.insert("format".into(), OwnedValue::from(17u32));
        map.insert("width".into(), OwnedValue::from(2u32));
        map.insert("height".into(), OwnedValue::from(1u32));
        map.insert("stride".into(), OwnedValue::from(8u32));
        map.insert("scale".into(), OwnedValue::from(1.0f64));

        let parsed = parse_metadata(map).expect("metadata should parse");
        assert_eq!(parsed.format, 17);
        assert_eq!(parsed.width, 2);
        assert_eq!(parsed.height, 1);
        assert_eq!(parsed.stride, 8);
        assert_eq!(parsed.scale, 1.0);
    }

    #[test]
    fn convert_rgba8888_qimage_to_rgba() {
        let metadata = KWinImageMetadata {
            format: QIMAGE_FORMAT_RGBA8888,
            width: 2,
            height: 1,
            stride: 8,
            scale: 1.0,
        };
        let src = vec![1, 2, 3, 4, 10, 20, 30, 40];
        let out = convert_qimage_to_rgba(&src, metadata).expect("conversion should succeed");
        assert_eq!(out, src);
    }

    #[test]
    fn convert_rgb32_qimage_to_rgba() {
        let metadata = KWinImageMetadata {
            format: QIMAGE_FORMAT_RGB32,
            width: 1,
            height: 1,
            stride: 4,
            scale: 1.0,
        };
        let src = vec![3, 2, 1, 0];
        let out = convert_qimage_to_rgba(&src, metadata).expect("conversion should succeed");
        assert_eq!(out, vec![1, 2, 3, 255]);
    }
}
