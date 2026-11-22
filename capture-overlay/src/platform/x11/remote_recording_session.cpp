#include "platform/recording.h"

#include <QElapsedTimer>
#include <QSocketNotifier>
#include <QTimer>

#include <cerrno>
#include <cstdio>

#include <unistd.h>

namespace {

constexpr int kTickMs = 200;

/// One protocol line on stdout. The parent reads the bar's commands while the
/// recording runs, so each line is flushed as soon as it is written.
void writeProtocolLine(const char* line)
{
    std::fputs(line, stdout);
    std::fputc('\n', stdout);
    std::fflush(stdout);
}

} // namespace

namespace Platform {

/// The Linux controls helper never records. It mirrors the recording the Rust
/// process owns: commands leave on stdout, and the recorder's pause and resume
/// come back on stdin so the bar matches a change started by a shortcut or the
/// tray as well as by its own buttons.
class RecordingSession::Private
{
public:
    explicit Private(RecordingSession* owner)
        : q(owner)
    {
    }

    qint64 elapsedMs() const
    {
        return elapsedBeforeMs + ((active && !paused) ? clock.elapsed() : 0);
    }

    void readStdin()
    {
        char buffer[512];
        const ssize_t count = ::read(STDIN_FILENO, buffer, sizeof buffer);
        if (count <= 0) {
            if (count < 0 && errno == EINTR)
                return;
            input->setEnabled(false);
            finish();
            return;
        }

        pending.append(buffer, static_cast<int>(count));
        int newline = pending.indexOf('\n');
        while (newline >= 0) {
            const QByteArray line = pending.left(newline).trimmed();
            pending.remove(0, newline + 1);
            if (line == "paused")
                setPaused(true);
            else if (line == "resumed")
                setPaused(false);
            else if (line == "end")
                finish();
            newline = pending.indexOf('\n');
        }
    }

    /// Mirror a pause or resume decided by the recorder, without sending the
    /// command back to it.
    void setPaused(bool nowPaused)
    {
        if (!active || paused == nowPaused)
            return;
        if (nowPaused) {
            elapsedBeforeMs += clock.elapsed();
            paused = true;
        } else {
            paused = false;
            clock.restart();
        }
        emit q->stateChanged();
        emit q->elapsedChanged(elapsedMs());
    }

    /// The recorder has ended the recording (or closed the pipe); the bar only
    /// has to go away.
    void finish()
    {
        if (!active)
            return;
        active = false;
        if (!paused)
            elapsedBeforeMs += clock.elapsed();
        paused = false;
        tick.stop();
        emit q->stateChanged();
        emit q->finished(false, QString(), QString());
    }

    RecordingSession* q = nullptr;
    QTimer tick;
    QElapsedTimer clock;
    QSocketNotifier* input = nullptr;
    QByteArray pending;
    qint64 elapsedBeforeMs = 0;
    bool active = true;
    bool paused = false;
};

RecordingSession::RecordingSession(QObject* parent)
    : QObject(parent)
    , d(new Private(this))
{
    d->clock.start();
    d->tick.setInterval(kTickMs);
    connect(&d->tick, &QTimer::timeout, this, [this] {
        emit elapsedChanged(d->elapsedMs());
    });
    d->tick.start();

    d->input = new QSocketNotifier(STDIN_FILENO, QSocketNotifier::Read, this);
    connect(d->input, &QSocketNotifier::activated, this, [this] {
        d->readStdin();
    });
}

RecordingSession::~RecordingSession()
{
    delete d;
}

bool RecordingSession::available(QString* error)
{
    if (error)
        error->clear();
    return true;
}

bool RecordingSession::start(const RecordingRequest& request, QString& error)
{
    Q_UNUSED(request);
    error = QStringLiteral(
        "This helper only presents the controls bar; the recording lives in the main process.");
    return false;
}

void RecordingSession::pause()
{
    if (!d->active || d->paused)
        return;
    d->setPaused(true);
    writeProtocolLine("pause");
}

void RecordingSession::resume()
{
    if (!d->active || !d->paused)
        return;
    d->setPaused(false);
    writeProtocolLine("resume");
}

void RecordingSession::stop()
{
    if (!d->active)
        return;
    d->active = false;
    if (!d->paused)
        d->elapsedBeforeMs += d->clock.elapsed();
    d->paused = false;
    d->tick.stop();
    writeProtocolLine("stop");
    emit stateChanged();
    emit finished(true, QString(), QString());
}

void RecordingSession::discard()
{
    if (!d->active)
        return;
    d->active = false;
    if (!d->paused)
        d->elapsedBeforeMs += d->clock.elapsed();
    d->paused = false;
    d->tick.stop();
    writeProtocolLine("discard");
    emit stateChanged();
    emit finished(false, QString(), QString());
}

void RecordingSession::addMarker()
{
}

bool RecordingSession::isActive() const
{
    return d->active;
}

bool RecordingSession::isPaused() const
{
    return d->paused;
}

QString RecordingSession::outputPath() const
{
    return {};
}

void RecordingSession::setWebcamLayout(const WebcamLayout& layout)
{
    Q_UNUSED(layout);
}

bool RecordingSession::webcamActive() const
{
    return false;
}

QVector<UnfinishedRecording> RecordingSession::unfinishedRecordings(bool includeOwnProcess)
{
    Q_UNUSED(includeOwnProcess);
    return {};
}

bool RecordingSession::recover(const QString& workDir, QString& error)
{
    Q_UNUSED(workDir);
    error = QStringLiteral("Recovery is not available in the controls helper.");
    return false;
}

void RecordingSession::discardUnfinished(const QString& workDir)
{
    Q_UNUSED(workDir);
}

void RecordingSession::abandonForTesting()
{
}

QString ffmpegPath()
{
    return {};
}

} // namespace Platform
