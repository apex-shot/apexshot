#include "platform/windows/pointer_tracker.h"

#include <QCryptographicHash>
#include <QDir>
#include <QFileInfo>
#include <QHash>
#include <QImage>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QPoint>
#include <QSaveFile>
#include <QTimer>

#include <algorithm>
#include <array>
#include <cstring>
#include <utility>
#include <vector>

namespace Platform {

namespace {

constexpr int kMaxPointerSamples = 8000;
constexpr int kMaxClicks = 500;
constexpr int kMaxPresses = 500;
constexpr int kMaxScrolls = 2000;
constexpr int kMaxKeys = 4000;
constexpr int kMaxVisibilityChanges = 1000;
constexpr double kScrollHoverWindowSeconds = 0.15;
constexpr double kDragThresholdSquared = 16.0;
constexpr std::array<int, 3> kVirtualButtons = {VK_LBUTTON, VK_MBUTTON, VK_RBUTTON};
constexpr std::array<int, 3> kSidecarButtons = {1, 2, 3};
constexpr int kModifierShift = 1;
constexpr int kModifierControl = 2;
constexpr int kModifierAlt = 4;
constexpr int kModifierWin = 8;

int currentModifiers()
{
    int modifiers = 0;
    if (GetKeyState(VK_SHIFT) & 0x8000)
        modifiers |= kModifierShift;
    if (GetKeyState(VK_CONTROL) & 0x8000)
        modifiers |= kModifierControl;
    if (GetKeyState(VK_MENU) & 0x8000)
        modifiers |= kModifierAlt;
    if ((GetKeyState(VK_LWIN) & 0x8000) || (GetKeyState(VK_RWIN) & 0x8000))
        modifiers |= kModifierWin;
    return modifiers;
}

QJsonArray modifierArray(int modifiers)
{
    QJsonArray array;
    if (modifiers & kModifierShift)
        array.append(QStringLiteral("shift"));
    if (modifiers & kModifierControl)
        array.append(QStringLiteral("control"));
    if (modifiers & kModifierAlt)
        array.append(QStringLiteral("alt"));
    if (modifiers & kModifierWin)
        array.append(QStringLiteral("win"));
    return array;
}

struct StandardCursor {
    LPCSTR id;
    CursorKind kind;
};

/// Standard cursors are shared system resources: the loader returns the same
/// handle for the same IDC_* every time, so comparing handles is exact and
/// cheap. The handles are loaded once and cached. A user's cursor scheme still
/// resolves each ID to one shared handle, so a themed arrow or hand is still
/// the arrow or hand.
const std::array<StandardCursor, 10> kStandardCursors = {
    StandardCursor{IDC_HAND, CursorKind::Hand},
    StandardCursor{IDC_IBEAM, CursorKind::Text},
    StandardCursor{IDC_CROSS, CursorKind::Crosshair},
    StandardCursor{IDC_WAIT, CursorKind::Wait},
    StandardCursor{IDC_APPSTARTING, CursorKind::Wait},
    StandardCursor{IDC_SIZEWE, CursorKind::Resize},
    StandardCursor{IDC_SIZENS, CursorKind::Resize},
    StandardCursor{IDC_SIZENWSE, CursorKind::Resize},
    StandardCursor{IDC_SIZENESW, CursorKind::Resize},
    StandardCursor{IDC_SIZEALL, CursorKind::Resize},
};

struct StandardCursorHandles {
    std::array<HCURSOR, kStandardCursors.size()> handles = {};

