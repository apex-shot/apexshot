#include "platform/platform.h"
#include "platform/windows/quick_access.h"

#include <QApplication>

#include <cstring>
#include <windows.h>

namespace {

// The screenshot scripts start this binary with a --demo- flag to capture a
// window, sometimes while the real shell is already in the tray.
bool requestsDemo(int argc, char** argv)
{
    for (int index = 1; index < argc; ++index) {
        if (std::strncmp(argv[index], "--demo-", 7) == 0)
            return true;
    }
    return false;
}

} // namespace

int main(int argc, char** argv)
{
    // One shell per session. A second copy would add a duplicate tray icon and
    // fail to register the hotkeys the first one already owns, so the layout
    // shortcut and the menu it opens would stop working.
    HANDLE instance = nullptr;
    if (!requestsDemo(argc, argv)) {
        instance = CreateMutexW(nullptr, TRUE, L"Local\\ApexShotShell");
        if (instance && GetLastError() == ERROR_ALREADY_EXISTS) {
            CloseHandle(instance);
            return 0;
        }
    }

    // Per-monitor DPI awareness has to be set before Qt creates any window, so
    // the shell, the frozen frame, and the recorder share device pixels.
    Platform::enableDpiAwareness();

    QApplication app(argc, argv);
    QApplication::setApplicationName(QStringLiteral("ApexShot"));
    QApplication::setApplicationDisplayName(QStringLiteral("ApexShot"));
    QApplication::setOrganizationName(QStringLiteral("ApexShot"));
    QApplication::setApplicationVersion(QStringLiteral(APEXSHOT_VERSION));

    const int status = QuickAccess::run(app);
    if (instance)
        CloseHandle(instance);
    return status;
}
