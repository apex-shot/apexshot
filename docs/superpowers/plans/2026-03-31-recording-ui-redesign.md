# Pre-Recording UI Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Redesign the pre-recording setup UI in the C++ capture overlay into a two-module recording deck outside the selection while keeping advanced menus contextual inside the frame.

**Architecture:** Keep the implementation centered in `capture-overlay/src/CaptureOverlay_Drawing.cpp`, but replace the current in-selection stacked recording panel with an explicit outside-the-selection deck layout model made of a primary module and a secondary module. Reuse the existing `RecordPanelTile`, `m_recTileRects`, settings popup, click options popup, and keystroke options popup so the work stays a layout and rendering refactor instead of a feature rewrite.

**Tech Stack:** C++, Qt5 Widgets, QPainter rendering, CMake, standalone `apexshot-capture` target

---

## File Structure

- Modify: `capture-overlay/src/CaptureOverlay_p.h`
  - Add shared constants and a layout struct for the outside recording deck
- Modify: `capture-overlay/src/CaptureOverlay.h`
  - Add module rect caches needed for hit-testing and popup anchoring
- Modify: `capture-overlay/src/CaptureOverlay_Drawing.cpp`
  - Compute the new pre-recording deck layout and draw the two modules
  - Keep advanced settings menus rendered inside the selection area
- Modify: `capture-overlay/src/CaptureOverlay_Events.cpp`
  - Update hit interaction assumptions for the new module rects and tile ordering
- Modify: `capture-overlay/src/CaptureOverlay_HitTest.cpp`
  - Update cursor logic to match the moved deck and module boundaries
- Verify: `capture-overlay/CMakeLists.txt`
  - No build-graph changes expected

---

### Task 1: Add Recording Deck Layout Model

**Files:**
- Modify: `capture-overlay/src/CaptureOverlay_p.h`
- Modify: `capture-overlay/src/CaptureOverlay.h`
- Test: `capture-overlay/src/CaptureOverlay_Drawing.cpp`

- [ ] **Step 1: Add the failing layout compilation surface**

Add these declarations to `capture-overlay/src/CaptureOverlay_p.h` so later drawing code can target them:

```cpp
inline constexpr double REC_DECK_GAP = 14.0;
inline constexpr double REC_PRIMARY_W = 312.0;
inline constexpr double REC_PRIMARY_H = 132.0;
inline constexpr double REC_SECONDARY_W = 260.0;
inline constexpr double REC_SECONDARY_H = 132.0;
inline constexpr double REC_DECK_TOP_GAP = 14.0;

struct RecordingDeckLayout {
    QRectF primaryModule;
    QRectF secondaryModule;
    QRectF deckBounds;
    bool placedAbove = false;
};

RecordingDeckLayout computeRecordingDeckLayout(double selX, double selY,
                                               double selW, double selH,
                                               double screenW, double screenH);
```

Add matching caches to `capture-overlay/src/CaptureOverlay.h`:

```cpp
    QRectF m_recordingPrimaryModuleRect;
    QRectF m_recordingSecondaryModuleRect;
```

- [ ] **Step 2: Run the C++ build to verify it fails before implementation**

Run:

```bash
cmake --build capture-overlay/build -j"$(nproc)"
```

Expected: FAIL at link or compile time because `computeRecordingDeckLayout(...)` is declared but not defined, or because the new fields are unused/incomplete.

- [ ] **Step 3: Implement the new deck layout function**

Add the definition near `computeToolbarLayout(...)` in `capture-overlay/src/CaptureOverlay_Drawing.cpp`:

```cpp
RecordingDeckLayout computeRecordingDeckLayout(double selX, double selY,
                                               double selW, double selH,
                                               double screenW, double screenH)
{
    RecordingDeckLayout layout;
    const double deckW = REC_PRIMARY_W + REC_DECK_GAP + REC_SECONDARY_W;
    const double deckH = std::max(REC_PRIMARY_H, REC_SECONDARY_H);

    const double preferredX = selX + (selW - deckW) / 2.0;
    const double deckX = std::max(
        FEATURE_PANEL_MARGIN,
        std::min(preferredX, screenW - deckW - FEATURE_PANEL_MARGIN)
    );

    const double belowY = selY + selH + REC_DECK_TOP_GAP;
    const bool belowFits = (belowY + deckH + FEATURE_PANEL_MARGIN) <= screenH;

    double deckY = belowY;
    if (!belowFits) {
        const double aboveY = selY - REC_DECK_TOP_GAP - deckH;
        deckY = std::max(
            FEATURE_PANEL_MARGIN,
            std::min(aboveY, screenH - deckH - FEATURE_PANEL_MARGIN)
        );
        layout.placedAbove = true;
    }

    layout.primaryModule = QRectF(deckX, deckY, REC_PRIMARY_W, REC_PRIMARY_H);
    layout.secondaryModule = QRectF(deckX + REC_PRIMARY_W + REC_DECK_GAP,
                                    deckY + std::max(0.0, (deckH - REC_SECONDARY_H) / 2.0),
                                    REC_SECONDARY_W, REC_SECONDARY_H);
    layout.deckBounds = layout.primaryModule.united(layout.secondaryModule);
    return layout;
}
```

