#include "CaptureOverlay.h"
#include "CaptureOverlay_p.h"
#include "ScreenCapture.h"
#include <QApplication>
#include <QDateTime>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QImage>
#include <QPainter>
#include <QProcess>
#include <QRect>
#include <QSize>
#include <QStandardPaths>
#include <QThread>
#include <QTimer>
#include <QGuiApplication>
#include <QDBusConnection>
#include <QDBusInterface>
#include <QDBusMessage>
#include <X11/Xlib.h>
#include <X11/extensions/XTest.h>
#undef None
#undef Bool

static bool callDaemonBool(const QString& method, int arg = 0, bool hasArg = false)
{
    QDBusInterface iface(QStringLiteral("org.apexshot.Daemon"),
                         QStringLiteral("/org/apexshot/Daemon"),
                         QStringLiteral("org.apexshot.Daemon"),
                         QDBusConnection::sessionBus());
    if (!iface.isValid()) {
        return false;
    }

    const QDBusMessage reply = hasArg ? iface.call(method, arg) : iface.call(method);
    if (reply.type() == QDBusMessage::ErrorMessage || reply.arguments().isEmpty()) {
        return false;
    }

    return reply.arguments().constFirst().toBool();
}

static bool callDaemonScrollStep(int x, int y, int steps)
{
    QDBusInterface iface(QStringLiteral("org.apexshot.Daemon"),
                         QStringLiteral("/org/apexshot/Daemon"),
                         QStringLiteral("org.apexshot.Daemon"),
                         QDBusConnection::sessionBus());
    if (!iface.isValid()) {
        return false;
    }

    const QDBusMessage reply = iface.call(QStringLiteral("ScrollStepGnome"), x, y, steps);
    if (reply.type() == QDBusMessage::ErrorMessage || reply.arguments().isEmpty()) {
        return false;
    }

    return reply.arguments().constFirst().toBool();
}
static bool shouldUseManualScrollAssistMode()
{
    const QString platform = QGuiApplication::platformName().toLower();
    if (!platform.contains(QStringLiteral("wayland"))) {
        return false;
    }

    const QString desktop = QString::fromLocal8Bit(qgetenv("XDG_CURRENT_DESKTOP")).toLower();
    const QString session = QString::fromLocal8Bit(qgetenv("XDG_SESSION_DESKTOP")).toLower();
    return desktop.contains(QStringLiteral("gnome")) || session.contains(QStringLiteral("gnome"));
}

static void callDaemonVoid(const QString& method)
{
    QDBusInterface iface(QStringLiteral("org.apexshot.Daemon"),
                         QStringLiteral("/org/apexshot/Daemon"),
                         QStringLiteral("org.apexshot.Daemon"),
                         QDBusConnection::sessionBus());
    if (!iface.isValid()) {
        return;
    }
    iface.call(method);
}
QRectF CaptureOverlay::scrollPrimaryButtonRect() const
{
    const QRect sel = m_selection.normalized();
    if (sel.isEmpty()) {
        return QRectF();
    }

    double buttonW = std::max(
        SCROLL_BUTTON_MIN_W,
        std::min(sel.width() * 0.66, 220.0)
    );
    buttonW = std::min(buttonW, std::max(96.0, sel.width() - 12.0));

    double buttonX = sel.x() + (sel.width() - buttonW) / 2.0;
    double buttonY = sel.bottom() - SCROLL_BUTTON_H - 10.0;
    if (buttonY < sel.top() + 6.0) {
        buttonY = sel.bottom() + FEATURE_PANEL_TOP_GAP;
    }

    buttonX = std::max(FEATURE_PANEL_MARGIN,
              std::min(buttonX, width() - buttonW - FEATURE_PANEL_MARGIN));
    buttonY = std::max(FEATURE_PANEL_MARGIN,
              std::min(buttonY, height() - SCROLL_BUTTON_H - FEATURE_PANEL_MARGIN));

    return QRectF(buttonX, buttonY, buttonW, SCROLL_BUTTON_H);
}

