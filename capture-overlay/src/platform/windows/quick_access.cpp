#include "platform/windows/quick_access.h"

#include "CaptureModeToolbar.h"
#include "platform/platform.h"
#include "platform/recording.h"
#include "platform/recording_settings.h"
#include "platform/windows/audio_devices.h"
#include "platform/windows/ocr.h"
#include "platform/windows/usage_telemetry.h"
#include "platform/windows/qr.h"
#include "platform/windows/capture_sound.h"
#include "platform/windows/hotkeys.h"
#include "platform/windows/recording_controls.h"
#include "platform/windows/recording_countdown.h"
#include "platform/windows/recording_mask.h"
#include "platform/windows/webcam_bubble.h"
#include "platform/windows/webcam_preview.h"
#include "platform/windows/window_recording.h"
#include "platform/windows/scroll_capture.h"
#include "platform/windows/scroll_controls.h"
#include "platform/windows/screenshot_scale.h"
#include "platform/windows/settings_store.h"

#include <QAction>
#include <QActionGroup>
#include <QApplication>
#include <QClipboard>
#include <QCoreApplication>
#include <QCursor>
#include <QDateTime>
#include <QDebug>
#include <QDesktopServices>
#include <QDir>
#include <QFileInfo>
#include <QFileDialog>
#include <QEventLoop>
#include <QFile>
#include <QFont>
#include <QFontDatabase>
#include <QFontInfo>
#include <QFontMetrics>
#include <QGuiApplication>
#include <QHBoxLayout>
#include <QIcon>
#include <QKeyEvent>
#include <QLabel>
#include <QMenu>
#include <QLocalServer>
#include <QLocalSocket>
#include <QMimeData>
#include <QMessageBox>
#include <QMouseEvent>
#include <QPainter>
#include <QPainterPath>
#include <QPointer>
#include <QProcess>
#include <QPushButton>
#include <QRegion>
#include <QScreen>
#include <QSettings>
#include <QStandardPaths>
#include <QTextStream>
#include <QSystemTrayIcon>
#include <QTimer>
#include <QToolButton>
#include <QUrl>
#include <QVBoxLayout>
#include <QWidget>

#include <algorithm>
#include <cmath>
#include <functional>
#include <utility>

