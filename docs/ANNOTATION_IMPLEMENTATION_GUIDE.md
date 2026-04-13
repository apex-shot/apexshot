# ApexShot Annotation Storage: Implementation Guide

**Recommendation:** Centralized JSON storage in XDG data directory

---

## Architecture

### Storage Location

Annotations are stored in a centralized directory following XDG Base Directory specification:

```
~/.local/share/apexshot/annotations/
  ├── 7f3a8b2c9e1d4f6a.json    # Keyed by SHA256 hash of image path
  ├── a2b5c8d1e4f7g9h3.json
  └── ...
```

### Mapping

```
~/Pictures/screenshot.png
    → SHA256("/home/user/Pictures/screenshot.png")
    → ~/.local/share/apexshot/annotations/7f3a8b2c9e1d4f6a.json
```

### New User Flow

```
Screenshot → Preview → Edit → Done
                              ↓
                    Save flattened image
                    Save annotations to ~/.local/share/...
                    Open preview with edited image
                              ↓
                    Edit again → Load annotations from sidecar
                              ↓
                    Modify individual annotations
                    (move arrow, change text, etc.)
```

---

## JSON Schema

```json
{
  "version": "1.0",
  "imagePath": "/home/user/Pictures/screenshot.png",
  "imageHash": "sha256:abc123def456...",
  "canvasSize": { "width": 1920, "height": 1080 },
  "annotations": [
    {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "type": "Arrow",
      "start": { "x": 100, "y": 50 },
      "end": { "x": 200, "y": 80 },
      "color": { "r": 255, "g": 0, "b": 0, "a": 255 },
      "strokeWidth": 3,
      "arrowStyle": "Standard"
    },
    {
      "id": "660e8400-e29b-41d4-a716-446655440001",
      "type": "Text",
      "position": { "x": 150, "y": 200 },
      "text": "Click here",
      "fontSize": 16,
      "color": { "r": 255, "g": 255, "b": 255, "a": 255 }
    }
  ],
  "createdAt": "2025-04-13T21:30:00Z",
  "modifiedAt": "2025-04-13T21:35:00Z"
}
```

**Note:** Undo/redo history is NOT persisted. When re-editing, users start fresh but can modify existing annotations.

---

## Implementation Components

### New Module

```
src/annotations/
  mod.rs           — Public API exports
  storage.rs       — Save/load sidecar files, path resolution
  schema.rs        — Serde structs for JSON serialization
```

### Files to Modify

| File | Change |
|------|--------|
| `src/capture/editor/window/events.rs` | Done button: save annotations + open preview |
| `src/capture/editor/state.rs` | Add `Serialize/Deserialize` for `Annot` types |
| `src/capture/preview_overlay.rs` | "Edit" button: detect and pass annotation path |
| `src/capture/editor/window/mod.rs` | `open_image_editor`: accept optional annotation path |
| `src/main.rs` | CLI `edit` command: check for existing annotations |
| `src/lib.rs` | Add `pub mod annotations;` |

---

## Core Functions

```rust
// src/annotations/storage.rs

/// Get the annotation file path for an image
pub fn annotation_path_for_image(image_path: &Path) -> PathBuf {
    let path_str = image_path.to_string_lossy();
    let hash = sha256::digest(path_str.as_bytes());
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("apexshot")
        .join("annotations")
        .join(format!("{}.json", hash))
}

/// Save annotations for an image
pub fn save_annotations(image_path: &Path, state: &EditorState) -> Result<(), AnnotationError> {
    let annotation_path = annotation_path_for_image(image_path);
    let image_hash = compute_image_hash(image_path)?;

    let file = AnnotationFile::from_state(image_path, image_hash, state);
    let json = serde_json::to_string_pretty(&file)?;

    // Ensure directory exists
    if let Some(parent) = annotation_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Atomic write
    let temp_path = annotation_path.with_extension("json.tmp");
    std::fs::write(&temp_path, json)?;
    std::fs::rename(&temp_path, &annotation_path)?;

    Ok(())
}

/// Load annotations for an image
pub fn load_annotations(image_path: &Path) -> Result<Option<AnnotationFile>, AnnotationError> {
    let annotation_path = annotation_path_for_image(image_path);
    if !annotation_path.exists() {
        return Ok(None);
    }

    let json = std::fs::read_to_string(annotation_path)?;
    let file: AnnotationFile = serde_json::from_str(&json)?;

    // Verify hash matches
    let current_hash = compute_image_hash(image_path)?;
    if file.image_hash != current_hash {
        return Err(AnnotationError::HashMismatch);
    }

    Ok(Some(file))
}

/// Compute SHA256 hash of image file
pub fn compute_image_hash(image_path: &Path) -> Result<String, AnnotationError> {
    let bytes = std::fs::read(image_path)?;
    let hash = sha256::digest(&bytes);
    Ok(format!("sha256:{}", hash))
}
```

---

## Edge Cases

| Case | Handling |
|------|----------|
| Annotation file missing | Start with empty canvas (fresh edit) |
| Image hash mismatch | Warn: "Image modified externally; annotations may not align correctly" |
| Image moved | Hash of new path won't match; store original path in JSON for recovery |
| Annotation directory missing | Create on first save |
| Corrupted JSON | Log error, start fresh, delete corrupted file |

---

## Implementation Steps

### Step 1: Create annotations module
- [ ] Create `src/annotations/mod.rs`
- [ ] Create `src/annotations/schema.rs` with Serde structs
- [ ] Create `src/annotations/storage.rs` with save/load functions
- [ ] Add `sha2` or use existing `sha256` crate to Cargo.toml

### Step 2: Serialize existing types
- [ ] Add `Serialize/Deserialize` to `Annot` enum variants
- [ ] Add `Serialize/Deserialize` to `EditorState` relevant fields
- [ ] Handle `Rgba<u8>` color serialization

### Step 3: Modify editor Done behavior
- [ ] In `events.rs`, save annotations before closing
- [ ] Spawn preview overlay with the edited image path
- [ ] Pass annotation path to preview

### Step 4: Modify preview Edit behavior
- [ ] Check for existing annotations when Edit clicked
- [ ] Pass annotation path to editor spawn
- [ ] Editor loads annotations if present

### Step 5: Testing
- [ ] Test: Save annotations, close, re-edit → annotations load
- [ ] Test: Modify annotation, save, re-edit → changes persisted
- [ ] Test: Delete annotation file → fresh edit starts
- [ ] Test: Modify image externally → hash mismatch warning

---

## Dependencies

Add to `Cargo.toml`:
```toml
sha2 = "0.10"           # Or use existing if available
serde = { version = "1.0", features = ["derive"] }  # Already present
serde_json = "1.0"      # Already present
uuid = { version = "1.0", features = ["v4", "serde"] }  # For annotation IDs
```

---

## Future Enhancements (Out of Scope)

- Phase 2: XMP embedding for single-file portability
- Phase 3: SQLite database for large libraries
- "Export with annotations" feature for sharing
- Annotation search/filter UI
