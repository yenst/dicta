#include "annotation_item.h"

#include <QMouseEvent>
#include <QSGFlatColorMaterial>
#include <QSGGeometry>
#include <QSGGeometryNode>
#include <QTouchEvent>

#include <algorithm>
#include <cmath>

namespace {
constexpr int SpotlightSegments = 48;

QVector<QPointF> pixelPoints(
    const AnnotationItem::Tool tool,
    const QVector<QPointF> &normalizedPoints,
    const QSizeF &size
)
{
    QVector<QPointF> points;
    points.reserve(normalizedPoints.size());
    for (const QPointF &point : normalizedPoints) {
        points.append(QPointF(point.x() * size.width(), point.y() * size.height()));
    }
    if (points.isEmpty()) {
        return points;
    }

    if (tool == AnnotationItem::Rectangle && points.size() >= 2) {
        const QPointF start = points.front();
        const QPointF end = points.back();
        return {
            start,
            QPointF(end.x(), start.y()),
            end,
            QPointF(start.x(), end.y()),
            start,
        };
    }

    if (tool == AnnotationItem::Spotlight && points.size() >= 2) {
        const QRectF bounds(points.front(), points.back());
        const QPointF center = bounds.normalized().center();
        const qreal radiusX = bounds.normalized().width() / 2.0;
        const qreal radiusY = bounds.normalized().height() / 2.0;
        QVector<QPointF> ellipse;
        ellipse.reserve(SpotlightSegments + 1);
        for (int index = 0; index <= SpotlightSegments; ++index) {
            const qreal angle = 2.0 * M_PI * qreal(index) / qreal(SpotlightSegments);
            ellipse.append(QPointF(
                center.x() + std::cos(angle) * radiusX,
                center.y() + std::sin(angle) * radiusY
            ));
        }
        return ellipse;
    }

    if (tool != AnnotationItem::Pen && points.size() > 2) {
        return {points.front(), points.back()};
    }
    return points;
}

void appendLine(
    QSGNode *root,
    const QVector<QPointF> &points,
    const QColor &color,
    const qreal width
)
{
    if (points.size() < 2) {
        return;
    }
    auto *geometry = new QSGGeometry(QSGGeometry::defaultAttributes_Point2D(), points.size());
    geometry->setDrawingMode(QSGGeometry::DrawLineStrip);
    geometry->setLineWidth(float(width));
    auto *vertices = geometry->vertexDataAsPoint2D();
    for (qsizetype index = 0; index < points.size(); ++index) {
        vertices[index].set(float(points[index].x()), float(points[index].y()));
    }

    auto *material = new QSGFlatColorMaterial;
    material->setColor(color);
    auto *node = new QSGGeometryNode;
    node->setGeometry(geometry);
    node->setMaterial(material);
    node->setFlag(QSGNode::OwnsGeometry);
    node->setFlag(QSGNode::OwnsMaterial);
    root->appendChildNode(node);
}

void appendArrowHead(
    QSGNode *root,
    const QVector<QPointF> &points,
    const QColor &color,
    const qreal width
)
{
    if (points.size() < 2) {
        return;
    }
    const QPointF end = points.back();
    const QPointF delta = end - points.front();
    const qreal length = std::hypot(delta.x(), delta.y());
    if (length < 0.001) {
        return;
    }
    const QPointF direction(delta.x() / length, delta.y() / length);
    const QPointF perpendicular(-direction.y(), direction.x());
    const qreal headLength = std::clamp(length * 0.2, 8.0, 18.0);
    const QPointF base = end - direction * headLength;
    const qreal halfWidth = headLength * 0.55;
    appendLine(root, {base + perpendicular * halfWidth, end}, color, width);
    appendLine(root, {base - perpendicular * halfWidth, end}, color, width);
}
}

AnnotationItem::AnnotationItem(QQuickItem *parent)
    : QQuickItem(parent)
{
    setFlag(ItemHasContents, true);
    setAcceptedMouseButtons(Qt::NoButton);
    setAcceptTouchEvents(false);
}

bool AnnotationItem::annotationMode() const
{
    return m_annotationMode;
}

void AnnotationItem::setAnnotationMode(const bool enabled)
{
    if (m_annotationMode == enabled) {
        return;
    }
    m_annotationMode = enabled;
    setAcceptedMouseButtons(enabled ? Qt::LeftButton | Qt::RightButton : Qt::NoButton);
    setAcceptTouchEvents(enabled);
    if (!enabled) {
        m_activeStroke.reset();
        ungrabMouse();
        ungrabTouchPoints();
        update();
    }
    emit annotationModeChanged();
}

AnnotationItem::Tool AnnotationItem::tool() const
{
    return m_tool;
}

void AnnotationItem::setTool(const Tool tool)
{
    if (m_tool == tool) {
        return;
    }
    m_tool = tool;
    emit toolChanged();
}

QColor AnnotationItem::strokeColor() const
{
    return m_strokeColor;
}

void AnnotationItem::setStrokeColor(const QColor &color)
{
    if (!color.isValid() || color == m_strokeColor) {
        return;
    }
    m_strokeColor = color;
    emit strokeColorChanged();
}

qreal AnnotationItem::strokeWidth() const
{
    return m_strokeWidth;
}

