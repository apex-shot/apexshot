// Captures a small Win32 window through the same Graphics Capture source the
// recorder uses, then reads frames from its named pipe.

#include "platform/windows/window_capture.h"

#include <QCoreApplication>
#include <QFile>
#include <QString>

#include <windows.h>

#include <cstdio>
#include <vector>

namespace {

constexpr int kWidth = 320;
constexpr int kHeight = 240;
constexpr int kFrameBytes = kWidth * kHeight * 4;

LRESULT CALLBACK windowProc(HWND window, UINT message, WPARAM wParam, LPARAM lParam)
{
    return DefWindowProcW(window, message, wParam, lParam);
}

HWND createTestWindow()
{
    const HINSTANCE instance = GetModuleHandleW(nullptr);
    const wchar_t* className = L"ApexShotWindowCaptureTest";
    WNDCLASSW windowClass{};
    windowClass.lpfnWndProc = windowProc;
    windowClass.hInstance = instance;
    windowClass.lpszClassName = className;
    windowClass.hbrBackground = CreateSolidBrush(RGB(0, 128, 255));
    if (!RegisterClassW(&windowClass) && GetLastError() != ERROR_CLASS_ALREADY_EXISTS)
        return nullptr;

    HWND window = CreateWindowExW(0,
                                  className,
                                  L"Window capture test",
                                  WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                                  80,
                                  80,
                                  480,
                                  360,
                                  nullptr,
                                  nullptr,
                                  instance,
                                  nullptr);
    if (!window)
        return nullptr;
    ShowWindow(window, SW_SHOW);
    UpdateWindow(window);
    return window;
}

bool readExact(HANDLE pipe, uint8_t* data, DWORD size)
{
    DWORD offset = 0;
    while (offset < size) {
        DWORD read = 0;
        if (!ReadFile(pipe, data + offset, size - offset, &read, nullptr) || read == 0)
            return false;
        offset += read;
    }
    return true;
}

bool waitForData(HANDLE pipe, DWORD bytes, int timeoutMs)
{
    const ULONGLONG deadline = GetTickCount64() + timeoutMs;
    while (GetTickCount64() < deadline) {
        DWORD available = 0;
        if (!PeekNamedPipe(pipe, nullptr, 0, nullptr, &available, nullptr))
            return false;
        if (available >= bytes)
            return true;
        MSG message{};
        while (PeekMessageW(&message, nullptr, 0, 0, PM_REMOVE)) {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        Sleep(20);
    }
    return false;
}

void pumpMessages(int milliseconds)
{
    const ULONGLONG deadline = GetTickCount64() + milliseconds;
    MSG message{};
    while (GetTickCount64() < deadline) {
        while (PeekMessageW(&message, nullptr, 0, 0, PM_REMOVE)) {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        Sleep(5);
    }
}

} // namespace

int main(int argc, char** argv)
{
    QCoreApplication app(argc, argv);

    QString error;
    if (!Platform::WindowCaptureSource::supported(&error)) {
        std::printf("SKIP: %s\n", qPrintable(error));
        return 77;
    }

    HWND window = createTestWindow();
    if (!window) {
        std::printf("FAIL could not create the test window (Win32 error %lu)\n", GetLastError());
        return 1;
    }
    pumpMessages(500);

    Platform::WindowCaptureSource source;
    if (!source.prepare(reinterpret_cast<quintptr>(window), QSize(kWidth, kHeight), 30, &error)) {
        std::printf("FAIL prepare: %s\n", qPrintable(error));
        DestroyWindow(window);
        return 1;
    }

    if (!source.start(&error)) {
        std::printf("FAIL start: %s\n", qPrintable(error));
        DestroyWindow(window);
        return 1;
    }

    const std::wstring pipeName = source.pipePath().toStdWString();
    HANDLE pipe = CreateFileW(pipeName.c_str(),
                              GENERIC_READ,
                              0,
                              nullptr,
                              OPEN_EXISTING,
                              0,
                              nullptr);
    if (pipe == INVALID_HANDLE_VALUE) {
        std::printf("FAIL could not open the capture pipe (Win32 error %lu)\n", GetLastError());
        source.stop();
        DestroyWindow(window);
        return 1;
    }

    bool ok = true;
    int frames = 0;
    std::vector<uint8_t> frame(kFrameBytes);
    for (; frames < 10; ++frames) {
        if (!waitForData(pipe, kFrameBytes, 5000)) {
            std::printf("FAIL no frame %d arrived\n", frames);
            ok = false;
            break;
        }
        if (!readExact(pipe, frame.data(), kFrameBytes)) {
            std::printf("FAIL could not read frame %d\n", frames);
            ok = false;
            break;
        }

        const size_t nonZero = std::count_if(frame.begin(),
                                             frame.end(),
                                             [](uint8_t value) { return value != 0; });
        const uint8_t* center = frame.data() + ((kHeight / 2) * kWidth + kWidth / 2) * 4;
        if (nonZero < frame.size() / 100) {
            std::printf("FAIL frame %d is almost entirely black (%zu non-zero bytes)\n",
                        frames,
                        nonZero);
            ok = false;
            break;
        }
        if (frames == 0) {
            std::printf("frame 0 center BGRA=%u,%u,%u non-zero=%zu/%zu\n",
                        center[0],
                        center[1],
                        center[2],
                        nonZero,
                        frame.size());
        }
    }

    std::printf("WINDOW_CAPTURE_FRAMES=%d\n", frames);
    source.stop();
    CloseHandle(pipe);
    DestroyWindow(window);
    return ok && frames == 10 ? 0 : 1;
}
