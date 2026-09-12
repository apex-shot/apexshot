// SPDX-License-Identifier: GPL-3.0-or-later

#pragma once

#include <QRect>

class QScreen;

namespace CaptureCountdownPill {

/// Shows the compact top-of-screen countdown and returns false if cancelled.
bool run(QScreen* screen, int seconds, const QRect& fadeGlobalRect = QRect());

} // namespace CaptureCountdownPill
