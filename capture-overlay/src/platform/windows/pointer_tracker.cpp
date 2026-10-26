#include "platform/windows/pointer_tracker.h"

#include <QCryptographicHash>
#include <QDir>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSaveFile>
#include <QTimer>

#include <algorithm>
#include <array>
#include <utility>
#include <vector>

namespace Platform {

namespace {

constexpr int kMaxPointerSamples = 8000;
constexpr int kMaxClicks = 500;
constexpr int kMaxPresses = 500;
constexpr double kDragThresholdSquared = 16.0;
constexpr std::array<int, 3> kVirtualButtons = {VK_LBUTTON, VK_MBUTTON, VK_RBUTTON};
constexpr std::array<int, 3> kSidecarButtons = {1, 2, 3};

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

struct PointerSample {
    double t = 0.0;
    double x = 0.0;
    double y = 0.0;
    CursorKind kind = CursorKind::Default;
};

struct ClickSample {
    double t = 0.0;
    double x = 0.0;
    double y = 0.0;
    int button = 0;
};

struct PressSample {
    double down = 0.0;
    double up = 0.0;
    int button = 0;
    bool dragged = false;
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
    return {{QStringLiteral("t"), sample.t},
            {QStringLiteral("x"), sample.x},
            {QStringLiteral("y"), sample.y},
            {QStringLiteral("kind"), cursorKindName(sample.kind)}};
}

QJsonObject clickObject(const ClickSample& sample)
{
    return {{QStringLiteral("t"), sample.t},
            {QStringLiteral("x"), sample.x},
            {QStringLiteral("y"), sample.y},
            {QStringLiteral("button"), sample.button}};
}

QJsonObject pressObject(const PressSample& sample)
{
    return {{QStringLiteral("down"), sample.down},
            {QStringLiteral("up"), sample.up},
            {QStringLiteral("button"), sample.button},
            {QStringLiteral("dragged"), sample.dragged}};
}

} // namespace

class PointerTracker::Data
{
public:
    QTimer timer;
    std::vector<PointerSample> pointer;
    std::vector<ClickSample> clicks;
    std::vector<PressSample> presses;
    std::array<Press, 3> activePresses;
    // Monotonic origin of this recording session, in microseconds.
    qint64 t0MonotonicUs = 0;
};

PointerTracker::PointerTracker(QObject* parent)
    : QObject(parent)
    , data_(std::make_unique<Data>())
{
    QObject::connect(&data_->timer, &QTimer::timeout, this, [this] { sample(); });
}

PointerTracker::~PointerTracker() = default;

void PointerTracker::reset()
{
    data_->timer.stop();
    data_->pointer.clear();
    data_->clicks.clear();
    data_->presses.clear();
    data_->activePresses = {};
}

void PointerTracker::start(const QRect& region, int fps, MediaClock mediaClock)
{
    reset();
    region_ = region;
    mediaClock_ = std::move(mediaClock);
    data_->t0MonotonicUs = static_cast<qint64>(GetTickCount64()) * 1000;
    data_->timer.setTimerType(Qt::PreciseTimer);
    data_->timer.setInterval(qRound(1000.0 / std::clamp(fps, 30, 60)));
    data_->timer.start();
}

void PointerTracker::pause(qint64 mediaTimeMs)
{
    data_->timer.stop();
    closePresses(mediaTimeMs);
}

void PointerTracker::resume()
{
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
}

void PointerTracker::stop(qint64 mediaTimeMs)
{
    data_->timer.stop();
    closePresses(mediaTimeMs);
}

void PointerTracker::discard()
{
    data_->timer.stop();
    data_->activePresses = {};
}

void PointerTracker::sample()
{
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
    const CursorKind kind = GetCursorInfo(&cursorInfo) ? cursorKindFor(cursorInfo.hCursor)
                                                      : CursorKind::Default;

    data_->pointer.push_back({t, x, y, kind});
    compactToLimit(data_->pointer, kMaxPointerSamples);

    for (size_t i = 0; i < kVirtualButtons.size(); ++i) {
        const bool down = (GetAsyncKeyState(kVirtualButtons[i]) & 0x8000) != 0;
        Press& press = data_->activePresses[i];
        if (down && !press.active) {
            press.active = true;
            press.down = t;
            press.x = x;
            press.y = y;
            press.dragged = false;
            data_->clicks.push_back({t, x, y, kSidecarButtons[i]});
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
        data_->presses.push_back({press.down, t, kSidecarButtons[i], press.dragged});
        compactToLimit(data_->presses, kMaxPresses);
        press = {};
    }
}

bool PointerTracker::writeSidecar(const QString& videoPath, QString* error) const
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
        {QStringLiteral("visibility"), QJsonArray{}},
        {QStringLiteral("presses"), presses},
        {QStringLiteral("source"), QStringLiteral("recorded")},
    };

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
    const QByteArray json = QJsonDocument(sidecar).toJson(QJsonDocument::Indented);
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
