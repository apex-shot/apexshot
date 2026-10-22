#include "platform/platform.h"
#include "platform/windows/quick_access.h"

#include <QApplication>

int main(int argc, char** argv)
{
    // Per-monitor DPI awareness has to be set before Qt creates any window, so
    // the shell, the frozen frame, and the recorder share device pixels.
    Platform::enableDpiAwareness();

    QApplication app(argc, argv);
    QApplication::setApplicationName(QStringLiteral("ApexShot"));
    QApplication::setApplicationDisplayName(QStringLiteral("ApexShot"));
    QApplication::setOrganizationName(QStringLiteral("ApexShot"));
    QApplication::setApplicationVersion(QStringLiteral(APEXSHOT_VERSION));

    return QuickAccess::run(app);
}