void CaptureOverlay::enterScrollMode()
{
    if (!m_hasSelection) {
        return;
    }

    exitWindowMode();
    if (m_scrollCaptureTimer && m_scrollCaptureTimer->isActive()) {
        m_scrollCaptureTimer->stop();
    }

    for (const QString& path : m_scrollFramePaths) {
        QFile::remove(path);
    }
    m_scrollFramePaths.clear();

    if (!m_scrollCapturePath.isEmpty() && !m_scrollCaptureReady) {
        QFile::remove(m_scrollCapturePath);
    }

    m_scrollCapturePath.clear();
    m_scrollCaptureSize = QSize();
    m_scrollCaptureReady = false;
    m_scrollStage = ScrollStage::Armed;
    m_manualScrollAssistMode = false;
    setAttribute(Qt::WA_TransparentForMouseEvents, false);
    m_captureIntent = CaptureIntent::Scroll;
    m_fullscreenMode = false;
    m_dragging = false;
    m_moving = false;
    m_resizing = HandlePos::None;
    update();
}

void CaptureOverlay::exitScrollMode(bool keepIntent)
{
    if (m_scrollCaptureTimer && m_scrollCaptureTimer->isActive()) {
        m_scrollCaptureTimer->stop();
    }

    for (const QString& path : m_scrollFramePaths) {
        QFile::remove(path);
    }
    m_scrollFramePaths.clear();

    if (!m_scrollCaptureReady && !m_scrollCapturePath.isEmpty()) {
        QFile::remove(m_scrollCapturePath);
    }

    if (!keepIntent) {
        m_scrollCapturePath.clear();
        m_scrollCaptureSize = QSize();
        m_scrollCaptureReady = false;
        m_captureIntent = CaptureIntent::Area;
    }

    m_scrollStage = ScrollStage::Inactive;
    m_manualScrollAssistMode = false;
    setAttribute(Qt::WA_TransparentForMouseEvents, false);
    update();
}

bool CaptureOverlay::handleScrollButtonClick(const QPoint& pos)
{
    if (m_captureIntent != CaptureIntent::Scroll || !m_hasSelection) {
        return false;
    }

    if (m_scrollStage == ScrollStage::Armed) {
        if (!scrollPrimaryButtonRect().contains(pos)) {
            return false;
        }
        startAutoScrollCapture();
        return true;
    }

    return false;
}

// ── Auto-scroll capture methods ──────────────────────────────────────────────

void CaptureOverlay::startAutoScrollCapture()
{
    if (!m_hasSelection) return;

    m_scrollCaptureReady = false;
    m_scrollCapturePath.clear();
    m_scrollCaptureSize = QSize();
    for (const QString& framePath : m_scrollFramePaths) {
        QFile::remove(framePath);
    }
    m_scrollFramePaths.clear();

    m_dragging = false;
    m_moving = false;
    m_resizing = HandlePos::None;
    m_scrollStage = ScrollStage::Capturing;
    m_scrollSimilarCount = 0;
    m_scrollFrameCount = 0;
    m_scrollCaptureArea = m_selection.normalized().translated(geometry().topLeft());
    m_manualScrollAssistMode = shouldUseManualScrollAssistMode();

    if (m_manualScrollAssistMode) {
        setAttribute(Qt::WA_TransparentForMouseEvents, true);
        releaseKeyboard();
        hide();
    } else {
        setAttribute(Qt::WA_TransparentForMouseEvents, false);
    }
    QApplication::processEvents();

    // Position and show the control panel
    m_scrollControlPanel->setFrameCount(0);
    m_scrollControlPanel->setStatus(m_manualScrollAssistMode
        ? QStringLiteral("Scroll manually, then click Done")
        : QStringLiteral("Capturing…"));
    m_scrollControlPanel->positionNear(m_scrollCaptureArea, size());
    m_scrollControlPanel->show();

    if (!m_manualScrollAssistMode) {
        callDaemonBool(QStringLiteral("ScrollBeginGnome"));
    }
    update();

    if (!m_manualScrollAssistMode) {
        m_scrollControlPanel->hide();
        QApplication::processEvents();
    }

    // Capture the first frame immediately
    if (captureScrollFrameSilent()) {
        m_scrollControlPanel->setFrameCount(m_scrollFrameCount);
        if (!m_manualScrollAssistMode) {
            m_scrollControlPanel->show();
            m_scrollControlPanel->raise();
        }
    }

    update();
    QApplication::processEvents();

    // Start the auto-scroll loop
    if (m_scrollCaptureTimer) {
        m_scrollCaptureTimer->start();
    }
}

