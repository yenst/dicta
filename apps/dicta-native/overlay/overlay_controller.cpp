#include "overlay_controller.h"

#include <QGuiApplication>
#include <QQmlComponent>
#include <QQmlContext>
#include <QQmlEngine>
#include <QQmlError>
#include <QQuickWindow>
#include <QScreen>

namespace {
QString componentErrors(const QQmlComponent &component)
{
    QStringList messages;
    for (const QQmlError &error : component.errors()) {
        messages.append(error.toString());
    }
    return messages.join(QLatin1Char('\n'));
}
}

OverlayController::OverlayController(QObject *parent)
    : QObject(parent)
    , m_placement(createOverlayPlacementPort())
{
}

bool OverlayController::initialize(QQmlEngine &engine)
{
    if (m_ready) {
        return true;
    }

    QQmlComponent component(&engine);
    component.loadFromModule(QStringLiteral("Dicta.Native"), QStringLiteral("AnnotationOverlay"));
    if (component.status() != QQmlComponent::Ready) {
        fail(tr("Could not load the annotation overlay:\n%1").arg(componentErrors(component)));
        return false;
    }

    QObject *created = component.create(engine.rootContext());
    auto *window = qobject_cast<QQuickWindow *>(created);
    if (window == nullptr) {
        delete created;
        fail(tr("The annotation overlay root is not a QQuickWindow."));
        return false;
    }

    auto *surface = window->findChild<AnnotationItem *>(QStringLiteral("dictaAnnotationSurface"));
    if (surface == nullptr) {
        delete window;
        fail(tr("The annotation overlay has no annotation surface."));
        return false;
    }

    QQmlEngine::setObjectOwnership(window, QQmlEngine::CppOwnership);
    window->QObject::setParent(this);
    applyWindowInputMode(*window, false);
    m_window = window;
    m_surface = surface;

    connect(window, &QQuickWindow::visibleChanged, this, &OverlayController::visibleChanged);
    connect(
        surface,
        &AnnotationItem::annotationModeChanged,
        this,
        &OverlayController::annotationModeChanged
    );
    connect(surface, &AnnotationItem::strokeCommitted, this, &OverlayController::strokeCommitted);
    connect(
        window,
        SIGNAL(passThroughRequested()),
        this,
        SLOT(enterPassThroughMode())
    );

    m_ready = true;
    emit readyChanged();
    return true;
}

bool OverlayController::ready() const
{
    return m_ready;
}

bool OverlayController::visible() const
{
    return m_window != nullptr && m_window->isVisible();
}

bool OverlayController::annotationMode() const
{
    return m_surface != nullptr && m_surface->annotationMode();
}

OverlayController::Tool OverlayController::tool() const
{
    return m_tool;
}

QString OverlayController::outputName() const
{
    return m_outputName;
}

QString OverlayController::placementMode() const
{
    return m_placement->mode();
}

bool OverlayController::guaranteedLayerShell() const
{
    return m_placement->guaranteesLayerShell();
}

QString OverlayController::lastError() const
{
    return m_lastError;
}

QStringList OverlayController::availableOutputs() const
{
    QStringList names;
    for (const QScreen *screen : QGuiApplication::screens()) {
        names.append(screen->name());
    }
    return names;
}

bool OverlayController::showOnOutput(const QString &outputName)
{
    if (!m_ready || m_window == nullptr || m_surface == nullptr) {
        fail(tr("The annotation overlay is not ready."));
        return false;
    }
    QScreen *screen = findOutput(outputName);
    if (screen == nullptr) {
        fail(tr("Recording output '%1' is not available.").arg(outputName));
        return false;
    }

    setAnnotationMode(false);
    m_surface->clear();
    QString error;
    if (!m_placement->show(*m_window, *screen, &error)) {
        fail(error);
        return false;
    }

    if (m_outputName != screen->name()) {
        m_outputName = screen->name();
        emit outputNameChanged();
    }
    return true;
}

bool OverlayController::startRecordingClock()
{
    if (!m_ready || m_surface == nullptr) {
        fail(tr("The annotation overlay is not ready."));
        return false;
    }
    m_surface->startRecordingClock();
    return true;
}

void OverlayController::enterAnnotationMode()
{
    setAnnotationMode(true);
}

void OverlayController::enterPassThroughMode()
{
    setAnnotationMode(false);
}

void OverlayController::setAnnotationMode(const bool enabled)
{
    if (!m_ready || m_window == nullptr || m_surface == nullptr) {
        fail(tr("The annotation overlay is not ready."));
        return;
    }
    if (enabled && !m_window->isVisible()) {
        fail(tr("The annotation overlay must be visible before it can capture input."));
        return;
    }

    applyWindowInputMode(*m_window, enabled);
    m_window->setProperty("annotationMode", enabled);
    m_surface->setAnnotationMode(enabled);
    if (enabled) {
        m_window->requestActivate();
        m_surface->forceActiveFocus();
    }
}

void OverlayController::applyWindowInputMode(
    QQuickWindow &window,
    const bool annotationMode
)
{
    const bool remap = window.isVisible();
    QScreen *const screen = window.screen();
    const QRect geometry = window.geometry();
    if (remap) {
        window.hide();
    }

    if (annotationMode) {
        window.setFlag(Qt::WindowDoesNotAcceptFocus, false);
        window.setFlag(Qt::WindowTransparentForInput, false);
    } else {
        window.setFlag(Qt::WindowTransparentForInput, true);
        window.setFlag(Qt::WindowDoesNotAcceptFocus, true);
    }

    if (remap) {
        if (screen != nullptr) {
            window.setScreen(screen);
        }
        window.setGeometry(geometry);
        window.showFullScreen();
    }
}

void OverlayController::setTool(const Tool tool)
{
    switch (tool) {
    case Pen:
    case Arrow:
    case Rectangle:
    case Spotlight:
        break;
    default:
        fail(tr("The requested annotation tool is not supported."));
        return;
    }
    if (m_tool == tool) {
        return;
    }
    m_tool = tool;
    if (m_surface != nullptr) {
        m_surface->setTool(static_cast<AnnotationItem::Tool>(tool));
    }
    emit toolChanged();
}

bool OverlayController::undo()
{
    if (!m_ready || m_surface == nullptr) {
        fail(tr("The annotation overlay is not ready."));
        return false;
    }
    return m_surface->undo();
}

void OverlayController::clear()
{
    if (!m_ready || m_surface == nullptr) {
        fail(tr("The annotation overlay is not ready."));
        return;
    }
    m_surface->clear();
}

void OverlayController::finishAndHide()
{
    if (!m_ready || m_window == nullptr || m_surface == nullptr) {
        fail(tr("The annotation overlay is not ready."));
        return;
    }

    setAnnotationMode(false);
    m_window->hide();
    m_surface->clear();
    emit sessionFinished();
}

QScreen *OverlayController::findOutput(const QString &outputName) const
{
    if (outputName.isEmpty()) {
        return QGuiApplication::primaryScreen();
    }
    for (QScreen *screen : QGuiApplication::screens()) {
        if (screen->name() == outputName) {
            return screen;
        }
    }
    return nullptr;
}

void OverlayController::fail(const QString &message)
{
    if (m_lastError != message) {
        m_lastError = message;
        emit lastErrorChanged();
    }
    emit errorOccurred(message);
}
