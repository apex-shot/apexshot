//! Suggested names for the Save choosers, so a repeat export never proposes
//! a file that is already there and the user only renames when they want to.

use std::path::Path;

/// `file_name` when `folder` has nothing by that name yet, otherwise the first
/// free `stem (n).ext` counting from 2, the way Explorer numbers copies.
pub fn available_file_name(folder: &Path, file_name: &str) -> String {
    if !folder.join(file_name).exists() {
        return file_name.to_string();
    }
    let name = Path::new(file_name);
    let stem = name
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(file_name);
    let extension = name.extension().and_then(|extension| extension.to_str());
    for number in 2..10_000u32 {
        let candidate = match extension {
            Some(extension) => format!("{stem} ({number}).{extension}"),
            None => format!("{stem} ({number})"),
        };
        if !folder.join(&candidate).exists() {
            return candidate;
        }
    }
    file_name.to_string()
}

#[cfg(test)]
mod tests {
    use super::available_file_name;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let folder =
            std::env::temp_dir().join(format!("apexshot-unique-name-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn a_free_name_is_kept() {
        let folder = scratch("free");
        assert_eq!(
            available_file_name(&folder, "shot-annotated.png"),
            "shot-annotated.png"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_taken_name_gets_the_next_free_number() {
        let folder = scratch("taken");
        std::fs::write(folder.join("clip-edited.mp4"), b"").unwrap();
        assert_eq!(
            available_file_name(&folder, "clip-edited.mp4"),
            "clip-edited (2).mp4"
        );
        std::fs::write(folder.join("clip-edited (2).mp4"), b"").unwrap();
        assert_eq!(
            available_file_name(&folder, "clip-edited.mp4"),
            "clip-edited (3).mp4"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_missing_folder_keeps_the_name() {
        let folder = std::env::temp_dir().join("apexshot-unique-name-missing-folder-xyz");
        assert_eq!(available_file_name(&folder, "a.png"), "a.png");
    }
}
