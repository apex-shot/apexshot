fn append_screenshot_timer_args(args: &mut Vec<String>, config: &crate::config::AppConfig) {
    if config.screenshot_timer_interval == 0 {
        args.push("--hide-timer".into());
    } else {
        args.push("--show-timer".into());
        args.push(format!(
            "--timer-seconds={}",
            config.screenshot_timer_interval
        ));
    }
}

fn build_area_init_args(config: &crate::config::AppConfig) -> Vec<String> {
    let mut extra_args: Vec<String> = vec!["--area-init".into()];

    if config.rec_remember_selection {
        if let (Some(x), Some(y), Some(w), Some(h)) = (
            config.last_selection_x,
            config.last_selection_y,
            config.last_selection_w,
            config.last_selection_h,
        ) {
            extra_args.push(format!("--restore-selection={x},{y},{w},{h}"));
        }
    }

    extra_args.push(format!(
        "--selection-cursor={}",
        config.screenshot_crosshair_mode
    ));
    extra_args.push(format!(
        "--show-zoom-preview={}",
        if config.screenshot_show_magnifier {
            1
        } else {
            0
        }
    ));
    extra_args.push(format!(
        "--freeze-selection-bg={}",
        if config.screenshot_freeze_screen {
            1
        } else {
            0
        }
    ));
    append_screenshot_timer_args(&mut extra_args, config);

    if config.rec_mic {
        extra_args.push("--rec-mic".into());
    }
    if config.rec_speaker {
        extra_args.push("--rec-speaker".into());
    }
    extra_args.push(if config.rec_controls {
        "--rec-controls".into()
    } else {
        "--no-rec-controls".into()
    });
    // Recording controls always show elapsed time; the old setting is unused.
    extra_args.push("--display-rec-time".into());
    extra_args.push(if config.rec_hidpi {
        "--hidpi".into()
    } else {
        "--no-hidpi".into()
    });
    extra_args.push(if config.rec_notifications {
        "--do-not-disturb".into()
    } else {
        "--no-do-not-disturb".into()
    });
    extra_args.push(if config.rec_cursor {
        "--show-cursor".into()
    } else {
        "--no-show-cursor".into()
    });
    extra_args.push(if config.rec_remember_selection {
        "--remember-selection".into()
    } else {
        "--no-remember-selection".into()
    });
    extra_args.push(if config.rec_dim_screen {
        "--dim-screen".into()
    } else {
        "--no-dim-screen".into()
    });
    extra_args.push(if config.rec_countdown {
        "--show-countdown".into()
    } else {
        "--no-show-countdown".into()
    });
    extra_args.push(format!("--video-max-res={}", config.rec_video_max_res));
    extra_args.push("--video-format=0".to_string());
    extra_args.push(format!("--video-fps={}", config.rec_video_fps));
    extra_args.push(if config.rec_video_mono {
        "--record-mono".into()
    } else {
        "--no-record-mono".into()
    });
    extra_args.push(if config.rec_video_open_editor {
        "--open-editor".into()
    } else {
        "--no-open-editor".into()
    });
    extra_args.push(format!("--gif-fps={}", config.rec_gif_fps));
    extra_args.push(format!("--gif-quality={:.4}", config.rec_gif_quality));
    extra_args.push(format!("--gif-size={}", config.rec_gif_size_idx));
    if config.rec_gif_optimize {
        extra_args.push("--gif-optimize".into());
    } else {
        extra_args.push("--no-gif-optimize".into());
    }

    extra_args
}

fn build_recording_ui_args(config: &crate::config::AppConfig) -> Vec<String> {
    let mut args = build_area_init_args(config);
    args.push("--open-recording-ui".into());
    args
}

fn build_quick_capture_args(config: &crate::config::AppConfig) -> Vec<String> {
    let mut args = build_area_init_args(config);
    args.push("--capture-menu".into());
    args
}

fn build_crosshair_args(config: &crate::config::AppConfig) -> Vec<String> {
    let mut args = vec!["--crosshair-capture".into()];
    append_screenshot_timer_args(&mut args, config);
    args
}
