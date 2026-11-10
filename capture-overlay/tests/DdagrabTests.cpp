// Unit checks for the DXGI output and crop mapping used by the recorder.

#include "platform/platform.h"
#include "platform/windows/ddagrab.h"

#include <windows.h>
#include <objbase.h>

#include <cstdio>

namespace {

bool expect(bool condition, const char* message)
{
    if (!condition)
        std::printf("FAIL %s\n", message);
    return condition;
}

Platform::MonitorInfo monitor(const QString& name, const QRect& geometry, bool primary)
{
    Platform::MonitorInfo info;
    info.name = name;
    info.geometry = geometry;
    info.workArea = geometry;
    info.primary = primary;
    return info;
}

Platform::DdagrabOutput output(int index, const QString& name, const QRect& bounds)
{
    Platform::DdagrabOutput info;
    info.index = index;
    info.deviceName = name;
    info.bounds = bounds;
    return info;
}

bool testFullDisplay()
{
    const QVector<Platform::MonitorInfo> monitors{
        monitor(QStringLiteral("\\\\.\\DISPLAY1"), QRect(0, 0, 1920, 1080), true),
    };
    const QVector<Platform::DdagrabOutput> outputs{
        output(0, QStringLiteral("\\\\.\\DISPLAY1"), QRect(0, 0, 1920, 1080)),
    };
    const Platform::DdagrabSource source =
        Platform::ddagrabSourceForRegion(QRect(0, 0, 1920, 1080), monitors, outputs);
    return expect(source.valid, "a full display maps to an output")
        && expect(source.outputIndex == 0, "the full display uses output 0")
        && expect(source.offset == QPoint(0, 0), "the full display starts at 0,0")
        && expect(source.size == QSize(1920, 1080), "the full display keeps its size");
}

bool testAreaOffset()
{
    const QVector<Platform::MonitorInfo> monitors{
        monitor(QStringLiteral("\\\\.\\DISPLAY1"), QRect(0, 0, 1920, 1080), true),
    };
    const QVector<Platform::DdagrabOutput> outputs{
        output(0, QStringLiteral("\\\\.\\DISPLAY1"), QRect(0, 0, 1920, 1080)),
    };
    const Platform::DdagrabSource source =
        Platform::ddagrabSourceForRegion(QRect(100, 200, 640, 360), monitors, outputs);
    return expect(source.valid, "an area maps to an output")
        && expect(source.offset == QPoint(100, 200), "the area keeps its local offset")
        && expect(source.size == QSize(640, 360), "the area keeps its size");
}

bool testSecondaryDisplay()
{
    const QVector<Platform::MonitorInfo> monitors{
        monitor(QStringLiteral("\\\\.\\DISPLAY1"), QRect(0, 0, 1920, 1080), true),
        monitor(QStringLiteral("\\\\.\\DISPLAY2"), QRect(1920, 0, 2560, 1440), false),
    };
    const QVector<Platform::DdagrabOutput> outputs{
        output(0, QStringLiteral("\\\\.\\DISPLAY1"), QRect(0, 0, 1920, 1080)),
        output(1, QStringLiteral("\\\\.\\DISPLAY2"), QRect(1920, 0, 2560, 1440)),
    };
    const Platform::DdagrabSource source =
        Platform::ddagrabSourceForRegion(QRect(2020, 100, 800, 600), monitors, outputs);
    return expect(source.valid, "a secondary-display area maps to an output")
        && expect(source.outputIndex == 1, "the secondary area uses output 1")
        && expect(source.offset == QPoint(100, 100), "the secondary area is output-local")
        && expect(source.size == QSize(800, 600), "the secondary area keeps its size");
}

bool testNameMismatchFallsBackToBounds()
{
    const QVector<Platform::MonitorInfo> monitors{
        monitor(QStringLiteral("\\\\.\\DISPLAY9"), QRect(0, 0, 1920, 1080), true),
    };
    const QVector<Platform::DdagrabOutput> outputs{
        output(0, QStringLiteral("\\\\.\\DISPLAY1"), QRect(0, 0, 1920, 1080)),
    };
    const Platform::DdagrabSource source =
        Platform::ddagrabSourceForRegion(QRect(40, 50, 320, 240), monitors, outputs);
    return expect(source.valid, "a display-name mismatch falls back to bounds")
        && expect(source.outputIndex == 0, "the bounds match keeps the output index")
        && expect(source.offset == QPoint(40, 50), "the bounds match keeps the crop");
}

bool testSpanningRegionIsRejected()
{
    const QVector<Platform::MonitorInfo> monitors{
        monitor(QStringLiteral("\\\\.\\DISPLAY1"), QRect(0, 0, 1920, 1080), true),
        monitor(QStringLiteral("\\\\.\\DISPLAY2"), QRect(1920, 0, 2560, 1440), false),
    };
    const QVector<Platform::DdagrabOutput> outputs{
        output(0, QStringLiteral("\\\\.\\DISPLAY1"), QRect(0, 0, 1920, 1080)),
        output(1, QStringLiteral("\\\\.\\DISPLAY2"), QRect(1920, 0, 2560, 1440)),
    };
    const Platform::DdagrabSource source =
        Platform::ddagrabSourceForRegion(QRect(1800, 0, 400, 400), monitors, outputs);
    return expect(!source.valid, "a region spanning displays is rejected")
        && expect(!source.reason.isEmpty(), "a rejected region explains why");
}

bool testEmptyAndUnmatchedRegions()
{
    const QVector<Platform::MonitorInfo> monitors{
        monitor(QStringLiteral("\\\\.\\DISPLAY1"), QRect(0, 0, 1920, 1080), true),
    };
    const QVector<Platform::DdagrabOutput> outputs{
        output(0, QStringLiteral("\\\\.\\DISPLAY1"), QRect(0, 0, 1920, 1080)),
    };
    const Platform::DdagrabSource empty =
        Platform::ddagrabSourceForRegion(QRect(), monitors, outputs);
    const Platform::DdagrabSource unmatched =
        Platform::ddagrabSourceForRegion(QRect(10, 10, 100, 100), monitors, {});
    return expect(!empty.valid, "an empty region is rejected")
        && expect(!unmatched.valid, "a region without a matching output is rejected");
}

bool testFilterString()
{
    Platform::DdagrabSource source;
    source.valid = true;
    source.outputIndex = 1;
    source.offset = QPoint(100, 200);
    source.size = QSize(800, 600);
    const QString filter = Platform::ddagrabFilter(source, 30, false);
    const QString expected =
        QStringLiteral("ddagrab=output_idx=1:draw_mouse=0:framerate=30:video_size=800x600"
                       ":offset_x=100:offset_y=200");
    return expect(filter == expected, "the ddagrab filter string is stable")
        && expect(Platform::ddagrabFilter(source, 60, true).contains(
                      QStringLiteral(":draw_mouse=1")),
                  "the filter carries the cursor setting")
        && expect(Platform::ddagrabFilter(source, 60, true).contains(
                      QStringLiteral(":framerate=60")),
                  "the filter carries the frame rate")
        && expect(Platform::ddagrabFilter(Platform::DdagrabSource{}, 30, false).isEmpty(),
                  "an invalid source produces no filter");
}

bool testLiveOutputs()
{
    QString error;
    const QVector<Platform::DdagrabOutput> outputs = Platform::dxgiOutputs(&error);
    if (outputs.isEmpty()) {
        std::printf("SKIP: no DXGI outputs: %s\n", qPrintable(error));
        return true;
    }

    const QVector<Platform::MonitorInfo> monitors = Platform::monitors();
    bool ok = true;
    for (const Platform::DdagrabOutput& output : outputs) {
        if (output.index < 0 || output.deviceName.isEmpty() || output.bounds.isEmpty()) {
            std::printf("FAIL live DXGI output is malformed\n");
            ok = false;
            continue;
        }
        std::printf("DXGI out=%d name=%s bounds=%d,%d %dx%d\n",
                    output.index,
                    qPrintable(output.deviceName),
                    output.bounds.x(),
                    output.bounds.y(),
                    output.bounds.width(),
                    output.bounds.height());
        for (const Platform::MonitorInfo& monitor : monitors) {
            if (monitor.name.compare(output.deviceName, Qt::CaseInsensitive) != 0)
                continue;
            if (monitor.geometry != output.bounds) {
                std::printf("FAIL %s reports %dx%d at %d,%d but DXGI reports %dx%d at %d,%d\n",
                            qPrintable(monitor.name),
                            monitor.geometry.width(),
                            monitor.geometry.height(),
                            monitor.geometry.x(),
                            monitor.geometry.y(),
                            output.bounds.width(),
                            output.bounds.height(),
                            output.bounds.x(),
                            output.bounds.y());
                ok = false;
            }
            break;
        }
    }
    return ok;
}

} // namespace

int main()
{
    // The recorder runs per-monitor DPI aware before it builds any capture
    // arguments. Match that here so monitor and DXGI rectangles are both in
    // device pixels.
    Platform::enableDpiAwareness();
    const HRESULT com = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    const bool ok = testFullDisplay() && testAreaOffset() && testSecondaryDisplay()
        && testNameMismatchFallsBackToBounds() && testSpanningRegionIsRejected()
        && testEmptyAndUnmatchedRegions() && testFilterString() && testLiveOutputs();
    if (SUCCEEDED(com))
        CoUninitialize();
    if (!ok)
        return 1;
    std::printf("DDAGRAB_TESTS=ok\n");
    return 0;
}