void AnnotationItem::setStrokeWidth(const qreal width)
{
    if (!std::isfinite(width) || width <= 0.0 || qFuzzyCompare(width, m_strokeWidth)) {
        return;
    }
    m_strokeWidth = width;
    emit strokeWidthChanged();
}

void AnnotationItem::startRecordingClock()
{
    m_recordingClock.restart();
}

bool AnnotationItem::undo()
{
    if (m_activeStroke.has_value() || m_strokes.isEmpty()) {
        return false;
    }
    m_strokes.removeLast();
    update();
    return true;
}

void AnnotationItem::clear()
{
    m_activeStroke.reset();
    m_strokes.clear();
    update();
}

QSGNode *AnnotationItem::updatePaintNode(QSGNode *oldNode, UpdatePaintNodeData *)
{
    delete oldNode;
    auto *root = new QSGNode;
    const QSizeF surfaceSize(width(), height());
    auto appendStroke = [&](const Stroke &stroke) {
        const QVector<QPointF> points = pixelPoints(stroke.tool, stroke.points, surfaceSize);
        appendLine(root, points, stroke.color, stroke.width);
        if (stroke.tool == Arrow) {
            appendArrowHead(root, points, stroke.color, stroke.width);
        }
    };
    for (const Stroke &stroke : std::as_const(m_strokes)) {
        appendStroke(stroke);
    }
    if (m_activeStroke.has_value()) {
        appendStroke(*m_activeStroke);
    }
    return root;
}

void AnnotationItem::mousePressEvent(QMouseEvent *event)
{
    if (!m_annotationMode) {
        event->ignore();
        return;
    }
    if (event->button() == Qt::RightButton) {
        m_activeStroke.reset();
        ungrabMouse();
        update();
        emit passThroughRequested();
        event->accept();
        return;
    }
    if (event->button() != Qt::LeftButton) {
        event->ignore();
        return;
    }
    beginStroke(event->position());
    event->accept();
}

void AnnotationItem::mouseMoveEvent(QMouseEvent *event)
{
    if (!m_annotationMode || !m_activeStroke.has_value()) {
        event->ignore();
        return;
    }
    extendStroke(event->position());
    event->accept();
}

void AnnotationItem::mouseReleaseEvent(QMouseEvent *event)
{
    if (!m_annotationMode || event->button() != Qt::LeftButton || !m_activeStroke.has_value()) {
        event->ignore();
        return;
    }
    finishStroke(event->position());
    event->accept();
}

void AnnotationItem::touchEvent(QTouchEvent *event)
{
    if (!m_annotationMode || event->points().isEmpty()) {
        event->ignore();
        return;
    }
    const QEventPoint &point = event->points().front();
    switch (point.state()) {
    case QEventPoint::Pressed:
        beginStroke(point.position());
        break;
    case QEventPoint::Updated:
    case QEventPoint::Stationary:
        extendStroke(point.position());
        break;
    case QEventPoint::Released:
        finishStroke(point.position());
        break;
    default:
        break;
    }
    event->accept();
}

QPointF AnnotationItem::normalized(const QPointF &position) const
{
    if (width() <= 0.0 || height() <= 0.0) {
        return {};
    }
    return QPointF(
        std::clamp(position.x() / width(), 0.0, 1.0),
        std::clamp(position.y() / height(), 0.0, 1.0)
    );
}

void AnnotationItem::beginStroke(const QPointF &position)
{
    if (m_activeStroke.has_value()) {
        return;
    }
    Stroke stroke;
    stroke.tool = m_tool;
    stroke.color = m_strokeColor;
    stroke.width = m_strokeWidth;
    stroke.points.append(normalized(position));
    stroke.startedAtMilliseconds = recordingMilliseconds();
    m_activeStroke = std::move(stroke);
    update();
}

void AnnotationItem::extendStroke(const QPointF &position)
{
    if (!m_activeStroke.has_value()) {
        return;
    }
    const QPointF point = normalized(position);
    if (m_activeStroke->tool == Pen) {
        m_activeStroke->points.append(point);
    } else if (m_activeStroke->points.size() == 1) {
        m_activeStroke->points.append(point);
    } else {
        m_activeStroke->points.last() = point;
    }
    update();
}

void AnnotationItem::finishStroke(const QPointF &position)
{
    extendStroke(position);
    if (!m_activeStroke.has_value()) {
        return;
    }
    const qint64 endedAtMilliseconds = recordingMilliseconds();
    Stroke stroke = std::move(*m_activeStroke);
    m_activeStroke.reset();
    QVariantList normalizedPoints;
    normalizedPoints.reserve(stroke.points.size());
    for (const QPointF &point : std::as_const(stroke.points)) {
        normalizedPoints.append(point);
    }
    m_strokes.append(stroke);
    const Stroke &committed = m_strokes.back();
    emit strokeCommitted(
        normalizedPoints,
        int(committed.tool),
        double(committed.startedAtMilliseconds) / 1000.0,
        double(endedAtMilliseconds) / 1000.0
    );
    update();
}

qint64 AnnotationItem::recordingMilliseconds() const
{
    return m_recordingClock.isValid() ? m_recordingClock.elapsed() : 0;
}
