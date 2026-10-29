#include <QBuffer>
#include <QCoreApplication>
#include <QFile>
#include <QImage>
#include <QPainter>
#include <QTimer>

#include <fcntl.h>
#include <io.h>
#include <cstdio>

int main(int argc, char** argv)
{
    QCoreApplication app(argc, argv);
    if (app.arguments().contains(QStringLiteral("-list_options"))) {
        std::fprintf(stderr, "vcodec=mjpeg min s=640x480 fps=30 max s=640x480 fps=30\n");
        return 1;
    }
    if (app.arguments().contains(QStringLiteral("video=Unavailable camera")))
        return 1;
    _setmode(_fileno(stdout), _O_BINARY);
    QFile output;
    if (!output.open(stdout, QIODevice::WriteOnly))
        return 1;
    QImage image(640, 480, QImage::Format_RGB32);
    image.fill(QColor(235, 20, 20));
    QPainter painter(&image);
    painter.fillRect(QRect(320, 0, 320, 480), QColor(20, 20, 235));
    painter.end();
    QByteArray jpeg;
    QBuffer buffer(&jpeg);
    buffer.open(QIODevice::WriteOnly);
    if (!image.save(&buffer, "JPEG", 90))
        return 1;
    QTimer timer;
    QObject::connect(&timer, &QTimer::timeout, &app, [&] {
        output.write(jpeg);
        output.flush();
    });
    timer.start(33);
    return app.exec();
}
