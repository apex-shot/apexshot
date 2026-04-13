# Annotation Storage: Quick Comparison

## Three Approaches Analyzed

| Aspect | Sidecar JSON | Embedded XMP | Database |
|--------|---|---|---|
| **Files** | 2 files (image + .annotations.json) | 1 file (metadata in image) | 1 DB + image files |
| **Portability** | Good (both files move together) | Excellent (single file) | Poor (tied to DB) |
| **Sharing** | Requires both files | Single file | Requires export |
| **Offline Access** | Full | Full | Full (local DB) |
| **Implementation** | ⭐⭐ Easy | ⭐⭐⭐ Medium | ⭐⭐⭐⭐⭐ Hard |
| **File Move/Rename** | Requires watcher | Automatic | Hash-based lookup |
| **Metadata Loss** | No | Yes (if recompressed) | No |
| **Tool Ecosystem** | Basic | Excellent (Lightroom, Darktable) | Vendor-locked |
| **User Confusion** | Medium (two files) | None (single file) | None (transparent) |
| **Best For** | Quick re-editing | Professional workflows | Team collaboration |

---

## Industry Leaders' Choices

| Tool | Approach | Notes |
|------|----------|-------|
| **Snagit** | Project File (ZIP) | Enterprise standard; cloud-synced |
| **Lightroom** | XMP Sidecar | Professional; ecosystem integration |
| **Capture One** | XMP + Sessions | Hybrid: file-based (sessions) or database (catalog) |
| **Darktable** | XMP Sidecar | Open source; standard compliance |
| **Flameshot** | None (lossy) | Community asking for annotation persistence |
| **ShareX** | Moving to project file | XIP0068 proposal (in progress) |

---

## ApexShot Recommendation: Phased Approach

### Phase 1: Sidecar JSON (MVP)
**Why:** Low implementation cost, transparent to users, works everywhere.
- Filename: `screenshot.png` → `screenshot.annotations.json`
- Schema: Version, imageHash, annotation array, timestamp
- Detection: File watcher for renames/moves

**Time to MVP:** 2-3 weeks

### Phase 2: Optional XMP (Professional)
**Why:** After proving MVP, add interop with pro tools.
- Requires: `exifr` crate + XMP namespace
- Fallback: XMP for JPEG, sidecar for others

**Time to implementation:** 2-3 weeks

### Phase 3: Optional Database (Advanced)
**Why:** Large libraries, batch ops, team features.
- Requires: SQLite + content hash indexing
- Scope: Not in MVP; revisit after 10k+ screenshots

**Time to implementation:** 4-6 weeks

---

## Risk Analysis

### Sidecar JSON Risks
| Risk | Probability | Impact | Mitigation |
|------|---|---|---|
| User deletes sidecar | Medium | High | Warn on delete, move to trash |
| File sync issues (cloud) | Medium | Medium | Document best practices |
| Sharing friction | Low | Low | Provide "Export as single file" UI |
| Hash collision | Very Low | Low | Use SHA256 (cryptographic) |

### XMP Risks
| Risk | Probability | Impact | Mitigation |
|------|---|---|---|
| Metadata stripped by cloud | Medium | High | Fallback to sidecar |
| Format incompatibility | Low | Medium | Test all formats before release |
| Library dependency | Low | High | Vendor: use exifr (stable) |

### Database Risks
| Risk | Probability | Impact | Mitigation |
|------|---|---|---|
| Database corruption | Low | Very High | Regular backups, repair tools |
| Schema migration | Medium | High | Version migrations before release |
| Performance (large libraries) | Medium | Medium | Index on imageHash |

---

## Feature Comparison Table

| Feature | Sidecar | XMP | Database |
|---------|---------|-----|----------|
| Re-edit saved annotations | ✓ | ✓ | ✓ |
| Single file sharing | ✗ | ✓ | ✗ |
| Cloud-safe | ✓ (with care) | ✗ | ✓ (with sync) |
| Batch operations | ✗ | ✗ | ✓ |
| Version history | ✗ | ✗ | ✓ |
| Collaboration | ✗ | ✗ | ✓ |
| Offline library | ✓ | ✓ | ✓ |
| Tool interop | ✗ | ✓ | ✗ |
| Search/filter | ✗ | ✗ | ✓ |

---

## Decision Matrix for ApexShot

**MVP Goal:** Allow users to re-edit saved screenshots

**Must Haves:**
- Re-edit annotations ✓ (all approaches)
- Easy file management ✓ (sidecar: watcher; XMP: automatic; DB: transparent)
- Low complexity ✓ (sidecar: best; XMP: medium; DB: worst)

**Nice to Haves:**
- Tool interop ✓ (XMP only)
- Batch ops ✓ (DB only)
- Single file ✓ (XMP only)

**Constraint:** Ship in 2-3 weeks

**Verdict:** **Sidecar JSON (Phase 1)** → Easy MVP, good UX, clear upgrade path

---

## References

- Snagit SNAGX: TechSmith's format for cross-platform projects
- Lightroom XMP: Adobe's industry standard (https://helpx.adobe.com/lightroom-classic/help/create-xmp-acr-files.html)
- PhotoPrism Sidecars: Open-source tool documentation
- Capture One Sessions: Professional photo workflow standard
- ShareX XIP0068: "Re-editing saved annotations" proposal
- PNG/EXIF: W3C specification (https://www.w3.org/TR/png-3/)