Initialize the new member rects in `drawRecordingPanel()` before drawing:

```cpp
    const RecordingDeckLayout deck = computeRecordingDeckLayout(selX, selY, selW, selH, screenW, screenH);
    m_recordingPrimaryModuleRect = deck.primaryModule;
    m_recordingSecondaryModuleRect = deck.secondaryModule;
```

- [ ] **Step 4: Rebuild to verify the layout layer compiles**

Run:

```bash
cmake --build capture-overlay/build -j"$(nproc)"
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add capture-overlay/src/CaptureOverlay_p.h capture-overlay/src/CaptureOverlay.h capture-overlay/src/CaptureOverlay_Drawing.cpp
git commit -m "refactor: add recording deck layout model"
```

---

### Task 2: Move the Outer Recording Deck Outside the Selection

**Files:**
- Modify: `capture-overlay/src/CaptureOverlay_Drawing.cpp`
- Test: manual visual verification via built binary

- [ ] **Step 1: Remove the current in-selection panel centering logic**

Replace the current `panelX`, `startY`, `topY`, and `bottomY` calculations in `drawRecordingPanel()`:

```cpp
    double panelX = selX + (selW - topPanelW) / 2.0;
    double startY = selY + (selH - totalH) / 2.0;
    const double margin = 20.0;
    panelX = std::max(selX + margin, std::min(panelX, selX + selW - topPanelW - margin));
    startY = std::max(selY + margin, std::min(startY, selY + selH - totalH - margin));
```

with deck-based positioning:

```cpp
    const RecordingDeckLayout deck = computeRecordingDeckLayout(selX, selY, selW, selH, screenW, screenH);
    const double primaryX = deck.primaryModule.x();
    const double primaryY = deck.primaryModule.y();
    const double secondaryX = deck.secondaryModule.x();
    const double secondaryY = deck.secondaryModule.y();
```

- [ ] **Step 2: Redraw the main module as the primary deck**

Use the existing top-panel content as the basis of the primary module, but draw it inside `deck.primaryModule`:

```cpp
    drawPanelGlow(primaryX, primaryY, REC_PRIMARY_W, REC_PRIMARY_H, panelRadius);
    drawFrostedPanel(p, primaryX, primaryY, REC_PRIMARY_W, REC_PRIMARY_H, panelRadius, blurPtr, screenW, screenH);
```

Map these controls into the primary module:
- settings entry
- size / region status
- crop/fullscreen framing action
- start action area
- record type choice (`Video` / `GIF`)

When drawing the start area, use the same warm family accent introduced by the capture cockpit:

```cpp
    const QColor warmAccent(176, 92, 56);
    const QColor warmRim(255, 212, 178);
```

- [ ] **Step 3: Redraw the secondary module as the support deck**

Move the always-visible quick toggles into `deck.secondaryModule`:

```cpp
    drawPanelGlow(secondaryX, secondaryY, REC_SECONDARY_W, REC_SECONDARY_H, panelRadius);
    drawFrostedPanel(p, secondaryX, secondaryY, REC_SECONDARY_W, REC_SECONDARY_H, panelRadius, blurPtr, screenW, screenH);
```

Always-visible toggles:
- mic
- speaker
- webcam

Entry points only:
- clicks
- keystrokes
- settings

Do not permanently surface every advanced control in the support deck.

- [ ] **Step 4: Keep menus inside the frame**

Update the `drawSettingsMenu(p, panelX, startY)` call so it still opens relative to the selection area instead of the new deck:

```cpp
    if (m_settingsOpen) {
        drawSettingsMenu(p, selX + (selW - 300.0) / 2.0, selY + 24.0);
    }
```

This keeps deeper settings contextual to the selected frame rather than the outer deck.

- [ ] **Step 5: Rebuild the C++ target**

Run:

