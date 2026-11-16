// Regression: a cancel that reaches the selector while it is still settling —
// after the control socket exists but before the event loop runs — must end the
// selector instead of being dropped, which used to leave it up until a second
// cancel or a click completed a capture.
//
// The startup window is made deterministic with a fake portal helper rather
// than a tight poll: FLATPAK_ID forces the portal prepare path offscreen, the
// fake helper signals that it has started (the selector blocks until it
// answers) and then waits for a barrier, and the test queues the cancel on the
// real control socket before releasing that barrier. The selector therefore
// cannot reach app.exec() before the cancel is queued, no matter how the parent
// is scheduled.

#include <QElapsedTimer>
#include <QFile>
#include <QGuiApplication>
#include <QLocalSocket>
#include <QProcess>
#include <QTemporaryDir>
#include <QtTest/QtTest>

namespace {

constexpr int kStartBudgetMs = 10000;
constexpr int kPrepareBudgetMs = 10000;
constexpr int kSocketBudgetMs = 10000;
constexpr int kConnectBudgetMs = 2000;
constexpr int kExitBudgetMs = 8000;
constexpr int kCancelDelayMs = 300;
constexpr int kFreshProcessRuns = 3;

// The selector starts this helper and blocks waiting for its ready line, so
// touching the marker proves the selector is inside its prepare phase. The
// helper then waits for the test's barrier before answering. The barrier wait
// is bounded so a failed test cannot leave the helper behind.
constexpr char kFakePortalHelper[] = R"SH(#!/bin/sh
# Fake portal helper: signal that the selector is waiting for the ready reply,
# wait for the test to release the barrier, then answer ready without stream
# metadata so the monitor match accepts it.
set -e
: > "$FAKE_PORTAL_PREPARED"
waited=0
while [ ! -f "$FAKE_PORTAL_GO" ]; do
    waited=$((waited + 1))
    if [ "$waited" -gt 3000 ]; then
        exit 5
    fi
    sleep 0.01
done
printf '%s\n' '{"ready":true}'
# The selector writes "capture" for a fresh frame; this test disables the
# freeze, so just drain until it closes the channel.
while read -r request; do
    if [ "$request" = "capture" ]; then
        printf '%s\n' '{"error":"no synthetic frame in this test"}'
    fi
done
exit 0
)SH";

QString overlaySocketPath(const QString& runtimeDir)
{
    // FLATPAK_ID makes the selector keep its IPC under the sandbox directory.
    return runtimeDir + QStringLiteral("/app/org.apexshot.ApexShot")
           + QStringLiteral("/apexshot-capture-overlay.sock");
}

bool writeFile(const QString& path, const QByteArray& contents)
{
    QFile file(path);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Truncate))
        return false;
    if (file.write(contents) != contents.size())
        return false;
    file.close();
    return true;
}

bool waitForPath(const QString& path, int budgetMs)
{
    QElapsedTimer timer;
    timer.start();
    while (!QFile::exists(path)) {
        if (timer.elapsed() > budgetMs)
            return false;
        QTest::qWait(10);
    }
    return true;
}

/// A failed assertion must not leave the selector running on the test machine.
/// Releasing the fake helper's barrier first lets it answer and exit as soon as
/// the channel closes, instead of waiting out its own bounded loop.
class RunningProcessGuard
{
public:
    RunningProcessGuard(QProcess& process, const QString& barrierPath)
        : m_process(process)
        , m_barrierPath(barrierPath)
    {
    }
    ~RunningProcessGuard()
    {
        if (m_process.state() != QProcess::NotRunning) {
            writeFile(m_barrierPath, "go");
            m_process.kill();
            m_process.waitForFinished(2000);
        }
    }

private:
    QProcess& m_process;
    QString m_barrierPath;
};

} // namespace

class CancelDuringStartupTests : public QObject
{
    Q_OBJECT

private Q_SLOTS:
    void early_cancel_exits_without_entering_the_event_loop();
    void early_cancel_repeats_on_fresh_processes();

private:
    void run_one_early_cancel();
};

