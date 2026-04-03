# Shortcuts Tab + Recording UI Entry Design

Date: 2026-04-03
Topic: Shortcuts settings as hotkey source of truth, expanded shortcut coverage, and direct recording UI entry

## Summary

This change makes the **Settings → Shortcuts** tab the source of truth for ApexShot hotkeys. Whatever shortcut values are configured there and saved should be the same values used by the app’s runtime hotkey system.

The Shortcuts tab will also be expanded to include all user-facing app shortcuts, including recording-related actions. A new shortcut, **Open Recording UI**, will open the recording UI directly instead of first entering area screenshot capture.

A non-blocking tip will be added at the top of the Shortcuts tab to explain that desktop environments may already reserve some key combinations, and users may need to disable conflicting system shortcuts in their OS keyboard settings.

## Goals

- Make the Shortcuts tab the canonical source for app hotkeys.
- Ensure saved shortcut values are actually used by the runtime hotkey system.
- Expand the tab to cover all relevant app shortcuts, including recording actions.
- Add an **Open Recording UI** shortcut that navigates directly to recording UI.
- Keep shortcut-conflict handling user-friendly and non-blocking.

## Non-Goals

- Automatic detection of all desktop/global shortcut conflicts.
- Blocking save when a conflict is suspected.
- Automatically starting recording from a shortcut.
- Broad unrelated refactors outside shortcut/config/runtime wiring needed for this feature.

## User Experience

### Shortcuts tab behavior

The Shortcuts tab becomes the place where users define the hotkeys ApexShot uses.

When a user edits a shortcut and clicks Save:
- the value is persisted in settings/config
- the runtime hotkey configuration is updated to match
- ApexShot uses that exact saved value for the corresponding action

### Top-of-tab notice

A notice appears at the top of the Shortcuts tab and communicates:
- the shortcuts configured here are the shortcuts ApexShot uses
- some shortcuts may not work if the desktop environment already uses them
- if needed, users should open their system keyboard shortcut settings and disable conflicting shortcuts there before using the same keys in ApexShot

This notice is informational only. It does not block saving.

### Shortcut coverage

The Shortcuts tab should include all relevant shortcuts exposed by the app, grouped clearly.

Expected categories:
- General shortcuts
- Screenshot shortcuts
- Recording shortcuts

Expected recording-related entries:
- Open Recording UI
- Pause/Resume Recording
- Stop and Save Recording
- Restart Recording
- Discard Recording

Recording control shortcuts should be visibly labeled as **Only during recording**.

## Functional Design

### 1. Settings as source of truth

The current settings-side shortcut fields and the runtime hotkey bindings must be aligned so that there is one authoritative representation per action.

Design requirements:
- each visible shortcut row in the Shortcuts tab maps to a specific runtime action
- saving settings updates the stored shortcut values for those actions
- runtime hotkey loading/registration uses those stored values rather than a separate divergent shortcut list
- default values remain defined in one place or are synchronized deterministically

If a shortcut is blank, the corresponding runtime binding should be treated as disabled.

### 2. Expanded shortcut model

The config model must be extended to include any missing actions needed by the fully expanded Shortcuts tab, especially the recording actions not currently represented there.

This includes adding config fields for:
- Open Recording UI
- Pause/Resume Recording
- Stop and Save Recording
- Restart Recording
- Discard Recording

If additional app-exposed shortcuts exist but are not currently represented in the tab, they should be reviewed and included as part of this expansion so the tab matches real hotkey coverage.

### 3. Open Recording UI action

A new hotkey action, **Open Recording UI**, will be introduced.

Behavior:
- when triggered, ApexShot opens directly into the recording UI flow
- it must not first enter the area screenshot capture flow
- it must not automatically start recording
- the user lands in the recording UI and then manually starts recording from there

This action is an entry/navigation action, not a capture action.

### 4. Recording control actions

Recording control shortcuts remain context-sensitive.

