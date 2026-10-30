#include "platform/windows/ocr.h"
#include "platform/windows/ocr_settings.h"
#include "platform/windows/win_util.h"

#include <winrt/Windows.Foundation.Collections.h>
#include <winrt/Windows.Globalization.h>
#include <winrt/Windows.Media.Ocr.h>
#include <winrt/base.h>

#include <QGuiApplication>

#include <cstdio>

int main(int argc, char** argv)
{
    QGuiApplication app(argc, argv);
    const QStringList installed{QStringLiteral("en-GB"), QStringLiteral("es-MX"),
                                QStringLiteral("zh-Hant-TW"), QStringLiteral("zh-Hans-CN")};
    if (Platform::ocrRecognizerTag(QStringLiteral("eng"), installed) != QLatin1String("en-GB")
        || Platform::ocrRecognizerTag(QStringLiteral("spa"), installed) != QLatin1String("es-MX")
        || Platform::ocrRecognizerTag(QStringLiteral("chi_sim"), installed) != QLatin1String("zh-Hans-CN")
        || !Platform::ocrRecognizerTag(QStringLiteral("deu"), installed).isEmpty()
        || !Platform::ocrRecognizerTag(QStringLiteral("chi_sim"),
                                       {QStringLiteral("zh-Hant-TW")}).isEmpty()
        || Platform::formatOcrLines({QStringLiteral("one"), QStringLiteral("two")}, true)
            != QLatin1String("one\ntwo")
        || Platform::formatOcrLines({QStringLiteral("one"), QStringLiteral("two")}, false)
            != QLatin1String("one two")) {
        std::fprintf(stderr, "OCR language selection or line formatting failed\n");
        return 1;
    }
    const QStringList allLanguages{QStringLiteral("en-US"), QStringLiteral("es-ES"),
                                   QStringLiteral("fr-FR"), QStringLiteral("de-DE"),
                                   QStringLiteral("it-IT"), QStringLiteral("pt-BR"),
                                   QStringLiteral("zh-Hans"), QStringLiteral("ja-JP"),
                                   QStringLiteral("ru-RU")};
    for (const QString& code : {QStringLiteral("eng"), QStringLiteral("spa"), QStringLiteral("fra"),
                                QStringLiteral("deu"), QStringLiteral("ita"), QStringLiteral("por"),
                                QStringLiteral("chi_sim"), QStringLiteral("jpn"), QStringLiteral("rus")}) {
        if (Platform::ocrRecognizerTag(code, allLanguages).isEmpty()) {
            std::fprintf(stderr, "No recognizer mapping for %s\n", qPrintable(code));
            return 1;
        }
    }

    QStringList available;
    try {
        winrt::init_apartment(winrt::apartment_type::multi_threaded);
        for (const auto& language : winrt::Windows::Media::Ocr::OcrEngine::AvailableRecognizerLanguages())
            available.append(QString::fromWCharArray(language.LanguageTag().c_str()));
    } catch (const winrt::hresult_error&) {
        std::printf("Windows OCR is unavailable; synthetic recognition skipped\n");
        return 0;
    }
    if (Platform::ocrRecognizerTag(QStringLiteral("eng"), available).isEmpty()) {
        std::printf("English OCR language pack is not installed; synthetic recognition skipped\n");
        return 0;
    }

    HDC dc = CreateCompatibleDC(nullptr);
    BITMAPINFO bitmapInfo{};
    bitmapInfo.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    bitmapInfo.bmiHeader.biWidth = 1000;
    bitmapInfo.bmiHeader.biHeight = -240;
    bitmapInfo.bmiHeader.biPlanes = 1;
    bitmapInfo.bmiHeader.biBitCount = 32;
    bitmapInfo.bmiHeader.biCompression = BI_RGB;
    void* pixels = nullptr;
    HBITMAP bitmap = dc ? CreateDIBSection(dc, &bitmapInfo, DIB_RGB_COLORS, &pixels, nullptr, 0) : nullptr;
    HFONT font = CreateFontW(-58, 0, 0, 0, FW_NORMAL, FALSE, FALSE, FALSE, DEFAULT_CHARSET,
                             OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, ANTIALIASED_QUALITY,
                             DEFAULT_PITCH, L"Segoe UI");
    if (!dc || !bitmap || !pixels || !font) {
        std::fprintf(stderr, "Could not create synthetic OCR text\n");
        return 1;
    }
    HGDIOBJ oldBitmap = SelectObject(dc, bitmap);
    HGDIOBJ oldFont = SelectObject(dc, font);
    RECT bounds{0, 0, 1000, 240};
    FillRect(dc, &bounds, static_cast<HBRUSH>(GetStockObject(WHITE_BRUSH)));
    SetBkMode(dc, TRANSPARENT);
    SetTextColor(dc, RGB(0, 0, 0));
    TextOutW(dc, 40, 20, L"HELLO WORLD", 11);
    TextOutW(dc, 40, 110, L"SECOND LINE", 11);
    GdiFlush();
    QImage image(static_cast<const uchar*>(pixels), 1000, 240, QImage::Format_RGB32);
    image = image.copy();
    SelectObject(dc, oldFont);
    SelectObject(dc, oldBitmap);
    DeleteObject(font);
    DeleteObject(bitmap);
    DeleteDC(dc);
    int darkPixels = 0;
    for (int y = 0; y < image.height(); ++y) {
        for (int x = 0; x < image.width(); ++x) {
            if (qGray(image.pixel(x, y)) < 128)
                ++darkPixels;
        }
    }
    if (darkPixels == 0) {
        std::fprintf(stderr, "Synthetic text could not be rendered\n");
        return 1;
    }

    QString preserved;
    QString flattened;
    QString error;
    if (Platform::recognizeText(image, QStringLiteral("missing"), true, preserved, error)
        || !error.contains(QStringLiteral("language pack"))) {
        std::fprintf(stderr, "Missing OCR language was not reported\n");
        return 1;
    }
    if (!Platform::recognizeText(image, QStringLiteral("eng"), true, preserved, error)
        || !preserved.contains(QStringLiteral("HELLO"), Qt::CaseInsensitive)
        || !preserved.contains(QLatin1Char('\n'))) {
        std::fprintf(stderr, "Synthetic OCR failed: %s (recognized: %s)\n",
                     qPrintable(error), qPrintable(preserved));
        return 1;
    }
    if (!Platform::recognizeText(image, QStringLiteral("eng"), false, flattened, error)
        || flattened.contains(QLatin1Char('\n'))
        || !flattened.contains(QStringLiteral("SECOND"), Qt::CaseInsensitive)) {
        std::fprintf(stderr, "Synthetic OCR without line breaks failed: %s\n", qPrintable(error));
        return 1;
    }
    return 0;
}
