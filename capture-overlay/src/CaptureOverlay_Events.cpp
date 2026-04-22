#include "CaptureOverlay.h"
#include "CaptureOverlay_p.h"
#include <QMouseEvent>
#include <QKeyEvent>
#include <QApplication>
#include <QMessageBox>
#include <QDateTime>
#include <QPoint>
#include <QRect>
#include <QTimer>
#include <cmath>

namespace {
constexpr double kAspectRatios[] = {
    0.0,
    1.0,
    5.0 / 4.0,
    4.0 / 3.0,
    7.0 / 5.0,
    3.0 / 2.0,
    16.0 / 10.0,
    16.0 / 9.0,
    2.35,
    2.0 / 3.0,
    9.0 / 16.0,
};

constexpr int kAspectRatioCount =
    static_cast<int>(sizeof(kAspectRatios) / sizeof(kAspectRatios[0]));

double aspectRatioForIndex(int index)
{
    if (index < 0 || index >= kAspectRatioCount) {
        return 0.0;
    }
    return kAspectRatios[index];
}
}

static void showWebScrollCaptureInfo(QWidget* parent)
{
    QMessageBox messageBox(parent);
    messageBox.setWindowTitle(QStringLiteral("ApexShot - Webpage scroll capture"));
    messageBox.setIcon(QMessageBox::Information);
    messageBox.setText(QStringLiteral("Scroll capture is available on webpages through the browser extension."));
    messageBox.setInformativeText(QStringLiteral("Use the ApexShot browser extension on the page you want to capture. After the extension sends the capture to the app, it will open in the normal screenshot preview overlay."));
    messageBox.setStandardButtons(QMessageBox::Ok);
    messageBox.exec();
}
// ── Helper: convert key event to preview display text ──────────────────────────

static QString keyEventToPreviewText(QKeyEvent* event)
{
    int key = event->key();
    Qt::KeyboardModifiers mods = event->modifiers();

    // Modifier keys alone — skip
    if (key == Qt::Key_Shift || key == Qt::Key_Control || key == Qt::Key_Alt ||
        key == Qt::Key_Meta || key == Qt::Key_CapsLock || key == Qt::Key_unknown) {
        return {};
    }

    // Special keys
    switch (key) {
    case Qt::Key_Return:
    case Qt::Key_Enter:  return "\u21A9";  // ↩
    case Qt::Key_Backspace: return "\u232B"; // ⌫
    case Qt::Key_Delete:    return "\u2326"; // ⌦
    case Qt::Key_Tab:       return "\u21E5"; // ⇥
    case Qt::Key_Space:     return " ";
    case Qt::Key_Escape:    return "Esc";
    case Qt::Key_Up:        return "\u2191"; // ↑
    case Qt::Key_Down:      return "\u2193"; // ↓
    case Qt::Key_Left:      return "\u2190"; // ←
    case Qt::Key_Right:     return "\u2192"; // →
    default: break;
    }

    // Build string with modifier prefixes
    QString result;
    if (mods & Qt::ControlModifier) result += "\u2318 ";  // ⌘
    if (mods & Qt::AltModifier)     result += "\u2325 ";  // ⌥
    if (mods & Qt::ShiftModifier)   result += "\u21E7 ";  // ⇧

    // Key text
    QString text = event->text();
    if (!text.isEmpty() && text.at(0).isPrint()) {
        result += text.toUpper();
    } else {
        // Named key
        QString name = QKeySequence(key).toString();
        if (!name.isEmpty()) result += name;
    }

    return result.trimmed();
}

