#include "CaptureMenuResult.h"
#include "ScreenCapture.h"

#include <QFile>
#include <QGuiApplication>
#include <QImage>
#include <QPainter>
#include <QScreen>
#include <QTemporaryDir>
#include <QtTest/QtTest>

class ScreenCaptureTests : public QObject
{
    Q_OBJECT

private Q_SLOTS:
    void parsesReadyWithStreamMetadata();
    void parsesReadyWithMissingMetadata();
    void parsesCancelledAndFailedErrors();
    void parsesFrameReply();
    void rejectsMalformedReplies();
    void acceptsNegativeOriginMatchingPosition();
    void rejectsStreamOnAnotherMonitor();
    void acceptsMixedDpiPhysicalSizeWithSameAspect();
    void rejectsMismatchedAspectRatio();
    void acceptsMissingMetadata();
    void mapsNegativeOriginUnderTwoXScale();
    void mapsFractionalScaleWithEdgeRounding();
    void clipsSelectionToCaptureRect();
    void rejectsSelectionOutsideCaptureRect();
    void freezeUsesChosenMonitorOrigin();
    void freezePreservesNativePixels_data();
    void freezePreservesNativePixels();
    void freezeMapsOddAndClippedRectsAtFractionalScale();
    void freezeRejectsInvalidAndOutsideRects();
    void freezeKeepsEachMonitorFrameIndependent();
    void paintFreezeBackgroundExactRender_data();
    void paintFreezeBackgroundExactRender();
    void paintFreezeBackgroundClippedSelection();
    void paintFreezeBackgroundHonorsExistingClipAndRestoresState();
    void cropsMixedDpiFrameToTempPng();
    void cropsEachSyntheticMonitorFromVirtualDesktop();
    void tempPngUsesRealCompression();
    void portalRoutePreparesAndCapturesThroughHelper();
    void portalRouteMapsHelperCancellation();
    void portalRouteMapsHelperFailure();
    void portalRouteRejectsMismatchedStream();
    void portalRouteFailsWithoutHelper();
    void portalWindowRoutePreparesAndCapturesThroughHelper();
    void portalWindowRouteMapsHelperCancellation();
    void portalWindowRouteMapsHelperFailure();
    void waylandSessionUsesPortalWithX11QtPlatform();
    void nativeX11DoesNotRequestPortal();
    void parsesLayerShellCaptureMenuResult();
    void rejectsMalformedLayerShellCaptureMenuResult();
    void rejectsUnsupportedLayerShellCaptureMenuState();
};

namespace {

using ScreenCapture::HelperMessage;

const char* kFakeHelperScript = R"SH(#!/bin/sh
printf '%s\n' "$@" > "$FAKE_HELPER_ARGS_PATH"
case "$FAKE_HELPER_MODE" in
  cancel) printf '%s\n' '{"error":"user dismissed","cancelled":true}'; exit 0 ;;
  fail) printf '%s\n' '{"error":"portal broke","cancelled":false}'; exit 0 ;;
  mismatch) printf '%s\n' '{"ready":true,"position":[9999,9999],"size":null}' ;;
  *) printf '%s\n' '{"ready":true,"position":null,"size":null}' ;;
esac
while IFS= read -r line; do
  if [ "$line" = "capture" ]; then
    printf '{"path":"%s","width":8,"height":6}\n' "$FAKE_FRAME_PATH"
  fi
done
exit 0
)SH";

struct FakePortalHelper
{
    QTemporaryDir dir;
    QString helperPath;
    QString framePath;
    QString argsPath;

    bool install()
    {
        if (!dir.isValid()) {
            return false;
        }
        helperPath = dir.filePath(QStringLiteral("apexshot-fake-helper"));
        framePath = dir.filePath(QStringLiteral("frame.png"));
        argsPath = dir.filePath(QStringLiteral("helper-args.txt"));
        QFile script(helperPath);
        if (!script.open(QIODevice::WriteOnly | QIODevice::Truncate)) {
            return false;
        }
        script.write(kFakeHelperScript);
        script.close();
        QFile::setPermissions(helperPath,
                              QFile::ReadOwner | QFile::WriteOwner | QFile::ExeOwner
                                | QFile::ReadGroup | QFile::ExeGroup
                                | QFile::ReadOther | QFile::ExeOther);
        qputenv("APEXSHOT_PORTAL_HELPER", helperPath.toUtf8());
        qputenv("FAKE_FRAME_PATH", framePath.toUtf8());
        qputenv("FAKE_HELPER_ARGS_PATH", argsPath.toUtf8());
        qputenv("FLATPAK_ID", QByteArrayLiteral("org.apexshot.ApexShot"));
        return true;
    }

    void writeFrame(int width, int height)
    {
        QImage frame(width, height, QImage::Format_ARGB32);
        frame.fill(Qt::green);
        frame.save(framePath, "PNG");
    }