    StandardCursorHandles()
    {
        // The build does not define UNICODE, so the IDC_* macros expand to char
        // resource pointers; the ANSI entry point takes them as they are and
        // returns the same shared cursor the wide one would for that ID.
        for (size_t i = 0; i < kStandardCursors.size(); ++i)
            handles[i] = LoadCursorA(nullptr, kStandardCursors[i].id);
    }
};

const StandardCursorHandles& standardCursorHandles()
{
    static const StandardCursorHandles handles;
    return handles;
}

struct CursorShape {
    QString id;
    QString file;
    int width = 0;
    int height = 0;
    int hotspotX = 0;
    int hotspotY = 0;
    CursorKind kind = CursorKind::Default;
    QImage image;
};

struct PointerSample {
    double t = 0.0;
    double x = 0.0;
    double y = 0.0;
    CursorKind kind = CursorKind::Default;
    QString cursorId;
    int modifiers = 0;
};

struct ClickSample {
    double t = 0.0;
    double x = 0.0;
    double y = 0.0;
    int button = 0;
    QString cursorId;
    int modifiers = 0;
};

struct PressSample {
    double down = 0.0;
    double up = 0.0;
    int button = 0;
    bool dragged = false;
};

struct ScrollSample {
    double t = 0.0;
    double x = 0.0;
    double y = 0.0;
    int delta = 0;
    bool horizontal = false;
    int modifiers = 0;
    QString cursorId;
};

struct KeySample {
    double t = 0.0;
    bool down = false;
    int key = 0;
    int scanCode = 0;
    int modifiers = 0;
    QString cursorId;
};

struct VisibilitySample {
    double t = 0.0;
    bool visible = true;
};

/// Halve a full buffer in place, keeping its first and last samples, so a long
/// recording thins out evenly instead of losing everything past the cap. The
/// reader applies the same rule to samples it is handed.
template <typename T>
void compactToLimit(std::vector<T>& samples, size_t limit)
{
    while (samples.size() > limit) {
        std::vector<T> compacted;
        compacted.reserve(samples.size() / 2 + 1);
        compacted.push_back(samples.front());
        for (size_t index = 2; index + 1 < samples.size(); index += 2)
            compacted.push_back(samples[index]);
        compacted.push_back(samples.back());
        samples.swap(compacted);
    }
}

QString sidecarFilePath(const QString& videoPath)
{
    const QString appData = qEnvironmentVariable("LOCALAPPDATA");
    const QString canonicalPath = canonicalVideoPathFor(videoPath);
    if (appData.isEmpty() || canonicalPath.isEmpty())
        return QString();

    const QByteArray digest = QCryptographicHash::hash(canonicalPath.toUtf8(),
                                                        QCryptographicHash::Sha256)
                                  .toHex();
    return QDir(appData).filePath(QStringLiteral("apexshot/pointer-sidecars/%1")
                                      .arg(QString::fromLatin1(digest)));
}

QJsonObject pointerObject(const PointerSample& sample)
{
    QJsonObject object{{QStringLiteral("t"), sample.t},
                       {QStringLiteral("x"), sample.x},
                       {QStringLiteral("y"), sample.y},
                       {QStringLiteral("kind"), cursorKindName(sample.kind)}};
    if (!sample.cursorId.isEmpty())
        object.insert(QStringLiteral("cursor_id"), sample.cursorId);
    if (sample.modifiers)
        object.insert(QStringLiteral("modifiers"), modifierArray(sample.modifiers));
    return object;
}

QJsonObject clickObject(const ClickSample& sample)
{
    QJsonObject object{{QStringLiteral("t"), sample.t},
                       {QStringLiteral("x"), sample.x},
                       {QStringLiteral("y"), sample.y},
                       {QStringLiteral("button"), sample.button}};
    if (!sample.cursorId.isEmpty())
        object.insert(QStringLiteral("cursor_id"), sample.cursorId);
    if (sample.modifiers)
        object.insert(QStringLiteral("modifiers"), modifierArray(sample.modifiers));
    return object;
}

QJsonObject pressObject(const PressSample& sample)
{
    return {{QStringLiteral("down"), sample.down},
            {QStringLiteral("up"), sample.up},
            {QStringLiteral("button"), sample.button},
            {QStringLiteral("dragged"), sample.dragged}};
}

QJsonObject scrollObject(const ScrollSample& sample)
{
    QJsonObject object{{QStringLiteral("t"), sample.t},
                       {QStringLiteral("x"), sample.x},
                       {QStringLiteral("y"), sample.y},
                       {QStringLiteral("delta"), sample.delta},
                       {QStringLiteral("horizontal"), sample.horizontal}};
    if (!sample.cursorId.isEmpty())
        object.insert(QStringLiteral("cursor_id"), sample.cursorId);
    if (sample.modifiers)
        object.insert(QStringLiteral("modifiers"), modifierArray(sample.modifiers));
    return object;
}

QJsonObject keyObject(const KeySample& sample)
{
    QJsonObject object{{QStringLiteral("t"), sample.t},
                       {QStringLiteral("type"), sample.down ? QStringLiteral("key_down")
                                                            : QStringLiteral("key_up")},
                       {QStringLiteral("key"), sample.key},
                       {QStringLiteral("scan_code"), sample.scanCode}};
    if (!sample.cursorId.isEmpty())
        object.insert(QStringLiteral("cursor_id"), sample.cursorId);
    if (sample.modifiers)
        object.insert(QStringLiteral("modifiers"), modifierArray(sample.modifiers));
    return object;
}

QJsonObject visibilityObject(const VisibilitySample& sample)
{
    return {{QStringLiteral("t"), sample.t}, {QStringLiteral("visible"), sample.visible}};
}

/// Decode the cursor's bitmap the way the studied recorder does: the image is
/// kept per shape, and its hotspot travels with it. Cursors are stored as
/// PNG files beside the sidecar instead of being burned into the video.
QImage cursorImageFromHandle(HCURSOR cursor, QPoint* hotspot)
{
    if (!cursor)
        return {};

    ICONINFO info{};
    if (!GetIconInfo(cursor, &info))
        return {};

    struct BitmapCleanup {
        HBITMAP color = nullptr;
        HBITMAP mask = nullptr;
        ~BitmapCleanup()
        {
            if (color)
                DeleteObject(color);
            if (mask)
                DeleteObject(mask);
        }
    } cleanup{info.hbmColor, info.hbmMask};

    int width = 0;
    int height = 0;
    BITMAP bitmap{};
    if (info.hbmColor && GetObject(info.hbmColor, sizeof(bitmap), &bitmap)) {
        width = bitmap.bmWidth;
        height = bitmap.bmHeight;
    } else if (info.hbmMask && GetObject(info.hbmMask, sizeof(bitmap), &bitmap)) {
        width = bitmap.bmWidth;
        height = bitmap.bmHeight / 2;
    }
    if (width <= 0 || height <= 0) {
        width = GetSystemMetrics(SM_CXCURSOR);
        height = GetSystemMetrics(SM_CYCURSOR);
    }
    if (width <= 0 || height <= 0)
        return {};

    BITMAPINFO dib{};
    dib.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    dib.bmiHeader.biWidth = width;
    dib.bmiHeader.biHeight = -height;
    dib.bmiHeader.biPlanes = 1;
    dib.bmiHeader.biBitCount = 32;
    dib.bmiHeader.biCompression = BI_RGB;

    void* bits = nullptr;
    HBITMAP dibBitmap = CreateDIBSection(nullptr, &dib, DIB_RGB_COLORS, &bits, nullptr, 0);
    if (!dibBitmap || !bits) {
        if (dibBitmap)
            DeleteObject(dibBitmap);
        return {};
    }

    HDC dc = CreateCompatibleDC(nullptr);
    if (!dc) {
        DeleteObject(dibBitmap);
        return {};
    }
    HGDIOBJ previous = SelectObject(dc, dibBitmap);
    std::memset(bits, 0, size_t(width) * size_t(height) * 4);
    DrawIconEx(dc, 0, 0, cursor, width, height, 0, nullptr, DI_NORMAL);
    GdiFlush();

    QImage image(width, height, QImage::Format_ARGB32);
    std::memcpy(image.bits(), bits, image.sizeInBytes());

    bool hasAlpha = false;
    for (int y = 0; y < height && !hasAlpha; ++y) {
        const QRgb* line = reinterpret_cast<const QRgb*>(image.constScanLine(y));
        for (int x = 0; x < width; ++x) {
            if (qAlpha(line[x]) != 0) {
                hasAlpha = true;
                break;
            }
        }
    }

    // A monochrome cursor has no alpha, and DrawIconEx can leave the colour
    // bitmap empty. Its mask bitmap carries two 1-bit rows: the AND mask and
    // the XOR mask. Decode both directly so an I-beam keeps its white stroke
    // instead of becoming an empty PNG.
    if (!hasAlpha && info.hbmMask) {
        BITMAP maskBitmap{};
        if (GetObject(info.hbmMask, sizeof(maskBitmap), &maskBitmap)
            && maskBitmap.bmWidth == width && maskBitmap.bmHeight > 0) {
            const int rows = maskBitmap.bmHeight;
            const int stride = ((width + 31) / 32) * 4;
            std::vector<unsigned char> mask(size_t(stride) * size_t(rows), 0);
            BITMAPINFO maskInfo{};
            maskInfo.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
            maskInfo.bmiHeader.biWidth = width;
            maskInfo.bmiHeader.biHeight = -rows;
            maskInfo.bmiHeader.biPlanes = 1;
            maskInfo.bmiHeader.biBitCount = 1;
            maskInfo.bmiHeader.biCompression = BI_RGB;
            HDC maskDc = CreateCompatibleDC(nullptr);
            const int maskLines = maskDc
                ? GetDIBits(maskDc,
                            info.hbmMask,
                            0,
                            rows,
                            mask.data(),
                            &maskInfo,
                            DIB_RGB_COLORS)
                : 0;
            if (maskDc)
                DeleteDC(maskDc);
            if (maskLines > 0) {
                const bool haveXor = rows >= height * 2;
                const bool monochrome = info.hbmColor == nullptr;
                const auto maskBit = [&](int row, int x) {
                    return (mask[size_t(row) * size_t(stride) + size_t(x / 8)]
                            & (0x80 >> (x % 8)))
                        != 0;
                };
                for (int y = 0; y < height; ++y) {
                    QRgb* line = reinterpret_cast<QRgb*>(image.scanLine(y));
                    for (int x = 0; x < width; ++x) {
                        const bool andBit = maskBit(y, x);
                        const bool xorBit = haveXor && maskBit(y + height, x);
                        if (monochrome) {
                            // AND=1, XOR=0 leaves the screen unchanged.
                            // The other combinations paint black or white.
                            line[x] = andBit && !xorBit
                                ? qRgba(0, 0, 0, 0)
                                : (xorBit ? qRgba(255, 255, 255, 255)
                                          : qRgba(0, 0, 0, 255));
                        } else if (andBit) {
                            line[x] = qRgba(0, 0, 0, 0);
                        } else if (qAlpha(line[x]) == 0) {
                            line[x] = qRgba(qRed(line[x]), qGreen(line[x]), qBlue(line[x]), 255);
                        }
                    }
                }
            }
        }
    }

    SelectObject(dc, previous);
    DeleteDC(dc);
    DeleteObject(dibBitmap);

    if (hotspot)
        *hotspot = QPoint(info.xHotspot, info.yHotspot);
    return image;
}

struct StandardBitmap {
    CursorKind kind;
    QImage image;
};

/// The bitmaps of the shared system cursors, captured once. An app that hands
/// back its own HCURSOR still usually draws the same arrow, hand, or I-beam,
/// so comparing the pixels recovers the semantic kind a theme can restyle.
const QVector<StandardBitmap>& standardBitmaps()
{
    static const QVector<StandardBitmap> bitmaps = [] {
        QVector<StandardBitmap> list;
        const StandardCursorHandles& table = standardCursorHandles();
        for (size_t i = 0; i < kStandardCursors.size(); ++i) {
            if (!table.handles[i])
                continue;
            QPoint hotspot;
            const QImage image = cursorImageFromHandle(table.handles[i], &hotspot);
            if (!image.isNull())
                list.push_back({kStandardCursors[i].kind, image});
        }
        return list;
    }();
    return bitmaps;
}

/// Whether two cursor bitmaps draw the same shape. Cursors are small and
/// anti-aliasing is identical for the same shape, so a few percent of pixels
/// may differ without being a different cursor.
bool similarCursorBitmap(const QImage& a, const QImage& b)
{
    if (a.size() != b.size())
        return false;
    const QImage left = a.convertToFormat(QImage::Format_ARGB32);
    const QImage right = b.convertToFormat(QImage::Format_ARGB32);
    int considered = 0;
    int differing = 0;
    for (int y = 0; y < left.height(); ++y) {
        const QRgb* leftLine = reinterpret_cast<const QRgb*>(left.constScanLine(y));
        const QRgb* rightLine = reinterpret_cast<const QRgb*>(right.constScanLine(y));
        for (int x = 0; x < left.width(); ++x) {
            const QRgb l = leftLine[x];
            const QRgb r = rightLine[x];
            const bool leftInk = qAlpha(l) > 8;
            const bool rightInk = qAlpha(r) > 8;
            // Transparent padding says nothing about the shape, and two mostly
            // empty cursors would otherwise match by sharing their emptiness.
            if (!leftInk && !rightInk)
                continue;
            ++considered;
            if (qAbs(qAlpha(l) - qAlpha(r)) > 24) {
                ++differing;
                continue;
            }
            if (leftInk && rightInk
                && qAbs(qRed(l) - qRed(r)) + qAbs(qGreen(l) - qGreen(r))
                        + qAbs(qBlue(l) - qBlue(r))
                    > 48) {
                ++differing;
            }
        }
    }
    return considered >= 36 && differing * 100 <= considered * 6;
}

/// The kind a captured bitmap draws, when it matches a standard cursor.
bool standardKindForBitmap(const QImage& image, CursorKind* kind)
{
    for (const StandardBitmap& standard : standardBitmaps()) {
        if (similarCursorBitmap(image, standard.image)) {
            if (kind)
                *kind = standard.kind;
            return true;
        }
    }
    return false;
}

PointerTracker* gHookOwner = nullptr;

} // namespace

class PointerTracker::Data
{
public:
    QTimer timer;
    std::vector<PointerSample> pointer;
    std::vector<ClickSample> clicks;
    std::vector<PressSample> presses;
    std::vector<ScrollSample> scrolls;
    std::vector<KeySample> keys;
    std::vector<VisibilitySample> visibility;
    std::array<Press, 3> activePresses;
    // Monotonic origin of this recording session, in microseconds.
    qint64 t0MonotonicUs = 0;
    HHOOK hook = nullptr;
    HHOOK keyboardHook = nullptr;
    bool hookInstalled = false;
    bool keyboardHookInstalled = false;
    bool captureKeystrokes = false;
    bool active = false;
    POINT lastPoint{};
    bool haveLastPoint = false;
    CursorKind lastKind = CursorKind::Default;
    QString lastCursorId;
    bool lastVisible = true;
    bool haveVisibility = false;
    double lastScrollTime = -1.0;
    QHash<quintptr, QString> cursorIds;
    QHash<quintptr, CursorKind> cursorKinds;
    QHash<QString, CursorShape> cursors;
};

PointerTracker::PointerTracker(QObject* parent)
    : QObject(parent)
    , data_(std::make_unique<Data>())
{
    QObject::connect(&data_->timer, &QTimer::timeout, this, [this] { sample(); });
}

PointerTracker::~PointerTracker()
{
    removeHook();
}

void PointerTracker::reset()
{
    removeHook();
    data_->timer.stop();
    data_->active = false;
    data_->pointer.clear();
    data_->clicks.clear();
    data_->presses.clear();
    data_->scrolls.clear();
    data_->keys.clear();
    data_->visibility.clear();
    data_->activePresses = {};
    data_->lastPoint = {};
    data_->haveLastPoint = false;
    data_->lastKind = CursorKind::Default;
    data_->lastCursorId.clear();
    data_->lastVisible = true;
    data_->haveVisibility = false;
    data_->lastScrollTime = -1.0;
    data_->captureKeystrokes = false;
    data_->cursorIds.clear();
    data_->cursorKinds.clear();
    data_->cursors.clear();
}

void PointerTracker::start(const QRect& region,
                           int fps,
                           MediaClock mediaClock,
                           bool captureKeystrokes)
{
    reset();
    region_ = region;
    mediaClock_ = std::move(mediaClock);
    data_->t0MonotonicUs = static_cast<qint64>(GetTickCount64()) * 1000;
    data_->captureKeystrokes = captureKeystrokes;
    data_->active = true;
    installHook();
    if (captureKeystrokes)
        installKeyboardHook();
    data_->timer.setTimerType(Qt::PreciseTimer);
    data_->timer.setInterval(qRound(1000.0 / std::clamp(fps, 30, 60)));
    data_->timer.start();
    sample();
}

void PointerTracker::pause(qint64 mediaTimeMs)
{
    data_->active = false;
    data_->timer.stop();
    closePresses(mediaTimeMs);
}

void PointerTracker::resume()
{
    data_->active = true;
    const double t = static_cast<double>(mediaClock_ ? mediaClock_() : 0) / 1000.0;
    POINT point;
    const bool havePoint = GetCursorPos(&point) != 0;
    const double x = havePoint ? static_cast<double>(point.x - region_.x()) : 0.0;
    const double y = havePoint ? static_cast<double>(point.y - region_.y()) : 0.0;
    for (size_t i = 0; i < kVirtualButtons.size(); ++i) {
        Press& press = data_->activePresses[i];
        if (press.active || (GetAsyncKeyState(kVirtualButtons[i]) & 0x8000) == 0)
            continue;
        // A button held across a pause keeps holding, but it is not a new click.
        press.active = true;
        press.down = t;
        press.x = x;
        press.y = y;
        press.dragged = false;
    }
    data_->timer.start();
    sample();
}

void PointerTracker::stop(qint64 mediaTimeMs)
{
    updateVisibility(mediaTimeMs);
    data_->active = false;
    data_->timer.stop();
    removeHook();
    closePresses(mediaTimeMs);
}

void PointerTracker::discard()
{
    data_->active = false;
    data_->timer.stop();
    removeHook();
    data_->activePresses = {};
}

bool PointerTracker::installHook()
{
    if (data_->hook)
        return true;
    gHookOwner = this;
    data_->hook = SetWindowsHookExW(
        WH_MOUSE_LL, &PointerTracker::mouseHook, GetModuleHandleW(nullptr), 0);
    data_->hookInstalled = data_->hook != nullptr;
    if (!data_->hookInstalled) {
        gHookOwner = nullptr;
        qWarning().noquote() << "[recording] the low-level mouse hook could not be installed;"
                                " falling back to cursor polling";
    }
    return data_->hookInstalled;
}

bool PointerTracker::installKeyboardHook()
{
    if (data_->keyboardHook)
        return true;
    gHookOwner = this;
    data_->keyboardHook = SetWindowsHookExW(
        WH_KEYBOARD_LL, &PointerTracker::keyboardHook, GetModuleHandleW(nullptr), 0);
    data_->keyboardHookInstalled = data_->keyboardHook != nullptr;
    if (!data_->keyboardHookInstalled) {
        qWarning().noquote() << "[recording] the low-level keyboard hook could not be installed;"
                                " recording without key events";
    }
    return data_->keyboardHookInstalled;
}

void PointerTracker::removeHook()
{
    if (data_->hook) {
        UnhookWindowsHookEx(data_->hook);
        data_->hook = nullptr;
    }
    if (data_->keyboardHook) {
        UnhookWindowsHookEx(data_->keyboardHook);
        data_->keyboardHook = nullptr;
    }
    data_->hookInstalled = false;
    data_->keyboardHookInstalled = false;
    if (gHookOwner == this)
        gHookOwner = nullptr;
}

LRESULT CALLBACK PointerTracker::mouseHook(int code, WPARAM wParam, LPARAM lParam)
{
    PointerTracker* owner = gHookOwner;
    if (code >= 0 && owner && owner->data_ && owner->data_->active) {
        const auto* event = reinterpret_cast<const MSLLHOOKSTRUCT*>(lParam);
        if (event)
            owner->handleMouseEvent(wParam, *event);
    }
    return CallNextHookEx(owner && owner->data_ ? owner->data_->hook : nullptr,
                          code,
                          wParam,
                          lParam);
}

LRESULT CALLBACK PointerTracker::keyboardHook(int code, WPARAM wParam, LPARAM lParam)
{
    PointerTracker* owner = gHookOwner;
    if (code >= 0 && owner && owner->data_ && owner->data_->active
        && owner->data_->captureKeystrokes) {
        const auto* event = reinterpret_cast<const KBDLLHOOKSTRUCT*>(lParam);
        if (event)
            owner->handleKeyEvent(wParam, *event);
    }
    return CallNextHookEx(owner && owner->data_ ? owner->data_->keyboardHook : nullptr,
                          code,
                          wParam,
                          lParam);
}

void PointerTracker::sample()
{
    if (!data_->active)
        return;

    POINT point;
    if (!GetCursorPos(&point))
        return;

    const qint64 mediaTimeMs = mediaClock_ ? mediaClock_() : 0;
    const double t = static_cast<double>(mediaTimeMs) / 1000.0;
    const double x = static_cast<double>(point.x - region_.x());
    const double y = static_cast<double>(point.y - region_.y());

    // GetCursorInfo also reports the shape of the current cursor; it fails on a
    // desktop the process cannot see (the secure desktop, a locked session), in
    // which case the sample keeps the neutral shape.
    CURSORINFO cursorInfo{};
    cursorInfo.cbSize = sizeof(cursorInfo);
    const bool haveCursor = GetCursorInfo(&cursorInfo) != 0;
    CursorKind kind = haveCursor ? cursorKindFor(cursorInfo.hCursor) : CursorKind::Default;
    const QString cursorId = haveCursor ? cursorIdFor(cursorInfo.hCursor, kind) : QString();

    const bool samePoint = data_->haveLastPoint && data_->lastPoint.x == point.x
        && data_->lastPoint.y == point.y;
    const bool shapeChanged =
        samePoint && (data_->lastKind != kind || data_->lastCursorId != cursorId);
    const bool scrollRecent = data_->lastScrollTime >= 0.0
        && t - data_->lastScrollTime < kScrollHoverWindowSeconds;
    if (shapeChanged && scrollRecent) {
        // The shape moved under a stationary pointer because the content
        // scrolled. Keep the old event stream position; the catalog already
        // holds the new shape for the next real hover.
        data_->lastKind = kind;
        data_->lastCursorId = cursorId;
    } else {
        appendPointer(mediaTimeMs, point, kind, cursorId);
    }
    appendVisibility(t, haveCursor && (cursorInfo.flags & CURSOR_SHOWING) != 0);

    // Polling remains the fallback for a hook that could not be installed and
    // for drivers that do not deliver an injected button event. An active press
    // makes the poll idempotent when the hook already saw the same transition.
    for (size_t i = 0; i < kVirtualButtons.size(); ++i) {
        const bool down = (GetAsyncKeyState(kVirtualButtons[i]) & 0x8000) != 0;
        Press& press = data_->activePresses[i];
        if (down && !press.active) {
            press.active = true;
            press.down = t;
            press.x = x;
            press.y = y;
            press.dragged = false;
            data_->clicks.push_back(
                {t, x, y, kSidecarButtons[i], cursorId, currentModifiers()});
            compactToLimit(data_->clicks, kMaxClicks);
        } else if (down && press.active) {
            const double dx = x - press.x;
            const double dy = y - press.y;
            if (dx * dx + dy * dy > kDragThresholdSquared)
                press.dragged = true;
        } else if (!down && press.active) {
            const double dx = x - press.x;
            const double dy = y - press.y;
            if (dx * dx + dy * dy > kDragThresholdSquared)
                press.dragged = true;
            data_->presses.push_back({press.down, t, kSidecarButtons[i], press.dragged});
            compactToLimit(data_->presses, kMaxPresses);
            press = {};
        }
    }
}

void PointerTracker::appendPointer(qint64 mediaTimeMs,
                                   const POINT& point,
                                   CursorKind kind,
                                   const QString& cursorId)
{
    if (data_->haveLastPoint && data_->lastPoint.x == point.x && data_->lastPoint.y == point.y
        && data_->lastKind == kind && data_->lastCursorId == cursorId) {
        return;
    }

    const double t = static_cast<double>(mediaTimeMs) / 1000.0;
    const double x = static_cast<double>(point.x - region_.x());
    const double y = static_cast<double>(point.y - region_.y());
    data_->pointer.push_back({t, x, y, kind, cursorId, currentModifiers()});
    compactToLimit(data_->pointer, kMaxPointerSamples);
    data_->lastPoint = point;
    data_->haveLastPoint = true;
    data_->lastKind = kind;
    data_->lastCursorId = cursorId;

    for (Press& press : data_->activePresses) {
        if (!press.active)
            continue;
        const double dx = x - press.x;
        const double dy = y - press.y;
        if (dx * dx + dy * dy > kDragThresholdSquared)
            press.dragged = true;
    }
}

void PointerTracker::handleMouseEvent(WPARAM message, const MSLLHOOKSTRUCT& event)
{
    if (!data_->active)
        return;

    size_t index = 0;
    bool buttonDown = false;
    bool buttonUp = false;
    switch (message) {
    case WM_LBUTTONDOWN:
        buttonDown = true;
        break;
    case WM_LBUTTONUP:
        buttonUp = true;
        break;
    case WM_MBUTTONDOWN:
        index = 1;
        buttonDown = true;
        break;
    case WM_MBUTTONUP:
        index = 1;
        buttonUp = true;
        break;
    case WM_RBUTTONDOWN:
        index = 2;
        buttonDown = true;
        break;
    case WM_RBUTTONUP:
        index = 2;
        buttonUp = true;
        break;
    default:
        break;
    }

    CURSORINFO cursorInfo{};
    cursorInfo.cbSize = sizeof(cursorInfo);
    const bool haveCursor = GetCursorInfo(&cursorInfo) != 0;
    CursorKind kind = haveCursor ? cursorKindFor(cursorInfo.hCursor) : CursorKind::Default;
    QString cursorId = haveCursor ? cursorIdFor(cursorInfo.hCursor, kind) : QString();
    if (cursorId.isEmpty() && !data_->pointer.empty())
        cursorId = data_->pointer.back().cursorId;

    const qint64 mediaTimeMs = eventMediaTime(event);
    const double t = static_cast<double>(mediaTimeMs) / 1000.0;
    const double x = static_cast<double>(event.pt.x - region_.x());
    const double y = static_cast<double>(event.pt.y - region_.y());
    appendVisibility(t, haveCursor && (cursorInfo.flags & CURSOR_SHOWING) != 0);

    if (message == WM_MOUSEWHEEL || message == WM_MOUSEHWHEEL) {
        appendPointer(mediaTimeMs, event.pt, kind, cursorId);
        data_->scrolls.push_back({t,
                                  x,
                                  y,
                                  static_cast<short>(HIWORD(event.mouseData)),
                                  message == WM_MOUSEHWHEEL,
                                  currentModifiers(),
                                  cursorId});
        compactToLimit(data_->scrolls, kMaxScrolls);
        data_->lastScrollTime = t;
        return;
    }
    if (message == WM_MOUSEMOVE) {
        appendPointer(mediaTimeMs, event.pt, kind, cursorId);
        return;
    }
    if (!buttonDown && !buttonUp)
        return;

    appendPointer(mediaTimeMs, event.pt, kind, cursorId);
    Press& press = data_->activePresses[index];

    if (buttonDown) {
        if (!press.active) {
            press.active = true;
            press.down = t;
            press.x = x;
            press.y = y;
            press.dragged = false;
            data_->clicks.push_back(
                {t, x, y, kSidecarButtons[index], cursorId, currentModifiers()});
            compactToLimit(data_->clicks, kMaxClicks);
        }
    } else if (press.active) {
        const double dx = x - press.x;
        const double dy = y - press.y;
        if (dx * dx + dy * dy > kDragThresholdSquared)
            press.dragged = true;
        data_->presses.push_back(
            {press.down, std::max(t, press.down), kSidecarButtons[index], press.dragged});
        compactToLimit(data_->presses, kMaxPresses);
        press = {};
    }
}

qint64 PointerTracker::eventMediaTime(DWORD eventTime) const
{
    const qint64 mediaNow = mediaClock_ ? mediaClock_() : 0;
    if (eventTime == 0)
        return mediaNow;

    // Low-level hooks report a 32-bit GetTickCount-style time. Keep the
    // event's own time instead of the time the callback happened to run, so a
    // busy frame cannot make two clicks look simultaneous.
    constexpr qint64 wrap = qint64(1) << 32;
    const qint64 nowTick = static_cast<qint64>(GetTickCount64());
    qint64 eventTick = static_cast<qint64>(eventTime) + (nowTick & ~(wrap - 1));
    if (eventTick > nowTick)
        eventTick -= wrap;
    const qint64 delay = std::clamp<qint64>(nowTick - eventTick, 0, 1000);
    return std::max<qint64>(0, mediaNow - delay);
}

qint64 PointerTracker::eventMediaTime(const MSLLHOOKSTRUCT& event) const
{
    return eventMediaTime(event.time);
}

void PointerTracker::handleKeyEvent(WPARAM message, const KBDLLHOOKSTRUCT& event)
{
    if (!data_->active || !data_->captureKeystrokes)
        return;
    const bool down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
    const bool up = message == WM_KEYUP || message == WM_SYSKEYUP;
    if (!down && !up)
        return;

    CURSORINFO cursorInfo{};
    cursorInfo.cbSize = sizeof(cursorInfo);
    const bool haveCursor = GetCursorInfo(&cursorInfo) != 0;
    CursorKind kind = haveCursor ? cursorKindFor(cursorInfo.hCursor) : CursorKind::Default;
    const QString cursorId = haveCursor ? cursorIdFor(cursorInfo.hCursor, kind) : QString();
    const qint64 mediaTimeMs = eventMediaTime(event.time);
    const double t = static_cast<double>(mediaTimeMs) / 1000.0;
    appendVisibility(t, haveCursor && (cursorInfo.flags & CURSOR_SHOWING) != 0);
    data_->keys.push_back({t,
                           down,
                           static_cast<int>(event.vkCode),
                           static_cast<int>(event.scanCode),
                           currentModifiers(),
                           cursorId});
    compactToLimit(data_->keys, kMaxKeys);
}

void PointerTracker::appendVisibility(double t, bool visible)
{
    if (data_->haveVisibility && data_->lastVisible == visible)
        return;
    data_->visibility.push_back({t, visible});
    compactToLimit(data_->visibility, kMaxVisibilityChanges);
    data_->lastVisible = visible;
    data_->haveVisibility = true;
}

void PointerTracker::updateVisibility(qint64 mediaTimeMs)
{
    CURSORINFO cursorInfo{};
    cursorInfo.cbSize = sizeof(cursorInfo);
    const bool haveCursor = GetCursorInfo(&cursorInfo) != 0;
    appendVisibility(static_cast<double>(mediaTimeMs) / 1000.0,
                     haveCursor && (cursorInfo.flags & CURSOR_SHOWING) != 0);
}

QString PointerTracker::cursorIdFor(HCURSOR cursor, CursorKind& kind)
{
    if (!cursor)
        return {};

    const quintptr key = reinterpret_cast<quintptr>(cursor);
    const auto cached = data_->cursorIds.constFind(key);
    if (cached != data_->cursorIds.constEnd()) {
        const auto cachedKind = data_->cursorKinds.constFind(key);
        if (cachedKind != data_->cursorKinds.constEnd())
            kind = cachedKind.value();
        return cached.value();
    }

    QPoint hotspot;
    const QImage image = cursorImageFromHandle(cursor, &hotspot);
    if (image.isNull())
        return {};

    // A handle that is not one of the shared standards may still draw one: an
    // app often installs its own arrow or I-beam. Matching the bitmap recovers
    // the semantic kind so the chosen theme can restyle it; only a shape no
    // standard matches stays a recorded-only `custom-` cursor.
    bool standard = false;
    {
        const StandardCursorHandles& table = standardCursorHandles();
        for (size_t i = 0; i < kStandardCursors.size(); ++i) {
            if (table.handles[i] && cursor == table.handles[i]) {
                standard = true;
                break;
            }
        }
    }
    if (!standard) {
        CursorKind matched = kind;
        if (standardKindForBitmap(image, &matched)) {
            kind = matched;
            standard = true;
        }
    }

    QCryptographicHash hash(QCryptographicHash::Sha256);
    hash.addData(QByteArray::fromRawData(reinterpret_cast<const char*>(image.constBits()),
                                         int(image.sizeInBytes())));
    const int width = image.width();
    const int height = image.height();
    const int hotspotX = hotspot.x();
    const int hotspotY = hotspot.y();
    hash.addData(reinterpret_cast<const char*>(&width), sizeof(width));
    hash.addData(reinterpret_cast<const char*>(&height), sizeof(height));
    hash.addData(reinterpret_cast<const char*>(&hotspotX), sizeof(hotspotX));
    hash.addData(reinterpret_cast<const char*>(&hotspotY), sizeof(hotspotY));
    hash.addData(cursorKindName(kind).toUtf8());
    const QString prefix = standard ? QStringLiteral("std-") : QStringLiteral("custom-");
    const QString id = prefix + QString::fromLatin1(hash.result().toHex().left(16));

    if (!data_->cursors.contains(id)) {
        CursorShape shape;
        shape.id = id;
        shape.file = id + QStringLiteral(".png");
        shape.width = width;
        shape.height = height;
        shape.hotspotX = hotspot.x();
        shape.hotspotY = hotspot.y();
        shape.kind = kind;
        shape.image = image;
        data_->cursors.insert(id, shape);
    }
    data_->cursorIds.insert(key, id);
    data_->cursorKinds.insert(key, kind);
    return id;
}

void PointerTracker::closePresses(qint64 mediaTimeMs)
{
    const double t = static_cast<double>(mediaTimeMs) / 1000.0;
    POINT point;
    const bool havePoint = GetCursorPos(&point) != 0;
    const double x = havePoint ? static_cast<double>(point.x - region_.x()) : 0.0;
    const double y = havePoint ? static_cast<double>(point.y - region_.y()) : 0.0;
    for (size_t i = 0; i < data_->activePresses.size(); ++i) {
        Press& press = data_->activePresses[i];
        if (!press.active)
            continue;
        if (havePoint) {
            const double dx = x - press.x;
            const double dy = y - press.y;
            if (dx * dx + dy * dy > kDragThresholdSquared)
                press.dragged = true;
        }
        data_->presses.push_back(
            {press.down, std::max(t, press.down), kSidecarButtons[i], press.dragged});
        compactToLimit(data_->presses, kMaxPresses);
        press = {};
    }
}

bool PointerTracker::writeSidecar(const QString& videoPath,
                                  const WebcamTrack& webcam,
                                  QString* error) const
{
    const QString path = sidecarFilePath(videoPath);
    if (path.isEmpty()) {
        if (error)
            *error = QStringLiteral("Could not canonicalize the recording path or find LOCALAPPDATA.");
        return false;
    }

    QJsonArray pointer;
    for (const PointerSample& sample : data_->pointer)
        pointer.append(pointerObject(sample));

    QJsonArray clicks;
    for (const ClickSample& sample : data_->clicks)
        clicks.append(clickObject(sample));

    QJsonArray presses;
    for (const PressSample& sample : data_->presses)
        presses.append(pressObject(sample));

    QJsonArray scrolls;
    for (const ScrollSample& sample : data_->scrolls)
        scrolls.append(scrollObject(sample));

    QJsonArray keys;
    for (const KeySample& sample : data_->keys)
        keys.append(keyObject(sample));

    QJsonArray visibility;
    for (const VisibilitySample& sample : data_->visibility)
        visibility.append(visibilityObject(sample));

    const QJsonObject region = {
        {QStringLiteral("x"), region_.x()},
        {QStringLiteral("y"), region_.y()},
        {QStringLiteral("w"), static_cast<int>(region_.width())},
        {QStringLiteral("h"), static_cast<int>(region_.height())},
    };
    const QJsonObject sidecar = {
        {QStringLiteral("version"), 2},
        {QStringLiteral("t0_monotonic_us"), static_cast<double>(data_->t0MonotonicUs)},
        {QStringLiteral("region"), region},
        {QStringLiteral("pointer"), pointer},
        {QStringLiteral("clicks"), clicks},
        {QStringLiteral("visibility"), visibility},
        {QStringLiteral("presses"), presses},
        {QStringLiteral("scrolls"), scrolls},
        {QStringLiteral("keys"), keys},
        {QStringLiteral("source"), QStringLiteral("recorded")},
    };
    QJsonObject document = sidecar;

    // Cursor images live beside the sidecar as a catalog: each sample's
    // cursor_id points at a PNG and its hotspot travels with it. The studied
    // recorder stores the same split, so the editor can draw the real shape
    // instead of guessing from a name.
    const QFileInfo sidecarInfo(path);
    const QString cursorFolderName = sidecarInfo.fileName() + QStringLiteral(".cursors");
    const QString cursorFolder = sidecarInfo.dir().filePath(cursorFolderName);
    QDir(cursorFolder).removeRecursively();
    QJsonArray cursors;
    if (!data_->cursors.isEmpty() && QDir().mkpath(cursorFolder)) {
        QStringList ids = data_->cursors.keys();
        std::sort(ids.begin(), ids.end());
        for (const QString& id : ids) {
            const CursorShape& shape = data_->cursors.value(id);
            if (!shape.image.save(QDir(cursorFolder).filePath(shape.file), "PNG"))
                continue;
            cursors.append(QJsonObject{
                {QStringLiteral("id"), shape.id},
                {QStringLiteral("file"), shape.file},
                {QStringLiteral("width"), shape.width},
                {QStringLiteral("height"), shape.height},
                {QStringLiteral("hotspot_x"), shape.hotspotX},
                {QStringLiteral("hotspot_y"), shape.hotspotY},
                {QStringLiteral("kind"), cursorKindName(shape.kind)},
            });
        }
    }
    if (!cursors.isEmpty()) {
        document.insert(QStringLiteral("cursor_images"), cursorFolderName);
        document.insert(QStringLiteral("cursors"), cursors);
    }

    if (webcam.is_valid()) {
        QJsonObject camera{
                            {QStringLiteral("file"), QFileInfo(webcam.file).fileName()},
                            {QStringLiteral("width"), webcam.width},
                            {QStringLiteral("height"), webcam.height},
                            {QStringLiteral("duration"), webcam.duration},
                            {QStringLiteral("start"), webcam.start},
                            {QStringLiteral("x"), webcam.x},
                            {QStringLiteral("y"), webcam.y},
                            {QStringLiteral("w"), webcam.w},
                            {QStringLiteral("h"), webcam.h},
                            {QStringLiteral("shape"), webcam.shape},
                            {QStringLiteral("flip"), webcam.flip},
                        };
        document.insert(QStringLiteral("webcam"), camera);
        if (!webcam.screenFile.isEmpty())
            document.insert(QStringLiteral("screen_file"), QFileInfo(webcam.screenFile).fileName());
    }

    const QFileInfo info(path);
    if (!QDir().mkpath(info.absolutePath())) {
        if (error)
            *error = QStringLiteral("Could not create the pointer sidecar directory.");
        return false;
    }

    QSaveFile file(path);
    if (!file.open(QIODevice::WriteOnly)) {
        if (error)
            *error = file.errorString();
        return false;
    }
    const QByteArray json = QJsonDocument(document).toJson(QJsonDocument::Indented);
    if (file.write(json) != json.size() || !file.commit()) {
        if (error)
            *error = file.errorString();
        return false;
    }
    return true;
}

QString canonicalVideoPathFor(const QString& videoPath)
{
    const QString nativePath = QDir::toNativeSeparators(videoPath);
    HANDLE file = CreateFileW(reinterpret_cast<LPCWSTR>(nativePath.utf16()),
                              FILE_READ_ATTRIBUTES,
                              FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                              nullptr,
                              OPEN_EXISTING,
                              FILE_ATTRIBUTE_NORMAL,
                              nullptr);
    if (file == INVALID_HANDLE_VALUE)
        return QString();

    const DWORD flags = FILE_NAME_NORMALIZED | VOLUME_NAME_DOS;
    const DWORD required = GetFinalPathNameByHandleW(file, nullptr, 0, flags);
    if (required == 0) {
        CloseHandle(file);
        return QString();
    }

    QString canonical(static_cast<int>(required), Qt::Uninitialized);
    const DWORD length = GetFinalPathNameByHandleW(file,
                                                   reinterpret_cast<LPWSTR>(canonical.data()),
                                                   required,
                                                   flags);
    CloseHandle(file);
    if (length == 0 || length >= required)
        return QString();
    canonical.resize(static_cast<int>(length));
    return canonical;
}

QString sidecarPathFor(const QString& videoPath)
{
    return sidecarFilePath(videoPath);
}

QString webcamTrackPathFor(const QString& videoPath)
{
    const QString sidecar = sidecarFilePath(videoPath);
    if (sidecar.isEmpty())
        return QString();
    return sidecar + QStringLiteral(".webcam.mp4");
}

QString cursorKindName(CursorKind kind)
{
    switch (kind) {
    case CursorKind::Hand:
        return QStringLiteral("hand");
    case CursorKind::Text:
        return QStringLiteral("text");
    case CursorKind::Crosshair:
        return QStringLiteral("crosshair");
    case CursorKind::Wait:
        return QStringLiteral("wait");
    case CursorKind::Resize:
        return QStringLiteral("resize");
    case CursorKind::Default:
        break;
    }
    return QStringLiteral("default");
}

CursorKind cursorKindFor(HCURSOR cursor)
{
    if (!cursor)
        return CursorKind::Default;

    // Only the standard cursors can be recognised by handle. An application
    // that installs its own cursor (a browser's custom hand, a toolkit's drawn
    // resize handle) hands back an HCURSOR that is none of these, so it records
    // as default; the shape is not recovered from the bitmap, which would mean
    // guessing from pixels.
    const StandardCursorHandles& standard = standardCursorHandles();
    for (size_t i = 0; i < kStandardCursors.size(); ++i) {
        if (standard.handles[i] && cursor == standard.handles[i])
            return kStandardCursors[i].kind;
    }
    return CursorKind::Default;
}

} // namespace Platform