void CaptureOverlay::mousePressEvent(QMouseEvent* event)
{
    const QPoint pos = event->pos();
    m_pointerPos = pos;

    if (isCrosshairMode()) {
        if (event->button() == Qt::LeftButton) {
            const QPoint oldPoint = m_lastCrosshairPaintPoint;
            const QRect oldSelection = m_lastCrosshairSelectionRect;
            const bool oldHadSelection = m_lastCrosshairHadSelection;

            m_dragStart = pos;
            m_selection = QRect(pos, pos);
            m_hasSelection = false;
            m_dragging = true;
            m_moving = false;
            m_resizing = HandlePos::None;

            if (m_lastCursorShape != Qt::CrossCursor) {
                setCursor(Qt::CrossCursor);
                m_lastCursorShape = Qt::CrossCursor;
            }

            const QRect newSelection = m_selection.normalized();
            const QRegion dirty = crosshairDirtyRegion(
                oldPoint,
                pos,
                oldSelection,
                newSelection,
                oldHadSelection,
                true);

            m_lastCrosshairPaintPoint = pos;
            m_lastCrosshairSelectionRect = newSelection;
            m_lastCrosshairHadSelection = true;
            m_lastCrosshairBubbleRect = crosshairBubbleRectForPoint(pos);
            update(dirty);
        }
        return;
    }

    auto closeRecordingMenus = [&]() {
        m_settingsOpen = false;
        m_clickOptionsOpen = false;
        m_keystrokeOptionsOpen = false;
        m_cropMenuOpen = false;
        m_hoveredCropMenuItem = -1;
        stopClickAnimTimer();
        m_clickPreviews.clear();
        m_showKeystrokePreview = false;
        m_keyPreviews.clear();
        m_dropdownOpen = -1;
        m_dropdownAnchor = QRectF();
        m_dropdownOptions.clear();
        m_dropdownColors.clear();
        m_dropdownValuePtr = nullptr;
        m_hoveredDropdownItem = -1;
        m_dropdownItemRects.clear();
    };
    auto closeCaptureCropMenu = [&]() {
        m_captureCropMenuOpen = false;
        m_hoveredCaptureCropMenuItem = -1;
        m_captureCropMenuPanelRect = QRectF();
        m_captureCropMenuItemRects.clear();
    };
    auto applyCurrentRecordingAspect = [&]() {
        const double ratio = aspectRatioForIndex(m_recordAspectRatioIndex);
        if (ratio <= 0.0 || !m_hasSelection) {
            return;
        }

        const QRect bounds = rect();
        QRect sel = m_selection.normalized();
        double newW = sel.width();
        double newH = newW / ratio;
        if (newH > sel.height()) {
            newH = sel.height();
            newW = newH * ratio;
        }

        newW = std::max<double>(kMinSize, std::min<double>(newW, bounds.width()));
        newH = std::max<double>(kMinSize, std::min<double>(newH, bounds.height()));
        if (newW / ratio > bounds.height()) {
            newH = bounds.height();
            newW = newH * ratio;
        }
        if (newH * ratio > bounds.width()) {
            newW = bounds.width();
            newH = newW / ratio;
        }

        const QPoint center = sel.center();
        int x = center.x() - static_cast<int>(std::round(newW / 2.0));
        int y = center.y() - static_cast<int>(std::round(newH / 2.0));
        int w = std::max(kMinSize, static_cast<int>(std::round(newW)));
        int h = std::max(kMinSize, static_cast<int>(std::round(newH)));

        x = std::max(0, std::min(x, bounds.width() - w));
        y = std::max(0, std::min(y, bounds.height() - h));
        m_selection = QRect(x, y, w, h);
        m_hasSelection = true;
    };
    auto applyCurrentCaptureAspect = [&]() {
        const double ratio = aspectRatioForIndex(m_captureAspectRatioIndex);
        if (ratio <= 0.0 || !m_hasSelection) {
            return;
        }

        const QRect bounds = rect();
        QRect sel = m_selection.normalized();
        double newW = sel.width();
        double newH = newW / ratio;
        if (newH > sel.height()) {
            newH = sel.height();
            newW = newH * ratio;
        }

        newW = std::max<double>(kMinSize, std::min<double>(newW, bounds.width()));
        newH = std::max<double>(kMinSize, std::min<double>(newH, bounds.height()));
        if (newW / ratio > bounds.height()) {
            newH = bounds.height();
            newW = newH * ratio;
        }
        if (newH * ratio > bounds.width()) {
            newW = bounds.width();
            newH = newW / ratio;
        }

        const QPoint center = sel.center();
        int x = center.x() - static_cast<int>(std::round(newW / 2.0));
        int y = center.y() - static_cast<int>(std::round(newH / 2.0));
        int w = std::max(kMinSize, static_cast<int>(std::round(newW)));
        int h = std::max(kMinSize, static_cast<int>(std::round(newH)));

        x = std::max(0, std::min(x, bounds.width() - w));
        y = std::max(0, std::min(y, bounds.height() - h));
        m_selection = QRect(x, y, w, h);
        m_hasSelection = true;
    };

    if (m_countdownActive) {
        if (event->button() == Qt::LeftButton && m_countdownBubbleRect.contains(pos)) {
            m_countdownCancelRequested = true;
            // Trigger immediate cancel processing in the timer callback
            m_countdownTimer->stop();
            onCountdownTick();
            return;
        }
        // Allow moving/resizing the selection during countdown
        if (event->button() == Qt::LeftButton && m_hasSelection) {
            HandlePos h = hitTest(pos);
            if (h == HandlePos::Inside) {
                m_moving = true;
                m_selectionAtDragStart = m_selection.normalized();
                m_dragStart = pos;
                setCursor(Qt::ClosedHandCursor);
                return;
            } else if (h != HandlePos::None) {
                m_resizing = h;
                m_selectionAtDragStart = m_selection.normalized();
                m_dragStart = pos;
                return;
            }
        }
        // Ignore other clicks during countdown (don't start new selections)
        return;
    }

    // Right-click on webcam tile shows context menu
    if (event->button() == Qt::RightButton && m_recordingPanelOpen) {
        std::fprintf(stderr, "[mousePressEvent] Right-click detected, m_recordingPanelOpen=true\n");
        RecordPanelTile tile = hitTestRecordingPanel(pos);
        std::fprintf(stderr, "[mousePressEvent] hitTest returned tile=%d (Webcam=%d)\n", 
                     (int)tile, (int)RecordPanelTile::Webcam);
        if (tile == RecordPanelTile::Webcam) {
            std::fprintf(stderr, "[mousePressEvent] Showing webcam context menu\n");
            const QRect sel = m_selection.normalized();
            const double contextualX = std::max(10.0, std::min(sel.x() + (sel.width() - 440.0) / 2.0, width() - 450.0));
            const double contextualY = std::max(10.0, std::min(sel.y() + 24.0, height() - 510.0));
            closeRecordingMenus();
            showWebcamContextMenu(mapToGlobal(QPoint((int)contextualX, (int)contextualY)));
            return;
        }
    }

    if (event->button() != Qt::LeftButton) return;

    // ── Global Dropdown Logic ───────────────────────────────────────────────
    if (m_dropdownOpen != -1) {
        for (int i = 0; i < m_dropdownItemRects.size(); ++i) {
            if (m_dropdownItemRects[i].contains(pos)) {
                if (m_dropdownValuePtr) *m_dropdownValuePtr = i;
                m_dropdownOpen = -1;
                m_dropdownColors.clear();
                m_hoveredDropdownItem = -1;
                update();
                return;
            }
        }
        // Click outside dropdown — close it
        m_dropdownOpen = -1;
        m_dropdownColors.clear();
        m_hoveredDropdownItem = -1;
        update();
        return;
    }

    if (m_captureCropMenuOpen) {
        for (int i = 0; i < m_captureCropMenuItemRects.size(); ++i) {
            if (m_captureCropMenuItemRects[i].contains(pos)) {
                m_captureAspectRatioIndex = i;
                closeCaptureCropMenu();
                applyCurrentCaptureAspect();
                update();
                return;
            }
        }
        closeCaptureCropMenu();
        update();
    }

    if (m_cropMenuOpen) {
        for (int i = 0; i < m_cropMenuItemRects.size(); ++i) {
            if (m_cropMenuItemRects[i].contains(pos)) {
                m_recordAspectRatioIndex = i;
                m_cropMenuOpen = false;
                m_hoveredCropMenuItem = -1;
                applyCurrentRecordingAspect();
                update();
                return;
            }
        }
        m_cropMenuOpen = false;
        m_hoveredCropMenuItem = -1;
        update();
    }

    // Keystroke Options sub-panel clicks
    if (m_keystrokeOptionsOpen && m_keystrokeOptionsPanelRect.contains(pos)) {
        for (int i = 0; i < m_keystrokeOptionsClickableRects.size(); ++i) {
            if (m_keystrokeOptionsClickableRects[i].contains(pos)) {
                switch (i) {
                case 0: { // Slider — start drag
                    double relX = pos.x() - m_keystrokeOptionsClickableRects[i].x();
                    m_keySize = std::max(0.0, std::min(1.0, relX / m_keystrokeOptionsClickableRects[i].width()));
                    m_keySliderDragging = true;
                    break;
                }
                case 1: // Position dropdown
                    m_dropdownOpen = i;
                    m_dropdownAnchor = m_keystrokeOptionsClickableRects[i];
                    m_dropdownOptions = QStringList() << "Bottom-Center" << "Bottom-Left" << "Bottom-Right" 
                                                    << "Top-Center" << "Top-Left" << "Top-Right";
                    m_dropdownValuePtr = &m_keyPosition;
                    break;
                case 2: // Appearance dropdown
                    m_dropdownOpen = i;
                    m_dropdownAnchor = m_keystrokeOptionsClickableRects[i];
                    m_dropdownOptions = QStringList() << "Dark" << "Light";
                    m_dropdownValuePtr = &m_keyAppearance;
                    break;
                case 3: m_keyBlurBg = !m_keyBlurBg; break;
                case 4: m_keyFilter = 0; break;
                case 5: m_keyFilter = 1; break;
                case 6: m_showKeystrokePreview = !m_showKeystrokePreview; break;
                case 7: 
                    m_keystrokeOptionsOpen = false; 
                    m_showKeystrokePreview = false;
                    m_keyPreviews.clear();
                    break; // OK
                }
                update();
                return;
            }
        }
        return;
    }

    // Click Options sub-panel clicks
    if (m_clickOptionsOpen && m_clickOptionsPanelRect.contains(pos)) {
        for (int i = 0; i < m_clickOptionsClickableRects.size(); ++i) {
            if (m_clickOptionsClickableRects[i].contains(pos)) {
                switch (i) {
                case 0: { // Slider — start drag
                    double relX = pos.x() - m_clickOptionsClickableRects[i].x();
                    m_clickSize = std::max(0.0, std::min(1.0, relX / m_clickOptionsClickableRects[i].width()));
                    m_sliderDragging = true;
                    break;
                }
                case 1: // Color dropdown
                    m_dropdownOpen = i;
                    m_dropdownAnchor = m_clickOptionsClickableRects[i];
                    m_dropdownOptions = QStringList()
                        << "Gray" << "Indigo" << "Red" << "Blue" << "Green"
                        << "Yellow" << "Orange" << "Purple" << "White";
                    m_dropdownColors = QList<QColor>()
                        << QColor(180, 180, 180) << QColor(122, 100, 255) << QColor(255, 60, 60)
                        << QColor(60, 120, 255) << QColor(60, 200, 80) << QColor(255, 210, 50)
                        << QColor(255, 150, 30) << QColor(180, 60, 220) << QColor(255, 255, 255);
                    m_dropdownValuePtr = &m_clickColor;
                    break;
                case 2: // Style dropdown
                    m_dropdownOpen = i;
                    m_dropdownAnchor = m_clickOptionsClickableRects[i];
                    m_dropdownOptions = QStringList() << "Outline" << "Filled";
                    m_dropdownValuePtr = &m_clickStyle;
                    break;
                case 3: { // Animate toggle
                    m_clickAnimate = !m_clickAnimate;
                    if (m_clickAnimate && !m_clickPreviews.isEmpty()) {
                        startClickAnimTimer();
                    } else {
                        stopClickAnimTimer();
                    }
                    break;
                }
                case 4: { // Preview — add click point
                    qint64 now = QDateTime::currentMSecsSinceEpoch();
                    m_clickPreviews.append({pos, now});
                    if (m_clickPreviews.size() > 10) m_clickPreviews.removeFirst();
                    startClickAnimTimer();
                    break;
                }
                case 5: { // OK — close panel
                    m_clickOptionsOpen = false;
                    stopClickAnimTimer();
                    m_clickPreviews.clear();
                    break;
                }
                }
                update();
                return;
            }
        }
        return;
    }

    // Settings menu clicks
    if (m_settingsOpen) {
        if (m_settingsPanelRect.contains(pos)) {
            // Check in reverse order so the latest clickable rects win when rows overlap.
            for (int i = static_cast<int>(m_settingsClickableRects.size()) - 1; i >= 0; --i) {
                if (m_settingsClickableRects[i].contains(pos)) {
                    if (i < 3) { // Tab clicks (indices 0, 1, 2)
                        m_settingsTab = i;
                        m_dropdownOpen = -1;
                        m_dropdownColors.clear();
                        update();
                        return;
                    }
                    
                    if (m_settingsTab == 0) { // General tab logic
                        switch (i) {
                        case 3: m_recControls = !m_recControls; break;
                        case 4: m_displayRecTime = !m_displayRecTime; break;
                        case 5: m_hidpi = !m_hidpi; break;
                        case 6: m_doNotDisturb = !m_doNotDisturb; break;
                        case 7: m_showCursor = !m_showCursor; break;
                        case 8:
                            m_recClicks = !m_recClicks;
                            if (!m_recClicks) {
                                m_clickOptionsOpen = false;
                                stopClickAnimTimer();
                                m_clickPreviews.clear();
                            }
                            break;
                        case 9:
                            m_recKeystrokes = !m_recKeystrokes;
                            if (!m_recKeystrokes) {
                                m_keystrokeOptionsOpen = false;
                                m_showKeystrokePreview = false;
                                m_keyPreviews.clear();
                            }
                            break;
                        case 10: m_rememberSelection = !m_rememberSelection; break;
                        case 11: m_dimScreen = !m_dimScreen; break;
                        case 12: m_showCountdown = !m_showCountdown; break;
                        }
                        update();
                        return;
                    } else if (m_settingsTab == 1) { // Video tab logic
                        switch (i) {
                        case 3: // Max Resolution
                            m_dropdownOpen = i;
                            m_dropdownAnchor = m_settingsClickableRects[i];
                            m_dropdownOptions = QStringList() << "Original" << "1080p" << "720p";
                            m_dropdownValuePtr = &m_videoMaxRes;
                            break;
                        case 4: // Video FPS
                            m_dropdownOpen = i;
                            m_dropdownAnchor = m_settingsClickableRects[i];
                            m_dropdownOptions = QStringList() << "24" << "30" << "50" << "60";
                            m_dropdownValuePtr = &m_videoFps;
                            break;
                        case 5: m_recordMono = !m_recordMono; break;
                        case 6: m_openEditor = !m_openEditor; break;
                        }
                        update();
                        return;
                    } else if (m_settingsTab == 2) { // GIF tab logic
                        switch (i) {
                        case 3: { // FPS Slider
                            double relX = pos.x() - m_settingsClickableRects[i].x();
                            m_gifFps = 5 + (int)(55.0 * std::max(0.0, std::min(1.0, relX / m_settingsClickableRects[i].width())));
                            m_gifFpsDragging = true;
                            break;
                        }
                        case 4: { // Quality Slider
                            double relX = pos.x() - m_settingsClickableRects[i].x();
                            m_gifQuality = std::max(0.0, std::min(1.0, relX / m_settingsClickableRects[i].width()));
                            m_gifQualityDragging = true;
                            break;
                        }
                        case 5: m_optimizeGif = !m_optimizeGif; break;
                        case 6: // GIF Size dropdown
                            m_dropdownOpen = i;
                            m_dropdownAnchor = m_settingsClickableRects[i];
                            m_dropdownOptions = QStringList() << "800 x auto (default)" << "640 x auto" << "480 x auto" << "Original";
                            m_dropdownValuePtr = &m_gifSizeIdx;
                            break;
                        }
                        update();
                        return;
                    }
                }
            }
            return; // Click inside panel but no hit
        } else {
            // Clicked outside settings panel - check if it's the Controls tile
            RecordPanelTile tile = hitTestRecordingPanel(pos);
            if (tile != RecordPanelTile::Controls) {
                m_settingsOpen = false;
                m_clickOptionsOpen = false;
                stopClickAnimTimer();
                m_clickPreviews.clear();
                update();
                // continue to handle the click normally
            }
        }
    }

    // Window mode — click selects the hovered window
    if (m_windowMode) {
        if (m_hoveredWindow >= 0 && m_hoveredWindow < m_windows.size()) {
            m_selection = m_windows[m_hoveredWindow].rect;
            m_hasSelection = true;
            exitWindowMode();
            confirmSelection();
        }
        return;
    }

    // Recording panel tile clicks
    if (m_recordingPanelOpen) {
        RecordPanelTile tile = hitTestRecordingPanel(pos);
        switch (tile) {
        case RecordPanelTile::Controls:
        {
            const bool willOpen = !m_settingsOpen;
            closeRecordingMenus();
            m_settingsOpen = willOpen;
            update();
            return;
        }
        case RecordPanelTile::Crop:
        {
            const bool willOpen = !m_cropMenuOpen;
            closeRecordingMenus();
            m_cropMenuOpen = willOpen;
            update();
            return;
        }
        case RecordPanelTile::Mic:
            m_recMic = !m_recMic;
            update();
            return;
        case RecordPanelTile::Speaker:
            m_recSpeaker = !m_recSpeaker;
            update();
            return;
        case RecordPanelTile::Click:
            if (m_recClicks) {
                closeRecordingMenus();
                m_recClicks = false;
                stopClickAnimTimer();
                m_clickPreviews.clear();
            } else {
                closeRecordingMenus();
                m_recClicks = true;
                m_clickOptionsOpen = true;
            }
            update();
            return;
        case RecordPanelTile::Keystrokes:
            if (m_recKeystrokes) {
                closeRecordingMenus();
                m_recKeystrokes = false;
                m_showKeystrokePreview = false;
                m_keyPreviews.clear();
            } else {
                closeRecordingMenus();
                m_recKeystrokes = true;
                m_keystrokeOptionsOpen = true;
            }
            update();
            return;
        case RecordPanelTile::Webcam:
            m_recWebcam = !m_recWebcam;
            if (!m_recWebcam) {
                stopWebcamCapture();
            } else if (m_recordingPanelOpen && m_webcamDevice >= 0) {
                startWebcamCapture();
            }
            update();
            return;
        case RecordPanelTile::RecordVideo:
            m_recordType = RecordType::Video;
            m_captureIntent = CaptureIntent::Record;
            confirmRecordingSelection();
            return;
        case RecordPanelTile::RecordGif:
            m_recordType = RecordType::Gif;
            m_captureIntent = CaptureIntent::Record;
            confirmRecordingSelection();
            return;
        default:
            break;
        }
        if (tile == RecordPanelTile::None && m_recWebcam && m_hasSelection) {
            const QRect sel = m_selection.normalized();
            const QRectF previewRect = webcamPreviewRect(
                sel.x(), sel.y(), sel.width(), sel.height());
            if (previewRect.contains(pos)) {
                m_draggingWebcam = true;
                m_webcamDragOffset = QPointF(pos) - previewRect.topLeft();
                setCursor(Qt::SizeAllCursor);
                return;
            }
        }
        // If click is on resize handle, allow it
        HandlePos h = hitTest(pos);
        if (h != HandlePos::None) {
            // Pass through to resize handling below
        } else {
            return; // Click was on panel background, don't start drag
        }
    }

    if (m_captureIntent == CaptureIntent::Scroll && handleScrollButtonClick(pos)) {
        return;
    }
    if (m_captureIntent == CaptureIntent::Scroll && m_scrollStage == ScrollStage::Capturing) {
        return;
    }

    // Check toolbar click — only confirm if click is within or on the selection
    // (not when clicking outside, which should start a fresh selection)
    if (m_hasSelection) {
        const QRect sel = m_selection.normalized();
        ToolbarLayout layout = computeToolbarLayout(
            sel.x(),
            sel.y(),
            sel.width(),
            sel.height(),
            width(),
            height(),
            m_captureIntent == CaptureIntent::Scroll
        );

        // Only allow toolbar clicks when the click point is NOT outside the
        // selection area — otherwise the user is starting a new selection and
        // accidentally grazing the toolbar should not confirm anything.
        HandlePos h = hitTest(pos);
        bool clickInsideOrOnSelection = (h != HandlePos::None);

        // Helper lambda to handle toolbar tool click
        auto handleToolClick = [&](int toolIndex) -> bool {
            if (toolIndex == 1) {
                // Fullscreen: expand selection to cover entire screen, wait for Enter
                closeCaptureCropMenu();
                exitScrollMode();
                exitWindowMode();
                m_selection = QRect(0, 0, width(), height());
                m_hasSelection = true;
                m_fullscreenMode = true;
                m_captureIntent = CaptureIntent::Area;
                update();
                return true;
            } else if (toolIndex == 0) {
                // Area: restore default centered area selection
                closeCaptureCropMenu();
                exitScrollMode();
                exitWindowMode();
                int defaultW = std::max(kMinSize, std::min(DEFAULT_SELECTION_W, width()));
                int defaultH = std::max(kMinSize, std::min(DEFAULT_SELECTION_H, height()));
                int defaultX = (width() - defaultW) / 2;
                int defaultY = (height() - defaultH) / 2;
                m_selection = QRect(defaultX, defaultY, defaultW, defaultH);
                m_hasSelection = true;
                m_fullscreenMode = false;
                m_timerDelayActive = false;
                m_captureAspectRatioIndex = 0;
                m_captureIntent = CaptureIntent::Area;
                update();
                return true;
            } else if (toolIndex == 2) {
                // Window: on Wayland use GNOME DBus (exit code 3),
                // on X11 use hover-select mode
                closeCaptureCropMenu();
                exitScrollMode();
                std::fprintf(stderr, "[CaptureOverlay] Window tool clicked (index 3)\n");
                std::fprintf(stderr, "[CaptureOverlay] WAYLAND_DISPLAY=%s\n",
                    qgetenv("WAYLAND_DISPLAY").constData());
                if (qEnvironmentVariableIsSet("WAYLAND_DISPLAY")) {
                    std::fprintf(stderr, "[CaptureOverlay] Exiting with code 3 for window capture\n");
                    releaseKeyboard();
                    hide();
                    QApplication::exit(3); // special code: window capture via DBus
                } else {
                    std::fprintf(stderr, "[CaptureOverlay] Entering X11 window mode\n");
                    m_captureIntent = CaptureIntent::Area;
                    enterWindowMode();
                }
                return true;
            } else if (toolIndex == 3) {
                closeCaptureCropMenu();
                exitScrollMode();
                m_captureIntent = CaptureIntent::Area;
                update();
                showWebScrollCaptureInfo(this);
                return true;
            } else if (toolIndex == 4) {
                closeCaptureCropMenu();
                if (!m_timerCaptureEnabled) {
                    m_timerCaptureEnabled = true;
                }
                if (!m_timerDelayActive) {
                    m_timerDelayActive = true;
                    if (m_captureDelaySeconds <= 0) {
                        m_captureDelaySeconds = 5;
                    }
                    update();
                } else {
                    cycleCaptureDelay();
                }
                return true;
            } else if (toolIndex == 5) {
                closeCaptureCropMenu();
                exitScrollMode();
                m_captureIntent = CaptureIntent::Ocr;
                update();
                return true;
            } else if (toolIndex == 6) {
                // Recording: open recording panel
                closeCaptureCropMenu();
                exitScrollMode();
                m_captureIntent = CaptureIntent::Record;
                m_recordingPanelOpen = true;
                m_recordingToolsHidden = false;
                if (m_recWebcam && m_webcamDevice >= 0)
                    startWebcamCapture();
                update();
                return true;
            } else {
                closeCaptureCropMenu();
                exitScrollMode();
                m_captureIntent = CaptureIntent::Area;
                confirmSelection();
                return true;
            }
        };

        auto handleActionClick = [&](CaptureOverlay::ToolbarActionCard action) -> bool {
            switch (action) {
            case ToolbarActionCard::Confirm:
                if (m_captureIntent == CaptureIntent::Record) {
                    confirmRecordingSelection();
                } else {
                    confirmSelection();
                }
                return true;
            case ToolbarActionCard::Cancel:
                cancelSelection();
                return true;
            case ToolbarActionCard::None:
                return false;
            }
            return false;
        };

        if (clickInsideOrOnSelection) {
            for (int i = 0; i < NUM_TOOLS; ++i) {
                if (layout.toolCells[i].contains(pos)) {
                    std::fprintf(stderr, "[CaptureOverlay] Tool clicked (inside): index=%d\n", i);
                    handleToolClick(i);
                    return;
                }
            }
            if (layout.cropCard.contains(pos)) {
                const bool wasOpen = m_captureCropMenuOpen;
                closeCaptureCropMenu();
                m_captureCropMenuOpen = !wasOpen;
                update();
                return;
            }
        } else {
            bool clickedToolbar = layout.leftToolsPanel.contains(pos) ||
                                  layout.sizeCard.contains(pos);
            if (clickedToolbar) {
                for (int i = 0; i < NUM_TOOLS; ++i) {
                    if (layout.toolCells[i].contains(pos)) {
                        handleToolClick(i);
                        return;
                    }
                }
                if (layout.cropCard.contains(pos)) {
                    const bool wasOpen = m_captureCropMenuOpen;
                    closeCaptureCropMenu();
                    m_captureCropMenuOpen = !wasOpen;
                    update();
                    return;
                }
                // Clicked toolbar panel background but not a specific tool —
                // do nothing (don't start a new selection from here).
                return;
            }

            // Click is outside selection AND outside toolbar — start fresh selection.
            m_dragging = true;
            m_moving = false;
            m_resizing = HandlePos::None;
            m_hasSelection = false;
            closeCaptureCropMenu();
            m_selection = QRect(pos, pos);
            m_dragStart = pos;
            setCursor(Qt::CrossCursor);
            update();
            return;
        }
    }

    m_dragStart = pos;

    if (m_captureIntent == CaptureIntent::Scroll && m_scrollStage == ScrollStage::Capturing) {
        return;
    }

    if (m_hasSelection) {
        HandlePos h = hitTest(pos);
        if (h == HandlePos::Inside) {
            m_moving = true;
            m_selectionAtDragStart = m_selection.normalized();
            setCursor(Qt::ClosedHandCursor);
            return;
        } else if (h != HandlePos::None) {
            m_resizing = h;
            m_selectionAtDragStart = m_selection.normalized();
            return;
        }
    }

    m_dragging = true;
    m_hasSelection = false;
    m_fullscreenMode = false;
    m_selection = QRect(pos, pos);
    setCursor(Qt::CrossCursor);
}

