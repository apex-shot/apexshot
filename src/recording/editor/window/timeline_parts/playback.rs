pub fn seek_from_x(
    state: &Arc<Mutex<VideoEditState>>,
    media: &MediaFile,
    widget: Option<&gtk4::Widget>,
    x: f64,
) {
    let width = widget
        .map(|widget| widget.allocated_width().max(1) as f64)
        .unwrap_or(1.0);
    let mut state = state.lock().unwrap();
    let seconds = composition_x_to_source(&state, width, x);
    state.playhead_seconds = seconds;
    media.seek((seconds * 1_000_000.0) as i64);
}

pub fn pause_playback(media: &MediaFile, playing: &Cell<bool>, play_button: &Button) {
    if !playing.get() && !media.is_playing() {
        return;
    }
    media.pause();
    playing.set(false);
    let icon = Image::from_icon_name("media-playback-start-symbolic");
    icon.set_pixel_size(18);
    play_button.set_child(Some(&icon));
}

