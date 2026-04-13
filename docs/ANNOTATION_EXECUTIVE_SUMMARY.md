# Annotation Storage: Executive Summary

## The Question
How should ApexShot store screenshot annotations to enable re-editing?

## The Answer
**Use Sidecar JSON files (Phase 1), with optional XMP embedding (Phase 2) and database (Phase 3).**

---

## Quick Comparison

### Sidecar JSON ⭐ RECOMMENDED
```
screenshot.png
screenshot.annotations.json  ← Paired file with editable annotations
```
**Best for MVP:** Simple, portable, industry-standard pattern (used by Lightroom, Darktable, PhotoPrism, Google Photos)

**Pros:** Easy to implement, transparent, works everywhere, human-readable, Git-friendly
**Cons:** Two files to manage, sharing requires both files

---

### Embedded Metadata (XMP)
```
screenshot.png  ← Metadata embedded inside image file
```
**Best for:** Professional workflows needing single-file portability
**Problem:** Metadata stripped by cloud storage, format-dependent, complex to implement

---

### Project Database
```
database/
  annotations.db
screenshot.png
```
**Best for:** Team collaboration, version history, batch operations
**Problem:** Heavy setup, vendor-locked, poor portability, not for MVP

---

## Industry Proof Points

| Tool | Method | Use Case |
|------|--------|----------|
| Lightroom | XMP Sidecar | Professional photo editing |
| Capture One | XMP + Sessions | Professional RAW editing |
| Darktable | XMP Sidecar | Open-source photo tool |
| Snagit | Project File (ZIP) | Enterprise screenshots |
| PhotoPrism | JSON Sidecar | Open-source photo library |
| ShareX | Moving to projects | Community feedback (XIP0068) |
| Flameshot | None yet | Community requesting it |

**Pattern:** Every major tool uses sidecar files for non-destructive annotation.

---

## Phase Implementation Timeline

### Phase 1: Sidecar JSON (2-3 weeks) ⭐ DO THIS FIRST
- Save/load `.annotations.json` alongside image
- File watcher for renames/moves
- Hash validation (detect external edits)
- **Ship this:** Enables core re-editing feature

### Phase 2: Optional XMP (2-3 weeks)
- Embed annotations in JPEG/PNG metadata
- Fallback to sidecar for unsupported formats
- **Ship when:** Users ask for Lightroom integration

### Phase 3: Optional Database (4-6 weeks)
- SQLite for library indexing
- Search, filter, batch operations
- **Ship when:** 10k+ screenshots in library

---

## Risk Mitigation

| Risk | Probability | Solution |
|------|---|---|
| User deletes sidecar | Medium | Warn before delete, move to `.Trash` folder |
| File sync issues | Medium | Document cloud-safe practices |
| Hash collision | Very low | SHA256 (cryptographic strength) |
| Sharing friction | Low | Provide "Export as single file" button |

---

## Implementation Checklist (Phase 1)

### Data Structure
- [ ] JSON schema: version, imageHash, annotations[], timestamps
- [ ] Annotation types: rectangle, arrow, text, circle, highlight
- [ ] Fields per annotation: id, type, x, y, width, height, color, stroke, text, opacity

### Core Functions
- [ ] `save_to_disk(image_path)` — Write `.annotations.json`
- [ ] `load_from_disk(image_path)` — Read `.annotations.json`
- [ ] `compute_hash(image_path)` — SHA256 of image
- [ ] `validate(sidecar, image_path)` — Check hash match

### File Operations
- [ ] Filesystem watcher — Detect image renames
- [ ] Auto-rename sidecar on image rename
- [ ] Auto-move sidecar on image move
- [ ] Warning dialog if sidecar missing
- [ ] Trash handling for deleted sidecars

### UX
- [ ] Show badge "Annotations found" on images with sidecars
- [ ] "Re-edit" button loads full annotation state
- [ ] Warn if hash mismatch: "Image modified externally"
- [ ] User docs: "Both files must be shared together"

---

## Key Design Decisions

1. **Filename:** `image.png` → `image.annotations.json`
   - Rationale: Follows Immich/PhotoPrism convention, understood by ecosystem

2. **Hash Algorithm:** SHA256
   - Rationale: Cryptographic; collision impossible in practice; industry standard

3. **Format:** JSON (not YAML, not binary)
   - Rationale: Human-readable, version-controllable, tooling ubiquitous

4. **Atomic Writes:** Temp file → rename
   - Rationale: Prevents corruption if app crashes during save

5. **Fallback Strategy:** Hash mismatch → warn, don't fail
   - Rationale: User still gets UI, can choose to proceed or refresh

---

## Success Criteria

- [ ] Annotation data persists to `.annotations.json`
- [ ] Re-opening image loads all annotations
- [ ] Renaming/moving image keeps sidecar paired
- [ ] Hash mismatch detects external image modifications
- [ ] User can share image + annotations together
- [ ] Tests cover: rename, move, delete, external edit, hash collision

---

## Documentation Created

1. **ANNOTATION_STORAGE_RESEARCH.md** (6.9 KB)
   - Deep dive: 5 industry tools, detailed pros/cons per approach

2. **ANNOTATION_IMPLEMENTATION_GUIDE.md** (5.8 KB)
   - Code structure, JSON schema, Rust templates, edge case handling

3. **ANNOTATION_STORAGE_COMPARISON.md** (4.6 KB)
   - Quick comparison tables, risk analysis, decision matrix

4. **This file:** Executive summary for quick reference

---

## Next Steps

1. **Design review:** Confirm Phase 1 approach with stakeholders
2. **Schema review:** Lock down annotation JSON structure
3. **Implementation:** Build save/load functions + file watcher
4. **Testing:** Cover rename, move, delete, hash mismatch scenarios
5. **Release:** Announce "Re-edit annotations" feature

---

## References

- Full research: `/docs/ANNOTATION_STORAGE_RESEARCH.md`
- Implementation: `/docs/ANNOTATION_IMPLEMENTATION_GUIDE.md`
- Comparison: `/docs/ANNOTATION_STORAGE_COMPARISON.md`
