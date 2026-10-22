#include "platform/windows/ocr.h"

#include "platform/windows/settings_store.h"

#include <winrt/Windows.Foundation.Collections.h>
#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.Globalization.h>
#include <winrt/Windows.Graphics.Imaging.h>
#include <winrt/Windows.Media.Ocr.h>
#include <winrt/Windows.Storage.Streams.h>
#include <winrt/base.h>

#include <QHash>

#include <cstring>
#include <string>

namespace {

/// The Advanced page stores tesseract codes for Linux compatibility. Windows
/// OCR wants BCP-47 tags, so translate the shipped list and pass through tags
/// that already look like one.
QString ocrLanguageTag(const QString& code)
{
    const QString trimmed = code.trimmed();
    if (trimmed.isEmpty())
        return QString();

    static const QHash<QString, QString> tags = {
        {QStringLiteral("eng"), QStringLiteral("en-US")},
        {QStringLiteral("spa"), QStringLiteral("es-ES")},
        {QStringLiteral("fra"), QStringLiteral("fr-FR")},
        {QStringLiteral("deu"), QStringLiteral("de-DE")},
        {QStringLiteral("ita"), QStringLiteral("it-IT")},
        {QStringLiteral("por"), QStringLiteral("pt-BR")},
        {QStringLiteral("chi_sim"), QStringLiteral("zh-Hans")},
        {QStringLiteral("jpn"), QStringLiteral("ja-JP")},
        {QStringLiteral("rus"), QStringLiteral("ru-RU")},
    };

    if (trimmed.contains(QLatin1Char('-')))
        return trimmed;
    return tags.value(trimmed);
}

/// The engine needs a COM apartment. Qt does not create one, and a second
/// attempt from an already-initialised thread fails harmlessly.
void ensureApartment()
{
    static const bool ready = [] {
        try {
            winrt::init_apartment(winrt::apartment_type::multi_threaded);
        } catch (...) {
            // RPC_E_CHANGED_MODE: the thread already has a different apartment.
        }
        return true;
    }();
    Q_UNUSED(ready);
}

} // namespace

namespace Platform {

bool recognizeText(const QImage& image, QString& outText, QString& outError)
{
    outText.clear();
    outError.clear();

    if (image.isNull()) {
        outError = QStringLiteral("empty image");
        return false;
    }

    ensureApartment();

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

        const Platform::ShellSettings settings = Platform::loadShellSettings();

        OcrEngine engine = nullptr;
        const QString tag = ocrLanguageTag(settings.ocr.language);
        if (!tag.isEmpty()) {
            try {
                engine = OcrEngine::TryCreateFromLanguage(
                    winrt::Windows::Globalization::Language(winrt::hstring(tag.toStdWString())));
            } catch (const winrt::hresult_error&) {
                // A missing language pack falls back to the profile list below.
                engine = nullptr;
            }
        }
        if (!engine)
            engine = OcrEngine::TryCreateFromUserProfileLanguages();
        if (!engine) {
            outError = QStringLiteral("no OCR language pack installed");
            return false;
        }

        const auto result = engine.RecognizeAsync(bitmap).get();

        QStringList lines;
        for (const auto& line : result.Lines())
            lines.append(QString::fromWCharArray(line.Text().c_str()));

        outText = lines.join(settings.ocr.keepLineBreaks ? QChar('\n') : QChar(' '));
        return true;
    } catch (const winrt::hresult_error& error) {
        outError = QString::fromWCharArray(error.message().c_str());
        return false;
    } catch (...) {
        outError = QStringLiteral("OCR failed");
        return false;
    }
}

} // namespace Platform
