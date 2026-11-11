#include "platform/recording.h"

#include "platform/recording_settings.h"
#include "platform/windows/audio_devices.h"
#include "platform/windows/ddagrab.h"
#include "platform/windows/disk_space.h"
#include "platform/windows/pointer_tracker.h"
#include "platform/windows/recording_manifest.h"
#include "platform/windows/settings_store.h"
#include "platform/windows/webcam_format.h"
#include "platform/windows/window_capture.h"
#include "platform/windows/win_util.h"

#include <QCoreApplication>
#include <QDateTime>
#include <QDebug>
#include <QDir>
#include <QElapsedTimer>
#include <QFile>
#include <QFileInfo>
#include <QHash>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QMutex>
#include <QMutexLocker>
#include <QProcess>
#include <QRegularExpression>
#include <QSaveFile>
#include <QStandardPaths>
#include <QThread>
#include <QTimer>
#include <QUuid>
#include <QVector>

#include <algorithm>
#include <functional>
#include <memory>

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

/// Diagnostics for the recording pipeline, which has no console of its own.
void recordingLog(const QString& text)
{
    static QMutex mutex;
    QMutexLocker lock(&mutex);
    QFile file(QDir(QDir::tempPath()).filePath(QStringLiteral("apexshot-recording.log")));
    if (file.open(QIODevice::Append | QIODevice::WriteOnly)) {
        QTextStream out(&file);
        out.setCodec("UTF-8");
        out << QDateTime::currentDateTime().toString(Qt::ISODate) << " " << text << Qt::endl;
    }
}

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

void appendHardwareEncoder(QStringList& args, const QString& hardware, int quality)
{
    if (hardware.endsWith(QLatin1String("_qsv"))) {
        args << QStringLiteral("-c:v") << hardware << QStringLiteral("-global_quality")
             << QString::number(quality) << QStringLiteral("-preset")
             << QStringLiteral("veryfast") << QStringLiteral("-pix_fmt")
             << QStringLiteral("nv12");
    } else if (hardware.endsWith(QLatin1String("_amf"))) {
        args << QStringLiteral("-c:v") << hardware << QStringLiteral("-rc")
             << QStringLiteral("cqp") << QStringLiteral("-qp_i") << QString::number(quality)
             << QStringLiteral("-qp_p") << QString::number(quality) << QStringLiteral("-qp_b")
             << QString::number(quality) << QStringLiteral("-quality")
             << QStringLiteral("speed") << QStringLiteral("-pix_fmt")
             << QStringLiteral("nv12");
    } else {
        args << QStringLiteral("-c:v") << hardware << QStringLiteral("-rc")
             << QStringLiteral("vbr") << QStringLiteral("-cq") << QString::number(quality)
             << QStringLiteral("-b:v") << QStringLiteral("0")
             << QStringLiteral("-preset") << QStringLiteral("p5")
             << QStringLiteral("-pix_fmt") << QStringLiteral("yuv420p");
    }
    // The hvc1 tag is what lets an HEVC MP4 play in the system player and in
    // browsers; the default hev1 tag does not.
    if (hardware.startsWith(QLatin1String("hevc_")))
        args << QStringLiteral("-tag:v") << QStringLiteral("hvc1");
}

/// A hardware encoder of the requested codec this machine can open, or an
/// empty string for the software encoder. Encoding the desktop and the camera
/// in software keeps two cores busy for the whole recording, which is enough
/// to make everything else on a laptop stutter; the integrated GPU encodes the
/// same picture for a fraction of that.
QString hardwareVideoEncoder(const QSize& size, int fps, bool hevc = false)
{
    static QHash<QString, QString> encoders;
    const QString key = QStringLiteral("%1x%2@%3%4")
                            .arg(size.width())
                            .arg(size.height())
                            .arg(fps)
                            .arg(hevc ? QStringLiteral(" hevc") : QString());
    if (encoders.contains(key))
        return encoders.value(key);

    const QString encoder = [&] {
        for (const QString& candidate :
             hevc ? QStringList{QStringLiteral("hevc_qsv"), QStringLiteral("hevc_nvenc"),
                                QStringLiteral("hevc_amf")}
                  : QStringList{QStringLiteral("h264_qsv"), QStringLiteral("h264_nvenc"),
                                QStringLiteral("h264_amf")}) {
            QString output;
            QString error;
            QStringList args{QStringLiteral("-hide_banner"), QStringLiteral("-loglevel"),
                             QStringLiteral("error"), QStringLiteral("-f"),
                             QStringLiteral("lavfi"), QStringLiteral("-i"),
                             QStringLiteral("color=size=%1x%2:rate=%3")
                                 .arg(size.width()).arg(size.height()).arg(fps),
                             QStringLiteral("-frames:v"), QStringLiteral("3")};
            appendHardwareEncoder(args, candidate, 20);
            args << QStringLiteral("-bf") << QStringLiteral("2")
                 << QStringLiteral("-f") << QStringLiteral("null") << QStringLiteral("-");
            if (runProbe(args, &output, &error)) {
                recordingLog(QStringLiteral("encoder: %1 for %2").arg(candidate, key));
                return candidate;
            }
        }
        recordingLog(QStringLiteral("encoder: %1 for %2 (no hardware encoder answered)")
                         .arg(hevc ? QStringLiteral("libx265") : QStringLiteral("libx264"), key));
        return QString();
    }();
    encoders.insert(key, encoder);
    return encoder;
}

/// Append a video encoder chosen for this machine. `quality` is the H.264
/// CRF; HEVC adds kHevcQualityOffset, which lands at the same quality with a
/// smaller file. A hardware encoder takes it as its constant-quality value.
void appendVideoEncoder(QStringList& args,
                        int quality,
                        const QSize& size,
                        int fps,
                        bool hevc = false)
{
    const int codecQuality = hevc ? quality + Platform::kHevcQualityOffset : quality;
    const QString hardware = hardwareVideoEncoder(size, fps, hevc);
    if (!hardware.isEmpty()) {
        appendHardwareEncoder(args, hardware, codecQuality);
        return;
    }
    if (hevc) {
        args << QStringLiteral("-c:v") << QStringLiteral("libx265") << QStringLiteral("-preset")
             << QStringLiteral("veryfast") << QStringLiteral("-crf")
             << QString::number(codecQuality) << QStringLiteral("-pix_fmt")
             << QStringLiteral("yuv420p") << QStringLiteral("-x265-params")
             << QStringLiteral("log-level=error") << QStringLiteral("-tag:v")
             << QStringLiteral("hvc1");
        return;
    }
    args << QStringLiteral("-c:v") << QStringLiteral("libx264") << QStringLiteral("-preset")
         << QStringLiteral("veryfast") << QStringLiteral("-profile:v")
         << QStringLiteral("high") << QStringLiteral("-crf") << QString::number(quality)
         << QStringLiteral("-pix_fmt") << QStringLiteral("yuv420p");
}

/// Whether this ffmpeg can capture the selected DXGI output and download it to
/// a normal frame. Probing the real output matters when output 0 is detached
/// and the only attached display is a later index.
bool ddagrabSupported(int outputIndex)
{
    static QHash<int, bool> results;
    if (results.contains(outputIndex))
        return results.value(outputIndex);

    QString output;
    QString error;
    const QStringList args{
        QStringLiteral("-hide_banner"),
        QStringLiteral("-loglevel"),
        QStringLiteral("error"),
        QStringLiteral("-f"),
        QStringLiteral("lavfi"),
        QStringLiteral("-i"),
        QStringLiteral("ddagrab=output_idx=%1:draw_mouse=0:framerate=30:video_size=16x16")
            .arg(outputIndex),
        QStringLiteral("-frames:v"),
        QStringLiteral("1"),
        QStringLiteral("-vf"),
        QStringLiteral("hwdownload,format=bgra"),
        QStringLiteral("-f"),
        QStringLiteral("null"),
        QStringLiteral("-"),
    };
    const bool supported = runProbe(args, &output, &error);
    if (!supported) {
        recordingLog(QStringLiteral("capture: ddagrab unavailable for output %1: %2")
                         .arg(outputIndex)
                         .arg(error));
    } else {
        recordingLog(QStringLiteral("capture: ddagrab available for output %1").arg(outputIndex));
    }
    results.insert(outputIndex, supported);
    return supported;
}

QStringList audioInputs()
{
    return Platform::directShowDevices(false);
}

QString microphoneDevice()
{
    return Platform::resolveMicrophoneDevice(audioInputs(), {}, Platform::defaultMicrophoneName());
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

/// Every camera DirectShow registers. Enumeration reads the device category
/// rather than opening each camera, so it is safe to call while the setup
/// surface is up.
QStringList cameraDevices()
{
    return Platform::directShowDevices(true);
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

struct CameraProbe {
    QSize size;
    double fps = 0.0;
};

double parseFrameRate(const QString& text)
{
    bool ok = false;
    const double direct = text.trimmed().toDouble(&ok);
    if (ok)
        return direct;

    const QStringList parts = text.trimmed().split(QLatin1Char('/'));
    if (parts.size() != 2)
        return 0.0;
    bool numeratorOk = false;
    bool denominatorOk = false;
    const double numerator = parts[0].toDouble(&numeratorOk);
    const double denominator = parts[1].toDouble(&denominatorOk);
    return numeratorOk && denominatorOk && denominator > 0.0 ? numerator / denominator : 0.0;
}

/// The size and default rate a camera opens at, without pinning a rate. This
/// follows the studied recorder: the camera's active format is chosen by
/// size, and its own frame rate is reported separately instead of copying
/// the desktop's rate. Cached because opening a capture device takes about a
/// second and the recorder asks once per recording.
CameraProbe probeCamera(const QString& device, const QSize& size, const QString& pixelFormat)
{
    static QHash<QString, CameraProbe> probes;
    const QString key =
        QStringLiteral("%1|%2x%3|%4").arg(device).arg(size.width()).arg(size.height()).arg(
            pixelFormat);
    const auto cached = probes.constFind(key);
    if (cached != probes.constEnd())
        return cached.value();

    QStringList arguments{QStringLiteral("-v"), QStringLiteral("error"),
                          QStringLiteral("-f"), QStringLiteral("dshow")};
    if (!size.isEmpty()) {
        arguments << QStringLiteral("-video_size")
                  << QStringLiteral("%1x%2").arg(size.width()).arg(size.height());
    }
    if (!pixelFormat.isEmpty())
        arguments << QStringLiteral("-pixel_format") << pixelFormat;
    arguments << QStringLiteral("-i") << QStringLiteral("video=%1").arg(device)
              << QStringLiteral("-select_streams") << QStringLiteral("v:0")
              << QStringLiteral("-show_entries")
              << QStringLiteral("stream=width,height,avg_frame_rate,r_frame_rate")
              << QStringLiteral("-of") << QStringLiteral("csv=p=0");

    CameraProbe probe;
    QString output;
    if (runFfprobe(arguments, &output)) {
        probe.size = parseProbeSize(output);
        const QStringList fields =
            output.split(QRegularExpression(QStringLiteral("[\\r\\n]+")), Qt::SkipEmptyParts)
                .value(0)
                .split(QLatin1Char(','), Qt::SkipEmptyParts);
        for (int index = 2; index < fields.size(); ++index) {
            const double rate = parseFrameRate(fields[index]);
            if (rate > 0.0) {
                probe.fps = rate;
                break;
            }
        }
    }
    probes.insert(key, probe);
    return probe;
}

/// The camera mode to open at: the shared picker's best at 24fps. Raw modes
/// stay within the picker's 720p ceiling and MJPEG modes may reach 1080p,
/// the size the camera track is capped to anyway. Left alone, an integrated
/// camera opens at 1080p in uncompressed YUY2, which USB bandwidth caps at
/// five frames a second - the frozen picture a recording then carries.
Platform::WebcamCameraFormat cameraCaptureFormat(const QString& device)
{
    static QHash<QString, Platform::WebcamCameraFormat> formats;
    const auto cached = formats.constFind(device);
    if (cached != formats.constEnd())
        return cached.value();

    // Opening the camera twice in quick succession can make the second open
    // fail while the driver is still releasing the first, and `-list_options`
    // exits non-zero after printing, so retry once and parse the output.
    Platform::WebcamCameraFormat best;
    for (int attempt = 0; attempt < 2 && best.size.isEmpty(); ++attempt) {
        if (attempt > 0)
            QThread::msleep(250);
        QString output;
        runProbe({QStringLiteral("-hide_banner"), QStringLiteral("-f"),
                  QStringLiteral("dshow"), QStringLiteral("-list_options"),
                  QStringLiteral("true"), QStringLiteral("-i"),
                  QStringLiteral("video=%1").arg(device)},
                 &output,
                 nullptr);
        best = Platform::webcamPreferredFormat(output, 24, qint64(1920) * 1080);
    }
    formats.insert(device, best);
    return best;
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

bool hasAudioStream(const QString& path)
{
    QString output;
    const QStringList arguments{QStringLiteral("-v"), QStringLiteral("error"),
                                QStringLiteral("-select_streams"), QStringLiteral("a"),
                                QStringLiteral("-show_entries"), QStringLiteral("stream=index"),
                                QStringLiteral("-of"), QStringLiteral("csv=p=0"), path};
    return runFfprobe(arguments, &output) && !output.trimmed().isEmpty();
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
        return QSize(320, 320);
    case 2:  // rectangle, 4:3
        return QSize(320, 240);
    default: // vertical, 1:1.3
        return QSize(320, 416);
    }
}

QString concatListPath(const QString& workDir)
{
    return QDir(workDir).filePath(QStringLiteral("segments.txt"));
}

const QString kStateFileName = QStringLiteral("recording-state.json");
const QString kPointerFileName = QStringLiteral("pointer.json");

QJsonObject rectObject(const QRect& rect)
{
    return QJsonObject{{QStringLiteral("x"), rect.x()},
                       {QStringLiteral("y"), rect.y()},
                       {QStringLiteral("w"), rect.width()},
                       {QStringLiteral("h"), rect.height()}};
}

QRect rectFrom(const QJsonValue& value)
{
    const QJsonObject object = value.toObject();
    return QRect(object.value(QStringLiteral("x")).toInt(),
                 object.value(QStringLiteral("y")).toInt(),
                 object.value(QStringLiteral("w")).toInt(),
                 object.value(QStringLiteral("h")).toInt());
}

qint64 int64From(const QJsonValue& value)
{
    return value.toVariant().toLongLong();
}

/// Whether `pid` is a running copy of this program. A pid the system reused
/// for another program does not count.
bool recorderProcessRunning(qint64 pid)
{
    if (pid <= 0)
        return false;
    HANDLE process = OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, FALSE,
                                 static_cast<DWORD>(pid));
    if (!process)
        return false;
    bool running = WaitForSingleObject(process, 0) == WAIT_TIMEOUT;
    if (running) {
        wchar_t image[MAX_PATH] = {};
        DWORD size = MAX_PATH;
        if (QueryFullProcessImageNameW(process, 0, image, &size) && size > 0) {
            const QString name = QFileInfo(QString::fromWCharArray(image, int(size))).fileName();
            const QString ours = QFileInfo(QCoreApplication::applicationFilePath()).fileName();
            running = name.compare(ours, Qt::CaseInsensitive) == 0;
        }
    }
    CloseHandle(process);
    return running;
}

/// A free name beside `path`: "name (recovered).mp4", then "(recovered 2)".
QString recoveredOutputPath(const QString& path)
{
    if (!QFileInfo::exists(path))
        return path;
    const QFileInfo info(path);
    const QString suffix = info.suffix().isEmpty() ? QString() : QStringLiteral(".") + info.suffix();
    for (int index = 1; index < 1000; ++index) {
        const QString label = index == 1 ? QStringLiteral(" (recovered)")
                                         : QStringLiteral(" (recovered %1)").arg(index);
        const QString candidate =
            info.dir().filePath(info.completeBaseName() + label + suffix);
        if (!QFileInfo::exists(candidate))
            return candidate;
    }
    return path;
}

/// A job every capture process joins. Windows closes the job's last handle
/// when this process exits for any reason, crash included, and the
/// kill-on-close limit then ends every process in it, so no ffmpeg keeps
/// recording the screen after the recorder that owned it is gone.
HANDLE recorderJob()
{
    static HANDLE job = [] {
        HANDLE handle = CreateJobObjectW(nullptr, nullptr);
        if (handle) {
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION limits{};
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if (!SetInformationJobObject(handle, JobObjectExtendedLimitInformation, &limits,
                                         sizeof(limits))) {
                CloseHandle(handle);
                handle = nullptr;
            }
        }
        return handle;
    }();
    return job;
}

/// Put a started capture process in the recorder's job.
void bindToRecorderLifetime(QProcess* process)
{
    HANDLE job = recorderJob();
    if (!job || !process || process->processId() <= 0)
        return;
    HANDLE child = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, FALSE,
                               static_cast<DWORD>(process->processId()));
    if (!child)
        return;
    if (!AssignProcessToJobObject(job, child))
        recordingLog(QStringLiteral("capture: could not tie ffmpeg %1 to the recorder")
                         .arg(process->processId()));
    CloseHandle(child);
}

