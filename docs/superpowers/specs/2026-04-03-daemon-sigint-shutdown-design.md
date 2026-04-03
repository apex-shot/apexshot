# Daemon SIGINT and post-recording shutdown design

## Goal
Restore normal daemon terminal shutdown behavior after recordings while keeping recording controls bound only to explicit recording actions.

## Requirements
- `Ctrl+C` must not act as a recording control.
- While recording is active, `Ctrl+C` in the daemon terminal is ignored.
- When no recording is active, `Ctrl+C` exits `apexshot daemon` cleanly.
- Tray `Quit` remains a direct daemon quit action and must continue to work after recordings.

## Design
1. Remove process-wide `Ctrl+C` handling from the recording engine loops.
2. Track whether a recording session is currently active via the recording control session registry.
3. Install daemon-owned SIGINT handling in the daemon runtime:
   - active recording -> log and ignore
   - idle -> enqueue `Quit`
4. Keep tray `Quit` mapped directly to daemon shutdown.

## Validation
- unit test that active recording state toggles with control-session registration lifecycle
- unit test that daemon SIGINT policy ignores when recording is active and exits when idle
