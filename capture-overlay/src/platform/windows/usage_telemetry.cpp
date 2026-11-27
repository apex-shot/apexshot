#include "platform/windows/usage_telemetry.h"

#include "platform/windows/settings_store.h"
#include "platform/windows/win_util.h"

#include <winhttp.h>

#include <QDateTime>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <QFutureWatcher>
#include <QPointer>
#include <QSaveFile>
#include <QStringList>
#include <QtConcurrent/QtConcurrentRun>
#include <QUuid>

#include <algorithm>

namespace {

QString statePath()
{
    return QFileInfo(Platform::shellSettingsPath()).absoluteDir().filePath(
        QStringLiteral("windows-usage-telemetry.json"));
}

QJsonObject readState()
{
    QFile file(statePath());
    if (!file.open(QIODevice::ReadOnly))
        return {};
    return QJsonDocument::fromJson(file.readAll()).object();
}

bool writeState(const QJsonObject& state)
{
    const QString path = statePath();
    if (!QDir().mkpath(QFileInfo(path).absolutePath()))
        return false;
    const QByteArray data = QJsonDocument(state).toJson(QJsonDocument::Compact);
    QSaveFile file(path);
    return file.open(QIODevice::WriteOnly)
        && file.write(data) == data.size()
        && file.commit();
}

int count(const QJsonObject& state, const QString& key)
{
    return std::max(0, state.value(key).toInt());
}

/// Counters shared with the Rust editors, which write the same state file.
const QStringList& counterKeys()
{
    static const QStringList keys{
        QStringLiteral("screenshots"),
        QStringLiteral("recordings"),
        QStringLiteral("ocr"),
        QStringLiteral("upload_click_signed_out"),
        QStringLiteral("login_started"),
        QStringLiteral("login_completed"),
        QStringLiteral("upgrade_card_shown"),
        QStringLiteral("upgrade_clicked"),
        QStringLiteral("editor_opened"),
        QStringLiteral("studio_tool_opened"),
        QStringLiteral("studio_trial_export"),
        QStringLiteral("studio_upsell_shown"),
        QStringLiteral("studio_buy_clicked"),
        QStringLiteral("cloud_nudge_shown"),
        QStringLiteral("cloud_nudge_clicked"),
        QStringLiteral("cloud_nudge_dismissed"),
        QStringLiteral("connect_link_opened"),
    };
    return keys;
}

bool postHeartbeat(const QByteArray& data)
{
    HINTERNET session = WinHttpOpen(L"ApexShotUsageTelemetry",
                                    WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
                                    WINHTTP_NO_PROXY_NAME, WINHTTP_NO_PROXY_BYPASS, 0);
    if (!session)
        return false;
    WinHttpSetTimeouts(session, 2000, 2000, 2000, 2000);
    HINTERNET connection = WinHttpConnect(session, L"apexshot.org", INTERNET_DEFAULT_HTTPS_PORT, 0);
    HINTERNET request = connection
        ? WinHttpOpenRequest(connection, L"POST", L"/api/usage-telemetry", nullptr,
                             WINHTTP_NO_REFERER, WINHTTP_DEFAULT_ACCEPT_TYPES, WINHTTP_FLAG_SECURE)
        : nullptr;
    bool success = false;
    if (request) {
        DWORD redirectPolicy = WINHTTP_OPTION_REDIRECT_POLICY_NEVER;
        WinHttpSetOption(request, WINHTTP_OPTION_REDIRECT_POLICY, &redirectPolicy, sizeof(redirectPolicy));
        const wchar_t headers[] = L"Content-Type: application/json\r\n";
        if (WinHttpSendRequest(request, headers, static_cast<DWORD>(-1),
                               const_cast<char*>(data.constData()),
                               static_cast<DWORD>(data.size()), static_cast<DWORD>(data.size()), 0)
            && WinHttpReceiveResponse(request, nullptr)) {
            DWORD status = 0;
            DWORD length = sizeof(status);
            success = WinHttpQueryHeaders(request,
                                           WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                                           WINHTTP_HEADER_NAME_BY_INDEX, &status, &length,
                                           WINHTTP_NO_HEADER_INDEX)
                && status >= 200 && status < 300;
        }
        WinHttpCloseHandle(request);
    }
    if (connection)
        WinHttpCloseHandle(connection);
    WinHttpCloseHandle(session);
    return success;
}

bool telemetryEnabled()
{
    const QString override = qEnvironmentVariable("APEXSHOT_TELEMETRY").trimmed().toLower();
    if (override == QLatin1String("0") || override == QLatin1String("false")
        || override == QLatin1String("no") || override == QLatin1String("off"))
        return false;
    return Platform::loadShellSettings().telemetryEnabled;
}

} // namespace

