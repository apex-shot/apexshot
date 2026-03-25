#include "ScrollControlPanel.h"
#include <QPainter>
#include <QPainterPath>
#include <QHBoxLayout>
#include <QRect>
#include <QSize>

ScrollControlPanel::ScrollControlPanel(QWidget* parent)
    : QWidget(parent)
{
    setWindowFlags(Qt::FramelessWindowHint
                   | Qt::WindowStaysOnTopHint
                   | Qt::BypassWindowManagerHint
                   | Qt::Tool);
    setAttribute(Qt::WA_TranslucentBackground, true);
    setAttribute(Qt::WA_ShowWithoutActivating, true);
    setFixedHeight(56);

    auto* layout = new QHBoxLayout(this);
    layout->setContentsMargins(14, 8, 14, 8);
    layout->setSpacing(10);

    m_statusLabel = new QLabel(QStringLiteral("Capturing..."), this);
    m_statusLabel->setStyleSheet(
        "color: white; font-size: 13px; font-weight: bold;"
    );

    m_frameLabel = new QLabel(QStringLiteral("0 frames"), this);
    m_frameLabel->setStyleSheet(
        "color: rgba(255,255,255,180); font-size: 12px;"
    );

    m_cancelBtn = new QPushButton(QStringLiteral("Cancel"), this);
    m_cancelBtn->setStyleSheet(
        "QPushButton {"
        "  background: rgba(255,255,255,30);"
        "  color: white;"
        "  border: 1px solid rgba(255,255,255,60);"
        "  border-radius: 8px;"
        "  padding: 6px 18px;"
        "  font-size: 12px;"
        "  font-weight: bold;"
        "}"
        "QPushButton:hover {"
        "  background: rgba(255,60,60,120);"
        "  border-color: rgba(255,100,100,160);"
        "}"
    );

    m_doneBtn = new QPushButton(QStringLiteral("Done"), this);
    m_doneBtn->setStyleSheet(
        "QPushButton {"
        "  background: rgba(0,122,255,140);"
        "  color: white;"
        "  border: 1px solid rgba(100,180,255,160);"
        "  border-radius: 8px;"
        "  padding: 6px 18px;"
        "  font-size: 12px;"
        "  font-weight: bold;"
        "}"
        "QPushButton:hover {"
        "  background: rgba(0,122,255,200);"
        "  border-color: rgba(130,200,255,200);"
        "}"
    );

    layout->addWidget(m_statusLabel);
    layout->addWidget(m_frameLabel);
    layout->addStretch();
    layout->addWidget(m_cancelBtn);
    layout->addWidget(m_doneBtn);

    connect(m_cancelBtn, &QPushButton::clicked, this, &ScrollControlPanel::cancelClicked);
    connect(m_doneBtn, &QPushButton::clicked, this, &ScrollControlPanel::doneClicked);

    setMinimumWidth(400);
}

void ScrollControlPanel::paintEvent(QPaintEvent*)
{
    QPainter p(this);
    p.setRenderHint(QPainter::Antialiasing);
    QPainterPath path;
    path.addRoundedRect(rect().adjusted(1, 1, -1, -1), 12, 12);
    p.fillPath(path, QColor(20, 20, 24, 220));
    p.setPen(QPen(QColor(255, 255, 255, 40), 1));
    p.drawPath(path);
}
void ScrollControlPanel::setFrameCount(int count)
{
    m_frameLabel->setText(
        QString("%1 frame%2").arg(count).arg(count != 1 ? "s" : "")
    );
}

void ScrollControlPanel::setStatus(const QString& text)
{
    m_statusLabel->setText(text);
}

void ScrollControlPanel::setCapturingDone()
{
    m_statusLabel->setText(QStringLiteral("Capture complete"));
    m_statusLabel->setStyleSheet(
        "color: #6fdf6f; font-size: 13px; font-weight: bold;"
    );
}

void ScrollControlPanel::positionNear(const QRect& captureArea, const QSize& screenSize)
{
    int panelW = std::max(minimumWidth(), sizeHint().width());
    int panelH = height();

    // Position below the capture area, centered horizontally
    int x = captureArea.x() + (captureArea.width() - panelW) / 2;
    int y = captureArea.bottom() + 16;

    x = std::max(16, std::min(x, screenSize.width() - panelW - 16));
    y = std::min(y, screenSize.height() - panelH - 16);
    y = std::max(16, y);

    setGeometry(x, y, panelW, panelH);
}
