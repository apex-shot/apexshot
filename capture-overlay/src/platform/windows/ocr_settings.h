#pragma once

#include <QHash>
#include <QStringList>

namespace Platform {

/// Select an installed recognizer for the chosen language, not the system default.
inline QString ocrRecognizerTag(const QString& code, const QStringList& available)
{
    static const QHash<QString, QString> tags{
        {QStringLiteral("eng"), QStringLiteral("en-US")},
        {QStringLiteral("spa"), QStringLiteral("es-ES")},
        {QStringLiteral("fra"), QStringLiteral("fr-FR")},
        {QStringLiteral("deu"), QStringLiteral("de-DE")},
        {QStringLiteral("ita"), QStringLiteral("it-IT")},
        {QStringLiteral("por"), QStringLiteral("pt-BR")},
        {QStringLiteral("chi_sim"), QStringLiteral("zh-Hans")},
        {QStringLiteral("jpn"), QStringLiteral("ja-JP")},
        {QStringLiteral("rus"), QStringLiteral("ru-RU")},
    };
    const QString trimmed = code.trimmed();
    const QString preferred = tags.value(trimmed, trimmed.contains(QLatin1Char('-')) ? trimmed : QString());
    if (preferred.isEmpty())
        return {};
    for (const QString& tag : available) {
        if (tag.compare(preferred, Qt::CaseInsensitive) == 0)
            return tag;
    }
    const QString prefix = preferred.section(QLatin1Char('-'), 0, 0).toLower();
    for (const QString& tag : available) {
        const QString lower = tag.toLower();
        if (prefix == QLatin1String("zh")) {
            if (lower == QLatin1String("zh-cn") || lower == QLatin1String("zh-sg")
                || lower == QLatin1String("zh-hans") || lower.startsWith(QLatin1String("zh-hans-")))
                return tag;
        } else if (lower == prefix || lower.startsWith(prefix + QLatin1Char('-'))) {
            return tag;
        }
    }
    return {};
}

inline QString formatOcrLines(const QStringList& lines, bool keepLineBreaks)
{
    return lines.join(keepLineBreaks ? QChar('\n') : QChar(' '));
}

} // namespace Platform
