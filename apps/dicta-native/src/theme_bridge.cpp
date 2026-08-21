#include "theme_bridge.h"

#include <QDir>
#include <QFile>
#include <QFont>
#include <QFontDatabase>
#include <QGuiApplication>
#include <QHash>
#include <QFileInfo>
#include <QRegularExpression>
#include <QSaveFile>

#include <algorithm>

namespace {
QByteArray readFile(const QString &path)
{
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly | QIODevice::Text)) {
        return {};
    }
    return file.readAll();
}

QHash<QString, QString> parseToml(const QByteArray &bytes)
{
    static const QRegularExpression sectionPattern(
        QStringLiteral(R"(^\s*\[([A-Za-z0-9_-]+)\]\s*(?:#.*)?$)")
    );
    static const QRegularExpression valuePattern(
        QStringLiteral(
            R"(^\s*([A-Za-z0-9_-]+)\s*=\s*(?:\"([^\"]*)\"|'([^']*)'|([^#\s]+))\s*(?:#.*)?$)"
        )
    );

    QHash<QString, QString> values;
    QString section;
    const QString text = QString::fromUtf8(bytes);
    const QStringList lines = text.split(QLatin1Char('\n'));
    for (const QString &line : lines) {
        const QRegularExpressionMatch sectionMatch = sectionPattern.match(line);
        if (sectionMatch.hasMatch()) {
            section = sectionMatch.captured(1);
            continue;
        }
        const QRegularExpressionMatch valueMatch = valuePattern.match(line);
        if (!valueMatch.hasMatch()) {
            continue;
        }
        QString value = valueMatch.captured(2);
        if (value.isNull()) {
            value = valueMatch.captured(3);
        }
        if (value.isNull()) {
            value = valueMatch.captured(4);
        }
        const QString key = section.isEmpty()
            ? valueMatch.captured(1)
            : section + QLatin1Char('.') + valueMatch.captured(1);
        values.insert(key, value.trimmed());
    }
    return values;
}

QColor colorValue(
    const QHash<QString, QString> &values,
    const QString &key,
    const QColor &fallback
)
{
    const QColor candidate(values.value(key));
    return candidate.isValid() ? candidate : fallback;
}

qreal numberValue(
    const QHash<QString, QString> &values,
    const QString &key,
    const qreal fallback
)
{
    bool ok = false;
    const qreal candidate = values.value(key).toDouble(&ok);
    return ok ? candidate : fallback;
}

QString defaultStateHome()
{
    const QString configured = qEnvironmentVariable("XDG_STATE_HOME");
    return configured.isEmpty()
        ? QDir::home().filePath(QStringLiteral(".local/state"))
        : configured;
}

QString defaultConfigHome()
{
    const QString configured = qEnvironmentVariable("XDG_CONFIG_HOME");
    return configured.isEmpty()
        ? QDir::home().filePath(QStringLiteral(".config"))
        : configured;
}
}

ThemeBridge::ThemeBridge(QString stateHome, QString configHome, QObject *parent)
    : QObject(parent)
    , m_stateHome(stateHome.isEmpty() ? defaultStateHome() : std::move(stateHome))
    , m_configHome(configHome.isEmpty() ? defaultConfigHome() : std::move(configHome))
{
    m_reloadTimer.setInterval(750);
    connect(&m_reloadTimer, &QTimer::timeout, this, &ThemeBridge::reload);
    reload();
    m_reloadTimer.start();
}

