// SPDX-License-Identifier: GPL-3.0-or-later

#include "CaptureMenuResult.h"

#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QJsonValue>

#include <cmath>

namespace CaptureMenuResult {

bool parse(const QByteArray& json,
           bool recordingAvailable,
           Selection* selection,
           QString* error)
{
    const auto reject = [error](const QString& message) {
        if (error) {
            *error = message;
        }
        return false;
    };

    if (!selection) {
        return reject(QStringLiteral("missing output selection"));
    }

    QJsonParseError parseError;
    const QJsonDocument document = QJsonDocument::fromJson(json, &parseError);
    if (parseError.error != QJsonParseError::NoError || !document.isObject()) {
        return reject(QStringLiteral("invalid JSON object"));
    }

    const QJsonObject object = document.object();
    if (object.size() != 9
        || !object.contains(QStringLiteral("action"))
        || !object.contains(QStringLiteral("recording"))
        || !object.contains(QStringLiteral("ocr"))
        || !object.contains(QStringLiteral("timer_seconds"))
        || !object.contains(QStringLiteral("microphone"))
        || !object.contains(QStringLiteral("speaker"))
        || !object.contains(QStringLiteral("mic_device"))
        || !object.contains(QStringLiteral("speaker_app"))
        || !object.contains(QStringLiteral("speaker_app_pid"))) {
        return reject(QStringLiteral("unsupported menu fields"));
    }

    const QJsonValue actionValue = object.value(QStringLiteral("action"));
    const QJsonValue recordingValue = object.value(QStringLiteral("recording"));
    const QJsonValue ocrValue = object.value(QStringLiteral("ocr"));
    const QJsonValue timerValue = object.value(QStringLiteral("timer_seconds"));
    const QJsonValue microphoneValue = object.value(QStringLiteral("microphone"));
    const QJsonValue speakerValue = object.value(QStringLiteral("speaker"));
    const QJsonValue microphoneDeviceValue = object.value(QStringLiteral("mic_device"));
    const QJsonValue speakerAppValue = object.value(QStringLiteral("speaker_app"));
    const QJsonValue speakerAppPidValue = object.value(QStringLiteral("speaker_app_pid"));
    if (!actionValue.isString() || !recordingValue.isBool() || !ocrValue.isBool()
        || !timerValue.isDouble() || !microphoneValue.isBool() || !speakerValue.isBool()
        || !microphoneDeviceValue.isString() || !speakerAppValue.isString()
        || !speakerAppPidValue.isDouble()) {
        return reject(QStringLiteral("invalid menu field types"));
    }

    Selection parsed;
    const QString action = actionValue.toString();
    if (action == QStringLiteral("display")) {
        parsed.action = Action::Display;
    } else if (action == QStringLiteral("window")) {
        parsed.action = Action::Window;
    } else if (action == QStringLiteral("area")) {
        parsed.action = Action::Area;
    } else {
        return reject(QStringLiteral("unsupported menu action"));
    }

    const double timerSeconds = timerValue.toDouble();
    if (!std::isfinite(timerSeconds) || std::floor(timerSeconds) != timerSeconds
        || timerSeconds < 0 || timerSeconds > 10) {
        return reject(QStringLiteral("invalid timer value"));
    }

    const double speakerAppPid = speakerAppPidValue.toDouble();
    if (!std::isfinite(speakerAppPid) || std::floor(speakerAppPid) != speakerAppPid
        || speakerAppPid < 0 || speakerAppPid > 4294967295.0) {
        return reject(QStringLiteral("invalid app process id"));
    }

    parsed.recording = recordingValue.toBool();
    parsed.ocr = ocrValue.toBool();
    parsed.timerSeconds = static_cast<int>(timerSeconds);
    parsed.microphone = microphoneValue.toBool();
    parsed.speaker = speakerValue.toBool();
    parsed.microphoneDevice = microphoneDeviceValue.toString();
    parsed.speakerApp = speakerAppValue.toString();
    parsed.speakerAppPid = static_cast<quint32>(speakerAppPid);

    if (parsed.timerSeconds != 0 && parsed.timerSeconds != 3
        && parsed.timerSeconds != 5 && parsed.timerSeconds != 10) {
        return reject(QStringLiteral("unsupported timer value"));
    }
    if (parsed.recording) {
        if (!recordingAvailable || parsed.ocr || parsed.timerSeconds != 0
            || parsed.action == Action::Area) {
            return reject(QStringLiteral("unsupported recording state"));
        }
    } else if ((parsed.ocr && parsed.timerSeconds > 0)
               || (parsed.action == Action::Display && parsed.ocr)
               || (parsed.action == Action::Window && (parsed.ocr || parsed.timerSeconds > 0))
               || (parsed.action == Action::Area && parsed.timerSeconds > 0)) {
        return reject(QStringLiteral("unsupported screenshot state"));
    }

    *selection = parsed;
    if (error) {
        error->clear();
    }
    return true;
}

}