void CaptureOverlay::mouseMoveEvent(QMouseEvent* event)
{
    const QPoint pos = event->pos();
    m_pointerPos = pos;

    if (isCrosshairMode()) {
        const QPoint oldPoint = m_lastCrosshairPaintPoint;
        const QRect oldSelection = m_lastCrosshairSelectionRect;
        const bool oldHadSelection = m_lastCrosshairHadSelection;

        if (m_dragging) {
            m_selection = QRect(m_dragStart, pos);
        }

        if (m_lastCursorShape != Qt::CrossCursor) {
            setCursor(Qt::CrossCursor);
            m_lastCursorShape = Qt::CrossCursor;
        }

        const QRect newSelection = (m_dragging || m_hasSelection) ? m_selection.normalized() : QRect();
        const bool newHadSelection = m_dragging || m_hasSelection;
        const QRegion dirty = crosshairDirtyRegion(
            oldPoint,
            pos,
            oldSelection,
            newSelection,
            oldHadSelection,
            newHadSelection);

        m_lastCrosshairPaintPoint = pos;
        m_lastCrosshairSelectionRect = newSelection;
        m_lastCrosshairHadSelection = newHadSelection;
        m_lastCrosshairBubbleRect = crosshairBubbleRectForPoint(pos);
        update(dirty);
        return;
    }

    if (m_countdownActive) {
        const bool hoveringCancel = m_countdownBubbleRect.contains(pos);
        if (hoveringCancel != m_hoveredCountdownCancel) {
            m_hoveredCountdownCancel = hoveringCancel;
            update();
        }
        // Don't return — fall through to allow selection drag/move/resize during countdown.
        // Cursor is set at the end of this function.
    }

    // ── Slider Drag ─────────────────────────────────────────────────────────
    if (m_sliderDragging) {
        double relX = pos.x() - m_sliderTrackRect.x();
        m_clickSize = std::max(0.0, std::min(1.0, relX / m_sliderTrackRect.width()));
        update();
        return;
    }
    if (m_keySliderDragging) {
        double relX = pos.x() - m_keySliderTrackRect.x();
        m_keySize = std::max(0.0, std::min(1.0, relX / m_keySliderTrackRect.width()));
        update();
        return;
    }
    if (m_gifFpsDragging) {
        double relX = pos.x() - m_gifFpsTrackRect.x();
        m_gifFps = 5 + (int)(55.0 * std::max(0.0, std::min(1.0, relX / m_gifFpsTrackRect.width())));
        update();
        return;
    }
    if (m_gifQualityDragging) {
        double relX = pos.x() - m_gifQualityTrackRect.x();
        m_gifQuality = std::max(0.0, std::min(1.0, relX / m_gifQualityTrackRect.width()));
        update();
        return;
    }
    if (m_draggingWebcam && m_hasSelection) {
        const QRect sel = m_selection.normalized();
        setWebcamPreviewTopLeft(QPointF(pos) - m_webcamDragOffset,
                                sel.x(), sel.y(), sel.width(), sel.height());
        update();
        return;
    }

    // ── Global Dropdown Hover ───────────────────────────────────────────────
    if (m_dropdownOpen != -1) {
        int newHover = -1;
        for (int i = 0; i < m_dropdownItemRects.size(); ++i) {
            if (m_dropdownItemRects[i].contains(pos)) {
                newHover = i;
                break;
            }
        }
        if (newHover != m_hoveredDropdownItem) {
            m_hoveredDropdownItem = newHover;
            update();
        }
        setCursor(Qt::PointingHandCursor);
        return;
    }

    // Window mode — highlight the window under the cursor
    if (m_windowMode) {
        int newHover = -1;
        for (int i = 0; i < m_windows.size(); ++i) {
            if (m_windows[i].rect.contains(pos)) {
                newHover = i;
                break; // first (topmost) match wins
            }
        }
        if (newHover != m_hoveredWindow) {
            m_hoveredWindow = newHover;
            update();
        }
        return;
    }

    if (!m_recordingPanelOpen && m_captureCropMenuOpen) {
        int newHover = -1;
        for (int i = 0; i < m_captureCropMenuItemRects.size(); ++i) {
            if (m_captureCropMenuItemRects[i].contains(pos)) {
                newHover = i;
                break;
            }
        }
        if (newHover != m_hoveredCaptureCropMenuItem) {
            m_hoveredCaptureCropMenuItem = newHover;
            update();
        }
        if (newHover != -1) {
            setCursor(Qt::PointingHandCursor);
            return;
        }
    }

    // Recording panel hover
    if (m_recordingPanelOpen && !m_dragging && m_resizing == HandlePos::None && !m_moving) {
        if (m_cropMenuOpen && m_cropMenuPanelRect.contains(pos)) {
            int newHover = -1;
            for (int i = 0; i < m_cropMenuItemRects.size(); ++i) {
                if (m_cropMenuItemRects[i].contains(pos)) {
                    newHover = i;
                    break;
                }
            }
            if (newHover != m_hoveredCropMenuItem) {
                m_hoveredCropMenuItem = newHover;
                update();
            }
            setCursor(Qt::PointingHandCursor);
            return;
        }
        if (m_cropMenuOpen && m_hoveredCropMenuItem != -1) {
            m_hoveredCropMenuItem = -1;
            update();
        }

        // Click Options sub-panel hover
        if (m_clickOptionsOpen && m_clickOptionsPanelRect.contains(pos)) {
            int newHover = -1;
            for (int i = static_cast<int>(m_clickOptionsClickableRects.size()) - 1; i >= 0; --i) {
                if (m_clickOptionsClickableRects[i].contains(pos)) {
                    newHover = i;
                    break;
                }
            }
            setCursor(Qt::PointingHandCursor);
            return;
        }

        // Keystroke Options sub-panel hover
        if (m_keystrokeOptionsOpen && m_keystrokeOptionsPanelRect.contains(pos)) {
            int newHover = -1;
            for (int i = static_cast<int>(m_keystrokeOptionsClickableRects.size()) - 1; i >= 0; --i) {
                if (m_keystrokeOptionsClickableRects[i].contains(pos)) {
                    newHover = i;
                    break;
                }
            }
            setCursor(Qt::PointingHandCursor);
            return;
        }

        // Settings menu hover
        if (m_settingsOpen && m_settingsPanelRect.contains(pos)) {
            int newHover = -1;
            for (int i = static_cast<int>(m_settingsClickableRects.size()) - 1; i >= 0; --i) {
                if (m_settingsClickableRects[i].contains(pos)) {
                    newHover = i;
                    break;
                }
            }
            if (newHover != m_hoveredSettingsItem) {
                m_hoveredSettingsItem = newHover;
                update();
            }
            setCursor(Qt::PointingHandCursor);
            return;
        }

        RecordPanelTile newTile = hitTestRecordingPanel(pos);
        if (newTile != m_hoveredRecordTile) {
            m_hoveredRecordTile = newTile;
            update();
        }
        if (newTile != RecordPanelTile::None) {
            updateCursor(pos);
            return;
        }
        if (m_recWebcam && m_hasSelection) {
            const QRect sel = m_selection.normalized();
            const QRectF previewRect = webcamPreviewRect(
                sel.x(), sel.y(), sel.width(), sel.height());
            if (previewRect.contains(pos)) {
                setCursor(Qt::SizeAllCursor);
                return;
            }
        }
        updateCursor(pos);
        return;
    }

    if (m_dragging) {
        m_selection = QRect(m_dragStart, pos);
        m_hasSelection = true;
        update();
        return;
    }

    if (m_moving) {
        setCursor(Qt::ClosedHandCursor);
        QPoint delta = pos - m_dragStart;
        QRect newSel = m_selectionAtDragStart.translated(delta);
        const QRect bounds = rect();
        if (newSel.left() < bounds.left())     newSel.moveLeft(bounds.left());
        if (newSel.top()  < bounds.top())      newSel.moveTop(bounds.top());
        if (newSel.right()  > bounds.right())  newSel.moveRight(bounds.right());
        if (newSel.bottom() > bounds.bottom()) newSel.moveBottom(bounds.bottom());
        m_selection = newSel;
        update();
        return;
    }

    if (m_resizing != HandlePos::None) {
        QPoint delta = pos - m_dragStart;
        QRect r = m_selectionAtDragStart.normalized();
        double aspectRatio = 0.0;
        if (m_recordingPanelOpen && m_recordAspectRatioIndex > 0) {
            aspectRatio = aspectRatioForIndex(m_recordAspectRatioIndex);
        } else if (!m_recordingPanelOpen && m_captureAspectRatioIndex > 0) {
            aspectRatio = aspectRatioForIndex(m_captureAspectRatioIndex);
        }

        if (aspectRatio > 0.0) {
            const QRect bounds = rect();
            const int minW = std::max(kMinSize, static_cast<int>(std::ceil(kMinSize * aspectRatio)));
            const int minH = std::max(kMinSize, static_cast<int>(std::ceil(kMinSize / aspectRatio)));
            const int left = r.left();
            const int right = r.right();
            const int top = r.top();
            const int bottom = r.bottom();
            const double centerX = (left + right) / 2.0;
            const double centerY = (top + bottom) / 2.0;

            auto clampRect = [&](QRect candidate) {
                int w = std::max(minW, candidate.width());
                int h = std::max(minH, candidate.height());
                if (w > bounds.width()) {
                    w = bounds.width();
                    h = std::max(minH, static_cast<int>(std::round(w / aspectRatio)));
                }
                if (h > bounds.height()) {
                    h = bounds.height();
                    w = std::max(minW, static_cast<int>(std::round(h * aspectRatio)));
                }
                candidate.setSize(QSize(w, h));
                if (candidate.left() < bounds.left()) candidate.moveLeft(bounds.left());
                if (candidate.top() < bounds.top()) candidate.moveTop(bounds.top());
                if (candidate.right() > bounds.right()) candidate.moveRight(bounds.right());
                if (candidate.bottom() > bounds.bottom()) candidate.moveBottom(bounds.bottom());
                return candidate;
            };

            switch (m_resizing) {
            case HandlePos::Left: {
                int newWidth = std::max(minW, right - pos.x() + 1);
                int newHeight = std::max(minH, static_cast<int>(std::round(newWidth / aspectRatio)));
                QRect candidate(right - newWidth + 1,
                                static_cast<int>(std::round(centerY - newHeight / 2.0)),
                                newWidth, newHeight);
                m_selection = clampRect(candidate);
                update();
                return;
            }
            case HandlePos::Right: {
                int newWidth = std::max(minW, pos.x() - left + 1);
                int newHeight = std::max(minH, static_cast<int>(std::round(newWidth / aspectRatio)));
                QRect candidate(left,
                                static_cast<int>(std::round(centerY - newHeight / 2.0)),
                                newWidth, newHeight);
                m_selection = clampRect(candidate);
                update();
                return;
            }
            case HandlePos::Top: {
                int newHeight = std::max(minH, bottom - pos.y() + 1);
                int newWidth = std::max(minW, static_cast<int>(std::round(newHeight * aspectRatio)));
                QRect candidate(static_cast<int>(std::round(centerX - newWidth / 2.0)),
                                bottom - newHeight + 1,
                                newWidth, newHeight);
                m_selection = clampRect(candidate);
                update();
                return;
            }
            case HandlePos::Bottom: {
                int newHeight = std::max(minH, pos.y() - top + 1);
                int newWidth = std::max(minW, static_cast<int>(std::round(newHeight * aspectRatio)));
                QRect candidate(static_cast<int>(std::round(centerX - newWidth / 2.0)),
                                top,
                                newWidth, newHeight);
                m_selection = clampRect(candidate);
                update();
                return;
            }
            case HandlePos::TopLeft:
            case HandlePos::TopRight:
            case HandlePos::BottomLeft:
            case HandlePos::BottomRight: {
                const QPoint anchor =
                    (m_resizing == HandlePos::TopLeft) ? QPoint(right, bottom) :
                    (m_resizing == HandlePos::TopRight) ? QPoint(left, bottom) :
                    (m_resizing == HandlePos::BottomLeft) ? QPoint(right, top) :
                                                            QPoint(left, top);
                double rawW = std::abs(pos.x() - anchor.x()) + 1.0;
                double rawH = std::abs(pos.y() - anchor.y()) + 1.0;
                double newW = std::max<double>(minW, rawW);
                double newH = std::max<double>(minH, rawH);
                if (newW / newH > aspectRatio) {
                    newH = newW / aspectRatio;
                } else {
                    newW = newH * aspectRatio;
                }

                int leftEdge = anchor.x();
                int topEdge = anchor.y();
                if (m_resizing == HandlePos::TopLeft || m_resizing == HandlePos::BottomLeft) {
                    leftEdge = anchor.x() - static_cast<int>(std::round(newW)) + 1;
                }
                if (m_resizing == HandlePos::TopLeft || m_resizing == HandlePos::TopRight) {
                    topEdge = anchor.y() - static_cast<int>(std::round(newH)) + 1;
                }
                if (m_resizing == HandlePos::BottomRight) {
                    leftEdge = anchor.x();
                    topEdge = anchor.y();
                }
                if (m_resizing == HandlePos::BottomLeft) {
                    topEdge = anchor.y();
                }
                if (m_resizing == HandlePos::TopRight) {
                    leftEdge = anchor.x();
                }

                QRect candidate(leftEdge, topEdge,
                                static_cast<int>(std::round(newW)),
                                static_cast<int>(std::round(newH)));
                m_selection = clampRect(candidate);
                update();
                return;
            }
            default:
                break;
            }
        }

        switch (m_resizing) {
        case HandlePos::TopLeft:     r.setTopLeft(r.topLeft() + delta);         break;
        case HandlePos::Top:         r.setTop(r.top() + delta.y());             break;
        case HandlePos::TopRight:    r.setTopRight(r.topRight() + delta);       break;
        case HandlePos::Right:       r.setRight(r.right() + delta.x());         break;
        case HandlePos::BottomRight: r.setBottomRight(r.bottomRight() + delta); break;
        case HandlePos::Bottom:      r.setBottom(r.bottom() + delta.y());       break;
        case HandlePos::BottomLeft:  r.setBottomLeft(r.bottomLeft() + delta);   break;
        case HandlePos::Left:        r.setLeft(r.left() + delta.x());           break;
        default: break;
        }
        r = r.intersected(rect());
        if (r.width()  < kMinSize) r.setWidth(kMinSize);
        if (r.height() < kMinSize) r.setHeight(kMinSize);
        m_selection = r;
        update();
        return;
    }

    // Hover — update toolbar highlight + cursor
    if (m_hasSelection) {
        const QRect sel = m_selection.normalized();
        ToolbarLayout layout = computeToolbarLayout(
            sel.x(),
            sel.y(),
            sel.width(),
            sel.height(),
            width(),
            height(),
            m_captureIntent == CaptureIntent::Scroll
        );
        int newHover = -1;
        for (int i = 0; i < NUM_TOOLS; ++i) {
            if (layout.toolCells[i].contains(pos)) { newHover = i; break; }
        }
        bool newSizeHover = layout.sizeCard.contains(pos);
        bool newCropHover = layout.cropCard.contains(pos);
        if (newHover != m_hoveredTool
            || newSizeHover != m_hoveredSizeCard
            || newCropHover != m_hoveredCaptureCropCard) {
            m_hoveredTool = newHover;
            m_hoveredSizeCard = newSizeHover;
            m_hoveredCaptureCropCard = newCropHover;
            update();
        }
    }

    if (m_countdownActive) {
        // During countdown, only show drag/resize/move or cancel-bubble cursor
        if (m_moving) {
            setCursor(Qt::ClosedHandCursor);
        } else if (m_resizing != HandlePos::None) {
            switch (m_resizing) {
            case HandlePos::TopLeft:
            case HandlePos::BottomRight: setCursor(Qt::SizeFDiagCursor); break;
            case HandlePos::TopRight:
            case HandlePos::BottomLeft:   setCursor(Qt::SizeBDiagCursor); break;
            case HandlePos::Top:
            case HandlePos::Bottom:      setCursor(Qt::SizeVerCursor);   break;
            case HandlePos::Left:
            case HandlePos::Right:       setCursor(Qt::SizeHorCursor);   break;
            default:                     setCursor(Qt::ArrowCursor);       break;
            }
        } else if (m_hoveredCountdownCancel) {
            setCursor(Qt::PointingHandCursor);
        } else {
            setCursor(Qt::ArrowCursor);
        }
    } else {
        updateCursor(pos);
    }
}

