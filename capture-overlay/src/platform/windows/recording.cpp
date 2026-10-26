#include "platform/recording.h"

#include "platform/recording_settings.h"
#include "platform/windows/pointer_tracker.h"
#include "platform/windows/settings_store.h"

#include <QCoreApplication>
#include <QDateTime>
#include <QDebug>
#include <QDir>
#include <QElapsedTimer>
#include <QFile>
#include <QFileInfo>
#include <QHash>
#include <QProcess>
#include <QRegularExpression>
#include <QStandardPaths>
#include <QTimer>

#include <algorithm>

namespace {

constexpr int kProbeTimeoutMs = 5000;
// Opening a capture device takes a second on an idle machine and several under
// load, so a camera probe gets room the ffmpeg one-shot checks do not need.
constexpr int kCameraProbeTimeoutMs = 15000;
// Long recordings rewrite the MP4 for fast start on the way out. Give that
// pass room to finish instead of killing it and corrupting the file.
constexpr int kStopTimeoutMs = 30000;
// A discard deletes the file anyway, so a stuck encoder is not worth waiting
// for beyond a few seconds.
constexpr int kDiscardTimeoutMs = 3000;

QString ffmpegExecutable()
{
    const QString configured = qEnvironmentVariable("APEXSHOT_FFMPEG").trimmed();
    if (!configured.isEmpty() && QFileInfo::exists(configured))
        return QDir::cleanPath(configured);

    const QString appDir = QCoreApplication::applicationDirPath();
    const QStringList besideApp = {
        QDir(appDir).filePath(QStringLiteral("ffmpeg.exe")),
        QDir(appDir).filePath(QStringLiteral("bin/ffmpeg.exe")),
    };
    for (const QString& candidate : besideApp) {
        if (QFileInfo::exists(candidate))
            return QDir::cleanPath(candidate);
    }

    return QStandardPaths::findExecutable(QStringLiteral("ffmpeg"));
}

QString tailLines(const QString& text, int count)
{
    QStringList lines = text.split(QRegularExpression(QStringLiteral("[\\r\\n]+")),
                                   Qt::SkipEmptyParts);
    while (lines.size() > count)
        lines.removeFirst();
    return lines.join(QLatin1Char('\n')).trimmed();
}

bool runProbe(const QStringList& arguments, QString* out, QString* error)
{
    const QString program = ffmpegExecutable();
    if (program.isEmpty()) {
        if (error)
            *error = QStringLiteral("ffmpeg was not found. Place ffmpeg.exe next to "
                                    "ApexShot, in its bin folder, or on PATH.");
        return false;
    }

    QProcess probe;
    probe.setProgram(program);
    probe.setArguments(arguments);
    probe.setProcessChannelMode(QProcess::MergedChannels);
    probe.start();
    if (!probe.waitForStarted(kProbeTimeoutMs)) {
        if (error)
            *error = QStringLiteral("ffmpeg could not be started: %1").arg(probe.errorString());
        return false;
    }
    if (!probe.waitForFinished(kProbeTimeoutMs)) {
        probe.kill();
        probe.waitForFinished(1000);
        if (error)
            *error = QStringLiteral("ffmpeg did not answer its probe");
        return false;
    }

    const QString output = QString::fromUtf8(probe.readAll());
    if (out)
        *out = output;
    if (probe.exitStatus() != QProcess::NormalExit || probe.exitCode() != 0) {
        if (error) {
            const QString detail = tailLines(output, 6);
            *error = detail.isEmpty()
                ? QStringLiteral("ffmpeg exited with code %1").arg(probe.exitCode())
                : detail;
        }
        return false;
    }
    return true;
}

QStringList audioInputs()
{
    static bool probed = false;
    static QStringList names;
    if (probed)
        return names;
    probed = true;

    QString output;
    QString error;
    if (!runProbe({QStringLiteral("-hide_banner"),
                   QStringLiteral("-list_devices"),
                   QStringLiteral("true"),
                   QStringLiteral("-f"),
                   QStringLiteral("dshow"),
                   QStringLiteral("-i"),
                   QStringLiteral("dummy")},
                  &output,
                  &error)) {
        // `-list_devices` exits non-zero on every release; the device list is
        // still in the output, so only a launch failure is fatal.
        if (output.isEmpty())
            return names;
    }

    // Match one line at a time: a whole-output search would let a quoted
    // alternative device name on one line pair with the device name on the
    // next and capture the log prefix between them.
    static const QRegularExpression deviceLine(
        QStringLiteral("\"([^\"]+)\"\\s*\\(audio\\)"));
    const QStringList lines =
        output.split(QRegularExpression(QStringLiteral("[\\r\\n]+")));
    for (const QString& line : lines) {
        const QRegularExpressionMatch match = deviceLine.match(line);
        if (match.hasMatch())
            names.append(match.captured(1));
    }
    return names;
}

QString microphoneDevice()
{
    const QStringList devices = audioInputs();
    for (const QString& name : devices) {
        if (name.contains(QStringLiteral("microphone"), Qt::CaseInsensitive))
            return name;
    }
    return devices.isEmpty() ? QString() : devices.first();
}

QString loopbackDevice()
{
    static const QStringList loopbackHints = {
        QStringLiteral("stereo mix"),
        QStringLiteral("virtual-audio-capturer"),
        QStringLiteral("cable output"),
        QStringLiteral("loopback"),
    };
    const QStringList devices = audioInputs();
    for (const QString& hint : loopbackHints) {
        for (const QString& name : devices) {
            if (name.contains(hint, Qt::CaseInsensitive))
                return name;
        }
    }
    return QString();
}

QString defaultOutputPath()
{
    const Platform::ShellSettings settings = Platform::loadShellSettings();
    const QString path =
        Platform::shellRecordingOutputPath(settings, QDateTime::currentDateTime());
    QDir().mkpath(QFileInfo(path).absolutePath());
    return path;
}

QString ffprobeExecutable()
{
    const QString ffmpeg = ffmpegExecutable();
    if (!ffmpeg.isEmpty()) {
        const QDir directory = QFileInfo(ffmpeg).absoluteDir();
        const QString besideFfmpeg = directory.filePath(QStringLiteral("ffprobe.exe"));
        if (QFileInfo::exists(besideFfmpeg))
            return QDir::cleanPath(besideFfmpeg);
    }
    return QStandardPaths::findExecutable(QStringLiteral("ffprobe"));
}

/// Read the first `width,height` pair out of an ffprobe `csv=p=0` listing.
QSize parseProbeSize(const QString& output)
{
    for (const QString& line : output.split(QRegularExpression(QStringLiteral("[\\r\\n]+")),
                                            Qt::SkipEmptyParts)) {
        const QStringList fields = line.trimmed().split(QLatin1Char(','));
        if (fields.size() < 2)
            continue;
        bool widthOk = false;
        bool heightOk = false;
        const int width = fields.at(0).trimmed().toInt(&widthOk);
        const int height = fields.at(1).trimmed().toInt(&heightOk);
        if (widthOk && heightOk && width > 0 && height > 0)
            return QSize(width, height);
    }
    return QSize();
}

/// Every camera dshow reports. The device list is one probe for the whole run:
/// listing capture devices opens each one, which is slow enough to notice.
QStringList cameraDevices()
{
    static bool probed = false;
    static QStringList names;
    if (probed)
        return names;
    probed = true;

    QString output;
    if (!runProbe({QStringLiteral("-hide_banner"), QStringLiteral("-list_devices"),
                   QStringLiteral("true"), QStringLiteral("-f"), QStringLiteral("dshow"),
                   QStringLiteral("-i"), QStringLiteral("dummy")},
                  &output,
                  nullptr)) {
        if (output.isEmpty())
            return names;
    }

    static const QRegularExpression deviceLine(
        QStringLiteral("\"([^\"]+)\"\\s*\\(video\\)"));
    for (const QString& line : output.split(QRegularExpression(QStringLiteral("[\\r\\n]+")))) {
        const QRegularExpressionMatch match = deviceLine.match(line);
        if (match.hasMatch())
            names.append(match.captured(1));
    }
    return names;
}

bool runFfprobe(const QStringList& arguments, QString* out)
{
    const QString program = ffprobeExecutable();
    if (program.isEmpty())
        return false;

    QProcess probe;
    probe.setProgram(program);
    probe.setArguments(arguments);
    probe.setProcessChannelMode(QProcess::MergedChannels);
    probe.start();
    if (!probe.waitForStarted(kCameraProbeTimeoutMs))
        return false;
    if (!probe.waitForFinished(kCameraProbeTimeoutMs)) {
        probe.kill();
        probe.waitForFinished(1000);
        return false;
    }
    if (out)
        *out = QString::fromUtf8(probe.readAll());
    return probe.exitStatus() == QProcess::NormalExit && probe.exitCode() == 0;
}

/// The size a camera delivers at `fps`, or an empty size when it cannot be
/// opened. The answer is cached per camera and rate: opening a capture device
/// takes about a second, and the recorder asks once per recording.
QSize cameraFrameSize(const QString& device, int fps)
{
    static QHash<QString, QSize> sizes;
    const QString key = QStringLiteral("%1|%2").arg(device).arg(fps);
    const auto cached = sizes.constFind(key);
    if (cached != sizes.constEnd())
        return cached.value();

    QStringList arguments{QStringLiteral("-v"), QStringLiteral("error"),
                          QStringLiteral("-f"), QStringLiteral("dshow")};
    if (fps > 0)
        arguments << QStringLiteral("-framerate") << QString::number(fps);
    arguments << QStringLiteral("-i") << QStringLiteral("video=%1").arg(device)
              << QStringLiteral("-select_streams") << QStringLiteral("v:0")
              << QStringLiteral("-show_entries") << QStringLiteral("stream=width,height")
              << QStringLiteral("-of") << QStringLiteral("csv=p=0");

    QString output;
    const QSize size = runFfprobe(arguments, &output) ? parseProbeSize(output) : QSize();
    sizes.insert(key, size);
    return size;
}

double videoDuration(const QString& path)
{
    QString output;
    const QStringList arguments{QStringLiteral("-v"), QStringLiteral("error"),
                                QStringLiteral("-show_entries"), QStringLiteral("format=duration"),
                                QStringLiteral("-of"), QStringLiteral("csv=p=0"), path};
    if (!runFfprobe(arguments, &output))
        return 0.0;
    bool ok = false;
    const double seconds =
        output.trimmed()
            .split(QRegularExpression(QStringLiteral("[\\r\\n]+")), Qt::SkipEmptyParts)
            .value(0)
            .toDouble(&ok);
    return ok && seconds > 0.0 ? seconds : 0.0;
}

QSize videoFrameSize(const QString& path)
{
    QString output;
    const QStringList arguments{
        QStringLiteral("-v"), QStringLiteral("error"), QStringLiteral("-select_streams"),
        QStringLiteral("v:0"), QStringLiteral("-show_entries"),
        QStringLiteral("stream=width,height"), QStringLiteral("-of"), QStringLiteral("csv=p=0"),
        path,
    };
    return runFfprobe(arguments, &output) ? parseProbeSize(output) : QSize();
}

/// The bubble-sized frame the preview pipe carries for a shared shape index.
/// The pipe is fixed at the bubble's own aspect so the window paints it 1:1
/// without asking what the camera negotiated.
QSize webcamPreviewFrameSize(int shape)
{
    switch (shape) {
    case 0:  // circle
    case 1:  // square
        return QSize(480, 480);
    case 2:  // rectangle, 4:3
        return QSize(480, 360);
    default: // vertical, 1:1.3
        return QSize(480, 624);
    }
}

QString concatListPath(const QString& workDir)
{
    return QDir(workDir).filePath(QStringLiteral("segments.txt"));
}

QString quoteConcatPath(const QString& path)
{
    QString escaped = QDir::fromNativeSeparators(path);
    escaped.replace(QLatin1Char('\''), QStringLiteral("'\\''"));
    return QStringLiteral("file '%1'").arg(escaped);
}

} // namespace