QString lowDiskMessage(qint64 freeBytes)
{
    return QStringLiteral("Recording stopped because the disk is almost full. ApexShot keeps "
                          "at least %1 MB free; %2 MB was left. The recording so far was saved.")
        .arg(Platform::kRecordingDiskReserveBytes / (1024 * 1024))
        .arg(freeBytes / (1024 * 1024));
}

QString quoteConcatPath(const QString& path)
{
    QString escaped = QDir::fromNativeSeparators(path);
    escaped.replace(QLatin1Char('\''), QStringLiteral("'\\''"));
    return QStringLiteral("file '%1'").arg(escaped);
}

/// The file a segment's one-frame marker output writes.
QString markerPathFor(const QString& segmentPath)
{
    return segmentPath + QStringLiteral(".mark");
}

/// A second, one-frame output that writes a single byte to the segment's
/// marker file as the first frame clears the input, then closes while the
/// recording output carries on. Flushing the packet makes the file's write
/// time the moment of that frame, which the OS records whatever the
/// recorder's own thread was doing. `prefix` is any filter the frame needs
/// before it can be scaled.
void appendFirstFrameMarker(QStringList& args, const QString& prefix, const QString& segmentPath)
{
    args << QStringLiteral("-map") << QStringLiteral("0:v") << QStringLiteral("-an")
         << QStringLiteral("-vf") << prefix + QStringLiteral("scale=1:1")
         << QStringLiteral("-frames:v") << QStringLiteral("1") << QStringLiteral("-c:v")
         << QStringLiteral("rawvideo") << QStringLiteral("-pix_fmt") << QStringLiteral("gray")
         << QStringLiteral("-flush_packets") << QStringLiteral("1") << QStringLiteral("-f")
         << QStringLiteral("rawvideo") << markerPathFor(segmentPath);
}

/// When ffmpeg wrote a segment's marker, in milliseconds since the epoch, or
/// -1 when no first frame ever reached it.
qint64 markerWrittenMs(const QString& segmentPath)
{
    const QFileInfo info(markerPathFor(segmentPath));
    if (!info.isFile() || info.size() <= 0)
        return -1;
    return info.lastModified().toMSecsSinceEpoch();
}

} // namespace

namespace Platform {

QString ffmpegPath()
{
    return ffmpegExecutable();
}

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
        QObject::connect(tick, &QTimer::timeout, q, [this] {
            emit q->elapsedChanged(elapsedMs());
            // Every ten ticks (two seconds) is often enough to catch a filling
            // disk long before the encoder runs out of room.
            if (++diskCheckTicks % 10 == 0)
                checkDiskSpace();
            // Every two seconds the work directory gets a fresh state file
            // and pointer sidecar, so a crash loses at most that much cursor
            // data, about what it loses of the video's last fragment.
            if (diskCheckTicks % 10 == 0 && state == State::Recording)
                flushRecoveryState();
        });
    }

    ~Private() = default;

    bool startSegment(QString& error);
    void onCaptureFinished(int exitCode, QProcess::ExitStatus status);
    void onWebcamFinished(int exitCode, QProcess::ExitStatus status);
    void onMicFinished();
    void onProcessError(QProcess::ProcessError error);
    void checkCaptureStopped();
    void requestQuit(int timeoutMs);
    void finalize();
    void completeSystemAudio(const QString& videoPath);
    void completeWebcamTrack(const QString& videoPath);
    void compositeWebcam(const QString& videoPath,
                         const QString& trackPath,
                         std::function<void(bool, const QString&)> done);
    void finishJoin(bool ok, const QString& finalPath, const QString& failure, bool keepWorkDir);
    void cleanupAndEmit(bool kept,
                        const QString& path,
                        const QString& failure,
                        bool keepWorkDir = false);
    void fail(const QString& reason);
    /// The smallest free size of the work and output volumes, or -1.
    [[nodiscard]] qint64 freeRecordingBytes() const;
    /// Stop and keep the recording when free space is under the reserve.
    void checkDiskSpace();
    void stopForLowDisk(qint64 freeBytes);
    void cleanupWorkDir();
    void removeDiscardedSidecar();
    void consumeWebcamPreview(const QByteArray& chunk);
    [[nodiscard]] qint64 elapsedMs() const;
    [[nodiscard]] QStringList buildScreenArguments(const QString& segmentPath) const;
    [[nodiscard]] QStringList buildWebcamArguments(const QString& webcamSegmentPath) const;
    [[nodiscard]] QStringList buildAudioArguments(QStringList& warnings) const;
    [[nodiscard]] QString captureOutput() const;
    void openManifestSession();
    void closeManifestSession();
    void writeManifest(const QString& videoPath);
    /// Write `recording-state.json` and the pointer sidecar into the work
    /// directory, so a recorder that dies can be recovered.
    void flushRecoveryState();
    [[nodiscard]] QJsonObject recoveryState() const;

    RecordingSession* q = nullptr;
    State state = State::Idle;
    RecordingRequest request;
    QProcess* process = nullptr;
    /// Window recordings feed ffmpeg from a Windows Graphics Capture pipe
    /// instead of reading the desktop rectangle through gdigrab.
    std::unique_ptr<WindowCaptureSource> windowCapture;
    /// A window-capture start failure falls back to gdigrab for the rest of
    /// this recording instead of retrying the same failing pipe every segment.
    bool windowCaptureDisabled = false;
    /// The camera runs in its own process: a single ffmpeg doing the desktop,
    /// a camera, and two encodes at once starves every real-time input, and
    /// the forced frame rate turns the gaps into staggering on both tracks.
    QProcess* webcamProcess = nullptr;
    /// The screen process's last exit, the number the state machine reports
    /// on; a camera that dies only loses its track.
    int captureExitCode = 0;
    QProcess::ExitStatus captureExitStatus = QProcess::NormalExit;
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
    int diskCheckTicks = 0;
    /// Set once the low-disk stop ran, so the warning is shown only once.
    bool lowDiskStopped = false;
    /// Resume was requested while the previous segment was still finalising.
    bool resumeAfterPause = false;
    QStringList audioArguments;
    QByteArray processOutput;

    /// Set once a camera answered the probe, so a recording without one never
    /// asks ffmpeg for a track that was never opened.
    bool webcamActive = false;
    /// The camera's native frame size, used until the joined track is probed.
    QSize webcamCameraSize;
    /// The size the dshow input is asked to deliver; empty uses the camera's
    /// default, which can be a five-fps 1080p mode.
    QSize webcamCaptureSize;
    /// The dshow pixel_format the input asks for; empty uses the driver's
    /// default, which can be the heaviest raw layout the camera lists.
    QString webcamPixelFormat;
    /// The camera's own frame rate. The desktop rate is not applied to the
    /// track: the studied recorder keeps a device frame rate separate from
    /// the display refresh rate, and a 60 fps desktop can outrun a 30 fps
    /// camera.
    int webcamFps = 30;
    /// The bubble-sized frame the preview pipe carries.
    QSize webcamPreviewSize;
    QByteArray webcamPreviewBuffer;
    bool webcamPreviewLogged = false;
    Platform::WebcamTrack webcamTrack;

    /// The native system-audio track, written while the video records. It is
    /// muxed into the final file after the video segments are joined.
    std::unique_ptr<SystemAudioCapture> systemAudio;
    QString systemAudioPath;
    qint64 systemAudioOffsetMs = 0;

    /// The microphone runs in its own process writing raw PCM: a dshow audio
    /// input inside the capture process starves the real-time video, and the
    /// WAV is muxed into the recording once finalised. It records through
    /// pauses; the spans below are what gets cut back out.
    QProcess* micProcess = nullptr;
    QString micPath;
    /// How far ahead of the video the audio capture started, for the trim.
    qint64 micOffsetMs = 0;
    /// The elapsed time the current pause began at, or -1.
    qint64 micPauseStartMs = -1;
    /// Wall time that pause began at, to measure how long it lasted.
    qint64 pauseWallMs = 0;
    /// Paused spans in elapsed time: [start, start + paused wall duration).
    QVector<QPair<qint64, qint64>> micPauseSpans;

    /// The manifest written beside the sidecar when the recording is kept.
    qint64 recordingStartedAtUnixMs = 0;
    QVector<RecordingManifest::Session> manifestSessions;
    bool manifestSessionOpen = false;
    WindowIdentity manifestWindow;
    qint64 firstScreenStartUnixMs = 0;
    qint64 cameraProcessDelayMs = -1;
    /// Set by buildScreenArguments for the segment it builds.
    mutable QString manifestCaptureBackend;
    mutable QString manifestEncoder;
    mutable QSize manifestOutputSize;
    mutable int manifestCrf = 0;
    /// Set while recover() runs: the pointer sidecar comes from the work
    /// directory's copy instead of the tracker.
    QString recoveredSidecarPath;
    /// Moments marked during this recording, in media milliseconds.
    QVector<qint64> markers;
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
    QString output = tailLines(QString::fromUtf8(processOutput), 8);
    if (!request.webcamStreamUrl.isEmpty())
        output.replace(request.webcamStreamUrl, QStringLiteral("[local webcam stream]"));
    return output;
}

