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
inline constexpr double TOOL_RAIL_W              = 76.0;
inline constexpr double TOOL_CARD_H              = 62.0;
inline constexpr double ACTION_RAIL_W            = 120.0;
inline constexpr double SIZE_CARD_H              = 46.0;
inline constexpr double ACTION_CARD_H            = 50.0;
inline constexpr double TOOL_RAIL_GAP            = 18.0;
inline constexpr double ACTION_CARD_GAP          = 8.0;
inline constexpr double SCROLL_HANDLE_DOT_RADIUS  = 4.5;
inline constexpr double SCROLL_BUTTON_H           = 36.0;
inline constexpr double SCROLL_BUTTON_GAP         = 10.0;
inline constexpr double SCROLL_BUTTON_RADIUS      = 10.0;
inline constexpr double SCROLL_BUTTON_MIN_W       = 128.0;
inline constexpr double REC_TOP_CLUSTER_W         = 292.0;
inline constexpr double REC_TOP_CLUSTER_H         = 56.0;
inline constexpr double REC_DECK_TOP_GAP          = 14.0;
// ── Top-center instruction bar ("Draw an area" frame) ───────────────────────
// Fixed to the top-center of the screen, NOT attached to the selection, so it
// never shifts while dragging. See CaptureOverlay_Layout.cpp.
inline constexpr double TOP_BAR_H                 = 44.0;
inline constexpr double TOP_BAR_Y                 = 16.0;
inline constexpr double TOP_BAR_RADIUS            = 14.0;
inline constexpr double TOP_BAR_ASPECT_H          = 28.0;
inline constexpr double TOP_BAR_BTN_SIZE          = 32.0;
inline constexpr int    SCROLL_CAPTURE_INTERVAL_MS = 300;
inline constexpr int    DEFAULT_SELECTION_W       = 600;
inline constexpr int    DEFAULT_SELECTION_H       = 744;
// Legacy side tools; Quick Capture handles Window through the in-overlay picker.
inline constexpr int    NUM_TOOLS                 = 6;

extern const char* TOOLBAR_LABELS[NUM_TOOLS];
extern const int TOOLBAR_ICON_IDS[NUM_TOOLS];

struct ToolbarLayout {
    QRectF leftToolsPanel;
    QRectF rightActionsPanel;
    QRectF topCluster;
    QRectF toolCells[NUM_TOOLS];
    QRectF sizeCard;
    QRectF cropCard;
    QRectF confirmCard;
    QRectF cancelCard;
    bool compactMode = false;
};

struct RecordingDeckLayout {
    QRectF leftToggleRail;
    QRectF topCluster;
    QRectF bottomActionBar;
    QRectF deckBounds;
    bool placedAbove = false;
};

// Top-center instruction bar layout. All rects are in overlay-local coords.
// aspectPills: 0=Free, 1=16:9, 2=4:3, 3=1:1
// buttons: 0=Crop (opens ratio dropdown), 1=Cancel
struct TopBarLayout {
    QRectF bar;
    QRectF labelRect;
    QRectF aspectPills[4];
    QRectF buttons[2];
    bool valid = false;
};

ToolbarLayout computeToolbarLayout(double selX, double selY,
                                   double selW, double selH,
                                   double screenW, double screenH,
                                   bool forceAbove = false);

RecordingDeckLayout computeRecordingDeckLayout(double selX, double selY,
                                               double selW, double selH,
                                               double screenW, double screenH);

TopBarLayout computeTopBarLayout(double screenW);
