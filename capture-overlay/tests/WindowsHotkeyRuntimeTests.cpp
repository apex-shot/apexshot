#include "platform/windows/hotkeys.h"
#include "platform/windows/win_util.h"

#include <QCoreApplication>
#include <QDir>
#include <QEventLoop>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QMap>
#include <QSaveFile>
#include <QTemporaryDir>
#include <QTimer>

#include <cstdio>

namespace {

int failures = 0;

void check(bool condition, const char* message)
{
    if (!condition) {
        std::printf("FAIL: %s\n", message);
        ++failures;
    }
}

void waitForTimer(int ms)
{
    QEventLoop loop;
    QTimer::singleShot(ms, &loop, &QEventLoop::quit);
    loop.exec();
}

QJsonObject readObject(const QString& path)
{
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly))
        return {};
    return QJsonDocument::fromJson(file.readAll()).object();
}

void writeBindings(const QString& path, const QString& area, const QString& record)
{
    const QJsonObject shortcuts{
        {QStringLiteral("quick_capture"), QString()},
        {QStringLiteral("capture_area"), area},
        {QStringLiteral("capture_crosshair"), QString()},
        {QStringLiteral("capture_previous_area"), QString()},
        {QStringLiteral("capture_fullscreen"), QString()},
        {QStringLiteral("show_last_preview"), QString()},
        {QStringLiteral("save_last_capture"), QString()},
        {QStringLiteral("record_screen"), record},
        {QStringLiteral("recording_stop_save"), QString()},
    };
    QSaveFile file(path);
    check(file.open(QIODevice::WriteOnly), "open temporary settings");
    file.write(QJsonDocument(QJsonObject{{QStringLiteral("shortcuts"), shortcuts}}).toJson());
    check(file.commit(), "commit temporary settings");
}

} // namespace

int main(int argc, char** argv)
{
    QCoreApplication app(argc, argv);
    QTemporaryDir root;
    check(root.isValid(), "create temporary app data");
    qputenv("APPDATA", root.path().toUtf8());
    QDir().mkpath(root.filePath(QStringLiteral("apexshot")));
    const QString settings = root.filePath(QStringLiteral("apexshot/settings.json"));
    const QString status = root.filePath(QStringLiteral("apexshot/hotkeys-status.json"));
    writeBindings(settings, QStringLiteral("Ctrl+Alt+F23"), QStringLiteral("Ctrl+Alt+F24"));

    QMap<int, QPair<unsigned int, unsigned int>> registered;
    Platform::HotkeyRegistration backend;
    backend.registerKey = [&registered](int id, unsigned int modifiers, unsigned int key) {
        const auto combo = qMakePair(modifiers, key);
        if (registered.values().contains(combo))
            return false;
        registered.insert(id, combo);
        return true;
    };
    backend.unregisterKey = [&registered](int id) { registered.remove(id); };
    QString activated;
    const QString className = QStringLiteral("ApexShotHotkeyTest_%1").arg(GetCurrentProcessId());

    {
        Platform::GlobalHotkeys hotkeys(
            &app, [&activated](const QString& action) { activated = action; }, backend, className);
        check(registered.size() == 2, "register configured bindings");
        check(readObject(status).value(QStringLiteral("errors")).toArray().isEmpty(),
              "initial status is clear");

        const HWND window = FindWindowW(className.toStdWString().c_str(), nullptr);
        check(window != nullptr, "listener window exists");
        int areaId = -1;
        for (auto it = registered.cbegin(); it != registered.cend(); ++it) {
            if (it.value().second == VK_F23)
                areaId = it.key();
        }
        SendMessageW(window, WM_HOTKEY, areaId, 0);
        QCoreApplication::processEvents();
        check(activated == QLatin1String("capture_area"), "dispatch the configured action");

        activated.clear();
        SendMessageW(window, WM_HOTKEY, areaId, 0);
        check(SendMessageW(window, WM_APP + 0x531, 1, GetCurrentProcessId()) == 1,
              "pause for shortcut editing");
        QCoreApplication::processEvents();
        check(activated.isEmpty(), "queued hotkey is suppressed while editing");
        check(registered.isEmpty(), "pause releases all bindings");
        check(readObject(status).value(QStringLiteral("suspended")).toBool(),
              "pause is reported");
        check(SendMessageW(window, WM_APP + 0x531, 0, GetCurrentProcessId()) == 1,
              "resume after shortcut editing");
        check(registered.size() == 2, "resume restores bindings");

        writeBindings(settings, QStringLiteral("Ctrl+Alt+F24"), QStringLiteral("Ctrl+Alt+F23"));
        waitForTimer(1000);
        check(registered.size() == 2 && registered.value(areaId).second == VK_F24,
              "live reload supports swapping bindings");

        writeBindings(settings, QStringLiteral("Ctrl+Alt+F23"), QStringLiteral("Ctrl+Alt+F23"));
        waitForTimer(1000);
        const QJsonArray errors = readObject(status).value(QStringLiteral("errors")).toArray();
        check(registered.size() == 1 && errors.size() == 1
                  && errors.first().toObject().value(QStringLiteral("action"))
                      == QLatin1String("capture_area"),
              "conflicting shortcut is reported and not registered");

        writeBindings(settings, QStringLiteral("Ctrl+Alt+F23"), QString());
        waitForTimer(4000);
        check(registered.size() == 1 && registered.value(areaId).second == VK_F23
                  && readObject(status).value(QStringLiteral("errors")).toArray().isEmpty(),
              "failed registration is retried when the conflict is removed");
    }
    check(registered.isEmpty(), "destruction unregisters bindings");
    if (!failures)
        std::printf("hotkey runtime checks passed\n");
    return failures ? 1 : 0;
}
