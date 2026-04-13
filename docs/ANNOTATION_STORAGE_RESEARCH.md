# Screenshot Tool Annotation Storage: Research Findings

## Key Findings from Industry Tools

### 1. **Snagit (TechSmith)** - Project File Approach
- Uses **SNAGX format** (Windows/Mac compatible, cross-platform)
- Previous: SNAG (Windows), SNAGPROJ (Mac)
- **Strategy**: Stores complete project with editable annotations in single file
- **Edge case handling**: Projects stored in cloud storage (Google Drive, Dropbox)
- **Workflow**: Captures save as editable projects; exports as final images

### 2. **Capture One** - Hybrid Sidecar Approach  
- Creates **CaptureOne folder** (sidecar directory) alongside images
- Stores metadata as **XMP sidecar files**
- For RAW files: Keeps originals pristine
- **Session mode**: File-based workflow with session folder structure
- **Catalog mode**: Database-based (centralized)

### 3. **Lightroom Classic** - XMP Sidecar Standard
- **Automatically writes XMP sidecar files** (.xmp) alongside image
- **Standard format**: Adobe XMP specification
- **Non-destructive**: RAW files untouched
- **Portable**: XMP sidecar moves with renamed files
- **Tool ecosystem**: Darktable, RawTherapee, PhotoPrism all support XMP sidecars

### 4. **Flameshot** (Open source)
- Limited re-editing capability currently
- Saves annotations **baked into final image** (lossy)
- **GitHub issue**: Community requesting annotation persistence (no XMP/sidecar yet)
- No built-in project file or sidecar support

### 5. **ShareX** - Moving to Project File Model
- **XIP0068 proposal**: Add "re-editing saved annotations" feature
- Goal: Match Snagit's project-file workflow
- Strategy: Preserve full editable annotations for re-editing
- Framework: New ImageEditor (Avalonia-based) to support this

---

## Approach Analysis

### **1. Sidecar JSON File** (.annotations.json)

**Pros:**
- File moves with image when renamed/relocated
- Human-readable, version-controllable
- Isolated from image file (no format constraints)
- Easy to extend with custom properties
- Works with any image format (JPEG, PNG, GIF, WebP)
- Familiar pattern (Lightroom XMP, Darktable XMP, Google Takeout)

**Cons:**
- User must manage two files (confusion, accidental deletion)
- Sharing requires both files
- Sync issues if one file deleted/corrupted
- Requires explicit pairing mechanism
- Database needed for mass operations (library view)

**Edge Cases:**
- File renamed: Sidecar filename must update (brittle)
- File moved to trash: Is sidecar orphaned?
- Duplicate image: Sidecar duplicates or shared?
- Cloud sync: Both files must stay in sync

**Implementation Complexity:** ⭐⭐ (Low-Medium)

---

### **2. Embedded Metadata (EXIF/XMP)** 

**Pros:**
- Single file (no pairing issues)
- Portable: metadata travels with image
- Standardized (Adobe XMP, EXIF, IPTC specs)
- Professional tool ecosystem support
- File rename/move doesn't break link
- Industry standard (Lightroom, Capture One, PhotoPrism)

**Cons:**
- JPEG/PNG support varies
- File size increases slightly
- Complex XML format (XMP)
- Lossy if image re-compressed without preservation
- Not supported in all formats (e.g., GIF, WebP limited)
- Tools that strip metadata lose annotations
- Raster annotations don't survive re-export

**Edge Cases:**
- Image re-exported/compressed: Metadata stripped
- Format conversion (PNG→JPEG): Metadata may not transfer
- Online sharing: Many services strip metadata for privacy
- Archive tools: May lose metadata

**Implementation Complexity:** ⭐⭐⭐ (Medium-High, requires image library)

---

### **3. Project/Session Database** (Keyed by Image Hash)

**Pros:**
- Single source of truth
- Supports advanced features (version history, collaboration)
- Fast library operations (filtering, searching)
- Works across formats
- Survives file moves/renames if keyed by content hash
- Mature pattern (Snagit projects, Capture One catalogs)
- Can sync to cloud

**Cons:**
- Heavy: Requires database setup
- File portability reduced (database bound to project)
- Offline access limited
- Data loss risk (database corruption)
- Content hash computation overhead
- Sharing requires exporting project
- Platform-specific (per OS)

**Edge Cases:**
- Image hash collision (rare but possible)
- Duplicate images: Same hash, shared annotations?
- External edits: User modifies image outside app (hash breaks)
- Database migration: Schema changes complex
- Backups: Both DB and images must sync

**Implementation Complexity:** ⭐⭐⭐⭐⭐ (High, requires DB infrastructure)

---

## Recommendation by Use Case

### **Use Case 1: Simple Re-editing (Like Flameshot)**
**RECOMMENDATION: Sidecar JSON**
- Flameshot users want quick re-editing capability
- Lightweight, no database overhead
- Convention: `.image.annotations.json` or `.image.snagshot.json`
- Implementation: Simple JSON serialization of annotation objects
- **Edge case handling**:
  - Auto-detect renamed files in same directory
  - Warn if sidecar missing
  - Soft-delete: Move orphaned sidecars to .trash folder

### **Use Case 2: Professional Workflow (Photo Editing)**
**RECOMMENDATION: XMP Sidecar (Standard)**
- Industry standard (Lightroom, Darktable, PhotoPrism all support)
- Interoperable with existing tools
- Better than custom JSON (ecosystem integration)
- **Implementation**: Use exifr library + XMP namespace
- **Edge case handling**:
  - File moves: Re-scan by hash + location heuristic
  - Fallback to JSON sidecar if XMP unsupported

### **Use Case 3: Enterprise/Collaboration (Snagit Model)**
**RECOMMENDATION: Project File + Database**
- Team needs version history, sharing, cloud sync
- Single .snagx format (or .apexshot for ApexShot)
- ZIP container: metadata.json + rasterized images + thumbnails
- Database for library UI, filtering, searching
- **Edge case handling**:
  - Project export: Extract images + JSON sidecars
  - Cloud sync: Detect conflicts, merge versions

---

## Recommended Hybrid Approach for ApexShot

**Best of all three worlds:**

1. **Primary:** Sidecar JSON (`.image.annotations.json`)
   - Simple, portable, human-readable
   - One-file-pair model (easy to share)

2. **Secondary:** Optional XMP embedding
   - Fallback for portability
   - Interop with other tools

3. **Optional:** Lightweight SQLite database
   - For library operations (search, filter, batch ops)
   - Keyed by content hash + filename
   - Auto-repairs from sidecars if corrupted

**Implementation path:**
- Phase 1: Sidecar JSON only (MVP)
- Phase 2: Add XMP embedding (professional workflow)
- Phase 3: Add optional library database (advanced)

---

## Naming Convention Recommendation

**Sidecar filename pattern:**
```
screenshot.png → screenshot.annotations.json
  or
screenshot.png → screenshot.apexshot.json  (branded)
```

**Benefits of .annotations.json:**
- Generic, understood by other tools
- Follows Immich/PhotoPrism convention
- Less confusion than tool-specific suffix

**Hash-based tracking (Phase 3):**
- Store in database: `imageHash (SHA256) → annotations_path`
- Allows re-linking after renames/moves
- Detects external modifications

