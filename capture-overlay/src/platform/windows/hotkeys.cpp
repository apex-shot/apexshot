#include "platform/windows/hotkeys.h"

#include "platform/windows/settings_store.h"
#include "platform/windows/win_util.h"

#include <QDateTime>
#include <QDir>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QMap>
#include <QObject>
#include <QSaveFile>
#include <QTimer>

#include <utility>

namespace {

constexpr wchar_t kWindowClass[] = L"ApexShotGlobalHotkeys";
constexpr UINT kSuspendMessage = WM_APP + 0x531;

struct Entry {
    int id = 0;
    QString requested;
    QString error;
    bool registered = false;
};

} // namespace

namespace Platform {

class GlobalHotkeys::Impl {
public:
    Impl(QObject* context, std::function<void(const QString&)> activate,
         HotkeyRegistration registration, QString windowClass)
        : m_context(context), m_activate(std::move(activate)),
          m_registration(std::move(registration)),
          m_windowClass(windowClass.isEmpty() ? kWindowClass : windowClass.toStdWString())
    {
        createWindow();

        int id = 1;
        const auto shortcuts = ShellSettings{}.shortcuts;
        for (auto it = shortcuts.cbegin(); it != shortcuts.cend(); ++it) {
            Entry entry;
            entry.id = id++;
            m_entries.insert(it.key(), entry);
        }
        reload(true);
        m_timer.setInterval(750);
        QObject::connect(&m_timer, &QTimer::timeout, m_context, [this] {
            if (!m_window && !FindWindowW(m_windowClass.c_str(), nullptr)) {
                createWindow();
                if (m_window)
                    reload(true);
            }
            if (m_owner && WaitForSingleObject(m_owner, 0) == WAIT_OBJECT_0)
                resume();
            if (!m_suspended)
                reload(false);
            if (++m_ticks >= 4) {
                m_ticks = 0;
                if (!m_suspended)
                    retryFailed();
                writeStatus();
            }
        });
        m_timer.start();
    }

    ~Impl()
    {
        m_timer.stop();
        unregisterAll();
        if (m_owner)
            CloseHandle(m_owner);
        if (m_window)
            DestroyWindow(m_window);
    }

private:
    void createWindow()
    {
        WNDCLASSW klass{};
        klass.lpfnWndProc = windowProc;
        klass.hInstance = GetModuleHandleW(nullptr);
        klass.lpszClassName = m_windowClass.c_str();
        if (!RegisterClassW(&klass) && GetLastError() != ERROR_CLASS_ALREADY_EXISTS) {
            m_error = QStringLiteral("Could not register the shortcut listener window");
        } else if (FindWindowW(m_windowClass.c_str(), nullptr)) {
            m_error = QStringLiteral("Another ApexShot shortcut listener is already running");
        } else {
            m_window = CreateWindowExW(0, m_windowClass.c_str(), L"", 0, 0, 0, 0, 0,
                                       nullptr, nullptr, klass.hInstance, this);
            if (!m_window)
                m_error = QStringLiteral("Could not create the shortcut listener window");
            else
                m_error.clear();
        }
    }

    static LRESULT CALLBACK windowProc(HWND window, UINT message, WPARAM wparam, LPARAM lparam)
    {
        auto* self = reinterpret_cast<Impl*>(GetWindowLongPtrW(window, GWLP_USERDATA));
        if (message == WM_NCCREATE) {
            const auto* create = reinterpret_cast<CREATESTRUCTW*>(lparam);
            self = static_cast<Impl*>(create->lpCreateParams);
            SetWindowLongPtrW(window, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(self));
        }
        if (self && message == WM_HOTKEY) {
            self->activate(static_cast<int>(wparam));
            return 0;
        }
        if (self && message == kSuspendMessage)
            return self->setSuspended(wparam != 0, static_cast<DWORD>(lparam)) ? 1 : 0;
        return DefWindowProcW(window, message, wparam, lparam);
    }

    void activate(int id)
    {
        if (m_suspended)
            return;
        for (auto it = m_entries.cbegin(); it != m_entries.cend(); ++it) {
            if (it->id == id && it->registered) {
                const QString action = it.key();
                QTimer::singleShot(0, m_context, [this, action] {
                    if (!m_suspended && m_entries.value(action).registered)
                        m_activate(action);
                });
                return;
            }
        }
    }

    bool setSuspended(bool suspended, DWORD pid)
    {
        if (!suspended) {
            if (m_suspended && m_ownerPid == pid)
                resume();
            return true;
        }
        if (m_suspended)
            return m_ownerPid == pid;
        HANDLE owner = OpenProcess(SYNCHRONIZE, FALSE, pid);
        if (!owner)
            return false;
        m_owner = owner;
        m_ownerPid = pid;
        m_suspended = true;
        unregisterAll();
        writeStatus();
        return true;
    }

