#include "CaptureOverlay_p.h"

#include <algorithm>

ToolbarLayout computeToolbarLayout(double selX, double selY,
                                           double selW, double selH,
                                           double screenW, double screenH,
                                           bool forceAbove)
{
    ToolbarLayout layout;
    const double toolPanelH = TOOL_CARD_H * NUM_TOOLS;
    const double actionBarW = 0.0;
    const double actionBarH = 0.0;
    const double sizeCardW = 152.0;
    const double cropCardW = 62.0;
    const double topClusterH = REC_TOP_CLUSTER_H;
    const double topGroupW = sizeCardW + ACTION_CARD_GAP + cropCardW;
    const double centerY = selY + (selH / 2.0);

    const double leftCandidateX = selX - TOOL_RAIL_GAP - TOOL_RAIL_W;
    const bool toolRailClamped = leftCandidateX < FEATURE_PANEL_MARGIN;
    const double leftPanelX = std::max(FEATURE_PANEL_MARGIN, leftCandidateX);

    const double leftPanelY = std::max(
        FEATURE_PANEL_MARGIN,
        std::min(centerY - (toolPanelH / 2.0), screenH - toolPanelH - FEATURE_PANEL_MARGIN)
    );

    const double preferredActionX = selX + (selW - actionBarW) / 2.0;
    const double actionBarX = std::max(
        FEATURE_PANEL_MARGIN,
        std::min(preferredActionX, screenW - actionBarW - FEATURE_PANEL_MARGIN)
    );
    const double preferredActionY = selY + selH + FEATURE_PANEL_TOP_GAP;
    const bool actionBelowFits = (preferredActionY + actionBarH + FEATURE_PANEL_MARGIN) <= screenH;
    const double actionBarY = actionBelowFits
        ? preferredActionY
        : std::max(FEATURE_PANEL_MARGIN, screenH - actionBarH - FEATURE_PANEL_MARGIN);

    const double preferredTopX = selX + (selW - topGroupW) / 2.0;
    const double topGroupX = std::max(
        FEATURE_PANEL_MARGIN,
        std::min(preferredTopX, screenW - topGroupW - FEATURE_PANEL_MARGIN)
    );
    const double preferredSizeY = selY - FEATURE_PANEL_TOP_GAP - topClusterH;
    const bool sizeAboveFits = preferredSizeY >= FEATURE_PANEL_MARGIN;
    const double sizeCardY = sizeAboveFits
        ? preferredSizeY
        : FEATURE_PANEL_MARGIN;

    layout.compactMode = forceAbove || toolRailClamped || !actionBelowFits || !sizeAboveFits;

    layout.leftToolsPanel = QRectF(leftPanelX, leftPanelY, TOOL_RAIL_W, toolPanelH);
    layout.rightActionsPanel = QRectF();
    for (int i = 0; i < NUM_TOOLS; ++i) {
        layout.toolCells[i] = QRectF(
            layout.leftToolsPanel.x(),
            layout.leftToolsPanel.y() + (i * TOOL_CARD_H),
            TOOL_RAIL_W,
            TOOL_CARD_H
        );
    }

    layout.topCluster = QRectF(
        topGroupX,
        sizeCardY,
        topGroupW,
        topClusterH
    );
    layout.sizeCard = QRectF(
        topGroupX,
        sizeCardY,
        sizeCardW,
        topClusterH
    );
    layout.cropCard = QRectF(
        layout.sizeCard.right() + ACTION_CARD_GAP,
        sizeCardY,
        cropCardW,
        topClusterH
    );
    layout.confirmCard = QRectF();
    layout.cancelCard = QRectF();
    return layout;
}

RecordingDeckLayout computeRecordingDeckLayout(double selX, double selY,
                                               double selW, double selH,
                                               double screenW, double screenH)
{
    RecordingDeckLayout layout;
    const double railH = TOOL_CARD_H * 2.0;
    const double centerY = selY + (selH / 2.0);
    const double leftCandidateX = selX - TOOL_RAIL_GAP - TOOL_RAIL_W;
    const double leftPanelX = std::max(FEATURE_PANEL_MARGIN, leftCandidateX);
    const double leftPanelY = std::max(
        FEATURE_PANEL_MARGIN,
        std::min(centerY - (railH / 2.0), screenH - railH - FEATURE_PANEL_MARGIN)
    );

    const double preferredTopX = selX + (selW - REC_TOP_CLUSTER_W) / 2.0;
    const double topX = std::max(
        FEATURE_PANEL_MARGIN,
        std::min(preferredTopX, screenW - REC_TOP_CLUSTER_W - FEATURE_PANEL_MARGIN)
    );
    const double preferredTopY = selY - FEATURE_PANEL_TOP_GAP - REC_TOP_CLUSTER_H;
    const bool topFits = preferredTopY >= FEATURE_PANEL_MARGIN;
    const double topY = topFits ? preferredTopY : FEATURE_PANEL_MARGIN;

    const double actionBarW = (2.0 * ACTION_RAIL_W) + ACTION_CARD_GAP;
    const double preferredActionX = selX + (selW - actionBarW) / 2.0;
    const double actionX = std::max(
        FEATURE_PANEL_MARGIN,
        std::min(preferredActionX, screenW - actionBarW - FEATURE_PANEL_MARGIN)
    );
    const double preferredActionY = selY + selH + FEATURE_PANEL_TOP_GAP;
    const bool actionFits = (preferredActionY + ACTION_CARD_H + FEATURE_PANEL_MARGIN) <= screenH;
    const double actionY = actionFits
        ? preferredActionY
        : std::max(FEATURE_PANEL_MARGIN, screenH - ACTION_CARD_H - FEATURE_PANEL_MARGIN);

    layout.placedAbove = !actionFits;
    layout.leftToggleRail = QRectF(leftPanelX, leftPanelY, TOOL_RAIL_W, railH);
    layout.topCluster = QRectF(topX, topY, REC_TOP_CLUSTER_W, REC_TOP_CLUSTER_H);
    layout.bottomActionBar = QRectF(actionX, actionY, actionBarW, ACTION_CARD_H);
    layout.deckBounds = layout.leftToggleRail.united(layout.topCluster).united(layout.bottomActionBar);
    return layout;
}
