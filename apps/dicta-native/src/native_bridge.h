#pragma once

#include <QObject>
#include <QString>
#include <QTimer>
#include <QVariantList>
#include <QVariantMap>

class OverlayController;
struct DictaNativeOverlayCommand;

class NativeBridge final : public QObject
{
    Q_OBJECT
    Q_PROPERTY(QString apiVersion READ apiVersion CONSTANT)
    Q_PROPERTY(QString hostState READ hostState NOTIFY hostStateChanged)
    Q_PROPERTY(QString hostError READ hostError NOTIFY hostErrorChanged)
    Q_PROPERTY(QString socketPath READ socketPath NOTIFY socketPathChanged)
    Q_PROPERTY(qulonglong strokeCount READ strokeCount NOTIFY strokeCountChanged)
    Q_PROPERTY(QString runtimePhase READ runtimePhase NOTIFY dashboardChanged)
    Q_PROPERTY(QString activeRecordingId READ activeRecordingId NOTIFY dashboardChanged)
    Q_PROPERTY(bool annotationsEnabled READ annotationsEnabled NOTIFY dashboardChanged)
    Q_PROPERTY(QString annotationTool READ annotationTool NOTIFY dashboardChanged)
    Q_PROPERTY(QVariantList projects READ projects NOTIFY dashboardChanged)
    Q_PROPERTY(QVariantMap currentProject READ currentProject NOTIFY dashboardChanged)
    Q_PROPERTY(QVariantMap modelStatus READ modelStatus NOTIFY dashboardChanged)
    Q_PROPERTY(QVariantMap settings READ settings NOTIFY dashboardChanged)
    Q_PROPERTY(QString settingsMessage READ settingsMessage NOTIFY dashboardChanged)
    Q_PROPERTY(QVariantList recentRecordings READ recentRecordings NOTIFY dashboardChanged)
    Q_PROPERTY(QString uiError READ uiError NOTIFY dashboardChanged)
    Q_PROPERTY(QVariantMap selectedRecording READ selectedRecording NOTIFY selectedRecordingChanged)
    Q_PROPERTY(QString selectedRecordingId READ selectedRecordingId NOTIFY selectedRecordingChanged)
    Q_PROPERTY(bool multimediaAvailable READ multimediaAvailable CONSTANT)

public:
    explicit NativeBridge(OverlayController &overlay, QObject *parent = nullptr);
    ~NativeBridge() override;

    [[nodiscard]] QString apiVersion() const;
    Q_INVOKABLE [[nodiscard]] QString inspect(const QString &text) const;
    [[nodiscard]] QString hostState() const;
    [[nodiscard]] QString hostError() const;
    [[nodiscard]] QString socketPath() const;
    [[nodiscard]] qulonglong strokeCount() const;
    [[nodiscard]] QString runtimePhase() const;
    [[nodiscard]] QString activeRecordingId() const;
    [[nodiscard]] bool annotationsEnabled() const;
    [[nodiscard]] QString annotationTool() const;
    [[nodiscard]] QVariantList projects() const;
    [[nodiscard]] QVariantMap currentProject() const;
    [[nodiscard]] QVariantMap modelStatus() const;
    [[nodiscard]] QVariantMap settings() const;
    [[nodiscard]] QString settingsMessage() const;
    [[nodiscard]] QVariantList recentRecordings() const;
    [[nodiscard]] QString uiError() const;
    [[nodiscard]] QVariantMap selectedRecording() const;
    [[nodiscard]] QString selectedRecordingId() const;
    [[nodiscard]] bool multimediaAvailable() const;

    Q_INVOKABLE [[nodiscard]] bool startRecording(const QString &note);
    Q_INVOKABLE [[nodiscard]] bool stopRecording();
    Q_INVOKABLE [[nodiscard]] bool setAnnotationsEnabled(bool enabled);
    Q_INVOKABLE [[nodiscard]] bool chooseAnnotationTool(const QString &tool);
    Q_INVOKABLE [[nodiscard]] bool undoAnnotation();
    Q_INVOKABLE [[nodiscard]] bool clearAnnotations();
    Q_INVOKABLE [[nodiscard]] bool refreshDashboard();
    Q_INVOKABLE [[nodiscard]] bool selectProject(const QString &projectId);
    Q_INVOKABLE [[nodiscard]] bool createProject(const QString &name);
    Q_INVOKABLE [[nodiscard]] bool selectRecording(const QString &recordingId);
    Q_INVOKABLE void closeRecording();
    Q_INVOKABLE [[nodiscard]] bool deleteSelectedRecording();
    Q_INVOKABLE [[nodiscard]] bool transcribeSelectedRecording();
    Q_INVOKABLE [[nodiscard]] bool addTimelineNote(const QString &text, double timestampSeconds);
    Q_INVOKABLE [[nodiscard]] bool removeTimelineNote(const QString &noteId);
    Q_INVOKABLE [[nodiscard]] bool copySelectedContext();
    Q_INVOKABLE [[nodiscard]] bool revealSelectedRecording();
    Q_INVOKABLE [[nodiscard]] bool openSelectedRecording();
    Q_INVOKABLE [[nodiscard]] bool setShortcut(const QString &shortcutId);
    Q_INVOKABLE [[nodiscard]] bool setCleanupMergedVideos(bool enabled);
    Q_INVOKABLE [[nodiscard]] bool setBranchLocking(bool enabled);
    Q_INVOKABLE [[nodiscard]] bool setTranscriptionLanguage(const QString &language);
    Q_INVOKABLE [[nodiscard]] bool setGeneralPath(const QString &path);
    Q_INVOKABLE [[nodiscard]] bool cleanupMergedVideos();
    Q_INVOKABLE [[nodiscard]] bool installQualityModel();

    [[nodiscard]] bool startHost(
        const QString &socketPath,
        const QString &storageRoot,
        const QString &outputName,
        bool e2e
    );
    void stopHost();

signals:
    void hostStateChanged();
    void hostErrorChanged();
    void socketPathChanged();
    void strokeCountChanged();
    void dashboardChanged();
    void selectedRecordingChanged();
    void uiShowRequested();
    void hostFailed();

private:
    static void overlayCallback(void *context, const DictaNativeOverlayCommand *command);
    void dispatchOverlayCommand(quint32 kind, quint32 tool, const QString &outputName);
    void submitStroke(
        const QVariantList &normalizedPoints,
        int tool,
        double startedAtSeconds,
        double endedAtSeconds
    );
    void refreshHostDiagnostics();
    [[nodiscard]] bool sendAnnotationCommand(quint32 action, quint32 tool = 0);
    [[nodiscard]] bool updateSetting(quint32 key, const QString &value);
    [[nodiscard]] bool saveTimelineNotes(const QVariantList &notes);

    OverlayController &m_overlay;
    QTimer m_statusTimer;
    QString m_hostState = QStringLiteral("stopped");
    QString m_hostError;
    QString m_socketPath;
    qulonglong m_strokeCount = 0;
    QString m_runtimePhase = QStringLiteral("unavailable");
    QString m_activeRecordingId;
    QString m_annotationTool;
    QVariantList m_projects;
    QVariantMap m_currentProject;
    QVariantMap m_modelStatus;
    QVariantMap m_settings;
    QString m_settingsMessage;
    QVariantList m_recentRecordings;
    QString m_uiError;
    QVariantMap m_selectedRecording;
    int m_dashboardRefreshCountdown = 0;
    bool m_annotationsEnabled = false;
    bool m_started = false;
    bool m_e2e = false;
    bool m_failedEmitted = false;
};
