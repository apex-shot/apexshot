#include "platform/windows/hotkeys.h"

#include "platform/windows/win_util.h"

#include <QMap>
#include <QStringList>

namespace Platform {

bool parseHotkey(const QString& text, ParsedHotkey& out, QString& error)
{
    out = {};
    error.clear();
    const QStringList parts = text.trimmed().split(QLatin1Char('+'));
    if (parts.isEmpty() || text.trimmed().isEmpty()) {
        error = QStringLiteral("Empty shortcut");
        return false;
    }

    unsigned int modifiers = 0;
    for (int i = 0; i < parts.size() - 1; ++i) {
        const QString part = parts.at(i).trimmed().toLower();
        unsigned int modifier = 0;
        if (part == QLatin1String("ctrl") || part == QLatin1String("control"))
            modifier = MOD_CONTROL;
        else if (part == QLatin1String("alt"))
            modifier = MOD_ALT;
        else if (part == QLatin1String("shift"))
            modifier = MOD_SHIFT;
        else if (part == QLatin1String("super") || part == QLatin1String("win")
                 || part == QLatin1String("meta"))
            modifier = MOD_WIN;
        if (!modifier || (modifiers & modifier)) {
            error = QStringLiteral("Invalid modifier");
            return false;
        }
        modifiers |= modifier;
    }

    const QString key = parts.last().trimmed().toLower();
    const QMap<QString, unsigned int> named = {
        {QStringLiteral("esc"), VK_ESCAPE}, {QStringLiteral("escape"), VK_ESCAPE},
        {QStringLiteral("backspace"), VK_BACK}, {QStringLiteral("tab"), VK_TAB},
        {QStringLiteral("return"), VK_RETURN}, {QStringLiteral("enter"), VK_RETURN},
        {QStringLiteral("space"), VK_SPACE}, {QStringLiteral("delete"), VK_DELETE},
        {QStringLiteral("insert"), VK_INSERT}, {QStringLiteral("home"), VK_HOME},
        {QStringLiteral("end"), VK_END}, {QStringLiteral("pageup"), VK_PRIOR},
        {QStringLiteral("pagedown"), VK_NEXT}, {QStringLiteral("left"), VK_LEFT},
        {QStringLiteral("right"), VK_RIGHT}, {QStringLiteral("up"), VK_UP},
        {QStringLiteral("down"), VK_DOWN}, {QStringLiteral("print"), VK_SNAPSHOT},
        {QStringLiteral("pause"), VK_PAUSE},
    };
    unsigned int virtualKey = named.value(key, 0);
    if (!virtualKey && key.startsWith(QLatin1Char('f'))) {
        bool ok = false;
        const int number = key.mid(1).toInt(&ok);
        if (ok && number >= 1 && number <= 24)
            virtualKey = VK_F1 + number - 1;
    }
    if (!virtualKey) {
        const QMap<QString, QChar> symbols = {
            {QStringLiteral("plus"), QLatin1Char('+')},
            {QStringLiteral("minus"), QLatin1Char('-')},
            {QStringLiteral("equal"), QLatin1Char('=')},
            {QStringLiteral("exclam"), QLatin1Char('!')},
            {QStringLiteral("at"), QLatin1Char('@')},
            {QStringLiteral("numbersign"), QLatin1Char('#')},
            {QStringLiteral("dollar"), QLatin1Char('$')},
            {QStringLiteral("percent"), QLatin1Char('%')},
            {QStringLiteral("asciicircum"), QLatin1Char('^')},
            {QStringLiteral("ampersand"), QLatin1Char('&')},
            {QStringLiteral("asterisk"), QLatin1Char('*')},
            {QStringLiteral("parenleft"), QLatin1Char('(')},
            {QStringLiteral("parenright"), QLatin1Char(')')},
            {QStringLiteral("underscore"), QLatin1Char('_')},
            {QStringLiteral("comma"), QLatin1Char(',')},
            {QStringLiteral("period"), QLatin1Char('.')},
            {QStringLiteral("slash"), QLatin1Char('/')},
            {QStringLiteral("semicolon"), QLatin1Char(';')},
            {QStringLiteral("colon"), QLatin1Char(':')},
            {QStringLiteral("apostrophe"), QLatin1Char('\'')},
            {QStringLiteral("quotedbl"), QLatin1Char('"')},
            {QStringLiteral("bracketleft"), QLatin1Char('[')},
            {QStringLiteral("bracketright"), QLatin1Char(']')},
            {QStringLiteral("backslash"), QLatin1Char('\\')},
            {QStringLiteral("grave"), QLatin1Char('`')},
        };
        const QChar character = key.size() == 1 ? key.at(0) : symbols.value(key);
        if (!character.isNull()) {
            const SHORT mapped = VkKeyScanExW(character.unicode(), GetKeyboardLayout(0));
            if (mapped != -1) {
                virtualKey = LOBYTE(mapped);
                const BYTE shiftState = HIBYTE(mapped);
                if (shiftState & 1)
                    modifiers |= MOD_SHIFT;
                if (shiftState & 2)
                    modifiers |= MOD_CONTROL;
                if (shiftState & 4)
                    modifiers |= MOD_ALT;
            }
        }
    }
    if (!virtualKey) {
        error = QStringLiteral("Unsupported key");
        return false;
    }
    if (!(modifiers & (MOD_CONTROL | MOD_ALT | MOD_WIN))
        && !(virtualKey >= VK_F1 && virtualKey <= VK_F24)
        && virtualKey != VK_SNAPSHOT && virtualKey != VK_PAUSE) {
        error = QStringLiteral("Use Ctrl, Alt, or Win to avoid blocking normal typing");
        return false;
    }
    out = {modifiers | MOD_NOREPEAT, virtualKey};
    return true;
}

} // namespace Platform
