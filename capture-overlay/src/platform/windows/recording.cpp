#include "platform/recording.h"

#include "platform/recording_settings.h"

#include <QCoreApplication>
#include <QDateTime>
#include <QDebug>
#include <QDir>
#include <QElapsedTimer>
#include <QFile>
#include <QFileInfo>
#include <QProcess>
#include <QRegularExpression>
#include <QStandardPaths>
#include <QTimer>

#include <algorithm>

namespace {

constexpr int kProbeTimeoutMs = 5000;
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
    QString directory = QStandardPaths::writableLocation(QStandardPaths::MoviesLocation);
    if (directory.isEmpty())
        directory = QDir::home().filePath(QStringLiteral("Videos"));
    directory = QDir(directory).filePath(QStringLiteral("ApexShotRec"));
    QDir().mkpath(directory);

    const QString name = QStringLiteral("ApexShot Recording %1.mp4")
                             .arg(QDateTime::currentDateTime().toString(
                                 QStringLiteral("yyyy-MM-dd HH-mm-ss")));
    return QDir(directory).filePath(name);
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
    void cleanupAndEmit(bool kept,
                        const QString& path,
                        const QString& failure,
                        bool keepWorkDir = false);
    void fail(const QString& reason);
    void cleanupWorkDir();
    [[nodiscard]] qint64 elapsedMs() const;
    [[nodiscard]] QStringList buildArguments(const QString& segmentPath) const;
    [[nodiscard]] QStringList buildAudioArguments(QStringList& warnings) const;
    [[nodiscard]] QString captureOutput() const;

    RecordingSession* q = nullptr;
    State state = State::Idle;
    RecordingRequest request;
    QProcess* process = nullptr;
    QProcess* concat = nullptr;
    QTimer* tick = nullptr;
    QElapsedTimer active;
    qint64 elapsedBeforeMs = 0;
    QString workDir;
    QStringList segments;
    int segmentIndex = 0;
    bool discardRequested = false;
    bool finishedEmitted = false;
    /// Resume was requested while the previous segment was still finalising.
    bool resumeAfterPause = false;
    QStringList audioArguments;
    QByteArray processOutput;
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

QStringList RecordingSession::Private::buildArguments(const QString& segmentPath) const
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

    args << audioArguments;

    QString scale = QStringLiteral("scale=trunc(iw/2)*2:trunc(ih/2)*2");
    if (request.maxWidth > 0 && request.maxHeight > 0) {
        scale = QStringLiteral(
                    "scale=w='min(iw,%1)':h='min(ih,%2)':force_original_aspect_ratio=decrease,"
                    "scale=trunc(iw/2)*2:trunc(ih/2)*2")
                    .arg(request.maxWidth)
                    .arg(request.maxHeight);
    }
    args << QStringLiteral("-vf") << scale;

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

    const bool hasAudio = audioArguments.contains(QStringLiteral("-i"));
    if (hasAudio) {
        const int audioInputCount = audioArguments.count(QStringLiteral("-i"));
        if (audioInputCount > 1) {
            args << QStringLiteral("-filter_complex")
                 << QStringLiteral("[1:a][2:a]amix=inputs=2:duration=longest[aout]")
                 << QStringLiteral("-map") << QStringLiteral("0:v") << QStringLiteral("-map")
                 << QStringLiteral("[aout]");
        } else {
            args << QStringLiteral("-map") << QStringLiteral("0:v") << QStringLiteral("-map")
                 << QStringLiteral("1:a");
        }
        args << QStringLiteral("-c:a") << QStringLiteral("aac") << QStringLiteral("-b:a")
             << QStringLiteral("160k") << QStringLiteral("-ac")
             << (request.monoAudio ? QStringLiteral("1") : QStringLiteral("2"));
    }

    args << QStringLiteral("-movflags") << QStringLiteral("+faststart") << segmentPath;
    return args;
}

bool RecordingSession::Private::startSegment(QString& error)
{
    const QString path =
        QDir(workDir).filePath(QStringLiteral("segment-%1.mp4").arg(segmentIndex));
    processOutput.clear();
    process = new QProcess(q);
    process->setProgram(ffmpegExecutable());
    process->setArguments(buildArguments(path));
    process->setProcessChannelMode(QProcess::MergedChannels);

    QObject::connect(process, &QProcess::readyRead, q, [this] {
        processOutput.append(process ? process->readAll() : QByteArray());
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
    ++segmentIndex;
    return true;
}

void RecordingSession::Private::onProcessError(QProcess::ProcessError error)
{
    if (error != QProcess::FailedToStart || !process)
        return;
    processOutput.append(process->readAll());
    fail(QStringLiteral("ffmpeg could not start: %1").arg(process->errorString()));
}

void RecordingSession::Private::onProcessFinished(int exitCode, QProcess::ExitStatus status)
{
    if (process) {
        processOutput.append(process->readAll());
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

void RecordingSession::Private::cleanupWorkDir()
{
    if (workDir.isEmpty())
        return;
    QDir(workDir).removeRecursively();
    workDir.clear();
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
        cleanupWorkDir();
        cleanupAndEmit(true, finalPath, QString());
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
                             cleanupWorkDir();
                             cleanupAndEmit(true, finalPath, QString());
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