QStringList RecordingSession::Private::buildAudioArguments(QStringList& warnings) const
{
    QStringList arguments;
    QStringList sources;

    if (request.includeMicrophone) {
        QString microphone = request.microphoneDevice.trimmed();
        const QStringList devices = audioInputs();
        if (!microphone.isEmpty() && !devices.isEmpty()
            && !devices.contains(microphone, Qt::CaseInsensitive)) {
            QString matched;
            for (const QString& device : devices) {
                if (device.contains(microphone, Qt::CaseInsensitive)
                    || microphone.contains(device, Qt::CaseInsensitive)) {
                    matched = device;
                    break;
                }
            }
            if (!matched.isEmpty()) {
                microphone = matched;
            } else {
                warnings.append(
                    QStringLiteral("The microphone \"%1\" is not connected; using the default.")
                        .arg(request.microphoneDevice));
                microphone.clear();
            }
        }
        if (microphone.isEmpty())
            microphone = microphoneDevice();
        if (microphone.isEmpty())
            warnings.append(QStringLiteral("No microphone was found; recording without audio."));
        else
            sources.append(microphone);
    }

    // System audio is captured natively and muxed once the recording is
    // finalised; dshow cannot reach loopback audio.
    for (const QString& source : sources) {
        arguments << QStringLiteral("-f") << QStringLiteral("dshow") << QStringLiteral("-i")
                  << QStringLiteral("audio=%1").arg(source);
    }
    return arguments;
}

QStringList RecordingSession::Private::buildScreenArguments(const QString& segmentPath) const
{
    QStringList args;
    args << QStringLiteral("-y") << QStringLiteral("-hide_banner") << QStringLiteral("-loglevel")
         << QStringLiteral("error") << QStringLiteral("-nostats");

    const int fps = request.fps > 0 ? request.fps : 30;

    // Windows Graphics Capture and DXGI Desktop Duplication both hand back
    // composited frames, unlike the GDI path gdigrab uses. Desktop Duplication
    // is the smaller change: ffmpeg already exposes it as a lavfi source, and
    // the same crop and encoder arguments follow it.
    const bool useWindowCapture =
        request.target == RecordingRequest::Target::Window && windowCapture
        && windowCapture->isPrepared();
    bool useDdagrab = false;
    DdagrabSource ddagrab;
    if (!useWindowCapture && request.target != RecordingRequest::Target::Window) {
        ddagrab = ddagrabSourceForRegion(request.region, Platform::monitors());
        useDdagrab = ddagrab.valid && ddagrabSupported(ddagrab.outputIndex);
        if (useDdagrab) {
            recordingLog(QStringLiteral("capture: ddagrab output=%1 offset=%2,%3 size=%4x%5")
                             .arg(ddagrab.outputIndex)
                             .arg(ddagrab.offset.x())
                             .arg(ddagrab.offset.y())
                             .arg(ddagrab.size.width())
                             .arg(ddagrab.size.height()));
        } else if (!ddagrab.valid) {
            recordingLog(QStringLiteral("capture: ddagrab skipped for %1,%2 %3x%4: %5")
                             .arg(request.region.x())
                             .arg(request.region.y())
                             .arg(request.region.width())
                             .arg(request.region.height())
                             .arg(ddagrab.reason));
        }
    }

    // Raw 4K frames are roughly 32 MiB each, so bounding this queue matters.
    args << QStringLiteral("-thread_queue_size") << QStringLiteral("8");
    if (useWindowCapture) {
        recordingLog(QStringLiteral("capture: window graphics pipe=%1 size=%2x%3")
                         .arg(windowCapture->pipePath())
                         .arg(windowCapture->size().width())
                         .arg(windowCapture->size().height()));
        args << QStringLiteral("-f") << QStringLiteral("rawvideo") << QStringLiteral("-pixel_format")
             << QStringLiteral("bgra") << QStringLiteral("-video_size")
             << QStringLiteral("%1x%2")
                    .arg(windowCapture->size().width())
                    .arg(windowCapture->size().height())
             << QStringLiteral("-framerate") << QString::number(fps) << QStringLiteral("-i")
             << windowCapture->pipePath();
    } else if (useDdagrab) {
        args << QStringLiteral("-f") << QStringLiteral("lavfi") << QStringLiteral("-i")
             << ddagrabFilter(ddagrab, fps, request.includeCursor);
    } else {
        args << QStringLiteral("-f") << QStringLiteral("gdigrab") << QStringLiteral("-framerate")
             << QString::number(fps) << QStringLiteral("-draw_mouse")
             << (request.includeCursor ? QStringLiteral("1") : QStringLiteral("0"))
             << QStringLiteral("-offset_x") << QString::number(request.region.x())
             << QStringLiteral("-offset_y") << QString::number(request.region.y())
             << QStringLiteral("-video_size")
             << QStringLiteral("%1x%2").arg(request.region.width()).arg(request.region.height())
             << QStringLiteral("-i") << QStringLiteral("desktop");
    }

    const QSize sourceSize =
        useWindowCapture ? windowCapture->size() : request.region.size();
    const QSize outputSize = recordingOutputSize(sourceSize,
                                                 QSize(request.maxWidth, request.maxHeight),
                                                 request.displayScale);
    const QString scale = recordingScaleFilter(outputSize);
    // ddagrab hands the filter graph a D3D11 frame. Download it before the
    // normal scaler and encoder, which work on system-memory frames.
    const QString videoFilter =
        useDdagrab ? QStringLiteral("hwdownload,format=bgra,%1").arg(scale) : scale;
    args << QStringLiteral("-map") << QStringLiteral("0:v") << QStringLiteral("-vf")
         << videoFilter;

    const int crfBase = request.crf > 0 ? request.crf : 20;
    // Same resolution compensation as the Linux encoder: smaller outputs get a
    // lower CRF so they do not turn to mush. The source size is the capture
    // region; the maximum is already enforced by the scale filter above.
    const int reduction =
        !outputSize.isEmpty()
            ? recordingCrfResolutionReduction(outputSize.width(), outputSize.height())
            : 0;
    const int crf = std::max(1, crfBase - reduction);
    appendVideoEncoder(args, crf, outputSize, fps, request.hevc);
    manifestCaptureBackend = useWindowCapture ? QStringLiteral("window-graphics")
        : useDdagrab                          ? QStringLiteral("ddagrab")
                                              : QStringLiteral("gdigrab");
    const QString hardware = hardwareVideoEncoder(outputSize, fps, request.hevc);
    manifestEncoder = !hardware.isEmpty() ? hardware
        : request.hevc                    ? QStringLiteral("libx265")
                                          : QStringLiteral("libx264");
    manifestOutputSize = outputSize;
    manifestCrf = request.hevc ? crf + Platform::kHevcQualityOffset : crf;
    args << QStringLiteral("-g") << QString::number(fps * 2) << QStringLiteral("-bf")
         << QStringLiteral("2") << QStringLiteral("-r") << QString::number(fps);

    // Fragmented segments stay readable up to their last fragment when the
    // recorder dies, the way the studied recorder's segmented writer does; a
    // plain MP4 only gets its index when ffmpeg exits cleanly. The join in
    // finalize() writes the final file with a normal index. Flushing each
    // packet puts every finished fragment on disk instead of in ffmpeg's
    // output buffer, which a killed process takes with it.
    args << QStringLiteral("-movflags")
         << QStringLiteral("+frag_keyframe+empty_moov+default_base_moof")
         << QStringLiteral("-flush_packets") << QStringLiteral("1") << segmentPath;

    // The screen's first-frame mark; the camera writes the matching one.
    appendFirstFrameMarker(args,
                           useDdagrab ? QStringLiteral("hwdownload,format=bgra,") : QString(),
                           segmentPath);
    return args;
}