void CaptureOverlay::simulateScrollDown()
{
    int targetX = m_scrollCaptureArea.x() + m_scrollCaptureArea.width() / 2;
    int targetY = m_scrollCaptureArea.y() + m_scrollCaptureArea.height() / 2;

    const bool hadTransparentMouse = testAttribute(Qt::WA_TransparentForMouseEvents);
    if (!hadTransparentMouse) {
        setAttribute(Qt::WA_TransparentForMouseEvents, true);
    }
    releaseKeyboard();
    QApplication::processEvents();

    const bool daemonScrolled = callDaemonScrollStep(targetX, targetY, kScrollLinesPerTick);

    if (!hadTransparentMouse) {
        setAttribute(Qt::WA_TransparentForMouseEvents, false);
    }
    QApplication::processEvents();

    if (daemonScrolled) {
        std::fprintf(stderr, "[CaptureOverlay] Auto-scroll: portal step accepted, continuing with local fallback\n");
    }

    QString ydotoolPath;
    QStringList ydotoolPaths = {"/usr/local/bin/ydotool", "/usr/bin/ydotool", "ydotool"};
    for (const QString& path : ydotoolPaths) {
        QProcess testProc;
        testProc.setProgram(path);
        testProc.setArguments({"help"});
        testProc.start();
        if (testProc.waitForFinished(500) && testProc.exitCode() == 0) {
            ydotoolPath = path;
            break;
        }
    }

    QString wtypePath;
    QStringList wtypePaths = {"/usr/local/bin/wtype", "/usr/bin/wtype", "wtype"};
    for (const QString& candidate : wtypePaths) {
        if (candidate.startsWith('/')) {
            QFileInfo fi(candidate);
            if (fi.exists() && fi.isExecutable()) {
                wtypePath = fi.absoluteFilePath();
                break;
            }
            continue;
        }
        const QString resolved = QStandardPaths::findExecutable(candidate);
        if (!resolved.isEmpty()) {
            wtypePath = resolved;
            break;
        }
    }

    QProcessEnvironment env;
    env.insert("YDOTOOL_SOCKET", "/tmp/.ydotool_socket");

    const bool canUseYdotool = !ydotoolPath.isEmpty() && QFileInfo("/dev/uinput").isWritable();
    if (!ydotoolPath.isEmpty() && !canUseYdotool) {
        std::fprintf(stderr,
                     "[CaptureOverlay] Auto-scroll: ydotool detected but /dev/uinput is not writable; skipping ydotool path\n");
    }

    clearFocus();
    releaseKeyboard();
    QApplication::processEvents();

    const bool hadTransparentMouseFallback = testAttribute(Qt::WA_TransparentForMouseEvents);
    if (!hadTransparentMouseFallback) {
        setAttribute(Qt::WA_TransparentForMouseEvents, true);
        QApplication::processEvents();
    }

    auto restoreMouseCapture = [&]() {
        if (!hadTransparentMouseFallback) {
            setAttribute(Qt::WA_TransparentForMouseEvents, false);
            QApplication::processEvents();
        }
    };

    if (!wtypePath.isEmpty()) {
        if (canUseYdotool) {
            QProcess moveProc;
            moveProc.setProgram(ydotoolPath);
            moveProc.setProcessEnvironment(env);
            moveProc.setArguments({"mousemove", QString::number(targetX), QString::number(targetY)});
            moveProc.start();
            moveProc.waitForFinished(200);

            QProcess clickProc;
            clickProc.setProgram(ydotoolPath);
            clickProc.setProcessEnvironment(env);
            clickProc.setArguments({"click", "1"});
            clickProc.start();
            clickProc.waitForFinished(150);
            QThread::msleep(60);
        }

        bool sent = false;
        const int burst = std::max(1, kScrollLinesPerTick * 2);
        for (int i = 0; i < burst; ++i) {
            QProcess textProc;
            textProc.setProgram(wtypePath);
            textProc.setArguments({" "});
            textProc.start();
            if (textProc.waitForFinished(180) && textProc.exitCode() == 0) {
                sent = true;
            } else {
                break;
            }
        }

        if (sent) {
            restoreMouseCapture();
            std::fprintf(stderr,
                         "[CaptureOverlay] Auto-scroll: wtype text-space sent=true\n");
            return;
        }

        const QStringList keyVariants = {QStringLiteral("Page_Down"),
                                         QStringLiteral("Next"),
                                         QStringLiteral("pagedown")};
        const int pageDownCount = std::max(1, kScrollLinesPerTick);
        for (int i = 0; i < pageDownCount; ++i) {
            bool sentThisTick = false;
            for (const QString& keyName : keyVariants) {
                QProcess keyProc;
                keyProc.setProgram(wtypePath);
                keyProc.setProcessEnvironment(env);
                keyProc.setArguments({"-k", keyName});
                keyProc.start();
                if (keyProc.waitForFinished(180) && keyProc.exitCode() == 0) {
                    sent = true;
                    sentThisTick = true;
                    break;
                }
            }
            if (!sentThisTick) {
                break;
            }
        }

        restoreMouseCapture();
        std::fprintf(stderr,
                     "[CaptureOverlay] Auto-scroll: wtype PageDown sent=%s\n",
                     sent ? "true" : "false");
        return;
    }

    if (canUseYdotool) {
        QProcess moveProc;
        moveProc.setProgram(ydotoolPath);
        moveProc.setProcessEnvironment(env);
        moveProc.setArguments({"mousemove", QString::number(targetX), QString::number(targetY)});
        moveProc.start();
        moveProc.waitForFinished(200);

        QProcess clickProc;
        clickProc.setProgram(ydotoolPath);
        clickProc.setProcessEnvironment(env);
        clickProc.setArguments({"click", "1"});
        clickProc.start();
        clickProc.waitForFinished(150);

        QThread::msleep(60);

        const int wheelSteps = std::max(1, kScrollLinesPerTick);
        for (int i = 0; i < wheelSteps; ++i) {
            QProcess wheelProc;
            wheelProc.setProgram(ydotoolPath);
            wheelProc.setProcessEnvironment(env);
            wheelProc.setArguments({"click", "5"});
            wheelProc.start();
            wheelProc.waitForFinished(90);
        }

        restoreMouseCapture();
        std::fprintf(stderr, "[CaptureOverlay] Auto-scroll: ydotool wheel-down sent\n");
        return;
    }

    Display* display = XOpenDisplay(nullptr);
    if (!display) {
        restoreMouseCapture();
        std::fprintf(stderr, "[CaptureOverlay] Auto-scroll: XOpenDisplay failed\n");
        return;
    }

    XTestFakeMotionEvent(display, DefaultScreen(display), targetX, targetY, CurrentTime);
    XFlush(display);
    XTestFakeButtonEvent(display, 1, True, CurrentTime);
    XTestFakeButtonEvent(display, 1, False, CurrentTime);
    XFlush(display);
    QThread::msleep(50);

    for (int i = 0; i < kScrollLinesPerTick; ++i) {
        XTestFakeButtonEvent(display, 5, True, CurrentTime);
        XTestFakeButtonEvent(display, 5, False, CurrentTime);
        XFlush(display);
        QThread::msleep(50);
    }

    XCloseDisplay(display);
    restoreMouseCapture();
    std::fprintf(stderr, "[CaptureOverlay] Auto-scroll: X11 wheel-down simulation done\n");
}

