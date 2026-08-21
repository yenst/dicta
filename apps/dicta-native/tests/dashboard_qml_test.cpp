#include <QGuiApplication>
#include <QColor>
#include <QDir>
#include <QFileInfo>
#include <QImage>
#include <QQuickWindow>
#include <QQmlComponent>
#include <QQmlEngine>
#include <QScopedPointer>
#include <QTest>
#include <QVariantList>

#include "theme_icon_provider.h"

class DashboardBridge final : public QObject
{
    Q_OBJECT
    Q_PROPERTY(QString hostState MEMBER hostState NOTIFY changed)
    Q_PROPERTY(QString hostError MEMBER hostError NOTIFY changed)
    Q_PROPERTY(QString runtimePhase MEMBER runtimePhase NOTIFY changed)
    Q_PROPERTY(QString activeRecordingId MEMBER activeRecordingId NOTIFY changed)
    Q_PROPERTY(bool annotationsEnabled MEMBER annotationsEnabled NOTIFY changed)
    Q_PROPERTY(QString annotationTool MEMBER annotationTool NOTIFY changed)
    Q_PROPERTY(QVariantList projects MEMBER projects NOTIFY changed)
    Q_PROPERTY(QVariantMap currentProject MEMBER currentProject NOTIFY changed)
    Q_PROPERTY(QVariantMap modelStatus MEMBER modelStatus NOTIFY changed)
    Q_PROPERTY(QVariantMap settings MEMBER settings NOTIFY changed)
    Q_PROPERTY(QVariantMap codexMcp MEMBER codexMcp NOTIFY changed)
    Q_PROPERTY(QVariantMap voiceNoteStatus MEMBER voiceNoteStatus NOTIFY selectedRecordingChanged)
    Q_PROPERTY(QString settingsMessage MEMBER settingsMessage NOTIFY changed)
    Q_PROPERTY(QVariantList recentRecordings MEMBER recentRecordings NOTIFY changed)
    Q_PROPERTY(QString uiError MEMBER uiError NOTIFY changed)
    Q_PROPERTY(QVariantMap selectedRecording MEMBER selectedRecording NOTIFY changed)
    Q_PROPERTY(QString selectedRecordingId MEMBER selectedRecordingId NOTIFY changed)
    Q_PROPERTY(bool multimediaAvailable MEMBER multimediaAvailable CONSTANT)

public:
    QString hostState = QStringLiteral("running");
    QString hostError;
    QString runtimePhase = QStringLiteral("idle");
    QString activeRecordingId;
    bool annotationsEnabled = false;
    QString annotationTool;
    QVariantList projects;
    QVariantMap currentProject;
    QVariantMap modelStatus {
        {QStringLiteral("active_model"), QStringLiteral("Compact")},
        {QStringLiteral("quality_state"), QStringLiteral("ready")},
        {QStringLiteral("message"), QStringLiteral("ready")},
    };
    QVariantMap settings {
        {QStringLiteral("shortcut_id"), QStringLiteral("alt_shift_r")},
        {QStringLiteral("cleanup_merged_videos"), true},
        {QStringLiteral("branch_locking"), true},
        {QStringLiteral("transcription_language"), QStringLiteral("auto")},
        {QStringLiteral("general_path"), QVariant()},
    };
    QVariantMap codexMcp {
        {QStringLiteral("state"), QStringLiteral("disconnected")},
        {QStringLiteral("codex_path"), QStringLiteral("/usr/bin/codex")},
        {QStringLiteral("mcp_path"), QStringLiteral("/usr/lib/Dicta/dicta-mcp")},
        {QStringLiteral("message"), QStringLiteral("Dicta is not registered with Codex.")},
    };
    QVariantMap voiceNoteStatus {{QStringLiteral("state"), QStringLiteral("idle")}};
    QString settingsMessage;
    QVariantList recentRecordings;
    QString uiError;
    QVariantMap selectedRecording;
    QString selectedRecordingId;
    bool multimediaAvailable = false;
    QString submittedNote;
    int startCalls = 0;
    int stopCalls = 0;
    int selectCalls = 0;
    int closeCalls = 0;
    int deleteCalls = 0;
    int transcribeCalls = 0;
    int addTimelineNoteCalls = 0;
    int removeTimelineNoteCalls = 0;
    QString submittedTimelineNote;
    double submittedTimelineTimestamp = -1.0;
    int annotationEnableCalls = 0;
    int annotationDisableCalls = 0;
    int annotationToolCalls = 0;
    int annotationUndoCalls = 0;
    int annotationClearCalls = 0;
    int refreshCalls = 0;
    int projectSelectCalls = 0;
    int createProjectCalls = 0;
    int copyCalls = 0;
    int revealCalls = 0;
    int openCalls = 0;
    int settingCalls = 0;
    int cleanupCalls = 0;
    int installModelCalls = 0;
    int codexStatusCalls = 0;
    int codexConnectCalls = 0;
    int codexRestartCalls = 0;
    int voiceStartCalls = 0;
    int voiceStopCalls = 0;
    int voiceCancelCalls = 0;
    QString requestedAnnotationTool;

    Q_INVOKABLE bool startRecording(const QString &note)
    {
        submittedNote = note;
        ++startCalls;
        return true;
    }

    Q_INVOKABLE bool stopRecording()
    {
        ++stopCalls;
        return true;
    }