    ~FakePortalHelper()
    {
        qunsetenv("APEXSHOT_PORTAL_HELPER");
        qunsetenv("FAKE_FRAME_PATH");
        qunsetenv("FAKE_HELPER_ARGS_PATH");
        qunsetenv("FAKE_HELPER_MODE");
        qunsetenv("FLATPAK_ID");
    }
};

QImage twoByOneBlueRightHalfImage(int width, int height)
{
    QImage image(width, height, QImage::Format_ARGB32);
    image.fill(Qt::red);
    for (int y = 0; y < height; ++y) {
        for (int x = width / 2; x < width; ++x) {
            image.setPixelColor(x, y, Qt::blue);
        }
    }
    return image;
}

QImage highFrequencyFixture(int width, int height)
{
    QImage image(width, height, QImage::Format_ARGB32);
    for (int y = 0; y < height; ++y) {
        for (int x = 0; x < width; ++x) {
            image.setPixelColor(x, y,
                                QColor((x * 37 + y * 11) & 0xff,
                                       (x * 17 + y * 29) & 0xff,
                                       (x * 13 + y * 7) & 0xff));
        }
    }
    return image;
}

}

void ScreenCaptureTests::parsesReadyWithStreamMetadata()
{
    const HelperMessage message = ScreenCapture::parseHelperMessage(
      QByteArrayLiteral("{\"ready\":true,\"position\":[-1920,0],\"size\":[3840,2160]}\n"));
    QCOMPARE(static_cast<int>(message.kind), static_cast<int>(HelperMessage::Kind::Ready));
    QVERIFY(message.hasPosition);
    QCOMPARE(message.position, QPoint(-1920, 0));
    QVERIFY(message.hasSize);
    QCOMPARE(message.size, QSize(3840, 2160));
}

void ScreenCaptureTests::parsesReadyWithMissingMetadata()
{
    const HelperMessage message = ScreenCapture::parseHelperMessage(
      QByteArrayLiteral("{\"ready\":true,\"position\":null,\"size\":null}"));
    QCOMPARE(static_cast<int>(message.kind), static_cast<int>(HelperMessage::Kind::Ready));
    QVERIFY(!message.hasPosition);
    QVERIFY(!message.hasSize);
}

void ScreenCaptureTests::parsesCancelledAndFailedErrors()
{
    const HelperMessage cancelled = ScreenCapture::parseHelperMessage(
      QByteArrayLiteral("{\"error\":\"user dismissed\",\"cancelled\":true}"));
    QCOMPARE(static_cast<int>(cancelled.kind), static_cast<int>(HelperMessage::Kind::Error));
    QVERIFY(cancelled.cancelled);
    QCOMPARE(cancelled.error, QStringLiteral("user dismissed"));

    const HelperMessage failed = ScreenCapture::parseHelperMessage(
      QByteArrayLiteral("{\"error\":\"portal unavailable\",\"cancelled\":false}"));
    QCOMPARE(static_cast<int>(failed.kind), static_cast<int>(HelperMessage::Kind::Error));
    QVERIFY(!failed.cancelled);
}

void ScreenCaptureTests::parsesFrameReply()
{
    const HelperMessage message = ScreenCapture::parseHelperMessage(
      QByteArrayLiteral("{\"path\":\"/tmp/frame.png\",\"width\":3840,\"height\":2160}\n"));
    QCOMPARE(static_cast<int>(message.kind), static_cast<int>(HelperMessage::Kind::Frame));
    QCOMPARE(message.path, QStringLiteral("/tmp/frame.png"));
}

void ScreenCaptureTests::rejectsMalformedReplies()
{
    QCOMPARE(static_cast<int>(ScreenCapture::parseHelperMessage(QByteArrayLiteral("not json")).kind),
             static_cast<int>(HelperMessage::Kind::Invalid));
    QCOMPARE(static_cast<int>(ScreenCapture::parseHelperMessage(QByteArrayLiteral("[1,2]")).kind),
             static_cast<int>(HelperMessage::Kind::Invalid));
    QCOMPARE(static_cast<int>(ScreenCapture::parseHelperMessage(QByteArrayLiteral("{\"other\":1}")).kind),
             static_cast<int>(HelperMessage::Kind::Invalid));
}

void ScreenCaptureTests::acceptsNegativeOriginMatchingPosition()
{
    const HelperMessage ready = ScreenCapture::parseHelperMessage(
      QByteArrayLiteral("{\"ready\":true,\"position\":[-1920,0],\"size\":[1920,1080]}"));
    QVERIFY(ScreenCapture::readyMetadataMatches(ready, QRect(-1920, 0, 1920, 1080)));
}

void ScreenCaptureTests::rejectsStreamOnAnotherMonitor()
{
    const HelperMessage ready = ScreenCapture::parseHelperMessage(
      QByteArrayLiteral("{\"ready\":true,\"position\":[0,0],\"size\":[1920,1080]}"));
    QVERIFY(!ScreenCapture::readyMetadataMatches(ready, QRect(-1920, 0, 1920, 1080)));
}

