#include "platform/windows/hdr_capture.h"

#include "platform/windows/win_util.h"

#include <d3d11.h>
#include <dxgi1_5.h>
#include <wrl/client.h>

#include <algorithm>
#include <cmath>
#include <cstring>
#include <vector>

using Microsoft::WRL::ComPtr;

namespace {

// Highlights above SDR white are compressed into the top of the range,
// starting here, with a curve that meets the identity line smoothly.
constexpr float kKnee = 0.9f;
constexpr int kEncodeSize = 16384;
constexpr UINT kAcquireTimeoutMs = 250;
constexpr int kAcquireAttempts = 4;

float halfToFloat(std::uint16_t half)
{
    const std::uint32_t sign = (half & 0x8000u) << 16;
    const std::uint32_t exponent = (half >> 10) & 0x1fu;
    const std::uint32_t mantissa = half & 0x3ffu;
    if (exponent == 31)
        return 0.0f; // infinity or NaN: nothing sensible to show
    if (exponent == 0) {
        const float value = std::ldexp(static_cast<float>(mantissa), -24);
        return sign ? -value : value;
    }
    const std::uint32_t bits = sign | ((exponent + 112u) << 23) | (mantissa << 13);
    float value = 0.0f;
    std::memcpy(&value, &bits, sizeof(value));
    return value;
}

float srgbEncode(float linear)
{
    if (linear <= 0.0031308f)
        return 12.92f * linear;
    return 1.055f * std::pow(linear, 1.0f / 2.4f) - 0.055f;
}

QString hresultText(const char* what, HRESULT hr)
{
    return QStringLiteral("%1 failed (0x%2)")
        .arg(QString::fromLatin1(what))
        .arg(static_cast<quint32>(hr), 8, 16, QLatin1Char('0'));
}

} // namespace

namespace Platform::Win {

float hdrSdrWhiteNits(const QString& gdiDeviceName)
{
    UINT32 pathCount = 0;
    UINT32 modeCount = 0;
    if (GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &pathCount, &modeCount) != ERROR_SUCCESS)
        return 0.0f;
    std::vector<DISPLAYCONFIG_PATH_INFO> paths(pathCount);
    std::vector<DISPLAYCONFIG_MODE_INFO> modes(modeCount);
    if (QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &pathCount, paths.data(), &modeCount,
                           modes.data(), nullptr)
        != ERROR_SUCCESS)
        return 0.0f;

    for (UINT32 i = 0; i < pathCount; ++i) {
        const DISPLAYCONFIG_PATH_INFO& path = paths[i];
        DISPLAYCONFIG_SOURCE_DEVICE_NAME source{};
        source.header.type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
        source.header.size = sizeof(source);
        source.header.adapterId = path.sourceInfo.adapterId;
        source.header.id = path.sourceInfo.id;
        if (DisplayConfigGetDeviceInfo(&source.header) != ERROR_SUCCESS)
            continue;
        if (QString::fromWCharArray(source.viewGdiDeviceName)
                .compare(gdiDeviceName, Qt::CaseInsensitive)
            != 0)
            continue;

        DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO color{};
        color.header.type = DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO;
        color.header.size = sizeof(color);
        color.header.adapterId = path.targetInfo.adapterId;
        color.header.id = path.targetInfo.id;
        if (DisplayConfigGetDeviceInfo(&color.header) != ERROR_SUCCESS)
            return 0.0f;
        // Wide colour enforced is automatic colour management on an SDR
        // display, which GDI already captures correctly.
        if (!color.advancedColorEnabled || color.wideColorEnforced)
            return 0.0f;

        DISPLAYCONFIG_SDR_WHITE_LEVEL white{};
        white.header.type = DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL;
        white.header.size = sizeof(white);
        white.header.adapterId = path.targetInfo.adapterId;
        white.header.id = path.targetInfo.id;
        if (DisplayConfigGetDeviceInfo(&white.header) != ERROR_SUCCESS || white.SDRWhiteLevel == 0)
            return 80.0f;
        // SDRWhiteLevel is a multiplier of 80 nits, scaled by 1000.
        return static_cast<float>(white.SDRWhiteLevel) / 1000.0f * 80.0f;
    }
    return 0.0f;
}