    Q_INVOKABLE bool setAnnotationsEnabled(const bool enabled)
    {
        annotationsEnabled = enabled;
        if (enabled) {
            ++annotationEnableCalls;
            if (annotationTool.isEmpty()) {
                annotationTool = QStringLiteral("pen");
            }
        } else {
            ++annotationDisableCalls;
            annotationTool.clear();
        }
        emit changed();
        return true;
    }

    Q_INVOKABLE bool chooseAnnotationTool(const QString &tool)
    {
        ++annotationToolCalls;
        requestedAnnotationTool = tool;
        annotationTool = tool;
        emit changed();
        return true;
    }

    Q_INVOKABLE bool undoAnnotation()
    {
        ++annotationUndoCalls;
        return true;
    }

    Q_INVOKABLE bool clearAnnotations()
    {
        ++annotationClearCalls;
        return true;
    }

    Q_INVOKABLE bool refreshDashboard()
    {
        ++refreshCalls;
        return true;
    }

    Q_INVOKABLE bool selectProject(const QString &projectId)
    {
        ++projectSelectCalls;
        currentProject = QVariantMap {
            {QStringLiteral("id"), projectId},
            {QStringLiteral("name"), projectId},
            {QStringLiteral("selected"), true},
        };
        emit changed();
        emit dashboardChanged();
        return true;
    }

    Q_INVOKABLE bool createProject(const QString &name)
    {
        ++createProjectCalls;
        return !name.trimmed().isEmpty();
    }

    Q_INVOKABLE bool selectRecording(const QString &recordingId)
    {
        ++selectCalls;
        selectedRecordingId = recordingId;
        selectedRecording = QVariantMap {
            {QStringLiteral("id"), recordingId},
            {QStringLiteral("note"), QStringLiteral("A deterministic detail note")},
            {QStringLiteral("transcript"), QStringLiteral("Fake transcript")},
            {QStringLiteral("transcription_status"), QStringLiteral("complete")},
            {QStringLiteral("duration_seconds"), 42.0},
            {QStringLiteral("success"), true},
            {QStringLiteral("video_url"), QStringLiteral("file:///tmp/fake.mp4")},
            {QStringLiteral("timeline_notes"), QVariantList {}},
        };
        emit changed();
        emit selectedRecordingChanged();
        return true;
    }

    Q_INVOKABLE void closeRecording()
    {
        ++closeCalls;
        selectedRecordingId.clear();
        selectedRecording.clear();
        emit changed();
        emit selectedRecordingChanged();
    }

    Q_INVOKABLE bool deleteSelectedRecording()
    {
        ++deleteCalls;
        closeRecording();
        return true;
    }

    Q_INVOKABLE bool transcribeSelectedRecording()
    {
        ++transcribeCalls;
        return true;
    }

    Q_INVOKABLE bool addTimelineNote(const QString &text, const double timestampSeconds)
    {
        ++addTimelineNoteCalls;
        submittedTimelineNote = text;
        submittedTimelineTimestamp = timestampSeconds;
        QVariantList notes = selectedRecording
            .value(QStringLiteral("timeline_notes"))
            .toList();
        notes.append(QVariantMap {
            {QStringLiteral("id"), QStringLiteral("fake-note")},
            {QStringLiteral("timestamp_seconds"), timestampSeconds},
            {QStringLiteral("text"), text},
            {QStringLiteral("source"), QStringLiteral("typed")},
        });
        selectedRecording.insert(QStringLiteral("timeline_notes"), notes);
        emit changed();
        emit selectedRecordingChanged();
        return true;
    }

    Q_INVOKABLE bool removeTimelineNote(const QString &noteId)
    {
        ++removeTimelineNoteCalls;
        QVariantList notes;
        for (const QVariant &entry : selectedRecording
                 .value(QStringLiteral("timeline_notes"))
                 .toList()) {
            if (entry.toMap().value(QStringLiteral("id")).toString() != noteId) {
                notes.append(entry);
            }
        }
        selectedRecording.insert(QStringLiteral("timeline_notes"), notes);
        emit changed();
        emit selectedRecordingChanged();
        return true;
    }

    Q_INVOKABLE bool copySelectedContext()
    {
        ++copyCalls;
        return true;
    }

    Q_INVOKABLE bool revealSelectedRecording()
    {
        ++revealCalls;
        return true;
    }

    Q_INVOKABLE bool openSelectedRecording()
    {
        ++openCalls;
        return true;
    }

    Q_INVOKABLE bool setShortcut(const QString &shortcutId)
    {
        ++settingCalls;
        settings.insert(QStringLiteral("shortcut_id"), shortcutId);
        emit changed();
        emit dashboardChanged();
        return true;
    }

    Q_INVOKABLE bool setCleanupMergedVideos(const bool enabled)
    {
        ++settingCalls;
        settings.insert(QStringLiteral("cleanup_merged_videos"), enabled);
        emit changed();
        emit dashboardChanged();
        return true;
    }

    Q_INVOKABLE bool setBranchLocking(const bool enabled)
    {
        ++settingCalls;
        settings.insert(QStringLiteral("branch_locking"), enabled);
        emit changed();
        emit dashboardChanged();
        return true;
    }

