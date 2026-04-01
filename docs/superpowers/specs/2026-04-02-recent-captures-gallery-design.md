# Recent Captures Gallery Design

## Summary

Replace the tray's current last-capture behavior with a dedicated Recent Captures gallery window. The tray entry should no longer jump straight into the floating preview overlay. Instead, it should open a purpose-built gallery window that presents the newest capture as a featured item at the top and shows additional recent captures below in a more expressive contact-sheet layout.

The gallery should borrow the level of polish and window craftsmanship from the existing Settings UI, but it must not feel like a recycled settings tab or a generic asset browser. The visual direction is editorial rather than dashboard-like: strong spacing, intentional typography, crisp surfaces, and asymmetric composition.

## Goals

- Make the tray's last-capture entry feel like a real destination, not a redirect.
- Surface recent captures in a dedicated window that can grow over time.
- Keep the existing floating preview overlay as the per-capture viewing experience.
- Reuse existing windowing/theme helpers where that improves consistency without forcing settings-specific structure into the gallery.

## Non-Goals

- Replacing the floating preview overlay.
- Building a full media library with search, tagging, folders, or multi-select.
- Adding new capture persistence rules beyond reading from already-saved capture locations.
- Embedding preview-overlay editing controls into the gallery window.

## User Experience

### Tray Entry

The tray action currently associated with last capture should open the Recent Captures gallery window.

If there is a separate tray action for directly showing the old preview overlay flow, it may remain available, but the gallery becomes the primary entry point for browsing recent captures.

### Window Shell

The gallery should open in its own standalone GTK window. It should reuse the custom window chrome patterns already used by Settings:

- undecorated custom frame
- traffic-light window controls
- shared drag/resize behavior
- shared theme and CSS support where practical

The interior must not mirror the settings tab-strip layout. This is a gallery-first window, not a preferences surface.

### Visual Direction

The gallery should follow an editorial contact-sheet identity:

- latest capture featured prominently at the top
- recent captures below in an offset grid rather than a monotonous uniform table
- warm neutral surfaces in light mode, dark ink text, and sparing accent use
- crisp borders, disciplined spacing, and low-noise hover states
- deliberate typography that feels closer to a creative desktop tool than a generic admin panel

Avoid the common "AI slop" traps:

- no generic translucent card farm
- no default dashboard spacing/tokens without adjustment
- no overly symmetrical, template-like blocks
- no excessive glow, blur, or ornamental gradients that reduce clarity

### Featured Latest Capture

The top section should center the newest capture as the hero element of the window. It should include:

- a large thumbnail preview
- filename or title treatment
- human-readable timestamp
- supporting metadata if available and useful
- primary action to open the floating preview overlay for that capture
- secondary actions such as opening the file directly or revealing it in its folder

The featured area should feel distinct from the grid below through stronger scale, typography, and spacing rather than decorative gimmicks.

### Recent Capture Grid

Below the featured item, the gallery should show additional recent captures in reverse chronological order. Each card should include:

- image thumbnail
- file name
- timestamp
- light metadata if available

Cards should feel individually designed and readable at a glance. Clicking a card opens that specific file in the existing floating preview overlay.

The grid may be visually uneven or rhythm-based, but it must remain easy to scan and consistent enough to maintain trust.

### Empty and Error States

If no recent captures are found, the gallery should show an intentional empty state with guidance instead of a blank pane.

If a listed file is missing or cannot be opened:

- the window should stay responsive
- the failing item should not crash the gallery
- the user should get a clear, lightweight error indication
- refresh should allow the gallery to recover if the filesystem state changes

## Data and Discovery

The gallery should discover recent captures from the application's existing saved-capture locations and ordering rules. The first implementation should not invent a new database or manifest format.

Expected behavior:

- gather recently saved capture files from configured export locations
- sort by recency
- use the newest item as the featured capture
- populate the remaining items into the recent grid
- ignore unsupported or unreadable files gracefully

If discovery must be bounded for performance, a fixed recent-item cap is acceptable as long as the newest captures are prioritized.

## Architecture

### New Window Module

Add a dedicated GTK module for the Recent Captures gallery window rather than folding this into the settings module or the preview overlay module.

Responsibilities:

- create/present the gallery window
- load recent capture metadata
- build featured and grid sections
- wire item clicks to the existing floating preview overlay
- expose refresh/reload behavior

### Shared UI Support

Reuse settings window support utilities when they are genuinely shared infrastructure, especially:

- CSS/theme installation
- traffic-light buttons
- drag/resize helpers
- reduced-transparency and theme preference helpers

Do not reuse settings-specific navigation or save workflows.

### Integration With Existing Preview Overlay

The gallery does not replace preview rendering logic. Instead, selecting a featured item action or clicking a grid card should call into the existing floating preview overlay flow for the selected file path.

This keeps the current per-capture viewing behavior intact while upgrading the tray entry experience.

### Tray Wiring

Update the tray action handling so the last-capture-oriented tray entry launches the gallery window instead of immediately showing the floating preview overlay.

If the current action names are too preview-specific, rename them so the intent is explicit in code.

## Interaction Details

- Opening the tray gallery should present the window immediately even if thumbnail loading continues.
- Thumbnail generation/loading should not freeze the GTK UI thread.
- Clicking a recent item should open the preview overlay for that file.
- The gallery window may remain open after launching the overlay unless current UX patterns strongly suggest closing it.
- Refresh should reload the current recent-capture view from disk.

## Testing

Testing should cover:

- tray action routing to the gallery instead of direct preview
- recent capture discovery and ordering
- featured-item selection from the newest capture
- click behavior opening the existing preview overlay for the correct file
- empty state behavior when no captures are found
- graceful handling of missing/unreadable files

Where GTK UI tests are impractical, isolate file-discovery and selection logic so it can be tested without the full window.

## Risks and Mitigations

### Risk: Generic-looking UI despite custom styling

Mitigation: make layout hierarchy, spacing, card rhythm, and typography part of the implementation work, not just color/CSS changes.

### Risk: Slow directory scanning or thumbnail loading

Mitigation: bound the recent-item set, separate discovery from rendering where possible, and avoid blocking first paint on all thumbnails.

### Risk: Code duplication with settings window infrastructure

Mitigation: extract only the truly shared chrome/theme helpers and keep gallery-specific composition in its own module.

## Open Decisions Resolved

- The gallery is a standalone window, not a settings tab.
- The latest capture is featured at the top.
- Selecting an item opens the existing floating preview overlay.
- The gallery should show multiple recent captures immediately, not only the single newest capture.
