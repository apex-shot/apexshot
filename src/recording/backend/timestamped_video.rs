//! Stream native 32-bit pixels with producer PTS instead of FFmpeg's pipe-receipt clock.
//! Each frame is an independent Matroska cluster on a microsecond time base.

use std::io;

fn size(value: u64) -> Vec<u8> {
    let width = (1..=8)
        .find(|width| value < (1u64 << (7 * width)) - 1)
        .expect("video packet fits in an EBML size");
    let encoded = value | (1u64 << (7 * width));
    encoded.to_be_bytes()[8 - width..].to_vec()
}

fn element(id: u32, payload: &[u8]) -> Vec<u8> {
    let bytes = id.to_be_bytes();
    let mut output = bytes[bytes.iter().position(|&b| b != 0).unwrap()..].to_vec();
    output.extend(size(payload.len() as u64));
    output.extend(payload);
    output
}

fn uint(id: u32, value: u64) -> Vec<u8> {
    let bytes = value.to_be_bytes();
    element(
        id,
        &bytes[bytes.iter().position(|&b| b != 0).unwrap_or(7)..],
    )
}

pub(super) fn header(width: u32, height: u32, fps: u32, pix_fmt: &str) -> io::Result<Vec<u8>> {
    let tag = match pix_fmt {
        "bgra" => *b"BGRA",
        "bgr0" => *b"BGRA",
        "rgba" => *b"RGBA",
        "rgb0" => *b"RGBA",
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unsupported raw pixel format",
            ))
        }
    };
    if width == 0 || height == 0 || fps == 0 || width > i32::MAX as u32 || height > i32::MAX as u32
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid raw video dimensions or rate",
        ));
    }
    let ebml = [
        uint(0x4286, 1),
        uint(0x42f7, 1),
        uint(0x42f2, 4),
        uint(0x42f3, 8),
        element(0x4282, b"matroska"),
        uint(0x4287, 4),
        uint(0x4285, 2),
    ]
    .concat();
    let info = [
        uint(0x2ad7b1, 1_000),
        element(0x4d80, b"ApexShot"),
        element(0x5741, b"ApexShot"),
    ]
    .concat();
    let mut bitmap = Vec::with_capacity(40);
    bitmap.extend(40u32.to_le_bytes());
    bitmap.extend(width.to_le_bytes());
    bitmap.extend(height.to_le_bytes());
    bitmap.extend(1u16.to_le_bytes());
    bitmap.extend(32u16.to_le_bytes());
    bitmap.extend(tag);
    bitmap.extend([0u8; 20]);
    let track = [
        uint(0xd7, 1),
        uint(0x73c5, 1),
        uint(0x83, 1),
        uint(0x9c, 0),
        uint(0x23e383, 1_000_000_000 / u64::from(fps)),
        element(0x86, b"V_MS/VFW/FOURCC"),
        element(0x63a2, &bitmap),
        element(
            0xe0,
            &[uint(0xb0, u64::from(width)), uint(0xba, u64::from(height))].concat(),
        ),
    ]
    .concat();
    let mut output = element(0x1a45dfa3, &ebml);
    output.extend([
        0x18, 0x53, 0x80, 0x67, 0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    ]);
    output.extend(element(0x1549a966, &info));
    output.extend(element(0x1654ae6b, &element(0xae, &track)));
    Ok(output)
}

pub(super) fn frame_header(pts_us: u64, pixel_bytes: usize) -> Vec<u8> {
    let timecode = uint(0xe7, pts_us);
    let block_size = pixel_bytes as u64 + 4;
    let mut block = vec![0xa3];
    block.extend(size(block_size));
    let mut output = vec![0x1f, 0x43, 0xb6, 0x75];
    output.extend(size(
        timecode.len() as u64 + block.len() as u64 + block_size,
    ));
    output.extend(timecode);
    output.extend(block);
    output.extend([0x81, 0x00, 0x00, 0x80]);
    output
}