    Q_INVOKABLE bool setTranscriptionLanguage(const QString &language)
    {
        ++settingCalls;
        settings.insert(QStringLiteral("transcription_language"), language);
        emit changed();
        emit dashboardChanged();
        return true;
    }

    Q_INVOKABLE bool setGeneralPath(const QString &path)
    {
        ++settingCalls;
        settings.insert(QStringLiteral("general_path"), path);
        emit changed();
        emit dashboardChanged();
        return true;
    }

    Q_INVOKABLE bool cleanupMergedVideos()
    {
        ++cleanupCalls;
        settingsMessage = QStringLiteral("Removed 2 merged videos.");
        emit changed();
        emit dashboardChanged();
        return true;
    }

    Q_INVOKABLE bool installQualityModel()
    {
        ++installModelCalls;
        modelStatus.insert(QStringLiteral("quality_state"), QStringLiteral("installing"));
        modelStatus.insert(QStringLiteral("install_stage"), QStringLiteral("downloading"));
        emit changed();
        emit dashboardChanged();
        return true;
    }

    Q_INVOKABLE bool refreshCodexMcp()
    {
        ++codexStatusCalls;
        return true;
    }

    Q_INVOKABLE bool connectCodexMcp()
    {
        ++codexConnectCalls;
        codexMcp.insert(QStringLiteral("state"), QStringLiteral("connected"));
        emit changed();
        return true;
    }

    Q_INVOKABLE bool restartCodexMcp()
    {
        ++codexRestartCalls;
        return true;
    }

    Q_INVOKABLE bool startVoiceNote(const double timestampSeconds)
    {
        Q_UNUSED(timestampSeconds)
        ++voiceStartCalls;
        voiceNoteStatus = QVariantMap {
            {QStringLiteral("state"), QStringLiteral("recording")},
            {QStringLiteral("message"), QStringLiteral("Listening…")},
        };
        emit selectedRecordingChanged();
        return true;
    }

    Q_INVOKABLE bool stopVoiceNote()
    {
        ++voiceStopCalls;
        voiceNoteStatus = QVariantMap {
            {QStringLiteral("state"), QStringLiteral("processing")},
            {QStringLiteral("message"), QStringLiteral("Transcribing voice note…")},
        };
        emit selectedRecordingChanged();
        return true;
    }

    Q_INVOKABLE bool cancelVoiceNote()
    {
        ++voiceCancelCalls;
        voiceNoteStatus = QVariantMap {{QStringLiteral("state"), QStringLiteral("idle")}};
        emit selectedRecordingChanged();
        return true;
    }

signals:
    void changed();
    void dashboardChanged();
    void selectedRecordingChanged();
};

class DashboardTheme final : public QObject
{
    Q_OBJECT
    Q_PROPERTY(QString name MEMBER name CONSTANT)
    Q_PROPERTY(QString mode MEMBER mode CONSTANT)
    Q_PROPERTY(QString fontFamily MEMBER fontFamily CONSTANT)
    Q_PROPERTY(int baseFontSize MEMBER baseFontSize CONSTANT)
    Q_PROPERTY(qreal spacingScale MEMBER spacingScale CONSTANT)
    Q_PROPERTY(QColor accent MEMBER accent CONSTANT)
    Q_PROPERTY(QColor selection MEMBER selection CONSTANT)
    Q_PROPERTY(QColor muted MEMBER muted CONSTANT)
    Q_PROPERTY(QColor background MEMBER background CONSTANT)
    Q_PROPERTY(QColor darkBackground MEMBER darkBackground CONSTANT)
    Q_PROPERTY(QColor darkerBackground MEMBER darkerBackground CONSTANT)
    Q_PROPERTY(QColor lighterBackground MEMBER lighterBackground CONSTANT)
    Q_PROPERTY(QColor foreground MEMBER foreground CONSTANT)
    Q_PROPERTY(QColor darkForeground MEMBER darkForeground CONSTANT)
    Q_PROPERTY(QColor lightForeground MEMBER lightForeground CONSTANT)
    Q_PROPERTY(QColor brightForeground MEMBER brightForeground CONSTANT)
    Q_PROPERTY(QColor red MEMBER red CONSTANT)
    Q_PROPERTY(QColor yellow MEMBER yellow CONSTANT)
    Q_PROPERTY(QColor orange MEMBER orange CONSTANT)
    Q_PROPERTY(QColor green MEMBER green CONSTANT)
    Q_PROPERTY(QColor cyan MEMBER cyan CONSTANT)
    Q_PROPERTY(QColor blue MEMBER blue CONSTANT)
    Q_PROPERTY(QColor magenta MEMBER magenta CONSTANT)

public:
    QString name = QStringLiteral("tokyo-night");
    QString mode = QStringLiteral("dark");
    QString fontFamily = QStringLiteral("monospace");
    int baseFontSize = 14;
    qreal spacingScale = 1.0;
    QColor accent {QStringLiteral("#7aa2f7")};
    QColor selection {QStringLiteral("#292e42")};
    QColor muted {QStringLiteral("#414868")};
    QColor background {QStringLiteral("#1a1b26")};
    QColor darkBackground {QStringLiteral("#13141c")};
    QColor darkerBackground {QStringLiteral("#0e0e14")};
    QColor lighterBackground {QStringLiteral("#24283b")};
    QColor foreground {QStringLiteral("#a9b1d6")};
    QColor darkForeground {QStringLiteral("#565f89")};
    QColor lightForeground {QStringLiteral("#b4bee6")};
    QColor brightForeground {QStringLiteral("#c0caf5")};
    QColor red {QStringLiteral("#f7768e")};
    QColor yellow {QStringLiteral("#e0af68")};
    QColor orange {QStringLiteral("#eb927b")};
    QColor green {QStringLiteral("#9ece6a")};
    QColor cyan {QStringLiteral("#449dab")};
    QColor blue {QStringLiteral("#7aa2f7")};
    QColor magenta {QStringLiteral("#ad8ee6")};
};