void ScreenCaptureTests::acceptsMixedDpiPhysicalSizeWithSameAspect()
{
    const HelperMessage ready = ScreenCapture::parseHelperMessage(
      QByteArrayLiteral("{\"ready\":true,\"position\":[0,0],\"size\":[3840,2160]}"));
    QVERIFY(ScreenCapture::readyMetadataMatches(ready, QRect(0, 0, 1920, 1080)));
}

void ScreenCaptureTests::rejectsMismatchedAspectRatio()
{
    const HelperMessage ready = ScreenCapture::parseHelperMessage(
      QByteArrayLiteral("{\"ready\":true,\"position\":null,\"size\":[1280,1024]}"));
    QVERIFY(!ScreenCapture::readyMetadataMatches(ready, QRect(0, 0, 1920, 1080)));
}

void ScreenCaptureTests::acceptsMissingMetadata()
{
    const HelperMessage ready = ScreenCapture::parseHelperMessage(
      QByteArrayLiteral("{\"ready\":true,\"position\":null,\"size\":null}"));
    QVERIFY(ScreenCapture::readyMetadataMatches(ready, QRect(-1920, 0, 1920, 1080)));
    QVERIFY(!ScreenCapture::readyMetadataMatches(ready, QRect()));
}

void ScreenCaptureTests::mapsNegativeOriginUnderTwoXScale()
{
    const QRect mapped = ScreenCapture::mapLogicalRectToImage(
      QRect(-1820, 50, 200, 100), QRect(-1920, 0, 1920, 1080), QSize(3840, 2160));
    QCOMPARE(mapped, QRect(200, 100, 400, 200));
}

void ScreenCaptureTests::mapsFractionalScaleWithEdgeRounding()
{
    const QRect mapped = ScreenCapture::mapLogicalRectToImage(
      QRect(10, 10, 1, 1), QRect(0, 0, 1536, 864), QSize(1920, 1080));
    QCOMPARE(mapped, QRect(13, 13, 1, 1));

    const QRect wide = ScreenCapture::mapLogicalRectToImage(
      QRect(0, 0, 3, 3), QRect(0, 0, 1536, 864), QSize(1920, 1080));
    QCOMPARE(wide.size(), QSize(4, 4));
}

void ScreenCaptureTests::clipsSelectionToCaptureRect()
{
    const QRect mapped = ScreenCapture::mapLogicalRectToImage(
      QRect(-2000, -10, 200, 100), QRect(-1920, 0, 1920, 1080), QSize(3840, 2160));
    QCOMPARE(mapped, QRect(0, 0, 240, 180));
}

void ScreenCaptureTests::rejectsSelectionOutsideCaptureRect()
{
    const QRect mapped = ScreenCapture::mapLogicalRectToImage(
      QRect(0, 0, 50, 50), QRect(-1920, 0, 1920, 1080), QSize(3840, 2160));
    QVERIFY(mapped.isEmpty());
}

void ScreenCaptureTests::freezeUsesChosenMonitorOrigin()
{
    const QImage capture = twoByOneBlueRightHalfImage(200, 100);
    const QPixmap freeze = ScreenCapture::freezeBackgroundForLogicalRect(
      capture, QRect(-100, 0, 100, 50), QRect(-50, 0, 50, 50));
    QCOMPARE(freeze.size(), QSize(100, 100));
    QCOMPARE(freeze.toImage().pixelColor(0, 0), QColor(Qt::blue));
    QCOMPARE(freeze.toImage().pixelColor(99, 99), QColor(Qt::blue));
}

void ScreenCaptureTests::freezePreservesNativePixels_data()
{
    QTest::addColumn<int>("nativeWidth");
    QTest::addColumn<int>("nativeHeight");
    QTest::addColumn<QRect>("subrectCrop");

    QTest::newRow("1x") << 80 << 64 << QRect(16, 16, 32, 16);
    QTest::newRow("1.25x") << 100 << 80 << QRect(20, 20, 40, 20);
    QTest::newRow("1.5x") << 120 << 96 << QRect(24, 24, 48, 24);
    QTest::newRow("2x") << 160 << 128 << QRect(32, 32, 64, 32);
    QTest::newRow("1.75x") << 140 << 112 << QRect(28, 28, 56, 28);
    QTest::newRow("2.25x") << 180 << 144 << QRect(36, 36, 72, 36);
    QTest::newRow("2.5x") << 200 << 160 << QRect(40, 40, 80, 40);
    QTest::newRow("3x") << 240 << 192 << QRect(48, 48, 96, 48);
    QTest::newRow("nominal-4:3-frame") << 107 << 85 << QRect(21, 21, 43, 22);
}