namespace {

// ---------------------------------------------------------------- tokens ---
// Verbatim from capture-overlay/src/CaptureModeToolbar.cpp,
// CaptureOverlay_p.h, CaptureOverlay_Paint.cpp and CaptureOverlay_Layout.cpp.
const QColor kPanel(20, 20, 20, 250);
const QColor kAccent(255, 102, 0);
const QColor kText(245, 245, 247);
const QColor kCard(18, 18, 18, 235);
const QColor kRim(255, 255, 255, 26);
const QColor kDim(0, 0, 0, 140);

constexpr double kHandleLength = 20.0;
constexpr double kHandleThickness = 2.5;
constexpr double kTopBarY = 16.0;
constexpr double kTopBarH = 46.0;
constexpr double kTopBarAspectH = 28.0;
constexpr double kTopBarBtnSize = 32.0;
constexpr double kFeaturePanelMargin = 16.0;
constexpr double kTopBarRadius = 10.0;

const char* kPillLabels[4] = {"Free", "16:9", "4:3", "1:1"};
const double kAspectRatios[4] = {0.0, 16.0 / 9.0, 4.0 / 3.0, 1.0};

// Crop menu rows, verbatim from CaptureOverlay_ToolbarDrawing.cpp.
enum CropRow {
    CropRowReset = 0,
    CropRowFree = 1,
    CropRow1_1 = 2,
    CropRow2_1 = 3,
    CropRow3_2 = 4,
    CropRow4_3 = 5,
    CropRow9_16 = 6,
    CropRow16_9 = 7,
    CropRow16_10 = 8,
    CropRow21_9 = 9,
    CropRowSnap = 10,
    CropRowCount = 11
};

struct CropOption {
    const char* label;
    double ratio;
};

constexpr CropOption kCropOptions[CropRowCount] = {
    {"Reset selection", 0.0},
    {"Free", 0.0},
    {"1:1", 1.0},
    {"2:1", 2.0},
    {"3:2", 3.0 / 2.0},
    {"4:3", 4.0 / 3.0},
    {"9:16", 9.0 / 16.0},
    {"16:9", 16.0 / 9.0},
    {"16:10", 16.0 / 10.0},
    {"21:9", 21.0 / 9.0},
    {"Snap to ratios", 0.0},
};

constexpr double kCropMenuW = 200.0;
constexpr double kCropItemH = 34.0;
constexpr double kCropMenuPadding = 4.0;

bool ratiosEqual(double a, double b)
{
    return std::abs(a - b) < 0.01;
}

// ---------------------------------------------------------------- fonts -----
QString uiFamily()
{
    static const QString family = [] {
        const QStringList families = QFontDatabase().families();
        for (const QString& candidate :
             {QStringLiteral("DM Sans"), QStringLiteral("Inter"), QStringLiteral("Segoe UI")}) {
            if (families.contains(candidate))
                return candidate;
        }
        return QStringLiteral("Segoe UI");
    }();
    return family;
}

QString areaIconFamily()
{
    static const QString family = [] {
        const int id = QFontDatabase::addApplicationFont(QStringLiteral(":/fonts/Lucide.ttf"));
        if (id < 0)
            return QString();
        const QStringList families = QFontDatabase::applicationFontFamilies(id);
        return families.isEmpty() ? QString() : families.first();
    }();
    return family;
}

void drawAreaIcon(QPainter& painter, ushort glyph, const QRectF& rect, const QColor& color)
{
    const QString family = areaIconFamily();
    if (family.isEmpty())
        return;
    QFont font(family);
    font.setPixelSize(18);
    QPainterPath path;
    path.addText(0, 0, font, QString(QChar(glyph)));
    const QRectF bounds = path.boundingRect();
    painter.save();
    painter.translate(rect.center() - bounds.center());
    painter.fillPath(path, color);
    painter.restore();
}

void registerBundledFonts()
{
    const QString fontDir = QStringLiteral(APEXSHOT_FONT_DIR);
    int loaded = 0;
    for (const QString& file : {QStringLiteral("DMSans-Regular.ttf"),
                                QStringLiteral("DMSans-Medium.ttf"),
                                QStringLiteral("DMSans-SemiBold.ttf"),
                                QStringLiteral("DMSans-Bold.ttf"),
                                QStringLiteral("Inter-Regular.ttf"),
                                QStringLiteral("Inter-Medium.ttf"),
                                QStringLiteral("Inter-SemiBold.ttf"),
                                QStringLiteral("Inter-Bold.ttf")}) {
        if (QFontDatabase::addApplicationFont(QDir(fontDir).filePath(file)) >= 0)
            ++loaded;
    }

    QFile log(QDir(QDir::tempPath()).filePath(QStringLiteral("apexshot-quickaccess.log")));
    if (log.open(QIODevice::Append | QIODevice::WriteOnly)) {
        log.write(QStringLiteral("fonts: %1/8 from %2; DM Sans present=%3; family=%4\n")
                      .arg(loaded)
                      .arg(fontDir)
                      .arg(QFontDatabase().families().contains(QStringLiteral("DM Sans")) ? 1 : 0)
                      .arg(uiFamily())
                      .toUtf8());
    }
}

QFont uiFont(int pixelSize, bool bold = false)
{
    QFont font(uiFamily());
    font.setPixelSize(pixelSize);
    font.setWeight(bold ? QFont::Medium : QFont::Normal);
    font.setHintingPreference(QFont::PreferNoHinting);
    font.setLetterSpacing(QFont::PercentageSpacing, 98.0);
    return font;
}

void debugLog(const QString& text)
{
    QFile log(QDir(QDir::tempPath()).filePath(QStringLiteral("apexshot-quickaccess.log")));
    if (log.open(QIODevice::Append | QIODevice::WriteOnly)) {
        log.write(text.toUtf8());
        log.write("\n");
    }
}

/// How much the display is scaled, so the menu matches the Settings windows
/// instead of shrinking on a high-DPI screen. Qt reports the display's own DPI
/// here rather than dividing it out, and its pixel sizes are raw device pixels.
qreal uiScale()
{
    const QScreen* screen = QGuiApplication::primaryScreen();
    const qreal dpi = screen ? screen->logicalDotsPerInch() : 96.0;
    return dpi > 0.0 ? dpi / 96.0 : 1.0;
}

/// The overlay chrome grows with the display by the same factor.
qreal chromeScale()
{
    return uiScale();
}

/// The tray menu, in the Settings window's dropdown dress: the #252526 body, a
/// 10px body radius over a 1px rim, 6px rows, and a whisper of white on hover
/// instead of an accent fill, so a menu reads the same in either surface.
///
/// Every length is a 100%-display value multiplied by the screen scale, since
/// the shell draws in device pixels: the Settings sheet's 14px base becomes the
/// same physical size here at any scaling.
QString trayMenuStyle()
{
    const qreal scale = uiScale();
    const auto px = [scale](double value) {
        return QString::number(qRound(value * scale));
    };
    return QStringLiteral(
               "QMenu { background:#252526; color:rgba(255,255,255,0.90); "
               "border:1px solid rgba(255,255,255,0.10); border-radius:%1px; "
               // A submenu opens at the item's right edge, so a slimmer right
               // margin is what lets it sit flush against the body instead of
               // swallowing the rounded edge.
               "padding:%2px %13px %2px %2px; } "
               "QMenu::item { padding:%3px %4px; border-radius:%5px; "
               "font-size:%6px; font-weight:500; } "
               "QMenu::item:selected { background:rgba(255,255,255,0.07); color:#ffffff; } "
               "QMenu::item:disabled { color:rgba(255,255,255,0.35); } "
               "QMenu::separator { height:1px; margin:%7px %8px; "
               "background:rgba(255,255,255,0.08); } "
               "QMenu::indicator { width:%9px; height:%9px; margin-left:%10px; } "
               "QMenu::indicator:checked { image:url(:/icons/check.png); } "
               "QMenu::right-arrow { image:url(:/icons/chevron-right.png); "
               "width:%11px; height:%11px; margin-right:%12px; }")
        .arg(px(10))   // body radius
        .arg(px(4))    // body padding
        .arg(px(9))    // row height above and below the label
        .arg(px(14))   // row inset
        .arg(px(6))    // row radius
        .arg(px(14))   // label size, the Settings sheet's base
        .arg(px(7))    // separator margin, vertical
        .arg(px(9))    // separator margin, horizontal
        .arg(px(16))   // check indicator
        .arg(px(5))    // check indicator inset
        .arg(px(13))   // submenu arrow
        .arg(px(7))    // submenu arrow inset
        .arg(px(2));   // body margin on the submenu side
}

/// A rounded body needs the popup itself to be translucent and undecorated.
/// Qt cascades the stylesheet into a submenu but not these window properties,
/// which is why a submenu paints square corners under a rounded parent.
void decoratePopupMenu(QMenu* menu, const QString& style, const QFont& font)
{
    menu->setFont(font);
    menu->setStyleSheet(style);
    menu->setAttribute(Qt::WA_TranslucentBackground);
    menu->setWindowFlags(menu->windowFlags() | Qt::FramelessWindowHint
                         | Qt::NoDropShadowWindowHint);
}

/// Write a capture using the folder, format, and HiDPI suffix from Settings.
/// A capture the user asked not to keep still gets a temporary file so the
/// result card can open it and the file-URL clipboard mode still works.
QString saveCapture(const QImage& image, const QRect& region,
                    const Platform::ShellSettings& settings, QString& error)
{
    const QString directory =
        settings.screenshots.afterSave
            ? Platform::shellScreenshotOutputDir(settings)
            : QDir(QDir::tempPath()).filePath(QStringLiteral("ApexShot"));

    QString path;
    if (!Platform::saveScreenshotImage(image,
                                      settings,
                                      directory,
                                      Platform::screenshotScale(region, Platform::monitors()),
                                      path,
                                      error)) {
        debugLog(QStringLiteral("save failed: %1").arg(error));
        return QString();
    }
    return path;
}

QString videoOutputPath(const Platform::ShellSettings& settings)
{
    const QString path = settings.recording.afterSave
        ? Platform::shellRecordingOutputPath(settings, QDateTime::currentDateTime())
        : Platform::shellTemporaryRecordingOutputPath();
    if (!path.isEmpty())
        QDir().mkpath(QFileInfo(path).absolutePath());
    return path;
}

constexpr int kDefaultRecordingCountdown = 3;

int recordingCountdownSeconds()
{
    if (!Platform::loadShellSettings().recording.showCountdown)
        return 0;

    QSettings settings;
    const int seconds =
        settings.value(QStringLiteral("recording/countdown"), kDefaultRecordingCountdown).toInt();
    switch (seconds) {
    case 0:
    case 3:
    case 5:
    case 10:
        return seconds;
    default:
        return kDefaultRecordingCountdown;
    }
}

void setRecordingCountdownSeconds(int seconds)
{
    QSettings settings;
    settings.setValue(QStringLiteral("recording/countdown"), seconds);
}

QString settingsExecutable()
{
    QString executable = qEnvironmentVariable("APEXSHOT_SETTINGS");
    if (executable.isEmpty()) {
        const QString sibling =
            QDir(QCoreApplication::applicationDirPath())
                .filePath(QStringLiteral("apexshot-settings.exe"));
        if (QFileInfo::exists(sibling))
            executable = sibling;
    }
    if (executable.isEmpty())
        executable = QStringLiteral("apexshot-settings.exe");
    return executable;
}

/// Hand a capture to the Rust helper, which owns the Cloud transport. The
/// helper exits after the request and its notification.
bool startCloudUpload(const QString& path)
{
    if (path.isEmpty())
        return false;
    return QProcess::startDetached(settingsExecutable(),
                                   {QStringLiteral("--upload"),
                                    QDir::toNativeSeparators(path)});
}

void openSettingsWindow()
{
    const QString executable = settingsExecutable();

    if (!QProcess::startDetached(executable, {})) {
        QMessageBox::warning(
            nullptr,
            QStringLiteral("ApexShot Settings"),
            QStringLiteral("Could not start the settings window:\n%1\n\n"
                           "Set APEXSHOT_SETTINGS to the settings executable.")
                .arg(QDir::toNativeSeparators(executable)));
    }
}

void openHistoryWindow()
{
    QString executable = qEnvironmentVariable("APEXSHOT_HISTORY");
    if (executable.isEmpty()) {
        const QString sibling =
            QDir(QCoreApplication::applicationDirPath())
                .filePath(QStringLiteral("apexshot-history.exe"));
        if (QFileInfo::exists(sibling))
            executable = sibling;
    }
    if (executable.isEmpty())
        executable = QStringLiteral("apexshot-history.exe");

    if (!QProcess::startDetached(executable, {})) {
        QMessageBox::warning(
            nullptr,
            QStringLiteral("ApexShot History"),
            QStringLiteral("Could not start the history window:\n%1\n\n"
                           "Set APEXSHOT_HISTORY to the history executable.")
                .arg(QDir::toNativeSeparators(executable)));
    }
}

void openVideoEditorWindow(const QString& path = QString())
{
    QString executable = qEnvironmentVariable("APEXSHOT_VIDEO_EDITOR");
    if (executable.isEmpty()) {
        const QString sibling =
            QDir(QCoreApplication::applicationDirPath())
                .filePath(QStringLiteral("apexshot-video-editor.exe"));
        if (QFileInfo::exists(sibling))
            executable = sibling;
    }
    if (executable.isEmpty())
        executable = QStringLiteral("apexshot-video-editor.exe");

    const QStringList arguments = path.isEmpty() ? QStringList() : QStringList{path};
    if (!QProcess::startDetached(executable, arguments)) {
        QMessageBox::warning(
            nullptr,
            QStringLiteral("ApexShot Video Editor"),
            QStringLiteral("Could not start the video editor:\n%1\n\n"
                           "Set APEXSHOT_VIDEO_EDITOR to the editor executable.")
                .arg(QDir::toNativeSeparators(executable)));
    }
}

/// The updater owns the release lookup, its daily cache, and the window. It is
/// quiet when there is nothing to offer and when it is not installed beside
/// the shell.
void startUpdateCheck()
{
    QString executable = qEnvironmentVariable("APEXSHOT_UPDATE");
    if (executable.isEmpty()) {
        const QString sibling =
            QDir(QCoreApplication::applicationDirPath())
                .filePath(QStringLiteral("apexshot-update.exe"));
        if (QFileInfo::exists(sibling))
            executable = sibling;
    }
    if (executable.isEmpty()) {
        debugLog(QStringLiteral("update check: no updater beside the shell"));
        return;
    }
    const bool started = QProcess::startDetached(executable, {QStringLiteral("--check")});
    debugLog(QStringLiteral("update check: %1 started=%2")
                 .arg(QDir::toNativeSeparators(executable))
                 .arg(started));
}

void openImageEditorWindow(const QString& path = QString())
{
    QString executable = qEnvironmentVariable("APEXSHOT_IMAGE_EDITOR");
    if (executable.isEmpty()) {
        const QString sibling =
            QDir(QCoreApplication::applicationDirPath())
                .filePath(QStringLiteral("apexshot-image-editor.exe"));
        if (QFileInfo::exists(sibling))
            executable = sibling;
    }
    if (executable.isEmpty())
        executable = QStringLiteral("apexshot-image-editor.exe");

    const QStringList arguments = path.isEmpty() ? QStringList() : QStringList{path};
    if (!QProcess::startDetached(executable, arguments)) {
        QMessageBox::warning(
            nullptr,
            QStringLiteral("ApexShot Image Editor"),
            QStringLiteral("Could not start the image editor:\n%1\n\n"
                           "Set APEXSHOT_IMAGE_EDITOR to the editor executable.")
                .arg(QDir::toNativeSeparators(executable)));
    }
}

QIcon appIcon()
{
    // The tray wears the application's own mark - the same icon the installer
    // puts on the Start-menu shortcut - instead of a redraw of it.
    const QIcon bundled(QStringLiteral(":/icons/apexshot.ico"));
    if (!bundled.availableSizes().isEmpty()) {
        debugLog(QStringLiteral("tray icon: %1 sizes from the bundled ico")
                     .arg(bundled.availableSizes().size()));
        return bundled;
    }

    QPixmap pixmap(64, 64);
    pixmap.fill(Qt::transparent);

    QPainter painter(&pixmap);
    painter.setRenderHint(QPainter::Antialiasing);
    painter.setPen(Qt::NoPen);
    painter.setBrush(kAccent);
    painter.drawRoundedRect(QRectF(2, 2, 60, 60), 14, 14);

    painter.setBrush(Qt::NoBrush);
    painter.setPen(QPen(Qt::white, 4.0, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
    painter.drawRoundedRect(QRectF(18, 22, 28, 20), 4, 4);
    painter.drawEllipse(QPointF(37, 30), 3.0, 3.0);
    painter.drawLine(QPointF(22, 38), QPointF(29, 31));
    painter.drawLine(QPointF(29, 31), QPointF(34, 36));
    painter.end();

    return QIcon(pixmap);
}

void paintPanel(QPainter& painter, const QRectF& rect, const QColor& fill, qreal radius)
{
    painter.setRenderHint(QPainter::Antialiasing, true);

    QPainterPath shadow;
    shadow.addRoundedRect(rect.translated(0, 3), radius, radius);
    painter.fillPath(shadow, QColor(0, 0, 0, 105));

    QPainterPath body;
    body.addRoundedRect(rect, radius, radius);
    painter.fillPath(body, fill);
    painter.setPen(QPen(kRim, 1.2));
    painter.setBrush(Qt::NoBrush);
    painter.drawPath(body);
}

QString sizeLabel(const QImage& image, const QString& path, const QSize& originalSize = QSize())
{
    const QSize size = originalSize.isValid() ? originalSize : image.size();
    const qint64 bytes = path.isEmpty() ? 0 : QFileInfo(path).size();
    return QStringLiteral("%1 \u00D7 %2   -   %3 KB")
        .arg(size.width())
        .arg(size.height())
        .arg(bytes / 1024);
}

// ------------------------------------------------------------- top bar ------
struct TopBarLayout {
    QRectF bar;
    QRectF label;
    QRectF pills[4];
    QRectF buttons[2];
    bool valid = false;
};

TopBarLayout topBarLayout(double screenW)
{
    // Mirrors CaptureOverlay::computeTopBarLayout, but the label column is
    // measured instead of fixed: Inter 4.1 is a few pixels wider than the
    // Inter builds the Linux layout was tuned against, and a fixed 108px rect
    // clips "Draw an area".
    constexpr double padX = 14.0;
    constexpr double minLabelW = 108.0;
    constexpr double sepGap = 12.0;
    constexpr double sepW = 1.0;
    constexpr double pillGap = 6.0;
    constexpr double btnGap = 6.0;
    constexpr double pillW[4] = {52.0, 52.0, 48.0, 44.0};

    static const QFont labelFont = [] {
        QFont font(uiFamily());
        font.setPixelSize(14);
        font.setWeight(QFont::Medium);
        font.setHintingPreference(QFont::PreferNoHinting);
        font.setLetterSpacing(QFont::PercentageSpacing, 98.0);
        return font;
    }();
    static const double labelW = std::max(
        minLabelW, QFontMetrics(labelFont).horizontalAdvance(QStringLiteral("Draw an area")) + 10.0);

    const double pillsW = pillW[0] + pillW[1] + pillW[2] + pillW[3] + pillGap * 3;
    const double btnsW = kTopBarBtnSize * 2 + btnGap;
    const double totalW = padX * 2 + labelW + sepGap * 2 + sepW + sepGap * 2 + sepW
        + pillsW + btnsW;

    TopBarLayout layout;
    if (totalW > screenW - kFeaturePanelMargin * 2)
        return layout;

    const double barX = std::max(kFeaturePanelMargin, (screenW - totalW) / 2.0);
    layout.bar = QRectF(barX, kTopBarY, totalW, kTopBarH);

    double cx = barX + padX;
    layout.label = QRectF(cx, kTopBarY, labelW, kTopBarH);
    cx += labelW + sepGap + sepW + sepGap;

    const double pillY = kTopBarY + (kTopBarH - kTopBarAspectH) / 2.0;
    for (int i = 0; i < 4; ++i) {
        layout.pills[i] = QRectF(cx, pillY, pillW[i], kTopBarAspectH);
        cx += pillW[i] + pillGap;
    }

    cx -= pillGap;
    cx += sepGap + sepW + sepGap;
    const double btnY = kTopBarY + (kTopBarH - kTopBarBtnSize) / 2.0;
    for (int i = 0; i < 2; ++i) {
        layout.buttons[i] = QRectF(cx, btnY, kTopBarBtnSize, kTopBarBtnSize);
        cx += kTopBarBtnSize + btnGap;
    }

    layout.valid = true;
    return layout;
}

// --------------------------------------------------------- list popup -------
/// Dropdown copied from CaptureOverlay::drawDropdownPopup: 34px rows,
/// (20,20,20,250) body, radius 8, hover (255,255,255,20), accent (176,92,56).
// ------------------------------------------------------- area selector ------
/// Frozen-frame area selection, painted with the overlay's own rules.
class AreaSelector : public QWidget
{
public:
    /// The image is the cropped selection; the rectangle is that selection in
    /// frozen-image pixels, which the recorder maps back to desktop pixels.
    using Done = std::function<void(const QImage&, const QRect&)>;
    enum class Mode { Area, Crosshair };

    AreaSelector(QImage frozen, Mode mode, Done done)
        : m_frozen(std::move(frozen))
        , m_mode(mode)
        , m_done(std::move(done))
    {
        // A tool window, not a normal one: Windows would otherwise raise a
        // taskbar button for the seconds the selection is up, which is the
        // flash of a second app every capture used to show.
        setWindowFlags(Qt::Tool | Qt::FramelessWindowHint | Qt::WindowStaysOnTopHint);
        setCursor(Qt::CrossCursor);
        setAttribute(Qt::WA_DeleteOnClose, true);
        setMouseTracking(true);
        setFocusPolicy(Qt::StrongFocus);
    }

protected:
    void paintEvent(QPaintEvent*) override
    {
        QPainter painter(this);
        painter.setRenderHint(QPainter::SmoothPixmapTransform, true);
        painter.drawImage(rect(), m_frozen);

        // Crosshair mode: no dim, no chrome, just the frozen frame and a live
        // coordinate bubble. Release captures immediately.
        if (m_mode == Mode::Crosshair) {
            if (m_hasSelection) {
                const QRect selection = m_selection.normalized();
                painter.setPen(QPen(QColor(255, 102, 0, 240), 2.0));
                painter.setBrush(Qt::NoBrush);
                painter.drawRect(selection.adjusted(0, 0, -1, -1));
            }
            const double scale = chromeScale();
            painter.save();
            painter.scale(scale, scale);
            drawCrosshairBubble(painter);
            painter.restore();
            return;
        }

        const QRect sel = m_selection.normalized();

        if (m_hasSelection) {
            // Dim outside the selection, exactly as CaptureOverlay_Paint does.
            const QColor dim(0, 0, 0, 140);
            if (sel.y() > 0)
                painter.fillRect(QRect(0, 0, width(), sel.y()), dim);
            if (sel.bottom() < height() - 1)
                painter.fillRect(QRect(0, sel.bottom() + 1, width(), height() - sel.bottom() - 1), dim);
            if (sel.x() > 0)
                painter.fillRect(QRect(0, sel.y(), sel.x(), sel.height()), dim);
            if (sel.right() < width() - 1)
                painter.fillRect(QRect(sel.right() + 1, sel.y(), width() - sel.right() - 1, sel.height()), dim);

            painter.setPen(QPen(QColor(255, 102, 0, 240), 2.0));
            painter.setBrush(Qt::NoBrush);
            painter.drawRect(sel.adjusted(0, 0, -1, -1));

            // Corner brackets and edge midpoint ticks.
            painter.setBrush(Qt::NoBrush);
            painter.setPen(QPen(QColor(255, 102, 0, 245),
                                kHandleThickness,
                                Qt::SolidLine,
                                Qt::RoundCap,
                                Qt::RoundJoin));
            const double half = kHandleLength / 2.0;
            auto corner = [&](double x, double y, double dx, double dy) {
                QPainterPath path;
                path.moveTo(x, y + dy * half);
                path.lineTo(x, y);
                path.lineTo(x + dx * half, y);
                painter.drawPath(path);
            };
            corner(sel.left(), sel.top(), +1, +1);
            corner(sel.right(), sel.top(), -1, +1);
            corner(sel.left(), sel.bottom(), +1, -1);
            corner(sel.right(), sel.bottom(), -1, -1);

            const double midX = sel.left() + sel.width() / 2.0;
            const double midY = sel.top() + sel.height() / 2.0;
            painter.drawLine(QPointF(midX - half, sel.top()), QPointF(midX + half, sel.top()));
            painter.drawLine(QPointF(midX - half, sel.bottom()), QPointF(midX + half, sel.bottom()));
            painter.drawLine(QPointF(sel.left(), midY - half), QPointF(sel.left(), midY + half));
            painter.drawLine(QPointF(sel.right(), midY - half), QPointF(sel.right(), midY + half));

            const double scale = chromeScale();
            painter.save();
            painter.scale(scale, scale);
            drawSizeBubble(painter, sel);
            painter.restore();
        } else {
            painter.fillRect(rect(), QColor(0, 0, 0, 60));
        }

        const double scale = chromeScale();
        painter.save();
        painter.scale(scale, scale);
        drawTopBar(painter);
        painter.restore();
    }

    void mousePressEvent(QMouseEvent* event) override
    {
        if (event->button() != Qt::LeftButton) {
            finish(QImage());
            return;
        }

        if (m_mode == Mode::Crosshair) {
            m_origin = event->pos();
            m_selection = QRect(m_origin, QSize());
            m_hasSelection = false;
            m_dragging = true;
            update();
            return;
        }

        if (m_cropMenuOpen) {
            const int row = cropItemAt((QPointF(event->pos()) / chromeScale()).toPoint());
            if (row >= 0) {
                applyCropRow(row);
                return;
            }
            m_cropMenuOpen = false;
            m_hoveredCropItem = -1;
            update();
            return;
        }

        const QPointF pointer = QPointF(event->pos()) / chromeScale();
        const TopBarLayout layout = topBarLayout(width() / chromeScale());
        if (layout.valid) {
            for (int i = 0; i < 4; ++i) {
                if (layout.pills[i].contains(pointer)) {
                    m_ratio = kAspectRatios[i];
                    applyRatioToSelection();
                    update();
                    return;
                }
            }
            if (layout.buttons[0].contains(pointer)) {
                m_cropMenuOpen = true;
                m_hoveredCropItem = -1;
                debugLog(QStringLiteral("crop menu opened"));
                update();
                return;
            }
            if (layout.buttons[1].contains(pointer)) {
                finish(QImage());
                return;
            }
        }

        m_origin = event->pos();
        m_selection = QRect(m_origin, QSize());
        m_hasSelection = false;
        update();
    }

    void mouseMoveEvent(QMouseEvent* event) override
    {
        m_pointer = (QPointF(event->pos()) / chromeScale()).toPoint();

        if (m_mode == Mode::Crosshair) {
            if (m_dragging) {
                m_selection = QRect(m_origin, event->pos()).normalized();
                m_hasSelection = m_selection.width() > 1 && m_selection.height() > 1;
            }
            update();
            return;
        }

        if (m_cropMenuOpen) {
            const int row = cropItemAt((QPointF(event->pos()) / chromeScale()).toPoint());
            if (row != m_hoveredCropItem) {
                m_hoveredCropItem = row;
                update();
            }
            return;
        }

        if (!(event->buttons() & Qt::LeftButton)) {
            update();
            return;
        }

        QRect rect = QRect(m_origin, event->pos()).normalized();
        if (m_ratio > 0.0) {
            const int w = rect.width();
            const int h = qRound(w / m_ratio);
            rect.setHeight(h);
        }
        m_selection = rect.intersected(this->rect());
        m_hasSelection = m_selection.width() > 1 && m_selection.height() > 1;
        update();
    }

    void mouseReleaseEvent(QMouseEvent* event) override
    {
        if (event->button() != Qt::LeftButton)
            return;

        if (m_mode == Mode::Crosshair) {
            if (!m_dragging)
                return;
            m_dragging = false;
            if (m_selection.width() < 8 || m_selection.height() < 8) {
                finish(QImage());
                return;
            }
            finish(croppedImage());
            return;
        }
        if (!m_hasSelection || m_selection.width() < 8 || m_selection.height() < 8) {
            m_hasSelection = false;
            update();
            return;
        }
        if (m_snapToRatios)
            snapToNearestRatio();
        finish(croppedImage());
    }

    void keyPressEvent(QKeyEvent* event) override
    {
        if (event->key() == Qt::Key_Escape)
            finish(QImage());
    }

private:
    void drawSizeBubble(QPainter& painter, const QRect& sel)
    {
        QFont font(uiFamily());
        font.setPixelSize(13);
        font.setWeight(QFont::Medium);
        font.setHintingPreference(QFont::PreferNoHinting);
        font.setLetterSpacing(QFont::PercentageSpacing, 98.0);
        painter.setFont(font);

        const QString label = QStringLiteral("%1 \u00D7 %2").arg(sel.width()).arg(sel.height());
        const QFontMetrics metrics(font);
        const QSize textSize = metrics.size(Qt::TextSingleLine, label);

        // The numbers are capture pixels; the box around them is chrome and
        // grows with the display, so it is laid out in logical units.
        const qreal scale = chromeScale();
        const QRectF logical(sel.x() / scale, sel.y() / scale,
                             sel.width() / scale, sel.height() / scale);
        QRectF bubble(logical.left(), qMax<qreal>(4, logical.top() - textSize.height() - 12),
                      textSize.width() + 14, textSize.height() + 8);

        QPainterPath path;
        path.addRoundedRect(bubble, 8, 8);
        painter.fillPath(path, kPanel);
        painter.setPen(QPen(kRim, 1.0));
        painter.setBrush(Qt::NoBrush);
        painter.drawPath(path);
        painter.setPen(kText);
        painter.drawText(bubble, Qt::AlignCenter, label);
    }

    /// Crosshair bubble: "x, y" while idle, "W × H" once dragging. Geometry
    /// and styling follow crosshairBubbleRectForPoint.
    void drawCrosshairBubble(QPainter& painter)
    {
        const qreal scale = chromeScale();
        QString label;
        if (m_dragging || m_hasSelection) {
            const QRect sel = m_selection.normalized();
            label = QStringLiteral("%1 \u00D7 %2").arg(sel.width()).arg(sel.height());
        } else {
            label = QStringLiteral("%1, %2")
                        .arg(qRound(m_pointer.x() * scale))
                        .arg(qRound(m_pointer.y() * scale));
        }

        QFont font(uiFamily());
        font.setPixelSize(13);
        font.setWeight(QFont::Medium);
        font.setHintingPreference(QFont::PreferNoHinting);
        font.setLetterSpacing(QFont::PercentageSpacing, 98.0);
        painter.setFont(font);

        const QFontMetrics metrics(font);
        const QRect textRect = metrics.boundingRect(label);
        const int bw = textRect.width() + 20;
        const int bh = textRect.height() + 12;
        const qreal screenW = width() / scale;
        const qreal screenH = height() / scale;
        const QRectF bubble(std::clamp(m_pointer.x() + 16.0, 8.0, screenW - bw - 8.0),
                            std::clamp(m_pointer.y() + 16.0, 8.0, screenH - bh - 8.0),
                            bw,
                            bh);

        QPainterPath path;
        path.addRoundedRect(bubble, 8, 8);
        painter.fillPath(path, kPanel);
        painter.setPen(QPen(kRim, 1.0));
        painter.setBrush(Qt::NoBrush);
        painter.drawPath(path);
        painter.setPen(kText);
        painter.drawText(bubble, Qt::AlignCenter, label);
    }

    void drawTopBar(QPainter& painter)
    {
        const TopBarLayout layout = topBarLayout(width() / chromeScale());
        if (!layout.valid)
            return;

        painter.save();
        painter.setRenderHint(QPainter::Antialiasing);

        QPainterPath shadow;
        shadow.addRoundedRect(layout.bar.translated(0, 3), kTopBarRadius, kTopBarRadius);
        painter.fillPath(shadow, QColor(0, 0, 0, 105));

        QPainterPath body;
        body.addRoundedRect(layout.bar, kTopBarRadius, kTopBarRadius);
        painter.fillPath(body, kPanel);
        painter.setPen(QPen(kRim, 1.0));
        painter.setBrush(Qt::NoBrush);
        painter.drawPath(body);

        QFont labelFont(uiFamily());
        labelFont.setPixelSize(14);
        labelFont.setWeight(QFont::Medium);
        labelFont.setHintingPreference(QFont::PreferNoHinting);
        labelFont.setLetterSpacing(QFont::PercentageSpacing, 98.0);
        painter.setFont(labelFont);
        painter.setPen(kText);
        painter.drawText(layout.label, Qt::AlignVCenter | Qt::AlignLeft,
                         QStringLiteral("Draw an area"));

        painter.setPen(QPen(QColor(255, 255, 255, 30), 1.0));
        const double sep1 = layout.label.right() + 12.0;
        const double sep2 = layout.pills[3].right() + 12.0;
        painter.drawLine(QPointF(sep1, layout.bar.top() + 10),
                         QPointF(sep1, layout.bar.bottom() - 10));
        painter.drawLine(QPointF(sep2, layout.bar.top() + 10),
                         QPointF(sep2, layout.bar.bottom() - 10));

        for (int i = 0; i < 4; ++i) {
            const QRectF pill = layout.pills[i];
            const bool active = std::abs(m_ratio - kAspectRatios[i]) < 0.001;
            const bool hovered = pill.contains(m_pointer);

            QPainterPath path;
            path.addRoundedRect(pill, 8, 8);
            if (active)
                painter.fillPath(path, QColor(255, 102, 0, 205));
            else if (hovered)
                painter.fillPath(path, QColor(255, 255, 255, 20));
            else
                painter.fillPath(path, QColor(255, 255, 255, 12));

            QFont font(uiFamily());
            font.setPixelSize(13);
            font.setWeight(active || hovered ? QFont::Medium : QFont::Normal);
            font.setHintingPreference(QFont::PreferNoHinting);
            font.setLetterSpacing(QFont::PercentageSpacing, 98.0);
            painter.setFont(font);
            painter.setPen(active ? QColor(255, 255, 255) : QColor(232, 232, 236));
            painter.drawText(pill, Qt::AlignCenter, QString::fromLatin1(kPillLabels[i]));
        }

        // Crop and cancel, the shared 18px Lucide set.
        for (int i = 0; i < 2; ++i) {
            const QRectF button = layout.buttons[i];
            const bool hovered = button.contains(m_pointer);
            const bool cropMenu = (i == 0 && m_cropMenuOpen);
            QPainterPath path;
            path.addRoundedRect(button, 8, 8);
            if (cropMenu || hovered)
                painter.fillPath(path, QColor(255, 255, 255, 20));

            drawAreaIcon(painter, i == 0 ? 0xE0AB : 0xE1B2, button, QColor(236, 236, 240));
        }

        drawCropMenu(painter);

        painter.restore();
    }

    /// Crop dropdown, copied from CaptureOverlay::drawTopBarCropMenu.
    void drawCropMenu(QPainter& painter)
    {
        if (!m_cropMenuOpen)
            return;

        const TopBarLayout layout = topBarLayout(width() / chromeScale());
        if (!layout.valid)
            return;

        const QRectF menu = cropMenuRect(layout);
        const double menuX = menu.x();
        const double menuY = menu.y();
        const double menuW = menu.width();
        const double menuH = menu.height();

        QPainterPath shadow;
        shadow.addRoundedRect(QRectF(menuX, menuY + 3.0, menuW, menuH),
                              kTopBarRadius, kTopBarRadius);
        painter.fillPath(shadow, QColor(0, 0, 0, 105));

        QPainterPath body;
        body.addRoundedRect(QRectF(menuX, menuY, menuW, menuH), kTopBarRadius, kTopBarRadius);
        // The tray menu's dress, so the two menus a capture shows read as one.
        painter.fillPath(body, QColor(37, 37, 38));
        painter.setPen(QPen(kRim, 1.0));
        painter.setBrush(Qt::NoBrush);
        painter.drawPath(body);

        for (int i = 0; i < CropRowCount; ++i) {
            const QRectF itemRect(menuX + kCropMenuPadding,
                                  menuY + kCropMenuPadding + i * kCropItemH,
                                  menuW - kCropMenuPadding * 2,
                                  kCropItemH);
            const QRectF indicatorRect(itemRect.x() + 10.0, itemRect.y(), 16.0, itemRect.height());
            const QRectF labelRect(itemRect.x() + 32.0, itemRect.y(),
                                   itemRect.width() - 42.0, itemRect.height());

            if (i == m_hoveredCropItem) {
                QPainterPath hover;
                hover.addRoundedRect(itemRect, 6, 6);
                painter.fillPath(hover, QColor(255, 255, 255, 18));
            }

            bool checked = false;
            if (i == CropRowFree)
                checked = m_ratio <= 0.0;
            else if (i >= CropRow1_1 && i <= CropRow21_9)
                checked = ratiosEqual(m_ratio, kCropOptions[i].ratio);
            else if (i == CropRowSnap)
                checked = m_snapToRatios;

            if (checked && i != CropRowReset) {
                painter.setPen(QPen(QColor(255, 255, 255), 1.5));
                painter.setBrush(Qt::NoBrush);
                const double cy = indicatorRect.center().y();
                painter.drawLine(QPointF(indicatorRect.x() + 3.5, cy),
                                 QPointF(indicatorRect.x() + 6.5, cy + 3.0));
                painter.drawLine(QPointF(indicatorRect.x() + 6.5, cy + 3.0),
                                 QPointF(indicatorRect.x() + 12.5, cy - 4.0));
            }

            QFont itemFont(uiFont(14));
            itemFont.setWeight(checked || i == CropRowReset ? QFont::Medium : QFont::Normal);
            painter.setFont(itemFont);
            painter.setPen(checked ? QColor(255, 255, 255) : QColor(230, 230, 232));
            painter.drawText(labelRect, Qt::AlignVCenter | Qt::AlignLeft,
                             QString::fromLatin1(kCropOptions[i].label));
        }

        painter.setPen(QPen(QColor(255, 255, 255, 20), 1.0));
        const double y1 = menuY + kCropMenuPadding + kCropItemH;
        painter.drawLine(QPointF(menuX + 9.0, y1), QPointF(menuX + menuW - 9.0, y1));
        const double y2 = menuY + kCropMenuPadding + CropRowSnap * kCropItemH;
        painter.drawLine(QPointF(menuX + 12.0, y2), QPointF(menuX + menuW - 12.0, y2));
    }

    QRectF cropMenuRect(const TopBarLayout& layout) const
    {
        const QRectF anchor = layout.buttons[0];
        const double menuH = CropRowCount * kCropItemH + kCropMenuPadding * 2;
        const double screenW = width() / chromeScale();
        const double menuX = std::max(
            10.0, std::min(anchor.center().x() - kCropMenuW / 2.0, screenW - kCropMenuW - 10.0));
        const double menuY = std::max(layout.bar.bottom() + 4.0, anchor.bottom() + 8.0);
        return QRectF(menuX, menuY, kCropMenuW, menuH);
    }

    int cropItemAt(const QPoint& point) const
    {
        const TopBarLayout layout = topBarLayout(width());
        if (!layout.valid)
            return -1;
        const QRectF menu = cropMenuRect(layout);
        if (!menu.contains(point))
            return -1;
        const int index = static_cast<int>(
            (point.y() - menu.top() - kCropMenuPadding) / kCropItemH);
        return (index >= 0 && index < CropRowCount) ? index : -1;
    }

    void applyCropRow(int row)
    {
        if (row == CropRowReset) {
            // The selection only exists while the pointer is down, so the row
            // earns its place by dropping the aspect lock too: the next drag
            // starts free.
            m_selection = QRect();
            m_hasSelection = false;
            m_ratio = 0.0;
        } else if (row == CropRowSnap) {
            m_snapToRatios = !m_snapToRatios;
        } else if (row > CropRowSnap) {
            return;
        } else {
            m_ratio = kCropOptions[row].ratio;
            applyRatioToSelection();
        }
        m_cropMenuOpen = false;
        m_hoveredCropItem = -1;
        update();
    }

    void snapToNearestRatio()
    {
        if (!m_hasSelection)
            return;
        const double current = static_cast<double>(m_selection.width())
            / std::max(1, m_selection.height());
        double best = 0.0;
        double bestDelta = 1e9;
        for (int i = CropRow1_1; i <= CropRow21_9; ++i) {
            const double delta = std::abs(kCropOptions[i].ratio - current);
            if (delta < bestDelta) {
                bestDelta = delta;
                best = kCropOptions[i].ratio;
            }
        }
        if (best <= 0.0)
            return;
        m_ratio = best;
        applyRatioToSelection();
    }

    void applyRatioToSelection()
    {
        if (!m_hasSelection || m_ratio <= 0.0)
            return;
        QRect rect = m_selection.normalized();
        const int height = qRound(rect.width() / m_ratio);
        if (height > 0)
            rect.setHeight(height);
        m_selection = rect.intersected(this->rect());
        m_hasSelection = !m_selection.isEmpty();
    }

    QRect selectionInImage() const
    {
        if (width() <= 0 || height() <= 0)
            return {};
        const qreal scaleX = static_cast<qreal>(m_frozen.width()) / width();
        const qreal scaleY = static_cast<qreal>(m_frozen.height()) / height();
        return QRect(qRound(m_selection.x() * scaleX),
                     qRound(m_selection.y() * scaleY),
                     qRound(m_selection.width() * scaleX),
                     qRound(m_selection.height() * scaleY))
            .intersected(m_frozen.rect());
    }

    QImage croppedImage() const
    {
        const QRect inImage = selectionInImage();
        if (inImage.isEmpty())
            return {};
        return m_frozen.copy(inImage);
    }

    void finish(const QImage& result)
    {
        const QRect selection = selectionInImage();
        if (result.isNull())
            Platform::releaseShutterSound();
        if (m_done)
            m_done(result, selection);
        close();
    }

    QImage m_frozen;
    Mode m_mode = Mode::Area;
    Done m_done;
    QPoint m_origin;
    QPoint m_pointer;
    QRect m_selection;
    bool m_hasSelection = false;
    bool m_dragging = false;
    double m_ratio = 0.0;
    bool m_cropMenuOpen = false;
    int m_hoveredCropItem = -1;
    bool m_snapToRatios = false;
};

// ------------------------------------------------------ countdown pill ------
/// Capture-delay countdown, painted like the overlay's pill and held above the
/// topmost window so it stays visible while other windows are opened.
class CountdownPill : public QWidget
{
public:
    CountdownPill(int seconds, std::function<void()> onFinished)
        : m_remaining(seconds)
        , m_onFinished(std::move(onFinished))
    {
        setWindowFlags(Qt::Tool | Qt::FramelessWindowHint | Qt::WindowStaysOnTopHint);
        setAttribute(Qt::WA_TranslucentBackground, true);
        setAttribute(Qt::WA_ShowWithoutActivating, true);
        setAttribute(Qt::WA_DeleteOnClose, true);
        setFixedSize(112, 44);
        setCursor(Qt::PointingHandCursor);
        setToolTip(QStringLiteral("Click to cancel"));

        if (QScreen* screen = QGuiApplication::primaryScreen()) {
            const QRect work = screen->availableGeometry();
            move(work.center().x() - width() / 2, work.top() + 28);
        }

        m_timer.setInterval(1000);
        connect(&m_timer, &QTimer::timeout, this, [this] {
            if (--m_remaining > 0) {
                keepOnTop();
                update();
                return;
            }
            m_timer.stop();
            auto finished = m_onFinished;
            m_onFinished = nullptr;
            close();
            if (finished)
                QTimer::singleShot(80, qApp, std::move(finished));
        });
        m_timer.start();
    }

protected:
    void paintEvent(QPaintEvent*) override
    {
        QPainter painter(this);
        painter.setRenderHint(QPainter::Antialiasing);

        painter.setPen(Qt::NoPen);
        painter.setBrush(m_hovered ? QColor(200, 60, 40, 242) : QColor(233, 84, 32, 235));
        painter.drawRoundedRect(QRectF(rect()), height() / 2.0, height() / 2.0);

        const double cx = 22.0;
        const double cy = height() / 2.0;
        painter.setPen(QPen(Qt::white, 2.2, Qt::SolidLine, Qt::RoundCap));
        painter.setBrush(Qt::NoBrush);
        painter.drawEllipse(QPointF(cx, cy), 11.0, 11.0);
        painter.drawLine(QPointF(cx, cy), QPointF(cx, cy - 5.5));
        painter.drawLine(QPointF(cx, cy), QPointF(cx + 5.0, cy + 2.0));

        QFont font(uiFamily());
        font.setWeight(QFont::Medium);
        font.setHintingPreference(QFont::PreferNoHinting);
        font.setLetterSpacing(QFont::PercentageSpacing, 98.0);
        // 22px fills most of the 44px pill; the hovered Cancel label sits at
        // the 14px body size the rest of the capture chrome uses.
        font.setPixelSize(m_hovered ? 14 : 22);
        painter.setFont(font);
        painter.setPen(Qt::white);

        // Centred in the space right of the clock face, and swapped for a
        // Cancel label on hover, as the overlay does.
        const QRectF textRect(40.0, 0.0, width() - 44.0, height());
        painter.drawText(textRect,
                         Qt::AlignCenter,
                         m_hovered ? QStringLiteral("Cancel")
                                   : QString::number(m_remaining));
    }

    void mousePressEvent(QMouseEvent*) override
    {
        m_timer.stop();
        m_onFinished = nullptr;
        close();
    }

    void enterEvent(QEvent*) override
    {
        m_hovered = true;
        update();
    }

    void leaveEvent(QEvent*) override
    {
        m_hovered = false;
        update();
    }

    void showEvent(QShowEvent*) override { keepOnTop(); }

private:
    void keepOnTop()
    {
        raise();
        Platform::keepTopmost(static_cast<quintptr>(winId()));
    }

    int m_remaining = 0;
    bool m_hovered = false;
    QTimer m_timer;
    std::function<void()> m_onFinished;
};

// --------------------------------------------------------- result card ------
class ResultCard : public QWidget
{
public:
    ResultCard(const QImage& image, const QString& path, const QString& ocrText,
               const QString& hintText, const QSize& originalSize = QSize())
    {
        setWindowFlags(Qt::Tool | Qt::FramelessWindowHint | Qt::WindowStaysOnTopHint);
        setAttribute(Qt::WA_TranslucentBackground, true);
        setAttribute(Qt::WA_DeleteOnClose, true);

        const Platform::ShellSettings settings = Platform::loadShellSettings();
        const qreal scale = settings.quickAccess.overlayScale;
        const int previewWidth = qMax(180, qRound(420 * scale));
        const int previewHeight = qMax(100, qRound(240 * scale));

        auto* layout = new QVBoxLayout(this);
        layout->setContentsMargins(24, 22, 24, 24);
        layout->setSpacing(8);

        auto* thumb = new QLabel;
        QImage preview = image;
        // A scrolling capture is much taller than it is wide; showing all of
        // it in the card turns into a hairline. Preview its first screenful.
        if (image.height() > image.width() * 2) {
            const int previewHeightPixels =
                std::min(image.height(), std::max(1, image.width() * 9 / 16));
            preview = image.copy(0, 0, image.width(), previewHeightPixels);
        }
        thumb->setPixmap(QPixmap::fromImage(preview).scaled(
            previewWidth, previewHeight, Qt::KeepAspectRatio, Qt::SmoothTransformation));
        thumb->setAlignment(Qt::AlignCenter);
        layout->addWidget(thumb);

        auto* info = new QLabel(sizeLabel(image, path, originalSize));
        info->setFont(uiFont(13));
        info->setStyleSheet(QStringLiteral("color:#a4a4ac;"));
        layout->addWidget(info);

        auto* hint = new QLabel(hintText);
        hint->setFont(uiFont(13, true));
        hint->setStyleSheet(QStringLiteral("color:#7fd18b;"));
        layout->addWidget(hint);

        if (!ocrText.isEmpty()) {
            auto* text = new QLabel(ocrText);
            text->setWordWrap(true);
            text->setFont(uiFont(13));
            text->setTextInteractionFlags(Qt::TextSelectableByMouse);
            text->setMaximumHeight(140);
            text->setStyleSheet(QStringLiteral(
                "color:#e8e8ec; background:rgba(255,255,255,14); "
                "border-radius:8px; padding:8px;"));
            layout->addWidget(text);
        }

        auto* row = new QHBoxLayout;
        row->setSpacing(8);

        auto* open = new QToolButton;
        open->setObjectName(QStringLiteral("primary"));
        open->setText(QStringLiteral("Open"));
        open->setEnabled(!path.isEmpty());
        open->setCursor(Qt::PointingHandCursor);
        open->setFont(uiFont(13, true));
        connect(open, &QToolButton::clicked, this, [path] {
            QDesktopServices::openUrl(QUrl::fromLocalFile(path));
        });
        row->addWidget(open);

        auto* folder = new QToolButton;
        folder->setText(QStringLiteral("Show in folder"));
        folder->setEnabled(!path.isEmpty());
        folder->setCursor(Qt::PointingHandCursor);
        folder->setFont(uiFont(13, true));
        connect(folder, &QToolButton::clicked, this, [path] {
            QDesktopServices::openUrl(QUrl::fromLocalFile(QFileInfo(path).absolutePath()));
        });
        row->addWidget(folder);

        if (!path.isEmpty() && settings.cloud.configured) {
            auto* upload = new QToolButton;
            upload->setText(QStringLiteral("Upload"));
            upload->setCursor(Qt::PointingHandCursor);
            upload->setFont(uiFont(13, true));
            const bool closeAfter = settings.quickAccess.closeAfterUploading;
            connect(upload, &QToolButton::clicked, this, [this, path, closeAfter] {
                if (startCloudUpload(path) && closeAfter)
                    close();
            });
            row->addWidget(upload);
        }

        row->addStretch();

        auto* dismiss = new QToolButton;
        dismiss->setObjectName(QStringLiteral("danger"));
        dismiss->setText(QStringLiteral("Dismiss"));
        dismiss->setCursor(Qt::PointingHandCursor);
        dismiss->setFont(uiFont(13, true));
        connect(dismiss, &QToolButton::clicked, this, &QWidget::close);
        row->addWidget(dismiss);

        layout->addLayout(row);

        setStyleSheet(QStringLiteral(
            "QToolButton { color:#f5f5f7; background:transparent; border:none; "
            "border-radius:10px; padding:7px 16px; } "
            "QToolButton#primary { background:#f3f4f6; color:#111111; } "
            "QToolButton#primary:hover { background:#ffffff; } "
            "QToolButton#danger { color:#ffffff; background:rgba(232,17,35,190); "
            "border-radius:999px; } "
            "QToolButton#danger:hover { background:#e81123; }"));

        adjustSize();

        QScreen* screen = QGuiApplication::screenAt(QCursor::pos());
        if (!screen)
            screen = QGuiApplication::primaryScreen();
        if (screen) {
            const QRect work = screen->availableGeometry();
            const int x = settings.quickAccess.position == QLatin1String("Left")
                ? work.left() + 20
                : work.right() - width() - 20;
            move(x, work.top() + 20);
        }

        if (settings.quickAccess.autoCloseEnabled) {
            QTimer::singleShot(settings.quickAccess.autoCloseSeconds * 1000,
                               this,
                               &QWidget::close);
        }
    }

protected:
    void paintEvent(QPaintEvent*) override
    {
        QPainter painter(this);
        paintPanel(painter, QRectF(rect()).adjusted(0.5, 0.5, -0.5, -0.5), kCard, 16);
    }
};

// --------------------------------------------------------------- shell ------
struct CaptureOptions {
    int timerSeconds = 0;
    quintptr monitorId = 0;
    bool ocr = false;
    bool recording = false;
    bool microphone = false;
    QString microphoneDevice;
    Platform::RecordingRequest::SystemAudio systemAudio =
        Platform::RecordingRequest::SystemAudio::Off;
    quint32 systemAudioProcessId = 0;
    QString systemAudioApplication;
    bool cameraEnabled = false;
    QString cameraDevice;
    bool scroll = false;
};

class Shell : public QObject
{
public:
    Shell()
    {
        m_telemetry = new Platform::UsageTelemetry(this);
        m_telemetry->start();
        m_lastScreenshotPath = QSettings().value(QStringLiteral("shortcuts/last_screenshot")).toString();
        if (!m_lastScreenshotPath.isEmpty())
            m_lastScreenshot.load(m_lastScreenshotPath);
    }

    void setTray(QSystemTrayIcon* tray) { m_tray = tray; }

    /// A system notification, for an outcome the window is not going to show.
    ///
    /// The result card after a capture is the main feedback, so this is what
    /// speaks when the card is turned off: without it a capture that copied,
    /// saved, or read text would look like nothing happened.
    ///
    /// The shell already heads the notification with the app name, and the
    /// tray icon supplies the body's icon, so the default title is empty;
    /// only a caller with something more specific to say supplies one.
    void notify(const QString& body, const QString& title = QString())
    {
        if (m_tray)
            m_tray->showMessage(title, body, m_tray->icon(), 6000);
    }

    /// The shell has no Cloud transport; Settings mirrors whether auto-upload
    /// is on and a destination is configured, and the Rust helper does the
    /// request and reports the result.
    void maybeAutoUpload(const QString& path, const Platform::ShellSettings& settings)
    {
        if (path.isEmpty() || !settings.cloud.autoUploadAfterCapture
            || !settings.cloud.configured)
            return;
        if (!QProcess::startDetached(settingsExecutable(),
                                     {QStringLiteral("--auto-upload"),
                                      QDir::toNativeSeparators(path)}))
            debugLog(QStringLiteral("auto upload could not start"));
    }

    void openMenu()
    {
        if (m_menuOpen)
            return;
        m_menuOpen = true;

        QString recordingError;
        const bool recordingAvailable = Platform::RecordingSession::available(&recordingError);
        // Quick Access has no recording panel or tray-first scroll entry, so
        // the toolbar exposes both Area recording and the Scroll mode. Its
        // Mic, Speaker, and Webcam items are configured by right-clicking
        // them, and start from the devices the last recording used.
        const CaptureOptions defaultsOptions = recordingDefaults();
        CaptureModeToolbar::DeviceDefaults defaults;
        defaults.screenshotTimerSeconds = Platform::loadShellSettings().screenshots.timerInterval;
        defaults.timedSelection = true;
        defaults.microphone = defaultsOptions.microphone;
        defaults.microphoneDevice = defaultsOptions.microphoneDevice;
        defaults.speaker =
            defaultsOptions.systemAudio != Platform::RecordingRequest::SystemAudio::Off;
        defaults.systemAudioApplication = defaultsOptions.systemAudioApplication;
        defaults.systemAudioProcessId = defaultsOptions.systemAudioProcessId;
        defaults.camera = defaultsOptions.cameraEnabled;
        defaults.cameraDevice = defaultsOptions.cameraDevice;
        const CaptureModeToolbar::Result result = CaptureModeToolbar::choose(
            nullptr, nullptr, recordingAvailable, true, true, defaults, true,
            [this](bool enabled, const QString& device) {
                updateWebcamPreview(enabled, device);
            });

        m_menuOpen = false;

        CaptureOptions options;
        options.timerSeconds = result.timerSeconds;
        options.ocr = result.ocr;
        options.recording = result.recording;
        options.microphone = result.microphone;
        options.microphoneDevice = result.microphoneDevice;
        if (result.speaker && !result.systemAudioApplication.isEmpty()) {
            options.systemAudio = Platform::RecordingRequest::SystemAudio::Application;
            options.systemAudioProcessId = result.systemAudioProcessId;
            options.systemAudioApplication = result.systemAudioApplication;
        } else if (result.speaker) {
            options.systemAudio = Platform::RecordingRequest::SystemAudio::AllApps;
        } else {
            options.systemAudio = Platform::RecordingRequest::SystemAudio::Off;
        }
        options.cameraEnabled = result.camera;
        options.cameraDevice = result.cameraDevice;
        options.scroll = result.scroll;
        if (!options.recording || result.action == CaptureModeToolbar::Action::Cancel)
            stopWebcamPreview(true);
        debugLog(QStringLiteral("toolbar: action=%1 recording=%2 toolbar=%3 scroll=%4")
                     .arg(static_cast<int>(result.action))
                     .arg(options.recording ? 1 : 0)
                     .arg(result.toolbar ? 1 : 0)
                     .arg(options.scroll ? 1 : 0));
        if (options.recording && !recordingAvailable) {
            stopWebcamPreview(true);
            if (result.toolbar)
                result.toolbar->close();
            debugLog(QStringLiteral("recording unavailable: %1").arg(recordingError));
            return;
        }
        if (result.toolbar && result.action != CaptureModeToolbar::Action::Window)
            result.toolbar->close();

        switch (result.action) {
        case CaptureModeToolbar::Action::Display:
            // Display is disabled in Scroll mode; it only becomes reachable
            // here when the mode toggle is off.
            if (options.scroll)
                break;
            options.monitorId = Platform::monitorAt(QCursor::pos()).id;
            withCountdown(options, [this, options] { captureDisplay(options); });
            break;
        case CaptureModeToolbar::Action::Window:
            if (options.recording) {
                chooseWindowRecording(options, result.toolbar);
            } else {
                if (result.toolbar)
                    result.toolbar->close();
                chooseWindowCapture(options);
            }
            break;
        case CaptureModeToolbar::Action::Area:
            if (QGuiApplication::keyboardModifiers() & Qt::ShiftModifier) {
                captureCrosshair(options);
            } else {
                captureArea(options);
            }
            break;
        case CaptureModeToolbar::Action::Cancel:
            break;
        }
    }

    void demoArea()
    {
        Platform::CaptureResult frozen;
        QString error;
        const bool ok = Platform::captureVirtualDesktop(false, frozen, error);
        debugLog(QStringLiteral("demoArea: ok=%1 err=%2 image=%3x%4 monitors=%5")
                     .arg(ok ? 1 : 0)
                     .arg(error)
                     .arg(frozen.image.width())
                     .arg(frozen.image.height())
                     .arg(Platform::monitors().size()));
        if (ok)
            startArea(frozen, AreaSelector::Mode::Area, CaptureOptions{});
    }

    void demoCrosshair()
    {
        Platform::CaptureResult frozen;
        QString error;
        if (Platform::captureVirtualDesktop(false, frozen, error))
            startArea(frozen, AreaSelector::Mode::Crosshair, CaptureOptions{});
    }

    void shortcutCaptureArea() { captureArea(screenshotDefaults()); }
    void shortcutCaptureCrosshair() { captureCrosshair(screenshotDefaults()); }
    void shortcutCaptureFullscreen()
    {
        CaptureOptions options = screenshotDefaults();
        options.monitorId = Platform::monitorAt(QCursor::pos()).id;
        withCountdown(options, [this, options] { captureDisplay(options); });
    }

    void shortcutCapturePreviousArea()
    {
        const QRect region = QSettings().value(QStringLiteral("shortcuts/previous_area")).toRect();
        QRegion displays;
        for (const auto& monitor : Platform::monitors())
            displays += monitor.geometry;
        if (region.isEmpty() || !displays.contains(region)) {
            debugLog(QStringLiteral("previous area is not available on the current displays"));
            notify(QStringLiteral("No previous capture area is available."));
            return;
        }
        const CaptureOptions options = screenshotDefaults();
        withCountdown(options, [this, region, options] {
            Platform::CaptureResult capture;
            QString error;
            if (Platform::captureRegion(region,
                                        Platform::loadShellSettings().screenshots.showCursor,
                                        capture,
                                        error)) {
                Platform::playShutterSound();
                present(capture.image, options, capture.geometry);
            } else {
                debugLog(QStringLiteral("previous area capture failed: %1").arg(error));
            }
        });
    }

    void shortcutShowLastPreview()
    {
        if (m_lastScreenshot.isNull()) {
            notify(QStringLiteral("No screenshot is available to preview."));
            return;
        }
        auto* card = new ResultCard(m_lastScreenshot, m_lastScreenshotPath, m_lastScreenshotText,
                                    QStringLiteral("Last screenshot"));
        card->show();
        card->raise();
        card->activateWindow();
    }

    void shortcutSaveLastScreenshot()
    {
        if (m_lastScreenshot.isNull()) {
            notify(QStringLiteral("No screenshot is available to save."));
            return;
        }
        if (m_saveDialogOpen)
            return;
        const QImage screenshot = m_lastScreenshot;
        const Platform::ShellSettings settings = Platform::loadShellSettings();
        m_saveDialogOpen = true;
        QString filter = settings.screenshots.format == QLatin1String("JPEG")
            ? QStringLiteral("JPEG image (*.jpg *.jpeg)")
            : settings.screenshots.format == QLatin1String("WebP")
                ? QStringLiteral("WebP image (*.webp)")
                : QStringLiteral("PNG image (*.png)");
        QString path = QFileDialog::getSaveFileName(
            nullptr,
            QStringLiteral("Save screenshot"),
            QDir(Platform::shellScreenshotOutputDir(settings)).filePath(QStringLiteral("ApexShot")),
            QStringLiteral("PNG image (*.png);;JPEG image (*.jpg *.jpeg);;WebP image (*.webp)"),
            &filter);
        m_saveDialogOpen = false;
        if (!path.isEmpty() && QFileInfo(path).suffix().isEmpty()) {
            path += filter.startsWith(QLatin1String("JPEG")) ? QStringLiteral(".jpg")
                : filter.startsWith(QLatin1String("WebP")) ? QStringLiteral(".webp")
                                                           : QStringLiteral(".png");
            if (QFileInfo::exists(path)
                && QMessageBox::question(nullptr, QStringLiteral("Save screenshot"),
                                         QStringLiteral("Replace the existing file?\n%1").arg(path))
                    != QMessageBox::Yes)
                return;
        }
        if (!path.isEmpty() && !screenshot.save(path))
            QMessageBox::warning(nullptr, QStringLiteral("Save screenshot"),
                                 QStringLiteral("Could not save the screenshot."));
    }

    bool shortcutInteractionOpen() const
    {
        return m_menuOpen || m_saveDialogOpen || !m_screenshotCountdown.isNull()
            || !m_areaSelector.isNull()
            || !m_windowPicker.isNull() || !m_setupOverlay.isNull();
    }

    void demoOcr()
    {
        CaptureOptions options;
        options.ocr = true;
        captureDisplay(options);
    }

    void demoTimer()
    {
        CaptureOptions options;
        options.timerSeconds = 3;
        withCountdown(options, [this] { captureDisplay(CaptureOptions{}); });
    }

    void demoWindows()
    {
        chooseWindowCapture(CaptureOptions{});
    }

    void recordScreen()
    {
        CaptureOptions options = recordingDefaults();
        captureDisplay(options);
    }

    void recordArea()
    {
        CaptureOptions options = recordingDefaults();
        captureArea(options);
    }

    void recordWindow()
    {
        chooseWindowRecording(recordingDefaults());
    }

    void scrollWindow()
    {
        CaptureOptions options;
        options.scroll = true;
        chooseWindowCapture(options);
    }

    void scrollArea()
    {
        CaptureOptions options;
        options.scroll = true;
        captureArea(options);
    }

    void demoScrollWindow()
    {
        const QString needle =
            qEnvironmentVariable("APEXSHOT_SCROLL_TITLE", QStringLiteral("Notepad"));
        for (const Platform::WindowInfo& window : Platform::windows()) {
            if (!window.title.contains(needle, Qt::CaseInsensitive))
                continue;
            const QRect client = Platform::clientRect(window.id);
            startScrollCapture(client.isEmpty() ? window.geometry : client, window.id);
            QTimer::singleShot(30000, this, [this] {
                if (m_scrollSession)
                    m_scrollSession->stop();
            });
            return;
        }
        debugLog(QStringLiteral("scroll demo: no window matching '%1'").arg(needle));
    }

    void demoRecord()
    {
        CaptureOptions options;
        options.recording = true;
        captureDisplay(options);
        QTimer::singleShot((recordingCountdownSeconds() + 3) * 1000, this, [this] {
            if (m_activeSession)
                m_activeSession->stop();
        });
    }

    void demoRecordArea()
    {
        CaptureOptions options;
        options.recording = true;
        const QRect region(360, 220, 900, 520);
        Platform::CaptureResult still;
        QString error;
        Platform::captureRegion(region, false, still, error);
        startRecording(region,
                       still.image,
                       options,
                       Platform::RecordingRequest::Target::Area);
        QTimer::singleShot((recordingCountdownSeconds() + 5) * 1000, this, [this] {
            if (m_activeSession)
                m_activeSession->stop();
        });
    }

    /// Write the audio inventory and a two-second capture to a log, so the
    /// native paths can be checked without opening the interface.
    void probeAudio()
    {
        QFile log(QDir(QDir::tempPath()).filePath(QStringLiteral("apexshot-audio-probe.log")));
        if (!log.open(QIODevice::WriteOnly | QIODevice::Truncate))
            return;
        QTextStream out(&log);
        out.setCodec("UTF-8");

        const QString defaultMicrophone = Platform::defaultMicrophoneName();
        const QStringList microphones = Platform::directShowDevices(false);
        const QStringList cameras = Platform::directShowDevices(true);
        out << "default microphone: " << defaultMicrophone << Qt::endl;
        out << "microphones (" << microphones.size() << "):" << Qt::endl;
        for (const QString& name : microphones)
            out << "  " << name << Qt::endl;
        out << "cameras (" << cameras.size() << "):" << Qt::endl;
        for (const QString& name : cameras)
            out << "  " << name << Qt::endl;

        const QVector<Platform::AudioApplication> apps = Platform::audioApplications();
        out << "audio apps (" << apps.size() << "):" << Qt::endl;
        for (const Platform::AudioApplication& app : apps) {
            out << "  " << app.name << " pid=" << app.processId
                << (app.active ? " active" : " idle") << Qt::endl;
        }
        out.flush();

        const QString wav =
            QDir(QDir::tempPath()).filePath(QStringLiteral("apexshot-probe-audio.wav"));
        QFile::remove(wav);
        Platform::SystemAudioCapture capture;
        QString error;
        if (capture.start(Platform::SystemAudioCapture::Mode::AllApps, 0, wav, &error)) {
            out << "all-apps capture: started" << Qt::endl;
            QEventLoop loop;
            QTimer::singleShot(2000, &loop, &QEventLoop::quit);
            loop.exec();
            capture.stop();
            out << "all-apps wav bytes: " << QFileInfo(wav).size() << Qt::endl;
        } else {
            out << "all-apps capture failed: " << error << Qt::endl;
        }

        const quint32 processId = apps.isEmpty() ? 0 : apps.first().processId;
        if (processId != 0) {
            const QString appWav =
                QDir(QDir::tempPath()).filePath(QStringLiteral("apexshot-probe-app-audio.wav"));
            QFile::remove(appWav);
            Platform::SystemAudioCapture appCapture;
            if (appCapture.start(Platform::SystemAudioCapture::Mode::Application,
                                 processId,
                                 appWav,
                                 &error)) {
                out << "app capture: started for " << apps.first().name << Qt::endl;
                QEventLoop loop;
                QTimer::singleShot(1500, &loop, &QEventLoop::quit);
                loop.exec();
                appCapture.stop();
                out << "app wav bytes: " << QFileInfo(appWav).size() << Qt::endl;
            } else {
                out << "app capture failed: " << error << Qt::endl;
            }
        }

        Platform::AudioLevelMonitor meter;
        if (meter.listen(defaultMicrophone)) {
            float observed = 0.0f;
            QEventLoop loop;
            for (int i = 0; i < 20; ++i) {
                QTimer::singleShot(100, &loop, &QEventLoop::quit);
                loop.exec();
                observed = std::max(observed, meter.peak());
            }
            meter.stop();
            out << "meter peak: " << observed << Qt::endl;
        } else {
            out << "meter: could not open the default microphone" << Qt::endl;
        }

        out.flush();
        QCoreApplication::quit();
    }

    void demoWindowSetupStart()
    {
        demoWindowSetup();
        QTimer::singleShot(2500, this, [this] {
            if (m_setupOverlay)
                m_setupOverlay->startNow();
        });
        QTimer::singleShot(2500 + 1000 + (recordingCountdownSeconds() + 12) * 1000,
                           this,
                           [this] {
                               if (m_activeSession)
                                   m_activeSession->stop();
                           });
    }

    void demoWindowRecord()
    {
        for (const Platform::WindowInfo& window : Platform::windows()) {
            if (window.minimized || window.geometry.width() < 480
                || window.geometry.height() < 360)
                continue;
            CaptureOptions options = recordingDefaults();
            options.microphone = false;
            options.systemAudio = Platform::RecordingRequest::SystemAudio::AllApps;
            beginWindowRecording(window, options);
            QTimer::singleShot((recordingCountdownSeconds() + 4) * 1000, this, [this] {
                if (m_activeSession)
                    m_activeSession->stop();
            });
            return;
        }
        debugLog(QStringLiteral("demoWindowRecord: no suitable window"));
    }

    void demoWindowSetup()
    {
        for (const Platform::WindowInfo& window : Platform::windows()) {
            if (window.minimized || window.geometry.width() < 480
                || window.geometry.height() < 360)
                continue;
            Platform::activateWindow(window.id);
            CaptureOptions options = recordingDefaults();
            if (!options.cameraEnabled) {
                const QStringList cameras = Platform::directShowDevices(true);
                if (!cameras.isEmpty()) {
                    options.cameraEnabled = true;
                    options.cameraDevice = cameras.first();
                }
            }
            openWindowSetup(window, options);
            return;
        }
        debugLog(QStringLiteral("demoWindowSetup: no suitable window"));
    }

    void demoSystemAudio()
    {
        const Platform::ShellSettings settings = Platform::loadShellSettings();
        const Platform::MonitorInfo monitor = Platform::monitorAt(QCursor::pos());
        Platform::CaptureResult still;
        QString error;
        if (!Platform::captureMonitor(0, false, still, error))
            return;

        CaptureOptions options;
        options.recording = true;
        options.microphone = settings.recording.includeMicrophone;
        options.systemAudio = Platform::RecordingRequest::SystemAudio::AllApps;
        startRecording(monitor.geometry,
                       still.image,
                       options,
                       Platform::RecordingRequest::Target::Screen);
        QTimer::singleShot((recordingCountdownSeconds() + 4) * 1000, this, [this] {
            if (m_activeSession)
                m_activeSession->stop();
        });
    }

    bool hasActiveRecording() const
    {
        return m_activeSession && m_activeSession->isActive();
    }

    /// Save the current recording before the app is allowed to quit.
    void stopActiveRecording()
    {
        if (m_activeSession)
            m_activeSession->stop();
    }

    /// Offer to save recordings whose recorder stopped without finishing:
    /// a crash, a power cut, or a killed process. The studied recorder offers
    /// the same recover-or-discard choice on its next launch.
    void offerRecovery()
    {
        if (m_activeSession || !m_pendingRecoveries.isEmpty())
            return;
        const QVector<Platform::UnfinishedRecording> unfinished =
            Platform::RecordingSession::unfinishedRecordings();
        if (unfinished.isEmpty())
            return;

        QMessageBox box;
        box.setWindowTitle(QStringLiteral("Recording"));
        box.setIcon(QMessageBox::Question);
        box.setText(unfinished.size() == 1
                        ? QStringLiteral("A recording did not finish saving.")
                        : QStringLiteral("%1 recordings did not finish saving.")
                              .arg(unfinished.size()));
        box.setInformativeText(QStringLiteral(
            "ApexShot stopped before it could save them. Recover saves what was captured; "
            "Discard deletes it."));
        QPushButton* recoverButton =
            box.addButton(QStringLiteral("Recover"), QMessageBox::AcceptRole);
        QPushButton* discardButton =
            box.addButton(QStringLiteral("Discard"), QMessageBox::DestructiveRole);
        box.addButton(QStringLiteral("Later"), QMessageBox::RejectRole);
        box.setDefaultButton(recoverButton);
        box.exec();

        if (box.clickedButton() == discardButton) {
            for (const Platform::UnfinishedRecording& item : unfinished)
                Platform::RecordingSession::discardUnfinished(item.workDir);
            return;
        }
        if (box.clickedButton() != recoverButton)
            return;
        m_pendingRecoveries = unfinished;
        recoverNextRecording();
    }

    void recoverNextRecording()
    {
        if (m_pendingRecoveries.isEmpty() || m_activeSession)
            return;
        const Platform::UnfinishedRecording item = m_pendingRecoveries.takeFirst();
        auto* session = new Platform::RecordingSession(this);
        // The recovery holds the recorder like a recording would, so a new
        // recording cannot start while the old one is still being joined.
        m_activeSession = session;
        const Platform::ShellSettings settings = Platform::loadShellSettings();

        connect(session,
                &Platform::RecordingSession::warning,
                this,
                [](const QString& message) {
                    QMessageBox::information(nullptr, QStringLiteral("Recording"), message);
                });
        connect(session,
                &Platform::RecordingSession::finished,
                this,
                [this, session, settings](bool kept, const QString& path, const QString& error) {
                    if (m_activeSession == session)
                        m_activeSession = nullptr;
                    session->deleteLater();
                    if (kept)
                        presentRecording(QImage(), path, settings);
                    else if (!error.isEmpty())
                        QMessageBox::warning(
                            nullptr,
                            QStringLiteral("Recording"),
                            QStringLiteral("The recording could not be recovered: %1").arg(error));
                    QTimer::singleShot(0, this, [this] { recoverNextRecording(); });
                });

        QString error;
        if (!session->recover(item.workDir, error)) {
            if (m_activeSession == session)
                m_activeSession = nullptr;
            session->deleteLater();
            QMessageBox::warning(
                nullptr,
                QStringLiteral("Recording"),
                QStringLiteral("The recording could not be recovered: %1").arg(error));
            QTimer::singleShot(0, this, [this] { recoverNextRecording(); });
        }
    }

private:
    /// Keep the bubble's look and place, so the next preview opens the way this
    /// one was left instead of at the settings window's defaults.
    void rememberBubble(const QRect& region,
                        Platform::WebcamBubble* bubble,
                        bool rememberSize = true)
    {
        if (!bubble || region.isEmpty())
            return;
        const QRect rect = bubble->deviceGeometry();
        if (rect.isEmpty())
            return;
        double relX = 0.0;
        double relY = 0.0;
        if (!Platform::webcamRelativePositionForWidth(region,
                                                     rect.width(),
                                                     bubble->shape(),
                                                     rect,
                                                     &relX,
                                                     &relY))
            return;
        Platform::WebcamPlacement state = Platform::loadWebcamPlacement();
        state.present = true;
        state.relX = relX;
        state.relY = relY;
        // Only the setup bubble sits on the whole display, so only it can say
        // what fraction of the screen the person chose. A drag during a
        // recording keeps the size they set before it started.
        if (rememberSize && region.height() > 0)
            state.sizeFraction = double(rect.width()) / region.height();
        state.shape = bubble->shape();
        state.flip = bubble->flipped();
        Platform::saveWebcamPlacement(state);
    }

    void stopWebcamPreview(bool closeBubble)
    {
        if (m_webcamPreview) {
            delete m_webcamPreview;
            m_webcamPreview = nullptr;
        }
        m_previewDevice.clear();
        if (closeBubble && m_setupWebcamBubble) {
            m_setupWebcamBubble->close();
            m_setupWebcamBubble = nullptr;
        }
    }

    void updateWebcamPreview(bool enabled, const QString& device)
    {
        if (!enabled || device.isEmpty()) {
            stopWebcamPreview(true);
            return;
        }
        if (m_activeSession && m_activeSession->isActive())
            return;
        if (m_webcamPreview && m_previewDevice == device)
            return;

        stopWebcamPreview(false);
        const Platform::ShellSettings settings = Platform::loadShellSettings();
        if (!m_setupWebcamBubble) {
            Platform::ShellSettings::Webcam webcam = settings.recording.webcam;
            const Platform::WebcamPlacement placement = Platform::loadWebcamPlacement();
            if (placement.present) {
                webcam.relX = placement.relX;
                webcam.relY = placement.relY;
                webcam.shapeIndex = placement.shape;
                webcam.flip = placement.flip;
            }
            const QRect region = Platform::monitorAt(QCursor::pos()).geometry;
            if (region.isEmpty())
                return;
            const double width = placement.sizeFraction > 0.0
                ? placement.sizeFraction * region.height()
                : region.height()
                    * Platform::kWebcamSizeFractions[std::clamp(webcam.sizeIndex, 0, 3)];
            m_setupWebcamBubble = new Platform::WebcamBubble(webcam.shapeIndex, webcam.flip);
            m_setupWebcamBubble->placeAt(Platform::webcamBubbleRectForWidth(
                region, width, webcam.shapeIndex, webcam.relX, webcam.relY));
            m_setupWebcamBubble->setMoveBounds(region);
            m_setupWebcamBubble->setSetupMode(true);
            QPointer<Platform::WebcamBubble> setup = m_setupWebcamBubble;
            m_setupWebcamBubble->setMovedHandler(
                [this, setup, region](const QRect&) { rememberBubble(region, setup); });
            m_setupWebcamBubble->show();
            m_setupWebcamBubble->raise();
        } else {
            m_setupWebcamBubble->setFrame(QImage());
        }

        m_previewDevice = device;
        const int fps = Platform::recordingFpsForIndex(settings.recording.values.fpsIndex);
        m_webcamPreview = new Platform::WebcamPreview(device, fps, this);
        QPointer<Platform::WebcamBubble> bubble = m_setupWebcamBubble;
        connect(m_webcamPreview, &Platform::WebcamPreview::frameReady, bubble,
                [bubble](const QImage& frame) {
                    if (bubble)
                        bubble->setFrame(frame);
                });
        connect(m_webcamPreview, &Platform::WebcamPreview::failed, bubble,
                [bubble](const QString& error) {
                    if (bubble)
                        bubble->setPreviewError(error);
                });
        m_webcamPreview->start();
    }

    CaptureOptions recordingDefaults()
    {
        const Platform::ShellSettings settings = Platform::loadShellSettings();
        CaptureOptions options;
        options.recording = true;
        options.microphone = settings.recording.includeMicrophone;
        options.systemAudio = settings.recording.includeSpeaker
            ? Platform::RecordingRequest::SystemAudio::AllApps
            : Platform::RecordingRequest::SystemAudio::Off;
        options.cameraEnabled = settings.recording.webcam.enabled;
        options.cameraDevice = settings.recording.webcam.device;

        // The shell remembers the last devices chosen on the pre-record
        // surface; the Rust settings mirror stays the default for a first run.
        const Platform::RecordingDeviceChoices saved = Platform::loadRecordingDeviceChoices();
        if (saved.present) {
            options.microphone = saved.microphoneEnabled;
            options.microphoneDevice = saved.microphoneDevice;
            if (saved.systemAudioMode == QLatin1String("all")) {
                options.systemAudio = Platform::RecordingRequest::SystemAudio::AllApps;
            } else if (saved.systemAudioMode == QLatin1String("app")) {
                options.systemAudio = Platform::RecordingRequest::SystemAudio::Application;
                options.systemAudioApplication = saved.systemAudioApplication;
                // A per-app choice only means something while that app has an
                // audio session; a screen recording started outside the setup
                // surface resolves the saved name here too.
                for (const Platform::AudioApplication& application :
                     Platform::audioApplications()) {
                    if (application.name.compare(saved.systemAudioApplication,
                                                 Qt::CaseInsensitive)
                        == 0) {
                        options.systemAudioProcessId = application.processId;
                        options.systemAudioApplication = application.name;
                        break;
                    }
                }
                if (options.systemAudioProcessId == 0) {
                    options.systemAudio = Platform::RecordingRequest::SystemAudio::AllApps;
                    options.systemAudioApplication.clear();
                }
            } else {
                options.systemAudio = Platform::RecordingRequest::SystemAudio::Off;
            }
            options.cameraEnabled = saved.cameraEnabled;
            if (!saved.cameraDevice.isEmpty())
                options.cameraDevice = saved.cameraDevice;
        }
        return options;
    }

    /// Keep the devices a recording used as the defaults for the next one.
    void rememberRecordingChoices(const CaptureOptions& options)
    {
        Platform::RecordingDeviceChoices saved;
        saved.microphoneEnabled = options.microphone;
        saved.microphoneDevice = options.microphoneDevice;
        saved.systemAudioMode =
            options.systemAudio == Platform::RecordingRequest::SystemAudio::AllApps
                ? QStringLiteral("all")
                : (options.systemAudio == Platform::RecordingRequest::SystemAudio::Application
                       ? QStringLiteral("app")
                       : QStringLiteral("off"));
        saved.systemAudioApplication = options.systemAudioApplication;
        saved.cameraEnabled = options.cameraEnabled;
        saved.cameraDevice = options.cameraDevice;
        Platform::saveRecordingDeviceChoices(saved);
    }

    /// A window recording starts with the picker: hovering paints the window
    /// and clicking it only selects it. The selected window then carries just
    /// the Start action, so nothing is recorded before it.
    void raiseToolbar(const QPointer<CaptureModeToolbar>& toolbar)
    {
        if (!toolbar)
            return;
        // The picker and the selected window are full-screen topmost windows,
        // so the toolbar has to be raised after each of them is shown.
        toolbar->show();
        toolbar->raise();
        Platform::keepTopmost(static_cast<quintptr>(toolbar->winId()));
        if (m_setupWebcamBubble) {
            m_setupWebcamBubble->raise();
            Platform::keepTopmost(static_cast<quintptr>(m_setupWebcamBubble->winId()));
        }
    }

    void chooseWindowRecording(const CaptureOptions& options,
                               CaptureModeToolbar* toolbar = nullptr)
    {
        QPointer<CaptureModeToolbar> bar(toolbar);
        if (m_activeSession && m_activeSession->isActive()) {
            stopWebcamPreview(true);
            if (bar)
                bar->close();
            debugLog(QStringLiteral("window picker ignored: a recording is running"));
            return;
        }
        if (m_countdownPill) {
            stopWebcamPreview(true);
            if (bar)
                bar->close();
            return;
        }
        if (!m_windowPicker.isNull() || !m_setupOverlay.isNull()) {
            stopWebcamPreview(true);
            if (bar)
                bar->close();
            return;
        }

        if (bar) {
            bar->setCloseHandler([this] {
                if (m_setupOverlay)
                    m_setupOverlay->cancelNow();
                else if (m_windowPicker)
                    m_windowPicker->dismiss();
            });
        }

        auto* picker = new Platform::WindowPicker(
            [this, options, bar](const Platform::WindowInfo& window) {
                Platform::activateWindow(window.id);
                openWindowSetup(window, options, bar);
            },
            [this, bar] {
                if (bar)
                    bar->close();
                stopWebcamPreview(true);
            });
        m_windowPicker = picker;
        picker->show();
        picker->raise();
        picker->activateWindow();
        raiseToolbar(bar);
    }

    void openWindowSetup(const Platform::WindowInfo& window,
                         const CaptureOptions& options,
                         CaptureModeToolbar* toolbar = nullptr)
    {
        QPointer<CaptureModeToolbar> bar(toolbar);
        auto* overlay = new Platform::RecordingSetupOverlay(
            window,
            [this, options, window, bar] {
                CaptureOptions chosen = options;
                if (bar) {
                    // The toolbar stayed open through selection so the last
                    // device changes apply to this recording.
                    const CaptureModeToolbar::DeviceDefaults devices = bar->deviceState();
                    chosen.microphone = devices.microphone;
                    chosen.microphoneDevice = devices.microphoneDevice;
                    if (devices.speaker && !devices.systemAudioApplication.isEmpty()) {
                        chosen.systemAudio =
                            Platform::RecordingRequest::SystemAudio::Application;
                        chosen.systemAudioProcessId = devices.systemAudioProcessId;
                        chosen.systemAudioApplication = devices.systemAudioApplication;
                    } else if (devices.speaker) {
                        chosen.systemAudio =
                            Platform::RecordingRequest::SystemAudio::AllApps;
                    } else {
                        chosen.systemAudio = Platform::RecordingRequest::SystemAudio::Off;
                    }
                    chosen.cameraEnabled = devices.camera;
                    chosen.cameraDevice = devices.cameraDevice;
                    bar->close();
                }
                // Let the overlay leave the screen before the frozen frame is
                // taken, so neither the dim nor the Start button reaches the
                // video or its thumbnail.
                QTimer::singleShot(350, this, [this, window, chosen] {
                    beginWindowRecording(window, chosen);
                });
            },
            [this, bar] {
                if (bar)
                    bar->close();
                stopWebcamPreview(true);
            });
        m_setupOverlay = overlay;
        overlay->show();
        overlay->raise();
        overlay->activateWindow();
        raiseToolbar(bar);
    }

    void beginWindowRecording(const Platform::WindowInfo& window,
                              const CaptureOptions& options)
    {
        // The overlay held the foreground so Escape and the Start pill
        // worked; give the window its active title bar back before its first
        // frame is captured.
        Platform::activateWindow(window.id);

        CaptureOptions chosen = options;

        QRect outer = Platform::windowFrame(window.id);
        if (outer.isEmpty())
            outer = window.geometry;
        // DWM's visible frame carries a thin border and the rounded-corner
        // blend, which show up as the desktop along the edges of the video.
        // Record the pixels inside that border instead.
        constexpr int kFrameInset = 2;
        QRect region = outer;
        if (region.width() > kFrameInset * 2 + 32
            && region.height() > kFrameInset * 2 + 32) {
            region.adjust(kFrameInset, kFrameInset, -kFrameInset, -kFrameInset);
        }

        Platform::CaptureResult capture;
        QString error;
        if (Platform::captureWindow(window.id, false, capture, error)) {
            if (capture.image.size() == outer.size()) {
                capture.image = capture.image.copy(
                    QRect(QPoint(kFrameInset, kFrameInset), region.size()));
                capture.geometry = region;
            }
        } else if (!Platform::captureRegion(region, false, capture, error)) {
            stopWebcamPreview(true);
            QMessageBox::warning(nullptr,
                                 QStringLiteral("Recording"),
                                 QStringLiteral("The window could not be captured: %1")
                                     .arg(error));
            return;
        }
        startRecording(region,
                       capture.image,
                       chosen,
                       Platform::RecordingRequest::Target::Window,
                       window.id);
    }

    CaptureOptions screenshotDefaults() const
    {
        CaptureOptions options;
        options.timerSeconds = Platform::loadShellSettings().screenshots.timerInterval;
        return options;
    }

    void withCountdown(const CaptureOptions& options, std::function<void()> action)
    {
        if (options.timerSeconds <= 0) {
            action();
            return;
        }
        if (m_screenshotCountdown)
            return;
        auto* pill = new CountdownPill(options.timerSeconds, std::move(action));
        m_screenshotCountdown = pill;
        pill->show();
    }

    void captureDisplay(const CaptureOptions& options)
    {
        const Platform::ShellSettings settings = Platform::loadShellSettings();
        Platform::MonitorInfo target = Platform::monitorAt(QCursor::pos());
        const QVector<Platform::MonitorInfo> all = Platform::monitors();
        if (options.monitorId != 0) {
            const auto it = std::find_if(all.cbegin(), all.cend(), [&options](const auto& monitor) {
                return monitor.id == options.monitorId;
            });
            if (it == all.cend()) {
                debugLog(QStringLiteral("selected display is no longer available"));
                return;
            }
            target = *it;
        }
        int index = 0;
        for (int i = 0; i < all.size(); ++i) {
            if (all.at(i).id == target.id) {
                index = i;
                break;
            }
        }

        if (options.recording) {
            Platform::CaptureResult still;
            QString stillError;
            Platform::captureMonitor(index, settings.screenshots.showCursor, still, stillError);
            startRecording(target.geometry,
                           still.image,
                           options,
                           Platform::RecordingRequest::Target::Screen);
            return;
        }

        Platform::CaptureResult capture;
        QString error;
        if (!Platform::captureMonitor(index, settings.screenshots.showCursor, capture, error))
            return;
        Platform::playShutterSound();
        present(capture.image, options, capture.geometry);
    }

    void captureArea(const CaptureOptions& options)
    {
        Platform::CaptureResult frozen;
        QString error;
        const bool includeCursor = !options.recording && !options.scroll
            && options.timerSeconds == 0 && Platform::loadShellSettings().screenshots.showCursor;
        if (!Platform::captureVirtualDesktop(includeCursor, frozen, error)) {
            stopWebcamPreview(true);
            return;
        }
        startArea(frozen, AreaSelector::Mode::Area, options);
    }

    void captureCrosshair(const CaptureOptions& options)
    {
        Platform::CaptureResult frozen;
        QString error;
        const bool includeCursor = !options.recording && !options.scroll
            && options.timerSeconds == 0 && Platform::loadShellSettings().screenshots.showCursor;
        if (!Platform::captureVirtualDesktop(includeCursor, frozen, error)) {
            stopWebcamPreview(true);
            return;
        }
        startArea(frozen, AreaSelector::Mode::Crosshair, options);
    }

    void startArea(const Platform::CaptureResult& frozen, AreaSelector::Mode mode,
                   const CaptureOptions& options)
    {
        if (m_areaSelector)
            return;
        auto* selector =
            new AreaSelector(frozen.image, mode, [this, options, frozen](const QImage& cropped,
                                                                        const QRect& selection) {
                if (selection.isEmpty() || cropped.isNull()) {
                    stopWebcamPreview(true);
                    return;
                }
                const QRect desktop(selection.topLeft() + frozen.geometry.topLeft(),
                                    selection.size());
                if (options.scroll) {
                    startScrollCapture(desktop, 0);
                } else if (options.recording) {
                    startRecording(desktop, cropped, options);
                } else {
                    QSettings().setValue(QStringLiteral("shortcuts/previous_area"), desktop);
                    if (options.timerSeconds <= 0) {
                        // Windows can tell the truth here: the shutter belongs
                        // to the moment the image is taken, not to the freeze
                        // that opens the selection.
                        Platform::playShutterSound();
                        present(cropped, options, desktop);
                    } else {
                        withCountdown(options, [this, desktop, options] {
                            Platform::CaptureResult capture;
                            QString error;
                            if (!Platform::captureRegion(
                                    desktop,
                                    Platform::loadShellSettings().screenshots.showCursor,
                                    capture,
                                    error)) {
                                debugLog(QStringLiteral("timed area capture failed: %1").arg(error));
                                return;
                            }
                            Platform::playShutterSound();
                            present(capture.image, options, capture.geometry);
                        });
                    }
                }
            });
        m_areaSelector = selector;

        QRect virtualGeometry;
        for (const QScreen* screen : QGuiApplication::screens())
            virtualGeometry = virtualGeometry.united(screen->geometry());

        debugLog(QStringLiteral("startArea: screen=%1x%2 image=%3x%4")
                     .arg(virtualGeometry.width())
                     .arg(virtualGeometry.height())
                     .arg(frozen.image.width())
                     .arg(frozen.image.height()));

        selector->setGeometry(virtualGeometry);
        Platform::primeShutterSound();
        selector->show();
        selector->raise();
        selector->activateWindow();
        selector->setFocus();

        debugLog(QStringLiteral("startArea: visible=%1 geom=%2x%3 at %4,%5")
                     .arg(selector->isVisible() ? 1 : 0)
                     .arg(selector->width())
                     .arg(selector->height())
                     .arg(selector->x())
                     .arg(selector->y()));
    }

    /// Window capture and scroll selection: the same full-screen picker the
    /// recording flow uses, stopping at the pick instead of opening a setup
    /// surface. Pointing at a window, or walking the stack with Tab, is the
    /// one way to choose one.
    void chooseWindowCapture(const CaptureOptions& options)
    {
        if (!m_windowPicker.isNull() || !m_setupOverlay.isNull()) {
            debugLog(QStringLiteral("window picker ignored: another picker is up"));
            return;
        }

        const QString action =
            options.scroll ? QStringLiteral("scroll") : QStringLiteral("capture");
        auto* picker = new Platform::WindowPicker(
            [this, options](const Platform::WindowInfo& window) {
                // A window picked from under another comes to the front first,
                // so the capture shows the window the person actually chose.
                Platform::activateWindow(window.id);
                QTimer::singleShot(180, this, [this, options, window] {
                    if (options.scroll) {
                        const QRect client = Platform::clientRect(window.id);
                        startScrollCapture(client.isEmpty() ? window.geometry : client,
                                           window.id);
                        return;
                    }

                    withCountdown(options, [this, options, window] {
                        Platform::CaptureResult capture;
                        QString error;
                        const Platform::ShellSettings settings = Platform::loadShellSettings();
                        if (!Platform::captureWindow(window.id,
                                                     settings.screenshots.showCursor,
                                                     capture,
                                                     error)) {
                            debugLog(QStringLiteral("window capture failed: %1").arg(error));
                            return;
                        }
                        Platform::playShutterSound();
                        present(capture.image, options, capture.geometry);
                    });
                });
            },
            [] {},
            action);
        m_windowPicker = picker;
        picker->show();
        picker->raise();
        picker->activateWindow();
    }

    void startRecording(
        const QRect& region,
        const QImage& thumbnail,
        const CaptureOptions& options,
        Platform::RecordingRequest::Target target = Platform::RecordingRequest::Target::Area,
        quintptr windowId = 0)
    {
        if (region.isEmpty()) {
            stopWebcamPreview(true);
            return;
        }
        if (m_activeSession && m_activeSession->isActive()) {
            stopWebcamPreview(true);
            debugLog(QStringLiteral("recording ignored: a session is already active"));
            return;
        }
        if (m_countdownPill) {
            stopWebcamPreview(true);
            debugLog(QStringLiteral("recording ignored: a countdown is already running"));
            return;
        }

        rememberRecordingChoices(options);

        // The countdown runs after the source is chosen, so the user has time
        // to arrange the screen before the first frame.
        const int seconds = recordingCountdownSeconds();
        if (seconds > 0) {
            m_countdownPill = new RecordingCountdown(
                seconds,
                region,
                [this, region, thumbnail, options, target, windowId] {
                    m_countdownPill = nullptr;
                    beginRecording(region, thumbnail, options, target, windowId);
                });
            m_countdownPill->show();
            return;
        }

        beginRecording(region, thumbnail, options, target, windowId);
    }

    void beginRecording(
        const QRect& region,
        const QImage& thumbnail,
        const CaptureOptions& options,
        Platform::RecordingRequest::Target target,
        quintptr windowId)
    {
        if (m_activeSession && m_activeSession->isActive()) {
            debugLog(QStringLiteral("recording ignored: a session is already active"));
            return;
        }

        // Area and window recordings leave the rest of the desktop dimmed, so
        // the recorded region reads as the live one.
        if (target != Platform::RecordingRequest::Target::Screen && !m_recordingMask) {
            m_recordingMask = new RecordingMask(region);
            m_recordingMask->show();
            m_recordingMask->raise();
        }

        auto* session = new Platform::RecordingSession(this);
        m_activeSession = session;

        const Platform::ShellSettings settings = Platform::loadShellSettings();

        Platform::RecordingRequest request;
        request.target = target;
        request.region = region;
        request.windowId = windowId;
        request.outputPath = videoOutputPath(settings);
        if (request.outputPath.isEmpty()) {
            m_activeSession = nullptr;
            session->deleteLater();
            if (m_recordingMask) {
                m_recordingMask->close();
                m_recordingMask = nullptr;
            }
            stopWebcamPreview(true);
            QMessageBox::warning(nullptr,
                                 QStringLiteral("Recording"),
                                 QStringLiteral("Could not find an available recording filename."));
            return;
        }
        // The recorder writes the pointer track beside the video and the editor
        // draws the cursor from it, so the frames never carry a baked-in
        // pointer that would double up with the one the editor paints.
        request.includeCursor = false;
        request.includeMicrophone = options.microphone;
        request.microphoneDevice = options.microphoneDevice;
        request.systemAudio = options.systemAudio;
        request.systemAudioProcessId = options.systemAudioProcessId;
        request.systemAudioApplication = options.systemAudioApplication;

        // Video settings come from the shared settings store the Rust settings
        // window writes.
        const Platform::RecordingSettings videoSettings = settings.recording.values;
        const QSize maxResolution =
            Platform::recordingMaxResolutionForIndex(videoSettings.maxResolutionIndex);
        request.fps = Platform::recordingFpsForIndex(videoSettings.fpsIndex);
        request.crf = Platform::recordingCrfForQualityIndex(videoSettings.qualityIndex);
        request.maxWidth = maxResolution.width();
        request.maxHeight = maxResolution.height();
        request.monoAudio = videoSettings.monoAudio;
        const qreal displayDpr = Platform::monitorAt(region.center()).dpr;
        request.displayScale = !settings.recording.highDpi && displayDpr > 1.0
            ? 1.0 / displayDpr : 1.0;

        // The camera records to its own track and the editor composites it, so
        // what this window shows is a preview of that layer: it is excluded
        // from capture and leaves no pixels in the recording.
        Platform::ShellSettings::Webcam webcam = settings.recording.webcam;
        // A position the person dragged to last time wins over the default
        // corner, so the camera stays where they left it.
        const Platform::WebcamPlacement placement = Platform::loadWebcamPlacement();
        if (placement.present) {
            webcam.relX = placement.relX;
            webcam.relY = placement.relY;
            webcam.shapeIndex = placement.shape;
            webcam.flip = placement.flip;
        }
        QRect webcamRect;
        QRect previewRect;
        QRect previewRegion;
        if (options.cameraEnabled) {
            request.webcamEnabled = true;
            request.webcamDevice = options.cameraDevice.trimmed();
            if (m_setupWebcamBubble && m_previewDevice == request.webcamDevice) {
                webcam.shapeIndex = m_setupWebcamBubble->shape();
                webcam.flip = m_setupWebcamBubble->flipped();
                previewRect = m_setupWebcamBubble->deviceGeometry();
                previewRegion = Platform::monitorAt(previewRect.center()).geometry;
                webcamRect = target == Platform::RecordingRequest::Target::Window
                    ? Platform::webcamProjectRect(previewRegion, region, previewRect)
                    : Platform::webcamFitRect(region, previewRect);
                if (m_webcamPreview)
                    request.webcamStreamUrl = m_webcamPreview->streamUrl();
                m_webcamBubble = m_setupWebcamBubble;
                m_setupWebcamBubble = nullptr;
                if (request.webcamStreamUrl.isEmpty()) {
                    stopWebcamPreview(false);
                } else {
                    m_activeWebcamPreview = m_webcamPreview;
                    m_webcamPreview = nullptr;
                    m_previewDevice.clear();
                }
            } else {
                stopWebcamPreview(true);
                previewRegion = target == Platform::RecordingRequest::Target::Window
                    ? Platform::monitorAt(region.center()).geometry : region;
                if (previewRegion.isEmpty())
                    previewRegion = region;
                const double width = placement.sizeFraction > 0.0
                    ? placement.sizeFraction * previewRegion.height()
                    : previewRegion.height()
                        * Platform::kWebcamSizeFractions[std::clamp(webcam.sizeIndex, 0, 3)];
                previewRect = Platform::webcamBubbleRectForWidth(previewRegion,
                                                                 width,
                                                                 webcam.shapeIndex,
                                                                 webcam.relX,
                                                                 webcam.relY);
                webcamRect = target == Platform::RecordingRequest::Target::Window
                    ? Platform::webcamProjectRect(previewRegion, region, previewRect)
                    : previewRect;
            }
            request.webcamLayout = Platform::webcamLayoutForRect(
                region, webcamRect, webcam.shapeIndex, webcam.flip);
        } else {
            stopWebcamPreview(true);
        }

        connect(session,
                &Platform::RecordingSession::warning,
                this,
                [](const QString& message) {
                    QMessageBox::information(nullptr, QStringLiteral("Recording"), message);
                });

        connect(session, &Platform::RecordingSession::captureStopped, this, [this] {
            if (m_activeWebcamPreview) {
                delete m_activeWebcamPreview;
                m_activeWebcamPreview = nullptr;
            }
            if (m_webcamBubble)
                m_webcamBubble->hide();
        });

        if (!request.webcamDevice.isEmpty()) {
            if (!m_webcamBubble)
                m_webcamBubble = new Platform::WebcamBubble(request.webcamLayout.shape,
                                                            request.webcamLayout.flip);
            m_webcamBubble->setSetupMode(false);
            m_webcamBubble->placeAt(target == Platform::RecordingRequest::Target::Window
                                        ? previewRect : webcamRect);
            m_webcamBubble->setMoveBounds(target == Platform::RecordingRequest::Target::Window
                                             ? previewRegion : region);
            m_webcamBubble->setMovedHandler(
                [this, region, previewRegion, webcam, target](const QRect& rect) {
                    if (region.isEmpty() || rect.isEmpty())
                        return;
                    if (target == Platform::RecordingRequest::Target::Window) {
                        // A window recording keeps the camera where the person
                        // put it on screen and projects that position into the
                        // window's own frame, so the camera can sit outside it.
                        if (m_activeSession) {
                            m_activeSession->setWebcamLayout(Platform::webcamLayoutForRect(
                                region,
                                Platform::webcamProjectRect(previewRegion, region, rect),
                                std::clamp(webcam.shapeIndex, 0, 3),
                                webcam.flip));
                        }
                        return;
                    }
                    Platform::WebcamLayout layout;
                    layout.x = double(rect.x() - region.x()) / region.width();
                    layout.y = double(rect.y() - region.y()) / region.height();
                    layout.w = double(rect.width()) / region.width();
                    layout.h = double(rect.height()) / region.height();
                    layout.shape = std::clamp(webcam.shapeIndex, 0, 3);
                    layout.flip = webcam.flip;
                    if (m_activeSession)
                        m_activeSession->setWebcamLayout(layout);

                    rememberBubble(region, m_webcamBubble, false);
                });
            if (m_webcamBubble->isVisible()) {
                m_webcamBubble->raise();
                Platform::keepTopmost(static_cast<quintptr>(m_webcamBubble->winId()));
            }
            QPointer<Platform::WebcamBubble> bubble = m_webcamBubble;
            connect(session,
                    &Platform::RecordingSession::webcamFrame,
                    m_webcamBubble,
                    [bubble](const QImage& frame) {
                        if (!bubble)
                            return;
                        bubble->setFrame(frame);
                        // A camera that never answers leaves no bubble at all.
                        if (!bubble->isVisible())
                            bubble->show();
                    });
            const bool streamingPreview = !request.webcamStreamUrl.isEmpty();
            connect(session,
                    &Platform::RecordingSession::stateChanged,
                    m_webcamBubble,
                    [session, bubble, streamingPreview] {
                        if (!bubble)
                            return;
                        if (session->isActive() && (streamingPreview || !session->isPaused())
                            && bubble->hasFrame())
                            bubble->show();
                        else
                            bubble->hide();
                    });
        }

        connect(session,
                &Platform::RecordingSession::finished,
                this,
                [this, session, thumbnail, settings](bool kept,
                                           const QString& path,
                                           const QString& error) {
                    if (m_activeSession == session)
                        m_activeSession = nullptr;
                    if (m_recordingMask) {
                        m_recordingMask->close();
                        m_recordingMask = nullptr;
                    }
                    stopWebcamPreview(true);
                    if (m_activeWebcamPreview) {
                        delete m_activeWebcamPreview;
                        m_activeWebcamPreview = nullptr;
                    }
                    if (m_webcamBubble) {
                        // A frame already queued behind the finished signal
                        // would show the bubble again after this close.
                        disconnect(session, nullptr, m_webcamBubble, nullptr);
                        m_webcamBubble->close();
                        m_webcamBubble = nullptr;
                    }
                    session->deleteLater();
                    if (kept)
                        presentRecording(thumbnail, path, settings);
                    else if (!error.isEmpty())
                        QMessageBox::warning(nullptr, QStringLiteral("Recording"), error);

                    if (m_restartAfterFinish) {
                        auto restart = std::move(m_restartAfterFinish);
                        m_restartAfterFinish = nullptr;
                        QTimer::singleShot(0, this, restart);
                    }
                });

        QString error;
        if (!session->start(request, error)) {
            if (m_activeSession == session)
                m_activeSession = nullptr;
            if (m_recordingMask) {
                m_recordingMask->close();
                m_recordingMask = nullptr;
            }
            stopWebcamPreview(true);
            if (m_activeWebcamPreview) {
                delete m_activeWebcamPreview;
                m_activeWebcamPreview = nullptr;
            }
            if (m_webcamBubble) {
                disconnect(session, nullptr, m_webcamBubble, nullptr);
                m_webcamBubble->close();
                m_webcamBubble = nullptr;
            }
            session->deleteLater();
            QMessageBox::warning(nullptr, QStringLiteral("Recording"), error);
            return;
        }
        if (!session->webcamActive() && m_webcamBubble) {
            m_webcamBubble->close();
            m_webcamBubble = nullptr;
        }

        debugLog(QStringLiteral(
                     "recording: %1,%2 %3x%4 mic=%5 speaker=%6 %7fps crf=%8 max=%9x%10 mono=%11 "
                     "-> %12")
                     .arg(region.x())
                     .arg(region.y())
                     .arg(region.width())
                     .arg(region.height())
                     .arg(options.microphone ? 1 : 0)
                     .arg(static_cast<int>(options.systemAudio))
                     .arg(request.fps)
                     .arg(request.crf)
                     .arg(request.maxWidth)
                     .arg(request.maxHeight)
                     .arg(request.monoAudio ? 1 : 0)
                     .arg(request.outputPath));

        if (settings.recording.showControls) {
            auto* controls = new RecordingControls(session, region);
            connect(controls,
                    &RecordingControls::restartRequested,
                    this,
                    [this, region, thumbnail, options, target, windowId] {
                        if (!m_activeSession || !m_activeSession->isActive())
                            return;
                        // Wait for the camera to be released before restarting.
                        m_restartAfterFinish = [this, region, thumbnail, options, target, windowId] {
                            startRecording(region, thumbnail, options, target, windowId);
                        };
                        m_activeSession->discard();
                    });
            controls->show();
            controls->raise();
        }
    }

    void startScrollCapture(const QRect& region, quintptr windowId = 0)
    {
        if (region.isEmpty())
            return;
        if (m_scrollSession && m_scrollSession->isActive()) {
            debugLog(QStringLiteral("scroll ignored: a capture is already running"));
            return;
        }
        if (m_activeSession && m_activeSession->isActive()) {
            debugLog(QStringLiteral("scroll ignored: a recording is running"));
            return;
        }
        if (m_countdownPill)
            return;

        const int seconds = recordingCountdownSeconds();
        if (seconds > 0) {
            m_countdownPill = new RecordingCountdown(
                seconds,
                region,
                [this, region, windowId] {
                    m_countdownPill = nullptr;
                    beginScrollCapture(region, windowId);
                });
            m_countdownPill->show();
            return;
        }

        beginScrollCapture(region, windowId);
    }

    void beginScrollCapture(const QRect& region, quintptr windowId)
    {
        if (m_scrollSession && m_scrollSession->isActive())
            return;
        if (m_activeSession && m_activeSession->isActive())
            return;

        auto* session = new Platform::ScrollCaptureSession(this);
        m_scrollSession = session;

        connect(session,
                &Platform::ScrollCaptureSession::statusChanged,
                this,
                [](const QString& status) {
                    debugLog(QStringLiteral("scroll: %1").arg(status));
                });
        connect(session,
                &Platform::ScrollCaptureSession::finished,
                this,
                [this, session, region](bool ok, const QImage& image, const QString& error) {
                    if (m_scrollSession == session)
                        m_scrollSession = nullptr;
                    if (m_scrollMask) {
                        m_scrollMask->close();
                        m_scrollMask = nullptr;
                    }
                    session->deleteLater();
                    if (ok)
                        presentScroll(image, region);
                    else if (!error.isEmpty())
                        QMessageBox::warning(nullptr, QStringLiteral("Scroll capture"), error);
                });

        QString error;
        if (!session->start(region, windowId, error)) {
            if (m_scrollSession == session)
                m_scrollSession = nullptr;
            session->deleteLater();
            QMessageBox::warning(nullptr, QStringLiteral("Scroll capture"), error);
            return;
        }

        // The session resolves the scrollable UI element first; the mask is
        // shown afterwards so ElementFromPoint sees the target window.
        m_scrollMask = new RecordingMask(region);
        m_scrollMask->show();
        m_scrollMask->raise();

        debugLog(QStringLiteral("scroll: region=%1,%2 %3x%4 window=%5")
                     .arg(region.x())
                     .arg(region.y())
                     .arg(region.width())
                     .arg(region.height())
                     .arg(windowId));

        auto* controls = new ScrollControls(session, region);
        controls->show();
        controls->raise();
    }

    void presentScroll(const QImage& image, const QRect& region)
    {
        if (image.isNull())
            return;

        m_telemetry->recordScreenshot();
        const Platform::ShellSettings settings = Platform::loadShellSettings();
        QString saveError;
        const QString path = saveCapture(image, region, settings, saveError);
        if (path.isEmpty())
            QMessageBox::warning(nullptr, QStringLiteral("Screenshot"),
                                 QStringLiteral("Could not save the screenshot: %1").arg(saveError));
        m_lastScreenshot = image;
        m_lastScreenshotPath = path;
        m_lastScreenshotText.clear();
        if (!path.isEmpty())
            QSettings().setValue(QStringLiteral("shortcuts/last_screenshot"), path);
        maybeAutoUpload(path, settings);

        if (settings.screenshots.afterOpenEditor && !path.isEmpty())
            openImageEditorWindow(path);

        if (settings.screenshots.afterCopyToClipboard)
            Platform::copyCaptureToClipboard(image, path, settings.screenshots.clipboardMode);

        const QString hint = path.isEmpty() ? QStringLiteral("Save failed")
            : settings.screenshots.afterCopyToClipboard
            ? QStringLiteral("Scroll capture copied")
            : QStringLiteral("Scroll capture saved");

        if (!settings.screenshots.afterShowResult) {
            // A capture that did not save already raised its own warning.
            if (!path.isEmpty())
                notify(hint);
            return;
        }

        auto* card = new ResultCard(image, path, QString(), hint, image.size());
        card->show();
        card->raise();
        card->activateWindow();
    }

    void presentRecording(const QImage& thumbnail,
                          const QString& path,
                          const Platform::ShellSettings& settings)
    {
        m_telemetry->recordRecording();
        maybeAutoUpload(path, settings);
        if (settings.recording.afterCopyToClipboard) {
            auto* mime = new QMimeData;
            mime->setUrls({QUrl::fromLocalFile(path)});
            QGuiApplication::clipboard()->setMimeData(mime);
        }
        if (settings.recording.afterOpenEditor)
            openVideoEditorWindow(path);

        const QString hint = settings.recording.afterSave
            ? QStringLiteral("Recording saved")
            : QStringLiteral("Temporary recording");

        if (!settings.recording.afterShowResult) {
            notify(hint);
            return;
        }

        const QImage thumb = thumbnail.isNull()
            ? QImage(2, 2, QImage::Format_ARGB32)
            : thumbnail;
        auto* card = new ResultCard(thumb, path, QString(), hint);
        card->show();
        card->raise();
        card->activateWindow();
    }

    void present(const QImage& image, const CaptureOptions& options, const QRect& region)
    {
        if (image.isNull())
            return;

        m_telemetry->recordScreenshot();
        const Platform::ShellSettings settings = Platform::loadShellSettings();
        const bool persist = settings.screenshots.afterSave;
        QString saveError;
        const QString path = saveCapture(image, region, settings, saveError);
        if (path.isEmpty())
            QMessageBox::warning(nullptr, QStringLiteral("Screenshot"),
                                 QStringLiteral("Could not save the screenshot: %1").arg(saveError));

        QString text;
        bool ocrFailed = false;
        bool isQr = false;
        QString ocrError;
        if (options.ocr) {
            // QR first: ApexShot's OCR toggle also reads codes.
            QByteArray payload;
            QString qrError;
            if (Platform::decodeQr(image, payload, qrError)) {
                isQr = true;
                text = QString::fromUtf8(payload).trimmed();
                debugLog(QStringLiteral("qr detected: %1 bytes").arg(payload.size()));
            } else {
                debugLog(QStringLiteral("qr: %1").arg(qrError));
                if (!Platform::recognizeText(image, settings.ocr.language,
                                             settings.ocr.keepLineBreaks, text, ocrError)) {
                    ocrFailed = true;
                    debugLog(QStringLiteral("ocr failed: %1").arg(ocrError));
                    notify(ocrError, QStringLiteral("OCR unavailable"));
                }
                text = text.trimmed();
                debugLog(QStringLiteral("ocr chars=%1").arg(text.size()));
            }
        }

        QString hint = QStringLiteral("Ready");
        if (options.ocr) {
            if (isQr) {
                m_telemetry->recordOcr();
                QGuiApplication::clipboard()->setText(text);
                hint = QStringLiteral("QR code detected - copied");
            } else if (!text.isEmpty()) {
                m_telemetry->recordOcr();
                QGuiApplication::clipboard()->setText(text);
                hint = QStringLiteral("Text copied to clipboard");
            } else {
                Platform::copyCaptureToClipboard(
                    image, path, settings.screenshots.clipboardMode);
                hint = ocrFailed ? QStringLiteral("OCR unavailable - capture copied")
                                 : QStringLiteral("No text found - capture copied");
            }
        } else if (settings.screenshots.afterCopyToClipboard) {
            Platform::copyCaptureToClipboard(image, path, settings.screenshots.clipboardMode);
            hint = QStringLiteral("Copied to clipboard");
        } else if (path.isEmpty()) {
            hint = QStringLiteral("Save failed");
        } else {
            hint = persist ? QStringLiteral("Saved") : QStringLiteral("Ready");
        }

        m_lastScreenshot = image;
        m_lastScreenshotPath = path;
        m_lastScreenshotText = text;
        if (!path.isEmpty())
            QSettings().setValue(QStringLiteral("shortcuts/last_screenshot"), path);
        maybeAutoUpload(path, settings);
        if (settings.screenshots.afterOpenEditor && !path.isEmpty())
            openImageEditorWindow(path);

        if (!settings.screenshots.afterShowResult) {
            // The hint is what the card would have said - text copied, image
            // copied, saved - so with the card off the notification carries it.
            // A capture that did not save already raised its own warning.
            if (!path.isEmpty())
                notify(hint);
            return;
        }

        auto* card = new ResultCard(image, path, text, hint);
        card->show();
        card->raise();
        card->activateWindow();
    }

    bool m_menuOpen = false;
    Platform::UsageTelemetry* m_telemetry = nullptr;
    QPointer<QSystemTrayIcon> m_tray;
    bool m_saveDialogOpen = false;
    QImage m_lastScreenshot;
    QString m_lastScreenshotPath;
    QString m_lastScreenshotText;
    Platform::RecordingSession* m_activeSession = nullptr;
    /// Unfinished recordings the person chose to recover, saved one at a time.
    QVector<Platform::UnfinishedRecording> m_pendingRecoveries;
    /// Set when the restart button asks for a fresh take after the current
    /// one has finished discarding.
    std::function<void()> m_restartAfterFinish;
    QPointer<CountdownPill> m_screenshotCountdown;
    QPointer<RecordingCountdown> m_countdownPill;
    QPointer<RecordingMask> m_recordingMask;
    QPointer<Platform::WebcamBubble> m_webcamBubble;
    QPointer<Platform::WebcamBubble> m_setupWebcamBubble;
    QPointer<Platform::WebcamPreview> m_webcamPreview;
    QPointer<Platform::WebcamPreview> m_activeWebcamPreview;
    QString m_previewDevice;
    QPointer<Platform::WindowPicker> m_windowPicker;
    QPointer<AreaSelector> m_areaSelector;
    QPointer<Platform::RecordingSetupOverlay> m_setupOverlay;
    QPointer<Platform::ScrollCaptureSession> m_scrollSession;
    QPointer<RecordingMask> m_scrollMask;
};

} // namespace

namespace QuickAccess {

int run(QApplication& app)
{
    registerBundledFonts();
    app.setQuitOnLastWindowClosed(false);

    {
        // Measure the top bar's text against its fixed columns, so clipping is
        // visible in the log instead of only on screen.
        QFont label(uiFamily());
        label.setPixelSize(14);
        label.setWeight(QFont::Medium);
        label.setHintingPreference(QFont::PreferNoHinting);
        label.setLetterSpacing(QFont::PercentageSpacing, 98.0);
        QFont pill(uiFamily());
        pill.setPixelSize(13);
        pill.setWeight(QFont::Medium);
        pill.setHintingPreference(QFont::PreferNoHinting);
        pill.setLetterSpacing(QFont::PercentageSpacing, 98.0);
        const QFontMetrics labelMetrics(label);
        const QFontMetrics pillMetrics(pill);
        QFile log(QDir(QDir::tempPath()).filePath(QStringLiteral("apexshot-quickaccess.log")));
        if (log.open(QIODevice::Append | QIODevice::WriteOnly)) {
            log.write(QStringLiteral("layout: label=%1 (min 108) pills 52/52/48/44 -> "
                                     "Free=%2 16:9=%3 4:3=%4 1:1=%5\n")
                          .arg(labelMetrics.horizontalAdvance(QStringLiteral("Draw an area")))
                          .arg(pillMetrics.horizontalAdvance(QStringLiteral("Free")))
                          .arg(pillMetrics.horizontalAdvance(QStringLiteral("16:9")))
                          .arg(pillMetrics.horizontalAdvance(QStringLiteral("4:3")))
                          .arg(pillMetrics.horizontalAdvance(QStringLiteral("1:1")))
                          .toUtf8());
        }
    }

    auto* shell = new Shell;
    shell->setParent(&app);

    const QStringList arguments = QCoreApplication::arguments();
    if (arguments.contains(QStringLiteral("--demo-area"))) {
        QTimer::singleShot(0, shell, [shell] { shell->demoArea(); });
        return app.exec();
    }
    if (arguments.contains(QStringLiteral("--demo-windows"))) {
        QTimer::singleShot(0, shell, [shell] { shell->demoWindows(); });
        return app.exec();
    }
    if (arguments.contains(QStringLiteral("--demo-crosshair"))) {
        QTimer::singleShot(0, shell, [shell] { shell->demoCrosshair(); });
        return app.exec();
    }
    if (arguments.contains(QStringLiteral("--demo-ocr"))) {
        QTimer::singleShot(0, shell, [shell] { shell->demoOcr(); });
        return app.exec();
    }
    if (arguments.contains(QStringLiteral("--demo-timer"))) {
        QTimer::singleShot(0, shell, [shell] { shell->demoTimer(); });
        return app.exec();
    }
    if (arguments.contains(QStringLiteral("--demo-record"))) {
        QTimer::singleShot(0, shell, [shell] { shell->demoRecord(); });
        return app.exec();
    }
    if (arguments.contains(QStringLiteral("--probe-audio"))) {
        QTimer::singleShot(0, shell, [shell] { shell->probeAudio(); });
        return app.exec();
    }
    if (arguments.contains(QStringLiteral("--demo-window-setup-start"))) {
        QTimer::singleShot(0, shell, [shell] { shell->demoWindowSetupStart(); });
        return app.exec();
    }
    if (arguments.contains(QStringLiteral("--demo-window-record"))) {
        QTimer::singleShot(0, shell, [shell] { shell->demoWindowRecord(); });
        return app.exec();
    }
    if (arguments.contains(QStringLiteral("--demo-window-setup"))) {
        QTimer::singleShot(0, shell, [shell] { shell->demoWindowSetup(); });
        return app.exec();
    }
    if (arguments.contains(QStringLiteral("--demo-system-audio"))) {
        QTimer::singleShot(0, shell, [shell] { shell->demoSystemAudio(); });
        return app.exec();
    }
    if (arguments.contains(QStringLiteral("--demo-record-window"))) {
        QTimer::singleShot(0, shell, [shell] { shell->recordWindow(); });
        return app.exec();
    }
    if (arguments.contains(QStringLiteral("--demo-record-area"))) {
        QTimer::singleShot(0, shell, [shell] { shell->demoRecordArea(); });
        return app.exec();
    }
    if (arguments.contains(QStringLiteral("--demo-scroll-window"))) {
        QTimer::singleShot(0, shell, [shell] { shell->demoScrollWindow(); });
        return app.exec();
    }

    Platform::GlobalHotkeys hotkeys(shell, [shell](const QString& action) {
        if (action != QLatin1String("recording_stop_save") && shell->shortcutInteractionOpen())
            return;
        if (action == QLatin1String("quick_capture"))
            shell->openMenu();
        else if (action == QLatin1String("capture_area"))
            shell->shortcutCaptureArea();
        else if (action == QLatin1String("capture_crosshair"))
            shell->shortcutCaptureCrosshair();
        else if (action == QLatin1String("capture_previous_area"))
            shell->shortcutCapturePreviousArea();
        else if (action == QLatin1String("capture_fullscreen"))
            shell->shortcutCaptureFullscreen();
        else if (action == QLatin1String("show_last_preview"))
            shell->shortcutShowLastPreview();
        else if (action == QLatin1String("save_last_capture"))
            shell->shortcutSaveLastScreenshot();
        else if (action == QLatin1String("record_screen"))
            shell->recordScreen();
        else if (action == QLatin1String("recording_stop_save"))
            shell->stopActiveRecording();
    });

    // A second launch from the Start menu talks to this one. Exiting silently
    // made the app look broken: the shortcut was found, clicked, and nothing
    // ever appeared because the shell was already in the tray.
    static QLocalServer sessionServer;
    QLocalServer::removeServer(QStringLiteral("apexshot-shell"));
    if (sessionServer.listen(QStringLiteral("apexshot-shell"))) {
        QObject::connect(&sessionServer, &QLocalServer::newConnection, shell, [shell] {
            while (QLocalSocket* socket = sessionServer.nextPendingConnection()) {
                QObject::connect(socket, &QLocalSocket::readyRead, socket, [socket, shell] {
                    if (socket->readAll().trimmed() == QByteArray("menu"))
                        shell->openMenu();
                });
                QObject::connect(socket, &QLocalSocket::disconnected,
                                 socket, &QLocalSocket::deleteLater);
            }
        });
    } else {
        debugLog(QStringLiteral("session channel unavailable: %1")
                     .arg(sessionServer.errorString()));
    }

    auto* tray = new QSystemTrayIcon(appIcon(), &app);
    shell->setTray(tray);
    tray->setToolTip(QStringLiteral("ApexShot"));

    auto* menu = new QMenu;
    const QString menuStyle = trayMenuStyle();
    const QFont menuFont = uiFont(std::lround(14 * uiScale()));
    decoratePopupMenu(menu, menuStyle, menuFont);

    // Every submenu needs the same body and window properties, or it paints
    // square corners under a rounded parent.
    const auto styleSubmenu = [&](QMenu* submenu) {
        decoratePopupMenu(submenu, menuStyle, menuFont);
    };

    QAction* capture = menu->addAction(QStringLiteral("Capture..."));
    QAction* crosshair = menu->addAction(QStringLiteral("Crosshair capture"));

    QString recordingError;
    const bool recordingAvailable = Platform::RecordingSession::available(&recordingError);
    if (!recordingAvailable)
        debugLog(QStringLiteral("recording disabled: %1").arg(recordingError));

    QMenu* recordMenu = menu->addMenu(QStringLiteral("Record"));
    styleSubmenu(recordMenu);
    QAction* recordScreen = recordMenu->addAction(QStringLiteral("Screen"));
    QAction* recordArea = recordMenu->addAction(QStringLiteral("Area"));
    QAction* recordWindow = recordMenu->addAction(QStringLiteral("Window"));
    QAction* stopRecording = recordMenu->addAction(QStringLiteral("Stop and save recording"));
    stopRecording->setVisible(false);
    recordMenu->setEnabled(recordingAvailable);
    recordMenu->setToolTipsVisible(true);
    if (!recordingAvailable)
        recordMenu->setToolTip(recordingError);

    QMenu* scrollMenu = menu->addMenu(QStringLiteral("Scroll capture"));
    styleSubmenu(scrollMenu);
    QAction* scrollWindowAction = scrollMenu->addAction(QStringLiteral("Window"));
    QAction* scrollAreaAction = scrollMenu->addAction(QStringLiteral("Area"));
    scrollMenu->setToolTipsVisible(true);
    scrollMenu->setToolTip(QStringLiteral("Stitch a tall window or area into one image"));

    recordMenu->addSeparator();
    QMenu* countdownMenu = recordMenu->addMenu(QStringLiteral("Countdown"));
    styleSubmenu(countdownMenu);
    auto* countdownGroup = new QActionGroup(countdownMenu);
    countdownGroup->setExclusive(true);
    const int currentCountdown = recordingCountdownSeconds();
    for (const int seconds : {0, 3, 5, 10}) {
        QAction* action = countdownMenu->addAction(
            seconds == 0 ? QStringLiteral("Off")
                         : QStringLiteral("%1 seconds").arg(seconds));
        action->setCheckable(true);
        action->setData(seconds);
        action->setChecked(seconds == currentCountdown);
        countdownGroup->addAction(action);
        QObject::connect(action, &QAction::triggered, action,
                         [seconds] { setRecordingCountdownSeconds(seconds); });
    }

    // The Rust settings window can turn the countdown off entirely; keep the
    // submenu honest without restarting the shell.
    QObject::connect(menu, &QMenu::aboutToShow, menu,
                     [countdownMenu, countdownGroup, stopRecording, shell] {
        const Platform::ShellSettings settings = Platform::loadShellSettings();
        stopRecording->setVisible(shell->hasActiveRecording());
        countdownMenu->setEnabled(settings.recording.showCountdown);
        const int current = recordingCountdownSeconds();
        for (QAction* action : countdownGroup->actions()) {
            if (action->data().isValid())
                action->setChecked(action->data().toInt() == current);
        }
    });

    menu->addSeparator();
    QAction* videoEditorAction = menu->addAction(QStringLiteral("Video Editor..."));
    QAction* historyAction = menu->addAction(QStringLiteral("History..."));
    QAction* settingsAction = menu->addAction(QStringLiteral("Settings..."));
    menu->addSeparator();
    QAction* quit = menu->addAction(QStringLiteral("Quit"));
    tray->setContextMenu(menu);
    tray->show();

    // A release lookup is cached for a day, so this is one quiet request per
    // day however often the timer fires. The first check waits for the tray
    // and the hotkeys to settle.
    QTimer::singleShot(10000, &app, [] { startUpdateCheck(); });
    auto* updateTimer = new QTimer(&app);
    updateTimer->setInterval(60 * 60 * 1000);
    QObject::connect(updateTimer, &QTimer::timeout, &app, [] { startUpdateCheck(); });
    updateTimer->start();

    QObject::connect(settingsAction, &QAction::triggered, shell, [] {
        openSettingsWindow();
    });
    QObject::connect(historyAction, &QAction::triggered, shell, [] {
        openHistoryWindow();
    });
    QObject::connect(videoEditorAction, &QAction::triggered, shell, [] {
        openVideoEditorWindow();
    });

    QObject::connect(capture, &QAction::triggered, shell, [shell] { shell->openMenu(); });
    QObject::connect(crosshair, &QAction::triggered, shell,
                     [shell] { shell->shortcutCaptureCrosshair(); });
    QObject::connect(recordScreen, &QAction::triggered, shell,
                     [shell] { shell->recordScreen(); });
    QObject::connect(recordArea, &QAction::triggered, shell,
                     [shell] { shell->recordArea(); });
    QObject::connect(recordWindow, &QAction::triggered, shell,
                     [shell] { shell->recordWindow(); });
    QObject::connect(stopRecording, &QAction::triggered, shell,
                     [shell] { shell->stopActiveRecording(); });
    QObject::connect(scrollWindowAction, &QAction::triggered, shell,
                     [shell] { shell->scrollWindow(); });
    QObject::connect(scrollAreaAction, &QAction::triggered, shell,
                     [shell] { shell->scrollArea(); });
    QObject::connect(quit, &QAction::triggered, &app, [shell, &app] {
        // A quit would destroy the session and discard its segments. Save
        // first; the next Quit, once the result card is up, exits.
        if (shell->hasActiveRecording()) {
            shell->stopActiveRecording();
            return;
        }
        app.quit();
    });
    QObject::connect(tray, &QSystemTrayIcon::activated, shell,
                     [shell](QSystemTrayIcon::ActivationReason reason) {
                         if (reason == QSystemTrayIcon::Trigger)
                             shell->openMenu();
                     });

    // Give the tray a moment to settle before asking about unfinished
    // recordings from a previous run.
    QTimer::singleShot(1500, shell, [shell] { shell->offerRecovery(); });

    if (!arguments.contains(QStringLiteral("--background")))
        QTimer::singleShot(0, shell, [shell] { shell->openMenu(); });

    return app.exec();
}

} // namespace QuickAccess