class DashboardQmlTest final : public QObject
{
    Q_OBJECT

private slots:
    void idleDashboardRendersRecentRecordingAndStartsWithNote();
    void recordingDashboardInvokesStop();
    void recentSelectionShowsLazyDetailAndBackReturnsToCapture();
    void detailActionsRequireConfirmationAndUseSupportedCommands();
    void timelineNotesUseThePlaybackCursorAndTypedBridge();
    void narrowRecordingAnnotationControlsAreKeyboardAccessible();
    void filterShortcutOpensTheContextSearch();
    void settingsControlsUpdateTheTypedNativeBridge();
    void visualQaCapture();
};

namespace {
QObject *createDashboard(
    QQmlComponent &component,
    DashboardBridge &bridge,
    DashboardTheme &theme
)
{
    const QVariantMap properties {
        {QStringLiteral("bridge"), QVariant::fromValue(static_cast<QObject *>(&bridge))},
        {QStringLiteral("dictaTheme"), QVariant::fromValue(static_cast<QObject *>(&theme))},
        {QStringLiteral("autoOpenLatest"), false},
    };
    return component.createWithInitialProperties(properties);
}

QQmlComponent dashboardComponent(QQmlEngine &engine)
{
    engine.addImageProvider(QStringLiteral("dicta-icons"), new ThemeIconProvider);
    return QQmlComponent(
        &engine,
        QUrl::fromLocalFile(QStringLiteral(DICTA_MAIN_QML_PATH))
    );
}
}

void DashboardQmlTest::idleDashboardRendersRecentRecordingAndStartsWithNote()
{
    DashboardBridge bridge;
    DashboardTheme theme;
    bridge.recentRecordings.append(QVariantMap {
        {QStringLiteral("id"), QStringLiteral("demo-001")},
        {QStringLiteral("project"), QStringLiteral("General")},
        {QStringLiteral("duration_seconds"), 42.0},
        {QStringLiteral("transcription"), QStringLiteral("complete")},
    });
    QQmlEngine engine;
    QQmlComponent component = dashboardComponent(engine);
    QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    QScopedPointer<QObject> dashboard(createDashboard(component, bridge, theme));
    QVERIFY2(dashboard, qPrintable(component.errorString()));
    QObject *list = dashboard->findChild<QObject *>(QStringLiteral("recentRecordingsList"));
    QObject *note = dashboard->findChild<QObject *>(QStringLiteral("sessionNote"));
    QObject *toggle = dashboard->findChild<QObject *>(QStringLiteral("recordToggle"));
    QVERIFY(list != nullptr);
    QVERIFY(note != nullptr);
    QVERIFY(toggle != nullptr);
    QObject *annotationControls = dashboard->findChild<QObject *>(
        QStringLiteral("annotationControls")
    );
    QVERIFY(annotationControls != nullptr);
    QVERIFY(!annotationControls->property("visible").toBool());
    QCOMPARE(list->property("count").toInt(), 1);

    note->setProperty("text", QStringLiteral("Explain the native library"));
    QVERIFY(QMetaObject::invokeMethod(toggle, "clicked"));

    QCOMPARE(bridge.startCalls, 1);
    QCOMPARE(bridge.stopCalls, 0);
    QCOMPARE(bridge.submittedNote, QStringLiteral("Explain the native library"));
}

void DashboardQmlTest::recordingDashboardInvokesStop()
{
    DashboardBridge bridge;
    DashboardTheme theme;
    bridge.runtimePhase = QStringLiteral("recording");
    bridge.activeRecordingId = QStringLiteral("active-001");
    QQmlEngine engine;
    QQmlComponent component = dashboardComponent(engine);
    QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    QScopedPointer<QObject> dashboard(createDashboard(component, bridge, theme));
    QVERIFY2(dashboard, qPrintable(component.errorString()));
    QObject *toggle = dashboard->findChild<QObject *>(QStringLiteral("recordToggle"));
    QVERIFY(toggle != nullptr);

    QVERIFY(QMetaObject::invokeMethod(toggle, "clicked"));

    QCOMPARE(bridge.startCalls, 0);
    QCOMPARE(bridge.stopCalls, 1);
}

