#include "CaptureOverlay.h"
#include <QFile>
#include <QMenu>
#include <QAction>
#include <QImage>
#include <QPixmap>
#include <QMutexLocker>
#include <QRegularExpression>
#include <QString>
#include <QSet>
#include <gst/gst.h>
#include <gst/app/gstappsink.h>
#include <thread>

void CaptureOverlay::enumerateWebcamDevices()
{
    m_webcamDevices.clear();
    // Track names we've already added to deduplicate (same camera can have subdev nodes)
    QSet<QString> seenNames;

    for (int i = 0; i < 32; ++i) {
        QString devPath = QStringLiteral("/dev/video%1").arg(i);
        if (!QFile::exists(devPath)) continue;

        QString name;
        QFile nameFile(QStringLiteral("/sys/class/video4linux/video%1/name").arg(i));
        if (nameFile.open(QIODevice::ReadOnly)) {
            name = QString::fromUtf8(nameFile.readLine()).trimmed();
        } else {
            name = devPath;
        }

        // Skip metadata-only nodes
        if (name.contains("Metadata", Qt::CaseInsensitive)) continue;

        // Skip duplicate sub-devices from same physical camera (same name already seen)
        if (seenNames.contains(name)) continue;
        seenNames.insert(name);

        m_webcamDevices.append(QStringLiteral("%1 (%2)").arg(name, devPath));
    }
}

void CaptureOverlay::startWebcamCapture()
{
    stopWebcamCapture(); // stop any existing pipeline

    if (m_webcamDevice < 0) return;

    QString device = QStringLiteral("/dev/video%1").arg(m_webcamDevice);
    std::string pipelineStr = QStringLiteral(
        "v4l2src device=%1 ! video/x-raw,width=640,height=480,framerate=30/1 ! "
        "videoconvert ! video/x-raw,format=BGRA ! appsink name=sink emit-signals=true sync=false"
    ).arg(device).toStdString();

    GError* err = nullptr;
    GstElement* pipeline = gst_parse_launch(pipelineStr.c_str(), &err);
    if (err) {
        std::fprintf(stderr, "[CaptureOverlay] Webcam pipeline error: %s\n", err->message);
        g_error_free(err);
        return;
    }
    if (!pipeline) return;

    GstElement* sink = gst_bin_get_by_name(GST_BIN(pipeline), "sink");
    if (sink) {
        gst_app_sink_set_emit_signals(GST_APP_SINK(sink), TRUE);
        gst_app_sink_set_max_buffers(GST_APP_SINK(sink), 1);
        gst_object_unref(sink);
    }

    GstStateChangeReturn ret = gst_element_set_state(pipeline, GST_STATE_PLAYING);
    if (ret == GST_STATE_CHANGE_FAILURE) {
        std::fprintf(stderr, "[CaptureOverlay] Failed to start webcam pipeline\n");
        gst_object_unref(pipeline);
        return;
    }

    m_webcamPipeline = pipeline;
    std::fprintf(stderr, "[CaptureOverlay] Webcam capture started on %s\n", device.toLocal8Bit().constData());

    // Frame pull thread
    std::thread([this]() {
        GstElement* pipeline = static_cast<GstElement*>(m_webcamPipeline);
        if (!pipeline) return;
        GstElement* sink = gst_bin_get_by_name(GST_BIN(pipeline), "sink");
        if (!sink) return;

        while (m_webcamPipeline) {
            GstSample* sample = gst_app_sink_try_pull_sample(GST_APP_SINK(sink), 100 * GST_MSECOND);
            if (!sample) continue;

            GstBuffer* buffer = gst_sample_get_buffer(sample);
            GstCaps* caps = gst_sample_get_caps(sample);
            if (!buffer || !caps) {
                gst_sample_unref(sample);
                continue;
            }

            GstStructure* s = gst_caps_get_structure(caps, 0);
            int w = 0, h = 0;
            gst_structure_get_int(s, "width", &w);
            gst_structure_get_int(s, "height", &h);

            GstMapInfo map;
            if (gst_buffer_map(buffer, &map, GST_MAP_READ) && w > 0 && h > 0) {
                QImage img(map.data, w, h, QImage::Format_ARGB32);
                QPixmap frame = QPixmap::fromImage(img.copy());
                {
                    QMutexLocker lock(&m_webcamMutex);
                    m_webcamFrame = frame;
                }
                gst_buffer_unmap(buffer, &map);
            }
            gst_sample_unref(sample);

            // Trigger repaint from main thread
            QMetaObject::invokeMethod(this, "update", Qt::QueuedConnection);
        }
        gst_object_unref(sink);
    }).detach();
}

void CaptureOverlay::stopWebcamCapture()
{
    if (m_webcamPipeline) {
        GstElement* pipeline = static_cast<GstElement*>(m_webcamPipeline);
        m_webcamPipeline = nullptr; // signal thread to stop
        gst_element_set_state(pipeline, GST_STATE_NULL);
        gst_object_unref(pipeline);
        QMutexLocker lock(&m_webcamMutex);
        m_webcamFrame = QPixmap();
    }
}