void ScreenCaptureTests::freezePreservesNativePixels()
{
    QFETCH(int, nativeWidth);
    QFETCH(int, nativeHeight);
    QFETCH(QRect, subrectCrop);

    const QImage capture = highFrequencyFixture(nativeWidth, nativeHeight);
    const QRect captureRect(-80, -64, 80, 64);

    const QPixmap full = ScreenCapture::freezeBackgroundForLogicalRect(
      capture, captureRect, captureRect);
    QCOMPARE(full.size(), QSize(nativeWidth, nativeHeight));
    QCOMPARE(full.toImage().convertToFormat(QImage::Format_ARGB32), capture);

    const QPixmap sub = ScreenCapture::freezeBackgroundForLogicalRect(
      capture, captureRect, QRect(-64, -48, 32, 16));
    QCOMPARE(sub.size(), subrectCrop.size());
    QCOMPARE(sub.toImage().convertToFormat(QImage::Format_ARGB32),
             capture.copy(subrectCrop));
}

void ScreenCaptureTests::freezeMapsOddAndClippedRectsAtFractionalScale()
{
    const QImage capture = highFrequencyFixture(100, 80);
    const QRect captureRect(-80, -64, 80, 64);

    const QPixmap odd = ScreenCapture::freezeBackgroundForLogicalRect(
      capture, captureRect, QRect(-79, -63, 3, 3));
    QCOMPARE(odd.size(), QSize(4, 4));
    QCOMPARE(odd.toImage().convertToFormat(QImage::Format_ARGB32),
             capture.copy(QRect(1, 1, 4, 4)));

    const QPixmap clipped = ScreenCapture::freezeBackgroundForLogicalRect(
      capture, captureRect, QRect(-90, -70, 20, 16));
    QCOMPARE(clipped.size(), QSize(13, 13));
    QCOMPARE(clipped.toImage().convertToFormat(QImage::Format_ARGB32),
             capture.copy(QRect(0, 0, 13, 13)));
}

void ScreenCaptureTests::freezeRejectsInvalidAndOutsideRects()
{
    const QRect captureRect(-80, -64, 80, 64);
    QVERIFY(ScreenCapture::freezeBackgroundForLogicalRect(
              QImage(), captureRect, captureRect)
              .isNull());

    const QImage capture = highFrequencyFixture(100, 80);
    QVERIFY(ScreenCapture::freezeBackgroundForLogicalRect(
              capture, captureRect, QRect(10, 10, 10, 10))
              .isNull());
    QVERIFY(ScreenCapture::freezeBackgroundForLogicalRect(
              capture, captureRect, QRect())
              .isNull());
}

void ScreenCaptureTests::freezeKeepsEachMonitorFrameIndependent()
{
    struct MonitorCase {
        QRect captureRect;
        QSize native;
        QRect subCrop;
    };
    const MonitorCase cases[] = {
        {QRect(0, 0, 80, 64), QSize(100, 80), QRect(20, 20, 40, 20)},
        {QRect(80, -16, 80, 64), QSize(160, 128), QRect(32, 32, 64, 32)},
        {QRect(-80, 64, 80, 64), QSize(120, 96), QRect(24, 24, 48, 24)},
        {QRect(0, 0, 80, 64), QSize(100, 80), QRect(20, 20, 40, 20)},
    };
    for (const MonitorCase& monitorCase : cases) {
        const QImage capture = highFrequencyFixture(
          monitorCase.native.width(), monitorCase.native.height());

        const QPixmap full = ScreenCapture::freezeBackgroundForLogicalRect(
          capture, monitorCase.captureRect, monitorCase.captureRect);
        QCOMPARE(full.size(), monitorCase.native);
        QCOMPARE(full.toImage().convertToFormat(QImage::Format_ARGB32), capture);

        const QRect selection(monitorCase.captureRect.topLeft() + QPoint(16, 16),
                              QSize(32, 16));
        const QPixmap sub = ScreenCapture::freezeBackgroundForLogicalRect(
          capture, monitorCase.captureRect, selection);
        QCOMPARE(sub.size(), monitorCase.subCrop.size());
        QCOMPARE(sub.toImage().convertToFormat(QImage::Format_ARGB32),
                 capture.copy(monitorCase.subCrop));
    }
}

void ScreenCaptureTests::paintFreezeBackgroundExactRender_data()
{
    QTest::addColumn<int>("nativeWidth");
    QTest::addColumn<int>("nativeHeight");
    QTest::addColumn<qreal>("dpr");
    QTest::addColumn<bool>("tagPixmapDpr");

    QTest::newRow("1.25x") << 100 << 80 << 1.25 << false;
    QTest::newRow("1.5x") << 120 << 96 << 1.5 << false;
    QTest::newRow("2x") << 160 << 128 << 2.0 << false;
    QTest::newRow("1.25x-tagged") << 100 << 80 << 1.25 << true;
    QTest::newRow("2x-tagged") << 160 << 128 << 2.0 << true;
    QTest::newRow("1.75x") << 140 << 112 << 1.75 << false;
    QTest::newRow("2.25x") << 180 << 144 << 2.25 << false;
    QTest::newRow("2.5x") << 200 << 160 << 2.5 << false;
    QTest::newRow("3x") << 240 << 192 << 3.0 << false;
    QTest::newRow("1.75x-tagged") << 140 << 112 << 1.75 << true;
    QTest::newRow("2.5x-tagged") << 200 << 160 << 2.5 << true;
}

