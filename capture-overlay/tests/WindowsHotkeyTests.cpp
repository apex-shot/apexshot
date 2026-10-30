#include "platform/windows/hotkeys.h"
#include "platform/windows/win_util.h"

#include <QCoreApplication>

#include <cstdio>

int main(int argc, char** argv)
{
    QCoreApplication app(argc, argv);
    int failures = 0;
    auto check = [&failures](bool condition, const char* message) {
        if (!condition) {
            std::printf("FAIL: %s\n", message);
            ++failures;
        }
    };
    Platform::ParsedHotkey key;
    QString error;
    check(Platform::parseHotkey(QStringLiteral("Ctrl+Alt+X"), key, error)
              && key.key == 'X' && (key.modifiers & (MOD_CONTROL | MOD_ALT)) == (MOD_CONTROL | MOD_ALT),
          "Ctrl+Alt+X");
    check(Platform::parseHotkey(QStringLiteral("Ctrl+Alt+A"), key, error)
              && key.key == 'A' && (key.modifiers & (MOD_CONTROL | MOD_ALT)) == (MOD_CONTROL | MOD_ALT),
          "Ctrl+Alt+A");
    check(Platform::parseHotkey(QStringLiteral("Shift+Super+4"), key, error)
              && key.key == '4' && (key.modifiers & (MOD_SHIFT | MOD_WIN)) == (MOD_SHIFT | MOD_WIN),
          "Shift+Super+4");
    check(Platform::parseHotkey(QStringLiteral("Ctrl+Alt+Shift+S"), key, error)
              && key.key == 'S' && (key.modifiers & MOD_SHIFT),
          "recording stop shortcut");
    check(Platform::parseHotkey(QStringLiteral("F24"), key, error) && key.key == VK_F24,
          "function key");
    check(Platform::parseHotkey(QStringLiteral("Ctrl+Print"), key, error)
              && key.key == VK_SNAPSHOT,
          "print screen");
    check(!Platform::parseHotkey(QStringLiteral("A"), key, error), "single typing key rejected");
    check(!Platform::parseHotkey(QStringLiteral("Shift+A"), key, error),
          "shift typing key rejected");
    check(!Platform::parseHotkey(QStringLiteral("Ctrl+Ctrl+A"), key, error),
          "duplicate modifier rejected");
    check(!Platform::parseHotkey(QStringLiteral("Ctrl+NotAKey"), key, error),
          "unsupported key rejected");
    check(!Platform::parseHotkey(QString(), key, error), "empty shortcut rejected");
    if (!failures)
        std::printf("hotkey parser checks passed\n");
    return failures ? 1 : 0;
}