void CancelDuringStartupTests::run_one_early_cancel()
{
    const QString binary = qEnvironmentVariable("APEXSHOT_CAPTURE_UNDER_TEST");
    if (binary.isEmpty())
        QSKIP("APEXSHOT_CAPTURE_UNDER_TEST is not set");

    QTemporaryDir runtimeDir;
    QVERIFY(runtimeDir.isValid());
    const QString root = runtimeDir.path();
    QVERIFY(QFile::setPermissions(root, QFile::ReadOwner | QFile::WriteOwner | QFile::ExeOwner));

    const QString preparedPath = root + QStringLiteral("/portal-prepared");
    const QString goPath = root + QStringLiteral("/portal-go");
    const QString fakeHelperPath = root + QStringLiteral("/fake-portal-helper");
    QVERIFY(writeFile(fakeHelperPath, kFakePortalHelper));
    // The shell umask is 0077, so the executable bit has to be set explicitly.
    QVERIFY(QFile::setPermissions(fakeHelperPath,
                                  QFile::ReadOwner | QFile::WriteOwner | QFile::ExeOwner));

    QProcess process;
    RunningProcessGuard guard(process, goPath);
    QProcessEnvironment env = QProcessEnvironment::systemEnvironment();
    env.remove(QStringLiteral("WAYLAND_DISPLAY"));
    env.remove(QStringLiteral("DISPLAY"));
    env.remove(QStringLiteral("XDG_SESSION_TYPE"));
    env.remove(QStringLiteral("DBUS_SESSION_BUS_ADDRESS"));
    env.insert(QStringLiteral("QT_QPA_PLATFORM"), QStringLiteral("offscreen"));
    env.insert(QStringLiteral("XDG_RUNTIME_DIR"), root);
    // Forces the portal prepare path, which is where the selector waits.
    env.insert(QStringLiteral("FLATPAK_ID"), QStringLiteral("org.apexshot.ApexShot"));
    env.insert(QStringLiteral("APEXSHOT_PORTAL_HELPER"), fakeHelperPath);
    env.insert(QStringLiteral("FAKE_PORTAL_PREPARED"), preparedPath);
    env.insert(QStringLiteral("FAKE_PORTAL_GO"), goPath);
    process.setProcessEnvironment(env);
    process.setProcessChannelMode(QProcess::SeparateChannels);

    process.start(binary,
                  {QStringLiteral("-platform"),
                   QStringLiteral("offscreen"),
                   QStringLiteral("--crosshair-capture"),
                   QStringLiteral("--hide-timer"),
                   QStringLiteral("--freeze-selection-bg=0"),
                   QStringLiteral("--screenshot-cursor=0")});
    QVERIFY2(process.waitForStarted(kStartBudgetMs), "selector did not start");
    QVERIFY2(waitForPath(preparedPath, kPrepareBudgetMs),
             "selector never reached its portal prepare phase");

    const QString socketPath = overlaySocketPath(root);
    QVERIFY2(waitForPath(socketPath, kSocketBudgetMs),
             "selector never bound its control socket");

    // Deliberate delay before the cancel: the selector is blocked in prepare,
    // so this proves the outcome does not depend on startup timing.
    QTest::qWait(kCancelDelayMs);
    QVERIFY2(!QFile::exists(goPath), "portal helper was released too early");

    QLocalSocket socket;
    socket.connectToServer(socketPath);
    QVERIFY2(socket.waitForConnected(kConnectBudgetMs),
             "control socket refused the connection");
    QCOMPARE(socket.write("cancel\n"), qint64(7));
    QVERIFY(socket.flush());
    // An already-flushed payload reports nothing pending, which is success.
    QVERIFY(socket.waitForBytesWritten(1000) || socket.bytesToWrite() == 0);
    socket.disconnectFromServer();

    // Only now may startup continue; the cancel is already queued.
    QVERIFY(writeFile(goPath, "go"));

    if (!process.waitForFinished(kExitBudgetMs)) {
        process.kill();
        process.waitForFinished(2000);
        QFAIL(qPrintable(
            QStringLiteral("selector ignored the queued cancel (stderr: %1)")
                .arg(QString::fromLocal8Bit(process.readAllStandardError()))));
    }

    const QString stderrText = QString::fromLocal8Bit(process.readAllStandardError());
    QCOMPARE(process.exitStatus(), QProcess::NormalExit);
    QCOMPARE(process.exitCode(), 1);
    QVERIFY2(stderrText.contains(QStringLiteral("exit requested during startup")),
             qPrintable(stderrText));
    QVERIFY2(!QFile::exists(socketPath), "selector left its control socket behind");
}

void CancelDuringStartupTests::early_cancel_exits_without_entering_the_event_loop()
{
    run_one_early_cancel();
}

void CancelDuringStartupTests::early_cancel_repeats_on_fresh_processes()
{
    // Each run starts a fresh selector and a fresh runtime directory. Warm
    // worker recovery (repeated jobs in one helper process) is covered by the
    // separate worker lifecycle check, not by this test.
    for (int run = 0; run < kFreshProcessRuns; ++run)
        run_one_early_cancel();
}

int main(int argc, char* argv[])
{
    qputenv("QT_QPA_PLATFORM", "offscreen");
    QGuiApplication app(argc, argv);
    CancelDuringStartupTests tests;
    return QTest::qExec(&tests, argc, argv);
}

#include "CancelDuringStartupTests.moc"
