cmake_minimum_required(VERSION 3.16)
project(apexshot-capture LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

find_package(Qt5 REQUIRED COMPONENTS Widgets DBus X11Extras)
find_package(X11 REQUIRED COMPONENTS Xtst)

add_executable(apexshot-capture
    src/main.cpp
    src/ScreenCapture.cpp
    src/ScreenCapture.h
    src/request.cpp
    src/request.h
    src/CaptureOverlay.cpp
    src/CaptureOverlay.h
    src/WindowPickerOverlay.cpp
    src/WindowPickerOverlay.h
)

target_link_libraries(apexshot-capture
    PRIVATE Qt5::Widgets Qt5::DBus Qt5::X11Extras ${X11_LIBRARIES} ${X11_Xtst_LIB}
)

install(TARGETS apexshot-capture
    RUNTIME DESTINATION bin
)
