cmake_minimum_required(VERSION 3.16)
project(apexshot-capture LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

find_package(Qt5 REQUIRED COMPONENTS Widgets DBus X11Extras Network)
find_package(X11 REQUIRED COMPONENTS Xtst)

add_executable(apexshot-capture
    src/main.cpp
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
