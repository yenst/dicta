#include "annotation_item.h"
#include "overlay_controller.h"

#include <QMouseEvent>
#include <QQmlComponent>
#include <QQmlEngine>
#include <QQuickWindow>
#include <QScopedPointer>
#include <QSignalSpy>
#include <QTest>
#include <QtQml/qqml.h>

class TestAnnotationItem final : public AnnotationItem
{
public:
    using AnnotationItem::mouseMoveEvent;
    using AnnotationItem::mousePressEvent;
    using AnnotationItem::mouseReleaseEvent;
};

class AnnotationItemTest final : public QObject
{
    Q_OBJECT

private slots:
    void passThroughRejectsPointerInput();
    void rightClickCancelsActiveStrokeAndRequestsPassThrough();
    void commitsNormalizedSceneGraphStroke();
    void undoAndClearAreDeterministic();
    void overlayWindowFlagsFollowInputModeSynchronously();
    void visibleOverlayIsRemappedForEachInputMode();
    void focusedOverlayRoutesEscapeToPassThroughRequest();
    void helperCanRemainVisibleWhileDrawingSurfaceIsUnmapped();
};

namespace {
QMouseEvent mouseEvent(
    const QEvent::Type type,
    const QPointF &position,
    const Qt::MouseButton button,
    const Qt::MouseButtons buttons
)
{
    return QMouseEvent(
        type,
        position,
        position,
        position,
        button,
        buttons,
        Qt::NoModifier
    );
}

void drawStroke(TestAnnotationItem &item)
{
    QMouseEvent press = mouseEvent(
        QEvent::MouseButtonPress,
        QPointF(20.0, 25.0),
        Qt::LeftButton,
        Qt::LeftButton
    );
    item.mousePressEvent(&press);
    QMouseEvent move = mouseEvent(
        QEvent::MouseMove,
        QPointF(100.0, 50.0),
        Qt::NoButton,
        Qt::LeftButton
    );
    item.mouseMoveEvent(&move);
    QMouseEvent release = mouseEvent(
        QEvent::MouseButtonRelease,
        QPointF(180.0, 75.0),
        Qt::LeftButton,
        Qt::NoButton
    );
    item.mouseReleaseEvent(&release);
}
}

void AnnotationItemTest::passThroughRejectsPointerInput()
{
    TestAnnotationItem item;
    item.setWidth(200.0);
    item.setHeight(100.0);
    QSignalSpy strokes(&item, &AnnotationItem::strokeCommitted);

    drawStroke(item);

    QCOMPARE(strokes.count(), 0);
}

void AnnotationItemTest::rightClickCancelsActiveStrokeAndRequestsPassThrough()
{
    TestAnnotationItem item;
    item.setWidth(200.0);
    item.setHeight(100.0);
    item.setAnnotationMode(true);
    QSignalSpy strokes(&item, &AnnotationItem::strokeCommitted);
    QSignalSpy passThrough(&item, &AnnotationItem::passThroughRequested);

    QMouseEvent press = mouseEvent(
        QEvent::MouseButtonPress,
        QPointF(20.0, 25.0),
        Qt::LeftButton,
        Qt::LeftButton
    );
    item.mousePressEvent(&press);
    QMouseEvent escape = mouseEvent(
        QEvent::MouseButtonPress,
        QPointF(20.0, 25.0),
        Qt::RightButton,
        Qt::RightButton
    );
    item.mousePressEvent(&escape);
    QMouseEvent release = mouseEvent(
        QEvent::MouseButtonRelease,
        QPointF(180.0, 75.0),
        Qt::LeftButton,
        Qt::NoButton
    );
    item.mouseReleaseEvent(&release);

    QVERIFY(escape.isAccepted());
    QCOMPARE(passThrough.count(), 1);
    QCOMPARE(strokes.count(), 0);
    QVERIFY(!item.undo());
}

void AnnotationItemTest::commitsNormalizedSceneGraphStroke()
{
    TestAnnotationItem item;
    item.setWidth(200.0);
    item.setHeight(100.0);
    item.setAnnotationMode(true);
    item.setTool(AnnotationItem::Pen);
    item.startRecordingClock();
    QSignalSpy strokes(&item, &AnnotationItem::strokeCommitted);

    drawStroke(item);

    QCOMPARE(strokes.count(), 1);
    const QVariantList arguments = strokes.takeFirst();
    const QVariantList points = arguments.at(0).toList();
    QCOMPARE(points.size(), 3);
    QCOMPARE(points.front().toPointF(), QPointF(0.1, 0.25));
    QCOMPARE(points.back().toPointF(), QPointF(0.9, 0.75));
    QCOMPARE(arguments.at(1).toInt(), int(AnnotationItem::Pen));
    QVERIFY(arguments.at(3).toDouble() >= arguments.at(2).toDouble());
}

void AnnotationItemTest::undoAndClearAreDeterministic()
{
    TestAnnotationItem item;
    item.setWidth(200.0);
    item.setHeight(100.0);
    item.setAnnotationMode(true);

    drawStroke(item);
    QVERIFY(item.undo());
    QVERIFY(!item.undo());
    drawStroke(item);
    item.clear();
    QVERIFY(!item.undo());
}

