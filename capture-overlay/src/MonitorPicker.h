// SPDX-License-Identifier: GPL-3.0-or-later
// Multi-monitor picker (metadata UI only — freeze happens before this UI).

#pragma once

#include <QList>

class QScreen;

namespace MonitorPicker {

/// Show the monitor selection UI (no screenshot / freeze).
/// Returns the chosen screen index in @p screens, or -1 if cancelled (ESC).
int selectMonitorIndex(const QList<QScreen*>& screens);

/// Resolve the target screen for area capture:
/// - 1 monitor → that screen
/// - multi → interactive picker
/// Returns nullptr if the user cancels the picker.
QScreen* selectTargetScreen();

} // namespace MonitorPicker