void ScreenCaptureTests::paintFreezeBackgroundExactRender()
{
    QFETCH(int, nativeWidth);
    QFETCH(int, nativeHeight);
    QFETCH(qreal, dpr);
    QFETCH(bool, tagPixmapDpr);

    const QImage native = highFrequencyFixture(nativeWidth, nativeHeight);
    QPixmap pixmap = QPixmap::fromImage(native);
    if (tagPixmapDpr) {
        pixmap.setDevicePixelRatio(dpr);
    }

    QImage target(nativeWidth, nativeHeight, QImage::Format_ARGB32);
    target.setDevicePixelRatio(dpr);
    target.fill(Qt::black);
    {
        QPainter p(&target);
        ScreenCapture::paintFreezeBackground(
          p, pixmap, QRect(0, 0, 80, 64), QRect(0, 0, 80, 64));
    }
    QImage rendered = target;
    rendered.setDevicePixelRatio(1);
    QCOMPARE(rendered, native);
}

void ScreenCaptureTests::paintFreezeBackgroundClippedSelection()
{
    const qreal dpr = 1.25;
    const QImage native = highFrequencyFixture(100, 80);
    const QPixmap pixmap = QPixmap::fromImage(native);
    const QRect targetRect(8, 8, 80, 64);
    const QRect clipRect(24, 24, 32, 16);
    const QRect deviceClip(30, 30, 40, 20);
    const QColor sentinel(Qt::magenta);

    QImage reference(120, 100, QImage::Format_ARGB32);
    reference.setDevicePixelRatio(dpr);
    reference.fill(sentinel);
    {
        QPainter p(&reference);
        ScreenCapture::paintFreezeBackground(
          p, pixmap, targetRect, QRect(0, 0, 96, 80));
    }

    QImage clipped(120, 100, QImage::Format_ARGB32);
    clipped.setDevicePixelRatio(dpr);
    clipped.fill(sentinel);
    {
        QPainter p(&clipped);
        ScreenCapture::paintFreezeBackground(p, pixmap, targetRect, clipRect);
    }

    for (int y = 0; y < clipped.height(); ++y) {
        for (int x = 0; x < clipped.width(); ++x) {
            if (deviceClip.contains(x, y)) {
                QCOMPARE(clipped.pixelColor(x, y), reference.pixelColor(x, y));
            } else {
                QCOMPARE(clipped.pixelColor(x, y), sentinel);
            }
        }
    }
    QImage mapped = reference.copy(10, 10, 100, 80);
    mapped.setDevicePixelRatio(1);
    QCOMPARE(mapped, native);
}

void ScreenCaptureTests::paintFreezeBackgroundHonorsExistingClipAndRestoresState()
{
    const qreal dpr = 1.25;
    const QImage native = highFrequencyFixture(100, 80);
    const QPixmap pixmap = QPixmap::fromImage(native);
    const QRect targetRect(8, 8, 80, 64);
    const QColor sentinel(Qt::magenta);

    QImage reference(120, 100, QImage::Format_ARGB32);
    reference.setDevicePixelRatio(dpr);
    reference.fill(sentinel);
    {
        QPainter p(&reference);
        ScreenCapture::paintFreezeBackground(
          p, pixmap, targetRect, QRect(0, 0, 96, 80));
    }

    const QRect existingClip(40, 16, 32, 32);
    const QRect deviceIntersection(50, 30, 20, 20);

    QImage target(120, 100, QImage::Format_ARGB32);
    target.setDevicePixelRatio(dpr);
    target.fill(sentinel);
    {
        QPainter p(&target);
        p.setClipRect(existingClip);
        const QTransform transformBefore = p.worldTransform();
        ScreenCapture::paintFreezeBackground(
          p, pixmap, targetRect, QRect(24, 24, 32, 16));
        QCOMPARE(p.clipRegion(), QRegion(existingClip));
        QCOMPARE(p.worldTransform(), transformBefore);
    }

    for (int y = 0; y < target.height(); ++y) {
        for (int x = 0; x < target.width(); ++x) {
            if (deviceIntersection.contains(x, y)) {
                QCOMPARE(target.pixelColor(x, y), reference.pixelColor(x, y));
            } else {
                QCOMPARE(target.pixelColor(x, y), sentinel);
            }
        }
    }

    QImage unused(16, 16, QImage::Format_ARGB32);
    unused.fill(sentinel);
    {
        QPainter p(&unused);
        p.translate(3, 2);
        p.rotate(15);
        const QRect transformedClip(1, 1, 9, 7);
        p.setClipRect(transformedClip);
        const QRegion clipBefore = p.clipRegion();
        const QTransform transformBefore = p.worldTransform();
        ScreenCapture::paintFreezeBackground(
          p, pixmap, targetRect, QRect(0, 0, 96, 80));
        QCOMPARE(p.clipRegion(), clipBefore);
        QCOMPARE(p.worldTransform(), transformBefore);
    }
}

