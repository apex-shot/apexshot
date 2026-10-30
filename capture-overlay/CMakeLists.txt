cmake_minimum_required(VERSION 3.16)
project(apexshot-capture LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)
set(CMAKE_AUTORCC ON)

if(WIN32)
    # Windows builds the Quick Access shell, not the Linux overlay. The shell
    # reuses the portable toolbar and icon painter, plus the platform seam.
    enable_language(C)

    find_package(Qt5 REQUIRED COMPONENTS Widgets DBus Network Concurrent)

    add_executable(apexshot-capture WIN32
        src/platform/windows/main.cpp
        src/platform/windows/quick_access.cpp
        src/platform/windows/quick_access.h
        src/platform/recording.h
        src/platform/recording_settings.h
        src/platform/windows/recording.cpp
        src/platform/windows/pointer_tracker.cpp
        src/platform/windows/pointer_tracker.h
        src/platform/windows/recording_settings.cpp
        src/platform/windows/settings_store.cpp
        src/platform/windows/settings_store.h
        src/platform/windows/hotkeys.cpp
        src/platform/windows/hotkeys.h
        src/platform/windows/hotkey_parser.cpp
        src/platform/windows/audio_devices.cpp
        src/platform/windows/audio_devices.h
        src/platform/windows/capture_sound.cpp
        src/platform/windows/capture_sound.h
        src/platform/windows/recording_controls.cpp
        src/platform/windows/recording_controls.h
        src/platform/windows/recording_countdown.cpp
        src/platform/windows/recording_countdown.h
        src/platform/windows/recording_mask.cpp
        src/platform/windows/recording_mask.h
        src/platform/windows/scroll_capture.cpp
        src/platform/windows/scroll_capture.h
        src/platform/windows/scroll_controls.cpp
        src/platform/windows/scroll_controls.h
        src/platform/windows/webcam_bubble.cpp
        src/platform/windows/webcam_bubble.h
        src/platform/windows/webcam_format.cpp
        src/platform/windows/webcam_format.h
        src/platform/windows/webcam_preview.cpp
        src/platform/windows/webcam_preview.h
        src/platform/windows/window_recording.cpp
        src/platform/windows/window_recording.h
        src/platform/windows/webcam_layout.cpp
        src/platform/windows/webcam_layout.h
        src/platform/windows/screen_capture.cpp
        src/platform/windows/window_list.cpp
        src/platform/windows/ocr.cpp
        src/platform/windows/ocr.h
        src/platform/windows/usage_telemetry.cpp
        src/platform/windows/usage_telemetry.h
        src/platform/windows/qr.cpp
        src/platform/windows/qr.h
        src/platform/windows/win_util.h
        src/platform/windows/third_party/quirc/decode.c
        src/platform/windows/third_party/quirc/identify.c
        src/platform/windows/third_party/quirc/quirc.c
        src/platform/windows/third_party/quirc/version_db.c
        src/CaptureModeToolbar.cpp
        src/CaptureModeToolbar.h
        assets/lucide.qrc
        src/CaptureOverlay_DrawingPrimitives.cpp
        src/CaptureOverlay_DrawingPrimitives_p.h
    )

    target_include_directories(apexshot-capture PRIVATE src)

    # Keep the shell's reported version in step with the Rust package.
    set(APEXSHOT_VERSION "0.0.0")
    if(EXISTS "${CMAKE_CURRENT_SOURCE_DIR}/../Cargo.toml")
        file(STRINGS "${CMAKE_CURRENT_SOURCE_DIR}/../Cargo.toml" APEXSHOT_VERSION_LINE
            REGEX "^version = \"[0-9.]+\"")
        string(REGEX REPLACE "^version = \"([^\"]+)\"" "\\1"
            APEXSHOT_VERSION "${APEXSHOT_VERSION_LINE}")
    endif()

    target_compile_definitions(apexshot-capture PRIVATE
        APEXSHOT_FONT_DIR="${CMAKE_CURRENT_SOURCE_DIR}/assets/fonts"
        APEXSHOT_SOUND_DIR="${CMAKE_CURRENT_SOURCE_DIR}/../assets/sounds"
        APEXSHOT_VERSION="${APEXSHOT_VERSION}"
    )

    target_link_libraries(apexshot-capture
        PRIVATE Qt5::Widgets Qt5::DBus Qt5::Network Qt5::Concurrent
            windowsapp runtimeobject dwmapi uiautomationcore winmm
            mmdevapi ole32 oleaut32 strmiids shell32 uuid winhttp
    )

    if(MSVC)
        target_compile_options(apexshot-capture PRIVATE /utf-8)
    endif()

    option(APEXSHOT_CAPTURE_BUILD_TESTS "Build the capture helper unit tests" OFF)
    if(APEXSHOT_CAPTURE_BUILD_TESTS)
        enable_testing()
        add_executable(apexshot-capture-toolbar-tests
            tests/CaptureModeToolbarTests.cpp
            src/CaptureModeToolbar.cpp
            src/CaptureModeToolbar.h
            src/platform/windows/audio_devices.cpp
            src/platform/windows/audio_devices.h
            assets/lucide.qrc
        )
        target_include_directories(apexshot-capture-toolbar-tests PRIVATE src)
        target_link_libraries(apexshot-capture-toolbar-tests PRIVATE
            Qt5::Widgets Qt5::DBus Qt5::Network mmdevapi ole32 oleaut32 strmiids shell32 uuid)
        if(MSVC)
            target_compile_options(apexshot-capture-toolbar-tests PRIVATE /utf-8)
        endif()
        add_test(NAME apexshot-capture-toolbar-tests COMMAND apexshot-capture-toolbar-tests)
        set_tests_properties(apexshot-capture-toolbar-tests PROPERTIES
            ENVIRONMENT "QT_QPA_PLATFORM=offscreen")

        add_executable(apexshot-fake-webcam tests/FakeWebcam.cpp)
        target_link_libraries(apexshot-fake-webcam PRIVATE Qt5::Core Qt5::Gui)

        add_executable(apexshot-webcam-preview-tests
            tests/WebcamPreviewTests.cpp
            src/platform/recording.h
            src/platform/recording_settings.h
            src/platform/windows/recording.cpp
            src/platform/windows/audio_devices.cpp
            src/platform/windows/pointer_tracker.cpp
            src/platform/windows/recording_settings.cpp
            src/platform/windows/settings_store.cpp
            src/platform/windows/webcam_bubble.cpp
            src/platform/windows/webcam_bubble.h
            src/platform/windows/webcam_format.cpp
            src/platform/windows/webcam_format.h
            src/platform/windows/webcam_layout.cpp
            src/platform/windows/webcam_preview.cpp
            src/platform/windows/webcam_preview.h
        )
        target_include_directories(apexshot-webcam-preview-tests PRIVATE src)
        target_link_libraries(apexshot-webcam-preview-tests PRIVATE
            Qt5::Widgets Qt5::Network mmdevapi ole32 oleaut32 strmiids shell32 uuid)
        add_dependencies(apexshot-webcam-preview-tests apexshot-fake-webcam)
        add_test(NAME apexshot-webcam-preview-tests COMMAND apexshot-webcam-preview-tests)
        set_tests_properties(apexshot-webcam-preview-tests PROPERTIES
            ENVIRONMENT "QT_QPA_PLATFORM=offscreen")

        add_executable(apexshot-recording-tests
            tests/WindowsRecordingTests.cpp
            src/platform/recording.h
            src/platform/recording_settings.h
            src/platform/windows/recording.cpp
            src/platform/windows/audio_devices.cpp
            src/platform/windows/audio_devices.h
            src/platform/windows/pointer_tracker.cpp
            src/platform/windows/pointer_tracker.h
            src/platform/windows/recording_settings.cpp
            src/platform/windows/settings_store.cpp
            src/platform/windows/settings_store.h
            src/platform/windows/webcam_format.cpp
            src/platform/windows/webcam_format.h
            src/platform/windows/webcam_layout.cpp
            src/platform/windows/webcam_layout.h
        )
        target_include_directories(apexshot-recording-tests PRIVATE src)
        target_link_libraries(apexshot-recording-tests PRIVATE Qt5::Core Qt5::Gui
            mmdevapi ole32 oleaut32 strmiids shell32 uuid)
        add_test(NAME apexshot-recording-tests COMMAND apexshot-recording-tests)
        # The smoke test reports 77 when ffmpeg is absent instead of failing.
        set_tests_properties(apexshot-recording-tests PROPERTIES SKIP_RETURN_CODE 77)

        add_executable(apexshot-recording-settings-tests
            tests/RecordingSettingsTests.cpp
            src/platform/recording_settings.h
            src/platform/windows/recording_settings.cpp
        )
        target_include_directories(apexshot-recording-settings-tests PRIVATE src)
        target_link_libraries(apexshot-recording-settings-tests PRIVATE Qt5::Core)
        add_test(NAME apexshot-recording-settings-tests
            COMMAND apexshot-recording-settings-tests)

        add_executable(apexshot-settings-store-tests
            tests/SettingsStoreTests.cpp
            src/platform/recording_settings.h
            src/platform/windows/recording_settings.cpp
            src/platform/windows/settings_store.cpp
            src/platform/windows/settings_store.h
        )
        target_include_directories(apexshot-settings-store-tests PRIVATE src)
        target_link_libraries(apexshot-settings-store-tests PRIVATE Qt5::Core Qt5::Gui)
        add_test(NAME apexshot-settings-store-tests
            COMMAND apexshot-settings-store-tests)

        add_executable(apexshot-windows-screenshot-tests
            tests/WindowsScreenshotTests.cpp
            src/platform/windows/screen_capture.cpp
        )
        target_include_directories(apexshot-windows-screenshot-tests PRIVATE src)
        target_link_libraries(apexshot-windows-screenshot-tests PRIVATE Qt5::Core Qt5::Gui dwmapi)
        add_test(NAME apexshot-windows-screenshot-tests COMMAND apexshot-windows-screenshot-tests)

        add_executable(apexshot-windows-usage-telemetry-tests
            tests/WindowsUsageTelemetryTests.cpp
            src/platform/windows/usage_telemetry.cpp
            src/platform/windows/settings_store.cpp
            src/platform/windows/recording_settings.cpp
        )
        target_include_directories(apexshot-windows-usage-telemetry-tests PRIVATE src)
        target_compile_definitions(apexshot-windows-usage-telemetry-tests PRIVATE
            APEXSHOT_VERSION="${APEXSHOT_VERSION}")
        target_link_libraries(apexshot-windows-usage-telemetry-tests PRIVATE
            Qt5::Core Qt5::Gui Qt5::Concurrent winhttp)
        add_test(NAME apexshot-windows-usage-telemetry-tests
            COMMAND apexshot-windows-usage-telemetry-tests)

        add_executable(apexshot-windows-ocr-tests
            tests/WindowsOcrTests.cpp
            src/platform/windows/ocr.cpp
        )
        target_include_directories(apexshot-windows-ocr-tests PRIVATE src)
        target_link_libraries(apexshot-windows-ocr-tests PRIVATE
            Qt5::Core Qt5::Gui windowsapp runtimeobject)
        add_test(NAME apexshot-windows-ocr-tests COMMAND apexshot-windows-ocr-tests)
        set_tests_properties(apexshot-windows-ocr-tests PROPERTIES ENVIRONMENT "QT_QPA_PLATFORM=offscreen")

        add_executable(apexshot-hotkey-tests
            tests/WindowsHotkeyTests.cpp
            src/platform/windows/hotkeys.h
            src/platform/windows/hotkey_parser.cpp
        )
        target_include_directories(apexshot-hotkey-tests PRIVATE src)
        target_link_libraries(apexshot-hotkey-tests PRIVATE Qt5::Core Qt5::Gui)
        add_test(NAME apexshot-hotkey-tests COMMAND apexshot-hotkey-tests)

        add_executable(apexshot-hotkey-runtime-tests
            tests/WindowsHotkeyRuntimeTests.cpp
            src/platform/windows/hotkeys.cpp
            src/platform/windows/hotkey_parser.cpp
            src/platform/windows/settings_store.cpp
            src/platform/windows/recording_settings.cpp
        )
        target_include_directories(apexshot-hotkey-runtime-tests PRIVATE src)
        target_link_libraries(apexshot-hotkey-runtime-tests PRIVATE Qt5::Core Qt5::Gui)
        add_test(NAME apexshot-hotkey-runtime-tests COMMAND apexshot-hotkey-runtime-tests)
    endif()

    install(TARGETS apexshot-capture
        RUNTIME DESTINATION bin
    )
