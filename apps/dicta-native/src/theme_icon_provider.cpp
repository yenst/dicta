#include "theme_icon_provider.h"

#include <QColor>
#include <QHash>
#include <QIcon>
#include <QPainter>
#include <QUrl>
#include <QUrlQuery>

namespace {
QStringList candidates(const QString &name)
{
    static const QHash<QString, QStringList> aliases {
        {QStringLiteral("folder"), {QStringLiteral("folder-symbolic"), QStringLiteral("folder")}},
        {QStringLiteral("folder-open"), {QStringLiteral("folder-open-symbolic"), QStringLiteral("folder-open")}},
        {QStringLiteral("add"), {QStringLiteral("list-add-symbolic"), QStringLiteral("list-add")}},
        {QStringLiteral("settings"), {QStringLiteral("preferences-system-symbolic"), QStringLiteral("preferences-system")}},
        {QStringLiteral("play"), {QStringLiteral("media-playback-start-symbolic"), QStringLiteral("media-playback-start")}},
        {QStringLiteral("pause"), {QStringLiteral("media-playback-pause-symbolic"), QStringLiteral("media-playback-pause")}},
        {QStringLiteral("volume"), {QStringLiteral("audio-volume-high-symbolic"), QStringLiteral("audio-volume-high")}},
        {QStringLiteral("muted"), {QStringLiteral("audio-volume-muted-symbolic"), QStringLiteral("audio-volume-muted")}},
        {QStringLiteral("fullscreen"), {QStringLiteral("view-fullscreen-symbolic"), QStringLiteral("view-fullscreen")}},
        {QStringLiteral("restore"), {QStringLiteral("view-restore-symbolic"), QStringLiteral("view-restore")}},
        {QStringLiteral("search"), {QStringLiteral("edit-find-symbolic"), QStringLiteral("edit-find")}},
        {QStringLiteral("filter"), {QStringLiteral("view-filter-symbolic"), QStringLiteral("view-sort-ascending-symbolic")}},
        {QStringLiteral("copy"), {QStringLiteral("edit-copy-symbolic"), QStringLiteral("edit-copy")}},
        {QStringLiteral("more"), {QStringLiteral("view-more-symbolic"), QStringLiteral("view-more")}},
        {QStringLiteral("back"), {QStringLiteral("go-previous-symbolic"), QStringLiteral("go-previous")}},
        {QStringLiteral("undo"), {QStringLiteral("edit-undo-symbolic"), QStringLiteral("edit-undo")}},
        {QStringLiteral("clear"), {QStringLiteral("edit-clear-symbolic"), QStringLiteral("edit-clear")}},
        {QStringLiteral("record"), {QStringLiteral("media-record-symbolic"), QStringLiteral("media-record")}},
        {QStringLiteral("microphone"), {QStringLiteral("audio-input-microphone-symbolic"), QStringLiteral("audio-input-microphone")}},
    };
    return aliases.value(name, {name});
}

QString fallbackAsset(const QString &name)
{
    static const QHash<QString, QString> assets {
        {QStringLiteral("folder"), QStringLiteral("/usr/share/icons/Adwaita/symbolic/places/folder-symbolic.svg")},
        {QStringLiteral("folder-open"), QStringLiteral("/usr/share/icons/Adwaita/symbolic/status/folder-open-symbolic.svg")},
        {QStringLiteral("add"), QStringLiteral("/usr/share/icons/Adwaita/symbolic/actions/list-add-symbolic.svg")},
        {QStringLiteral("settings"), QStringLiteral("/usr/share/icons/Adwaita/symbolic/categories/preferences-system-symbolic.svg")},
        {QStringLiteral("play"), QStringLiteral("/usr/share/icons/Adwaita/symbolic/actions/media-playback-start-symbolic.svg")},
        {QStringLiteral("pause"), QStringLiteral("/usr/share/icons/Adwaita/symbolic/actions/media-playback-pause-symbolic.svg")},
        {QStringLiteral("volume"), QStringLiteral("/usr/share/icons/Adwaita/symbolic/status/audio-volume-high-symbolic.svg")},
        {QStringLiteral("muted"), QStringLiteral("/usr/share/icons/Adwaita/symbolic/status/audio-volume-muted-symbolic.svg")},
        {QStringLiteral("fullscreen"), QStringLiteral("/usr/share/icons/Adwaita/symbolic/actions/view-fullscreen-symbolic.svg")},
        {QStringLiteral("restore"), QStringLiteral("/usr/share/icons/Adwaita/symbolic/actions/view-restore-symbolic.svg")},
        {QStringLiteral("search"), QStringLiteral("/usr/share/icons/Adwaita/symbolic/actions/edit-find-symbolic.svg")},
        {QStringLiteral("filter"), QStringLiteral("/usr/share/icons/Adwaita/symbolic/actions/view-sort-ascending-symbolic.svg")},
        {QStringLiteral("copy"), QStringLiteral("/usr/share/icons/Adwaita/symbolic/actions/edit-copy-symbolic.svg")},
        {QStringLiteral("more"), QStringLiteral("/usr/share/icons/Adwaita/symbolic/actions/view-more-symbolic.svg")},
        {QStringLiteral("back"), QStringLiteral("/usr/share/icons/Adwaita/symbolic/actions/go-previous-symbolic.svg")},
        {QStringLiteral("undo"), QStringLiteral("/usr/share/icons/Adwaita/symbolic/actions/edit-undo-symbolic.svg")},
        {QStringLiteral("clear"), QStringLiteral("/usr/share/icons/Adwaita/symbolic/actions/edit-clear-symbolic.svg")},
        {QStringLiteral("record"), QStringLiteral("/usr/share/icons/Adwaita/symbolic/actions/media-record-symbolic.svg")},
        {QStringLiteral("microphone"), QStringLiteral("/usr/share/icons/Adwaita/symbolic/devices/audio-input-microphone-symbolic.svg")},
    };
    return assets.value(name);
}
}