namespace Platform {

UsageTelemetry::UsageTelemetry(QObject* parent, Sender sender)
    : QObject(parent), m_sender(std::move(sender))
{
    m_timer.setInterval(5000);
    connect(&m_timer, &QTimer::timeout, this, [this] { poll(); });
}

bool UsageTelemetry::enabled() const
{
    return telemetryEnabled();
}

void incrementUsageCounter(const QString& key)
{
    if (!telemetryEnabled())
        return;
    QJsonObject state = readState();
    state.insert(key, std::min(count(state, key), 999999999) + 1);
    writeState(state);
}

void UsageTelemetry::start()
{
    m_timer.start();
    QTimer::singleShot(3000, this, [this] { poll(); });
}

void UsageTelemetry::increment(const QString& key)
{
    incrementUsageCounter(key);
}

void UsageTelemetry::recordScreenshot() { increment(QStringLiteral("screenshots")); }
void UsageTelemetry::recordRecording() { increment(QStringLiteral("recordings")); }
void UsageTelemetry::recordOcr() { increment(QStringLiteral("ocr")); }

void UsageTelemetry::poll()
{
    if (!enabled()) {
        return;
    }
    if (m_inFlight)
        return;

    QJsonObject state = readState();
    const qint64 now = QDateTime::currentSecsSinceEpoch();
    const qint64 last = static_cast<qint64>(state.value(QStringLiteral("last_heartbeat")).toDouble());
    if (last > 0 && now - last < 24 * 60 * 60)
        return;
    if (m_lastAttempt > 0 && now - m_lastAttempt < 60 * 60)
        return;

    QString id = state.value(QStringLiteral("install_id")).toString();
    if (QUuid(id).isNull()) {
        id = QUuid::createUuid().toString(QUuid::WithoutBraces);
        state.insert(QStringLiteral("install_id"), id);
        if (!writeState(state))
            return;
    }
    QJsonObject sentCounts;
    QJsonObject payload{
        {QStringLiteral("event"), QStringLiteral("heartbeat")},
        {QStringLiteral("install_id"), id},
        {QStringLiteral("app_version"), QStringLiteral(APEXSHOT_VERSION)},
        {QStringLiteral("channel"), QStringLiteral("daemon")},
        {QStringLiteral("distro"), QStringLiteral("windows")},
        {QStringLiteral("desktop"), QStringLiteral("windows")},
        {QStringLiteral("session"), QStringLiteral("windows")},
    };
    for (const QString& key : counterKeys()) {
        const int value = count(state, key);
        sentCounts.insert(key, value);
        payload.insert(key, value);
    }
    m_lastAttempt = now;
    m_inFlight = true;
    QPointer<UsageTelemetry> self(this);
    Completion finished = [self, id, now, sentCounts](bool success) {
        if (!self)
            return;
        self->m_inFlight = false;
        if (!success)
            return;
        QJsonObject current = readState();
        if (current.value(QStringLiteral("install_id")).toString() != id)
            return;
        current.insert(QStringLiteral("last_heartbeat"), static_cast<double>(now));
        // Counters that landed while the request was in flight stay in the
        // difference, so only the amounts actually sent are cleared.
        for (auto it = sentCounts.constBegin(); it != sentCounts.constEnd(); ++it) {
            current.insert(it.key(),
                           std::max(0, count(current, it.key()) - it.value().toInt()));
        }
        writeState(current);
    };
    if (m_sender)
        m_sender(payload, std::move(finished));
    else
        sendNetwork(payload, std::move(finished));
}

void UsageTelemetry::sendNetwork(const QJsonObject& payload, Completion completion)
{
    const QByteArray data = QJsonDocument(payload).toJson(QJsonDocument::Compact);
    auto* watcher = new QFutureWatcher<bool>(this);
    connect(watcher, &QFutureWatcher<bool>::finished, this, [watcher, completion = std::move(completion)] {
        const bool success = watcher->result();
        watcher->deleteLater();
        completion(success);
    });
    watcher->setFuture(QtConcurrent::run([data] {
        return telemetryEnabled() && postHeartbeat(data);
    }));
}

} // namespace Platform