void DashboardQmlTest::recentSelectionShowsLazyDetailAndBackReturnsToCapture()
{
    DashboardBridge bridge;
    DashboardTheme theme;
    bridge.recentRecordings.append(QVariantMap {
        {QStringLiteral("id"), QStringLiteral("demo-001")},
        {QStringLiteral("project"), QStringLiteral("General")},
        {QStringLiteral("duration_seconds"), 42.0},
        {QStringLiteral("transcription"), QStringLiteral("complete")},
    });
    QQmlEngine engine;
    QQmlComponent component = dashboardComponent(engine);
    QScopedPointer<QObject> dashboard(createDashboard(component, bridge, theme));
    QVERIFY2(dashboard, qPrintable(component.errorString()));
    QObject *detail = dashboard->findChild<QObject *>(QStringLiteral("recordingDetailPage"));
    QObject *player = dashboard->findChild<QObject *>(QStringLiteral("recordingPlayerLoader"));
    QObject *back = dashboard->findChild<QObject *>(QStringLiteral("backToCapture"));
    QVERIFY(detail != nullptr);
    QVERIFY(player != nullptr);
    QVERIFY(back != nullptr);

    QVERIFY(QMetaObject::invokeMethod(
        dashboard.get(),
        "showRecording",
        Q_ARG(QVariant, QVariant(QStringLiteral("demo-001")))
    ));
    QTRY_COMPARE(bridge.selectCalls, 1);
    QTRY_VERIFY(detail->property("visible").toBool());
    QVERIFY(!player->property("active").toBool());
    QVERIFY(QMetaObject::invokeMethod(back, "clicked"));
    QCOMPARE(bridge.closeCalls, 1);
    QTRY_VERIFY(!detail->property("visible").toBool());
}

void DashboardQmlTest::detailActionsRequireConfirmationAndUseSupportedCommands()
{
    DashboardBridge bridge;
    DashboardTheme theme;
    bridge.selectRecording(QStringLiteral("demo-002"));
    QQmlEngine engine;
    QQmlComponent component = dashboardComponent(engine);
    QScopedPointer<QObject> dashboard(createDashboard(component, bridge, theme));
    QVERIFY2(dashboard, qPrintable(component.errorString()));
    QObject *transcribe = dashboard->findChild<QObject *>(QStringLiteral("transcribeRecording"));
    QObject *remove = dashboard->findChild<QObject *>(QStringLiteral("deleteRecording"));
    QVERIFY(transcribe != nullptr);
    QVERIFY(remove != nullptr);

    QVERIFY(QMetaObject::invokeMethod(transcribe, "clicked"));
    QCOMPARE(bridge.transcribeCalls, 1);
    QVERIFY(QMetaObject::invokeMethod(remove, "clicked"));
    QCOMPARE(bridge.deleteCalls, 0);
    QVERIFY(QMetaObject::invokeMethod(remove, "clicked"));
    QCOMPARE(bridge.deleteCalls, 1);
}

void DashboardQmlTest::timelineNotesUseThePlaybackCursorAndTypedBridge()
{
    DashboardBridge bridge;
    DashboardTheme theme;
    bridge.selectRecording(QStringLiteral("demo-notes"));
    QQmlEngine engine;
    QQmlComponent component = dashboardComponent(engine);
    QScopedPointer<QObject> dashboard(createDashboard(component, bridge, theme));
    QVERIFY2(dashboard, qPrintable(component.errorString()));
    QObject *detail = dashboard->findChild<QObject *>(QStringLiteral("recordingDetailPage"));
    QObject *field = dashboard->findChild<QObject *>(QStringLiteral("timelineNoteField"));
    QObject *add = dashboard->findChild<QObject *>(QStringLiteral("addTimelineNote"));
    QObject *voiceRecord = dashboard->findChild<QObject *>(QStringLiteral("voiceNoteRecord"));
    QObject *voiceStop = dashboard->findChild<QObject *>(QStringLiteral("voiceNoteStop"));
    QObject *voiceCancel = dashboard->findChild<QObject *>(QStringLiteral("voiceNoteCancel"));
    QVERIFY(detail != nullptr);
    QVERIFY(field != nullptr);
    QVERIFY(add != nullptr);
    QVERIFY(voiceRecord != nullptr);
    QVERIFY(voiceStop != nullptr);
    QVERIFY(voiceCancel != nullptr);
    detail->setProperty("currentTab", 2);
    field->setProperty("text", QStringLiteral("Check this transition"));
    QVERIFY(QMetaObject::invokeMethod(add, "clicked"));
    QCOMPARE(bridge.addTimelineNoteCalls, 1);
    QCOMPARE(bridge.submittedTimelineNote, QStringLiteral("Check this transition"));
    QCOMPARE(bridge.submittedTimelineTimestamp, 0.0);
    QCOMPARE(field->property("text").toString(), QString());
    QCOMPARE(
        bridge.selectedRecording.value(QStringLiteral("timeline_notes")).toList().size(),
        1
    );
    QVERIFY(QMetaObject::invokeMethod(voiceRecord, "clicked"));
    QCOMPARE(bridge.voiceStartCalls, 1);
    QVERIFY(QMetaObject::invokeMethod(voiceStop, "clicked"));
    QCOMPARE(bridge.voiceStopCalls, 1);
    QVERIFY(QMetaObject::invokeMethod(voiceCancel, "clicked"));
    QCOMPARE(bridge.voiceCancelCalls, 1);
}

