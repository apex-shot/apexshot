// SPDX-License-Identifier: GPL-3.0-or-later

#include "platform/linux_audio_devices.h"

#include <QByteArray>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QJsonValue>
#include <QProcess>
#include <QSet>
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

QString productName(const QJsonObject& object)
{
    return object.value(QStringLiteral("properties"))
        .toObject()
        .value(QStringLiteral("device.product.name"))
        .toString()
        .trimmed();
}

QString activePortDescription(const QJsonObject& object)
{
    const QString active = object.value(QStringLiteral("active_port")).toString();
    if (active.isEmpty())
        return {};
    for (const QJsonValue& value : object.value(QStringLiteral("ports")).toArray()) {
        const QJsonObject port = value.toObject();
        if (port.value(QStringLiteral("name")).toString() == active)
            return port.value(QStringLiteral("description")).toString().trimmed();
    }
    return {};
}

QString joinDisplayName(const QString& port, const QString& product, const QString& fallback)
{
    if (port.isEmpty())
        return fallback;
    if (product.isEmpty())
        return port;
    return QStringLiteral("%1 (%2)").arg(port, product);
}

/// "Digital Microphone (Raptor Lake-P/U/H cAVS)": the active port first and
/// the card in parentheses, the shape the Windows toolbar uses. Falls back to
/// the Pulse description when the server exposes no ports.
QString displayName(const QJsonObject& object, const QString& name)
{
    const QString product = productName(object);
    const QString port = activePortDescription(object);
    QString description = object.value(QStringLiteral("description")).toString().trimmed();
    if (description.isEmpty())
        description = object.value(QStringLiteral("properties"))
                          .toObject()
                          .value(QStringLiteral("device.description"))
                          .toString()
                          .trimmed();
    if (description.isEmpty())
        description = name;
    return joinDisplayName(port, product, description);
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
        devices.append({name, displayName(object, name)});
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
        devices.append({name, displayName(object, name), monitor});
    }
    return devices;
}

struct PactlPort {
    QString name;
    QString description;
};

struct PactlBlock {
    QString name;
    QString description;
    QString monitorSource;
    QString monitorOf;
    QString activePort;
    QString product;
    QVector<PactlPort> ports;
};

QString unquoted(const QString& value)
{
    QString text = value.trimmed();
    if (text.size() >= 2 && text.startsWith(QLatin1Char('"')) && text.endsWith(QLatin1Char('"')))
        text = text.mid(1, text.size() - 2);
    return text;
}

/// Split `pactl list sources` / `list sinks` long output into blocks. pactl
/// indents fields with one tab and properties/ports with two.
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
        PactlBlock& block = blocks.last();
        if (rawLine.startsWith(QStringLiteral("\t\t"))) {
            const QString line = rawLine.mid(2).trimmed();
            if (line.startsWith(QLatin1Char('['))) {
                // "[Out] HDMI3: HDMI / DisplayPort 3 Output (type: HDMI, ...)"
                const int separator = line.indexOf(QStringLiteral(": "));
                if (separator > 0) {
                    QString portDescription = line.mid(separator + 2);
                    const int typeAt = portDescription.indexOf(QStringLiteral(" (type:"));
                    if (typeAt >= 0)
                        portDescription.truncate(typeAt);
                    block.ports.append({line.left(separator).trimmed(),
                                        portDescription.trimmed()});
                }
            } else if (line.startsWith(QStringLiteral("device.product.name = "))) {
                block.product = unquoted(line.mid(22));
            }
            continue;
        }
        if (!rawLine.startsWith(QLatin1Char('\t')))
            continue;
        const QString line = rawLine.mid(1).trimmed();
        if (line.startsWith(QStringLiteral("Name: ")))
            block.name = line.mid(6).trimmed();
        else if (line.startsWith(QStringLiteral("Description: ")))
            block.description = line.mid(13).trimmed();
        else if (line.startsWith(QStringLiteral("Monitor Source: ")))
            block.monitorSource = line.mid(16).trimmed();
        else if (line.startsWith(QStringLiteral("Monitor of Sink: ")))
            block.monitorOf = line.mid(17).trimmed();
        else if (line.startsWith(QStringLiteral("Active Port: ")))
            block.activePort = line.mid(13).trimmed();
    }
    return blocks;
}