ThemeIconProvider::ThemeIconProvider()
    : QQuickImageProvider(QQuickImageProvider::Pixmap)
{
    // Headless Qt and minimal Wayland sessions can start without a platform
    // icon theme even though the freedesktop Adwaita assets are installed.
    // Keep the desktop's selected theme first and use the system icon set as
    // a real-asset fallback rather than drawing replacement glyphs in QML.
    QIcon::setFallbackThemeName(QStringLiteral("Adwaita"));
    if (QIcon::themeName().isEmpty()) {
        QIcon::setThemeName(QStringLiteral("Adwaita"));
    }
}

QPixmap ThemeIconProvider::requestPixmap(
    const QString &id,
    QSize *size,
    const QSize &requestedSize
)
{
    const QUrl url(QStringLiteral("dicta://icon/%1").arg(id));
    const QString name = url.path().section(QLatin1Char('/'), -1);
    const QColor color(QUrlQuery(url).queryItemValue(QStringLiteral("color")));
    const int width = requestedSize.width() > 0 ? requestedSize.width() : 18;
    const int height = requestedSize.height() > 0 ? requestedSize.height() : width;
    const QSize target(width, height);

    QIcon icon;
    for (const QString &candidate : candidates(name)) {
        icon = QIcon::fromTheme(candidate);
        if (!icon.isNull()) {
            break;
        }
    }
    if (icon.isNull()) {
        icon = QIcon(fallbackAsset(name));
    }
    QPixmap pixmap = icon.pixmap(target);
    if (!pixmap.isNull() && color.isValid()) {
        QPainter painter(&pixmap);
        painter.setCompositionMode(QPainter::CompositionMode_SourceIn);
        painter.fillRect(pixmap.rect(), color);
    }
    if (size != nullptr) {
        *size = pixmap.isNull() ? target : pixmap.size();
    }
    return pixmap;
}