void CaptureOverlay::onAutoScrollTick()
{
    if (m_scrollStage != ScrollStage::Capturing) {
        return;
    }

    // 1. Scroll progression
    if (!m_manualScrollAssistMode) {
        simulateScrollDown();
    }

    update();
    QApplication::processEvents();

    // 2. Wait for applications to render the scrolled content (smooth scrolling, etc)
    QThread::msleep(kScrollSettleMs);

    // 3. Capture the new frame
    if (!m_manualScrollAssistMode) {
        m_scrollControlPanel->hide();
        QApplication::processEvents();
    }

    bool captured = false;
    if (captureScrollFrameSilent()) {
        m_scrollControlPanel->setFrameCount(m_scrollFrameCount);
        captured = true;
    }

    if (captured && !m_manualScrollAssistMode) {
        // Show control panel briefly after capture to show progress
        m_scrollControlPanel->show();
        m_scrollControlPanel->raise();
    }

    // 4. Check stop conditions
    const int similarThreshold = m_manualScrollAssistMode
        ? kManualSimilarStopThreshold
        : kSimilarStopThreshold;
    if (m_scrollSimilarCount >= similarThreshold) {
        if (m_manualScrollAssistMode) {
            std::fprintf(stderr, "[CaptureOverlay] Manual scroll assist: inactivity reached, finalizing.\n");
        } else {
            std::fprintf(stderr, "[CaptureOverlay] Auto-scroll reached end of content.\n");
        }
        stopAutoScrollCapture(true);
        return;
    }

    if (m_scrollFrameCount >= kMaxScrollFrames) {
        std::fprintf(stderr, "[CaptureOverlay] Auto-scroll reached max frames (%d).\n", kMaxScrollFrames);
        stopAutoScrollCapture(true);
        return;
    }

    // 5. Schedule next capture using single-shot timer (prevents overlapping captures)
    // Total interval = settle time + processing time + small buffer
    const int nextTickDelay = SCROLL_CAPTURE_INTERVAL_MS;
    if (m_scrollCaptureTimer) {
        m_scrollCaptureTimer->start(nextTickDelay);
    }
}