    void resume()
    {
        if (m_owner)
            CloseHandle(m_owner);
        m_owner = nullptr;
        m_ownerPid = 0;
        m_suspended = false;
        reload(true);
    }

    void unregisterAll()
    {
        for (auto it = m_entries.begin(); it != m_entries.end(); ++it) {
            if (it->registered && m_window)
                unregisterKey(it->id);
            it->registered = false;
        }
    }

    void reload(bool force)
    {
        const auto desired = loadShellSettings().shortcuts;
        if (!force && desired == m_desired)
            return;
        m_desired = desired;

        // Release all changed bindings first, allowing two actions to swap keys.
        for (auto it = m_entries.begin(); it != m_entries.end(); ++it) {
            if (force || it->requested != desired.value(it.key()).trimmed()) {
                if (it->registered && m_window)
                    unregisterKey(it->id);
                it->registered = false;
            }
        }
        for (auto it = m_entries.begin(); it != m_entries.end(); ++it) {
            const QString requested = desired.value(it.key()).trimmed();
            if (!force && it->requested == requested)
                continue;
            it->requested = requested;
            it->error.clear();
            if (requested.isEmpty())
                continue;
            ParsedHotkey parsed;
            if (!parseHotkey(requested, parsed, it->error))
                continue;
            if (!m_window || !registerKey(it->id, parsed)) {
                it->error = QStringLiteral("Already in use or reserved by Windows");
                continue;
            }
            it->registered = true;
        }
        writeStatus();
    }

    void retryFailed()
    {
        if (!m_window)
            return;
        for (auto it = m_entries.begin(); it != m_entries.end(); ++it) {
            if (it->registered || it->error.isEmpty())
                continue;
            ParsedHotkey parsed;
            QString error;
            if (parseHotkey(it->requested, parsed, error)
                && registerKey(it->id, parsed)) {
                it->registered = true;
                it->error.clear();
            }
        }
    }

    void writeStatus()
    {
        // Only the listener that owns the registrations publishes their status.
        if (!m_window && FindWindowW(m_windowClass.c_str(), nullptr))
            return;
        QJsonArray errors;
        for (auto it = m_entries.cbegin(); it != m_entries.cend(); ++it) {
            if (!it->error.isEmpty())
                errors.append(QJsonObject{{QStringLiteral("action"), it.key()},
                                          {QStringLiteral("binding"), it->requested},
                                          {QStringLiteral("reason"), it->error}});
        }
        const QJsonObject status{
            {QStringLiteral("timestamp_ms"), static_cast<double>(QDateTime::currentMSecsSinceEpoch())},
            {QStringLiteral("suspended"), m_suspended},
            {QStringLiteral("error"), m_error},
            {QStringLiteral("errors"), errors},
        };
        const QString path =
            QFileInfo(shellSettingsPath()).absoluteDir().filePath(QStringLiteral("hotkeys-status.json"));
        QDir().mkpath(QFileInfo(path).absolutePath());
        QSaveFile file(path);
        if (file.open(QIODevice::WriteOnly)) {
            file.write(QJsonDocument(status).toJson(QJsonDocument::Compact));
            file.commit();
        }
    }

    QObject* m_context;
    std::function<void(const QString&)> m_activate;
    HotkeyRegistration m_registration;
    std::wstring m_windowClass;
    HWND m_window = nullptr;
    HANDLE m_owner = nullptr;
    DWORD m_ownerPid = 0;
    bool m_suspended = false;
    int m_ticks = 0;
    QString m_error;
    QMap<QString, QString> m_desired;
    QMap<QString, Entry> m_entries;
    QTimer m_timer;

    bool registerKey(int id, const ParsedHotkey& parsed)
    {
        if (m_registration.registerKey)
            return m_registration.registerKey(id, parsed.modifiers, parsed.key);
        return RegisterHotKey(m_window, id, parsed.modifiers, parsed.key) != FALSE;
    }

    void unregisterKey(int id)
    {
        if (m_registration.unregisterKey)
            m_registration.unregisterKey(id);
        else
            UnregisterHotKey(m_window, id);
    }
};

GlobalHotkeys::GlobalHotkeys(QObject* context, std::function<void(const QString&)> activate,
                             HotkeyRegistration registration, QString windowClass)
    : m_impl(std::make_unique<Impl>(context, std::move(activate), std::move(registration),
                                    std::move(windowClass)))
{
}

GlobalHotkeys::~GlobalHotkeys() = default;

} // namespace Platform
