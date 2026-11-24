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
    // Seed the toolbar's device menus with the last chosen devices.
    if !config.rec_mic_device.is_empty() {
        extra_args.push(format!("--rec-mic-device={}", config.rec_mic_device));
    }
    if !config.rec_speaker_app.is_empty() {
        // Only the name is remembered; a pid does not survive a restart, so
        // the toolbar looks it up from the app list.
        extra_args.push(format!("--rec-speaker-app={}", config.rec_speaker_app));
    }
    // The toolbar greys out the one-app choice where the recorder cannot
    // capture it.
    extra_args.push(format!(
        "--rec-app-audio={}",
        if crate::recording::app_audio_capture_available() {
            1
        } else {
            0
        }
    ));
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

    extra_args
}

fn build_recording_ui_args(config: &crate::config::AppConfig) -> Vec<String> {
    let mut args = build_area_init_args(config);
    args.push("--open-recording-ui".into());
    args
}

fn build_quick_capture_args(config: &crate::config::AppConfig) -> Vec<String> {
    let mut args = build_area_init_args(config);
    args.push(format!(
        "--screenshot-cursor={}",
        if config.screenshot_show_cursor { 1 } else { 0 }
    ));
    args.push("--capture-menu".into());
    args
}

fn build_quick_capture_args_with_menu(
    config: &crate::config::AppConfig,
    menu: crate::overlay::CaptureMenuResult,
) -> Result<Vec<String>, String> {
    let menu_json = quick_capture_menu_result_json(menu)?;
    let mut args = build_quick_capture_args(config);
    args.push(format!("--capture-menu-result={menu_json}"));
    Ok(args)
}

fn quick_capture_menu_result_json(
    menu: crate::overlay::CaptureMenuResult,
) -> Result<String, String> {
    use crate::overlay::CaptureMenuAction as Action;

    let valid_action = match menu.action {
        Action::Display => !menu.ocr,
        Action::Window => menu.timer_seconds == 0 && !menu.ocr,
        Action::Area => !menu.recording && menu.timer_seconds == 0,
        Action::Cancel => false,
    };
    let valid_mode = if menu.recording {
        !menu.ocr && menu.timer_seconds == 0
    } else {
        !(menu.ocr && menu.timer_seconds > 0)
            && menu.timer_seconds <= 10
            && matches!(menu.timer_seconds, 0 | 3 | 5 | 10)
    };

    if !valid_action || !valid_mode {
        return Err("Unsupported Quick Capture menu state".into());
    }

    serde_json::to_string(&menu).map_err(|error| format!("Serialize Quick Capture menu: {error}"))
}

fn build_crosshair_args(config: &crate::config::AppConfig) -> Vec<String> {
    let mut args = vec!["--crosshair-capture".into()];
    append_screenshot_timer_args(&mut args, config);
    // The crosshair selector freezes the screen before it becomes visible, and
    // the still it captures can include the pointer. Forward both settings so
    // the shortcut matches Settings instead of always taking overlay defaults.
    args.push(format!(
        "--freeze-selection-bg={}",
        if config.screenshot_freeze_screen { 1 } else { 0 }
    ));
    args.push(format!(
        "--screenshot-cursor={}",
        if config.screenshot_show_cursor { 1 } else { 0 }
    ));
    args
}
