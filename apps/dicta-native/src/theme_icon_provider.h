#pragma once

#include <QQuickImageProvider>

class ThemeIconProvider final : public QQuickImageProvider
{
public:
    ThemeIconProvider();

    [[nodiscard]] QPixmap requestPixmap(
        const QString &id,
        QSize *size,
        const QSize &requestedSize
    ) override;
};
