# Windows Scroll Capture Plan

Status: implemented in the Windows Quick Access shell.

## Goal

Stitch a tall window or a selected area into one long screenshot on Windows.
This is a still-image feature, not video; the browser path already has its own
extension, so this covers native windows and in-app content.

## Scrolling

`platform/windows/scroll_capture.cpp` uses UI Automation first:

1. Foreground the target and find the element under the capture region's
   centre with `IUIAutomation::ElementFromPoint`, walking its ancestors until
   one exposes `IUIAutomationScrollPattern` and reports
   `CurrentVerticallyScrollable`.
2. If the ancestor walk fails, search the target window's descendants for the
   first element with `UIA_IsScrollPatternAvailablePropertyId` and a vertical
   pattern.
3. Advance with `SetScrollPercent`: half of `CurrentVerticalViewSize`, capped
   at 12 percentage points. Half a viewport leaves roughly fifty percent
   overlap between frames.
4. End detection reads `CurrentVerticalScrollPercent`. The capture stops at
   100%, or after two settled frames that did not move the percentage. This is
   exact and does not depend on guessing from pixels.

Why UI Automation: `SendInput` wheel input goes to the focused window and is
subject to UIPI. Modern apps (WinUI 3 Notepad, Chromium, WPF) expose scroll
providers that scroll without focus and report their position. Several apps
also ignore `WM_MOUSEWHEEL` posted to the top-level window.

Fallbacks, in order, when no scroll pattern is available or a pattern call
fails:

- real wheel input via `SendInput` after re-asserting foreground with
  `AttachThreadInput`;
- `WM_MOUSEWHEEL` posted to the deepest child under the region;
- image-similarity end detection in place of the scroll percentage.

A provider that exposes a pattern but ignores `SetScrollPercent` gets one
wheel attempt before the capture is declared finished.

The scrolling capture implementation we studied uses the same wheel +
row-match strategy; its fallback ordering and fixed-bottom handling informed
this design.

## Stitching

The primary stitcher is row-exact:

- Compare full rows of the stitched content against the current frame,
  including the left text so line numbers disambiguate repeated content, and
  skipping only the right margin where scrollbars live.
- Find the current row that lines up with the bottom of the stitched content;
  append everything below it.
- Detect fixed top and bottom bands (toolbars, status bars) from the first pair
  of frames, keep each once, and stitch only the scrolling band between them.
- Stop after a maximum of 120 frames or 24,000 pixels.

A sampled template matcher remains as a fallback for content that is not
pixel-stable between frames (subpixel smooth scrolling, animations). It is
constrained by the UIA scroll delta when one is available, which prevents
repetitive text from matching one line off.

## UI

- The capture toolbar gains a **Scroll** cell, next to Window and Area. While
  armed, Display, OCR, and Timer are disabled; Window and Area choose the
  source. The tray keeps **Scroll capture → Window / Area** for direct access.
  The cell is opt-in, so the Linux overlay's 8-item toolbar is unchanged.
- The same large countdown circle used by recording, so the target can be
  arranged before the first frame.
- A click-through, capture-excluded mask dims everything outside the scroll
  region.
- A segmented bar shows the frame count with **Done** and a discard button.
  Done stitches what has been captured so far.
- The result card previews the first screenful of the tall image, reports its
  true dimensions and file size, opens the PNG, and copies the image to the
  clipboard. The file is written to the temp capture directory.

## Verified

Against a 400-line Notepad document (a WinUI 3 app) at 1234×1017:

- UIA found the `Text editor` element and advanced 0% → 99.97%.
- The stitched image was 1234×8973 and continuous from line 1 to line 400;
  top, middle, and bottom crops had no gaps or duplicates.
- The exact percentage stopped the capture without waiting on image guesses.

## Limits

- The scroll provider is only as good as the app's UI Automation tree. Classic
  controls without one fall back to wheel input and image end detection.
- Virtualized or infinite lists may grow while capturing; the frame and height
  caps bound the result.
- A fixed header taller than 28% of the view, or a changing header (clocks,
  progress), can defeat fixed-band detection.
- Horizontal scrolling is out of scope; the capture advances vertically.