void DashboardQmlTest::narrowRecordingAnnotationControlsAreKeyboardAccessible()
{
    DashboardBridge bridge;
    DashboardTheme theme;
    bridge.runtimePhase = QStringLiteral("recording");
    bridge.activeRecordingId = QStringLiteral("active-compact");
    QQmlEngine engine;
    QQmlComponent component = dashboardComponent(engine);
    QScopedPointer<QObject> dashboard(createDashboard(component, bridge, theme));
    QVERIFY2(dashboard, qPrintable(component.errorString()));
    dashboard->setProperty("width", 720);
    dashboard->setProperty("height", 900);
    auto *window = qobject_cast<QQuickWindow *>(dashboard.get());
    QVERIFY(window != nullptr);
    QTRY_VERIFY(window->isVisible());

    QObject *controls = dashboard->findChild<QObject *>(QStringLiteral("annotationControls"));
    QObject *toggle = dashboard->findChild<QObject *>(QStringLiteral("annotationToggle"));
    QObject *spotlight = dashboard->findChild<QObject *>(
        QStringLiteral("annotationToolSpotlight")
    );
    QObject *undo = dashboard->findChild<QObject *>(QStringLiteral("annotationUndo"));
    QObject *clear = dashboard->findChild<QObject *>(QStringLiteral("annotationClear"));
    QVERIFY(controls != nullptr);
    QVERIFY(toggle != nullptr);
    QVERIFY(spotlight != nullptr);
    QVERIFY(undo != nullptr);
    QVERIFY(clear != nullptr);
    QVERIFY(controls->property("visible").toBool());
    QVERIFY(!spotlight->property("enabled").toBool());

    QVERIFY(QMetaObject::invokeMethod(toggle, "forceActiveFocus"));
    QTest::keyClick(window, Qt::Key_Space);
    QTRY_COMPARE(bridge.annotationEnableCalls, 1);
    QTRY_VERIFY(spotlight->property("enabled").toBool());

    QVERIFY(QMetaObject::invokeMethod(spotlight, "forceActiveFocus"));
    QTest::keyClick(window, Qt::Key_Space);
    QTRY_COMPARE(bridge.annotationToolCalls, 1);
    QCOMPARE(bridge.requestedAnnotationTool, QStringLiteral("spotlight"));

    QVERIFY(QMetaObject::invokeMethod(undo, "forceActiveFocus"));
    QTest::keyClick(window, Qt::Key_Space);
    QTRY_COMPARE(bridge.annotationUndoCalls, 1);
    QVERIFY(QMetaObject::invokeMethod(clear, "forceActiveFocus"));
    QTest::keyClick(window, Qt::Key_Space);
    QTRY_COMPARE(bridge.annotationClearCalls, 1);

    QVERIFY(QMetaObject::invokeMethod(toggle, "forceActiveFocus"));
    QTest::keyClick(window, Qt::Key_Space);
    QTRY_COMPARE(bridge.annotationDisableCalls, 1);
}

void DashboardQmlTest::filterShortcutOpensTheContextSearch()
{
    DashboardBridge bridge;
    DashboardTheme theme;
    QQmlEngine engine;
    QQmlComponent component = dashboardComponent(engine);
    QScopedPointer<QObject> dashboard(createDashboard(component, bridge, theme));
    QVERIFY2(dashboard, qPrintable(component.errorString()));
    auto *window = qobject_cast<QQuickWindow *>(dashboard.get());
    QVERIFY(window != nullptr);
    window->show();
    QTRY_VERIFY(window->isVisible());
    QObject *panel = dashboard->findChild<QObject *>(QStringLiteral("recordingPanel"));
    QVERIFY(panel != nullptr);
    QVERIFY(!panel->property("filterVisible").toBool());

    QTest::keyClick(window, Qt::Key_K, Qt::ControlModifier);
    QTRY_VERIFY(panel->property("filterVisible").toBool());
}

