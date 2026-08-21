#include "theme_bridge.h"

#include <QDir>
#include <QFile>
#include <QGuiApplication>
#include <QTemporaryDir>
#include <QtTest>

namespace {
void writeText(const QString &path, const QByteArray &content)
{
    QDir().mkpath(QFileInfo(path).absolutePath());
    QFile file(path);
    QVERIFY2(file.open(QIODevice::WriteOnly | QIODevice::Truncate), qPrintable(path));
    QCOMPARE(file.write(content), content.size());
}
}

class ThemeBridgeTest final : public QObject
{
    Q_OBJECT

private slots:
    void loadsOmarchyPaletteAndScale();
    void userShellOverridesThemeScaleAndReloads();
    void persistsBuiltInAppearanceOverride();
};

void ThemeBridgeTest::loadsOmarchyPaletteAndScale()
{
    QTemporaryDir root;
    QVERIFY(root.isValid());
    const QString state = QDir(root.path()).filePath(QStringLiteral("state"));
    const QString config = QDir(root.path()).filePath(QStringLiteral("config"));
    const QString theme = QDir(state).filePath(QStringLiteral("omarchy/current/theme"));
    writeText(
        QDir(theme).filePath(QStringLiteral("colors.toml")),
        "mode = \"light\"\n"
        "accent = \"#123456\"\n"
        "selection = \"#234567\"\n"
        "background = \"#f4f4f4\"\n"
        "foreground = \"#171717\"\n"
        "red = \"#aa2233\"\n"
    );
    writeText(
        QDir(theme).filePath(QStringLiteral("shell.toml")),
        "[font]\nbase-size = 15\n[spacing]\nscale = 1.2\nscale-with-font = true\n"
    );
    writeText(
        QDir(state).filePath(QStringLiteral("omarchy/current/theme.name")),
        "paper\n"
    );

    ThemeBridge bridge(state, config);
    QCOMPARE(bridge.name(), QStringLiteral("paper"));
    QCOMPARE(bridge.mode(), QStringLiteral("light"));
    QCOMPARE(bridge.accent(), QColor(QStringLiteral("#123456")));
    QCOMPARE(bridge.selection(), QColor(QStringLiteral("#234567")));
    QCOMPARE(bridge.background(), QColor(QStringLiteral("#f4f4f4")));
    QCOMPARE(bridge.foreground(), QColor(QStringLiteral("#171717")));
    QCOMPARE(bridge.red(), QColor(QStringLiteral("#aa2233")));
    QCOMPARE(bridge.baseFontSize(), 17);
    QCOMPARE(bridge.spacingScale(), 1.5);
    QVERIFY(!bridge.fontFamily().isEmpty());
}

void ThemeBridgeTest::persistsBuiltInAppearanceOverride()
{
    QTemporaryDir root;
    QVERIFY(root.isValid());
    const QString state = QDir(root.path()).filePath(QStringLiteral("state"));
    const QString config = QDir(root.path()).filePath(QStringLiteral("config"));
    QDir().mkpath(state);
    ThemeBridge bridge(state, config);

    QVERIFY(bridge.setAppearance(QStringLiteral("light")));
    QCOMPARE(bridge.appearance(), QStringLiteral("light"));
    QCOMPARE(bridge.name(), QStringLiteral("Dicta Light"));
    QCOMPARE(bridge.mode(), QStringLiteral("light"));

    ThemeBridge restored(state, config);
    QCOMPARE(restored.appearance(), QStringLiteral("light"));
    QCOMPARE(restored.background(), QColor(QStringLiteral("#e6e7ed")));
    QVERIFY(!restored.setAppearance(QStringLiteral("unknown")));
}

void ThemeBridgeTest::userShellOverridesThemeScaleAndReloads()
{
    QTemporaryDir root;
    QVERIFY(root.isValid());
    const QString state = QDir(root.path()).filePath(QStringLiteral("state"));
    const QString config = QDir(root.path()).filePath(QStringLiteral("config"));
    const QString theme = QDir(state).filePath(QStringLiteral("omarchy/current/theme"));
    writeText(
        QDir(theme).filePath(QStringLiteral("colors.toml")),
        "background = \"#111111\"\nforeground = \"#eeeeee\"\naccent = \"#3366cc\"\n"
    );
    writeText(
        QDir(theme).filePath(QStringLiteral("shell.toml")),
        "[font]\nbase-size = 12\n[spacing]\nscale = 1.0\n"
    );
    const QString userShell = QDir(config).filePath(QStringLiteral("omarchy/shell.toml"));
    writeText(userShell, "[font]\nbase-size = 18\n[spacing]\nscale = 0.8\n");

    ThemeBridge bridge(state, config);
    QCOMPARE(bridge.baseFontSize(), 20);
    QCOMPARE(bridge.spacingScale(), 1.2);

    QSignalSpy changed(&bridge, &ThemeBridge::themeChanged);
    writeText(
        QDir(theme).filePath(QStringLiteral("colors.toml")),
        "background = \"#222222\"\nforeground = \"#dddddd\"\naccent = \"#cc6633\"\n"
    );
    QVERIFY(bridge.reload());
    QCOMPARE(changed.count(), 1);
    QCOMPARE(bridge.background(), QColor(QStringLiteral("#222222")));
    QCOMPARE(bridge.accent(), QColor(QStringLiteral("#cc6633")));
    QVERIFY(!bridge.reload());
    QCOMPARE(changed.count(), 1);
}

QTEST_MAIN(ThemeBridgeTest)
#include "theme_bridge_test.moc"
