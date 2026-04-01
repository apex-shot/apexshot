#pragma once

#include <QWidget>
#include <QPixmap>
#include <QList>
#include <QString>
#include <QRect>

struct AppWindowInfo {
    QString title;
    QString appName;
    QString wmClass;
    QRect   rect;
    QPixmap icon;
    quint64 xid;
};

class WindowPickerOverlay : public QWidget
{
    Q_OBJECT

public:
    explicit WindowPickerOverlay(QWidget* parent = nullptr);
    void focusAndRaiseOverlay();

    bool wasSelected() const { return m_selected; }
    AppWindowInfo selectedWindow() const { return m_selectedWindow; }

protected:
    void paintEvent(QPaintEvent* event) override;
    void resizeEvent(QResizeEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void keyPressEvent(QKeyEvent* event) override;

private:
    void drawToolbar(QPainter& p);
    QRectF toolbarItemRect(int i) const;
    void recomputeThumbnailLayout();

    QPixmap              m_background;
    QList<AppWindowInfo> m_windows;
    QList<QRect>         m_thumbnailRects;
    int                  m_hoveredIdx    = -1;
    int                  m_hoveredTool   = -1;
    bool                 m_selected      = false;
    AppWindowInfo        m_selectedWindow;
    mutable QRectF       m_toolbarRect;  // cached
};
