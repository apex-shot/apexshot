//! The recording ID a finished recording carries inside its MP4, and the
//! app-data folder named after it. Keying the pointer data by an ID stored in
//! the video, rather than by the video's path, keeps it with the video through
//! a rename or a move.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// What the MP4 `description` tag holds before the ID.
pub const RECORDING_ID_PREFIX: &str = "apexshot-recording:";

/// The sidecar's file name inside a recording's folder.
pub const RECORDING_SIDECAR_NAME: &str = "pointer.json";

/// `<data_local_dir>/apexshot/recordings`.
pub fn recordings_directory() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("apexshot")
        .join("recordings")
}

/// The folder that holds one recording's pointer data.
pub fn recording_directory(id: &str) -> PathBuf {
    recordings_directory().join(id)
}

/// A lowercase hyphenated UUID: 36 characters, hyphens at 8, 13, 18 and 23.
pub fn is_valid_recording_id(id: &str) -> bool {
    id.len() == 36
        && id.char_indices().all(|(index, character)| match index {
            8 | 13 | 18 | 23 => character == '-',
            _ => character.is_ascii_digit() || ('a'..='f').contains(&character),
        })
}

/// The recording ID in `video_path`'s MP4 tag, if it has one.
pub fn recording_id_of(video_path: &Path) -> Option<String> {
    let mut file = File::open(video_path).ok()?;
    let length = file.metadata().ok()?.len();
    let moov = find_child(&mut file, 0, length, b"moov")?;
    let udta = find_child(&mut file, moov.0, moov.1, b"udta")?;
    let meta = find_child(&mut file, udta.0, udta.1, b"meta")?;
    if meta.0 + 4 > meta.1 {
        return None;
    }
    let ilst = find_child(&mut file, meta.0 + 4, meta.1, b"ilst")?;
    let desc = find_child(&mut file, ilst.0, ilst.1, b"desc")?;
    let data = find_child(&mut file, desc.0, desc.1, b"data")?;
    if data.0 + 8 > data.1 {
        return None;
    }
    let text_length = (data.1 - data.0 - 8).min(256) as usize;
    file.seek(SeekFrom::Start(data.0 + 8)).ok()?;
    let mut text = vec![0_u8; text_length];
    file.read_exact(&mut text).ok()?;
    let text = String::from_utf8(text).ok()?;
    let id = text
        .trim_end_matches('\0')
        .strip_prefix(RECORDING_ID_PREFIX)?
        .trim()
        .to_ascii_lowercase();
    is_valid_recording_id(&id).then_some(id)
}

/// The first child box `kind` between `start` and `end`, as (payload start,
/// box end). 32-bit sizes, 64-bit `largesize` (size 1) and size 0 (to the
/// end) are read; a malformed header ends the search.
fn find_child(file: &mut File, start: u64, end: u64, kind: &[u8; 4]) -> Option<(u64, u64)> {
    let mut position = start;
    while position + 8 <= end {
        file.seek(SeekFrom::Start(position)).ok()?;
        let mut header = [0_u8; 8];
        file.read_exact(&mut header).ok()?;
        let mut size = u64::from(u32::from_be_bytes([header[0], header[1], header[2], header[3]]));
        let mut header_size = 8_u64;
        if size == 1 {
            let mut large = [0_u8; 8];
            file.read_exact(&mut large).ok()?;
            size = u64::from_be_bytes(large);
            header_size = 16;
        } else if size == 0 {
            size = end - position;
        }
        if size < header_size || end - position < size {
            return None;
        }
        if &header[4..8] == kind {
            return Some((position + header_size, position + size));
        }
        position += size;
    }
    None
}

/// A box with a 32-bit size header.
#[cfg(test)]
fn mp4_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let size = 8 + payload.len() as u32;
    let mut bytes = Vec::with_capacity(size as usize);
    bytes.extend_from_slice(&size.to_be_bytes());
    bytes.extend_from_slice(kind);
    bytes.extend_from_slice(payload);
    bytes
}