bool CaptureOverlay::captureScrollFrameSilent()
{
    if (m_scrollCaptureArea.width() <= 0 || m_scrollCaptureArea.height() <= 0) {
        return false;
    }

    QString imagePath;
    QSize imageSize;
    QString error;

    // Overlay stays visible; capture-safe rendering avoids drawing capture UI inside the capture area.
    const bool ok = ScreenCapture::captureAreaToTempPng(m_scrollCaptureArea, imagePath, imageSize, error);

    if (!ok) {
        std::fprintf(stderr,
                     "[CaptureOverlay] Scroll frame capture silent failed: %s\n",
                     error.toLocal8Bit().constData());
        return false;
    }

    QImage image(imagePath);
    if (image.isNull()) {
        QFile::remove(imagePath);
        return false;
    }

    // Compare with the previous frame to see if we've stopped moving
    if (!m_scrollFramePaths.isEmpty()) {
        const QImage previous(m_scrollFramePaths.back());
        if (!previous.isNull() && imagesSimilar(previous, image, 2.0)) {
            QFile::remove(imagePath);
            m_scrollSimilarCount++;
            std::fprintf(stderr, "[CaptureOverlay] Frame similar to previous (count=%d), diff=%.1f\n", 
                m_scrollSimilarCount, getImageDiff(previous, image));
            return true; // it was captured, but it is similar
        }
    }

    // It's a new unique frame
    m_scrollSimilarCount = 0;
    m_scrollFrameCount++;
    m_scrollFramePaths.push_back(imagePath);
    std::fprintf(stderr,
        "[CaptureOverlay] Captured unique frame #%d: %s\n",
        m_scrollFrameCount, imagePath.toLocal8Bit().constData());
    return true;
}