```bash
cmake --build capture-overlay/build -j"$(nproc)"
```

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add capture-overlay/src/CaptureOverlay_Drawing.cpp
git commit -m "feat: move pre-recording deck outside selection"
```

---

### Task 3: Re-map Recording Tiles and Hit-Testing

**Files:**
- Modify: `capture-overlay/src/CaptureOverlay_Drawing.cpp`
- Modify: `capture-overlay/src/CaptureOverlay_Events.cpp`
- Modify: `capture-overlay/src/CaptureOverlay_HitTest.cpp`

- [ ] **Step 1: Rebuild `m_recTileRects` in the new deck order**

In `drawRecordingPanel()`, clear and repopulate `m_recTileRects` to match the new visual order. Keep the `RecordPanelTile` enum intact, but change which rect is appended for each tile.

Primary module should append rects in this order:

```cpp
RecordPanelTile::Controls
RecordPanelTile::Size
RecordPanelTile::Crop
RecordPanelTile::RecordVideo
RecordPanelTile::RecordGif
```

Secondary module should append:

```cpp
RecordPanelTile::Mic
RecordPanelTile::Speaker
RecordPanelTile::Webcam
RecordPanelTile::Click
RecordPanelTile::Keystrokes
```

- [ ] **Step 2: Update hit-testing assumptions**

Adjust `hitTestRecordingPanel()` consumers in `capture-overlay/src/CaptureOverlay_Events.cpp` so they do not assume the old top-panel/bottom-panel geometry. Reuse the enum identities, but verify every switch arm still maps to the intended control:

```cpp
        switch (tile) {
        case RecordPanelTile::Controls:
            m_settingsOpen = !m_settingsOpen;
            break;
        case RecordPanelTile::Mic:
            m_recMic = !m_recMic;
            break;
        case RecordPanelTile::Speaker:
            m_recSpeaker = !m_recSpeaker;
            break;
        case RecordPanelTile::Click:
            m_clickOptionsOpen = true;
            break;
        case RecordPanelTile::Keystrokes:
            m_keystrokeOptionsOpen = true;
            break;
        case RecordPanelTile::Webcam:
            m_recWebcam = !m_recWebcam;
            break;
        case RecordPanelTile::RecordVideo:
            m_recordType = RecordType::Video;
            confirmRecordingSelection();
            break;
        case RecordPanelTile::RecordGif:
            m_recordType = RecordType::Gif;
            confirmRecordingSelection();
            break;
        default:
            break;
        }
```

- [ ] **Step 3: Update cursor logic for the moved deck**

In `capture-overlay/src/CaptureOverlay_HitTest.cpp`, keep `hitTestRecordingPanel()` as the authoritative tile detector, but make sure the deck no longer depends on the deck being inside the selection. The cursor should become `Qt::PointingHandCursor` whenever `hitTestRecordingPanel(pos)` returns a non-`None` tile, even when the pointer is outside the capture rect.

- [ ] **Step 4: Rebuild after interaction remap**

Run:

```bash
cmake --build capture-overlay/build -j"$(nproc)"
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add capture-overlay/src/CaptureOverlay_Drawing.cpp capture-overlay/src/CaptureOverlay_Events.cpp capture-overlay/src/CaptureOverlay_HitTest.cpp
git commit -m "refactor: remap recording deck interactions"
```

---

### Task 4: Keep Advanced Menus Contextual and Verify Placement

**Files:**
- Modify: `capture-overlay/src/CaptureOverlay_Drawing.cpp`
- Modify: `capture-overlay/src/CaptureOverlay_Events.cpp`

- [ ] **Step 1: Anchor settings/click/keystroke popups to in-frame context**

Preserve the existing popup mechanics, but ensure the popups open inside or adjacent to the selection instead of following the moved outer deck.

For settings:

```cpp
void CaptureOverlay::drawSettingsMenu(QPainter& p, double panelX, double startY)
```

should be called with in-frame anchor values derived from `selX`, `selY`, `selW`, and `selH`, not from `deck.primaryModule`.

For click and keystroke options, keep `m_clickOptionsPanelRect` and `m_keystrokeOptionsPanelRect` drawn relative to the selected frame area so they remain contextual.

- [ ] **Step 2: Add a visual fallback for above-selection placement**

When `computeRecordingDeckLayout(...).placedAbove` is true, ensure the deck stays linked and centered:

```cpp
    if (deck.placedAbove) {
        // use the same module spacing, just move the full deck above the frame
    }
```

No side-docking fallback should be introduced in this pass.

- [ ] **Step 3: Rebuild and run manual verification**

Run:

```bash
cmake --build capture-overlay/build -j"$(nproc)"
./capture-overlay/build/apexshot-capture --help >/dev/null 2>&1 || true
```

Expected: build succeeds. The binary invocation is only a smoke check that the executable is present; manual UI validation still must be performed interactively.

Manual verification:
- launch the recording setup UI
- confirm the outer deck renders below the selection when centered on screen
- move the selection near the bottom edge and confirm the deck moves above
- verify the deck remains outside the selected frame
- verify settings popup still opens inside the frame
- verify click options popup still opens inside the frame
- verify keystroke options popup still opens inside the frame
- verify mic/speaker/webcam toggles still react correctly
- verify video/GIF start actions still trigger the correct recording type

- [ ] **Step 4: Commit**

```bash
git add capture-overlay/src/CaptureOverlay_Drawing.cpp capture-overlay/src/CaptureOverlay_Events.cpp
git commit -m "feat: finalize pre-recording deck placement"
```

---

## Self-Review

- Spec coverage: the plan covers outside placement, same-family visual direction, primary/support module split, always-visible core toggles, contextual inside-frame menus, and above-selection fallback.
- Placeholder scan: no `TODO`, `TBD`, or deferred “implement later” markers remain.
- Type consistency: `RecordingDeckLayout`, `primaryModule`, `secondaryModule`, `placedAbove`, `m_recordingPrimaryModuleRect`, and `m_recordingSecondaryModuleRect` are used consistently across tasks.