void DashboardQmlTest::settingsControlsUpdateTheTypedNativeBridge()
{
    DashboardBridge bridge;
    bridge.modelStatus.insert(QStringLiteral("quality_state"), QStringLiteral("missing"));
    DashboardTheme theme;
    QQmlEngine engine;
    QQmlComponent component = dashboardComponent(engine);
    QScopedPointer<QObject> dashboard(createDashboard(component, bridge, theme));
    QVERIFY2(dashboard, qPrintable(component.errorString()));
    dashboard->setProperty("settingsOpen", true);
    bridge.currentProject = QVariantMap {
        {QStringLiteral("id"), QStringLiteral("dicta")},
        {QStringLiteral("name"), QStringLiteral("dicta")},
    };
    emit bridge.changed();
    auto *window = qobject_cast<QQuickWindow *>(dashboard.get());
    QVERIFY(window != nullptr);
    window->show();
    QTRY_VERIFY(window->isVisible());

    QObject *branch = nullptr;
    QObject *cleanup = nullptr;
    QObject *generalPath = nullptr;
    QObject *generalApply = nullptr;
    QObject *cleanupNow = nullptr;
    QObject *installModel = nullptr;
    QObject *connectCodex = nullptr;
    QObject *restartCodex = nullptr;
    QTRY_VERIFY((branch = dashboard->findChild<QObject *>(
        QStringLiteral("branchLockingToggle"))) != nullptr);
    QTRY_VERIFY((cleanup = dashboard->findChild<QObject *>(
        QStringLiteral("cleanupToggle"))) != nullptr);
    QTRY_VERIFY((generalPath = dashboard->findChild<QObject *>(
        QStringLiteral("generalPathField"))) != nullptr);
    QTRY_VERIFY((generalApply = dashboard->findChild<QObject *>(
        QStringLiteral("generalPathApply"))) != nullptr);
    QTRY_VERIFY((cleanupNow = dashboard->findChild<QObject *>(
        QStringLiteral("cleanupNow"))) != nullptr);
    QTRY_VERIFY((installModel = dashboard->findChild<QObject *>(
        QStringLiteral("installQualityModel"))) != nullptr);
    QTRY_VERIFY((connectCodex = dashboard->findChild<QObject *>(
        QStringLiteral("connectCodexMcp"))) != nullptr);
    QTRY_VERIFY((restartCodex = dashboard->findChild<QObject *>(
        QStringLiteral("restartCodexMcp"))) != nullptr);

    QVERIFY(QMetaObject::invokeMethod(
        dashboard.get(),
        "updateTranscriptionLanguage",
        Q_ARG(QVariant, QVariant(QStringLiteral("en")))
    ));
    QVERIFY(QMetaObject::invokeMethod(branch, "clicked"));
    QVERIFY(QMetaObject::invokeMethod(cleanup, "clicked"));
    QVERIFY(QMetaObject::invokeMethod(
        dashboard.get(),
        "updateShortcut",
        Q_ARG(QVariant, QVariant(QStringLiteral("control_space")))
    ));
    generalPath->setProperty("text", QStringLiteral("/tmp/dicta-general"));
    QVERIFY(QMetaObject::invokeMethod(generalApply, "clicked"));
    QVERIFY(QMetaObject::invokeMethod(cleanupNow, "clicked"));
    QVERIFY(QMetaObject::invokeMethod(installModel, "clicked"));
    QVERIFY(QMetaObject::invokeMethod(connectCodex, "clicked"));
    QVERIFY(QMetaObject::invokeMethod(restartCodex, "clicked"));

    QCOMPARE(bridge.settingCalls, 5);
    QCOMPARE(bridge.cleanupCalls, 1);
    QCOMPARE(bridge.installModelCalls, 1);
    QVERIFY(bridge.codexStatusCalls >= 1);
    QCOMPARE(bridge.codexConnectCalls, 1);
    QCOMPARE(bridge.codexRestartCalls, 1);
    QCOMPARE(bridge.modelStatus.value(QStringLiteral("quality_state")).toString(),
             QStringLiteral("installing"));
    QCOMPARE(bridge.settingsMessage, QStringLiteral("Removed 2 merged videos."));
    QCOMPARE(bridge.settings.value(QStringLiteral("transcription_language")).toString(),
             QStringLiteral("en"));
    QCOMPARE(bridge.settings.value(QStringLiteral("branch_locking")).toBool(), false);
    QCOMPARE(bridge.settings.value(QStringLiteral("cleanup_merged_videos")).toBool(), false);
    QCOMPARE(bridge.settings.value(QStringLiteral("shortcut_id")).toString(),
             QStringLiteral("control_space"));
    QCOMPARE(bridge.settings.value(QStringLiteral("general_path")).toString(),
             QStringLiteral("/tmp/dicta-general"));
}

