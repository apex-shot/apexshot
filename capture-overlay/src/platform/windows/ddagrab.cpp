#include "platform/windows/ddagrab.h"

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif

#include <windows.h>

#include <dxgi1_2.h>

#include <algorithm>

namespace {

QString hresultText(HRESULT result)
{
    return QStringLiteral("0x%1").arg(static_cast<qulonglong>(result), 8, 16, QLatin1Char('0'));
}

} // namespace

namespace Platform {

QVector<DdagrabOutput> dxgiOutputs(QString* error)
{
    QVector<DdagrabOutput> outputs;
    IDXGIFactory1* factory = nullptr;
    const HRESULT created = CreateDXGIFactory1(IID_PPV_ARGS(&factory));
    if (FAILED(created) || !factory) {
        if (error)
            *error = QStringLiteral("the DXGI factory could not be created (%1)")
                         .arg(hresultText(created));
        return outputs;
    }

    int index = 0;
    for (UINT adapterIndex = 0;; ++adapterIndex) {
        IDXGIAdapter1* adapter = nullptr;
        const HRESULT adapterResult = factory->EnumAdapters1(adapterIndex, &adapter);
        if (adapterResult == DXGI_ERROR_NOT_FOUND)
            break;
        if (FAILED(adapterResult) || !adapter) {
            if (error)
                *error = QStringLiteral("DXGI adapter %1 could not be opened (%2)")
                             .arg(adapterIndex)
                             .arg(hresultText(adapterResult));
            factory->Release();
            return {};
        }

        for (UINT outputIndex = 0;; ++outputIndex) {
            IDXGIOutput* output = nullptr;
            const HRESULT outputResult = adapter->EnumOutputs(outputIndex, &output);
            if (outputResult == DXGI_ERROR_NOT_FOUND)
                break;
            if (FAILED(outputResult) || !output) {
                if (error)
                    *error = QStringLiteral("DXGI output %1 on adapter %2 could not be opened (%3)")
                                 .arg(outputIndex)
                                 .arg(adapterIndex)
                                 .arg(hresultText(outputResult));
                adapter->Release();
                factory->Release();
                return {};
            }

            DXGI_OUTPUT_DESC description{};
            if (SUCCEEDED(output->GetDesc(&description))
                && description.AttachedToDesktop) {
                DdagrabOutput entry;
                // ffmpeg numbers every enumerated output, so the local index
                // has to advance even for outputs that are not attached.
                entry.index = index;
                entry.deviceName = QString::fromWCharArray(description.DeviceName);
                entry.bounds = QRect(QPoint(description.DesktopCoordinates.left,
                                            description.DesktopCoordinates.top),
                                     QSize(description.DesktopCoordinates.right
                                               - description.DesktopCoordinates.left,
                                           description.DesktopCoordinates.bottom
                                               - description.DesktopCoordinates.top));
                outputs.append(entry);
            }

            ++index;
            output->Release();
        }

        adapter->Release();
    }

    factory->Release();
    if (outputs.isEmpty() && error)
        *error = QStringLiteral("no attached DXGI outputs were found");
    return outputs;
}

DdagrabSource ddagrabSourceForRegion(const QRect& region,
                                     const QVector<MonitorInfo>& monitors,
                                     const QVector<DdagrabOutput>& outputs)
{
    DdagrabSource source;
    if (region.width() <= 0 || region.height() <= 0) {
        source.reason = QStringLiteral("the capture region is empty");
        return source;
    }

    const MonitorInfo* monitor = nullptr;
    for (const MonitorInfo& candidate : monitors) {
        if (candidate.geometry.contains(region)) {
            monitor = &candidate;
            break;
        }
    }
    if (!monitor) {
        source.reason = QStringLiteral("the capture region leaves or spans displays");
        return source;
    }

    const DdagrabOutput* output = nullptr;
    for (const DdagrabOutput& candidate : outputs) {
        if (!candidate.deviceName.isEmpty()
            && candidate.deviceName.compare(monitor->name, Qt::CaseInsensitive) == 0) {
            output = &candidate;
            break;
        }
    }
    if (!output) {
        // Display names are stable, but a driver that reports a different name
        // than EnumDisplayMonitors still has the same desktop rectangle.
        for (const DdagrabOutput& candidate : outputs) {
            if (candidate.bounds == monitor->geometry) {
                output = &candidate;
                break;
            }
        }
    }
    if (!output) {
        source.reason = QStringLiteral("no DXGI output matches display %1").arg(monitor->name);
        return source;
    }

    source.offset = region.topLeft() - monitor->geometry.topLeft();
    source.size = region.size();
    if (source.offset.x() < 0 || source.offset.y() < 0
        || source.offset.x() + source.size.width() > monitor->geometry.width()
        || source.offset.y() + source.size.height() > monitor->geometry.height()) {
        source.reason = QStringLiteral("the capture region leaves display %1").arg(monitor->name);
        return source;
    }

    source.valid = true;
    source.outputIndex = output->index;
    return source;
}

DdagrabSource ddagrabSourceForRegion(const QRect& region,
                                     const QVector<MonitorInfo>& monitors)
{
    QString error;
    const QVector<DdagrabOutput> outputs = dxgiOutputs(&error);
    if (outputs.isEmpty()) {
        DdagrabSource source;
        source.reason = error.isEmpty() ? QStringLiteral("no DXGI outputs were found") : error;
        return source;
    }
    return ddagrabSourceForRegion(region, monitors, outputs);
}

QString ddagrabFilter(const DdagrabSource& source, int fps, bool includeCursor)
{
    if (!source.valid)
        return {};
    return QStringLiteral(
               "ddagrab=output_idx=%1:draw_mouse=%2:framerate=%3:video_size=%4x%5"
               ":offset_x=%6:offset_y=%7")
        .arg(source.outputIndex)
        .arg(includeCursor ? 1 : 0)
        .arg(std::max(1, fps))
        .arg(source.size.width())
        .arg(source.size.height())
        .arg(source.offset.x())
        .arg(source.offset.y());
}

} // namespace Platform
