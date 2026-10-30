#pragma once

#include <QJsonObject>
#include <QObject>
#include <QTimer>

#include <functional>

namespace Platform {

/// Windows telemetry is opt-in and never includes capture contents or paths.
class UsageTelemetry : public QObject
{
public:
    using Completion = std::function<void(bool)>;
    using Sender = std::function<void(const QJsonObject&, Completion)>;

    explicit UsageTelemetry(QObject* parent = nullptr, Sender sender = {});
    void start();
    void poll();
    void recordScreenshot();
    void recordRecording();
    void recordOcr();

private:
    bool enabled() const;
    void increment(const QString& key);
    void sendNetwork(const QJsonObject& payload, Completion completion);

    QTimer m_timer;
    Sender m_sender;
    bool m_inFlight = false;
    qint64 m_lastAttempt = 0;
};

} // namespace Platform
