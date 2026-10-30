#include "platform/windows/ocr.h"

#include "platform/windows/ocr_settings.h"

#include <winrt/Windows.Foundation.Collections.h>
#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.Globalization.h>
#include <winrt/Windows.Graphics.Imaging.h>
#include <winrt/Windows.Media.Ocr.h>
#include <winrt/Windows.Storage.Streams.h>
#include <winrt/base.h>

#include <cstring>
#include <future>
#include <string>

namespace {

class OcrApartment
{
public:
    OcrApartment()
    {
        try {
            winrt::init_apartment(winrt::apartment_type::multi_threaded);
            m_owned = true;
        } catch (...) {
            // The thread may already have an apartment of another type.
        }
    }
    ~OcrApartment() { if (m_owned) winrt::uninit_apartment(); }

private:
    bool m_owned = false;
};

} // namespace

namespace Platform {

static bool recognizeTextOnWorker(const QImage& image, const QString& language,
                                  bool keepLineBreaks, QString& outText, QString& outError)
{
    outText.clear();
    outError.clear();

    if (image.isNull()) {
        outError = QStringLiteral("empty image");
        return false;
    }

    OcrApartment apartment;

    try {
        using winrt::Windows::Graphics::Imaging::BitmapPixelFormat;
        using winrt::Windows::Graphics::Imaging::SoftwareBitmap;
        using winrt::Windows::Media::Ocr::OcrEngine;
        using winrt::Windows::Storage::Streams::Buffer;

        const QImage bgra = image.convertToFormat(QImage::Format_ARGB32);
        const int width = bgra.width();
        const int height = bgra.height();
        const uint32_t stride = static_cast<uint32_t>(width) * 4u;
        const uint32_t size = stride * static_cast<uint32_t>(height);

        Buffer buffer(size);
        buffer.Length(size);
        uint8_t* destination = buffer.data();
        for (int y = 0; y < height; ++y) {
            std::memcpy(destination + static_cast<size_t>(y) * stride,
                        bgra.constScanLine(y),
                        stride);
        }

        const SoftwareBitmap bitmap =
            SoftwareBitmap::CreateCopyFromBuffer(buffer, BitmapPixelFormat::Bgra8, width, height);

        QStringList available;
        for (const auto& item : OcrEngine::AvailableRecognizerLanguages())
            available.append(QString::fromWCharArray(item.LanguageTag().c_str()));
        const QString tag = ocrRecognizerTag(language, available);
        OcrEngine engine = tag.isEmpty() ? nullptr : OcrEngine::TryCreateFromLanguage(
            winrt::Windows::Globalization::Language(winrt::hstring(tag.toStdWString())));
        if (!engine) {
            outError = QStringLiteral("Install the Windows OCR language pack for %1 in Windows Settings.")
                           .arg(language);
            return false;
        }

        const auto result = engine.RecognizeAsync(bitmap).get();

        QStringList lines;
        for (const auto& line : result.Lines())
            lines.append(QString::fromWCharArray(line.Text().c_str()));

        outText = formatOcrLines(lines, keepLineBreaks);
        return true;
    } catch (const winrt::hresult_error& error) {
        outError = QString::fromWCharArray(error.message().c_str());
        return false;
    } catch (...) {
        outError = QStringLiteral("OCR failed");
        return false;
    }
}

bool recognizeText(const QImage& image, const QString& language, bool keepLineBreaks,
                   QString& outText, QString& outError)
{
    try {
        // WinRT's synchronous .get() cannot run on Qt's STA GUI thread.
        return std::async(std::launch::async, [&] {
            return recognizeTextOnWorker(image, language, keepLineBreaks, outText, outError);
        }).get();
    } catch (const std::exception&) {
        outText.clear();
        outError = QStringLiteral("Could not start OCR");
        return false;
    }
}

} // namespace Platform
