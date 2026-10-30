#include "platform/windows/settings_store.h"
#include "platform/windows/usage_telemetry.h"

#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QJsonDocument>
#include <QTemporaryDir>

#include <cstdio>

namespace {
int failures = 0;
#define CHECK(expr) do { if (!(expr)) { \
    std::fprintf(stderr, "FAIL line %d: %s\n", __LINE__, #expr); ++failures; \
} } while (false)

void setEnabled(bool enabled)
{
    const QString path = Platform::shellSettingsPath();
    QDir().mkpath(QFileInfo(path).absolutePath());
    QFile file(path);
    CHECK(file.open(QIODevice::WriteOnly));
    file.write(QJsonDocument(QJsonObject{
        {QStringLiteral("telemetry"), QJsonObject{{QStringLiteral("enabled"), enabled}}}
    }).toJson());
}

QJsonObject state()
{
    QFile file(QFileInfo(Platform::shellSettingsPath()).absoluteDir().filePath(
        QStringLiteral("windows-usage-telemetry.json")));
    if (!file.open(QIODevice::ReadOnly))
        return {};
    return QJsonDocument::fromJson(file.readAll()).object();
}
} // namespace

int main(int argc, char** argv)
{
    QCoreApplication app(argc, argv);
    QTemporaryDir dir;
    CHECK(dir.isValid());
    qputenv("APPDATA", dir.path().toUtf8());
    qunsetenv("APEXSHOT_TELEMETRY");

    int sent = 0;
    QJsonObject payload;
    Platform::UsageTelemetry::Completion complete;
    Platform::UsageTelemetry telemetry(nullptr, [&](const QJsonObject& data,
                                                     Platform::UsageTelemetry::Completion done) {
        ++sent;
        payload = data;
        complete = std::move(done);
    });
    CHECK(!Platform::loadShellSettings().telemetryEnabled);
    telemetry.recordScreenshot();
    telemetry.poll();
    CHECK(sent == 0);
    CHECK(state().isEmpty());

    setEnabled(true);
    CHECK(Platform::loadShellSettings().telemetryEnabled);
    qputenv("APEXSHOT_TELEMETRY", "0");
    telemetry.recordScreenshot();
    telemetry.poll();
    CHECK(state().isEmpty() && sent == 0);
    qunsetenv("APEXSHOT_TELEMETRY");

    telemetry.recordScreenshot();
    telemetry.recordRecording();
    telemetry.recordOcr();
    telemetry.poll();
    CHECK(sent == 1);
    CHECK(payload.size() == 10);
    CHECK(payload.value(QStringLiteral("event")).toString() == QStringLiteral("heartbeat"));
    CHECK(payload.value(QStringLiteral("distro")).toString() == QStringLiteral("windows"));
    CHECK(!payload.value(QStringLiteral("install_id")).toString().isEmpty());
    CHECK(payload.value(QStringLiteral("screenshots")).toInt() == 1);
    CHECK(payload.value(QStringLiteral("recordings")).toInt() == 1);
    CHECK(payload.value(QStringLiteral("ocr")).toInt() == 1);
    CHECK(!payload.contains(QStringLiteral("path")));
    CHECK(!payload.contains(QStringLiteral("text")));

    telemetry.recordScreenshot();
    CHECK(bool(complete));
    if (complete)
        complete(true);
    CHECK(state().value(QStringLiteral("screenshots")).toInt() == 1);
    CHECK(state().value(QStringLiteral("recordings")).toInt() == 0);
    CHECK(state().value(QStringLiteral("ocr")).toInt() == 0);
    telemetry.poll();
    CHECK(sent == 1);

    setEnabled(false);
    telemetry.recordScreenshot();
    CHECK(state().value(QStringLiteral("screenshots")).toInt() == 1);
    telemetry.poll();
    CHECK(sent == 1);

    QTemporaryDir failedDir;
    CHECK(failedDir.isValid());
    qputenv("APPDATA", failedDir.path().toUtf8());
    setEnabled(true);
    int attempts = 0;
    Platform::UsageTelemetry failing(nullptr, [&](const QJsonObject&,
                                                  Platform::UsageTelemetry::Completion done) {
        ++attempts;
        done(false);
    });
    failing.recordScreenshot();
    failing.poll();
    failing.poll();
    CHECK(attempts == 1);
    CHECK(state().value(QStringLiteral("screenshots")).toInt() == 1);
    CHECK(!state().contains(QStringLiteral("last_heartbeat")));
    return failures ? 1 : 0;
}