QString blockDisplayName(const PactlBlock& block)
{
    QString port;
    for (const PactlPort& entry : block.ports) {
        if (entry.name == block.activePort) {
            port = entry.description;
            break;
        }
    }
    const QString fallback = block.description.isEmpty() ? block.name : block.description;
    return joinDisplayName(port, block.product, fallback);
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
        devices.append({block.name, blockDisplayName(block)});
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
        devices.append({block.name, blockDisplayName(block), monitor});
    }
    return devices;
}

/// The recorder's own streams are never a choice in its menus.
bool isOwnAudioApplication(const QString& name)
{
    return name.contains(QStringLiteral("ApexShot"), Qt::CaseInsensitive);
}

QString applicationName(const QJsonObject& properties)
{
    QString name = properties.value(QStringLiteral("application.name")).toString().trimmed();
    if (name.isEmpty())
        name = properties.value(QStringLiteral("application.process.binary")).toString().trimmed();
    return name;
}

QVector<Platform::AudioApplication> parseApplicationsJson(const QByteArray& data)
{
    QJsonParseError error;
    const QJsonDocument document = QJsonDocument::fromJson(data, &error);
    if (error.error != QJsonParseError::NoError || !document.isArray())
        return {};

    QVector<Platform::AudioApplication> applications;
    QSet<quint32> seen;
    for (const QJsonValue& value : document.array()) {
        if (!value.isObject())
            continue;
        const QJsonObject properties =
            value.toObject().value(QStringLiteral("properties")).toObject();
        bool ok = false;
        const quint32 processId = properties.value(QStringLiteral("application.process.id"))
                                      .toString()
                                      .toUInt(&ok);
        if (!ok || processId == 0 || seen.contains(processId))
            continue;
        const QString name = applicationName(properties);
        if (name.isEmpty() || isOwnAudioApplication(name))
            continue;
        seen.insert(processId);
        applications.append({name, processId});
    }
    return applications;
}

struct SinkInputBlock {
    QString applicationName;
    QString binary;
    QString processId;
};

QVector<SinkInputBlock> parseSinkInputText(const QByteArray& data)
{
    QVector<SinkInputBlock> blocks;
    for (const QString& rawLine : QString::fromUtf8(data).split(QLatin1Char('\n'))) {
        if (rawLine.startsWith(QStringLiteral("Sink Input #"))) {
            blocks.append(SinkInputBlock{});
            continue;
        }
        if (blocks.isEmpty() || !rawLine.startsWith(QStringLiteral("\t\t")))
            continue;
        const QString line = rawLine.mid(2).trimmed();
        SinkInputBlock& block = blocks.last();
        if (line.startsWith(QStringLiteral("application.name = ")))
            block.applicationName = unquoted(line.mid(19));
        else if (line.startsWith(QStringLiteral("application.process.binary = ")))
            block.binary = unquoted(line.mid(29));
        else if (line.startsWith(QStringLiteral("application.process.id = ")))
            block.processId = unquoted(line.mid(25));
    }
    return blocks;
}

QVector<Platform::AudioApplication> applicationsFromBlocks(const QVector<SinkInputBlock>& blocks)
{
    QVector<Platform::AudioApplication> applications;
    QSet<quint32> seen;
    for (const SinkInputBlock& block : blocks) {
        bool ok = false;
        const quint32 processId = block.processId.toUInt(&ok);
        if (!ok || processId == 0 || seen.contains(processId))
            continue;
        const QString name =
            block.applicationName.isEmpty() ? block.binary : block.applicationName;
        if (name.isEmpty() || isOwnAudioApplication(name))
            continue;
        seen.insert(processId);
        applications.append({name, processId});
    }
    return applications;
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
    if (devices.isEmpty())
        devices = outputsFromBlocks(
            parseListText(runPactl({QStringLiteral("list"), QStringLiteral("sinks")})));
    const QString defaultMonitor = defaultSpeakerMonitor();
    for (AudioOutputDevice& device : devices)
        device.isDefault = device.monitorSource == defaultMonitor;
    return devices;
}

QVector<AudioApplication> pulseApplications()
{
    const QByteArray json =
        runPactl({QStringLiteral("-f"), QStringLiteral("json"), QStringLiteral("list"),
                  QStringLiteral("sink-inputs")});
    QVector<AudioApplication> applications = parseApplicationsJson(json);
    if (!applications.isEmpty())
        return applications;
    return applicationsFromBlocks(
        parseSinkInputText(runPactl({QStringLiteral("list"), QStringLiteral("sink-inputs")})));
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
