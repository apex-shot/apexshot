// SPDX-License-Identifier: GPL-3.0-or-later

#include "platform/linux_audio_devices.h"

#include <QByteArray>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QJsonValue>
#include <QProcess>
#include <QStringList>

namespace {

constexpr int kPactlTimeoutMs = 1500;

/// Run pactl and return its stdout, or an empty array when it is missing,
/// hangs, or fails. The menus must never wait on Pulse for long.
QByteArray runPactl(const QStringList& args)
{
    QProcess process;
    process.start(QStringLiteral("pactl"), args);
    if (!process.waitForStarted(500))
        return {};
    if (!process.waitForFinished(kPactlTimeoutMs)) {
        process.kill();
        process.waitForFinished(200);
        return {};
    }
    if (process.exitStatus() != QProcess::NormalExit || process.exitCode() != 0)
        return {};
    return process.readAllStandardOutput();
}

QString pactlLine(const QStringList& args)
{
    const QByteArray output = runPactl(args);
    const QString line = QString::fromUtf8(output).trimmed();
    return line.section(QLatin1Char('\n'), 0, 0).trimmed();
}

QString descriptionFromObject(const QJsonObject& object, const QString& name)
{
    QString description = object.value(QStringLiteral("description")).toString().trimmed();
    if (description.isEmpty()) {
        const QJsonObject properties = object.value(QStringLiteral("properties")).toObject();
        description = properties.value(QStringLiteral("device.description")).toString().trimmed();
    }
    if (description.isEmpty())
        description = name;
    return description;
}

QVector<Platform::AudioSourceDevice> parseSourcesJson(const QByteArray& data)
{
    QJsonParseError error;
    const QJsonDocument document = QJsonDocument::fromJson(data, &error);
    if (error.error != QJsonParseError::NoError || !document.isArray())
        return {};

    QVector<Platform::AudioSourceDevice> devices;
    for (const QJsonValue& value : document.array()) {
        if (!value.isObject())
            continue;
        const QJsonObject object = value.toObject();
        const QString name = object.value(QStringLiteral("name")).toString().trimmed();
        if (name.isEmpty() || name.contains(QStringLiteral("auto_null"))
            || name.endsWith(QStringLiteral(".monitor"))) {
            continue;
        }
        // A monitor source records an output, not a microphone.
        if (!object.value(QStringLiteral("monitor_source")).toString().isEmpty())
            continue;
        devices.append({name, descriptionFromObject(object, name)});
    }
    return devices;
}

QVector<Platform::AudioOutputDevice> parseSinksJson(const QByteArray& data)
{
    QJsonParseError error;
    const QJsonDocument document = QJsonDocument::fromJson(data, &error);
    if (error.error != QJsonParseError::NoError || !document.isArray())
        return {};

    QVector<Platform::AudioOutputDevice> devices;
    for (const QJsonValue& value : document.array()) {
        if (!value.isObject())
            continue;
        const QJsonObject object = value.toObject();
        const QString name = object.value(QStringLiteral("name")).toString().trimmed();
        if (name.isEmpty())
            continue;
        QString monitor = object.value(QStringLiteral("monitor_source")).toString().trimmed();
        if (monitor.isEmpty())
            monitor = name + QStringLiteral(".monitor");
        devices.append({name, descriptionFromObject(object, name), monitor});
    }
    return devices;
}

struct PactlBlock {
    QString name;
    QString description;
    QString monitorSource;
    QString monitorOf;
};

/// Split `pactl list sources` / `list sinks` long output into blocks. pactl
/// indents fields with one tab and properties with two, so only the one-tab
/// keys are read.
QVector<PactlBlock> parseListText(const QByteArray& data)
{
    QVector<PactlBlock> blocks;
    for (const QString& rawLine : QString::fromUtf8(data).split(QLatin1Char('\n'))) {
        if (rawLine.startsWith(QStringLiteral("Source #"))
            || rawLine.startsWith(QStringLiteral("Sink #"))) {
            blocks.append(PactlBlock{});
            continue;
        }
        if (blocks.isEmpty())
            continue;
        if (!rawLine.startsWith(QLatin1Char('\t')) || rawLine.startsWith(QStringLiteral("\t\t")))
            continue;
        const QString line = rawLine.mid(1).trimmed();
        PactlBlock& block = blocks.last();
        if (line.startsWith(QStringLiteral("Name: ")))
            block.name = line.mid(6).trimmed();
        else if (line.startsWith(QStringLiteral("Description: ")))
            block.description = line.mid(13).trimmed();
        else if (line.startsWith(QStringLiteral("Monitor Source: ")))
            block.monitorSource = line.mid(16).trimmed();
        else if (line.startsWith(QStringLiteral("Monitor of Sink: ")))
            block.monitorOf = line.mid(17).trimmed();
    }
    return blocks;
}

QVector<Platform::AudioSourceDevice> microphonesFromBlocks(const QVector<PactlBlock>& blocks)
{
    QVector<Platform::AudioSourceDevice> devices;
    for (const PactlBlock& block : blocks) {
        if (block.name.isEmpty() || block.name.contains(QStringLiteral("auto_null")))
            continue;
        // Real microphones report "n/a" here; monitors name their sink.
        if (!(block.monitorOf.isEmpty() || block.monitorOf == QStringLiteral("n/a")))
            continue;
        if (block.name.endsWith(QStringLiteral(".monitor")))
            continue;
        devices.append({block.name,
                        block.description.isEmpty() ? block.name : block.description});
    }
    return devices;
}

QVector<Platform::AudioOutputDevice> outputsFromBlocks(const QVector<PactlBlock>& blocks)
{
    QVector<Platform::AudioOutputDevice> devices;
    for (const PactlBlock& block : blocks) {
        if (block.name.isEmpty())
            continue;
        QString monitor = block.monitorSource;
        if (monitor.isEmpty())
            monitor = block.name + QStringLiteral(".monitor");
        devices.append({block.name,
                        block.description.isEmpty() ? block.name : block.description,
                        monitor});
    }
    return devices;
}

}  // namespace

namespace Platform {

QVector<AudioSourceDevice> pulseMicrophones()
{
    const QByteArray json = runPactl({QStringLiteral("-f"), QStringLiteral("json"),
                                      QStringLiteral("list"), QStringLiteral("sources")});
    QVector<AudioSourceDevice> devices = parseSourcesJson(json);
    if (!devices.isEmpty())
        return devices;
    return microphonesFromBlocks(
        parseListText(runPactl({QStringLiteral("list"), QStringLiteral("sources")})));
}

QVector<AudioOutputDevice> pulseOutputs()
{
    const QByteArray json = runPactl({QStringLiteral("-f"), QStringLiteral("json"),
                                      QStringLiteral("list"), QStringLiteral("sinks")});
    QVector<AudioOutputDevice> devices = parseSinksJson(json);
    if (!devices.isEmpty())
        return devices;
    return outputsFromBlocks(
        parseListText(runPactl({QStringLiteral("list"), QStringLiteral("sinks")})));
}

QString defaultMicrophoneSource()
{
    const QString source = pactlLine({QStringLiteral("get-default-source")});
    return source == QStringLiteral("default") ? QString() : source;
}

QString defaultSpeakerMonitor()
{
    const QString sink = pactlLine({QStringLiteral("get-default-sink")});
    if (sink.isEmpty() || sink == QStringLiteral("default"))
        return {};
    return sink + QStringLiteral(".monitor");
}

}  // namespace Platform