void ScreenCaptureTests::cropsMixedDpiFrameToTempPng()
{
    const QImage frame = twoByOneBlueRightHalfImage(3840, 2160);
    QString path;
    QSize size;
    QString error;
    QVERIFY(ScreenCapture::cropLogicalSelectionToTempPng(
      frame, QRect(-1920, 0, 1920, 1080), QRect(-1820, 50, 200, 100), path, size, error));
    QCOMPARE(size, QSize(400, 200));
    QVERIFY(QFile::exists(path));
    QCOMPARE(QImage(path).size(), QSize(400, 200));
    QFile::remove(path);
}

void ScreenCaptureTests::tempPngUsesRealCompression()
{
    // A screenshot-shaped frame: a smooth gradient with structured detail on
    // top. At libpng's default level this is a small fraction of its raw size;
    // written as stored (uncompressed) blocks it is about the raw size. The
    // temp PNG is what gets renamed straight into the user's Pictures folder,
    // so its size is the size the user sees.
    QImage frame(1600, 1000, QImage::Format_ARGB32);
    for (int y = 0; y < frame.height(); ++y) {
        const int shade = y * 255 / frame.height();
        QRgb* row = reinterpret_cast<QRgb*>(frame.scanLine(y));
        for (int x = 0; x < frame.width(); ++x) {
            row[x] = (x % 40 < 20) ? qRgb(shade, (x / 4) & 0xff, 0x40)
                                   : qRgb(shade, shade, shade);
        }
    }

    QString path;
    QSize size;
    QString error;
    QVERIFY(ScreenCapture::saveImageToTempPng(frame, path, size, error));
    QCOMPARE(size, frame.size());

    QFile file(path);
    QVERIFY(file.open(QIODevice::ReadOnly));
    const qint64 written = file.size();
    file.close();
    const QImage decoded(path);
    QCOMPARE(decoded.size(), frame.size());
    QVERIFY2(decoded.convertToFormat(QImage::Format_RGBA8888)
               == frame.convertToFormat(QImage::Format_RGBA8888),
             "the temp PNG must round-trip the pixels unchanged");
    QFile::remove(path);

    const qint64 raw = qint64(frame.width()) * frame.height() * 4;
    QVERIFY2(written < raw / 8,
             qPrintable(QStringLiteral("temp PNG is not really compressed: %1 bytes for %2 raw")
                          .arg(written)
                          .arg(raw)));
}

void ScreenCaptureTests::cropsEachSyntheticMonitorFromVirtualDesktop()
{
    const QImage desktop = twoByOneBlueRightHalfImage(3840, 1080);
    const QRect desktopBounds(0, 0, 3840, 1080);
    QString leftPath;
    QString rightPath;
    QSize size;
    QString error;
    QVERIFY(ScreenCapture::cropLogicalSelectionToTempPng(
      desktop, desktopBounds, QRect(0, 0, 1920, 1080), leftPath, size, error));
    QCOMPARE(size, QSize(1920, 1080));
    QVERIFY(ScreenCapture::cropLogicalSelectionToTempPng(
      desktop, desktopBounds, QRect(1920, 0, 1920, 1080), rightPath, size, error));
    QCOMPARE(size, QSize(1920, 1080));
    const QImage left(leftPath);
    const QImage right(rightPath);
    QCOMPARE(left.pixelColor(20, 20), QColor(Qt::red));
    QCOMPARE(right.pixelColor(20, 20), QColor(Qt::blue));
    QFile::remove(leftPath);
    QFile::remove(rightPath);
}

void ScreenCaptureTests::portalRoutePreparesAndCapturesThroughHelper()
{
    FakePortalHelper helper;
    QVERIFY(helper.install());
    qputenv("FAKE_HELPER_MODE", QByteArrayLiteral("ready"));
    helper.writeFrame(8, 6);

    ScreenCapture::CaptureSession session;
    QString error;
    QCOMPARE(session.prepare(QGuiApplication::primaryScreen(), true, error), ScreenCapture::CaptureStatus::Ok);
    QVERIFY(session.isPrepared());
    QCOMPARE(session.captureRect(), QGuiApplication::primaryScreen()->geometry());

    QImage frame;
    QCOMPARE(session.captureFresh(frame, error), ScreenCapture::CaptureStatus::Ok);
    QCOMPARE(frame.size(), QSize(8, 6));
    QVERIFY(!QFile::exists(helper.framePath));
}

void ScreenCaptureTests::portalRouteMapsHelperCancellation()
{
    FakePortalHelper helper;
    QVERIFY(helper.install());
    qputenv("FAKE_HELPER_MODE", QByteArrayLiteral("cancel"));

    ScreenCapture::CaptureSession session;
    QString error;
    QCOMPARE(session.prepare(QGuiApplication::primaryScreen(), true, error), ScreenCapture::CaptureStatus::Cancelled);
    QCOMPARE(error, QStringLiteral("user dismissed"));
    QVERIFY(!session.isPrepared());
}