QString ThemeBridge::name() const { return m_name; }
QString ThemeBridge::appearance() const { return m_appearance; }
QString ThemeBridge::mode() const { return m_mode; }
QString ThemeBridge::fontFamily() const { return m_fontFamily; }
int ThemeBridge::baseFontSize() const { return m_baseFontSize; }
qreal ThemeBridge::spacingScale() const { return m_spacingScale; }
QColor ThemeBridge::accent() const { return m_accent; }
QColor ThemeBridge::selection() const { return m_selection; }
QColor ThemeBridge::muted() const { return m_muted; }
QColor ThemeBridge::background() const { return m_background; }
QColor ThemeBridge::darkBackground() const { return m_darkBackground; }
QColor ThemeBridge::darkerBackground() const { return m_darkerBackground; }
QColor ThemeBridge::lighterBackground() const { return m_lighterBackground; }
QColor ThemeBridge::foreground() const { return m_foreground; }
QColor ThemeBridge::darkForeground() const { return m_darkForeground; }
QColor ThemeBridge::lightForeground() const { return m_lightForeground; }
QColor ThemeBridge::brightForeground() const { return m_brightForeground; }
QColor ThemeBridge::red() const { return m_red; }
QColor ThemeBridge::yellow() const { return m_yellow; }
QColor ThemeBridge::orange() const { return m_orange; }
QColor ThemeBridge::green() const { return m_green; }
QColor ThemeBridge::cyan() const { return m_cyan; }
QColor ThemeBridge::blue() const { return m_blue; }
QColor ThemeBridge::magenta() const { return m_magenta; }