void CaptureOverlay::stopAutoScrollCapture(bool finalize)
{
    if (!m_manualScrollAssistMode) {
        callDaemonVoid(QStringLiteral("ScrollEndGnome"));
    }
    setAttribute(Qt::WA_TransparentForMouseEvents, false);
    QApplication::processEvents();

    if (m_scrollCaptureTimer && m_scrollCaptureTimer->isActive()) {
        m_scrollCaptureTimer->stop();
    }

    m_scrollControlPanel->setCapturingDone();
    QApplication::processEvents();

    if (finalize) {
        if (finalizeScrollCapture()) {
            m_scrollControlPanel->hide();
            confirmSelection();
        } else {
            // Stitching failed
            m_scrollStage = ScrollStage::Armed;
            m_scrollControlPanel->hide();
            show(); // Show main overlay again
            raise();
            activateWindow();
            grabKeyboard();
            update();
        }
    } else {
        // Cancelled
        m_scrollStage = ScrollStage::Armed;
        m_scrollControlPanel->hide();
        show(); // Show main overlay again
        raise();
        activateWindow();
        grabKeyboard();
        update();
    }
}

bool CaptureOverlay::finalizeScrollCapture()
{

    QString stitchedPath;
    QSize stitchedSize;
    QString stitchError;
    const bool stitched = stitchScrollFrames(stitchedPath, stitchedSize, stitchError);

    for (const QString& framePath : m_scrollFramePaths) {
        QFile::remove(framePath);
    }
    m_scrollFramePaths.clear();

    if (!stitched) {
        std::fprintf(stderr,
                     "[CaptureOverlay] Scroll stitching failed: %s\n",
                     stitchError.toLocal8Bit().constData());
        if (!stitchedPath.isEmpty()) {
            QFile::remove(stitchedPath);
        }
        return false;
    }

    m_scrollCapturePath = stitchedPath;
    m_scrollCaptureSize = stitchedSize;
    m_scrollCaptureReady = true;
    m_scrollStage = ScrollStage::Inactive;
    return true;
}

bool CaptureOverlay::stitchScrollFrames(QString& outPath,
                                        QSize& outSize,
                                        QString& outError) const
{
    if (m_scrollFramePaths.isEmpty()) {
        outError = QStringLiteral("No captured scroll frames to stitch");
        return false;
    }

    std::fprintf(stderr, "[CaptureOverlay] Stitching %d frames...\n", m_scrollFramePaths.size());

    QList<QImage> frames;
    frames.reserve(m_scrollFramePaths.size());
    int targetWidth = std::numeric_limits<int>::max();

    for (const QString& path : m_scrollFramePaths) {
        QImage frame(path);
        if (frame.isNull()) {
            std::fprintf(stderr, "[CaptureOverlay] Warning: Failed to load frame: %s\n", qPrintable(path));
            continue;
        }
        frame = frame.convertToFormat(QImage::Format_ARGB32);
        targetWidth = std::min(targetWidth, frame.width());
        std::fprintf(stderr, "[CaptureOverlay] Frame size: %dx%d\n", frame.width(), frame.height());
        frames.push_back(frame);
    }

    if (frames.isEmpty()) {
        outError = QStringLiteral("Captured scroll frames are unreadable");
        return false;
    }

    if (targetWidth <= 0) {
        outError = QStringLiteral("Captured scroll frame width is invalid");
        return false;
    }

    for (QImage& frame : frames) {
        if (frame.width() > targetWidth) {
            frame = frame.copy(0, 0, targetWidth, frame.height());
        }
    }

    QImage stitched = frames.first();
    QImage previous = frames.first();
    int totalHeight = stitched.height();

    for (int i = 1; i < frames.size(); ++i) {
        const QImage next = frames[i];
        if (next.isNull()) {
            continue;
        }

        int overlap = estimateOverlapRows(previous, next);
        overlap = std::max(0, std::min(overlap, next.height() - 1));
        
        std::fprintf(stderr,
            "[CaptureOverlay] Frame %d overlap=%d append=%d prevH=%d nextH=%d\n",
            i, overlap, next.height() - overlap, previous.height(), next.height());
        
        const int appendHeight = next.height() - overlap;
        if (appendHeight <= 2) {
            std::fprintf(stderr, "[CaptureOverlay] Frame %d: skipping (too small append)\n", i);
            previous = next;
            continue;
        }

        QImage combined(stitched.width(),
                        stitched.height() + appendHeight,
                        QImage::Format_ARGB32);
        combined.fill(Qt::transparent);

        {
            QPainter painter(&combined);
            painter.drawImage(0, 0, stitched);
            painter.drawImage(0,
                              stitched.height(),
                              next,
                              0,
                              overlap,
                              next.width(),
                              appendHeight);
        }

        stitched = combined;
        totalHeight += appendHeight;
        previous = next;
    }

    std::fprintf(stderr, "[CaptureOverlay] Final stitched size: %dx%d\n", stitched.width(), stitched.height());

    const QString tempPath =
      QDir::tempPath() +
      QStringLiteral("/apexshot-scroll-%1.png")
        .arg(QDateTime::currentMSecsSinceEpoch());

    if (!stitched.save(tempPath, "PNG")) {
        outError = QStringLiteral("Failed to save stitched scroll capture");
        return false;
    }

    outPath = tempPath;
    outSize = stitched.size();
    return true;
}

