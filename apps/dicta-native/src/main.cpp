#include "native_bridge.h"
#include "overlay_controller.h"
#include "theme_bridge.h"
#include "theme_icon_provider.h"

#include <QCommandLineOption>
#include <QCommandLineParser>
#include <QCoreApplication>
#include <QDebug>
#include <QDir>
#include <QFileInfo>
#include <QFile>
#include <QGuiApplication>
#include <QIcon>
#include <QPointer>
#include <QQuickWindow>
#include <QQmlApplicationEngine>
#include <QScreen>
#include <QStandardPaths>
#include <QTimer>
#include <QVariant>

#include <cstdlib>
#include <cstdio>
#include <string_view>
#include <unistd.h>

namespace {
QString environmentOrDefault(const char *name, const QString &fallback)
{
    const QString value = qEnvironmentVariable(name);
    return value.isEmpty() ? fallback : value;
}

QString defaultSocketPath()
{
    QString runtimeRoot = qEnvironmentVariable("XDG_RUNTIME_DIR");
    if (runtimeRoot.isEmpty()) {
        runtimeRoot = QStringLiteral("/run/user/%1").arg(::geteuid());
    }
    return QDir(runtimeRoot).filePath(QStringLiteral("dicta/control-v1.sock"));
}

QString defaultStorageRoot()
{
    QString documents = QStandardPaths::writableLocation(QStandardPaths::DocumentsLocation);
    if (documents.isEmpty()) {
        documents = QDir::homePath();
    }
    const QString current = QDir(documents).filePath(QStringLiteral("Dicta"));
    const QString legacy = QDir(documents).filePath(QStringLiteral("PromptReel"));
    return QFileInfo::exists(current) || !QFileInfo::exists(legacy) ? current : legacy;
}

QString defaultOutputName()
{
    const QScreen *screen = QGuiApplication::primaryScreen();
    return screen == nullptr ? QString() : screen->name();
}

void appendUiMarker(const QString &state)
{
    const QString markerPath = qEnvironmentVariable("DICTA_NATIVE_E2E_UI_MARKER");
    if (markerPath.isEmpty()) {
        return;
    }
    QFile marker(markerPath);
    if (marker.open(QIODevice::WriteOnly | QIODevice::Append | QIODevice::Text)) {
        marker.write(state.toUtf8());
        marker.write("\n");
    }
}
}

