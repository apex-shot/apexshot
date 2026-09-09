#[cfg(test)]
mod tests {
    use super::{
        append_screenshot_timer_args, build_area_init_args, build_crosshair_args,
        build_recording_ui_args, classify_overlay_exit_code, execute_builtin_overlay_query,
        is_gnome_wayland_session_from_env, parse_area_capture_output_with_persist,
        parse_area_capture_output_with_stderr, parse_capture_screen_json,
        parse_capture_screen_json_with_mode, parse_recording_json, parse_selection_json,
        save_capture_to_temp_png, should_request_screenshot_lock,
        should_use_gtk_layer_shell_selector_from_env, tracked_overlay_id,
        CaptureSessionCoordinator, LaunchBlockedReason, OverlayExitCode, OverlaySelection,
        RecordingType,
    };
    use crate::{
        backend::{CaptureData, PixelFormat},
        config::AppConfig,
    };

    #[test]
    fn crosshair_capture_does_not_build_area_init_settings_args() {
        let config = AppConfig {
            screenshot_timer_interval: 0,
            ..AppConfig::default()
        };
        assert_eq!(
            build_crosshair_args(&config),
            vec!["--crosshair-capture", "--hide-timer"]
        );
    }

    #[test]
    fn screenshot_timer_args_follow_setting() {
        let mut off_args = Vec::new();
        append_screenshot_timer_args(
            &mut off_args,
            &AppConfig {
                screenshot_timer_interval: 0,
                ..AppConfig::default()
            },
        );
        assert_eq!(off_args, vec!["--hide-timer"]);

        let mut on_args = Vec::new();
        append_screenshot_timer_args(
            &mut on_args,
            &AppConfig {
                screenshot_timer_interval: 3,
                ..AppConfig::default()
            },
        );
        assert_eq!(on_args, vec!["--show-timer", "--timer-seconds=3"]);
    }

    #[test]
    fn gnome_wayland_detection_accepts_ubuntu_and_setup_display() {
        assert!(is_gnome_wayland_session_from_env(
            Some("wayland-0"),
            Some("ubuntu:GNOME"),
            None
        ));
        assert!(is_gnome_wayland_session_from_env(
            Some("wayland-0"),
            None,
            Some(":1")
        ));
        assert!(!is_gnome_wayland_session_from_env(
            Some("wayland-0"),
            Some("KDE"),
            None
        ));
        assert!(!is_gnome_wayland_session_from_env(
            None,
            Some("GNOME"),
            Some(":1")
        ));
    }

    #[test]
    fn gtk_layer_shell_selector_is_not_used_on_gnome_wayland() {
        assert!(!should_use_gtk_layer_shell_selector_from_env(
            Some("wayland-0"),
            Some("ubuntu:GNOME"),
            None,
            None,
            None,
            false,
        ));
        assert!(!should_use_gtk_layer_shell_selector_from_env(
            Some("wayland-0"),
            None,
            Some(":1"),
            None,
            None,
            false,
        ));
    }

    #[test]
    fn gtk_layer_shell_selector_is_reserved_for_arch_wlroots_sessions() {
        assert!(should_use_gtk_layer_shell_selector_from_env(
            Some("wayland-0"),
            Some("Hyprland"),
            None,
            Some("hyprland-instance"),
            None,
            true,
        ));
        assert!(!should_use_gtk_layer_shell_selector_from_env(
            Some("wayland-0"),
            Some("Hyprland"),
            None,
            Some("hyprland-instance"),
            None,
            false,
        ));
        assert!(!should_use_gtk_layer_shell_selector_from_env(
            Some("wayland-0"),
            Some("arch:GNOME"),
            None,
            None,
            None,
            true,
        ));
        assert!(!should_use_gtk_layer_shell_selector_from_env(
            Some("wayland-0"),
            Some("KDE"),
            None,
            None,
            None,
            true,
        ));
        assert!(!should_use_gtk_layer_shell_selector_from_env(
            Some("wayland-0"),
            Some("COSMIC"),
            None,
            None,
            None,
            true,
        ));
    }