void ScreenCaptureTests::portalRouteMapsHelperFailure()
{
    FakePortalHelper helper;
    QVERIFY(helper.install());
    qputenv("FAKE_HELPER_MODE", QByteArrayLiteral("fail"));

    ScreenCapture::CaptureSession session;
    QString error;
    QCOMPARE(session.prepare(QGuiApplication::primaryScreen(), true, error), ScreenCapture::CaptureStatus::Failed);
    QCOMPARE(error, QStringLiteral("portal broke"));
}

void ScreenCaptureTests::portalRouteRejectsMismatchedStream()
{
    FakePortalHelper helper;
    QVERIFY(helper.install());
    qputenv("FAKE_HELPER_MODE", QByteArrayLiteral("mismatch"));

    ScreenCapture::CaptureSession session;
    QString error;
    QCOMPARE(session.prepare(QGuiApplication::primaryScreen(), true, error), ScreenCapture::CaptureStatus::Failed);
    QVERIFY(!session.isPrepared());
}

void ScreenCaptureTests::portalRouteFailsWithoutHelper()
{
    FakePortalHelper helper;
    QVERIFY(helper.install());
    qputenv("APEXSHOT_PORTAL_HELPER", helper.dir.filePath(QStringLiteral("missing-helper")).toUtf8());

    ScreenCapture::CaptureSession session;
    QString error;
    QCOMPARE(session.prepare(QGuiApplication::primaryScreen(), true, error), ScreenCapture::CaptureStatus::Failed);
    QVERIFY(!error.isEmpty());
}

void ScreenCaptureTests::portalWindowRoutePreparesAndCapturesThroughHelper()
{
    FakePortalHelper helper;
    QVERIFY(helper.install());
    qputenv("FAKE_HELPER_MODE", QByteArrayLiteral("ready"));
    helper.writeFrame(8, 6);

    ScreenCapture::CaptureSession session;
    QString error;
    QCOMPARE(session.prepareWindow(true, error), ScreenCapture::CaptureStatus::Ok);
    QVERIFY(session.isPrepared());
    QFile arguments(helper.argsPath);
    QVERIFY(arguments.open(QIODevice::ReadOnly));
    QCOMPARE(arguments.readAll(), QByteArrayLiteral("portal-window-still-internal\n1\n"));

    QImage frame;
    QCOMPARE(session.captureFresh(frame, error), ScreenCapture::CaptureStatus::Ok);
    QCOMPARE(frame.size(), QSize(8, 6));
    QVERIFY(!QFile::exists(helper.framePath));
}

void ScreenCaptureTests::portalWindowRouteMapsHelperCancellation()
{
    FakePortalHelper helper;
    QVERIFY(helper.install());
    qputenv("FAKE_HELPER_MODE", QByteArrayLiteral("cancel"));

    ScreenCapture::CaptureSession session;
    QString error;
    QCOMPARE(session.prepareWindow(false, error), ScreenCapture::CaptureStatus::Cancelled);
    QCOMPARE(error, QStringLiteral("user dismissed"));
    QVERIFY(!session.isPrepared());
}

void ScreenCaptureTests::portalWindowRouteMapsHelperFailure()
{
    FakePortalHelper helper;
    QVERIFY(helper.install());
    qputenv("FAKE_HELPER_MODE", QByteArrayLiteral("fail"));

    ScreenCapture::CaptureSession session;
    QString error;
    QCOMPARE(session.prepareWindow(false, error), ScreenCapture::CaptureStatus::Failed);
    QCOMPARE(error, QStringLiteral("portal broke"));
    QVERIFY(!session.isPrepared());
}

void ScreenCaptureTests::waylandSessionUsesPortalWithX11QtPlatform()
{
    FakePortalHelper helper;
    QVERIFY(helper.install());
    qunsetenv("FLATPAK_ID");
    const QByteArray previous = qgetenv("XDG_SESSION_TYPE");
    qputenv("XDG_SESSION_TYPE", "wayland");
    qputenv("FAKE_HELPER_MODE", "cancel");
    ScreenCapture::CaptureSession session;
    QString error;
    const auto status = session.prepare(QGuiApplication::primaryScreen(), false, error);
    qputenv("XDG_SESSION_TYPE", previous);
    QCOMPARE(status, ScreenCapture::CaptureStatus::Cancelled);
}

void ScreenCaptureTests::nativeX11DoesNotRequestPortal()
{
    FakePortalHelper helper;
    QVERIFY(helper.install());
    qunsetenv("FLATPAK_ID");
    const QByteArray previous = qgetenv("XDG_SESSION_TYPE");
    qputenv("XDG_SESSION_TYPE", "x11");
    qputenv("FAKE_HELPER_MODE", "cancel");
    ScreenCapture::CaptureSession session;
    QString error;
    const auto status = session.prepare(QGuiApplication::primaryScreen(), false, error);
    qputenv("XDG_SESSION_TYPE", previous);
    QCOMPARE(status, ScreenCapture::CaptureStatus::Ok);
}

