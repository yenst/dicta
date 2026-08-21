#include "overlay_placement.h"

#include <QQuickWindow>
#include <QScreen>

namespace {
class QtToplevelPlacement final : public OverlayPlacementPort
{
public:
    [[nodiscard]] QString mode() const override
    {
        return QStringLiteral("hyprland_bypass_toplevel");
    }

    [[nodiscard]] bool guaranteesLayerShell() const override
    {
        return false;
    }

    bool show(QQuickWindow &window, QScreen &screen, QString *error) override
    {
        if (screen.geometry().isEmpty()) {
            if (error != nullptr) {
                *error = QStringLiteral("The selected output has no usable geometry.");
            }
            return false;
        }

        window.hide();
        window.setFlags(
            window.flags()
            | Qt::FramelessWindowHint
            | Qt::Tool
            | Qt::BypassWindowManagerHint
            | Qt::WindowStaysOnTopHint
        );
        window.setScreen(&screen);
        window.setGeometry(screen.geometry());
        window.show();
        return true;
    }
};
}

std::unique_ptr<OverlayPlacementPort> createOverlayPlacementPort()
{
    return std::make_unique<QtToplevelPlacement>();
}