QImage scRgbToSdr(const std::uint16_t* pixels,
                  int width,
                  int height,
                  int strideBytes,
                  float sdrWhiteNits)
{
    if (!pixels || width <= 0 || height <= 0 || sdrWhiteNits <= 0.0f)
        return {};

    static const std::vector<float> halves = [] {
        std::vector<float> table(65536);
        for (std::uint32_t i = 0; i < 65536; ++i)
            table[i] = halfToFloat(static_cast<std::uint16_t>(i));
        return table;
    }();
    static const std::vector<uchar> encode = [] {
        std::vector<uchar> table(kEncodeSize);
        for (int i = 0; i < kEncodeSize; ++i) {
            const float value = srgbEncode(static_cast<float>(i) / (kEncodeSize - 1));
            table[i] = static_cast<uchar>(std::clamp(std::lround(value * 255.0f), 0L, 255L));
        }
        return table;
    }();

    const float scale = 80.0f / sdrWhiteNits;
    const auto row = [&](int y) {
        return reinterpret_cast<const std::uint16_t*>(reinterpret_cast<const uchar*>(pixels)
                                                      + static_cast<size_t>(y) * strideBytes);
    };

    float peak = 0.0f;
    for (int y = 0; y < height; ++y) {
        const std::uint16_t* source = row(y);
        for (int x = 0; x < width; ++x) {
            const std::uint16_t* p = source + x * 4;
            peak = std::max(peak, std::max({halves[p[0]], halves[p[1]], halves[p[2]]}) * scale);
        }
    }
    const bool rollOff = peak > 1.0f;

    const auto index = [](float value) {
        return static_cast<int>(std::min(value, 1.0f) * (kEncodeSize - 1) + 0.5f);
    };

    QImage out(width, height, QImage::Format_RGB32);
    for (int y = 0; y < height; ++y) {
        const std::uint16_t* source = row(y);
        auto* target = reinterpret_cast<QRgb*>(out.scanLine(y));
        for (int x = 0; x < width; ++x) {
            const std::uint16_t* p = source + x * 4;
            float r = std::max(0.0f, halves[p[0]] * scale);
            float g = std::max(0.0f, halves[p[1]] * scale);
            float b = std::max(0.0f, halves[p[2]] * scale);
            if (rollOff) {
                const float m = std::max({r, g, b});
                if (m > kKnee) {
                    const float span = 1.0f - kKnee;
                    const float mapped = kKnee + span * (1.0f - std::exp(-(m - kKnee) / span));
                    const float factor = mapped / m;
                    r *= factor;
                    g *= factor;
                    b *= factor;
                }
            }
            target[x] = qRgb(encode[index(r)], encode[index(g)], encode[index(b)]);
        }
    }
    return out;
}