void ScreenCaptureTests::parsesLayerShellCaptureMenuResult()
{
    CaptureMenuResult::Selection selection;
    QString error;
    QVERIFY(CaptureMenuResult::parse(
      QByteArrayLiteral(R"({
          "action":"area",
          "recording":false,
          "ocr":true,
          "timer_seconds":0,
          "microphone":true,
          "speaker":true,
          "mic_device":"alsa_input.test",
          "speaker_device":"alsa_output.test.monitor"
      })"),
      true,
      &selection,
      &error));
    QCOMPARE(static_cast<int>(selection.action),
             static_cast<int>(CaptureMenuResult::Action::Area));
    QVERIFY(!selection.recording);
    QVERIFY(selection.ocr);
    QCOMPARE(selection.timerSeconds, 0);
    QVERIFY(selection.microphone);
    QVERIFY(selection.speaker);
    QVERIFY(error.isEmpty());

    QVERIFY(CaptureMenuResult::parse(
      QByteArrayLiteral(R"({
          "action":"area",
          "recording":false,
          "ocr":false,
          "timer_seconds":0,
          "microphone":true,
          "speaker":true,
          "mic_device":"alsa_input.test",
          "speaker_device":"alsa_output.test.monitor"
      })"),
      true,
      &selection));
    QVERIFY(!selection.recording);
    QVERIFY(selection.microphone);
    QVERIFY(selection.speaker);

    QVERIFY(CaptureMenuResult::parse(
      QByteArrayLiteral(R"({
          "action":"window",
          "recording":true,
          "ocr":false,
          "timer_seconds":0,
          "microphone":true,
          "speaker":true,
          "mic_device":"alsa_input.test",
          "speaker_device":"alsa_output.test.monitor"
      })"),
      true,
      &selection));
    QCOMPARE(static_cast<int>(selection.action),
             static_cast<int>(CaptureMenuResult::Action::Window));
    QVERIFY(selection.recording);
    QVERIFY(selection.microphone);
    QVERIFY(selection.speaker);
    QCOMPARE(selection.microphoneDevice, QStringLiteral("alsa_input.test"));
    QCOMPARE(selection.speakerDevice, QStringLiteral("alsa_output.test.monitor"));
}

void ScreenCaptureTests::rejectsMalformedLayerShellCaptureMenuResult()
{
    CaptureMenuResult::Selection selection;
    const QByteArray invalid[] = {
        QByteArrayLiteral("not json"),
        QByteArrayLiteral("[]"),
        QByteArrayLiteral("{\"action\":\"display\"}"),
        QByteArrayLiteral(R"({
            "action":"cancel",
            "recording":false,
            "ocr":false,
            "timer_seconds":0,
            "microphone":false,
            "speaker":false,
            "mic_device":"",
            "speaker_device":""
        })"),
        QByteArrayLiteral(R"({
            "action":"display",
            "recording":false,
            "ocr":false,
            "timer_seconds":0,
            "microphone":false,
            "speaker":false,
            "mic_device":"",
            "speaker_device":"",
            "unexpected":true
        })"),
    };
    for (const QByteArray& json : invalid) {
        QVERIFY(!CaptureMenuResult::parse(json, true, &selection));
    }
}

void ScreenCaptureTests::rejectsUnsupportedLayerShellCaptureMenuState()
{
    CaptureMenuResult::Selection selection;
    QVERIFY(!CaptureMenuResult::parse(
      QByteArrayLiteral(R"({
          "action":"display",
          "recording":false,
          "ocr":true,
          "timer_seconds":0,
          "microphone":false,
          "speaker":false,
          "mic_device":"alsa_input.test",
          "speaker_device":"alsa_output.test.monitor"
      })"),
      true,
      &selection));
    QVERIFY(!CaptureMenuResult::parse(
      QByteArrayLiteral(R"({
          "action":"area",
          "recording":true,
          "ocr":false,
          "timer_seconds":0,
          "microphone":false,
          "speaker":false,
          "mic_device":"alsa_input.test",
          "speaker_device":"alsa_output.test.monitor"
      })"),
      true,
      &selection));
    QVERIFY(!CaptureMenuResult::parse(
      QByteArrayLiteral(R"({
          "action":"window",
          "recording":false,
          "ocr":false,
          "timer_seconds":5,
          "microphone":false,
          "speaker":false,
          "mic_device":"alsa_input.test",
          "speaker_device":"alsa_output.test.monitor"
      })"),
      true,
      &selection));
    QVERIFY(!CaptureMenuResult::parse(
      QByteArrayLiteral(R"({
          "action":"display",
          "recording":true,
          "ocr":false,
          "timer_seconds":0,
          "microphone":false,
          "speaker":false,
          "mic_device":"alsa_input.test",
          "speaker_device":"alsa_output.test.monitor"
      })"),
      false,
      &selection));
}

int main(int argc, char* argv[])
{
    qputenv("QT_QPA_PLATFORM", "offscreen");
    QGuiApplication app(argc, argv);
    ScreenCaptureTests tests;
    return QTest::qExec(&tests, argc, argv);
}

#include "ScreenCaptureTests.moc"
