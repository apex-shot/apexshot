#pragma once

#include "platform/platform.h"

#include <QPoint>
#include <QRect>
#include <QSize>
#include <QString>
#include <QVector>

/// Windows Desktop Duplication helpers for the ffmpeg recorder.
///
/// The filter indexes outputs in DXGI enumeration order, so a monitor has to
/// be translated to that index and the capture rectangle has to be expressed
/// relative to the monitor's own origin.
namespace Platform {

struct DdagrabOutput {
    int index = -1;
    QString deviceName;
    QRect bounds;
};

struct DdagrabSource {
    bool valid = false;
    int outputIndex = -1;
    QPoint offset;
    QSize size;
    QString reason;
};

/// Attached DXGI outputs in the order the ffmpeg filter numbers them.
QVector<DdagrabOutput> dxgiOutputs(QString* error = nullptr);

/// Map a desktop rectangle to a DXGI output and an output-local crop.
///
/// Invalid when the rectangle is empty, leaves its display, or spans more than
/// one display. Callers fall back to gdigrab in that case.
DdagrabSource ddagrabSourceForRegion(const QRect& region,
                                     const QVector<MonitorInfo>& monitors,
                                     const QVector<DdagrabOutput>& outputs);

/// Same mapping, using the outputs attached to this session.
DdagrabSource ddagrabSourceForRegion(const QRect& region,
                                     const QVector<MonitorInfo>& monitors);

/// The `ddagrab` source string for an ffmpeg lavfi input.
QString ddagrabFilter(const DdagrabSource& source, int fps, bool includeCursor);

} // namespace Platform