bool CaptureOverlay::imagesSimilar(const QImage& a, const QImage& b, double threshold)
{
    if (a.isNull() || b.isNull()) {
        return false;
    }
    if (a.size() != b.size()) {
        return false;
    }
    const int overlap = std::min(a.height(), b.height());
    if (overlap <= 0) {
        return false;
    }
    return overlapDiffScore(a, b, overlap) <= threshold;
}

double CaptureOverlay::getImageDiff(const QImage& a, const QImage& b)
{
    if (a.isNull() || b.isNull()) {
        return 1000.0;
    }
    if (a.size() != b.size()) {
        return 1000.0;
    }
    const int overlap = std::min(a.height(), b.height());
    if (overlap <= 0) {
        return 1000.0;
    }
    return overlapDiffScore(a, b, overlap);
}

int CaptureOverlay::estimateOverlapRows(const QImage& prev, const QImage& next)
{
    if (prev.isNull() || next.isNull()) {
        return 0;
    }

    const int maxOverlap = std::min(prev.height(), next.height());
    if (maxOverlap <= 8) {
        return 0;
    }

    // Use template matching approach: find where the bottom of prev matches in next
    // This is more robust than assuming a fixed overlap
    
    const int width = std::min(prev.width(), next.width());
    const int templateHeight = std::min(100, maxOverlap / 2);  // Use 100px or half of max as template
    
    // Extract template from bottom of previous frame
    QImage prevRgba = prev.convertToFormat(QImage::Format_RGBA8888);
    QImage nextRgba = next.convertToFormat(QImage::Format_RGBA8888);
    
    if (prevRgba.isNull() || nextRgba.isNull()) {
        return 0;
    }
    
    // Search range: look for the template in the top half of next image
    const int searchStart = 0;
    const int searchEnd = std::min(maxOverlap - templateHeight, nextRgba.height() - templateHeight);
    
    if (searchEnd <= searchStart) {
        return 0;
    }
    
    double bestScore = std::numeric_limits<double>::max();
    int bestOffset = 0;
    
    // Try each possible offset
    for (int offset = searchStart; offset <= searchEnd; offset += 2) {
        double score = computeTemplateMatchScore(
            prevRgba, prevRgba.height() - templateHeight, templateHeight,
            nextRgba, offset, width);
        
        if (score < bestScore) {
            bestScore = score;
            bestOffset = offset;
        }
    }
    
    // If best match score is too high, no valid overlap found
    // Use per-pixel threshold: average difference per pixel should be reasonable
    const double avgDiffPerPixel = bestScore / (width * templateHeight);
    if (avgDiffPerPixel > 15.0) {  // More than 15 avg difference per pixel = no match
        return 0;
    }
    
    // The overlap is where the template from prev matches in next
    // bestOffset is the Y position in next where the match occurs
    // The template from the bottom of prev was found at 'bestOffset' in next
    //
    // If bestOffset is small: the template from bottom of prev is near top of next
    //   -> we scrolled a lot -> small overlap
    // If bestOffset is large: the template from bottom of prev is near bottom of next
    //   -> we scrolled a little -> large overlap
    //
    // The overlap is approximately bestOffset + templateHeight because:
    // - Content from prev.height()-templateHeight to prev.height() (the template)
    //   is now at bestOffset to bestOffset+templateHeight in next
    // - So content from 0 to bestOffset+templateHeight in next overlaps with prev
    
    const int expectedOverlap = maxOverlap / 2;  // Assume ~50% overlap
    const int detectedOverlap = bestOffset + templateHeight;
    
    // Validate the detected overlap is reasonable
    if (detectedOverlap < 20 || detectedOverlap > maxOverlap - 20) {
        // Overlap is too small or too large, use expected
        return std::max(50, maxOverlap / 3);
    }
    
    // If detected overlap is very different from expected, it might be a false match
    // Use a weighted average to be conservative
    if (std::abs(detectedOverlap - expectedOverlap) > maxOverlap / 3) {
        // Blend detected with expected to avoid extreme values
        return (detectedOverlap + expectedOverlap) / 2;
    }
    
    return detectedOverlap;
}

