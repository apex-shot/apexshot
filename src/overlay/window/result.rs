//! Selection result delivery and recording-request construction.
//!
//! Owns the intent → `OverlaySelection` mapping used when the user confirms
//! a capture/OCR/recording selection. Callers must release the state mutex
//! before GTK window closure (this function locks briefly then closes).

use super::super::api::{OverlaySelection, SelectionResult};
use super::super::background::BackgroundFrame;
use super::super::geometry::{current_selection_rect, selection_area_from_state};
use super::super::recording::state::OverlayIntent;
use super::super::state::SelectorState;
use crate::capture_overlay::{RecordingRequest, RecordingType};
use gtk4::prelude::*;
use gtk4::ApplicationWindow;
use std::sync::{Arc, Mutex};

/// Build a `RecordingRequest` from the current selection geometry and
/// recording-panel toggles. Coordinates are screen-space selection rects
/// (not image-mapped); fullscreen is taken from `SelectorState`.
pub(super) fn recording_request_from_state(
    st: &SelectorState,
    record_type: RecordingType,
) -> RecordingRequest {
    let area = current_selection_rect(st);
    RecordingRequest {
        x: area.left.round() as i32,
        y: area.top.round() as i32,
        width: area.width().round() as i32,
        height: area.height().round() as i32,
        record_type,
        controls: st.recording.rec_controls,
        mic: st.recording.mic_toggle,
        speaker: st.recording.speaker_toggle,
        display_rec_time: true, // always show recording time in top bar
        hidpi: st.recording.hidpi,
        notifications: st.recording.do_not_disturb,
        cursor: true,
        remember_selection: st.recording.remember_selection,
        dim_screen: st.recording.dim_screen,
        countdown: st.recording.show_countdown,
        video_max_res: st.recording.video_max_res as u8,
        video_fps: st.recording.video_fps as u8,
        record_mono: st.recording.record_mono,
        noise_suppression: st.recording.noise_suppression,
        open_editor: st.recording.open_editor,
        gif_fps: st.recording.gif_fps.round().clamp(5.0, 60.0) as u8,
        gif_quality: st.recording.gif_quality,
        gif_size_idx: st.recording.gif_size_idx as u8,
        optimize_gif: st.recording.optimize_gif,
        fullscreen: st.fullscreen_mode,
        ..RecordingRequest::default()
    }
}

/// Map the active intent + selection to an `OverlaySelection`, send it on
/// `result_tx`, and close the overlay window.
///
/// Preserves:
/// - screenshot/background coordinate mapping via `selection_area_from_state`
/// - recording request construction for `OverlayIntent::Record`
/// - OCR and area both deliver `Area(Some)` when valid
/// - invalid selection → `Area(None)` for every intent
pub(crate) fn send_selection_result(
    state: &Arc<Mutex<SelectorState>>,
    result_tx: &std::sync::mpsc::Sender<SelectionResult>,
    window: &ApplicationWindow,
    screen_width: i32,
    screen_height: i32,
    background: Option<&BackgroundFrame>,
) {
    let st = state.lock().unwrap();
    let area = selection_area_from_state(&st, screen_width, screen_height, background);
    let intent = st.intent;
    let result = match intent {
        OverlayIntent::Record => {
            if area.is_valid() {
                let record_type = st
                    .recording
                    .selected_record_type
                    .unwrap_or(RecordingType::Video);
                let request = recording_request_from_state(&st, record_type);
                Ok(OverlaySelection::Recording(request))
            } else {
                Ok(OverlaySelection::Area(None))
            }
        }
        OverlayIntent::Ocr => {
            if area.is_valid() {
                Ok(OverlaySelection::Area(Some(area)))
            } else {
                Ok(OverlaySelection::Area(None))
            }
        }
        OverlayIntent::Area => {
            if area.is_valid() {
                Ok(OverlaySelection::Area(Some(area)))
            } else {
                Ok(OverlaySelection::Area(None))
            }
        }
    };
    drop(st);
    let _ = result_tx.send(result);
    window.close();
}

#[cfg(test)]
mod tests {
    /// Owner contract: result delivery and recording-request construction live here.
    #[test]
    fn result_owner_covers_request_and_delivery() {
        let source = include_str!("result.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("production result source");
        assert!(
            production.contains("fn recording_request_from_state"),
            "result must own recording_request_from_state"
        );
        assert!(
            production.contains("fn send_selection_result"),
            "result must own send_selection_result"
        );
        assert!(
            production.contains("OverlayIntent::Record")
                && production.contains("OverlayIntent::Ocr")
                && production.contains("OverlayIntent::Area"),
            "result must map all overlay intents"
        );
        assert!(
            production.contains("OverlaySelection::Area(None)"),
            "invalid selection must still yield Area(None)"
        );
        assert!(
            production.contains("selection_area_from_state"),
            "result must keep screenshot/background coordinate mapping"
        );
        assert!(
            production.contains("window.close()"),
            "result delivery closes the overlay window"
        );
    }
}