int main(int argc, char *argv[])
{
    for (int index = 1; index < argc; ++index) {
        const std::string_view argument(argv[index]);
        if (argument == "--version" || argument == "-v") {
            std::fputs("Dicta " DICTA_NATIVE_VERSION "\n", stdout);
            return EXIT_SUCCESS;
        }
    }
    QGuiApplication application(argc, argv);
    application.setApplicationName(QStringLiteral("Dicta"));
    application.setApplicationVersion(QStringLiteral(DICTA_NATIVE_VERSION));
    application.setOrganizationName(QStringLiteral("Dicta"));
    application.setWindowIcon(QIcon(QStringLiteral("qrc:/dicta/assets/dicta-mark.png")));

    ThemeBridge themeBridge;

    QCommandLineParser parser;
    parser.setApplicationDescription(QStringLiteral("Dicta native recording service"));
    parser.addHelpOption();
    parser.addVersionOption();
    const QCommandLineOption backgroundOption(
        QStringLiteral("background"),
        QStringLiteral("Run the native recording service without the main window.")
    );
    const QCommandLineOption socketOption(
        QStringLiteral("socket"),
        QStringLiteral("Listen on the local control socket at <path>."),
        QStringLiteral("path")
    );
    const QCommandLineOption storageOption(
        QStringLiteral("storage-root"),
        QStringLiteral("Store recordings below the absolute directory <path>."),
        QStringLiteral("path")
    );
    const QCommandLineOption outputOption(
        QStringLiteral("output"),
        QStringLiteral("Record the compositor output named <name>."),
        QStringLiteral("name")
    );
    const QCommandLineOption e2eOption(
        QStringLiteral("e2e-fake-capture"),
        QStringLiteral("Use the deterministic test capture platform.")
    );
    const QCommandLineOption smokeOverlayOption(
        QStringLiteral("smoke-overlay"),
        QStringLiteral("Exercise overlay construction without starting the service.")
    );
    parser.addOptions({
        backgroundOption,
        socketOption,
        storageOption,
        outputOption,
        e2eOption,
        smokeOverlayOption,
    });
    parser.process(application);

    const bool background = parser.isSet(backgroundOption);
    const bool e2e = parser.isSet(e2eOption)
        || qEnvironmentVariableIntValue("DICTA_NATIVE_E2E") != 0;
    if (background) {
        application.setQuitOnLastWindowClosed(false);
    }

    QQmlApplicationEngine engine;
    engine.addImageProvider(QStringLiteral("dicta-icons"), new ThemeIconProvider);
    OverlayController overlayController;
    QObject::connect(
        &overlayController,
        &OverlayController::errorOccurred,
        &application,
        [](const QString &message) { qCritical().noquote() << message; }
    );
    if (!overlayController.initialize(engine)) {
        return EXIT_FAILURE;
    }

    if (parser.isSet(smokeOverlayOption)) {
        if (!overlayController.showOnOutput(QString())
            || !overlayController.startRecordingClock()) {
            return EXIT_FAILURE;
        }
        overlayController.setTool(OverlayController::Rectangle);
        overlayController.enterAnnotationMode();
        overlayController.clear();
        overlayController.enterPassThroughMode();
        overlayController.finishAndHide();
        return EXIT_SUCCESS;
    }

    NativeBridge nativeBridge(overlayController);
    QObject::connect(
        &application,
        &QCoreApplication::aboutToQuit,
        &nativeBridge,
        &NativeBridge::stopHost
    );
    QObject::connect(
        &nativeBridge,
        &NativeBridge::hostFailed,
        &application,
        [&nativeBridge] {
            qCritical().noquote() << nativeBridge.hostError();
            QCoreApplication::exit(EXIT_FAILURE);
        },
        Qt::QueuedConnection
    );

    const QString socketPath = parser.isSet(socketOption)
        ? parser.value(socketOption)
        : environmentOrDefault("DICTA_SOCKET", defaultSocketPath());
    const QString storageRoot = parser.isSet(storageOption)
        ? parser.value(storageOption)
        : environmentOrDefault(
            "DICTA_STORAGE_ROOT",
            environmentOrDefault("DICTA_HOME", defaultStorageRoot())
        );
    QString outputName = parser.isSet(outputOption)
        ? parser.value(outputOption)
        : environmentOrDefault("DICTA_OUTPUT", defaultOutputName());
    if (outputName.isEmpty() && e2e) {
        outputName = QStringLiteral("E2E-1");
    }
    if (!nativeBridge.startHost(socketPath, storageRoot, outputName, e2e)) {
        qCritical().noquote() << nativeBridge.hostError();
        return EXIT_FAILURE;
    }

    QObject::connect(
        &engine,
        &QQmlApplicationEngine::objectCreationFailed,
        &application,
        [] { qCritical() << "The Dicta dashboard could not be created."; },
        Qt::QueuedConnection
    );
    const auto showMainWindow = [&engine, &nativeBridge, &themeBridge] {
        QQuickWindow *window = nullptr;
        for (QObject *object : engine.rootObjects()) {
            if (object->objectName() == QStringLiteral("dictaMainWindow")) {
                window = qobject_cast<QQuickWindow *>(object);
                break;
            }
        }
        if (window == nullptr) {
            engine.setInitialProperties({
                {QStringLiteral("bridge"), QVariant::fromValue(&nativeBridge)},
                {QStringLiteral("dictaTheme"), QVariant::fromValue(&themeBridge)},
            });
            engine.loadFromModule(QStringLiteral("Dicta.Native"), QStringLiteral("Main"));
            for (QObject *object : engine.rootObjects()) {
                if (object->objectName() == QStringLiteral("dictaMainWindow")) {
                    window = qobject_cast<QQuickWindow *>(object);
                    break;
                }
            }
        }
        if (window == nullptr) {
            qCritical() << "The Dicta dashboard window is unavailable.";
            return;
        }
        if (!window->property("dictaUiObserved").toBool()) {
            window->setProperty("dictaUiObserved", true);
            QObject::connect(window, &QWindow::visibleChanged, window, [window] {
                appendUiMarker(window->isVisible()
                    ? QStringLiteral("shown") : QStringLiteral("hidden"));
            });
        }
        window->show();
        window->setWindowStates(window->windowStates() & ~Qt::WindowMinimized);
        window->raise();
        window->requestActivate();
        appendUiMarker(QStringLiteral("shown"));

        bool hideDelayOk = false;
        const int hideDelay = qEnvironmentVariableIntValue(
            "DICTA_NATIVE_E2E_HIDE_UI_AFTER_MS",
            &hideDelayOk
        );
        if (hideDelayOk && hideDelay > 0) {
            const QPointer<QQuickWindow> guardedWindow(window);
            QTimer::singleShot(hideDelay, window, [guardedWindow] {
                if (guardedWindow != nullptr) {
                    guardedWindow->hide();
                }
            });
        }
    };
    QObject::connect(
        &nativeBridge,
        &NativeBridge::uiShowRequested,
        &application,
        showMainWindow,
        Qt::QueuedConnection
    );

    if (qEnvironmentVariableIntValue("DICTA_NATIVE_E2E_EXIT_AFTER_STOP") != 0) {
        QObject::connect(
            &overlayController,
            &OverlayController::sessionFinished,
            &application,
            &QCoreApplication::quit,
            Qt::QueuedConnection
        );
    }
    bool maximumRuntimeOk = false;
    const int maximumRuntime = qEnvironmentVariableIntValue(
        "DICTA_NATIVE_E2E_MAX_MS",
        &maximumRuntimeOk
    );
    if (e2e && maximumRuntimeOk && maximumRuntime > 0) {
        QTimer::singleShot(maximumRuntime, &application, &QCoreApplication::quit);
    }

    if (!background) {
        showMainWindow();
    }

    return application.exec();
}
