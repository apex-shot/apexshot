// CaptureOverlay_p.h — Private implementation constants shared across TUs
#pragma once

#include <algorithm>
#include <QRectF>

// ── Constants ────────────────────────────────────────────────────────────────
inline constexpr double HANDLE_MARKER_LENGTH      = 20.0;
inline constexpr double HANDLE_MARKER_THICKNESS   = 2.5;
inline constexpr double FEATURE_PANEL_ITEM_W      = 62.0;
inline constexpr double FEATURE_PANEL_H           = 46.0;
inline constexpr double FEATURE_PANEL_RADIUS      = 13.0;
inline constexpr double FEATURE_PANEL_TOP_GAP     = 12.0;
inline constexpr double FEATURE_PANEL_MARGIN      = 16.0;
inline constexpr double SCROLL_HANDLE_DOT_RADIUS  = 4.5;
inline constexpr double SCROLL_BUTTON_H           = 36.0;
inline constexpr double SCROLL_BUTTON_GAP         = 10.0;
inline constexpr double SCROLL_BUTTON_RADIUS      = 10.0;
inline constexpr double SCROLL_BUTTON_MIN_W       = 128.0;
inline constexpr int    SCROLL_CAPTURE_INTERVAL_MS = 300;
inline constexpr int    DEFAULT_SELECTION_W       = 600;
inline constexpr int    DEFAULT_SELECTION_H       = 744;
inline constexpr int    NUM_TOOLS                 = 8;

extern const char* TOOLBAR_LABELS[NUM_TOOLS];

struct ToolbarLayout {
    QRectF toolsPanel;
    QRectF sizePanel;
    QRectF itemCells[NUM_TOOLS];
};

ToolbarLayout computeToolbarLayout(double selX, double selY,
                                   double selW, double selH,
                                   double screenW, double screenH,
                                   bool forceAbove = false);