bool ThemeBridge::reload()
{
    const QByteArray colorsBytes = readFile(themePath(QStringLiteral("colors.toml")));
    const QByteArray themeShellBytes = readFile(themePath(QStringLiteral("shell.toml")));
    const QByteArray userShellBytes = readFile(userShellPath());
    const QByteArray themeNameBytes = readFile(
        QDir(m_stateHome).filePath(QStringLiteral("omarchy/current/theme.name"))
    );
    const QByteArray fontConfigBytes = readFile(fontConfigPath());
    const QByteArray appearanceBytes = readFile(appearancePath());
    const QByteArray snapshot = colorsBytes + '\0' + themeShellBytes + '\0'
        + userShellBytes + '\0' + themeNameBytes + '\0' + fontConfigBytes
        + '\0' + appearanceBytes;
    if (snapshot == m_snapshot) {
        return false;
    }
    m_snapshot = snapshot;

    const QHash<QString, QString> colors = parseToml(colorsBytes);
    QHash<QString, QString> shell = parseToml(themeShellBytes);
    const QHash<QString, QString> userShell = parseToml(userShellBytes);
    for (auto iterator = userShell.constBegin(); iterator != userShell.constEnd(); ++iterator) {
        shell.insert(iterator.key(), iterator.value());
    }

    const QString requestedAppearance = QString::fromUtf8(appearanceBytes).trimmed().toLower();
    m_appearance = requestedAppearance == QStringLiteral("dark")
            || requestedAppearance == QStringLiteral("light")
        ? requestedAppearance : QStringLiteral("system");
    const QString nextName = QString::fromUtf8(themeNameBytes).trimmed();
    m_name = nextName.isEmpty() ? QStringLiteral("default") : nextName;
    const QString nextMode = colors.value(QStringLiteral("mode")).toLower();
    m_mode = nextMode == QStringLiteral("light")
        ? QStringLiteral("light") : QStringLiteral("dark");
    // Reset to Dicta's portable baseline before applying optional Omarchy data.
    // This also makes switching from a built-in override back to system reliable
    // on desktops that do not provide Omarchy theme files.
    m_accent = QColor(QStringLiteral("#7aa2f7"));
    m_selection = QColor(QStringLiteral("#292e42"));
    m_muted = QColor(QStringLiteral("#414868"));
    m_background = QColor(QStringLiteral("#1a1b26"));
    m_darkBackground = QColor(QStringLiteral("#13141c"));
    m_darkerBackground = QColor(QStringLiteral("#0e0e14"));
    m_lighterBackground = QColor(QStringLiteral("#24283b"));
    m_foreground = QColor(QStringLiteral("#a9b1d6"));
    m_darkForeground = QColor(QStringLiteral("#565f89"));
    m_lightForeground = QColor(QStringLiteral("#b4bee6"));
    m_brightForeground = QColor(QStringLiteral("#c0caf5"));
    m_red = QColor(QStringLiteral("#f7768e"));
    m_yellow = QColor(QStringLiteral("#e0af68"));
    m_orange = QColor(QStringLiteral("#eb927b"));
    m_green = QColor(QStringLiteral("#9ece6a"));
    m_cyan = QColor(QStringLiteral("#449dab"));
    m_blue = m_accent;
    m_magenta = QColor(QStringLiteral("#ad8ee6"));
    m_accent = colorValue(colors, QStringLiteral("accent"), m_accent);
    m_selection = colorValue(colors, QStringLiteral("selection"), m_selection);
    m_muted = colorValue(colors, QStringLiteral("muted"), m_muted);
    m_background = colorValue(colors, QStringLiteral("background"), m_background);
    m_darkBackground = colorValue(
        colors,
        QStringLiteral("dark_background"),
        m_background.darker(112)
    );
    m_darkerBackground = colorValue(
        colors,
        QStringLiteral("darker_background"),
        m_darkBackground.darker(112)
    );
    m_lighterBackground = colorValue(
        colors,
        QStringLiteral("lighter_background"),
        m_background.lighter(112)
    );
    m_foreground = colorValue(colors, QStringLiteral("foreground"), m_foreground);
    m_darkForeground = colorValue(
        colors,
        QStringLiteral("dark_foreground"),
        m_foreground.darker(155)
    );
    m_lightForeground = colorValue(
        colors,
        QStringLiteral("light_foreground"),
        m_foreground.lighter(108)
    );
    m_brightForeground = colorValue(
        colors,
        QStringLiteral("bright_foreground"),
        m_foreground.lighter(116)
    );
    m_red = colorValue(colors, QStringLiteral("red"), m_red);
    m_yellow = colorValue(colors, QStringLiteral("yellow"), m_yellow);
    m_orange = colorValue(colors, QStringLiteral("orange"), m_orange);
    m_green = colorValue(colors, QStringLiteral("green"), m_green);
    m_cyan = colorValue(colors, QStringLiteral("cyan"), m_cyan);
    m_blue = colorValue(colors, QStringLiteral("blue"), m_accent);
    m_magenta = colorValue(colors, QStringLiteral("magenta"), m_magenta);

    if (m_appearance == QStringLiteral("dark")) {
        m_name = QStringLiteral("Dicta Dark");
        m_mode = QStringLiteral("dark");
        m_accent = QColor(QStringLiteral("#7aa2f7"));
        m_selection = QColor(QStringLiteral("#292e42"));
        m_muted = QColor(QStringLiteral("#414868"));
        m_background = QColor(QStringLiteral("#1a1b26"));
        m_darkBackground = QColor(QStringLiteral("#13141c"));
        m_darkerBackground = QColor(QStringLiteral("#0e0e14"));
        m_lighterBackground = QColor(QStringLiteral("#24283b"));
        m_foreground = QColor(QStringLiteral("#a9b1d6"));
        m_darkForeground = QColor(QStringLiteral("#66709b"));
        m_lightForeground = QColor(QStringLiteral("#b4bee6"));
        m_brightForeground = QColor(QStringLiteral("#c0caf5"));
        m_red = QColor(QStringLiteral("#f7768e"));
        m_yellow = QColor(QStringLiteral("#e0af68"));
        m_orange = QColor(QStringLiteral("#ff9e64"));
        m_green = QColor(QStringLiteral("#9ece6a"));
        m_cyan = QColor(QStringLiteral("#7dcfff"));
        m_blue = m_accent;
        m_magenta = QColor(QStringLiteral("#bb9af7"));
    } else if (m_appearance == QStringLiteral("light")) {
        m_name = QStringLiteral("Dicta Light");
        m_mode = QStringLiteral("light");
        m_accent = QColor(QStringLiteral("#34548a"));
        m_selection = QColor(QStringLiteral("#c4d6f3"));
        m_muted = QColor(QStringLiteral("#b4b5b9"));
        m_background = QColor(QStringLiteral("#e6e7ed"));
        m_darkBackground = QColor(QStringLiteral("#d9dbe3"));
        m_darkerBackground = QColor(QStringLiteral("#cfd1da"));
        m_lighterBackground = QColor(QStringLiteral("#f3f4f8"));
        m_foreground = QColor(QStringLiteral("#4c505e"));
        m_darkForeground = QColor(QStringLiteral("#707280"));
        m_lightForeground = QColor(QStringLiteral("#343b58"));
        m_brightForeground = QColor(QStringLiteral("#1f2335"));
        m_red = QColor(QStringLiteral("#8c4351"));
        m_yellow = QColor(QStringLiteral("#8f5e15"));
        m_orange = QColor(QStringLiteral("#965027"));
        m_green = QColor(QStringLiteral("#485e30"));
        m_cyan = QColor(QStringLiteral("#0f4b6e"));
        m_blue = m_accent;
        m_magenta = QColor(QStringLiteral("#5a4a78"));
    }

    const int shellBaseFontSize = std::clamp(
        qRound(numberValue(shell, QStringLiteral("font.base-size"), 12.0)),
        8,
        32
    );
    // Dicta is a reading surface, so its body token follows Omarchy's
    // subtitle step rather than the smaller bar/body root.
    m_baseFontSize = std::clamp(shellBaseFontSize + 2, 10, 34);
    const qreal spacing = numberValue(shell, QStringLiteral("spacing.scale"), 1.0);
    const bool scaleWithFont = shell.value(
        QStringLiteral("spacing.scale-with-font"),
        QStringLiteral("true")
    ).compare(QStringLiteral("false"), Qt::CaseInsensitive) != 0;
    m_spacingScale = std::clamp(
        spacing * (scaleWithFont ? qreal(shellBaseFontSize) / 12.0 : 1.0),
        0.5,
        3.0
    );

    const QString resolvedFont = QFontDatabase::systemFont(QFontDatabase::FixedFont).family();
    m_fontFamily = resolvedFont.isEmpty() ? QStringLiteral("monospace") : resolvedFont;
    if (QGuiApplication::instance() != nullptr) {
        QFont font(m_fontFamily);
        font.setPixelSize(m_baseFontSize);
        QGuiApplication::setFont(font);
    }

    emit themeChanged();
    return true;
}

bool ThemeBridge::setAppearance(const QString &appearance)
{
    const QString normalized = appearance.trimmed().toLower();
    if (normalized != QStringLiteral("system")
        && normalized != QStringLiteral("dark")
        && normalized != QStringLiteral("light")) {
        return false;
    }
    QDir().mkpath(QFileInfo(appearancePath()).absolutePath());
    QSaveFile file(appearancePath());
    if (!file.open(QIODevice::WriteOnly | QIODevice::Text)
        || file.write(normalized.toUtf8() + '\n') < 0
        || !file.commit()) {
        return false;
    }
    return reload() || m_appearance == normalized;
}

QString ThemeBridge::themePath(const QString &fileName) const
{
    return QDir(m_stateHome).filePath(
        QStringLiteral("omarchy/current/theme/%1").arg(fileName)
    );
}

QString ThemeBridge::userShellPath() const
{
    return QDir(m_configHome).filePath(QStringLiteral("omarchy/shell.toml"));
}

QString ThemeBridge::fontConfigPath() const
{
    return QDir(m_configHome).filePath(QStringLiteral("fontconfig/fonts.conf"));
}

QString ThemeBridge::appearancePath() const
{
    return QDir(m_configHome).filePath(QStringLiteral("dicta/appearance"));
}
