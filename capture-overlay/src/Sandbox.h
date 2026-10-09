#pragma once

#include <QDir>
#include <QCoreApplication>
#include <QDBusConnection>
#include <QDBusInterface>
#include <QDBusMessage>
#include <QDBusPendingCallWatcher>
#include <QFileInfo>
#include <QGuiApplication>
#include <QObject>
#include <QString>

inline bool isFlatpakSandboxed()
{
    return !qEnvironmentVariable("FLATPAK_ID").isEmpty()
           || QFileInfo::exists(QStringLiteral("/.flatpak-info"));
}

inline bool isFlatpakWaylandSession()
{
    return isFlatpakSandboxed()
           && QGuiApplication::platformName().contains(QStringLiteral("wayland"),
                                                       Qt::CaseInsensitive);
}

inline QString daemonBusName()
{
    return isFlatpakSandboxed() ? QStringLiteral("org.apexshot.ApexShot.Daemon")
                                : QStringLiteral("org.apexshot.Daemon");
}

inline QString shellOverlayBusName()
{
    return isFlatpakSandboxed() ? QStringLiteral("org.apexshot.ApexShot.ShellOverlay")
                                : QStringLiteral("org.apexshot.ShellOverlay");
}

inline QString windowListBusName()
{
    return isFlatpakSandboxed() ? QStringLiteral("org.apexshot.ApexShot.WindowList")
                                : QStringLiteral("org.apexshot.WindowList");
}

inline void requestShellOverlayFocus(const QString& windowTitle)
{
    QDBusInterface shellOverlay(shellOverlayBusName(),
                                QStringLiteral("/org/apexshot/ShellOverlay"),
                                QStringLiteral("org.apexshot.ShellOverlay"),
                                QDBusConnection::sessionBus());
    if (!shellOverlay.isValid()) {
        return;
    }

    const auto pid = static_cast<qlonglong>(QCoreApplication::applicationPid());
    if (!isFlatpakSandboxed()) {
        shellOverlay.asyncCall(QStringLiteral("FocusCaptureMenu"), pid);
        return;
    }

    auto* watcher = new QDBusPendingCallWatcher(
        shellOverlay.asyncCall(QStringLiteral("FocusCaptureMenuV2"),
                               QStringLiteral("org.apexshot.ApexShot"), windowTitle, pid),
        QCoreApplication::instance());
    QObject::connect(watcher,
                     &QDBusPendingCallWatcher::finished,
                     watcher,
                     [watcher, pid]() {
                         const QDBusMessage reply = watcher->reply();
                         watcher->deleteLater();
                         if (reply.type() != QDBusMessage::ErrorMessage
                             || reply.errorName()
                                  != QStringLiteral("org.freedesktop.DBus.Error.UnknownMethod")) {
                             return;
                         }

                         QDBusInterface legacyShellOverlay(
                             shellOverlayBusName(),
                             QStringLiteral("/org/apexshot/ShellOverlay"),
                             QStringLiteral("org.apexshot.ShellOverlay"),
                             QDBusConnection::sessionBus());
                         if (legacyShellOverlay.isValid()) {
                             legacyShellOverlay.asyncCall(QStringLiteral("FocusCaptureMenu"), pid);
                         }
                     });
}

inline QDBusMessage hideShellOverlayCountdown(QDBusInterface& shellOverlay)
{
    if (!isFlatpakSandboxed()) {
        return shellOverlay.call(QStringLiteral("HideCountdown"));
    }

    const QDBusMessage reply =
        shellOverlay.call(QStringLiteral("HideCountdownV2"), daemonBusName());
    if (reply.type() == QDBusMessage::ErrorMessage
        && reply.errorName() == QStringLiteral("org.freedesktop.DBus.Error.UnknownMethod")) {
        return shellOverlay.call(QStringLiteral("HideCountdown"));
    }
    return reply;
}

inline QString runtimeIpcDirectory()
{
    const QString runtimeDir = qEnvironmentVariable("XDG_RUNTIME_DIR");
    const QString baseDir = runtimeDir.isEmpty() ? QDir::tempPath() : runtimeDir;
    if (isFlatpakSandboxed()) {
        return QDir(baseDir).filePath(QStringLiteral("app/org.apexshot.ApexShot"));
    }
    return baseDir;
}