    #[test]
    fn test_parse_normal() {
        let result = parse_selection_json(r#"{"x":10,"y":20,"width":300,"height":200}"#).unwrap();
        let area = match result {
            OverlaySelection::Area(Some(area)) => area,
            other => panic!("unexpected selection: {other:?}"),
        };
        assert_eq!(area.x, 10);
        assert_eq!(area.y, 20);
        assert_eq!(area.width, 300);
        assert_eq!(area.height, 200);
    }

    #[test]
    fn test_parse_zero_size_is_error() {
        assert!(parse_selection_json(r#"{"x":0,"y":0,"width":0,"height":0}"#).is_err());
    }

    #[test]
    fn test_parse_capture_screen_path() {
        let path =
            parse_capture_screen_json(r#"{"path":"/tmp/demo.png","width":1920,"height":1080}"#)
                .unwrap();
        assert_eq!(path.to_string_lossy(), "/tmp/demo.png");
    }

    #[test]
    fn test_parse_capture_screen_path_with_mode() {
        let (path, mode) = parse_capture_screen_json_with_mode(
            r#"{"path":"/tmp/demo.png","width":1920,"height":1080,"mode":"ocr"}"#,
        )
        .unwrap();
        assert_eq!(path.to_string_lossy(), "/tmp/demo.png");
        assert_eq!(mode.as_deref(), Some("ocr"));
    }

    #[test]
    fn test_parse_selection_coords() {
        let parsed = parse_selection_json(r#"{"x":1,"y":2,"width":3,"height":4}"#).unwrap();
        let area = match parsed {
            OverlaySelection::Area(Some(area)) => area,
            other => panic!("unexpected selection: {other:?}"),
        };
        assert_eq!(area.x, 1);
        assert_eq!(area.y, 2);
        assert_eq!(area.width, 3);
        assert_eq!(area.height, 4);
    }

    #[test]
    fn parse_recording_json_reads_runtime_overlay_fields() {
        let request = parse_recording_json(
            r#"{
                "x":12,"y":34,"width":567,"height":890,
                "mode":"record","record_type":"video",
                "controls":true,"mic":true,"speaker":false,
                "display_rec_time":true,"hidpi":false,
                "notifications":true,"cursor":false,
                "remember_selection":true,"dim_screen":false,
                "countdown":true,
                "video_format":1,"video_max_res":2,"video_fps":1,
                "record_mono":true,"open_editor":false,
                "gif_fps":33,"gif_quality":0.8125,
                "gif_size_idx":2,"optimize_gif":false,"fullscreen":true
            }"#,
        )
        .unwrap();

        assert_eq!(request.x, 12);
        assert_eq!(request.y, 34);
        assert_eq!(request.width, 567);
        assert_eq!(request.height, 890);
        assert_eq!(request.record_type, RecordingType::Video);
        assert!(request.controls);
        assert!(request.mic);
        assert!(!request.speaker);
        assert!(request.display_rec_time);
        assert!(!request.hidpi);
        assert!(request.notifications);
        assert!(!request.cursor);
        assert!(request.remember_selection);
        assert!(!request.dim_screen);
        assert!(request.countdown);
        assert_eq!(request.video_format, 0);
        assert_eq!(request.video_max_res, 2);
        assert_eq!(request.video_fps, 1);
        assert!(request.record_mono);
        assert!(!request.open_editor);
        assert_eq!(request.gif_fps, 33);
        assert_eq!(request.gif_quality, 0.8125);
        assert_eq!(request.gif_size_idx, 2);
        assert!(!request.optimize_gif);
        assert!(request.fullscreen);
    }

    #[test]
    fn build_area_init_args_includes_runtime_overlay_defaults() {
        let config = AppConfig {
            rec_video_format: 1,
            ..AppConfig::default()
        };

        let args = build_area_init_args(&config);

        assert!(args.contains(&"--video-format=0".to_string()));
    }

    #[test]
    fn forwarded_overlay_exit_code_is_classified_distinctly() {
        assert_eq!(
            classify_overlay_exit_code(Some(OverlayExitCode::ForwardedToExistingOverlay as i32)),
            Ok(Some("forwarded"))
        );
    }

    #[test]
    fn builtin_block_overlay_exit_code_is_classified_distinctly() {
        assert_eq!(
            classify_overlay_exit_code(Some(OverlayExitCode::BlockedByBuiltinOverlay as i32)),
            Err(LaunchBlockedReason::BuiltinOverlayActive)
        );
    }

    #[test]
    fn capture_session_coordinator_blocks_duplicate_apex_sessions() {
        let coordinator = CaptureSessionCoordinator::default();
        let _guard = coordinator
            .begin_apex_overlay_session(false)
            .expect("first session should acquire the guard");

        assert!(matches!(
            coordinator.begin_apex_overlay_session(false),
            Err(LaunchBlockedReason::ApexOverlayAlreadyActive)
        ));
    }

    #[test]
    fn capture_session_coordinator_blocks_builtin_overlay_without_latching() {
        let coordinator = CaptureSessionCoordinator::default();

        assert!(matches!(
            coordinator.begin_apex_overlay_session(true),
            Err(LaunchBlockedReason::BuiltinOverlayActive)
        ));

        assert!(
            coordinator.begin_apex_overlay_session(false).is_ok(),
            "builtin detection should not permanently wedge the coordinator"
        );
    }

    #[test]
    fn builtin_overlay_query_can_run_inside_tokio_runtime_without_panicking() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");

        let result = runtime.block_on(async {
            std::panic::catch_unwind(|| execute_builtin_overlay_query(|| true))
        });

        assert!(result.expect("query should not panic"));
    }

    #[test]
    fn screenshot_lock_only_wraps_interactive_overlay_launches() {
        assert!(should_request_screenshot_lock(&[]));
        assert!(should_request_screenshot_lock(&["--area-init"]));
        assert!(should_request_screenshot_lock(&["--window-capture"]));
        assert!(should_request_screenshot_lock(&["--crosshair-capture"]));
        assert!(!should_request_screenshot_lock(&["--capture-screen"]));
    }

    #[test]
    fn tracked_overlay_id_matches_preview_helper_contract() {
        assert_eq!(
            tracked_overlay_id("session-123"),
            "capture-overlay-session-123"
        );
    }

    #[test]
    fn build_area_init_args_includes_screenshot_selection_settings() {
        let config = AppConfig {
            screenshot_freeze_screen: false,
            screenshot_crosshair_mode: "Crosshair".into(),
            screenshot_show_magnifier: true,
            ..AppConfig::default()
        };

        let args = build_area_init_args(&config);

        assert!(args.iter().any(|arg| arg == "--area-init"));
        assert!(args.iter().any(|arg| arg == "--selection-cursor=Crosshair"));
        assert!(args.iter().any(|arg| arg == "--show-zoom-preview=1"));
        assert!(args.iter().any(|arg| arg == "--freeze-selection-bg=0"));
    }

    #[test]
    fn build_recording_ui_args_adds_direct_recording_flag() {
        let args = build_recording_ui_args(&crate::config::AppConfig::default());
        assert!(args.iter().any(|arg| arg == "--area-init"));
        assert!(args.iter().any(|arg| arg == "--open-recording-ui"));
    }

    #[test]
    fn area_init_cancel_does_not_parse_record_config_payload() {
        let result = parse_area_capture_output_with_stderr(
            Some(OverlayExitCode::Cancelled as i32),
            r#"{"x":636,"y":177,"width":600,"height":744,"mode":"record-config","record_type":"video"}"#,
            "",
        )
        .expect("cancel should parse");

        assert!(matches!(result, super::AreaCapturePathResult::Cancelled));
    }

    #[test]
    fn explicit_record_config_exit_is_distinct_from_cancel() {
        let result = parse_area_capture_output_with_persist(
            Some(OverlayExitCode::RecordConfigUpdated as i32),
            r#"{"x":636,"y":177,"width":600,"height":744,"mode":"record-config","record_type":"video","controls":true,"mic":false,"speaker":false,"display_rec_time":false,"hidpi":false,"notifications":true,"cursor":true,"remember_selection":false,"dim_screen":true,"countdown":true,"video_max_res":0,"video_fps":1,"record_mono":false,"open_editor":false,"gif_fps":60,"gif_quality":0.7500,"gif_size_idx":0,"optimize_gif":true,"fullscreen":false}"#,
            "",
            |_| Ok(()),
        )
        .expect("record config should parse");

        assert!(matches!(
            result,
            super::AreaCapturePathResult::RecordingConfigUpdated
        ));
    }

    #[test]
    fn area_init_error_includes_portal_stderr_detail() {
        let err = parse_area_capture_output_with_stderr(
            Some(2),
            "",
            "apexshot-capture: area capture failed: Portal permission capture failed (Portal screenshot rejected: status=2); overlay-local fallback failed (Portal fullscreen capture failed (Portal screenshot rejected: status=2))\n",
        )
        .expect_err("exit 2 should be an error");

        let msg = err.to_string();
        assert!(msg.contains("status=2"), "msg={msg}");
        assert!(msg.contains("Portal"), "msg={msg}");
    }

    #[test]
    fn user_facing_message_strips_exit_prefix_and_keeps_portal_detail() {
        let technical = "apexshot-capture --area-init exited with code 2: area capture failed: Portal permission capture failed (Portal screenshot rejected: status=2)";
        let body = super::user_facing_capture_failure_message(technical);
        assert!(
            body.contains("Portal screenshot rejected: status=2"),
            "body={body}"
        );
        assert!(!body.contains("exited with code"), "body={body}");
    }

    #[test]
    fn extract_capture_error_prefers_capture_failed_line() {
        let stderr = "qt.something: noise\napexshot-capture: area capture failed: Portal screenshot rejected: status=2\n";
        let detail = super::extract_capture_error_detail(stderr).expect("detail");
        assert!(detail.contains("Portal screenshot rejected: status=2"));
        assert!(!detail.starts_with("apexshot-capture:"));
    }

    #[test]
    fn save_capture_to_temp_png_round_trips_rgba_capture() {
        let capture = CaptureData::new(
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
            ],
            2,
            2,
            PixelFormat::RGBA32,
        );

        let path = save_capture_to_temp_png(&capture).expect("temp png should save");
        let loaded = image::open(&path)
            .expect("temp png should load")
            .into_rgba8();
        let _ = std::fs::remove_file(&path);

        assert_eq!(loaded.width(), 2);
        assert_eq!(loaded.height(), 2);
    }
}
