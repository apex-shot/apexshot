#pragma once

#include "platform/platform.h"

#include <algorithm>

namespace Platform {

/// A region spanning displays gets the highest scale among its source displays.
inline qreal screenshotScale(const QRect& region, const QVector<MonitorInfo>& displays)
{
    qreal scale = 1.0;
    for (const MonitorInfo& display : displays) {
        if (display.geometry.intersects(region))
            scale = std::max(scale, display.dpr);
    }
    return scale;
}

} // namespace Platform