void CaptureOverlay::mouseReleaseEvent(QMouseEvent* event)
{
    if (event->button() != Qt::LeftButton) return;
    m_pointerPos = event->pos();

    if (isCrosshairMode()) {
        if (!m_dragging) {
            return;
        }

        m_dragging = false;
        const QRect norm = m_selection.normalized();
        if (norm.width() < kMinSize || norm.height() < kMinSize) {
            m_hasSelection = false;
            cancelSelection();
            return;
        }

        m_selection = norm;
        m_hasSelection = true;
        confirmSelection();
        return;
    }

    // Stop slider drag
    if (m_sliderDragging) {
        m_sliderDragging = false;
        update();
    }
    if (m_keySliderDragging) {
        m_keySliderDragging = false;
        update();
    }
    if (m_gifFpsDragging) {
        m_gifFpsDragging = false;
    }
    if (m_gifQualityDragging) {
        m_gifQualityDragging = false;
    }
    if (m_draggingWebcam) {
        m_draggingWebcam = false;
        update();
    }

    // Reset recording panel hover state
    if (m_recordingPanelOpen) {
        m_hoveredRecordTile = RecordPanelTile::None;
    }

    if (m_dragging) {
        m_dragging = false;
        QRect norm = m_selection.normalized();
        if (norm.width() < kMinSize || norm.height() < kMinSize)
            m_hasSelection = false;
        else { m_selection = norm; m_hasSelection = true; }
        update();
    }
    m_moving = false;
    m_resizing = HandlePos::None;
    updateCursor(event->pos());
}

