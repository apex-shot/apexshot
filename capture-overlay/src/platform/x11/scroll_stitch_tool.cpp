#include "platform/x11/scroll_stitcher.h"

#include <QCoreApplication>
#include <QDir>
#include <QFileInfo>
#include <QImage>
#include <QStringList>
#include <QTextStream>

#include <cstdio>

namespace {

const char* methodName(Platform::ScrollStitcher::Method method)
{
    switch (method) {
    case Platform::ScrollStitcher::Method::Exact:
        return "exact";
    case Platform::ScrollStitcher::Method::Fuzzy:
        return "fuzzy";
    case Platform::ScrollStitcher::Method::Skipped:
        return "skipped";
    case Platform::ScrollStitcher::Method::None:
        break;
    }
    return "none";
}

} // namespace

int main(int argc, char* argv[])
{
    QCoreApplication app(argc, argv);
    const QStringList args = app.arguments();
    if (args.size() != 3) {
        std::fprintf(stderr, "usage: apexshot-scroll-stitch <frames-dir> <out.png>\n");
        return 2;
    }

    const QString framesDir = args.at(1);
    const QString outPath = args.at(2);

    QDir dir(framesDir);
    if (!dir.exists()) {
        std::fprintf(stderr,
                     "apexshot-scroll-stitch: no such directory: %s\n",
                     framesDir.toLocal8Bit().constData());
        return 2;
    }

    QStringList names = dir.entryList(QStringList{QStringLiteral("*.png")}, QDir::Files, QDir::Name);
    names.removeAll(QStringLiteral("result.png"));
    names.removeAll(QFileInfo(outPath).fileName());
    if (names.isEmpty()) {
        std::fprintf(stderr,
                     "apexshot-scroll-stitch: no frames in %s\n",
                     framesDir.toLocal8Bit().constData());
        return 2;
    }

    Platform::ScrollStitcher stitcher;
    for (const QString& name : names) {
        QImage frame(dir.filePath(name));
        if (frame.isNull()) {
            std::fprintf(stderr,
                         "apexshot-scroll-stitch: skipping unreadable frame %s\n",
                         name.toLocal8Bit().constData());
            continue;
        }
        stitcher.addFrame(frame.convertToFormat(QImage::Format_RGBA8888));
        if (stitcher.reachedEnd())
            break;
    }

    const QImage result = stitcher.result();
    if (result.isNull()) {
        std::fprintf(stderr, "apexshot-scroll-stitch: no usable frames\n");
        return 2;
    }
    if (!result.save(outPath, "PNG")) {
        std::fprintf(stderr,
                     "apexshot-scroll-stitch: could not write %s\n",
                     outPath.toLocal8Bit().constData());
        return 2;
    }

    QTextStream out(stdout);
    out << "frames=" << stitcher.frames()
        << " header=" << stitcher.headerRows()
        << " footer=" << stitcher.footerRows()
        << " scrollbar=" << stitcher.scrollbarColumns()
        << " size=" << result.width() << "x" << result.height() << "\n";
    for (const Platform::ScrollStitcher::Join& join : stitcher.joins()) {
        out << "join " << join.index
            << " method=" << methodName(join.method)
            << " overlap=" << join.overlap
            << " advance=" << join.advance
            << " expected=" << join.expectedOverlap
            << " score=" << QString::number(join.score, 'f', 2)
            << " forced=" << (join.forcedExpected ? 1 : 0)
            << " header=" << join.headerRows
            << " footer=" << join.footerRows
            << " scrollbar=" << join.scrollbarColumns << "\n";
    }
    out.flush();
    return 0;
}