namespace Platform {

class RecordingSession::Private
{
public:
    enum class State {
        Idle,
        Recording,
        Pausing,
        Paused,
        Stopping,
        Failed,
    };

    explicit Private(RecordingSession* owner)
        : q(owner)
    {
        tick = new QTimer(q);
        tick->setInterval(200);
        QObject::connect(tick, &QTimer::timeout, q, [this] { emit q->elapsedChanged(elapsedMs()); });
    }

    ~Private() = default;

    bool startSegment(QString& error);
    void onProcessFinished(int exitCode, QProcess::ExitStatus status);
    void onProcessError(QProcess::ProcessError error);
    void requestQuit(int timeoutMs);
    void finalize();
    void completeWebcamTrack(const QString& videoPath);
    void finishJoin(bool ok, const QString& finalPath, const QString& failure, bool keepWorkDir);
    void cleanupAndEmit(bool kept,
                        const QString& path,
                        const QString& failure,
                        bool keepWorkDir = false);
    void fail(const QString& reason);
    void cleanupWorkDir();
    void removeDiscardedSidecar();
    void consumeWebcamPreview(const QByteArray& chunk);
    [[nodiscard]] qint64 elapsedMs() const;
    [[nodiscard]] QStringList buildArguments(const QString& segmentPath,
                                             const QString& webcamSegmentPath) const;
    [[nodiscard]] QStringList buildAudioArguments(QStringList& warnings) const;
    [[nodiscard]] QString captureOutput() const;