QStringList RecordingSession::Private::buildWebcamArguments(
    const QString& webcamSegmentPath) const
{
    QStringList args;
    args << QStringLiteral("-y") << QStringLiteral("-hide_banner") << QStringLiteral("-loglevel")
         << QStringLiteral("error") << QStringLiteral("-nostats");

    const int fps = webcamFps > 0 ? webcamFps : 30;
    // The camera runs on its own clock; a deep buffer keeps the encoder from
    // making dshow drop frames out of the track.
    args << QStringLiteral("-thread_queue_size") << QStringLiteral("1024");
    if (!request.webcamStreamUrl.isEmpty()) {
        args << QStringLiteral("-f") << QStringLiteral("mjpeg")
             << QStringLiteral("-probesize") << QStringLiteral("32768")
             << QStringLiteral("-analyzeduration") << QStringLiteral("100000")
             << QStringLiteral("-use_wallclock_as_timestamps") << QStringLiteral("1")
             << QStringLiteral("-i") << request.webcamStreamUrl;
    } else {
        if (!webcamCaptureSize.isEmpty()) {
            args << QStringLiteral("-video_size")
                 << QStringLiteral("%1x%2")
                        .arg(webcamCaptureSize.width())
                        .arg(webcamCaptureSize.height());
        }
        args << QStringLiteral("-f") << QStringLiteral("dshow") << QStringLiteral("-rtbufsize")
             << QStringLiteral("256M") << QStringLiteral("-framerate") << QString::number(fps);
        if (!webcamPixelFormat.isEmpty())
            args << QStringLiteral("-pixel_format") << webcamPixelFormat;
        args << QStringLiteral("-i")
             << QStringLiteral("video=%1").arg(request.webcamDevice);
    }

    // The track file is the camera alone: the editor places it over the
    // recording, so nothing about the desktop may appear in it. 1080p is
    // more than any bubble size can show, and the cap keeps the extra
    // encoder cheap.
    const QString trackScale =
        QStringLiteral("scale=w='min(iw,1920)':h='min(ih,1080)'"
                       ":force_original_aspect_ratio=decrease,"
                       "scale=trunc(iw/2)*2:trunc(ih/2)*2");
    args << QStringLiteral("-map") << QStringLiteral("0:v") << QStringLiteral("-an")
         << QStringLiteral("-vf") << trackScale;
    const QSize trackSize = webcamCaptureSize.isEmpty()
        ? QSize(1920, 1080)
        : recordingOutputSize(webcamCaptureSize, QSize(1920, 1080));
    appendVideoEncoder(args, 18, trackSize, fps);
    args << QStringLiteral("-g") << QString::number(fps * 2) << QStringLiteral("-r")
         << QString::number(fps) << QStringLiteral("-movflags")
         << QStringLiteral("+frag_keyframe+empty_moov+default_base_moof")
         << QStringLiteral("-flush_packets") << QStringLiteral("1") << webcamSegmentPath;

    // The camera's first-frame mark, timed against the screen's.
    appendFirstFrameMarker(args, QString(), webcamSegmentPath);

    // The preview is a small MJPEG stream rather than raw frames: at the
    // bubble's size a raw pipe carries tens of megabytes a second, so any
    // hitch in the window blocks ffmpeg's stdout and stalls the whole
    // recording with it.
    if (request.webcamStreamUrl.isEmpty()) {
        const QSize preview =
            webcamPreviewSize.isEmpty() ? QSize(320, 320) : webcamPreviewSize;
        args << QStringLiteral("-map") << QStringLiteral("0:v") << QStringLiteral("-an")
             << QStringLiteral("-vf")
             << QStringLiteral(
                    "scale=%1:%2:force_original_aspect_ratio=increase,crop=%1:%2,fps=%3")
                    .arg(preview.width())
                    .arg(preview.height())
                    .arg(fps)
             << QStringLiteral("-c:v") << QStringLiteral("mjpeg") << QStringLiteral("-q:v")
             << QStringLiteral("6") << QStringLiteral("-f") << QStringLiteral("mjpeg")
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

    windowCapture.reset();
    if (request.target == RecordingRequest::Target::Window && !windowCaptureDisabled) {
        auto capture = std::make_unique<WindowCaptureSource>();
        QString captureError;
        if (capture->prepare(request.windowId, request.region.size(), request.fps, &captureError)) {
            windowCapture = std::move(capture);
        } else {
            recordingLog(QStringLiteral("capture: window graphics skipped: %1")
                             .arg(captureError));
        }
    }

    process = new QProcess(q);
    process->setProgram(ffmpegExecutable());
    process->setArguments(buildScreenArguments(path));
    process->setProcessChannelMode(QProcess::SeparateChannels);

    QObject::connect(process, &QProcess::readyReadStandardError, q, [this] {
        processOutput.append(process ? process->readAllStandardError() : QByteArray());
    });
    QObject::connect(process,
                     QOverload<int, QProcess::ExitStatus>::of(&QProcess::finished),
                     q,
                     [this](int code, QProcess::ExitStatus status) {
                         onCaptureFinished(code, status);
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

    bindToRecorderLifetime(process);

    if (windowCapture && !windowCapture->start(&error)) {
        // The pipe never delivered frames. Retry this segment through the
        // gdigrab fallback rather than failing the recording outright.
        recordingLog(QStringLiteral("capture: window graphics failed: %1").arg(error));
        process->kill();
        process->waitForFinished(1000);
        delete process;
        process = nullptr;
        windowCapture.reset();
        windowCaptureDisabled = true;
        return startSegment(error);
    }

    segments.append(path);
    if (segmentIndex == 0)
        firstScreenStartUnixMs = QDateTime::currentMSecsSinceEpoch();

    if (!webcamPath.isEmpty()) {
        webcamProcess = new QProcess(q);
        webcamProcess->setProgram(ffmpegExecutable());
        webcamProcess->setArguments(buildWebcamArguments(webcamPath));
        // The preview frames own stdout, so stderr has to stay on its own
        // channel for the failure messages the recorder reports.
        webcamProcess->setProcessChannelMode(QProcess::SeparateChannels);
        QObject::connect(webcamProcess, &QProcess::readyReadStandardError, q, [this] {
            processOutput.append(webcamProcess ? webcamProcess->readAllStandardError()
                                               : QByteArray());
        });
        QObject::connect(webcamProcess, &QProcess::readyReadStandardOutput, q, [this] {
            consumeWebcamPreview(webcamProcess ? webcamProcess->readAllStandardOutput()
                                               : QByteArray());
        });
        QObject::connect(webcamProcess,
                         QOverload<int, QProcess::ExitStatus>::of(&QProcess::finished),
                         q,
                         [this](int code, QProcess::ExitStatus status) {
                             onWebcamFinished(code, status);
                         });
        webcamProcess->start();
        if (!webcamProcess->waitForStarted(kProbeTimeoutMs)) {
            // A camera process that will not start loses the track, not the
            // recording: warn and carry on with the desktop alone.
            recordingLog(QStringLiteral("webcam process could not start: %1")
                             .arg(webcamProcess->errorString()));
            emit q->warning(
                QStringLiteral("The webcam could not be recorded: %1")
                    .arg(webcamProcess->errorString()));
            delete webcamProcess;
            webcamProcess = nullptr;
            webcamActive = false;
            webcamTrack = Platform::WebcamTrack{};
        } else {
            bindToRecorderLifetime(webcamProcess);
            webcamSegments.append(webcamPath);
            if (segmentIndex == 0 && firstScreenStartUnixMs > 0)
                cameraProcessDelayMs =
                    QDateTime::currentMSecsSinceEpoch() - firstScreenStartUnixMs;
        }
    }

    ++segmentIndex;
    return true;
}

void RecordingSession::Private::consumeWebcamPreview(const QByteArray& chunk)
{
    if (chunk.isEmpty() || webcamPreviewSize.isEmpty())
        return;

    static const QByteArray kSoi("\xff\xd8", 2);
    static const QByteArray kEoi("\xff\xd9", 2);

    webcamPreviewBuffer.append(chunk);
    // A bubble that fell behind only needs the newest picture; a runaway
    // buffer means the window is not reading at all, so resynchronise on the
    // last start marker rather than decoding a stale backlog.
    if (webcamPreviewBuffer.size() > 4 * 1024 * 1024) {
        const int lastStart = webcamPreviewBuffer.lastIndexOf(kSoi);
        webcamPreviewBuffer =
            lastStart > 0 ? webcamPreviewBuffer.mid(lastStart) : QByteArray();
    }

    QImage newest;
    int start = webcamPreviewBuffer.indexOf(kSoi);
    while (start >= 0) {
        if (start > 0)
            webcamPreviewBuffer.remove(0, start);
        const int end = webcamPreviewBuffer.indexOf(kEoi, 2);
        if (end < 0)
            break;
        const QByteArray jpeg = webcamPreviewBuffer.left(end + 2);
        webcamPreviewBuffer.remove(0, end + 2);
        QImage frame;
        if (frame.loadFromData(jpeg)) {
            newest = frame;
            if (!webcamPreviewLogged) {
                webcamPreviewLogged = true;
                recordingLog(QStringLiteral("webcam preview %1x%2")
                                 .arg(frame.width())
                                 .arg(frame.height()));
            }
        }
        start = webcamPreviewBuffer.indexOf(kSoi);
    }

    // One paint per read: emitting every buffered frame would replay stale
    // pictures instead of catching up to the newest one.
    if (!newest.isNull())
        emit q->webcamFrame(newest);
}

void RecordingSession::Private::onProcessError(QProcess::ProcessError error)
{
    if (error != QProcess::FailedToStart || !process)
        return;
    processOutput.append(process->readAllStandardError());
    fail(QStringLiteral("ffmpeg could not start: %1").arg(process->errorString()));
}

void RecordingSession::Private::onCaptureFinished(int exitCode, QProcess::ExitStatus status)
{
    if (windowCapture)
        windowCapture->stop();
    if (process) {
        processOutput.append(process->readAllStandardError());
        process->deleteLater();
        process = nullptr;
    }
    captureExitCode = exitCode;
    captureExitStatus = status;
    if (state == State::Recording && webcamProcess) {
        // The screen is gone; the track follows it down rather than keep
        // writing a camera file nothing will join.
        webcamProcess->kill();
    }
    checkCaptureStopped();
}

void RecordingSession::Private::onWebcamFinished(int exitCode, QProcess::ExitStatus status)
{
    if (webcamProcess) {
        processOutput.append(webcamProcess->readAllStandardError());
        consumeWebcamPreview(webcamProcess->readAllStandardOutput());
        webcamProcess->deleteLater();
        webcamProcess = nullptr;
    }
    if (state == State::Recording && process) {
        // A camera that dies mid-recording loses its track, not the video:
        // the desktop keeps recording and the file simply has no bubble.
        webcamActive = false;
        webcamTrack = Platform::WebcamTrack{};
        Q_UNUSED(exitCode);
        Q_UNUSED(status);
        recordingLog(QStringLiteral("webcam process died mid-recording"));
        emit q->warning(
            QStringLiteral("The webcam stopped recording; the video continues without it."));
    }
    checkCaptureStopped();
}

void RecordingSession::Private::onMicFinished()
{
    if (micProcess) {
        processOutput.append(micProcess->readAllStandardError());
        micProcess->deleteLater();
        micProcess = nullptr;
    }
    if (state == State::Recording && process) {
        // A mic that dies mid-recording loses its track, not the video.
        micPath.clear();
        recordingLog(QStringLiteral("microphone process died mid-recording"));
        emit q->warning(QStringLiteral(
            "The microphone stopped recording; the video continues without it."));
    }
    checkCaptureStopped();
}

void RecordingSession::Private::checkCaptureStopped()
{
    if (process || webcamProcess)
        return;

    const QString detail = captureOutput();

    if (state == State::Pausing) {
        if (captureExitStatus != QProcess::NormalExit || captureExitCode != 0) {
            fail(detail.isEmpty()
                     ? QStringLiteral("ffmpeg stopped while pausing (code %1)")
                           .arg(captureExitCode)
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
        if (micProcess)
            return;
        emit q->captureStopped();
        if (discardRequested) {
            cleanupAndEmit(false, QString(), QString());
            return;
        }
        if (captureExitStatus != QProcess::NormalExit || captureExitCode != 0) {
            fail(detail.isEmpty()
                     ? QStringLiteral("ffmpeg stopped before the recording was saved (code %1)")
                           .arg(captureExitCode)
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
    // Closing the window-capture pipe gives ffmpeg a clean end of input.
    if (windowCapture)
        windowCapture->stop();
    for (QProcess* target : {process, webcamProcess}) {
        if (!target)
            continue;
        target->write("q\n");
        target->closeWriteChannel();
        // A pause can be followed by a resume before the timeout, so only
        // the process that received `q` may be killed here.
        QTimer::singleShot(timeoutMs, q, [this, target] {
            if ((process && process == target) || (webcamProcess && webcamProcess == target))
                target->kill();
        });
    }
}

void RecordingSession::Private::fail(const QString& reason)
{
    tick->stop();
    if (windowCapture)
        windowCapture->stop();
    if (systemAudio)
        systemAudio->stop();
    pointerTracker.discard();
    state = State::Failed;
    qWarning().noquote() << "[recording]" << reason;
    for (QProcess* target : {process, webcamProcess, micProcess}) {
        if (!target)
            continue;
        target->kill();
        target->deleteLater();
    }
    process = nullptr;
    webcamProcess = nullptr;
    micProcess = nullptr;
    // Keep the segments so a failed save can still be recovered from the work
    // directory the message names.
    const QString message = workDir.isEmpty()
        ? reason
        : QStringLiteral("%1 The captured segments are in %2.")
              .arg(reason, QDir::toNativeSeparators(workDir));
    cleanupAndEmit(false, QString(), message, true);
}

qint64 RecordingSession::Private::freeRecordingBytes() const
{
    return lowestFreeDiskBytes({workDir, QFileInfo(request.outputPath).absolutePath()});
}

void RecordingSession::Private::checkDiskSpace()
{
    if (lowDiskStopped || state != State::Recording)
        return;
    const qint64 freeBytes = freeRecordingBytes();
    if (freeBytes >= 0 && freeBytes < kRecordingDiskReserveBytes)
        stopForLowDisk(freeBytes);
}

void RecordingSession::Private::stopForLowDisk(qint64 freeBytes)
{
    if (lowDiskStopped)
        return;
    lowDiskStopped = true;
    const QString message = lowDiskMessage(freeBytes);
    recordingLog(QStringLiteral("low disk: %1").arg(message));
    qWarning().noquote() << "[recording]" << message;
    // Stop first: the warning's handler may show a modal box, and the disk
    // keeps filling while it is open.
    q->stop();
    emit q->warning(message);
}

void RecordingSession::Private::completeSystemAudio(const QString& videoPath)
{
    const auto next = [this](const QString& path) { completeWebcamTrack(path); };

    if (systemAudio)
        systemAudio->stop();
    systemAudio.reset();

    struct AudioInput {
        QString path;
        double offsetSeconds;
        QString select;
    };
    QVector<AudioInput> inputs;
    const auto usable = [](const QString& path) {
        return !path.isEmpty() && QFileInfo::exists(path) && QFileInfo(path).size() > 68;
    };
    if (usable(micPath)) {
        // The mic recorded through pauses; drop those spans so the audio
        // tracks the kept video segments.
        QString select;
        if (!micPauseSpans.isEmpty()) {
            QStringList spans;
            for (const auto& span : micPauseSpans) {
                spans << QStringLiteral("between(t,%1,%2)")
                             .arg(span.first / 1000.0, 0, 'f', 3)
                             .arg(span.second / 1000.0, 0, 'f', 3);
            }
            select = QStringLiteral(",aselect='not(%1)',asetpts=N/SR/TB")
                         .arg(spans.join(QStringLiteral("+")));
        }
        inputs.append(
            {micPath, std::max<qint64>(0, micOffsetMs) / 1000.0, select});
    }
    if (usable(systemAudioPath)) {
        inputs.append({systemAudioPath,
                       std::max<qint64>(0, systemAudioOffsetMs) / 1000.0,
                       QString()});
    }
    micPath.clear();
    systemAudioPath.clear();

    if (inputs.isEmpty()) {
        next(videoPath);
        return;
    }

    const QString mixedPath = QDir(workDir).filePath(QStringLiteral("mixed-audio.mp4"));
    QFile::remove(mixedPath);
    recordingLog(QStringLiteral("audio: mux %1 tracks onto %2")
                     .arg(inputs.size())
                     .arg(videoPath));

    const bool videoHasAudio = hasAudioStream(videoPath);
    QStringList labels;
    QStringList parts;
    for (int i = 0; i < inputs.size(); ++i) {
        labels << QStringLiteral("[a%1]").arg(i);
        parts << QStringLiteral("[%1:a]atrim=start=%2,asetpts=PTS-STARTPTS%3,apad%4")
                     .arg(i + 1)
                     .arg(inputs[i].offsetSeconds, 0, 'f', 3)
                     .arg(inputs[i].select)
                     .arg(labels.last());
    }
    QStringList mix = labels;
    if (videoHasAudio)
        mix.prepend(QStringLiteral("[0:a]"));
    QString filter = parts.join(QLatin1Char(';'));
    if (mix.size() > 1) {
        filter += QStringLiteral(";%1amix=inputs=%2:duration=longest:"
                                 "dropout_transition=0:normalize=0[aout]")
                      .arg(mix.join(QString()))
                      .arg(mix.size());
    } else {
        filter += QStringLiteral(";%1anull[aout]").arg(labels.first());
    }

    QStringList arguments{QStringLiteral("-y"), QStringLiteral("-hide_banner"),
                          QStringLiteral("-loglevel"), QStringLiteral("error"),
                          QStringLiteral("-i"), videoPath};
    for (const AudioInput& input : inputs) {
        // A recorder that died leaves its WAV header's lengths unwritten;
        // reading to the end of the file works for a finished one too.
        if (input.path.endsWith(QStringLiteral(".wav"), Qt::CaseInsensitive))
            arguments << QStringLiteral("-ignore_length") << QStringLiteral("1");
        arguments << QStringLiteral("-i") << input.path;
    }
    arguments << QStringLiteral("-filter_complex")
              << filter
              << QStringLiteral("-map") << QStringLiteral("0:v")
              << QStringLiteral("-map") << QStringLiteral("[aout]")
              << QStringLiteral("-c:v") << QStringLiteral("copy")
              << QStringLiteral("-c:a") << QStringLiteral("aac")
              << QStringLiteral("-b:a") << QStringLiteral("160k")
              << QStringLiteral("-ac")
              << (request.monoAudio ? QStringLiteral("1") : QStringLiteral("2"))
              << QStringLiteral("-movflags") << QStringLiteral("+faststart")
              << QStringLiteral("-shortest");
    if (request.hevc)
        arguments << QStringLiteral("-tag:v") << QStringLiteral("hvc1");
    arguments << mixedPath;

    auto* mixer = new QProcess(q);
    mixer->setProgram(ffmpegExecutable());
    mixer->setProcessChannelMode(QProcess::MergedChannels);
    mixer->setArguments(arguments);
    QObject::connect(
        mixer,
        QOverload<int, QProcess::ExitStatus>::of(&QProcess::finished),
        q,
        [this, mixer, mixedPath, videoPath, next, inputs](int code,
                                                        QProcess::ExitStatus status) {
            const QString detail = tailLines(QString::fromUtf8(mixer->readAll()), 6);
            mixer->deleteLater();
            for (const AudioInput& input : inputs)
                QFile::remove(input.path);

            recordingLog(QStringLiteral("system audio: mux exit=%1 size=%2 detail=%3")
                             .arg(code)
                             .arg(QFileInfo(mixedPath).size())
                             .arg(detail));
            if (status == QProcess::NormalExit && code == 0
                && QFileInfo(mixedPath).size() > 0) {
                // Keep the silent video until the mixed file is in its place;
                // a failed rename must never leave the recording with neither.
                const QString originalPath = videoPath + QStringLiteral(".video");
                QFile::remove(originalPath);
                const bool parked = QFile::rename(videoPath, originalPath);
                if (parked && QFile::rename(mixedPath, videoPath)) {
                    QFile::remove(originalPath);
                } else {
                    if (parked)
                        QFile::rename(originalPath, videoPath);
                    QFile::remove(mixedPath);
                    emit q->warning(
                        QStringLiteral("The system audio could not be joined to the recording."));
                }
            } else {
                QFile::remove(mixedPath);
                qWarning().noquote() << "[recording] system audio mux failed:" << detail;
                emit q->warning(
                    QStringLiteral("The system audio could not be added to the recording: %1")
                        .arg(detail.isEmpty() ? QStringLiteral("the audio join failed") : detail));
            }
            next(videoPath);
        });
    mixer->start();
    if (!mixer->waitForStarted(kProbeTimeoutMs)) {
        const QString detail = mixer->errorString();
        mixer->deleteLater();
        systemAudio.reset();
        QFile::remove(systemAudioPath);
        systemAudioPath.clear();
        emit q->warning(QStringLiteral("The system audio could not be added: %1").arg(detail));
        next(videoPath);
    }
}

void RecordingSession::Private::completeWebcamTrack(const QString& videoPath)
{
    const auto finish = [this, videoPath](bool joined, const QString& detail) {
        if (!joined && !detail.isEmpty()) {
            recordingLog(QStringLiteral("webcam track failed: %1").arg(detail));
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

    // Each camera segment's lag behind its screen segment, from the write
    // times of their first-frame markers; empty when any marker is missing.
    QVector<double> lags;
    if (segments.size() == webcamSegments.size()) {
        for (int index = 0; index < webcamSegments.size(); ++index) {
            const qint64 screen = markerWrittenMs(segments.at(index));
            const qint64 camera = markerWrittenMs(webcamSegments.at(index));
            if (screen < 0 || camera < 0) {
                recordingLog(QStringLiteral("webcam sync: segment %1 has no first-frame mark")
                                 .arg(index));
                lags.clear();
                break;
            }
            lags.append(std::max(0.0, double(camera - screen) / 1000.0));
            recordingLog(QStringLiteral("webcam sync: segment %1 camera lag %2 s")
                             .arg(index)
                             .arg(lags.last(), 0, 'f', 3));
        }
    }
    // Joining segments needs each screen segment's length to place the next
    // camera segment; without them the join falls back to the estimate.
    QVector<double> segmentSeconds;
    if (!lags.isEmpty() && webcamSegments.size() > 1) {
        if (segments.size() == webcamSegments.size()) {
            for (const QString& segment : segments)
                segmentSeconds.append(videoDuration(segment));
        }
        bool usable = segmentSeconds.size() == webcamSegments.size();
        for (int index = 0; usable && index + 1 < webcamSegments.size(); ++index)
            usable = segmentSeconds.at(index) + lags.at(index + 1) - lags.at(index) > 0.05;
        if (!usable) {
            recordingLog(QStringLiteral("webcam sync: segment lengths unreadable, estimating"));
            lags.clear();
        }
    }

    const auto settle = [this, videoPath, trackPath, finish, lags](bool written,
                                                                   const QString& detail) {
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
        // The camera's first frame was timed against the screen's first
        // frame, so the gap is measured rather than inferred. Without that
        // measurement, assume both stopped together and put the whole length
        // difference at the front.
        const double videoSeconds = videoDuration(videoPath);
        const double estimate = std::max(0.0, videoSeconds - webcamTrack.duration);
        webcamTrack.start = lags.isEmpty() ? estimate : lags.first();
        recordingLog(QStringLiteral("webcam sync: start %1 s (%2), length estimate %3 s")
                         .arg(webcamTrack.start, 0, 'f', 3)
                         .arg(lags.isEmpty() ? QStringLiteral("estimated")
                                             : QStringLiteral("measured"))
                         .arg(estimate, 0, 'f', 3));
        if (!webcamTrack.is_valid()) {
            QFile::remove(trackPath);
            finish(false, QStringLiteral("the camera track had no readable frames"));
            return;
        }
        compositeWebcam(
            videoPath,
            trackPath,
            [this, finish](bool baked, const QString& detail) {
                if (!baked && !detail.isEmpty()) {
                    recordingLog(QStringLiteral("webcam composite failed: %1").arg(detail));
                    qWarning().noquote() << "[recording] webcam composite failed:" << detail;
                    emit q->warning(
                        QStringLiteral("The webcam could not be added to the recording: %1")
                            .arg(detail));
                }
                // The camera track and any clean screen file stay beside the
                // sidecar even when the saved video already carries the
                // camera, so the editor can rebuild the layer.
                finish(true, QString());
            });
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
    for (int index = 0; index < webcamSegments.size(); ++index) {
        list.write(quoteConcatPath(webcamSegments.at(index)).toUtf8() + '\n');
        // Each camera segment starts its own lag after its screen segment.
        // Ending every segment but the last where the next one belongs keeps
        // the joined track in step with the joined screen: a short segment
        // leaves a gap that holds its last frame, a long one is trimmed.
        if (!lags.isEmpty() && index + 1 < webcamSegments.size()) {
            const double length =
                segmentSeconds.at(index) + lags.at(index + 1) - lags.at(index);
            list.write(QStringLiteral("outpoint %1\n").arg(length, 0, 'f', 6).toUtf8());
        }
    }
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


void RecordingSession::Private::compositeWebcam(
    const QString& videoPath,
    const QString& trackPath,
    std::function<void(bool, const QString&)> done)
{
    const QSize videoSize = videoFrameSize(videoPath);
    const double videoSeconds = videoDuration(videoPath);
    const double cameraSeconds = videoDuration(trackPath);
    if (videoSize.isEmpty() || videoSeconds <= 0.0 || cameraSeconds <= 0.0) {
        done(false, QStringLiteral("the recording or camera track had no readable frames"));
        return;
    }

    const Platform::WebcamLayout& layout = request.webcamLayout;
    const int width = std::max(2, int(std::lround(layout.w * videoSize.width())));
    const int height = std::max(2, int(std::lround(layout.h * videoSize.height())));
    const int x = std::clamp(int(std::lround(layout.x * videoSize.width())),
                             0,
                             std::max(0, videoSize.width() - 2));
    const int y = std::clamp(int(std::lround(layout.y * videoSize.height())),
                             0,
                             std::max(0, videoSize.height() - 2));
    // The camera's first frame sits webcamTrack.start into the video: the
    // measured gap, or the length estimate without one. Hold that frame over
    // the gap rather than showing the desktop without a bubble at the start.
    const double startDelay = std::max(0.0, webcamTrack.start);

    QString filter = layout.flip ? QStringLiteral("[1:v]hflip,") : QStringLiteral("[1:v]");
    filter += QStringLiteral(
                         "scale=%1:%2:force_original_aspect_ratio=increase,"
                         "crop=%1:%2,setsar=1,format=rgba")
                         .arg(width)
                         .arg(height);
    // The bubble's outline is an alpha shape, so the bake has to carry the same
    // mask: a plain crop would put a square camera into a round bubble. The
    // mask is the shape's signed distance, feathered over one pixel so the edge
    // is as clean as the live preview's.
    const auto mask = [](const QString& distance) {
        return QStringLiteral(",geq=r='r(X,Y)':g='g(X,Y)':b='b(X,Y)':a='255*clip(0.5-(%1),0,1)'")
            .arg(distance);
    };
    if (layout.shape == 0) {
        filter += mask(QStringLiteral("sqrt((X-%1)*(X-%1)+(Y-%2)*(Y-%2))-%3")
                           .arg(width / 2.0, 0, 'f', 3)
                           .arg(height / 2.0, 0, 'f', 3)
                           .arg(width / 2.0, 0, 'f', 3));
    } else {
        // The studied corner: 0.15 of the camera's shorter side.
        const double radius = 0.15 * double(std::min(width, height));
        filter += mask(QStringLiteral("sqrt(max(abs(X-%1)-%2,0)*max(abs(X-%1)-%2,0)"
                                      "+max(abs(Y-%3)-%4,0)*max(abs(Y-%3)-%4,0))-%5")
                           .arg(width / 2.0, 0, 'f', 3)
                           .arg(width / 2.0 - radius, 0, 'f', 3)
                           .arg(height / 2.0, 0, 'f', 3)
                           .arg(height / 2.0 - radius, 0, 'f', 3)
                           .arg(radius, 0, 'f', 3));
    }
    if (startDelay > 0.02) {
        filter += QStringLiteral(",tpad=start_duration=%1:start_mode=clone")
                      .arg(startDelay, 0, 'f', 3);
    }
    filter += QStringLiteral(
        "[cam];[0:v][cam]overlay=%1:%2:shortest=0:eof_action=repeat[v]")
                  .arg(x)
                  .arg(y);

    const QString compositePath = videoPath + QStringLiteral(".webcam-composite.mp4");
    QFile::remove(compositePath);
    auto* composer = new QProcess(q);
    composer->setProgram(ffmpegExecutable());
    composer->setProcessChannelMode(QProcess::SeparateChannels);
    QStringList compositeArgs{
        QStringLiteral("-y"),
        QStringLiteral("-hide_banner"),
        QStringLiteral("-loglevel"),
        QStringLiteral("error"),
        QStringLiteral("-i"),
        videoPath,
        QStringLiteral("-i"),
        trackPath,
        QStringLiteral("-filter_complex"),
        filter,
        QStringLiteral("-map"),
        QStringLiteral("[v]"),
        QStringLiteral("-map"),
        QStringLiteral("0:a?"),
    };
    // The composite is what the saved video shows, and it is the second
    // generation of the screen capture. Keep the selected tier, but give it
    // two extra quality points so the re-encode does not flatten the choice.
    const int compositeBase = request.crf > 0 ? request.crf : 20;
    const int compositeReduction = recordingCrfResolutionReduction(videoSize.width(),
                                                                   videoSize.height());
    const int compositeCrf = std::max(1, compositeBase - compositeReduction - 2);
    appendVideoEncoder(compositeArgs,
                       compositeCrf,
                       videoSize,
                       request.fps > 0 ? request.fps : 30,
                       request.hevc);
    compositeArgs << QStringLiteral("-c:a") << QStringLiteral("copy")
                  << QStringLiteral("-movflags") << QStringLiteral("+faststart")
                  << compositePath;
    composer->setArguments(compositeArgs);
    QObject::connect(
        composer,
        QOverload<int, QProcess::ExitStatus>::of(&QProcess::finished),
        q,
        [this, composer, videoPath, compositePath, done](int code, QProcess::ExitStatus status) {
            const QString detail =
                tailLines(QString::fromUtf8(composer->readAllStandardError()), 6);
            composer->deleteLater();
            if (status != QProcess::NormalExit || code != 0
                || QFileInfo(compositePath).size() <= 0) {
                QFile::remove(compositePath);
                done(false,
                     detail.isEmpty() ? QStringLiteral("the webcam join failed") : detail);
                return;
            }

            // Keep the clean screen footage beside the composited video. The
            // editor uses it to rebuild the camera layer without drawing the
            // camera that is already baked into the playable file.
            const QString parked = QFileInfo(videoPath).dir().filePath(
                QStringLiteral(".apexshot-screen-%1.mp4")
                    .arg(QUuid::createUuid().toString(QUuid::WithoutBraces)));
            QFile::remove(parked);
            const bool held = QFile::rename(videoPath, parked);
            if (held && QFile::rename(compositePath, videoPath)) {
                const DWORD attributes =
                    GetFileAttributesW(reinterpret_cast<LPCWSTR>(parked.utf16()));
                if (attributes != INVALID_FILE_ATTRIBUTES)
                    SetFileAttributesW(reinterpret_cast<LPCWSTR>(parked.utf16()),
                                       attributes | FILE_ATTRIBUTE_HIDDEN);
                webcamTrack.screenFile = QFileInfo(parked).fileName();
                done(true, QString());
            } else {
                if (held)
                    QFile::rename(parked, videoPath);
                QFile::remove(compositePath);
                done(false, QStringLiteral("the recording could not be replaced"));
            }
        });
    composer->start();
    if (!composer->waitForStarted(kProbeTimeoutMs)) {
        const QString detail = composer->errorString();
        composer->deleteLater();
        QFile::remove(compositePath);
        done(false, detail);
    }
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
    QFile::remove(path + QStringLiteral(".recording.json"));
    QDir(path + QStringLiteral(".cursors")).removeRecursively();
    if (!webcamTrack.screenFile.isEmpty()) {
        QFileInfo info(request.outputPath);
        QFile::remove(info.dir().filePath(webcamTrack.screenFile));
    }
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
        QString sidecarError;
        if (!recoveredSidecarPath.isEmpty()) {
            if (QFileInfo::exists(recoveredSidecarPath)
                && !PointerTracker::adoptSidecar(
                    recoveredSidecarPath, path, webcamTrack, &sidecarError))
                qWarning().noquote() << "[recording] could not restore pointer sidecar:"
                                     << sidecarError;
            QDir(QFileInfo(recoveredSidecarPath).absolutePath()).removeRecursively();
            recoveredSidecarPath.clear();
        } else {
            pointerTracker.stop(elapsedMs());
            if (!pointerTracker.writeSidecar(path, webcamTrack, &sidecarError))
                qWarning().noquote() << "[recording] could not write pointer sidecar:"
                                     << sidecarError;
        }
        writeManifest(path);
    } else if (!kept && discardRequested) {
        pointerTracker.discard();
        removeDiscardedSidecar();
    }
    if (!kept && !keepWorkDir)
        cleanupWorkDir();
    // A kept work directory outlives this session: forget it so the
    // destructor does not delete what the failure message points at, and so
    // the next launch can offer to recover it.
    if (keepWorkDir)
        workDir.clear();
    emit q->finished(kept, kept ? path : QString(), failure);
    emit q->stateChanged();
}

void RecordingSession::Private::openManifestSession()
{
    if (manifestSessionOpen)
        return;
    RecordingManifest::Session session;
    session.startedAtUnixMs = QDateTime::currentMSecsSinceEpoch();
    session.startMs = elapsedBeforeMs;
    if (request.target == RecordingRequest::Target::Window)
        session.windowBounds = windowIdentity(request.windowId).bounds;
    manifestSessions.append(session);
    manifestSessionOpen = true;
}

void RecordingSession::Private::closeManifestSession()
{
    if (!manifestSessionOpen || manifestSessions.isEmpty())
        return;
    manifestSessions.last().endedAtUnixMs = QDateTime::currentMSecsSinceEpoch();
    manifestSessions.last().endMs = elapsedBeforeMs;
    manifestSessionOpen = false;
}

void RecordingSession::Private::writeManifest(const QString& videoPath)
{
    closeManifestSession();
    RecordingManifest manifest;
    manifest.startedAtUnixMs = recordingStartedAtUnixMs;
    manifest.finishedAtUnixMs = QDateTime::currentMSecsSinceEpoch();
    manifest.durationMs = elapsedBeforeMs;
    switch (request.target) {
    case RecordingRequest::Target::Screen:
        manifest.target = QStringLiteral("screen");
        break;
    case RecordingRequest::Target::Area:
        manifest.target = QStringLiteral("area");
        break;
    case RecordingRequest::Target::Window:
        manifest.target = QStringLiteral("window");
        break;
    }
    manifest.region = request.region;
    manifest.fps = request.fps;
    manifest.displayScale = request.displayScale;
    manifest.displayRefreshRate = displayRefreshRateAt(request.region.center());
    manifest.outputSize = manifestOutputSize;
    manifest.encoder = manifestEncoder;
    manifest.crf = manifestCrf;
    manifest.captureBackend = manifestCaptureBackend;
    if (request.target == RecordingRequest::Target::Window) {
        manifest.hasWindow = true;
        manifest.windowId = request.windowId;
        manifest.windowTitle = manifestWindow.title;
        manifest.windowApp = manifestWindow.app;
        manifest.windowBounds = manifestWindow.bounds;
    }
    manifest.microphone = !micPath.isEmpty();
    manifest.microphoneDevice = request.microphoneDevice;
    if (!systemAudioPath.isEmpty()) {
        manifest.systemAudio =
            request.systemAudio == RecordingRequest::SystemAudio::Application
                ? QStringLiteral("app")
                : QStringLiteral("all");
        manifest.systemAudioApplication = request.systemAudioApplication;
    }
    manifest.monoAudio = request.monoAudio;
    if (webcamActive && webcamTrack.is_valid()) {
        manifest.hasCamera = true;
        manifest.cameraDevice = request.webcamDevice;
        manifest.cameraWidth = webcamTrack.width;
        manifest.cameraHeight = webcamTrack.height;
        manifest.cameraFps = webcamFps;
        manifest.cameraStartMs = qRound64(webcamTrack.start * 1000.0);
        manifest.cameraDurationMs = qRound64(webcamTrack.duration * 1000.0);
        manifest.cameraProcessDelayMs = cameraProcessDelayMs;
    }
    manifest.keys = request.captureKeystrokes;
    manifest.sessions = manifestSessions;
    manifest.markers = markers;

    QString error;
    if (!writeRecordingManifest(videoPath, manifest, &error))
        qWarning().noquote() << "[recording] could not write the recording manifest:" << error;
}

QJsonObject RecordingSession::Private::recoveryState() const
{
    const auto names = [](const QStringList& paths) {
        QJsonArray array;
        for (const QString& path : paths)
            array.append(QFileInfo(path).fileName());
        return array;
    };
    QJsonArray spans;
    for (const auto& span : micPauseSpans)
        spans.append(QJsonArray{span.first, span.second});
    QJsonArray sessions;
    for (const RecordingManifest::Session& session : manifestSessions) {
        sessions.append(QJsonObject{{QStringLiteral("startedAtUnixMs"), session.startedAtUnixMs},
                                    {QStringLiteral("endedAtUnixMs"), session.endedAtUnixMs},
                                    {QStringLiteral("startMs"), session.startMs},
                                    {QStringLiteral("endMs"), session.endMs},
                                    {QStringLiteral("windowBounds"),
                                     rectObject(session.windowBounds)}});
    }
    const QJsonObject requestObject{
        {QStringLiteral("target"), static_cast<int>(request.target)},
        {QStringLiteral("region"), rectObject(request.region)},
        {QStringLiteral("windowId"), QString::number(static_cast<qulonglong>(request.windowId))},
        {QStringLiteral("fps"), request.fps},
        {QStringLiteral("displayScale"), request.displayScale},
        {QStringLiteral("crf"), request.crf},
        {QStringLiteral("hevc"), request.hevc},
        {QStringLiteral("maxWidth"), request.maxWidth},
        {QStringLiteral("maxHeight"), request.maxHeight},
        {QStringLiteral("monoAudio"), request.monoAudio},
        {QStringLiteral("captureKeystrokes"), request.captureKeystrokes},
        {QStringLiteral("microphoneDevice"), request.microphoneDevice},
        {QStringLiteral("systemAudio"), static_cast<int>(request.systemAudio)},
        {QStringLiteral("systemAudioApplication"), request.systemAudioApplication},
        {QStringLiteral("webcamDevice"), request.webcamDevice},
        {QStringLiteral("webcamLayout"),
         QJsonObject{{QStringLiteral("x"), request.webcamLayout.x},
                     {QStringLiteral("y"), request.webcamLayout.y},
                     {QStringLiteral("w"), request.webcamLayout.w},
                     {QStringLiteral("h"), request.webcamLayout.h},
                     {QStringLiteral("shape"), request.webcamLayout.shape},
                     {QStringLiteral("flip"), request.webcamLayout.flip}}},
    };
    return QJsonObject{
        {QStringLiteral("version"), 1},
        {QStringLiteral("pid"), static_cast<qint64>(QCoreApplication::applicationPid())},
        {QStringLiteral("outputPath"), request.outputPath},
        {QStringLiteral("startedAtUnixMs"), recordingStartedAtUnixMs},
        {QStringLiteral("elapsedMs"), elapsedMs()},
        {QStringLiteral("request"), requestObject},
        {QStringLiteral("segments"), names(segments)},
        {QStringLiteral("webcamSegments"), names(webcamSegments)},
        {QStringLiteral("webcamActive"), webcamActive},
        {QStringLiteral("webcamCameraWidth"), webcamCameraSize.width()},
        {QStringLiteral("webcamCameraHeight"), webcamCameraSize.height()},
        {QStringLiteral("webcamFps"), webcamFps},
        {QStringLiteral("micPath"), micPath.isEmpty() ? QString() : QFileInfo(micPath).fileName()},
        {QStringLiteral("micOffsetMs"), micOffsetMs},
        {QStringLiteral("micPauseStartMs"), micPauseStartMs},
        {QStringLiteral("micPauseSpans"), spans},
        {QStringLiteral("systemAudioPath"),
         systemAudioPath.isEmpty() ? QString() : QFileInfo(systemAudioPath).fileName()},
        {QStringLiteral("systemAudioOffsetMs"), systemAudioOffsetMs},
        {QStringLiteral("sessions"), sessions},
        {QStringLiteral("markers"),
         [this] {
             QJsonArray array;
             for (const qint64 marker : markers)
                 array.append(marker);
             return array;
         }()},
        {QStringLiteral("window"),
         QJsonObject{{QStringLiteral("title"), manifestWindow.title},
                     {QStringLiteral("app"), manifestWindow.app},
                     {QStringLiteral("bounds"), rectObject(manifestWindow.bounds)}}},
        {QStringLiteral("capture"),
         QJsonObject{{QStringLiteral("backend"), manifestCaptureBackend},
                     {QStringLiteral("encoder"), manifestEncoder},
                     {QStringLiteral("width"), manifestOutputSize.width()},
                     {QStringLiteral("height"), manifestOutputSize.height()},
                     {QStringLiteral("crf"), manifestCrf}}},
    };
}

void RecordingSession::Private::flushRecoveryState()
{
    if (workDir.isEmpty())
        return;
    QString error;
    if (!pointerTracker.writeSidecarTo(QDir(workDir).filePath(kPointerFileName),
                                       Platform::WebcamTrack{},
                                       &error))
        recordingLog(QStringLiteral("recovery: pointer flush failed: %1").arg(error));
    QSaveFile file(QDir(workDir).filePath(kStateFileName));
    if (file.open(QIODevice::WriteOnly)) {
        file.write(QJsonDocument(recoveryState()).toJson(QJsonDocument::Indented));
        if (!file.commit())
            recordingLog(QStringLiteral("recovery: state write failed: %1").arg(file.errorString()));
    }
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
    });
    if (request.hevc)
        concat->setArguments(concat->arguments() << QStringLiteral("-tag:v")
                                                 << QStringLiteral("hvc1"));
    concat->setArguments(concat->arguments() << finalPath);

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
                             completeSystemAudio(finalPath);
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
    if (d->windowCapture)
        d->windowCapture->stop();
    if (d->process) {
        d->process->kill();
        d->process->waitForFinished(1000);
    }
    if (d->webcamProcess) {
        d->webcamProcess->kill();
        d->webcamProcess->waitForFinished(1000);
    }
    if (d->micProcess) {
        d->micProcess->kill();
        d->micProcess->waitForFinished(1000);
    }
    if (d->concat) {
        d->concat->kill();
        d->concat->waitForFinished(1000);
    }
    if (d->systemAudio)
        d->systemAudio->stop();
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

    if (request.region.width() < 2 || request.region.height() < 2) {
        error = QStringLiteral("The recording area must be at least 2 by 2 pixels.");
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

    {
        const qint64 freeBytes = d->freeRecordingBytes();
        if (freeBytes >= 0 && freeBytes < kRecordingDiskReserveBytes) {
            QDir(d->workDir).removeRecursively();
            d->workDir.clear();
            error = QStringLiteral("Not enough free disk space to record. ApexShot needs at "
                                   "least %1 MB free; %2 MB is free.")
                        .arg(kRecordingDiskReserveBytes / (1024 * 1024))
                        .arg(freeBytes / (1024 * 1024));
            return false;
        }
    }

    QStringList warnings;
    d->audioArguments = d->buildAudioArguments(warnings);

    // A camera that cannot be opened must not take the recording down with it:
    // probe it first and record the desktop alone when it does not answer.
    d->webcamActive = false;
    d->webcamCameraSize = QSize();
    d->webcamCaptureSize = QSize();
    d->webcamPixelFormat.clear();
    d->webcamFps = 30;
    d->webcamPreviewSize = QSize();
    d->webcamPreviewBuffer.clear();
    d->webcamPreviewLogged = false;
    d->webcamSegments.clear();
    d->webcamTrack = Platform::WebcamTrack{};
    QString camera;
    recordingLog(QStringLiteral("webcam request enabled=%1 device=%2")
                     .arg(d->request.webcamEnabled ? 1 : 0)
                     .arg(d->request.webcamDevice));
    if (d->request.webcamEnabled && d->request.webcamStreamUrl.isEmpty()) {
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
    if (!d->request.webcamStreamUrl.isEmpty() && d->request.webcamEnabled) {
        // The setup preview already owns the camera and carries its negotiated
        // format, so the track reuses that stream instead of reopening the
        // device for a second, colder view of the same picture.
        recordingLog(QStringLiteral("webcam stream reuse requested"));
        d->webcamFps = std::min(d->request.fps, 30);
        d->webcamActive = true;
    } else if (!camera.isEmpty()) {
        const Platform::WebcamCameraFormat captureFormat = cameraCaptureFormat(camera);
        QString pixelFormat = captureFormat.pixelFormat;
        CameraProbe probe = probeCamera(camera, captureFormat.size, pixelFormat);
        if (probe.size.isEmpty() && !pixelFormat.isEmpty()) {
            // A driver can list a format it then refuses to open; ask for its
            // default rather than lose the track over the pin.
            pixelFormat.clear();
            probe = probeCamera(camera, captureFormat.size, {});
        }
        const QSize cameraSize = probe.size;
        const int cameraFps = probe.fps > 0.0 ? int(std::lround(probe.fps)) : 30;
        d->webcamFps = std::max(1, cameraFps);
        recordingLog(
            QStringLiteral("webcam probe %1 -> %2x%3 at %4 fps (camera default) capture=%5x%6 %7")
                .arg(camera)
                .arg(cameraSize.width())
                .arg(cameraSize.height())
                .arg(cameraFps)
                .arg(captureFormat.size.width())
                .arg(captureFormat.size.height())
                .arg(captureFormat.mjpeg ? QStringLiteral("mjpeg") : pixelFormat));
        if (cameraSize.isEmpty()) {
            warnings.append(QStringLiteral("The camera \"%1\" could not be opened; recording "
                                           "without a webcam track.")
                                .arg(camera));
            d->request.webcamDevice.clear();
        } else {
            d->webcamActive = true;
            d->webcamCameraSize = cameraSize;
            d->webcamCaptureSize = captureFormat.size;
            d->webcamPixelFormat = pixelFormat;
            d->webcamPreviewSize = webcamPreviewFrameSize(d->request.webcamLayout.shape);
        }
    }
    if (d->webcamActive) {
        d->webcamTrack.x = std::clamp(d->request.webcamLayout.x, 0.0, 1.0);
        d->webcamTrack.y = std::clamp(d->request.webcamLayout.y, 0.0, 1.0);
        d->webcamTrack.w = std::clamp(d->request.webcamLayout.w, 0.0, 1.0);
        d->webcamTrack.h = std::clamp(d->request.webcamLayout.h, 0.0, 1.0);
        d->webcamTrack.shape = std::clamp(d->request.webcamLayout.shape, 0, 3);
        d->webcamTrack.flip = d->request.webcamLayout.flip;
    }

    for (const QString& warning : warnings) {
        recordingLog(QStringLiteral("warning: %1").arg(warning));
        qWarning().noquote() << "[recording]" << warning;
    }
    if (!warnings.isEmpty())
        emit warning(warnings.join(QLatin1Char('\n')));

    d->segments.clear();
    d->segmentIndex = 0;
    d->windowCaptureDisabled = false;
    d->elapsedBeforeMs = 0;
    d->processOutput.clear();
    d->discardRequested = false;
    d->resumeAfterPause = false;
    d->finishedEmitted = false;
    d->diskCheckTicks = 0;
    d->lowDiskStopped = false;
    d->systemAudio.reset();
    d->systemAudioPath.clear();
    d->systemAudioOffsetMs = 0;
    d->micPath.clear();
    d->micOffsetMs = 0;
    d->micPauseStartMs = -1;
    d->micPauseSpans.clear();

    QElapsedTimer audioClock;
    audioClock.start();
    if (d->request.systemAudio != RecordingRequest::SystemAudio::Off) {
        const QString audioPath = QDir(d->workDir).filePath(QStringLiteral("system-audio.wav"));
        auto capture = std::make_unique<SystemAudioCapture>();
        QString audioError;
        const SystemAudioCapture::Mode mode =
            d->request.systemAudio == RecordingRequest::SystemAudio::Application
                ? SystemAudioCapture::Mode::Application
                : SystemAudioCapture::Mode::AllApps;
        if (capture->start(mode, d->request.systemAudioProcessId, audioPath, &audioError)) {
            d->systemAudio = std::move(capture);
            d->systemAudioPath = audioPath;
            recordingLog(QStringLiteral("system audio: started %1 mode=%2 pid=%3")
                             .arg(audioPath)
                             .arg(static_cast<int>(mode))
                             .arg(d->request.systemAudioProcessId));
        } else {
            recordingLog(QStringLiteral("system audio: start failed: %1").arg(audioError));
            const QString source = d->request.systemAudioApplication.isEmpty()
                ? QStringLiteral("system audio")
                : d->request.systemAudioApplication;
            qWarning().noquote() << "[recording] system audio unavailable:" << audioError;
            warnings.append(QStringLiteral("The %1 could not be recorded: %2")
                                .arg(source, audioError));
            QFile::remove(audioPath);
        }
    }

    // The microphone records in its own process writing raw PCM, muxed in
    // once the recording is finalised: a dshow audio input inside the
    // capture process starves the real-time video of frames.
    if (!d->audioArguments.isEmpty()) {
        const QString path = QDir(d->workDir).filePath(QStringLiteral("mic.wav"));
        QStringList micArgs{QStringLiteral("-y"), QStringLiteral("-hide_banner"),
                            QStringLiteral("-loglevel"), QStringLiteral("error"),
                            QStringLiteral("-nostats")};
        micArgs << d->audioArguments << QStringLiteral("-vn") << QStringLiteral("-c:a")
                << QStringLiteral("pcm_s16le") << path;
        auto* mic = new QProcess(this);
        mic->setProgram(ffmpegExecutable());
        mic->setProcessChannelMode(QProcess::SeparateChannels);
        mic->setArguments(micArgs);
        QObject::connect(mic, &QProcess::readyReadStandardError, this, [d = d, mic] {
            d->processOutput.append(mic->readAllStandardError());
        });
        QObject::connect(mic,
                         QOverload<int, QProcess::ExitStatus>::of(&QProcess::finished),
                         this,
                         [d = d](int, QProcess::ExitStatus) { d->onMicFinished(); });
        mic->start();
        if (!mic->waitForStarted(kProbeTimeoutMs)) {
            recordingLog(QStringLiteral("microphone process could not start: %1")
                             .arg(mic->errorString()));
            emit warning(QStringLiteral("The microphone could not be recorded: %1")
                             .arg(mic->errorString()));
            delete mic;
        } else {
            bindToRecorderLifetime(mic);
            d->micProcess = mic;
            d->micPath = path;
            recordingLog(QStringLiteral("microphone: started %1").arg(path));
        }
    }

    d->recordingStartedAtUnixMs = QDateTime::currentMSecsSinceEpoch();
    d->manifestSessions.clear();
    d->markers.clear();
    d->manifestSessionOpen = false;
    d->firstScreenStartUnixMs = 0;
    d->cameraProcessDelayMs = -1;
    d->manifestWindow = d->request.target == RecordingRequest::Target::Window
        ? windowIdentity(d->request.windowId)
        : WindowIdentity{};
    if (!d->startSegment(error)) {
        if (d->systemAudio)
            d->systemAudio->stop();
        if (d->micProcess) {
            d->micProcess->kill();
            d->micProcess->deleteLater();
            d->micProcess = nullptr;
        }
        d->cleanupWorkDir();
        return false;
    }
    d->systemAudioOffsetMs = audioClock.elapsed();
    d->micOffsetMs = audioClock.elapsed();

    d->state = Private::State::Recording;
    d->active.start();
    d->openManifestSession();
    d->pointerTracker.start(d->request.region,
                            d->request.fps,
                            [d = d] { return d->elapsedMs(); },
                            d->request.captureKeystrokes);
    d->flushRecoveryState();
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
    d->closeManifestSession();
    d->pointerTracker.pause(d->elapsedBeforeMs);
    // The mic keeps recording through the pause; the span is cut at mux time.
    d->micPauseStartMs = d->elapsedBeforeMs;
    d->pauseWallMs = QDateTime::currentMSecsSinceEpoch();
    d->flushRecoveryState();
    if (d->systemAudio)
        d->systemAudio->setPaused(true);
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

    {
        const qint64 freeBytes = d->freeRecordingBytes();
        if (freeBytes >= 0 && freeBytes < kRecordingDiskReserveBytes) {
            d->stopForLowDisk(freeBytes);
            return;
        }
    }

    QString error;
    if (!d->startSegment(error)) {
        d->fail(error);
        return;
    }

    d->state = Private::State::Recording;
    d->active.start();
    d->openManifestSession();
    d->pointerTracker.resume();
    if (d->micPauseStartMs >= 0) {
        const qint64 paused = QDateTime::currentMSecsSinceEpoch() - d->pauseWallMs;
        d->micPauseSpans.append({d->micPauseStartMs, d->micPauseStartMs + paused});
        d->micPauseStartMs = -1;
    }
    d->flushRecoveryState();
    if (d->systemAudio)
        d->systemAudio->setPaused(false);
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
    d->closeManifestSession();
    d->pointerTracker.stop(d->elapsedBeforeMs);
    if (d->systemAudio)
        d->systemAudio->stop();
    if (d->micProcess) {
        d->micProcess->write("q\n");
        d->micProcess->closeWriteChannel();
        QProcess* target = d->micProcess;
        QTimer::singleShot(kStopTimeoutMs, this, [d = d, target] {
            if (d->micProcess == target)
                d->micProcess->kill();
        });
    }
    d->tick->stop();
    d->discardRequested = false;
    d->resumeAfterPause = false;
    d->state = Private::State::Stopping;
    emit stateChanged();
    emit elapsedChanged(d->elapsedBeforeMs);

    if (d->process)
        d->requestQuit(kStopTimeoutMs);
    else {
        emit captureStopped();
        d->finalize();
    }
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
    if (d->systemAudio)
        d->systemAudio->stop();
    if (d->micProcess)
        d->micProcess->kill();
    d->tick->stop();
    d->discardRequested = true;
    d->resumeAfterPause = false;
    d->state = Private::State::Stopping;
    emit stateChanged();

    if (d->process)
        d->requestQuit(kDiscardTimeoutMs);
    else {
        emit captureStopped();
        d->cleanupAndEmit(false, QString(), QString());
    }
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

void RecordingSession::setWebcamLayout(const WebcamLayout& layout)
{
    if (!d || !d->webcamActive)
        return;
    d->request.webcamLayout = layout;
    // The sidecar keeps its own copy for the editor; if the bake fails, that
    // copy is what places the camera over the recording.
    d->webcamTrack.x = std::clamp(layout.x, 0.0, 1.0);
    d->webcamTrack.y = std::clamp(layout.y, 0.0, 1.0);
    d->webcamTrack.w = std::clamp(layout.w, 0.0, 1.0);
    d->webcamTrack.h = std::clamp(layout.h, 0.0, 1.0);
}

QVector<UnfinishedRecording> RecordingSession::unfinishedRecordings(bool includeOwnProcess)
{
    QVector<UnfinishedRecording> found;
    const QDir temp(QDir::tempPath());
    const qint64 ownPid = QCoreApplication::applicationPid();
    for (const QString& name :
         temp.entryList({QStringLiteral("apexshot-recording-*")}, QDir::Dirs | QDir::NoDotAndDotDot)) {
        const QString workDir = temp.filePath(name);
        QFile stateFile(QDir(workDir).filePath(kStateFileName));
        if (!stateFile.open(QIODevice::ReadOnly))
            continue;
        const QJsonObject state = QJsonDocument::fromJson(stateFile.readAll()).object();
        stateFile.close();
        if (state.isEmpty())
            continue;
        const qint64 pid = int64From(state.value(QStringLiteral("pid")));
        if (pid == ownPid ? !includeOwnProcess : recorderProcessRunning(pid))
            continue;
        bool hasVideo = false;
        for (const QJsonValue& segment : state.value(QStringLiteral("segments")).toArray()) {
            if (QFileInfo(QDir(workDir).filePath(segment.toString())).size() > 0)
                hasVideo = true;
        }
        if (!hasVideo) {
            QDir(workDir).removeRecursively();
            continue;
        }
        UnfinishedRecording recording;
        recording.workDir = workDir;
        recording.outputPath = state.value(QStringLiteral("outputPath")).toString();
        recording.startedAtUnixMs = int64From(state.value(QStringLiteral("startedAtUnixMs")));
        recording.durationMs = int64From(state.value(QStringLiteral("elapsedMs")));
        found.append(recording);
    }
    std::sort(found.begin(), found.end(), [](const auto& left, const auto& right) {
        return left.startedAtUnixMs < right.startedAtUnixMs;
    });
    return found;
}

void RecordingSession::discardUnfinished(const QString& workDir)
{
    // Only ever delete one of our own work directories.
    if (QFileInfo(workDir).fileName().startsWith(QStringLiteral("apexshot-recording-"))
        && QFileInfo::exists(QDir(workDir).filePath(kStateFileName)))
        QDir(workDir).removeRecursively();
}

bool RecordingSession::recover(const QString& workDir, QString& error)
{
    if (d->state != Private::State::Idle && d->state != Private::State::Failed) {
        error = QStringLiteral("A recording is already running.");
        return false;
    }
    QFile stateFile(QDir(workDir).filePath(kStateFileName));
    if (!stateFile.open(QIODevice::ReadOnly)) {
        error = QStringLiteral("The unfinished recording's state could not be read.");
        return false;
    }
    const QJsonObject state = QJsonDocument::fromJson(stateFile.readAll()).object();
    stateFile.close();
    if (state.isEmpty()) {
        error = QStringLiteral("The unfinished recording's state is damaged.");
        return false;
    }

    const QDir dir(workDir);
    const auto existing = [&dir](const QJsonValue& value) {
        QStringList paths;
        for (const QJsonValue& entry : value.toArray()) {
            const QString path = dir.filePath(entry.toString());
            if (QFileInfo(path).size() > 0)
                paths.append(path);
        }
        return paths;
    };

    const QJsonObject requestObject = state.value(QStringLiteral("request")).toObject();
    RecordingRequest request;
    request.target = static_cast<RecordingRequest::Target>(
        requestObject.value(QStringLiteral("target")).toInt());
    request.region = rectFrom(requestObject.value(QStringLiteral("region")));
    request.windowId = static_cast<quintptr>(
        requestObject.value(QStringLiteral("windowId")).toString().toULongLong());
    request.outputPath =
        recoveredOutputPath(state.value(QStringLiteral("outputPath")).toString());
    request.includeCursor = false;
    request.fps = requestObject.value(QStringLiteral("fps")).toInt(30);
    request.displayScale = requestObject.value(QStringLiteral("displayScale")).toDouble(1.0);
    request.crf = requestObject.value(QStringLiteral("crf")).toInt(20);
    request.hevc = requestObject.value(QStringLiteral("hevc")).toBool();
    request.maxWidth = requestObject.value(QStringLiteral("maxWidth")).toInt();
    request.maxHeight = requestObject.value(QStringLiteral("maxHeight")).toInt();
    request.monoAudio = requestObject.value(QStringLiteral("monoAudio")).toBool();
    request.captureKeystrokes = requestObject.value(QStringLiteral("captureKeystrokes")).toBool();
    request.microphoneDevice = requestObject.value(QStringLiteral("microphoneDevice")).toString();
    request.systemAudio = static_cast<RecordingRequest::SystemAudio>(
        requestObject.value(QStringLiteral("systemAudio")).toInt());
    request.systemAudioApplication =
        requestObject.value(QStringLiteral("systemAudioApplication")).toString();
    request.webcamDevice = requestObject.value(QStringLiteral("webcamDevice")).toString();
    const QJsonObject layout = requestObject.value(QStringLiteral("webcamLayout")).toObject();
    request.webcamLayout.x = layout.value(QStringLiteral("x")).toDouble();
    request.webcamLayout.y = layout.value(QStringLiteral("y")).toDouble();
    request.webcamLayout.w = layout.value(QStringLiteral("w")).toDouble();
    request.webcamLayout.h = layout.value(QStringLiteral("h")).toDouble();
    request.webcamLayout.shape = layout.value(QStringLiteral("shape")).toInt();
    request.webcamLayout.flip = layout.value(QStringLiteral("flip")).toBool();
    if (request.outputPath.isEmpty()) {
        error = QStringLiteral("The unfinished recording has no output path.");
        return false;
    }

    d->request = request;
    d->workDir = workDir;
    d->existingSidecarPath.clear();
    d->segments = existing(state.value(QStringLiteral("segments")));
    if (d->segments.isEmpty()) {
        d->workDir.clear();
        error = QStringLiteral("The unfinished recording captured no video.");
        return false;
    }
    d->webcamSegments = existing(state.value(QStringLiteral("webcamSegments")));
    d->webcamActive =
        state.value(QStringLiteral("webcamActive")).toBool() && !d->webcamSegments.isEmpty();
    d->webcamCameraSize = QSize(state.value(QStringLiteral("webcamCameraWidth")).toInt(),
                                state.value(QStringLiteral("webcamCameraHeight")).toInt());
    d->webcamFps = state.value(QStringLiteral("webcamFps")).toInt(30);
    d->webcamTrack = Platform::WebcamTrack{};
    if (d->webcamActive) {
        d->webcamTrack.x = std::clamp(request.webcamLayout.x, 0.0, 1.0);
        d->webcamTrack.y = std::clamp(request.webcamLayout.y, 0.0, 1.0);
        d->webcamTrack.w = std::clamp(request.webcamLayout.w, 0.0, 1.0);
        d->webcamTrack.h = std::clamp(request.webcamLayout.h, 0.0, 1.0);
        d->webcamTrack.shape = std::clamp(request.webcamLayout.shape, 0, 3);
        d->webcamTrack.flip = request.webcamLayout.flip;
    }

    const QString micName = state.value(QStringLiteral("micPath")).toString();
    d->micPath = micName.isEmpty() ? QString() : dir.filePath(micName);
    d->micOffsetMs = int64From(state.value(QStringLiteral("micOffsetMs")));
    d->micPauseSpans.clear();
    for (const QJsonValue& span : state.value(QStringLiteral("micPauseSpans")).toArray()) {
        const QJsonArray pair = span.toArray();
        d->micPauseSpans.append({int64From(pair.at(0)), int64From(pair.at(1))});
    }
    // A recorder that died while paused kept the microphone running to the
    // end; everything after the pause began is cut.
    const qint64 openPause = int64From(state.value(QStringLiteral("micPauseStartMs")));
    if (openPause >= 0)
        d->micPauseSpans.append({openPause, openPause + 24LL * 60 * 60 * 1000});
    d->micPauseStartMs = -1;
    const QString systemName = state.value(QStringLiteral("systemAudioPath")).toString();
    d->systemAudioPath = systemName.isEmpty() ? QString() : dir.filePath(systemName);
    d->systemAudioOffsetMs = int64From(state.value(QStringLiteral("systemAudioOffsetMs")));
    d->systemAudio.reset();

    d->elapsedBeforeMs = int64From(state.value(QStringLiteral("elapsedMs")));
    // The state file can be seconds older than the crash; the segments are
    // what was actually captured.
    qint64 capturedMs = 0;
    for (const QString& segment : d->segments)
        capturedMs += qRound64(videoDuration(segment) * 1000.0);
    d->elapsedBeforeMs = std::max(d->elapsedBeforeMs, capturedMs);
    d->recordingStartedAtUnixMs = int64From(state.value(QStringLiteral("startedAtUnixMs")));
    d->manifestSessions.clear();
    for (const QJsonValue& value : state.value(QStringLiteral("sessions")).toArray()) {
        const QJsonObject object = value.toObject();
        RecordingManifest::Session session;
        session.startedAtUnixMs = int64From(object.value(QStringLiteral("startedAtUnixMs")));
        session.endedAtUnixMs = int64From(object.value(QStringLiteral("endedAtUnixMs")));
        session.startMs = int64From(object.value(QStringLiteral("startMs")));
        session.endMs = int64From(object.value(QStringLiteral("endMs")));
        session.windowBounds = rectFrom(object.value(QStringLiteral("windowBounds")));
        if (session.endedAtUnixMs == 0) {
            session.endMs = d->elapsedBeforeMs;
            session.endedAtUnixMs = session.startedAtUnixMs + (session.endMs - session.startMs);
        }
        d->manifestSessions.append(session);
    }
    d->markers.clear();
    for (const QJsonValue& value : state.value(QStringLiteral("markers")).toArray())
        d->markers.append(int64From(value));
    d->manifestSessionOpen = false;
    const QJsonObject window = state.value(QStringLiteral("window")).toObject();
    d->manifestWindow.title = window.value(QStringLiteral("title")).toString();
    d->manifestWindow.app = window.value(QStringLiteral("app")).toString();
    d->manifestWindow.bounds = rectFrom(window.value(QStringLiteral("bounds")));
    const QJsonObject capture = state.value(QStringLiteral("capture")).toObject();
    d->manifestCaptureBackend = capture.value(QStringLiteral("backend")).toString();
    d->manifestEncoder = capture.value(QStringLiteral("encoder")).toString();
    d->manifestOutputSize = QSize(capture.value(QStringLiteral("width")).toInt(),
                                  capture.value(QStringLiteral("height")).toInt());
    d->manifestCrf = capture.value(QStringLiteral("crf")).toInt();
    d->cameraProcessDelayMs = -1;

    // The pointer sidecar is copied out of the work directory, which the
    // normal finish deletes before the sidecar is installed.
    d->recoveredSidecarPath.clear();
    const QString pointerPath = dir.filePath(kPointerFileName);
    if (QFileInfo::exists(pointerPath)) {
        const QString holding = QDir(QDir::tempPath())
                                    .filePath(QStringLiteral("apexshot-recovered-pointer-%1")
                                                  .arg(QDateTime::currentMSecsSinceEpoch()));
        if (QDir().mkpath(holding)) {
            QFile::copy(pointerPath, QDir(holding).filePath(kPointerFileName));
            const QDir cursors(pointerPath + QStringLiteral(".cursors"));
            if (cursors.exists()) {
                const QString target = QDir(holding).filePath(kPointerFileName + QStringLiteral(".cursors"));
                QDir().mkpath(target);
                for (const QString& file : cursors.entryList(QDir::Files))
                    QFile::copy(cursors.filePath(file), QDir(target).filePath(file));
            }
            d->recoveredSidecarPath = QDir(holding).filePath(kPointerFileName);
        }
    }

    d->processOutput.clear();
    d->discardRequested = false;
    d->resumeAfterPause = false;
    d->finishedEmitted = false;
    d->lowDiskStopped = true;
    d->state = Private::State::Stopping;
    recordingLog(QStringLiteral("recovery: %1 segments from %2 -> %3")
                     .arg(d->segments.size())
                     .arg(workDir, d->request.outputPath));
    emit stateChanged();
    QTimer::singleShot(0, this, [d = d] { d->finalize(); });
    return true;
}

void RecordingSession::addMarker()
{
    if (d->state != Private::State::Recording && d->state != Private::State::Paused
        && d->state != Private::State::Pausing)
        return;
    const qint64 at = d->elapsedMs();
    d->markers.append(at);
    recordingLog(QStringLiteral("marker at %1 ms").arg(at));
    d->flushRecoveryState();
    emit markerAdded(at);
}

void RecordingSession::abandonForTesting()
{
    if (d->state != Private::State::Recording && d->state != Private::State::Paused)
        return;
    d->flushRecoveryState();
    d->tick->stop();
    if (d->windowCapture)
        d->windowCapture->stop();
    for (QProcess** target : {&d->process, &d->webcamProcess, &d->micProcess}) {
        if (!*target)
            continue;
        QObject::disconnect(*target, nullptr, this, nullptr);
        (*target)->kill();
        (*target)->waitForFinished(2000);
        delete *target;
        *target = nullptr;
    }
    if (d->systemAudio)
        d->systemAudio->stop();
    d->systemAudio.reset();
    d->pointerTracker.discard();
    d->state = Private::State::Idle;
    d->workDir.clear();
}

bool RecordingSession::webcamActive() const
{
    return d && d->webcamActive;
}

} // namespace Platform
