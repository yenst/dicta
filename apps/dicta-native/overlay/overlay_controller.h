#pragma once

#include "annotation_item.h"
#include "overlay_placement.h"

#include <QObject>
#include <QPointer>
#include <QStringList>
#include <QVariantList>

#include <memory>

class QQmlEngine;
class QQuickWindow;
class QScreen;

class OverlayController final : public QObject
{
    Q_OBJECT
    Q_PROPERTY(bool ready READ ready NOTIFY readyChanged)
    Q_PROPERTY(bool visible READ visible NOTIFY visibleChanged)
    Q_PROPERTY(bool annotationMode READ annotationMode WRITE setAnnotationMode NOTIFY annotationModeChanged)
    Q_PROPERTY(Tool tool READ tool WRITE setTool NOTIFY toolChanged)
    Q_PROPERTY(QString outputName READ outputName NOTIFY outputNameChanged)
    Q_PROPERTY(QString placementMode READ placementMode CONSTANT)
    Q_PROPERTY(bool guaranteedLayerShell READ guaranteedLayerShell CONSTANT)
    Q_PROPERTY(QString lastError READ lastError NOTIFY lastErrorChanged)

public:
    enum Tool {
        Pen = AnnotationItem::Pen,
        Arrow = AnnotationItem::Arrow,
        Rectangle = AnnotationItem::Rectangle,
        Spotlight = AnnotationItem::Spotlight,
    };
    Q_ENUM(Tool)

    explicit OverlayController(QObject *parent = nullptr);

    [[nodiscard]] bool initialize(QQmlEngine &engine);
    [[nodiscard]] bool ready() const;
    [[nodiscard]] bool visible() const;
    [[nodiscard]] bool annotationMode() const;
    [[nodiscard]] Tool tool() const;
    [[nodiscard]] QString outputName() const;
    [[nodiscard]] QString placementMode() const;
    [[nodiscard]] bool guaranteedLayerShell() const;
    [[nodiscard]] QString lastError() const;

    Q_INVOKABLE [[nodiscard]] QStringList availableOutputs() const;
    Q_INVOKABLE [[nodiscard]] bool showOnOutput(const QString &outputName);
    Q_INVOKABLE [[nodiscard]] bool startRecordingClock();
    Q_INVOKABLE void showToast(const QString &message);
    Q_INVOKABLE void enterAnnotationMode();
    Q_INVOKABLE void setAnnotationMode(bool enabled);
    Q_INVOKABLE void setTool(Tool tool);
    Q_INVOKABLE [[nodiscard]] bool undo();
    Q_INVOKABLE void clear();
    Q_INVOKABLE void finishAndHide();

    static void applyWindowInputMode(QQuickWindow &window, bool annotationMode);

public slots:
    void enterPassThroughMode();

signals:
    void readyChanged();
    void visibleChanged();
    void annotationModeChanged();
    void toolChanged();
    void outputNameChanged();
    void lastErrorChanged();
    void strokeCommitted(
        const QVariantList &normalizedPoints,
        int tool,
        double startedAtSeconds,
        double endedAtSeconds
    );
    void sessionFinished();
    void errorOccurred(const QString &message);

private:
    [[nodiscard]] QScreen *findOutput(const QString &outputName) const;
    void fail(const QString &message);

    std::unique_ptr<OverlayPlacementPort> m_placement;
    QPointer<QQuickWindow> m_window;
    QPointer<AnnotationItem> m_surface;
    QString m_outputName;
    QString m_lastError;
    Tool m_tool = Pen;
    bool m_sessionActive = false;
    bool m_ready = false;
};