pub(super) fn cfr_filter(fps: u32, filter: &str) -> String {
    format!("fps=fps={fps}:start_time=0:round=near:eof_action=pass,{filter}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};

    #[test]
    fn timestamped_video_preserves_producer_pts_pixel_order_and_row_orientation() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            return;
        }
        for format in ["bgra", "bgr0", "rgba", "rgb0"] {
            let directory = std::env::temp_dir().join(format!(
                "apexshot-timestamped-{format}-{}",
                std::process::id()
            ));
            std::fs::create_dir_all(&directory).unwrap();
            let source = directory.join("input.mkv");
            let mut file = std::fs::File::create(&source).unwrap();
            file.write_all(&header(2, 2, 30, format).unwrap()).unwrap();
            let rgb = format.starts_with("rgb");
            let mut pixels = if rgb {
                [
                    255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
                ]
            } else {
                [
                    0, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255, 255, 255, 255, 255,
                ]
            };
            if format.ends_with('0') {
                for pixel in pixels.chunks_exact_mut(4) {
                    pixel[3] = 0;
                }
            }
            for pts in [0, 433_333, 10_133_333] {
                file.write_all(&frame_header(pts, pixels.len())).unwrap();
                file.write_all(&pixels).unwrap();
            }
            drop(file);
            let probe = Command::new("ffprobe")
                .args([
                    "-v",
                    "error",
                    "-show_entries",
                    "packet=pts_time",
                    "-of",
                    "csv=p=0",
                ])
                .arg(&source)
                .output()
                .unwrap();
            assert!(
                probe.status.success(),
                "{}",
                String::from_utf8_lossy(&probe.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&probe.stdout).trim(),
                "0.000000\n0.433333\n10.133333"
            );
            let decoded = Command::new("ffmpeg")
                .args(["-v", "error", "-i"])
                .arg(&source)
                .args([
                    "-fps_mode",
                    "passthrough",
                    "-f",
                    "rawvideo",
                    "-pix_fmt",
                    "rgb24",
                    "pipe:1",
                ])
                .output()
                .unwrap();
            assert!(
                decoded.status.success(),
                "{}",
                String::from_utf8_lossy(&decoded.stderr)
            );
            assert_eq!(
                decoded.stdout,
                [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255].repeat(3)
            );
            std::fs::remove_dir_all(directory).unwrap();
        }
    }

    #[test]
    fn timestamped_video_cfr_encoding_ignores_pipe_delivery_delay() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            return;
        }
        let directory =
            std::env::temp_dir().join(format!("apexshot-timestamped-cfr-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let output = directory.join("output.mp4");
        let mut child = Command::new("ffmpeg")
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "matroska",
                "-i",
                "pipe:0",
                "-vf",
                &cfr_filter(30, "format=yuv420p"),
                "-fps_mode",
                "cfr",
                "-r",
                "30",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&output)
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        input.write_all(&header(2, 2, 30, "rgba").unwrap()).unwrap();
        for (pts, pixels) in [
            (0, [255, 0, 0, 255]),
            (400_000, [0, 255, 0, 255]),
            (1_300_000, [0, 0, 255, 255]),
        ] {
            input.write_all(&frame_header(pts, 16)).unwrap();
            input.write_all(&pixels.repeat(4)).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        drop(input);
        let result = child.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let probe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=start_time,duration",
                "-of",
                "json",
            ])
            .arg(&output)
            .output()
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
        assert_eq!(json["streams"][0]["start_time"], "0.000000");
        let duration: f64 = json["streams"][0]["duration"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        assert!(
            (duration - 1.333333).abs() <= 1.0 / 30.0,
            "duration={duration}"
        );
        let decoded = Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(&output)
            .args([
                "-fps_mode",
                "passthrough",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
                "pipe:1",
            ])
            .output()
            .unwrap();
        assert!(decoded.status.success());
        for (channel, expected_time) in [(1, 0.4), (2, 1.3)] {
            let frame = decoded
                .stdout
                .chunks_exact(12)
                .position(|pixels| pixels[channel] > 150 && pixels[0] < 100)
                .unwrap();
            let actual_time = frame as f64 / 30.0;
            assert!(
                (actual_time - expected_time).abs() <= 1.0 / 30.0 + 1e-6,
                "channel={channel} actual_time={actual_time}"
            );
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn timestamped_video_rejects_unknown_formats_and_invalid_dimensions() {
        assert!(header(2, 2, 30, "unknown").is_err());
        assert!(header(0, 2, 30, "bgra").is_err());
        assert!(header(2, 2, 0, "bgra").is_err());
    }
}
