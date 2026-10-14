// SPDX-License-Identifier: GPL-3.0-or-later

#pragma once

#include <QByteArray>
#include <QString>

namespace CaptureMenuResult {

enum class Action {
    Display,
    Window,
    Area,
};

struct Selection {
    Action action = Action::Display;
    bool recording = false;
    bool ocr = false;
    int timerSeconds = 0;
    bool microphone = false;
    bool speaker = false;
};

bool parse(const QByteArray& json,
           bool recordingAvailable,
           Selection* selection,
           QString* error = nullptr);

}
