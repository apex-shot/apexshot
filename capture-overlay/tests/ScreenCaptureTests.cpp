#include "ScreenCapture.h"

#include <QFile>
#include <QGuiApplication>
#include <QImage>
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
    void cropsMixedDpiFrameToTempPng();
    void cropsEachSyntheticMonitorFromVirtualDesktop();
    void portalRoutePreparesAndCapturesThroughHelper();
    void portalRouteMapsHelperCancellation();
    void portalRouteMapsHelperFailure();
    void portalRouteRejectsMismatchedStream();
    void portalRouteFailsWithoutHelper();
    void waylandSessionUsesPortalWithX11QtPlatform();
    void nativeX11DoesNotRequestPortal();
};

namespace {

using ScreenCapture::HelperMessage;

const char* kFakeHelperScript = R"SH(#!/bin/sh
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

    bool install()
    {
        if (!dir.isValid()) {
            return false;
        }
        helperPath = dir.filePath(QStringLiteral("apexshot-fake-helper"));
        framePath = dir.filePath(QStringLiteral("frame.png"));
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
    QCOMPARE(freeze.size(), QSize(50, 50));
    QCOMPARE(freeze.toImage().pixelColor(0, 0), QColor(Qt::blue));
    QCOMPARE(freeze.toImage().pixelColor(49, 49), QColor(Qt::blue));
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

int main(int argc, char* argv[])
{
    qputenv("QT_QPA_PLATFORM", "offscreen");
    QGuiApplication app(argc, argv);
    ScreenCaptureTests tests;
    return QTest::qExec(&tests, argc, argv);
}

#include "ScreenCaptureTests.moc"
