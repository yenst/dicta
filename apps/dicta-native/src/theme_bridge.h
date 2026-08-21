#pragma once

#include <QColor>
#include <QObject>
#include <QString>
#include <QTimer>

class ThemeBridge final : public QObject
{
    Q_OBJECT
    Q_PROPERTY(QString name READ name NOTIFY themeChanged)
    Q_PROPERTY(QString appearance READ appearance NOTIFY themeChanged)
    Q_PROPERTY(QString mode READ mode NOTIFY themeChanged)
    Q_PROPERTY(QString fontFamily READ fontFamily NOTIFY themeChanged)
    Q_PROPERTY(int baseFontSize READ baseFontSize NOTIFY themeChanged)
    Q_PROPERTY(qreal spacingScale READ spacingScale NOTIFY themeChanged)
    Q_PROPERTY(QColor accent READ accent NOTIFY themeChanged)
    Q_PROPERTY(QColor selection READ selection NOTIFY themeChanged)
    Q_PROPERTY(QColor muted READ muted NOTIFY themeChanged)
    Q_PROPERTY(QColor background READ background NOTIFY themeChanged)
    Q_PROPERTY(QColor darkBackground READ darkBackground NOTIFY themeChanged)
    Q_PROPERTY(QColor darkerBackground READ darkerBackground NOTIFY themeChanged)
    Q_PROPERTY(QColor lighterBackground READ lighterBackground NOTIFY themeChanged)
    Q_PROPERTY(QColor foreground READ foreground NOTIFY themeChanged)
    Q_PROPERTY(QColor darkForeground READ darkForeground NOTIFY themeChanged)
    Q_PROPERTY(QColor lightForeground READ lightForeground NOTIFY themeChanged)
    Q_PROPERTY(QColor brightForeground READ brightForeground NOTIFY themeChanged)
    Q_PROPERTY(QColor red READ red NOTIFY themeChanged)
    Q_PROPERTY(QColor yellow READ yellow NOTIFY themeChanged)
    Q_PROPERTY(QColor orange READ orange NOTIFY themeChanged)
    Q_PROPERTY(QColor green READ green NOTIFY themeChanged)
    Q_PROPERTY(QColor cyan READ cyan NOTIFY themeChanged)
    Q_PROPERTY(QColor blue READ blue NOTIFY themeChanged)
    Q_PROPERTY(QColor magenta READ magenta NOTIFY themeChanged)

public:
    explicit ThemeBridge(
        QString stateHome = {},
        QString configHome = {},
        QObject *parent = nullptr
    );

    [[nodiscard]] QString name() const;
    [[nodiscard]] QString appearance() const;
    [[nodiscard]] QString mode() const;
    [[nodiscard]] QString fontFamily() const;
    [[nodiscard]] int baseFontSize() const;
    [[nodiscard]] qreal spacingScale() const;
    [[nodiscard]] QColor accent() const;
    [[nodiscard]] QColor selection() const;
    [[nodiscard]] QColor muted() const;
    [[nodiscard]] QColor background() const;
    [[nodiscard]] QColor darkBackground() const;
    [[nodiscard]] QColor darkerBackground() const;
    [[nodiscard]] QColor lighterBackground() const;
    [[nodiscard]] QColor foreground() const;
    [[nodiscard]] QColor darkForeground() const;
    [[nodiscard]] QColor lightForeground() const;
    [[nodiscard]] QColor brightForeground() const;
    [[nodiscard]] QColor red() const;
    [[nodiscard]] QColor yellow() const;
    [[nodiscard]] QColor orange() const;
    [[nodiscard]] QColor green() const;
    [[nodiscard]] QColor cyan() const;
    [[nodiscard]] QColor blue() const;
    [[nodiscard]] QColor magenta() const;

    Q_INVOKABLE bool reload();
    Q_INVOKABLE bool setAppearance(const QString &appearance);

signals:
    void themeChanged();

private:
    [[nodiscard]] QString themePath(const QString &fileName) const;
    [[nodiscard]] QString userShellPath() const;
    [[nodiscard]] QString fontConfigPath() const;
    [[nodiscard]] QString appearancePath() const;

    QString m_stateHome;
    QString m_configHome;
    QByteArray m_snapshot;
    QTimer m_reloadTimer;
    QString m_name = QStringLiteral("default");
    QString m_appearance = QStringLiteral("system");
    QString m_mode = QStringLiteral("dark");
    QString m_fontFamily = QStringLiteral("monospace");
    int m_baseFontSize = 12;
    qreal m_spacingScale = 1.0;
    QColor m_accent = QColor(QStringLiteral("#7aa2f7"));
    QColor m_selection = QColor(QStringLiteral("#292e42"));
    QColor m_muted = QColor(QStringLiteral("#414868"));
    QColor m_background = QColor(QStringLiteral("#1a1b26"));
    QColor m_darkBackground = QColor(QStringLiteral("#13141c"));
    QColor m_darkerBackground = QColor(QStringLiteral("#0e0e14"));
    QColor m_lighterBackground = QColor(QStringLiteral("#24283b"));
    QColor m_foreground = QColor(QStringLiteral("#a9b1d6"));
    QColor m_darkForeground = QColor(QStringLiteral("#565f89"));
    QColor m_lightForeground = QColor(QStringLiteral("#b4bee6"));
    QColor m_brightForeground = QColor(QStringLiteral("#c0caf5"));
    QColor m_red = QColor(QStringLiteral("#f7768e"));
    QColor m_yellow = QColor(QStringLiteral("#e0af68"));
    QColor m_orange = QColor(QStringLiteral("#eb927b"));
    QColor m_green = QColor(QStringLiteral("#9ece6a"));
    QColor m_cyan = QColor(QStringLiteral("#449dab"));
    QColor m_blue = QColor(QStringLiteral("#7aa2f7"));
    QColor m_magenta = QColor(QStringLiteral("#ad8ee6"));
};