void DashboardQmlTest::visualQaCapture()
{
    const QString outputPath = qEnvironmentVariable("DICTA_DESIGN_QA_OUTPUT");
    if (outputPath.isEmpty()) {
        QSKIP("Set DICTA_DESIGN_QA_OUTPUT to capture the deterministic visual fixture");
    }

    DashboardBridge bridge;
    DashboardTheme theme;
    bridge.projects = QVariantList {
        QVariantMap {{QStringLiteral("id"), QStringLiteral("general")},
                     {QStringLiteral("name"), QStringLiteral("General")},
                     {QStringLiteral("path"), QStringLiteral("all recordings")},
                     {QStringLiteral("selected"), false}},
        QVariantMap {{QStringLiteral("id"), QStringLiteral("dicta")},
                     {QStringLiteral("name"), QStringLiteral("dicta")},
                     {QStringLiteral("path"), QStringLiteral("/home/jihmy/Projects/dicta")},
                     {QStringLiteral("selected"), true}},
        QVariantMap {{QStringLiteral("id"), QStringLiteral("placeholder")},
                     {QStringLiteral("name"), QStringLiteral("placeholder")},
                     {QStringLiteral("path"), QStringLiteral("/home/jihmy/Projects/placeholder")},
                     {QStringLiteral("selected"), false}},
        QVariantMap {{QStringLiteral("id"), QStringLiteral("peepel")},
                     {QStringLiteral("name"), QStringLiteral("peepel")},
                     {QStringLiteral("path"), QStringLiteral("securex-historical-import")},
                     {QStringLiteral("selected"), false}},
    };
    bridge.currentProject = QVariantMap {
        {QStringLiteral("id"), QStringLiteral("dicta")},
        {QStringLiteral("name"), QStringLiteral("dicta")},
        {QStringLiteral("path"), QStringLiteral("~/Projects/dicta")},
        {QStringLiteral("selected"), true},
    };
    const auto summary = [](const QString &id, const QString &time, const QString &note,
                            const QString &preview, const double duration,
                            const QString &status) {
        return QVariantMap {
            {QStringLiteral("id"), id},
            {QStringLiteral("project"), QStringLiteral("dicta")},
            {QStringLiteral("branch"), QStringLiteral("main")},
            {QStringLiteral("started_at"), QStringLiteral("2026-08-20T") + time
                + QStringLiteral(":00+02:00")},
            {QStringLiteral("note"), note},
            {QStringLiteral("transcript_preview"), preview},
            {QStringLiteral("duration_seconds"), duration},
            {QStringLiteral("transcription"), status},
        };
    };
    bridge.recentRecordings = QVariantList {
        summary(QStringLiteral("rec-2018"), QStringLiteral("20:18"),
                QStringLiteral("Refine compact viewer"),
                QStringLiteral("Stockbar looks stupid. Not responsive."), 151.0,
                QStringLiteral("complete")),
        summary(QStringLiteral("rec-1942"), QStringLiteral("19:42"),
                QStringLiteral("Omarchy plugin flow"),
                QStringLiteral("Walk through auth + settings."), 194.0,
                QStringLiteral("complete")),
        summary(QStringLiteral("rec-1806"), QStringLiteral("18:06"),
                QStringLiteral("Stockbar responsiveness"),
                QStringLiteral("Investigate overflow + layout."), 108.0,
                QStringLiteral("complete")),
        summary(QStringLiteral("rec-1521"), QStringLiteral("15:21"),
                QStringLiteral("Recording edge cases"),
                QStringLiteral("Mic off state + permissions."), 51.0,
                QStringLiteral("complete")),
        summary(QStringLiteral("rec-1409"), QStringLiteral("14:09"),
                QStringLiteral("Fast program install"),
                QStringLiteral("If fast option installed, menu bounces."), 62.0,
                QStringLiteral("failed")),
    };
    bridge.selectedRecordingId = QStringLiteral("rec-2018");
    bridge.selectedRecording = QVariantMap {
        {QStringLiteral("id"), QStringLiteral("rec-2018")},
        {QStringLiteral("note"), QStringLiteral("Refine compact viewer")},
        {QStringLiteral("started_at"), QStringLiteral("2026-08-20T20:18:00+02:00")},
        {QStringLiteral("duration_seconds"), 151.0},
        {QStringLiteral("transcription_status"), QStringLiteral("complete")},
        {QStringLiteral("success"), true},
        {QStringLiteral("git_branch"), QStringLiteral("main")},
        {QStringLiteral("recording_scope"), QStringLiteral("repository")},
        {QStringLiteral("annotation_count"), 2},
        {QStringLiteral("video_url"), QStringLiteral(
            "file:///home/jihmy/Videos/screenrecording-2026-08-20_12-26-32.mp4")},
        {QStringLiteral("preview_image_url"),
         QStringLiteral("file:///tmp/codex-clipboard-887d40c2-1628-4bc1-8aaf-5e52de5fecd6.png")},
        {QStringLiteral("transcript"), QStringLiteral(
            "Stockbar looks stupid. It's not the same style as our app and minimise and make something. It doesn't work on Arch. So I think in this view we wanted to render the video and delete it.")},
        {QStringLiteral("transcript_segments"), QVariantList {
            QVariantMap {{QStringLiteral("start_seconds"), 0.0},
                         {QStringLiteral("end_seconds"), 29.0},
                         {QStringLiteral("text"), QStringLiteral(
                             "Stockbar looks stupid. It's not the same style as our app and minimise and make something. It doesn't work on Arch…")}},
            QVariantMap {{QStringLiteral("start_seconds"), 30.0},
                         {QStringLiteral("end_seconds"), 46.0},
                         {QStringLiteral("text"), QStringLiteral(
                             "So I think in this view we wanted to render the video and delete it. Also, this is all like getting squashed…")}},
            QVariantMap {{QStringLiteral("start_seconds"), 47.0},
                         {QStringLiteral("end_seconds"), 71.0},
                         {QStringLiteral("text"), QStringLiteral(
                             "Switch to compact list view; reduce padding; align actions.")}},
            QVariantMap {{QStringLiteral("start_seconds"), 72.0},
                         {QStringLiteral("end_seconds"), 110.0},
                         {QStringLiteral("text"), QStringLiteral(
                             "Another thing we didn't get to fix is basically the option to turn off git scoped.")}},
        }},
        {QStringLiteral("timeline_notes"), QVariantList {
            QVariantMap {{QStringLiteral("timestamp_seconds"), 47.0},
                         {QStringLiteral("text"), QStringLiteral(
                             "Switch to compact list view; reduce padding; align actions.")}},
        }},
    };

    QQmlEngine engine;
    QQmlComponent component = dashboardComponent(engine);
    QScopedPointer<QObject> dashboard(createDashboard(component, bridge, theme));
    QVERIFY2(dashboard, qPrintable(component.errorString()));
    dashboard->setProperty("width", 1487);
    dashboard->setProperty("height", 1058);
    auto *window = qobject_cast<QQuickWindow *>(dashboard.get());
    QVERIFY(window != nullptr);
    window->show();
    QTRY_VERIFY(window->isVisible());
    QTest::qWait(350);

    const QFileInfo outputInfo(outputPath);
    QVERIFY(QDir().mkpath(outputInfo.absolutePath()));
    const QImage image = window->grabWindow();
    QCOMPARE(image.size(), QSize(1487, 1058));
    QVERIFY2(image.save(outputPath), qPrintable(outputPath));
}

QTEST_MAIN(DashboardQmlTest)

#include "dashboard_qml_test.moc"
