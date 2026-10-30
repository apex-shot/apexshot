#pragma once

#include <QString>
#include <functional>
#include <memory>

class QObject;

namespace Platform {

struct ParsedHotkey {
    unsigned int modifiers = 0;
    unsigned int key = 0;
};

struct HotkeyRegistration {
    std::function<bool(int, unsigned int, unsigned int)> registerKey;
    std::function<void(int)> unregisterKey;
};

/// Parse the shortcut editor's stored form into Windows virtual-key codes.
bool parseHotkey(const QString& text, ParsedHotkey& out, QString& error);

/// Owns the Windows registrations for the lifetime of the capture shell.
class GlobalHotkeys {
public:
    GlobalHotkeys(QObject* context, std::function<void(const QString&)> activate,
                  HotkeyRegistration registration = {}, QString windowClass = {});
    ~GlobalHotkeys();

    GlobalHotkeys(const GlobalHotkeys&) = delete;
    GlobalHotkeys& operator=(const GlobalHotkeys&) = delete;

private:
    class Impl;
    std::unique_ptr<Impl> m_impl;
};

} // namespace Platform
