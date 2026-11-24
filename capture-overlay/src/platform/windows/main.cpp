#include "platform/platform.h"
#include "platform/recording.h"
#include "platform/windows/quick_access.h"

#include <QApplication>
#include <QFile>
#include <QFileInfo>
#include <QLocalSocket>
#include <QTimer>

#include <cstring>
#include <windows.h>

namespace {

/// The channel a second launch announces itself on.
const char* sessionChannel()
{
    return "apexshot-shell";
}

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

/// Start the shell without its menu, for the settings window that wants the
/// tray and the hotkeys up but not a capture bar over its own UI.
bool requestsBackground(int argc, char** argv)
{
    for (int index = 1; index < argc; ++index) {
        if (std::strcmp(argv[index], "--background") == 0)
            return true;
    }
    return false;
}

/// `--self-test-record <path>`: record two seconds through the real recorder,
/// for the release smoke test. Returns the output path, or empty when absent.
QString selfTestRecordPath(int argc, char** argv)
{
    for (int index = 1; index + 1 < argc; ++index) {
        if (std::strcmp(argv[index], "--self-test-record") == 0)
            return QString::fromLocal8Bit(argv[index + 1]);
    }
    return QString();
}

/// The shell is a GUI binary with no console, so the self-test reports through
/// a text file beside the recording as well as its exit code.
void writeSelfTestReport(const QString& outputPath, const QString& text)
{
    QFile report(outputPath + QStringLiteral(".selftest.txt"));
    if (report.open(QIODevice::WriteOnly | QIODevice::Truncate | QIODevice::Text))
        report.write(text.toUtf8());
}

int runSelfTestRecord(const QString& outputPath)
{
    QString error;
    if (!Platform::RecordingSession::available(&error)) {
        writeSelfTestReport(outputPath, QStringLiteral("recorder unavailable: %1").arg(error));
        return 1;
    }

    Platform::RecordingRequest request;
    request.target = Platform::RecordingRequest::Target::Area;
    request.region = QRect(0, 0, 320, 240);
    request.outputPath = outputPath;
    request.includeCursor = false;
    request.fps = 30;

    Platform::RecordingSession session;
    int status = 1;
    QObject::connect(&session,
                     &Platform::RecordingSession::finished,
                     [&status, outputPath](bool kept, const QString& path, const QString& failure) {
                         const bool ok = kept && failure.isEmpty() && QFileInfo(path).size() > 0;
                         writeSelfTestReport(outputPath,
                                             ok ? QStringLiteral("ok: %1").arg(path)
                                                : QStringLiteral("failed: %1").arg(failure));
                         status = ok ? 0 : 1;
                         QCoreApplication::exit(status);
                     });

    if (!session.start(request, error)) {
        writeSelfTestReport(outputPath, QStringLiteral("could not start: %1").arg(error));
        return 1;
    }
    QTimer::singleShot(2000, &session, [&session] { session.stop(); });
    // A recorder that never finishes fails the test instead of hanging CI.
    QTimer::singleShot(60000, [outputPath] {
        writeSelfTestReport(outputPath, QStringLiteral("failed: timed out"));
        QCoreApplication::exit(1);
    });
    QCoreApplication::exec();
    return status;
}

} // namespace

int main(int argc, char** argv)
{
    // The release smoke test records through the real recorder and exits; it
    // must not take the single-instance lock or start the tray shell.
    const QString selfTestPath = selfTestRecordPath(argc, argv);
    if (!selfTestPath.isEmpty()) {
        Platform::enableDpiAwareness();
        QApplication app(argc, argv);
        QApplication::setApplicationName(QStringLiteral("ApexShot"));
        QApplication::setOrganizationName(QStringLiteral("ApexShot"));
        return runSelfTestRecord(selfTestPath);
    }

    // One shell per session. A second copy would add a duplicate tray icon and
    // fail to register the hotkeys the first one already owns, so the layout
    // shortcut and the menu it opens would stop working.
    HANDLE instance = nullptr;
    bool alreadyRunning = false;
    if (!requestsDemo(argc, argv)) {
        instance = CreateMutexW(nullptr, TRUE, L"Local\\ApexShotShell");
        if (instance && GetLastError() == ERROR_ALREADY_EXISTS) {
            CloseHandle(instance);
            alreadyRunning = true;
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

    if (alreadyRunning) {
        // Silent invisibility was the bug: launching the app again found the
        // shortcut, clicked it, and nothing happened because this copy simply
        // exited. Ask the shell in the tray to open its menu instead - unless
        // this copy was asked to stay in the background, where it exists only
        // to make sure the shell is up.
        if (!requestsBackground(argc, argv)) {
            QLocalSocket socket;
            socket.connectToServer(QString::fromLatin1(sessionChannel()));
            if (socket.waitForConnected(1500)) {
                socket.write("menu");
                socket.flush();
                socket.waitForBytesWritten(1000);
            }
        }
        return 0;
    }

    const int status = QuickAccess::run(app);
    if (instance)
        CloseHandle(instance);
    return status;
}
