#pragma once

#include <QByteArray>
#include <QImage>
#include <QPixmap>
#include <QPointer>
#include <QPoint>
#include <QRect>
#include <QSize>
#include <QString>

#include <memory>

class QProcess;
class QScreen;

namespace ScreenCapture {

enum class CaptureStatus { Ok, Cancelled, Failed };

struct HelperMessage {
    enum class Kind { Invalid, Ready, Error, Frame };

    Kind kind = Kind::Invalid;
    bool hasPosition = false;
    QPoint position;
    bool hasSize = false;
    QSize size;
    QString path;
    QString error;
    bool cancelled = false;
};

HelperMessage parseHelperMessage(const QByteArray& line);
bool readyMetadataMatches(const HelperMessage& ready, const QRect& captureRect);
QRect mapLogicalRectToImage(const QRect& logicalRect,
                            const QRect& captureRect,
                            const QSize& imageSize);

class CaptureSession
{
public:
    CaptureSession();
    ~CaptureSession();
    CaptureSession(const CaptureSession&) = delete;
    CaptureSession& operator=(const CaptureSession&) = delete;

    CaptureStatus prepare(QScreen* screen, bool includeCursor, QString& outError);
    CaptureStatus captureFresh(QImage& outImage, QString& outError);
    bool isPrepared() const;
    QRect captureRect() const;

private:
    void close();

    QPointer<QScreen> m_screen;
    QRect m_captureRect;
    bool m_prepared = false;
    std::unique_ptr<QProcess> m_helper;
};

bool saveImageToTempPng(const QImage& image,
                        QString& outPath,
                        QSize& outSize,
                        QString& outError);
bool cropLogicalSelectionToTempPng(const QImage& captureImage,
                                   const QRect& captureRect,
                                   const QRect& logicalSelection,
                                   QString& outPath,
                                   QSize& outSize,
                                   QString& outError);
QPixmap freezeBackgroundForLogicalRect(const QImage& captureImage,
                                       const QRect& captureRect,
                                       const QRect& logicalRect);

}
