// Windows-only smoke test for the ffmpeg-backed recorder.
//
// Records a small desktop region twice: once straight through, once with a
// pause and a resume. Each run must produce a non-empty MP4. Intentional
// console output; this is not part of the shipped binary.

#include "platform/recording.h"

#include <QCoreApplication>
#include <QDir>
#include <QEventLoop>
#include <QFile>
#include <QFileInfo>
#include <QTimer>

#include <cstdio>

namespace {

const QRect kRegion(24, 24, 320, 240);

int recordOnce(bool pauseAndResume)
{
    Platform::RecordingSession session;
    const QString path = QDir(QDir::tempPath())
                             .filePath(pauseAndResume
                                           ? QStringLiteral("apexshot-record-pause.mp4")
                                           : QStringLiteral("apexshot-record-basic.mp4"));
    QFile::remove(path);

    Platform::RecordingRequest request;
    request.region = kRegion;
    request.outputPath = path;
    request.includeCursor = true;
    request.includeMicrophone = !qEnvironmentVariableIsEmpty("APEXSHOT_TEST_MIC");
    request.fps = 30;
    request.crf = 28;

    int result = 1;
    QEventLoop loop;

    QObject::connect(&session,
                     &Platform::RecordingSession::finished,
                     &session,
                     [&](bool kept, const QString& output, const QString& error) {
                         const QFileInfo info(output);
                         if (!kept || output.isEmpty() || !info.exists() || info.size() <= 0) {
                             std::printf("FAIL %s: %s\n",
                                         pauseAndResume ? "pause/resume" : "basic",
                                         error.isEmpty() ? "no output" : qPrintable(error));
                             result = 1;
                         } else {
                             std::printf("OK %s: %s (%lld bytes)\n",
                                         pauseAndResume ? "pause/resume" : "basic",
                                         qPrintable(output),
                                         static_cast<long long>(info.size()));
                             result = 0;
                             if (qEnvironmentVariableIsEmpty("APEXSHOT_KEEP_TEST_OUTPUT"))
                                 QFile::remove(output);
                         }
                         loop.quit();
                     });

    QString error;
    if (!session.start(request, error)) {
        std::printf("FAIL start: %s\n", qPrintable(error));
        return 1;
    }

    if (pauseAndResume) {
        QTimer::singleShot(800, &session, [&session] { session.pause(); });
        QTimer::singleShot(1800, &session, [&session] { session.resume(); });
        QTimer::singleShot(2800, &session, [&session] { session.stop(); });
    } else {
        QTimer::singleShot(2000, &session, [&session] { session.stop(); });
    }

    QTimer::singleShot(20000, &session, [&loop] {
        std::printf("FAIL timeout\n");
        loop.quit();
    });

    loop.exec();
    return result;
}

} // namespace

int main(int argc, char** argv)
{
    QCoreApplication app(argc, argv);

    QString error;
    if (!Platform::RecordingSession::available(&error)) {
        std::printf("SKIP: %s\n", qPrintable(error));
        return 77;
    }

    int result = recordOnce(false);
    if (result == 0)
        result = recordOnce(true);
    return result;
}