else()
    find_package(Qt5 REQUIRED COMPONENTS Widgets DBus X11Extras Network)
    find_package(X11 REQUIRED COMPONENTS Xtst)

    add_executable(apexshot-capture
        src/main.cpp
        src/CaptureMenuResult.cpp
        src/CaptureMenuResult.h
        src/ScreenCapture.cpp
        src/ScreenCapture.h
        src/Sandbox.h
        src/MonitorPicker.cpp
        src/MonitorPicker.h
        src/CaptureModeToolbar.cpp
        src/CaptureModeToolbar.h
        assets/lucide.qrc
        src/CaptureCountdownPill.cpp
        src/CaptureCountdownPill.h
        src/CaptureOverlay.cpp
        src/CaptureOverlay.h
        src/CaptureOverlay_p.h
        src/CaptureOverlay_DrawingPrimitives_p.h
        src/CaptureOverlay_DrawingPrimitives.cpp
        src/CaptureOverlay_Layout.cpp
        src/CaptureOverlay_Drawing.cpp
        src/CaptureOverlay_Paint.cpp
        src/CaptureOverlay_Audio.cpp
        src/CaptureOverlay_ScrollPopupDrawing.cpp
        src/CaptureOverlay_DropdownDrawing.cpp
        src/CaptureOverlay_RecordingSettingsDrawing.cpp
        src/CaptureOverlay_RecordingDrawing.cpp
        src/CaptureOverlay_ToolbarDrawing.cpp
        src/CaptureOverlay_Events.cpp
        src/CaptureOverlay_Scroll.cpp
        src/CaptureOverlay_HitTest.cpp
        src/CaptureOverlay_Window.cpp
        src/RecordingControlsWindow.cpp
        src/RecordingControlsWindow.h
        src/ScrollControlPanel.cpp
        src/ScrollControlPanel.h
    )

    target_link_libraries(apexshot-capture
        PRIVATE Qt5::Widgets Qt5::DBus Qt5::X11Extras Qt5::Network ${X11_LIBRARIES} ${X11_Xtst_LIB}
    )

    option(APEXSHOT_CAPTURE_BUILD_TESTS "Build the capture helper unit tests" OFF)
    if(APEXSHOT_CAPTURE_BUILD_TESTS)
        find_package(Qt5 REQUIRED COMPONENTS Test)
        enable_testing()
        add_executable(apexshot-capture-tests
            tests/ScreenCaptureTests.cpp
            src/CaptureMenuResult.cpp
            src/CaptureMenuResult.h
            src/ScreenCapture.cpp
            src/ScreenCapture.h
            src/Sandbox.h
        )
        target_include_directories(apexshot-capture-tests PRIVATE src)
        target_link_libraries(apexshot-capture-tests
            PRIVATE Qt5::Gui Qt5::DBus Qt5::Test
        )
        add_test(NAME apexshot-capture-tests COMMAND apexshot-capture-tests)
    endif()

    install(TARGETS apexshot-capture
        RUNTIME DESTINATION bin
    )
endif()