double CaptureOverlay::computeTemplateMatchScore(const QImage& prev, int prevY, int templateHeight,
                                              const QImage& next, int nextY, int width)
{
    if (width <= 0 || templateHeight <= 0) {
        return std::numeric_limits<double>::max();
    }
    
    const int stepX = std::max(1, width / 64);
    const int stepY = std::max(1, templateHeight / 8);
    
    double diffSum = 0.0;
    int sampleCount = 0;
    
    for (int y = 0; y < templateHeight; y += stepY) {
        const uchar* prevLine = prev.constScanLine(prevY + y);
        const uchar* nextLine = next.constScanLine(nextY + y);
        for (int x = 0; x < width; x += stepX) {
            const uchar* a = prevLine + x * 4;
            const uchar* b = nextLine + x * 4;
            // Compare RGB (skip alpha)
            diffSum += std::abs(int(a[0]) - int(b[0]));
            diffSum += std::abs(int(a[1]) - int(b[1]));
            diffSum += std::abs(int(a[2]) - int(b[2]));
            sampleCount += 3;
        }
    }
    
    return sampleCount > 0 ? diffSum / sampleCount : std::numeric_limits<double>::max();
}

double CaptureOverlay::overlapDiffScore(const QImage& prev,
                                        const QImage& next,
                                        int overlapRows)
{
    if (overlapRows <= 0) {
        return std::numeric_limits<double>::max();
    }

    const QImage prevRgba = prev.convertToFormat(QImage::Format_RGBA8888);
    const QImage nextRgba = next.convertToFormat(QImage::Format_RGBA8888);
    if (prevRgba.isNull() || nextRgba.isNull()) {
        return std::numeric_limits<double>::max();
    }

    const int width = std::min(prevRgba.width(), nextRgba.width());
    const int rows = std::min(overlapRows, std::min(prevRgba.height(), nextRgba.height()));
    if (width <= 0 || rows <= 0) {
        return std::numeric_limits<double>::max();
    }

    const int prevStartY = prevRgba.height() - rows;
    const int stepX = std::max(1, width / 64);
    const int stepY = std::max(1, rows / 42);

    double diffSum = 0.0;
    int sampleCount = 0;

    for (int y = 0; y < rows; y += stepY) {
        const uchar* prevLine = prevRgba.constScanLine(prevStartY + y);
        const uchar* nextLine = nextRgba.constScanLine(y);
        for (int x = 0; x < width; x += stepX) {
            const uchar* a = prevLine + x * 4;
            const uchar* b = nextLine + x * 4;
            diffSum += std::abs(int(a[0]) - int(b[0]));
            diffSum += std::abs(int(a[1]) - int(b[1]));
            diffSum += std::abs(int(a[2]) - int(b[2]));
            sampleCount += 3;
        }
    }

    if (sampleCount <= 0) {
        return std::numeric_limits<double>::max();
    }

    return diffSum / sampleCount;
}