void CaptureOverlay::mouseDoubleClickEvent(QMouseEvent* event)
{
    if (event->button() != Qt::LeftButton) return;
    if (isCrosshairMode()) return;
    if (!m_hasSelection) return;

    // Ignore double-click when recording panel is open to prevent accidental triggers
    if (m_recordingPanelOpen) return;

    const QPoint pos = event->pos();
    // Only confirm if the double-click is actually inside the selection rect.
    // Double-clicking outside should start a new selection, not confirm — the
    // mouseDoubleClickEvent arrives instead of the second mousePressEvent, so
    // we need to replicate the "click outside → new selection" logic here.
    const QRect sel = m_selection.normalized();
    if (sel.contains(pos)) {
        if (m_captureIntent == CaptureIntent::Scroll) {
            return;
        }
        // Double-click inside the selection → confirm (take screenshot)
        confirmSelection();
    } else {
        // Double-click outside the selection → treat like a press: start fresh.
        // Check toolbar first.
        ToolbarLayout layout = computeToolbarLayout(
            sel.x(),
            sel.y(),
            sel.width(),
            sel.height(),
            width(),
            height(),
            m_captureIntent == CaptureIntent::Scroll
        );
        bool clickedToolbar = layout.leftToolsPanel.contains(pos) ||
                              layout.sizeCard.contains(pos) ||
                              layout.cropCard.contains(pos);
        if (clickedToolbar) {
            for (int i = 0; i < NUM_TOOLS; ++i) {
                if (layout.toolCells[i].contains(pos)) {
                    // Reuse same handleToolClick logic
                    if (i == 1) {
                        exitScrollMode();
                        m_selection = QRect(0, 0, width(), height());
                        m_hasSelection = true;
                        m_fullscreenMode = true;
                        m_captureIntent = CaptureIntent::Area;
                        m_captureCropMenuOpen = false;
                        update();
                    } else if (i == 0) {
                        exitScrollMode();
                        int defaultW = std::max(kMinSize, std::min(DEFAULT_SELECTION_W, width()));
                        int defaultH = std::max(kMinSize, std::min(DEFAULT_SELECTION_H, height()));
                        m_selection = QRect((width()-defaultW)/2, (height()-defaultH)/2, defaultW, defaultH);
                        m_hasSelection = true;
                        m_fullscreenMode = false;
                        m_timerDelayActive = false;
                        m_captureAspectRatioIndex = 0;
                        m_captureCropMenuOpen = false;
                        m_captureIntent = CaptureIntent::Area;
                        update();
                    } else if (i == 2) {
                        exitScrollMode();
                        m_captureCropMenuOpen = false;
                        m_captureIntent = CaptureIntent::Area;
                        update();
                        showWebScrollCaptureInfo(this);
                    } else if (i == 3) {
                        if (m_timerCaptureEnabled) {
                            if (!m_timerDelayActive) {
                                m_timerDelayActive = true;
                                if (m_captureDelaySeconds <= 0) {
                                    m_captureDelaySeconds = 5;
                                }
                                update();
                            } else {
                                cycleCaptureDelay();
                            }
                        }
                    } else if (i == 4) {
                        exitScrollMode();
                        m_captureCropMenuOpen = false;
                        m_captureIntent = CaptureIntent::Ocr;
                        update();
                    } else if (i == 5) {
                        exitScrollMode();
                        m_captureCropMenuOpen = false;
                        m_captureIntent = CaptureIntent::Record;
                        m_recordingPanelOpen = true;
                        m_recordingToolsHidden = false;
                        if (m_recWebcam && m_webcamDevice >= 0)
                            startWebcamCapture();
                        update();
                    } else {
                        exitScrollMode();
                        m_captureCropMenuOpen = false;
                        m_captureIntent = CaptureIntent::Area;
                        confirmSelection();
                    }
                    return;
                }
            }
            if (layout.cropCard.contains(pos)) {
                const bool wasOpen = m_captureCropMenuOpen;
                m_captureCropMenuOpen = !wasOpen;
                update();
                return;
            }
            return; // Clicked toolbar background — do nothing
        }
        // Outside selection and toolbar — start a new selection drag.
        m_dragging = true;
        m_moving = false;
        m_resizing = HandlePos::None;
        m_hasSelection = false;
        m_fullscreenMode = false;
        m_captureCropMenuOpen = false;
        m_hoveredCaptureCropMenuItem = -1;
        m_selection = QRect(pos, pos);
        m_dragStart = pos;
        setCursor(Qt::CrossCursor);
        update();
    }
}

