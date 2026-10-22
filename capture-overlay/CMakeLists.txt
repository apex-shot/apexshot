cmake_minimum_required(VERSION 3.16)
project(apexshot-capture LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

if(WIN32)
    # Windows builds the Quick Access shell, not the Linux overlay. The shell
    # reuses the portable toolbar and icon painter, plus the platform seam.
    enable_language(C)

    find_package(Qt5 REQUIRED COMPONENTS Widgets DBus Network)

    add_executable(apexshot-capture WIN32
        src/platform/windows/main.cpp
        src/platform/windows/quick_access.cpp
        src/platform/windows/quick_access.h
        src/platform/recording.h
        src/platform/windows/recording.cpp
        src/platform/windows/recording_controls.cpp
        src/platform/windows/recording_controls.h
        src/platform/windows/recording_countdown.cpp
        src/platform/windows/recording_countdown.h
        src/platform/windows/screen_capture.cpp
        src/platform/windows/window_list.cpp
        src/platform/windows/ocr.cpp
        src/platform/windows/ocr.h
        src/platform/windows/qr.cpp
        src/platform/windows/qr.h
        src/platform/windows/win_util.h
        src/platform/windows/third_party/quirc/decode.c
        src/platform/windows/third_party/quirc/identify.c
        src/platform/windows/third_party/quirc/quirc.c
        src/platform/windows/third_party/quirc/version_db.c
        src/CaptureModeToolbar.cpp
        src/CaptureModeToolbar.h
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
        APEXSHOT_VERSION="${APEXSHOT_VERSION}"
    )

    target_link_libraries(apexshot-capture
        PRIVATE Qt5::Widgets Qt5::DBus Qt5::Network
            windowsapp runtimeobject dwmapi
    )

    if(MSVC)
        target_compile_options(apexshot-capture PRIVATE /utf-8)
    endif()

    option(APEXSHOT_CAPTURE_BUILD_TESTS "Build the capture helper unit tests" OFF)
    if(APEXSHOT_CAPTURE_BUILD_TESTS)
        enable_testing()
        add_executable(apexshot-recording-tests
            tests/WindowsRecordingTests.cpp
            src/platform/recording.h
            src/platform/windows/recording.cpp
        )
        target_include_directories(apexshot-recording-tests PRIVATE src)
        target_link_libraries(apexshot-recording-tests PRIVATE Qt5::Core Qt5::Gui)
        add_test(NAME apexshot-recording-tests COMMAND apexshot-recording-tests)
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