void CaptureOverlay::showWebcamContextMenu(const QPoint& globalPos)
{
    // Always refresh device list to pick up newly connected cameras
    enumerateWebcamDevices();

    QMenu menu(this);
    
    // Keep the existing menu base styling, only warm up the hover state.
    menu.setStyleSheet(
        "QMenu {"
        "    background-color: rgba(30, 30, 30, 235);"
        "    border: 1px solid rgba(255, 255, 255, 40);"
        "    border-radius: 12px;"
        "    padding: 8px 4px;"
        "    color: #F1F1F3;" // Off-white
        "    font-family: 'Sans';"
        "    font-size: 13px;"
        "}"
        "QMenu::item {"
        "    padding: 6px 32px 6px 28px;"
        "    border-radius: 6px;"
        "    margin: 1px 4px;"
        "}"
        "QMenu::item:selected {"
        "    background-color: rgba(176, 92, 56, 220);"
        "    color: #FFEAD6;"
        "}"
        "QMenu::separator {"
        "    height: 1px;"
        "    background: rgba(255, 255, 255, 25);"
        "    margin: 6px 14px;"
        "}"
        "QMenu::item:disabled {"
        "    color: rgba(255, 255, 255, 110);"
        "    font-size: 11px;"
        "    font-weight: bold;"
        "    padding: 10px 14px 4px 14px;"
        "    background: transparent;"
        "}"
        "QMenu::indicator {"
        "    left: 8px;"
        "    width: 14px;"
        "    height: 14px;"
        "}"
    );

    // Helper: add a non-clickable section header
    auto addHeader = [&](const QString& title) {
        QAction* h = menu.addAction(title);
        h->setEnabled(false);
        h->setProperty("is_header", true);
    };

    // ── Camera Section ──
    addHeader("Camera");
    QAction* noneAct = menu.addAction("None");
    noneAct->setCheckable(true);
    noneAct->setChecked(m_webcamDevice == -1);
    noneAct->setData(-1);

    for (int i = 0; i < m_webcamDevices.size(); ++i) {
        QRegularExpression re(QStringLiteral("video(\\d+)"));
        QRegularExpressionMatch m = re.match(m_webcamDevices[i]);
        int devIdx = m.hasMatch() ? m.captured(1).toInt() : i;
        
        QAction* act = menu.addAction(m_webcamDevices[i]);
        act->setCheckable(true);
        act->setChecked(m_webcamDevice == devIdx);
        act->setData(devIdx);
    }

    // ── Size Section ──
    addHeader("Size");
    struct { const char* label; WebcamSize val; } sizes[] = {
        {"Small", WebcamSize::Small}, {"Medium", WebcamSize::Medium},
        {"Large", WebcamSize::Large}, {"Huge", WebcamSize::Huge}
    };
    for (auto& s : sizes) {
        QAction* a = menu.addAction(s.label);
        a->setCheckable(true);
        a->setChecked(m_webcamSize == s.val);
        a->setData((int)s.val);
        a->setProperty("is_size", true);
    }

    // ── Full Screen Section ──
    addHeader("Click on camera to toggle Full Screen");
    QAction* fullScreenAct = menu.addAction("Full Screen");
    fullScreenAct->setCheckable(true);
    fullScreenAct->setChecked(m_webcamSize == WebcamSize::Fullscreen);
    fullScreenAct->setData((int)WebcamSize::Fullscreen);
    fullScreenAct->setProperty("is_size", true);

    // ── Shape Section ──
    addHeader("Shape");
    struct { const char* label; WebcamShape val; } shapes[] = {
        {"Circle", WebcamShape::Circle}, {"Square", WebcamShape::Square},
        {"Rectangle", WebcamShape::Rectangle}, {"Vertical", WebcamShape::Vertical}
    };
    for (auto& s : shapes) {
        QAction* a = menu.addAction(s.label);
        a->setCheckable(true);
        a->setChecked(m_webcamShape == s.val);
        a->setData((int)s.val);
        a->setProperty("is_shape", true);
    }

    // ── Options Section ──
    addHeader("Options");
    QAction* flipAct = menu.addAction("Flip Camera");
    flipAct->setCheckable(true);
    flipAct->setChecked(m_webcamFlip);

    // ── Display and Handle Selection ──
    QAction* chosen = menu.exec(globalPos);
    if (!chosen) return;

    if (chosen == noneAct) {
        m_webcamDevice = -1;
        stopWebcamCapture();
    } else if (chosen == flipAct) {
        m_webcamFlip = !m_webcamFlip;
    } else if (chosen->property("is_size").toBool()) {
        m_webcamSize = (WebcamSize)chosen->data().toInt();
    } else if (chosen->property("is_shape").toBool()) {
        m_webcamShape = (WebcamShape)chosen->data().toInt();
    } else {
        // Camera device selection
        m_webcamDevice = chosen->data().toInt();
        if (!m_recWebcam) m_recWebcam = true;
        if (m_recordingPanelOpen)
            startWebcamCapture();
    }
    update();
}
