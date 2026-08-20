#pragma once

#include <QColor>
#include <QElapsedTimer>
#include <QPointF>
#include <QQuickItem>
#include <QVariantList>
#include <QtQml/qqmlregistration.h>

#include <optional>

class QMouseEvent;
class QSGNode;
class QTouchEvent;

class AnnotationItem : public QQuickItem
{
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(bool annotationMode READ annotationMode WRITE setAnnotationMode NOTIFY annotationModeChanged)
    Q_PROPERTY(Tool tool READ tool WRITE setTool NOTIFY toolChanged)
    Q_PROPERTY(QColor strokeColor READ strokeColor WRITE setStrokeColor NOTIFY strokeColorChanged)
    Q_PROPERTY(qreal strokeWidth READ strokeWidth WRITE setStrokeWidth NOTIFY strokeWidthChanged)

public:
    enum Tool {
        Pen,
        Arrow,
        Rectangle,
        Spotlight,
    };
    Q_ENUM(Tool)

    explicit AnnotationItem(QQuickItem *parent = nullptr);

    bool annotationMode() const;
    void setAnnotationMode(bool enabled);

    Tool tool() const;
    void setTool(Tool tool);

    QColor strokeColor() const;
    void setStrokeColor(const QColor &color);

    qreal strokeWidth() const;
    void setStrokeWidth(qreal width);

    Q_INVOKABLE void startRecordingClock();
    Q_INVOKABLE bool undo();
    Q_INVOKABLE void clear();

signals:
    void annotationModeChanged();
    void toolChanged();
    void strokeColorChanged();
    void strokeWidthChanged();
    void strokeCommitted(
        const QVariantList &normalizedPoints,
        int tool,
        double startedAtSeconds,
        double endedAtSeconds
    );
    void passThroughRequested();

protected:
    QSGNode *updatePaintNode(QSGNode *oldNode, UpdatePaintNodeData *) override;
    void mousePressEvent(QMouseEvent *event) override;
    void mouseMoveEvent(QMouseEvent *event) override;
    void mouseReleaseEvent(QMouseEvent *event) override;
    void touchEvent(QTouchEvent *event) override;

private:
    struct Stroke {
        Tool tool = Pen;
        QColor color;
        qreal width = 1.0;
        QVector<QPointF> points;
        qint64 startedAtMilliseconds = 0;
    };

    QPointF normalized(const QPointF &position) const;
    void beginStroke(const QPointF &position);
    void extendStroke(const QPointF &position);
    void finishStroke(const QPointF &position);
    qint64 recordingMilliseconds() const;

    bool m_annotationMode = false;
    Tool m_tool = Pen;
    QColor m_strokeColor = QColor(QStringLiteral("#ffcc00"));
    qreal m_strokeWidth = 3.0;
    QVector<Stroke> m_strokes;
    std::optional<Stroke> m_activeStroke;
    QElapsedTimer m_recordingClock;
};
