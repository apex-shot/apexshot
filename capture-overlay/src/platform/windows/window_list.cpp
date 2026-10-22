#include "platform/windows/win_util.h"

#include "platform/platform.h"

#include <QDir>
#include <QFile>
#include <QTextStream>

#include <memory>

namespace {

struct EnumContext {
    QVector<Platform::WindowInfo>* out = nullptr;
    DWORD ownProcess = 0;
    QTextStream* debug = nullptr;
};

void trace(EnumContext* context, HWND hwnd, const char* verdict)
{
    if (!context->debug)
        return;

    wchar_t title[512] = {};
    GetWindowTextW(hwnd, title, static_cast<int>(std::size(title)));
    wchar_t cls[256] = {};
    GetClassNameW(hwnd, cls, static_cast<int>(std::size(cls)));
    const LONG_PTR exStyle = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
    const QRect frame = Platform::Win::visibleFrame(hwnd);

    *context->debug << QString::fromLatin1(verdict) << " | "
                    << QString::fromWCharArray(title) << " | "
                    << QString::fromWCharArray(cls) << " | " << frame.width() << "x"
                    << frame.height() << " | ex=0x"
                    << QString::number(static_cast<qulonglong>(exStyle), 16) << " | tool="
                    << ((exStyle & WS_EX_TOOLWINDOW) ? 1 : 0) << " appwindow="
                    << ((exStyle & WS_EX_APPWINDOW) ? 1 : 0) << " cloak="
                    << (Platform::Win::isCloaked(hwnd) ? 1 : 0) << " iconic="
                    << (IsIconic(hwnd) ? 1 : 0) << "\n";
}

QString windowTitle(HWND hwnd)
{
    wchar_t buffer[512];
    const int copied = GetWindowTextW(hwnd, buffer, static_cast<int>(std::size(buffer)));
    if (copied <= 0)
        return {};
    return QString::fromWCharArray(buffer, copied);
}

QString windowClassName(HWND hwnd)
{
    wchar_t buffer[256];
    const int copied = GetClassNameW(hwnd, buffer, static_cast<int>(std::size(buffer)));
    if (copied <= 0)
        return {};
    return QString::fromWCharArray(buffer, copied);
}

BOOL CALLBACK enumWindowProc(HWND hwnd, LPARAM userData)
{
    auto* context = reinterpret_cast<EnumContext*>(userData);
    if (!context || !context->out)
        return FALSE;

    if (!IsWindowVisible(hwnd)) {
        trace(context, hwnd, "skip invisible");
        return TRUE;
    }

    const LONG_PTR exStyle = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
    DWORD processId = 0;
    GetWindowThreadProcessId(hwnd, &processId);
    const QString title = windowTitle(hwnd);
    const QString className = windowClassName(hwnd);
    const bool iconic = IsIconic(hwnd) != 0;

    // A minimized window reports only its caption through DWM (e.g. 183x34),
    // so its real bounds have to come from the placement record.
    QRect frame;
    if (iconic) {
        WINDOWPLACEMENT placement{};
        placement.length = sizeof(placement);
        if (GetWindowPlacement(hwnd, &placement))
            frame = QRect(QPoint(placement.rcNormalPosition.left, placement.rcNormalPosition.top),
                          QSize(placement.rcNormalPosition.right - placement.rcNormalPosition.left,
                                placement.rcNormalPosition.bottom - placement.rcNormalPosition.top));
    }
    if (frame.isNull())
        frame = Platform::Win::visibleFrame(hwnd);

    // The shell's own surfaces are top-level windows too, and they must never
    // reach the picker.
    const bool shellSurface = className == QStringLiteral("Progman")
        || className == QStringLiteral("WorkerW")
        || className == QStringLiteral("Shell_TrayWnd")
        || className == QStringLiteral("ShellHandwritingCanvas");

    // Same rule the task switcher uses: an app window is either flagged
    // WS_EX_APPWINDOW or is unowned and not a tool window. Without it, tool
    // windows and shell canvases crowd out real windows.
    const bool isAppWindow = (exStyle & WS_EX_APPWINDOW) != 0;
    const bool isToolWindow = (exStyle & WS_EX_TOOLWINDOW) != 0;
    const bool unowned = GetWindow(hwnd, GW_OWNER) == nullptr;
    const bool altTabWorthy = isAppWindow || (!isToolWindow && unowned);

    const char* verdict = "accept";
    if (processId == context->ownProcess)
        verdict = "skip own process";
    else if (title.trimmed().isEmpty())
        verdict = "skip untitled";
    else if (shellSurface)
        verdict = "skip shell surface";
    else if (!altTabWorthy)
        verdict = "skip not alt-tab worthy";
    else if (!iconic && (frame.width() < 48 || frame.height() < 48))
        verdict = "skip too small";
    else if (Platform::Win::isCloaked(hwnd))
        verdict = "skip cloaked";

    trace(context, hwnd, verdict);

    if (verdict[0] != 'a')
        return TRUE;

    Platform::WindowInfo info;
    info.id = reinterpret_cast<quintptr>(hwnd);
    info.title = title;
    info.className = className;
    info.processId = processId;
    info.geometry = frame;
    info.minimized = iconic;
    context->out->append(info);
    return TRUE;
}

} // namespace

namespace Platform {

QVector<WindowInfo> windows()
{
    QVector<WindowInfo> result;
    EnumContext context;
    context.out = &result;
    context.ownProcess = GetCurrentProcessId();

    QFile logFile;
    std::unique_ptr<QTextStream> stream;
    if (qEnvironmentVariableIsSet("APEXSHOT_WINDOW_DEBUG")) {
        logFile.setFileName(QDir(QDir::tempPath()).filePath(
            QStringLiteral("apexshot-windowlist.log")));
        if (logFile.open(QIODevice::WriteOnly | QIODevice::Truncate)) {
            stream = std::make_unique<QTextStream>(&logFile);
            context.debug = stream.get();
        }
    }

    // EnumWindows walks the z-order from topmost down, which is the order the
    // picker wants.
    EnumWindows(enumWindowProc, reinterpret_cast<LPARAM>(&context));

    if (context.debug) {
        *context.debug << "---- picked ----\n";
        for (const WindowInfo& window : result) {
            *context.debug << "picked | " << window.title << " | " << window.geometry.width()
                           << "x" << window.geometry.height() << " at " << window.geometry.x()
                           << "," << window.geometry.y() << " | min=" << window.minimized
                           << "\n";
        }
        context.debug->flush();
    }

    return result;
}

} // namespace Platform
