#pragma once

#include <QString>

#include <memory>

class QQuickWindow;
class QScreen;

class OverlayPlacementPort
{
public:
    virtual ~OverlayPlacementPort() = default;

    [[nodiscard]] virtual QString mode() const = 0;
    [[nodiscard]] virtual bool guaranteesLayerShell() const = 0;
    virtual bool show(QQuickWindow &window, QScreen &screen, QString *error) = 0;
};

[[nodiscard]] std::unique_ptr<OverlayPlacementPort> createOverlayPlacementPort();