Behavior:
- they are editable in the Shortcuts tab
- they are labeled as **Only during recording** in the UI
- they only function while a recording session/UI context is active
- when outside recording context, they should not trigger unrelated screenshot behavior

### 5. Conflict handling

Conflict handling remains non-blocking.

Behavior:
- users can save any supported shortcut string
- the app does not reject saves solely because a desktop conflict may exist
- guidance is provided via the top notice instead of hard validation
- runtime behavior continues to rely on the desktop/hotkey backend’s actual ability to register and deliver the shortcut

## UI Design

## Shortcuts tab additions

The Shortcuts tab should be updated to:
- add a top informational notice above the shortcut groups
- add missing rows for recording actions
- visually annotate recording-only controls with a secondary hint such as **Only during recording**
- preserve existing visual style for shortcut rows and groups

Suggested grouping:

### General
- Toggle Desktop Icons
- Open File
- Open From Clipboard
- Pin to the Screen
- Restore Recently Closed File
- Hide/Show Overlays

### Screenshots
- Capture Area
- Crosshair Capture
- Capture Previous Area
- Capture Full Screen
- Capture Window
- any other existing screenshot hotkeys that are runtime-supported

### Recording
- Open Recording UI
- Pause/Resume Recording — Only during recording
- Stop and Save Recording — Only during recording
- Restart Recording — Only during recording
- Discard Recording — Only during recording

## Architecture / Data Flow

### Save path

1. User edits shortcut values in Settings → Shortcuts.
2. User clicks Save.
3. Settings save flow writes updated shortcut values into app config.
4. Runtime hotkey configuration is regenerated or synchronized from the saved config.
5. Hotkey daemon/runtime registration uses those saved bindings.

### Activation path

1. User presses a configured shortcut.
2. Hotkey backend resolves it to a runtime action.
3. ApexShot dispatches the mapped action.
4. For **Open Recording UI**, dispatch goes directly to recording UI.
5. For recording controls, dispatch only has effect when recording context is active.

## Implementation Notes

Likely touched areas:
- `src/settings/shortcuts.rs` for expanded rows and top notice
- `src/settings/actions.rs` and related settings save plumbing
- `src/config.rs` for new shortcut fields/defaults/serialization
- runtime hotkey binding definitions in `src/hotkeys/mod.rs`
- daemon action mapping in `src/daemon/mod.rs`
- any recording entry flow needed to bypass screenshot area capture and open recording UI directly

The implementation should follow existing config and action naming patterns where possible.

## Error Handling

- Invalid or unsupported shortcut registration at runtime should continue to surface through existing daemon/backend logging or user-visible feedback patterns where already present.
- Save should not fail purely because of a potential OS-level conflict.
- Blank shortcuts should disable the associated action safely.

## Testing

Required verification areas:

### Config tests
- defaults include any newly added shortcut fields
- YAML round-trip preserves new shortcut fields
- blank shortcut values serialize and load correctly

### Mapping tests
- shortcut-to-action mapping includes **Open Recording UI**
- recording control mappings remain correct
- disabled/blank shortcuts do not create active bindings

### Settings save tests
- saving from the Shortcuts tab writes the expected config values
- saved values are the same values consumed by runtime hotkey loading

### Behavior tests
- **Open Recording UI** shortcut opens recording UI directly
- it does not route through area screenshot capture first
- recording control shortcuts only act during recording context

## Risks

- Settings config and runtime hotkey config may currently be partially duplicated; alignment work may uncover inconsistencies.
- The current screenshot/recording entry flow may assume screenshot-first navigation and require targeted adjustment.
- Adding all shortcut rows may reveal actions that exist in runtime but not yet in user-facing config naming conventions.

## Recommendation

Implement this as one focused shortcut-system alignment change:
1. expand config and UI coverage
2. unify settings values with runtime hotkey bindings
3. add direct **Open Recording UI** action
4. verify recording control behavior remains context-sensitive

This approach satisfies the product request without overcomplicating conflict detection or adding premature validation logic.