// ── Keyboard ──────────────────────────────────────────────────────────────────

void CaptureOverlay::keyPressEvent(QKeyEvent* event)
{
    if (isCrosshairMode()) {
        if (event->key() == Qt::Key_Escape) {
            cancelSelection();
            return;
        }
        QWidget::keyPressEvent(event);
        return;
    }

    // Recording panel: ESC closes panel, restores normal mode
    if (m_recordingPanelOpen) {
        switch (event->key()) {
        case Qt::Key_Escape:
            if (m_recordingToolsHidden) {
                // Back to full recording panel
                m_recordingToolsHidden = false;
                if (m_recWebcam && m_webcamDevice >= 0)
                    startWebcamCapture();
            } else {
                // Close recording panel, restore normal capture mode
                resetRecordingPanelToAreaMode();
            }
            update();
            return;
        case Qt::Key_Return:
        case Qt::Key_Enter:
            // Start video recording on Enter
            m_recordType = RecordType::Video;
            m_captureIntent = CaptureIntent::Record;
            confirmRecordingSelection();
            return;
        }
        // Let arrow keys through for resize/move
    }

    // Capture key presses for keystroke preview — block all actions except ESC
    if (m_showKeystrokePreview) {
        if (event->key() == Qt::Key_Escape) {
            // Allow ESC to still work
            m_showKeystrokePreview = false;
            m_keyPreviews.clear();
            update();
            return;
        }
        QString keyText = keyEventToPreviewText(event);
        if (!keyText.isEmpty()) {
            qint64 now = QDateTime::currentMSecsSinceEpoch();
            m_keyPreviews.append({keyText, now});
            if (m_keyPreviews.size() > 8) m_keyPreviews.removeFirst();
            startClickAnimTimer();
            update();
        }
        return; // consume the key — don't trigger any other actions
    }

    if (m_captureIntent == CaptureIntent::Scroll) {
        switch (event->key()) {
        case Qt::Key_Escape:
            cancelSelection();
            return;
        case Qt::Key_Return:
        case Qt::Key_Enter:
        case Qt::Key_Space:
            if (m_scrollStage == ScrollStage::Armed) {
                handleScrollButtonClick(scrollPrimaryButtonRect().center().toPoint());
                return;
            }
            if (m_scrollStage == ScrollStage::Capturing) {
                stopAutoScrollCapture(true);
                return;
            }
            break;
        default:
            break;
        }

        if (m_scrollStage == ScrollStage::Capturing) {
            return;
        }
    }

    bool shift = event->modifiers() & Qt::ShiftModifier;
    switch (event->key()) {
    case Qt::Key_Escape:
        if (m_windowMode) { exitWindowMode(); } else { cancelSelection(); }
        break;
    case Qt::Key_Return:
    case Qt::Key_Enter:
    case Qt::Key_Space:
        if (m_hasSelection) confirmSelection(); break;
    case Qt::Key_Left:
        if (m_hasSelection) {
            if (shift) m_selection.setRight(m_selection.right()-1);
            else       m_selection.translate(-1,0);
            update();
        } break;
    case Qt::Key_Right:
        if (m_hasSelection) {
            if (shift) m_selection.setRight(m_selection.right()+1);
            else       m_selection.translate(1,0);
            update();
        } break;
    case Qt::Key_Up:
        if (m_hasSelection) {
            if (shift) m_selection.setBottom(m_selection.bottom()-1);
            else       m_selection.translate(0,-1);
            update();
        } break;
    case Qt::Key_Down:
        if (m_hasSelection) {
            if (shift) m_selection.setBottom(m_selection.bottom()+1);
            else       m_selection.translate(0,1);
            update();
        } break;
    default: QWidget::keyPressEvent(event);
    }
}