/// An MP4 carrying `description` in the tag the recorder writes, laid out the
/// way ffmpeg stores it: `moov/udta/meta/ilst/desc/data`.
#[cfg(test)]
pub(crate) fn tagged_mp4_bytes(description: &str) -> Vec<u8> {
    let ftyp = mp4_box(b"ftyp", b"isom\0\0\x02\0isomiso2");
    let mvhd = mp4_box(b"mvhd", &[0_u8; 100]);

    let hdlr = mp4_box(b"hdlr", &[0_u8; 25]);

    let mut data_payload = vec![0, 0, 0, 1, 0, 0, 0, 0];
    data_payload.extend_from_slice(description.as_bytes());
    let data = mp4_box(b"data", &data_payload);
    let desc = mp4_box(b"desc", &data);
    let ilst = mp4_box(b"ilst", &desc);

    let mut meta_payload = vec![0, 0, 0, 0];
    meta_payload.extend_from_slice(&hdlr);
    meta_payload.extend_from_slice(&ilst);
    let meta = mp4_box(b"meta", &meta_payload);

    let udta = mp4_box(b"udta", &meta);
    let mut moov_payload = mvhd;
    moov_payload.extend_from_slice(&udta);
    let moov = mp4_box(b"moov", &moov_payload);

    let mut bytes = ftyp;
    bytes.extend_from_slice(&moov);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_video(name: &str, bytes: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "apexshot-recording-id-{}-{name}.mp4",
            std::process::id()
        ));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    const TEST_ID: &str = "0123abcd-0000-4000-8000-00000000abcd";

    #[test]
    fn reads_the_recording_id_from_the_description_tag() {
        let path = temp_video(
            "reads-id",
            &tagged_mp4_bytes(&format!("{RECORDING_ID_PREFIX}{TEST_ID}")),
        );
        assert_eq!(recording_id_of(&path).as_deref(), Some(TEST_ID));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn uppercase_ids_are_normalised() {
        let description = format!("{RECORDING_ID_PREFIX}{}", TEST_ID.to_ascii_uppercase());
        let path = temp_video("uppercase", &tagged_mp4_bytes(&description));
        assert_eq!(recording_id_of(&path).as_deref(), Some(TEST_ID));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn untagged_wrong_prefix_and_bad_ids_are_ignored() {
        let tagged = tagged_mp4_bytes(&format!("{RECORDING_ID_PREFIX}{TEST_ID}"));
        let cases: [(&str, Vec<u8>); 4] = [
            ("hello", tagged_mp4_bytes("hello")),
            ("bad-id", tagged_mp4_bytes(&format!("{RECORDING_ID_PREFIX}not-an-id"))),
            ("garbage", vec![0x42_u8; 10]),
            ("truncated", tagged[..tagged.len() / 2].to_vec()),
        ];
        for (name, bytes) in cases {
            let path = temp_video(name, &bytes);
            assert_eq!(recording_id_of(&path), None, "{name}");
            let _ = std::fs::remove_file(&path);
        }
    }

    #[test]
    fn a_64_bit_box_size_is_followed() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1_u32.to_be_bytes());
        bytes.extend_from_slice(b"free");
        bytes.extend_from_slice(&24_u64.to_be_bytes());
        bytes.extend_from_slice(&[0_u8; 8]);
        bytes.extend_from_slice(&tagged_mp4_bytes(&format!("{RECORDING_ID_PREFIX}{TEST_ID}")));
        let path = temp_video("large-size", &bytes);
        assert_eq!(recording_id_of(&path).as_deref(), Some(TEST_ID));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ids_are_validated() {
        assert!(is_valid_recording_id(TEST_ID));
        assert!(!is_valid_recording_id("0123abcd-0000-4000-8000-00000000abcg"));
        assert!(!is_valid_recording_id(&TEST_ID.replace('-', "")));
        assert!(!is_valid_recording_id(""));
    }
}