    RecordingSession* q = nullptr;
    State state = State::Idle;
    RecordingRequest request;
    QProcess* process = nullptr;
    QProcess* concat = nullptr;
    QTimer* tick = nullptr;
    PointerTracker pointerTracker;
    QElapsedTimer active;
    qint64 elapsedBeforeMs = 0;
    QString workDir;
    QString existingSidecarPath;
    QStringList segments;
    QStringList webcamSegments;
    int segmentIndex = 0;
    bool discardRequested = false;
    bool finishedEmitted = false;
    /// Resume was requested while the previous segment was still finalising.
    bool resumeAfterPause = false;
    QStringList audioArguments;
    QByteArray processOutput;

    /// Set once a camera answered the probe, so a recording without one never
    /// asks ffmpeg for a track that was never opened.
    bool webcamActive = false;
    /// The camera's native frame size, used until the joined track is probed.
    QSize webcamCameraSize;
    /// The bubble-sized frame the preview pipe carries.
    QSize webcamPreviewSize;
    QByteArray webcamPreviewBuffer;
    Platform::WebcamTrack webcamTrack;
};

qint64 RecordingSession::Private::elapsedMs() const
{
    qint64 total = elapsedBeforeMs;
    if (state == State::Recording && active.isValid())
        total += active.elapsed();
    return total;
}

QString RecordingSession::Private::captureOutput() const
{
    return tailLines(QString::fromUtf8(processOutput), 8);
}

QStringList RecordingSession::Private::buildAudioArguments(QStringList& warnings) const
{
    QStringList arguments;
    QStringList sources;

    if (request.includeMicrophone) {
        const QString microphone = microphoneDevice();
        if (microphone.isEmpty())
            warnings.append(QStringLiteral("No microphone was found; recording without audio."));
        else
            sources.append(microphone);
    }
    if (request.includeSpeaker) {
        const QString loopback = loopbackDevice();
        if (loopback.isEmpty()) {
            warnings.append(QStringLiteral(
                "No system-audio loopback device was found; the speaker track is omitted."));
        } else if (!sources.contains(loopback, Qt::CaseInsensitive)) {
            sources.append(loopback);
        }
    }

    for (const QString& source : sources) {
        arguments << QStringLiteral("-f") << QStringLiteral("dshow") << QStringLiteral("-i")
                  << QStringLiteral("audio=%1").arg(source);
    }
    return arguments;
}

QStringList RecordingSession::Private::buildArguments(const QString& segmentPath,
                                                     const QString& webcamSegmentPath) const
{
    QStringList args;
    args << QStringLiteral("-y") << QStringLiteral("-hide_banner") << QStringLiteral("-loglevel")
         << QStringLiteral("error") << QStringLiteral("-nostats");

    const int fps = request.fps > 0 ? request.fps : 30;
    args << QStringLiteral("-f") << QStringLiteral("gdigrab") << QStringLiteral("-framerate")
         << QString::number(fps) << QStringLiteral("-draw_mouse")
         << (request.includeCursor ? QStringLiteral("1") : QStringLiteral("0"))
         << QStringLiteral("-offset_x") << QString::number(request.region.x())
         << QStringLiteral("-offset_y") << QString::number(request.region.y())
         << QStringLiteral("-video_size")
         << QStringLiteral("%1x%2").arg(request.region.width()).arg(request.region.height())
         << QStringLiteral("-i") << QStringLiteral("desktop");

    const bool hasWebcam = !webcamSegmentPath.isEmpty();
    if (hasWebcam) {
        // The camera runs on its own clock, and the same process is encoding
        // three streams; a deep buffer keeps a busy desktop from making dshow
        // drop frames out of the track.
        args << QStringLiteral("-f") << QStringLiteral("dshow") << QStringLiteral("-rtbufsize")
             << QStringLiteral("256M") << QStringLiteral("-framerate") << QString::number(fps)
             << QStringLiteral("-i")
             << QStringLiteral("video=%1").arg(request.webcamDevice);
    }

    args << audioArguments;
    // The camera takes input 1, so every audio input moves up by one.
    const int audioStart = hasWebcam ? 2 : 1;

    QString scale = QStringLiteral("scale=trunc(iw/2)*2:trunc(ih/2)*2");
    if (request.maxWidth > 0 && request.maxHeight > 0) {
        scale = QStringLiteral(
                    "scale=w='min(iw,%1)':h='min(ih,%2)':force_original_aspect_ratio=decrease,"
                    "scale=trunc(iw/2)*2:trunc(ih/2)*2")
                    .arg(request.maxWidth)
                    .arg(request.maxHeight);
    }
    args << QStringLiteral("-map") << QStringLiteral("0:v") << QStringLiteral("-vf") << scale;

    const int crfBase = request.crf > 0 ? request.crf : 20;
    // Same resolution compensation as the Linux encoder: smaller outputs get a
    // lower CRF so they do not turn to mush. The source size is the capture
    // region; the maximum is already enforced by the scale filter above.
    const QSize outputSize =
        fitWithinMaxResolution(QSize(request.region.width(), request.region.height()),
                               QSize(request.maxWidth, request.maxHeight));
    const int reduction =
        !outputSize.isEmpty()
            ? recordingCrfResolutionReduction(outputSize.width(), outputSize.height())
            : 0;
    const int crf = std::max(1, crfBase - reduction);
    args << QStringLiteral("-c:v") << QStringLiteral("libx264") << QStringLiteral("-preset")
         << QStringLiteral("veryfast") << QStringLiteral("-profile:v") << QStringLiteral("high")
         << QStringLiteral("-crf") << QString::number(crf) << QStringLiteral("-pix_fmt")
         << QStringLiteral("yuv420p") << QStringLiteral("-g") << QString::number(fps * 2)
         << QStringLiteral("-bf") << QStringLiteral("2") << QStringLiteral("-r")
         << QString::number(fps);

    const int audioInputCount = audioArguments.count(QStringLiteral("-i"));
    if (audioInputCount > 0) {
        if (audioInputCount > 1) {
            args << QStringLiteral("-filter_complex")
                 << QStringLiteral("[%1:a][%2:a]amix=inputs=2:duration=longest[aout]")
                        .arg(audioStart)
                        .arg(audioStart + 1)
                 << QStringLiteral("-map") << QStringLiteral("[aout]");
        } else {
            args << QStringLiteral("-map") << QStringLiteral("%1:a").arg(audioStart);
        }
        args << QStringLiteral("-c:a") << QStringLiteral("aac") << QStringLiteral("-b:a")
             << QStringLiteral("160k") << QStringLiteral("-ac")
             << (request.monoAudio ? QStringLiteral("1") : QStringLiteral("2"));
    }

    args << QStringLiteral("-movflags") << QStringLiteral("+faststart") << segmentPath;

    if (hasWebcam) {
        // The track file is the camera alone: the editor places it over the
        // recording, so nothing about the desktop may appear in it. 1080p is
        // more than any bubble size can show, and the cap keeps the extra
        // encoder cheap.
        const QString trackScale =
            QStringLiteral("scale=w='min(iw,1280)':h='min(ih,960)'"
                           ":force_original_aspect_ratio=decrease,"
                           "scale=trunc(iw/2)*2:trunc(ih/2)*2");
        args << QStringLiteral("-map") << QStringLiteral("1:v") << QStringLiteral("-an")
             << QStringLiteral("-vf") << trackScale << QStringLiteral("-c:v")
             << QStringLiteral("libx264") << QStringLiteral("-preset")
             << QStringLiteral("veryfast") << QStringLiteral("-crf") << QStringLiteral("23")
             << QStringLiteral("-pix_fmt") << QStringLiteral("yuv420p") << QStringLiteral("-g")
             << QString::number(fps * 2) << QStringLiteral("-r") << QString::number(fps)
             << webcamSegmentPath;

        // The preview pipe is fixed at the bubble's shape, so the window can
        // parse the stream without knowing what the camera negotiated.
        const QSize preview = webcamPreviewSize.isEmpty() ? QSize(480, 270)
                                                          : webcamPreviewSize;
        args << QStringLiteral("-map") << QStringLiteral("1:v") << QStringLiteral("-an")
             << QStringLiteral("-vf")
             << QStringLiteral(
                    "scale=%1:%2:force_original_aspect_ratio=increase,crop=%1:%2,format=bgra")
                    .arg(preview.width())
                    .arg(preview.height())
             << QStringLiteral("-f") << QStringLiteral("rawvideo")
             << QStringLiteral("pipe:1");
    }

    return args;
}

bool RecordingSession::Private::startSegment(QString& error)
{
    const QString path =
        QDir(workDir).filePath(QStringLiteral("segment-%1.mp4").arg(segmentIndex));
    const QString webcamPath =
        webcamActive
            ? QDir(workDir).filePath(QStringLiteral("segment-webcam-%1.mp4").arg(segmentIndex))
            : QString();
    processOutput.clear();
    webcamPreviewBuffer.clear();
    process = new QProcess(q);
    process->setProgram(ffmpegExecutable());
    process->setArguments(buildArguments(path, webcamPath));
    // The preview frames own stdout, so stderr has to stay on its own channel
    // for the failure messages the recorder reports.
    process->setProcessChannelMode(QProcess::SeparateChannels);

    QObject::connect(process, &QProcess::readyReadStandardError, q, [this] {
        processOutput.append(process ? process->readAllStandardError() : QByteArray());
    });
    QObject::connect(process, &QProcess::readyReadStandardOutput, q, [this] {
        consumeWebcamPreview(process ? process->readAllStandardOutput() : QByteArray());
    });
    QObject::connect(process,
                     QOverload<int, QProcess::ExitStatus>::of(&QProcess::finished),
                     q,
                     [this](int code, QProcess::ExitStatus status) {
                         onProcessFinished(code, status);
                     });
    QObject::connect(process, &QProcess::errorOccurred, q, [this](QProcess::ProcessError error) {
        onProcessError(error);
    });

    process->start();
    if (!process->waitForStarted(kProbeTimeoutMs)) {
        error = QStringLiteral("ffmpeg could not start recording: %1")
                    .arg(process->errorString());
        delete process;
        process = nullptr;
        return false;
    }

    segments.append(path);
    if (!webcamPath.isEmpty())
        webcamSegments.append(webcamPath);
    ++segmentIndex;
    return true;
}

void RecordingSession::Private::consumeWebcamPreview(const QByteArray& chunk)
{
    if (chunk.isEmpty() || webcamPreviewSize.isEmpty())
        return;

    webcamPreviewBuffer.append(chunk);
    const qsizetype frameBytes =
        qsizetype(webcamPreviewSize.width()) * webcamPreviewSize.height() * 4;
    if (frameBytes <= 0) {
        webcamPreviewBuffer.clear();
        return;
    }
    // A live bubble only ever needs the newest frame; a backlog would make it
    // lag the recording instead of showing it.
    if (webcamPreviewBuffer.size() > frameBytes * 2) {
        const qsizetype keep = webcamPreviewBuffer.size() - (webcamPreviewBuffer.size() % frameBytes);
        webcamPreviewBuffer = webcamPreviewBuffer.right(qsizetype(keep));
    }
    while (webcamPreviewBuffer.size() >= frameBytes) {
        const QImage frame(reinterpret_cast<const uchar*>(webcamPreviewBuffer.constData()),
                           webcamPreviewSize.width(),
                           webcamPreviewSize.height(),
                           webcamPreviewSize.width() * 4,
                           QImage::Format_ARGB32);
        emit q->webcamFrame(frame.copy());
        webcamPreviewBuffer.remove(0, frameBytes);
    }
}

void RecordingSession::Private::onProcessError(QProcess::ProcessError error)
{
    if (error != QProcess::FailedToStart || !process)
        return;
    processOutput.append(process->readAllStandardError());
    fail(QStringLiteral("ffmpeg could not start: %1").arg(process->errorString()));
}

void RecordingSession::Private::onProcessFinished(int exitCode, QProcess::ExitStatus status)
{
    if (process) {
        processOutput.append(process->readAllStandardError());
        consumeWebcamPreview(process->readAllStandardOutput());
        process->deleteLater();
        process = nullptr;
    }

    const QString detail = captureOutput();

    if (state == State::Pausing) {
        if (status != QProcess::NormalExit || exitCode != 0) {
            fail(detail.isEmpty()
                     ? QStringLiteral("ffmpeg stopped while pausing (code %1)").arg(exitCode)
                     : QStringLiteral("ffmpeg stopped while pausing: %1").arg(detail));
            return;
        }
        state = State::Paused;
        emit q->stateChanged();
        emit q->elapsedChanged(elapsedMs());
        if (resumeAfterPause) {
            resumeAfterPause = false;
            q->resume();
        }
        return;
    }

    if (state == State::Stopping) {
        if (discardRequested) {
            cleanupAndEmit(false, QString(), QString());
            return;
        }
        if (status != QProcess::NormalExit || exitCode != 0) {
            fail(detail.isEmpty()
                     ? QStringLiteral("ffmpeg stopped before the recording was saved (code %1)")
                           .arg(exitCode)
                     : QStringLiteral("ffmpeg stopped before the recording was saved: %1")
                           .arg(detail));
            return;
        }
        finalize();
        return;
    }

    if (state == State::Recording) {
        fail(detail.isEmpty() ? QStringLiteral("ffmpeg stopped unexpectedly")
                              : QStringLiteral("ffmpeg stopped unexpectedly: %1").arg(detail));
    }
}

void RecordingSession::Private::requestQuit(int timeoutMs)
{
    QProcess* target = process;
    if (!target)
        return;
    target->write("q\n");
    target->closeWriteChannel();
    // A pause can be followed by a resume before the timeout, so only the
    // process that received `q` may be killed here.
    QTimer::singleShot(timeoutMs, q, [this, target] {
        if (process && process == target) {
            process->kill();
        }
    });
}

void RecordingSession::Private::fail(const QString& reason)
{
    tick->stop();
    pointerTracker.discard();
    state = State::Failed;
    qWarning().noquote() << "[recording]" << reason;
    if (process) {
        process->kill();
        process->deleteLater();
        process = nullptr;
    }
    // Keep the segments so a failed save can still be recovered from the work
    // directory the message names.
    const QString message = workDir.isEmpty()
        ? reason
        : QStringLiteral("%1 The captured segments are in %2.")
              .arg(reason, QDir::toNativeSeparators(workDir));
    cleanupAndEmit(false, QString(), message, true);
}

void RecordingSession::Private::completeWebcamTrack(const QString& videoPath)
{
    const auto finish = [this, videoPath](bool joined, const QString& detail) {
        if (!joined && !detail.isEmpty()) {
            qWarning().noquote() << "[recording] webcam track was not written:" << detail;
            emit q->warning(
                QStringLiteral("The webcam track could not be written: %1").arg(detail));
        }
        if (!joined) {
            webcamTrack = Platform::WebcamTrack{};
            for (const QString& path : webcamSegments)
                QFile::remove(path);
            webcamSegments.clear();
        }
        cleanupWorkDir();
        cleanupAndEmit(true, videoPath, QString());
    };

    if (!webcamActive || webcamSegments.isEmpty()) {
        webcamTrack = Platform::WebcamTrack{};
        finish(true, QString());
        return;
    }

    const QString trackPath = Platform::webcamTrackPathFor(videoPath);
    if (trackPath.isEmpty()) {
        finish(false, QStringLiteral("the camera track had no place to go"));
        return;
    }
    QDir().mkpath(QFileInfo(trackPath).absolutePath());
    QFile::remove(trackPath);

    const auto settle = [this, trackPath, finish](bool written, const QString& detail) {
        if (!written) {
            QFile::remove(trackPath);
            finish(false, detail);
            return;
        }
        const QSize size = videoFrameSize(trackPath).isEmpty() ? webcamCameraSize
                                                              : videoFrameSize(trackPath);
        webcamTrack.file = QFileInfo(trackPath).fileName();
        webcamTrack.width = size.width();
        webcamTrack.height = size.height();
        webcamTrack.duration = videoDuration(trackPath);
        if (!webcamTrack.is_valid()) {
            QFile::remove(trackPath);
            finish(false, QStringLiteral("the camera track had no readable frames"));
            return;
        }
        finish(true, QString());
    };

    if (webcamSegments.size() == 1) {
        const QString segment = webcamSegments.first();
        if (!QFile::rename(segment, trackPath) && !QFile::copy(segment, trackPath)) {
            settle(false, QStringLiteral("could not move the camera segment"));
            return;
        }
        settle(true, QString());
        return;
    }

    QFile list(QDir(workDir).filePath(QStringLiteral("webcam-segments.txt")));
    if (!list.open(QIODevice::WriteOnly | QIODevice::Truncate)) {
        settle(false, QStringLiteral("could not prepare the camera segment list"));
        return;
    }
    for (const QString& segment : webcamSegments)
        list.write(quoteConcatPath(segment).toUtf8() + '\n');
    list.close();

    auto* joiner = new QProcess(q);
    joiner->setProgram(ffmpegExecutable());
    joiner->setProcessChannelMode(QProcess::SeparateChannels);
    joiner->setArguments({
        QStringLiteral("-y"),
        QStringLiteral("-hide_banner"),
        QStringLiteral("-loglevel"),
        QStringLiteral("error"),
        QStringLiteral("-f"),
        QStringLiteral("concat"),
        QStringLiteral("-safe"),
        QStringLiteral("0"),
        QStringLiteral("-i"),
        list.fileName(),
        QStringLiteral("-an"),
        QStringLiteral("-c"),
        QStringLiteral("copy"),
        trackPath,
    });
    QObject::connect(joiner,
                     QOverload<int, QProcess::ExitStatus>::of(&QProcess::finished),
                     q,
                     [joiner, settle](int code, QProcess::ExitStatus status) {
                         const QString detail =
                             tailLines(QString::fromUtf8(joiner->readAllStandardError()), 6);
                         joiner->deleteLater();
                         if (status == QProcess::NormalExit && code == 0)
                             settle(true, QString());
                         else
                             settle(false,
                                    detail.isEmpty()
                                        ? QStringLiteral("the camera segments could not be joined")
                                        : detail);
                     });
    joiner->start();
}

void RecordingSession::Private::cleanupWorkDir()
{
    if (workDir.isEmpty())
        return;
    QDir(workDir).removeRecursively();
    workDir.clear();
}

void RecordingSession::Private::removeDiscardedSidecar()
{
    QString path = existingSidecarPath;
    if (path.isEmpty())
        path = sidecarPathFor(request.outputPath);
    if (path.isEmpty())
        return;
    QFile::remove(path);
    QFile::remove(path + QStringLiteral(".webcam.mp4"));
}

void RecordingSession::Private::cleanupAndEmit(bool kept,
                                               const QString& path,
                                               const QString& failure,
                                               bool keepWorkDir)
{
    if (finishedEmitted)
        return;
    finishedEmitted = true;
    tick->stop();
    if (kept && !path.isEmpty() && QFileInfo(path).isFile()) {
        pointerTracker.stop(elapsedMs());
        QString sidecarError;
        if (!pointerTracker.writeSidecar(path, webcamTrack, &sidecarError))
            qWarning().noquote() << "[recording] could not write pointer sidecar:" << sidecarError;
    } else if (!kept && discardRequested) {
        pointerTracker.discard();
        removeDiscardedSidecar();
    }
    if (!kept && !keepWorkDir)
        cleanupWorkDir();
    emit q->finished(kept, kept ? path : QString(), failure);
    emit q->stateChanged();
}

void RecordingSession::Private::finalize()
{
    tick->stop();

    if (segments.isEmpty()) {
        cleanupAndEmit(false, QString(), QStringLiteral("No video frames were captured."));
        return;
    }

    const QString finalPath = request.outputPath;
    QDir().mkpath(QFileInfo(finalPath).absolutePath());
    QFile::remove(finalPath);

    if (segments.size() == 1) {
        const QString segment = segments.first();
        if (!QFile::rename(segment, finalPath)) {
            if (!QFile::copy(segment, finalPath)) {
                cleanupAndEmit(false,
                               QString(),
                               QStringLiteral("Could not move the recording to %1. "
                                              "The captured segment is in %2.")
                                   .arg(QDir::toNativeSeparators(finalPath),
                                        QDir::toNativeSeparators(workDir)),
                               true);
                return;
            }
            QFile::remove(segment);
        }
        completeWebcamTrack(finalPath);
        return;
    }

    QFile list(concatListPath(workDir));
    if (!list.open(QIODevice::WriteOnly | QIODevice::Truncate)) {
        cleanupAndEmit(false,
                       QString(),
                       QStringLiteral("Could not prepare the segment list in %1.")
                           .arg(QDir::toNativeSeparators(workDir)),
                       true);
        return;
    }
    for (const QString& segment : segments)
        list.write(quoteConcatPath(segment).toUtf8() + '\n');
    list.close();

    concat = new QProcess(q);
    concat->setProgram(ffmpegExecutable());
    concat->setProcessChannelMode(QProcess::MergedChannels);
    concat->setArguments({
        QStringLiteral("-y"),
        QStringLiteral("-hide_banner"),
        QStringLiteral("-loglevel"),
        QStringLiteral("error"),
        QStringLiteral("-f"),
        QStringLiteral("concat"),
        QStringLiteral("-safe"),
        QStringLiteral("0"),
        QStringLiteral("-i"),
        concatListPath(workDir),
        QStringLiteral("-c"),
        QStringLiteral("copy"),
        QStringLiteral("-movflags"),
        QStringLiteral("+faststart"),
        finalPath,
    });

    QObject::connect(concat,
                     QOverload<int, QProcess::ExitStatus>::of(&QProcess::finished),
                     q,
                     [this, finalPath](int code, QProcess::ExitStatus status) {
                         const QString detail =
                             tailLines(QString::fromUtf8(concat->readAll()), 8);
                         concat->deleteLater();
                         concat = nullptr;
                         if (status == QProcess::NormalExit && code == 0
                             && QFileInfo(finalPath).size() > 0) {
                             completeWebcamTrack(finalPath);
                         } else {
                             cleanupAndEmit(
                                 false,
                                 QString(),
                                 detail.isEmpty()
                                     ? QStringLiteral("Could not join the recording segments. "
                                                      "They are in %1.")
                                           .arg(QDir::toNativeSeparators(workDir))
                                     : QStringLiteral("Could not join the recording segments: %1. "
                                                      "They are in %2.")
                                           .arg(detail,
                                                QDir::toNativeSeparators(workDir)),
                                 true);
                         }
                     });
    concat->start();
    if (!concat->waitForStarted(kProbeTimeoutMs)) {
        const QString detail = concat->errorString();
        concat->deleteLater();
        concat = nullptr;
        cleanupAndEmit(false,
                       QString(),
                       QStringLiteral("Could not start the segment join: %1. "
                                      "The segments are in %2.")
                           .arg(detail, QDir::toNativeSeparators(workDir)),
                       true);
    }
}

RecordingSession::RecordingSession(QObject* parent)
    : QObject(parent)
    , d(new Private(this))
{
}

RecordingSession::~RecordingSession()
{
    if (d->process) {
        d->process->kill();
        d->process->waitForFinished(1000);
    }
    if (d->concat) {
        d->concat->kill();
        d->concat->waitForFinished(1000);
    }
    d->cleanupWorkDir();
    delete d;
}

bool RecordingSession::available(QString* error)
{
    static bool probed = false;
    static bool usable = false;
    static QString probeError;

    if (!probed) {
        probed = true;
        QString output;
        QString launchError;
        if (!runProbe({QStringLiteral("-hide_banner"),
                       QStringLiteral("-devices")},
                      &output,
                      &launchError)) {
            usable = false;
            probeError = launchError;
        } else if (!output.contains(QStringLiteral("gdigrab"))) {
            usable = false;
            probeError = QStringLiteral(
                "This ffmpeg build has no gdigrab device, so screen recording is unavailable.");
        } else {
            usable = true;
        }
    }

    if (!usable && error)
        *error = probeError;
    return usable;
}

bool RecordingSession::start(const RecordingRequest& request, QString& error)
{
    if (d->state != Private::State::Idle && d->state != Private::State::Failed) {
        error = QStringLiteral("A recording is already running.");
        return false;
    }

    QString probeError;
    if (!available(&probeError)) {
        error = probeError;
        return false;
    }

    if (request.target != RecordingRequest::Target::Window && request.region.isEmpty()) {
        error = QStringLiteral("The recording area is empty.");
        return false;
    }

    d->request = request;
    if (d->request.outputPath.isEmpty())
        d->request.outputPath = defaultOutputPath();
    if (d->request.outputPath.isEmpty()) {
        error = QStringLiteral("No output path was selected.");
        return false;
    }
    d->existingSidecarPath = QFileInfo::exists(d->request.outputPath)
        ? sidecarPathFor(d->request.outputPath)
        : QString();
    if (d->request.fps <= 0)
        d->request.fps = 30;
    if (d->request.crf <= 0)
        d->request.crf = 20;
    QDir().mkpath(QFileInfo(d->request.outputPath).absolutePath());

    d->workDir = QDir(QDir::tempPath())
                     .filePath(QStringLiteral("apexshot-recording-%1-%2")
                                   .arg(QCoreApplication::applicationPid())
                                   .arg(QDateTime::currentMSecsSinceEpoch()));
    if (!QDir().mkpath(d->workDir)) {
        error = QStringLiteral("Could not create the recording work directory.");
        return false;
    }

    QStringList warnings;
    d->audioArguments = d->buildAudioArguments(warnings);

    // A camera that cannot be opened must not take the recording down with it:
    // probe it first and record the desktop alone when it does not answer.
    d->webcamActive = false;
    d->webcamCameraSize = QSize();
    d->webcamPreviewSize = QSize();
    d->webcamPreviewBuffer.clear();
    d->webcamSegments.clear();
    d->webcamTrack = Platform::WebcamTrack{};
    QString camera;
    if (d->request.webcamEnabled) {
        // A camera that was unplugged or renamed since the setting was written
        // falls back to the machine's only camera rather than losing the track.
        const QStringList cameras = cameraDevices();
        camera = d->request.webcamDevice.trimmed();
        if (camera.isEmpty()) {
            camera = cameras.value(0);
            d->request.webcamDevice = camera;
        } else if (!cameras.isEmpty() && !cameras.contains(camera, Qt::CaseInsensitive)) {
            warnings.append(QStringLiteral("The camera \"%1\" is not connected; using \"%2\".")
                                .arg(camera, cameras.first()));
            camera = cameras.first();
            d->request.webcamDevice = camera;
        }
    }
    if (!camera.isEmpty()) {
        const QSize cameraSize = cameraFrameSize(camera, d->request.fps);
        if (cameraSize.isEmpty()) {
            warnings.append(QStringLiteral("The camera \"%1\" could not be opened; recording "
                                           "without a webcam track.")
                                .arg(camera));
            d->request.webcamDevice.clear();
        } else {
            d->webcamActive = true;
            d->webcamCameraSize = cameraSize;
            d->webcamPreviewSize = webcamPreviewFrameSize(d->request.webcamLayout.shape);
            d->webcamTrack.x = std::clamp(d->request.webcamLayout.x, 0.0, 1.0);
            d->webcamTrack.y = std::clamp(d->request.webcamLayout.y, 0.0, 1.0);
            d->webcamTrack.w = std::clamp(d->request.webcamLayout.w, 0.0, 1.0);
            d->webcamTrack.h = std::clamp(d->request.webcamLayout.h, 0.0, 1.0);
            d->webcamTrack.shape = std::clamp(d->request.webcamLayout.shape, 0, 3);
            d->webcamTrack.flip = d->request.webcamLayout.flip;
        }
    }

    for (const QString& warning : warnings)
        qWarning().noquote() << "[recording]" << warning;
    if (!warnings.isEmpty())
        emit warning(warnings.join(QLatin1Char('\n')));

    d->segments.clear();
    d->segmentIndex = 0;
    d->elapsedBeforeMs = 0;
    d->processOutput.clear();
    d->discardRequested = false;
    d->resumeAfterPause = false;
    d->finishedEmitted = false;

    if (!d->startSegment(error)) {
        d->cleanupWorkDir();
        return false;
    }

    d->state = Private::State::Recording;
    d->active.start();
    d->pointerTracker.start(d->request.region,
                            d->request.fps,
                            [d = d] { return d->elapsedMs(); });
    d->tick->start();
    emit stateChanged();
    emit elapsedChanged(0);
    return true;
}

void RecordingSession::pause()
{
    if (d->state != Private::State::Recording)
        return;

    d->elapsedBeforeMs += d->active.elapsed();
    d->pointerTracker.pause(d->elapsedBeforeMs);
    d->tick->stop();
    d->state = Private::State::Pausing;
    emit stateChanged();
    emit elapsedChanged(d->elapsedBeforeMs);

    if (d->process)
        d->requestQuit(kStopTimeoutMs);
    else {
        d->state = Private::State::Paused;
        emit stateChanged();
    }
}

void RecordingSession::resume()
{
    if (d->state == Private::State::Pausing) {
        // The segment is still finalising; resume as soon as it is safe.
        d->resumeAfterPause = true;
        return;
    }
    if (d->state != Private::State::Paused)
        return;

    QString error;
    if (!d->startSegment(error)) {
        d->fail(error);
        return;
    }

    d->state = Private::State::Recording;
    d->active.start();
    d->pointerTracker.resume();
    d->tick->start();
    emit stateChanged();
    emit elapsedChanged(d->elapsedBeforeMs);
}

void RecordingSession::stop()
{
    if (d->state == Private::State::Idle || d->state == Private::State::Failed)
        return;
    if (d->state == Private::State::Stopping)
        return;

    if (d->state == Private::State::Recording)
        d->elapsedBeforeMs += d->active.elapsed();
    d->pointerTracker.stop(d->elapsedBeforeMs);
    d->tick->stop();
    d->discardRequested = false;
    d->resumeAfterPause = false;
    d->state = Private::State::Stopping;
    emit stateChanged();
    emit elapsedChanged(d->elapsedBeforeMs);

    if (d->process)
        d->requestQuit(kStopTimeoutMs);
    else
        d->finalize();
}

void RecordingSession::discard()
{
    if (d->state == Private::State::Idle || d->state == Private::State::Failed) {
        d->cleanupWorkDir();
        return;
    }
    if (d->state == Private::State::Stopping)
        return;

    if (d->state == Private::State::Recording)
        d->elapsedBeforeMs += d->active.elapsed();
    d->pointerTracker.discard();
    d->tick->stop();
    d->discardRequested = true;
    d->resumeAfterPause = false;
    d->state = Private::State::Stopping;
    emit stateChanged();

    if (d->process)
        d->requestQuit(kDiscardTimeoutMs);
    else
        d->cleanupAndEmit(false, QString(), QString());
}

bool RecordingSession::isActive() const
{
    return d->state == Private::State::Recording || d->state == Private::State::Pausing
        || d->state == Private::State::Paused || d->state == Private::State::Stopping;
}

bool RecordingSession::isPaused() const
{
    return d->state == Private::State::Paused;
}

QString RecordingSession::outputPath() const
{
    return d->request.outputPath;
}

} // namespace Platform