void AnnotationItemTest::overlayWindowFlagsFollowInputModeSynchronously()
{
    QQuickWindow window;
    window.setFlags(
        Qt::FramelessWindowHint
        | Qt::Tool
        | Qt::BypassWindowManagerHint
        | Qt::WindowStaysOnTopHint
    );

    OverlayController::applyWindowInputMode(window, false);
    QVERIFY(window.flags().testFlag(Qt::FramelessWindowHint));
    QVERIFY(window.flags().testFlag(Qt::Tool));
    QVERIFY(window.flags().testFlag(Qt::BypassWindowManagerHint));
    QVERIFY(window.flags().testFlag(Qt::WindowStaysOnTopHint));
    QVERIFY(window.flags().testFlag(Qt::WindowTransparentForInput));
    QVERIFY(window.flags().testFlag(Qt::WindowDoesNotAcceptFocus));

    OverlayController::applyWindowInputMode(window, true);
    QVERIFY(window.flags().testFlag(Qt::FramelessWindowHint));
    QVERIFY(window.flags().testFlag(Qt::Tool));
    QVERIFY(window.flags().testFlag(Qt::BypassWindowManagerHint));
    QVERIFY(window.flags().testFlag(Qt::WindowStaysOnTopHint));
    QVERIFY(!window.flags().testFlag(Qt::WindowTransparentForInput));
    QVERIFY(!window.flags().testFlag(Qt::WindowDoesNotAcceptFocus));

    OverlayController::applyWindowInputMode(window, false);
    QVERIFY(window.flags().testFlag(Qt::FramelessWindowHint));
    QVERIFY(window.flags().testFlag(Qt::WindowTransparentForInput));
    QVERIFY(window.flags().testFlag(Qt::WindowDoesNotAcceptFocus));
}

void AnnotationItemTest::visibleOverlayIsRemappedForEachInputMode()
{
    QQuickWindow window;
    window.setFlags(
        Qt::FramelessWindowHint
        | Qt::WindowTransparentForInput
        | Qt::WindowDoesNotAcceptFocus
    );
    window.setGeometry(10, 20, 320, 180);
    window.show();
    QTRY_VERIFY(window.isVisible());

    OverlayController::applyWindowInputMode(window, true);
    QVERIFY(window.isVisible());
    QVERIFY(window.visibility() == QWindow::Windowed);
    QCOMPARE(window.geometry(), QRect(10, 20, 320, 180));
    QVERIFY(!window.flags().testFlag(Qt::WindowTransparentForInput));
    QVERIFY(!window.flags().testFlag(Qt::WindowDoesNotAcceptFocus));

    OverlayController::applyWindowInputMode(window, false);
    QVERIFY(window.isVisible());
    QVERIFY(window.visibility() == QWindow::Windowed);
    QCOMPARE(window.geometry(), QRect(10, 20, 320, 180));
    QVERIFY(window.flags().testFlag(Qt::WindowTransparentForInput));
    QVERIFY(window.flags().testFlag(Qt::WindowDoesNotAcceptFocus));
}

void AnnotationItemTest::focusedOverlayRoutesEscapeToPassThroughRequest()
{
    qmlRegisterType<AnnotationItem>("Dicta.Native", 1, 0, "AnnotationItem");
    QQmlEngine engine;
    QQmlComponent component(
        &engine,
        QUrl::fromLocalFile(QStringLiteral(DICTA_OVERLAY_QML_PATH))
    );
    QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    QScopedPointer<QObject> created(component.create());
    QVERIFY2(created, qPrintable(component.errorString()));
    auto *window = qobject_cast<QQuickWindow *>(created.get());
    QVERIFY(window != nullptr);
    auto *surface = window->findChild<AnnotationItem *>(
        QStringLiteral("dictaAnnotationSurface")
    );
    QVERIFY(surface != nullptr);
    QSignalSpy passThroughRequested(window, SIGNAL(passThroughRequested()));

    QVERIFY(!window->flags().testFlag(Qt::WindowTransparentForInput));
    QVERIFY(!window->flags().testFlag(Qt::WindowDoesNotAcceptFocus));
    OverlayController::applyWindowInputMode(*window, false);
    QVERIFY(window->flags().testFlag(Qt::WindowTransparentForInput));
    QVERIFY(window->flags().testFlag(Qt::WindowDoesNotAcceptFocus));
    OverlayController::applyWindowInputMode(*window, true);
    window->setProperty("annotationMode", true);
    surface->setAnnotationMode(true);
    window->show();
    window->requestActivate();
    surface->forceActiveFocus();
    QTRY_VERIFY(surface->hasActiveFocus());

    QTest::keyClick(window, Qt::Key_Escape);

    QCOMPARE(passThroughRequested.count(), 1);
}

void AnnotationItemTest::helperCanRemainVisibleWhileDrawingSurfaceIsUnmapped()
{
    qmlRegisterType<AnnotationItem>("Dicta.Native", 1, 0, "AnnotationItem");
    QQmlEngine engine;
    QQmlComponent component(
        &engine,
        QUrl::fromLocalFile(QStringLiteral(DICTA_OVERLAY_QML_PATH))
    );
    QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    QScopedPointer<QObject> created(component.create());
    QVERIFY2(created, qPrintable(component.errorString()));
    auto *overlay = qobject_cast<QQuickWindow *>(created.get());
    QVERIFY(overlay != nullptr);
    auto *helper = overlay->findChild<QQuickWindow *>(
        QStringLiteral("dictaAnnotationHelper")
    );
    QVERIFY(helper != nullptr);
    QVERIFY(!overlay->isVisible());
    QVERIFY(!helper->isVisible());
    QVERIFY(helper->flags().testFlag(Qt::WindowTransparentForInput));
    QVERIFY(helper->flags().testFlag(Qt::WindowDoesNotAcceptFocus));

    QVERIFY(QMetaObject::invokeMethod(overlay, "showHelper"));
    QTRY_VERIFY(helper->isVisible());
    QVERIFY(!overlay->isVisible());

    QVERIFY(QMetaObject::invokeMethod(overlay, "hideHelper"));
    QTRY_VERIFY(!helper->isVisible());
}

QTEST_MAIN(AnnotationItemTest)

#include "annotation_item_test.moc"