bool captureHdrDisplay(const QString& gdiDeviceName,
                       const QSize& expectedSize,
                       QImage& out,
                       QString& outError)
{
    const float white = hdrSdrWhiteNits(gdiDeviceName);
    if (white <= 0.0f) {
        outError = QStringLiteral("display is not in HDR mode");
        return false;
    }

    ComPtr<IDXGIFactory1> factory;
    HRESULT hr = CreateDXGIFactory1(IID_PPV_ARGS(&factory));
    if (FAILED(hr)) {
        outError = hresultText("CreateDXGIFactory1", hr);
        return false;
    }

    ComPtr<IDXGIAdapter1> adapter;
    ComPtr<IDXGIOutput> output;
    for (UINT a = 0; !output && factory->EnumAdapters1(a, &adapter) != DXGI_ERROR_NOT_FOUND; ++a) {
        ComPtr<IDXGIOutput> candidate;
        for (UINT o = 0; adapter->EnumOutputs(o, &candidate) != DXGI_ERROR_NOT_FOUND; ++o) {
            DXGI_OUTPUT_DESC desc{};
            if (SUCCEEDED(candidate->GetDesc(&desc))
                && QString::fromWCharArray(desc.DeviceName)
                           .compare(gdiDeviceName, Qt::CaseInsensitive)
                    == 0) {
                output = candidate;
                break;
            }
        }
        if (!output)
            adapter.Reset();
    }
    if (!output || !adapter) {
        outError = QStringLiteral("no DXGI output for %1").arg(gdiDeviceName);
        return false;
    }

    ComPtr<IDXGIOutput5> output5;
    hr = output.As(&output5);
    if (FAILED(hr)) {
        outError = hresultText("IDXGIOutput5", hr);
        return false;
    }

    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11DeviceContext> context;
    hr = D3D11CreateDevice(adapter.Get(), D3D_DRIVER_TYPE_UNKNOWN, nullptr,
                           D3D11_CREATE_DEVICE_BGRA_SUPPORT, nullptr, 0, D3D11_SDK_VERSION,
                           &device, nullptr, &context);
    if (FAILED(hr)) {
        outError = hresultText("D3D11CreateDevice", hr);
        return false;
    }

    const DXGI_FORMAT formats[] = {DXGI_FORMAT_R16G16B16A16_FLOAT};
    ComPtr<IDXGIOutputDuplication> duplication;
    hr = output5->DuplicateOutput1(device.Get(), 0, 1, formats, &duplication);
    if (FAILED(hr)) {
        outError = hresultText("DuplicateOutput1", hr);
        return false;
    }

    DXGI_OUTDUPL_DESC duplicationDesc{};
    duplication->GetDesc(&duplicationDesc);
    if (duplicationDesc.Rotation != DXGI_MODE_ROTATION_IDENTITY
        && duplicationDesc.Rotation != DXGI_MODE_ROTATION_UNSPECIFIED) {
        outError = QStringLiteral("rotated display");
        return false;
    }

    // A fresh duplication delivers the current desktop on its first frame.
    ComPtr<IDXGIResource> resource;
    DXGI_OUTDUPL_FRAME_INFO frameInfo{};
    hr = DXGI_ERROR_WAIT_TIMEOUT;
    for (int attempt = 0; attempt < kAcquireAttempts && hr == DXGI_ERROR_WAIT_TIMEOUT; ++attempt)
        hr = duplication->AcquireNextFrame(kAcquireTimeoutMs, &frameInfo, &resource);
    if (FAILED(hr)) {
        outError = hresultText("AcquireNextFrame", hr);
        return false;
    }

    bool ok = false;
    ComPtr<ID3D11Texture2D> frame;
    if (SUCCEEDED(resource.As(&frame))) {
        D3D11_TEXTURE2D_DESC desc{};
        frame->GetDesc(&desc);
        desc.Usage = D3D11_USAGE_STAGING;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
        desc.BindFlags = 0;
        desc.MiscFlags = 0;
        desc.MipLevels = 1;
        desc.ArraySize = 1;
        ComPtr<ID3D11Texture2D> staging;
        hr = device->CreateTexture2D(&desc, nullptr, &staging);
        if (SUCCEEDED(hr)) {
            context->CopyResource(staging.Get(), frame.Get());
            D3D11_MAPPED_SUBRESOURCE mapped{};
            hr = context->Map(staging.Get(), 0, D3D11_MAP_READ, 0, &mapped);
            if (SUCCEEDED(hr)) {
                if (desc.Format != DXGI_FORMAT_R16G16B16A16_FLOAT) {
                    outError = QStringLiteral("duplication did not return FP16 pixels");
                } else if (QSize(static_cast<int>(desc.Width), static_cast<int>(desc.Height))
                           != expectedSize) {
                    outError = QStringLiteral("duplicated frame size does not match the display");
                } else {
                    out = scRgbToSdr(static_cast<const std::uint16_t*>(mapped.pData),
                                     static_cast<int>(desc.Width),
                                     static_cast<int>(desc.Height),
                                     static_cast<int>(mapped.RowPitch),
                                     white);
                    ok = !out.isNull();
                    if (!ok)
                        outError = QStringLiteral("HDR conversion failed");
                }
                context->Unmap(staging.Get(), 0);
            } else {
                outError = hresultText("Map", hr);
            }
        } else {
            outError = hresultText("CreateTexture2D", hr);
        }
    } else {
        outError = QStringLiteral("duplicated frame is not a texture");
    }
    duplication->ReleaseFrame();
    return ok;
}

} // namespace Platform::Win
