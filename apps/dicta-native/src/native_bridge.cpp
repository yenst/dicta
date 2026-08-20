#include "native_bridge.h"
#include "overlay_controller.h"

#include <QByteArray>
#include <QClipboard>
#include <QDesktopServices>
#include <QDateTime>
#include <QFileInfo>
#include <QGuiApplication>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QMetaObject>
#include <QPointF>
#include <QVariant>
#include <QUrl>
#include <QUuid>

#include <cmath>
#include <cstddef>
#include <cstdint>
#include <vector>

extern "C" {
struct DictaNativeHostConfig {
    const unsigned char *socketPath;
    std::size_t socketPathLength;
    const unsigned char *storageRoot;
    std::size_t storageRootLength;
    const unsigned char *outputName;
    std::size_t outputNameLength;
    std::uint32_t flags;
};

struct DictaNativeOverlayCommand {
    std::uint32_t kind;
    std::uint32_t tool;
    const unsigned char *outputName;
    std::size_t outputNameLength;
};

using DictaNativeOverlayCallback = void (*)(void *, const DictaNativeOverlayCommand *);

const char *dicta_native_api_version();
std::size_t dicta_native_utf8_scalar_count(const unsigned char *data, std::size_t len);
int dicta_native_host_start(
    const DictaNativeHostConfig *config,
    DictaNativeOverlayCallback callback,
    void *callbackContext
);
void dicta_native_host_request_stop();
int dicta_native_host_join();
std::uint32_t dicta_native_host_state();
std::uint64_t dicta_native_host_stroke_count();
std::size_t dicta_native_host_last_error(unsigned char *output, std::size_t capacity);
int dicta_native_host_overlay_stroke(
    std::uint32_t tool,
    double startedAtSeconds,
    double endedAtSeconds,
    const double *xy,
    std::size_t pointCount
);
int dicta_native_host_record_start(const unsigned char *note, std::size_t noteLength);
int dicta_native_host_record_stop();
int dicta_native_host_annotation_command(std::uint32_t action, std::uint32_t tool);
int dicta_native_host_settings_set(
    std::uint32_t key,
    const unsigned char *value,
    std::size_t valueLength
);
std::size_t dicta_native_host_cleanup_merged(
    const unsigned char *projectId,
    std::size_t projectIdLength,
    unsigned char *output,
    std::size_t capacity
);
int dicta_native_host_model_install_quality();
std::size_t dicta_native_host_ui_snapshot(unsigned char *output, std::size_t capacity);
std::size_t dicta_native_host_recording_detail(
    const unsigned char *recordingId,
    std::size_t recordingIdLength,
    unsigned char *output,
    std::size_t capacity
);
int dicta_native_host_recording_delete(
    const unsigned char *recordingId,
    std::size_t recordingIdLength
);
int dicta_native_host_recording_transcribe(
    const unsigned char *recordingId,
    std::size_t recordingIdLength
);
int dicta_native_host_timeline_notes_set(
    const unsigned char *recordingId,
    std::size_t recordingIdLength,
    const unsigned char *notesJson,
    std::size_t notesJsonLength
);
int dicta_native_host_project_select(
    const unsigned char *projectId,
    std::size_t projectIdLength
);
int dicta_native_host_project_create(
    const unsigned char *name,
    std::size_t nameLength
);
std::size_t dicta_native_host_recording_context(
    const unsigned char *recordingId,
    std::size_t recordingIdLength,
    const unsigned char *projectId,
    std::size_t projectIdLength,
    unsigned char *output,
    std::size_t capacity
);
}

namespace {
constexpr std::uint32_t HostFlagE2e = 1;
constexpr qsizetype UiSnapshotCapacity = 64 * 1024;
constexpr qsizetype RecordingDetailCapacity = 1024 * 1024;
constexpr qsizetype RecordingContextCapacity = 4 * 1024 * 1024;
constexpr qsizetype CleanupSummaryCapacity = 64 * 1024;

QString hostStateName(const std::uint32_t state)
{
    switch (state) {
    case 1:
        return QStringLiteral("starting");
    case 2:
        return QStringLiteral("running");
    case 3:
        return QStringLiteral("stopping");
    case 4:
        return QStringLiteral("failed");
    default:
        return QStringLiteral("stopped");
    }
}

QString lastHostError()
{
    const std::size_t length = dicta_native_host_last_error(nullptr, 0);
    if (length == 0) {
        return {};
    }
    QByteArray bytes(qsizetype(length + 1), '\0');
    dicta_native_host_last_error(
        reinterpret_cast<unsigned char *>(bytes.data()),
        std::size_t(bytes.size())
    );
    bytes.truncate(qsizetype(length));
    return QString::fromUtf8(bytes);
}
}

NativeBridge::NativeBridge(OverlayController &overlay, QObject *parent)
    : QObject(parent)
    , m_overlay(overlay)
{
    m_statusTimer.setInterval(250);
    connect(&m_statusTimer, &QTimer::timeout, this, &NativeBridge::refreshHostDiagnostics);
    connect(
        &m_overlay,
        &OverlayController::strokeCommitted,
        this,
        &NativeBridge::submitStroke
    );
}

NativeBridge::~NativeBridge()
{
    stopHost();
}

QString NativeBridge::apiVersion() const
{
    return QString::fromUtf8(dicta_native_api_version());
}

QString NativeBridge::inspect(const QString &text) const
{
    const QByteArray utf8 = text.toUtf8();
    const auto *data = reinterpret_cast<const unsigned char *>(utf8.constData());
    const std::size_t count = dicta_native_utf8_scalar_count(
        data,
        static_cast<std::size_t>(utf8.size())
    );

    return tr("Rust received %1 Unicode scalar(s).").arg(count);
}

QString NativeBridge::hostState() const
{
    return m_hostState;
}

QString NativeBridge::hostError() const
{
    return m_hostError;
}

QString NativeBridge::socketPath() const
{
    return m_socketPath;
}

qulonglong NativeBridge::strokeCount() const
{
    return m_strokeCount;
}

QString NativeBridge::runtimePhase() const
{
    return m_runtimePhase;
}

QString NativeBridge::activeRecordingId() const
{
    return m_activeRecordingId;
}

bool NativeBridge::annotationsEnabled() const
{
    return m_annotationsEnabled;
}

QString NativeBridge::annotationTool() const
{
    return m_annotationTool;
}

QVariantList NativeBridge::projects() const
{
    return m_projects;
}

QVariantMap NativeBridge::currentProject() const
{
    return m_currentProject;
}

QVariantMap NativeBridge::modelStatus() const
{
    return m_modelStatus;
}

QVariantMap NativeBridge::settings() const
{
    return m_settings;
}

QString NativeBridge::settingsMessage() const
{
    return m_settingsMessage;
}

QVariantList NativeBridge::recentRecordings() const
{
    return m_recentRecordings;
}

QString NativeBridge::uiError() const
{
    return m_uiError;
}

QVariantMap NativeBridge::selectedRecording() const
{
    return m_selectedRecording;
}

QString NativeBridge::selectedRecordingId() const
{
    return m_selectedRecording.value(QStringLiteral("id")).toString();
}

bool NativeBridge::multimediaAvailable() const
{
#ifdef DICTA_HAS_MULTIMEDIA
    return true;
#else
    return false;
#endif
}

bool NativeBridge::startRecording(const QString &note)
{
    const QByteArray utf8 = note.trimmed().toUtf8();
    const auto *data = reinterpret_cast<const unsigned char *>(utf8.constData());
    if (dicta_native_host_record_start(data, std::size_t(utf8.size())) != 0) {
        const QString error = lastHostError();
        if (m_uiError != error) {
            m_uiError = error;
            emit dashboardChanged();
        }
        return false;
    }
    return refreshDashboard();
}

bool NativeBridge::stopRecording()
{
    if (dicta_native_host_record_stop() != 0) {
        const QString error = lastHostError();
        if (m_uiError != error) {
            m_uiError = error;
            emit dashboardChanged();
        }
        return false;
    }
    return refreshDashboard();
}

bool NativeBridge::setAnnotationsEnabled(const bool enabled)
{
    return sendAnnotationCommand(enabled ? 1U : 2U);
}

bool NativeBridge::chooseAnnotationTool(const QString &tool)
{
    const QString normalized = tool.trimmed().toLower();
    quint32 value = 0;
    if (normalized == QStringLiteral("pen")) {
        value = 0;
    } else if (normalized == QStringLiteral("arrow")) {
        value = 1;
    } else if (normalized == QStringLiteral("rectangle")) {
        value = 2;
    } else if (normalized == QStringLiteral("spotlight")) {
        value = 3;
    } else {
        m_uiError = tr("Unknown annotation tool: %1").arg(tool);
        emit dashboardChanged();
        return false;
    }
    return sendAnnotationCommand(3U, value);
}

bool NativeBridge::undoAnnotation()
{
    return sendAnnotationCommand(4U);
}

bool NativeBridge::clearAnnotations()
{
    return sendAnnotationCommand(5U);
}

bool NativeBridge::sendAnnotationCommand(const quint32 action, const quint32 tool)
{
    if (dicta_native_host_annotation_command(action, tool) != 0) {
        const QString error = lastHostError();
        if (m_uiError != error) {
            m_uiError = error;
            emit dashboardChanged();
        }
        return false;
    }
    return refreshDashboard();
}

bool NativeBridge::refreshDashboard()
{
    QByteArray bytes(UiSnapshotCapacity, '\0');
    const std::size_t length = dicta_native_host_ui_snapshot(
        reinterpret_cast<unsigned char *>(bytes.data()),
        std::size_t(bytes.size())
    );
    if (length == 0 || length > std::size_t(bytes.size())) {
        const QString error = lastHostError();
        if (m_uiError != error) {
            m_uiError = error;
            emit dashboardChanged();
        }
        return false;
    }
    bytes.truncate(qsizetype(length));
    QJsonParseError parseError;
    const QJsonDocument document = QJsonDocument::fromJson(bytes, &parseError);
    if (parseError.error != QJsonParseError::NoError || !document.isObject()) {
        const QString error = tr("The native dashboard returned invalid JSON: %1")
                                  .arg(parseError.errorString());
        if (m_uiError != error) {
            m_uiError = error;
            emit dashboardChanged();
        }
        return false;
    }

    const QJsonObject root = document.object();
    const QJsonObject status = root.value(QStringLiteral("status")).toObject();
    const QString phase = status.value(QStringLiteral("phase")).toString();
    if (root.value(QStringLiteral("version")).toInt() != 1
        || phase.isEmpty()
        || !root.value(QStringLiteral("projects")).isArray()
        || !root.value(QStringLiteral("recordings")).isArray()
        || !root.value(QStringLiteral("settings")).isObject()) {
        const QString error = tr("The native dashboard snapshot is incomplete.");
        if (m_uiError != error) {
            m_uiError = error;
            emit dashboardChanged();
        }
        return false;
    }
    QVariantList recordings;
    const QJsonArray recordingArray = root.value(QStringLiteral("recordings")).toArray();
    recordings.reserve(recordingArray.size());
    for (const QJsonValue &recording : recordingArray) {
        if (recording.isObject()) {
            recordings.append(recording.toObject().toVariantMap());
        }
    }
    QVariantList projects;
    QVariantMap currentProject;
    const QJsonArray projectArray = root.value(QStringLiteral("projects")).toArray();
    projects.reserve(projectArray.size());
    for (const QJsonValue &project : projectArray) {
        if (!project.isObject()) {
            continue;
        }
        const QVariantMap value = project.toObject().toVariantMap();
        projects.append(value);
        if (value.value(QStringLiteral("selected")).toBool()) {
            currentProject = value;
        }
    }

    m_runtimePhase = phase;
    m_activeRecordingId = status.value(QStringLiteral("recording_id")).toString();
    m_annotationsEnabled = status.value(QStringLiteral("annotations_enabled")).toBool();
    m_annotationTool = status.value(QStringLiteral("annotation_tool")).toString();
    m_projects = projects;
    m_currentProject = currentProject;
    m_modelStatus = root.value(QStringLiteral("model")).toObject().toVariantMap();
    m_settings = root.value(QStringLiteral("settings")).toObject().toVariantMap();
    if (m_modelStatus.isEmpty()) {
        const QString modelError = root.value(QStringLiteral("model_error")).toString();
        if (!modelError.isEmpty()) {
            m_modelStatus.insert(QStringLiteral("message"), modelError);
            m_modelStatus.insert(QStringLiteral("quality_state"), QStringLiteral("unavailable"));
        }
    }
    m_recentRecordings = recordings;
    m_uiError.clear();
    emit dashboardChanged();
    return true;
}

bool NativeBridge::updateSetting(const quint32 key, const QString &value)
{
    const QByteArray bytes = value.toUtf8();
    if (dicta_native_host_settings_set(
            key,
            reinterpret_cast<const unsigned char *>(bytes.constData()),
            std::size_t(bytes.size())
        ) != 0) {
        const QString error = lastHostError();
        if (m_uiError != error) {
            m_uiError = error;
            emit dashboardChanged();
        }
        return false;
    }
    return refreshDashboard();
}

bool NativeBridge::setShortcut(const QString &shortcutId)
{
    return updateSetting(1, shortcutId.trimmed());
}

bool NativeBridge::setCleanupMergedVideos(const bool enabled)
{
    return updateSetting(2, enabled ? QStringLiteral("true") : QStringLiteral("false"));
}

bool NativeBridge::setBranchLocking(const bool enabled)
{
    return updateSetting(3, enabled ? QStringLiteral("true") : QStringLiteral("false"));
}

bool NativeBridge::setTranscriptionLanguage(const QString &language)
{
    return updateSetting(4, language.trimmed());
}

bool NativeBridge::setGeneralPath(const QString &path)
{
    return updateSetting(5, path.trimmed());
}

bool NativeBridge::cleanupMergedVideos()
{
    const QByteArray project = m_currentProject.value(QStringLiteral("id")).toString().toUtf8();
    if (project.isEmpty()) {
        m_uiError = tr("Select a linked project before cleaning merged videos.");
        emit dashboardChanged();
        return false;
    }
    QByteArray bytes(CleanupSummaryCapacity, '\0');
    const std::size_t length = dicta_native_host_cleanup_merged(
        reinterpret_cast<const unsigned char *>(project.constData()),
        std::size_t(project.size()),
        reinterpret_cast<unsigned char *>(bytes.data()),
        std::size_t(bytes.size())
    );
    if (length == 0 || length > std::size_t(bytes.size())) {
        m_uiError = lastHostError();
        emit dashboardChanged();
        return false;
    }
    bytes.truncate(qsizetype(length));
    QJsonParseError parseError;
    const QJsonDocument document = QJsonDocument::fromJson(bytes, &parseError);
    if (parseError.error != QJsonParseError::NoError || !document.isObject()) {
        m_uiError = tr("Merged-video cleanup returned invalid JSON.");
        emit dashboardChanged();
        return false;
    }
    m_settingsMessage = document.object().value(QStringLiteral("message")).toString();
    m_uiError.clear();
    emit dashboardChanged();
    return true;
}

bool NativeBridge::installQualityModel()
{
    if (dicta_native_host_model_install_quality() != 0) {
        m_uiError = lastHostError();
        emit dashboardChanged();
        return false;
    }
    m_uiError.clear();
    return refreshDashboard();
}

bool NativeBridge::selectProject(const QString &projectId)
{
    const QByteArray id = projectId.trimmed().toUtf8();
    if (id.isEmpty()) {
        return false;
    }
    if (dicta_native_host_project_select(
            reinterpret_cast<const unsigned char *>(id.constData()),
            std::size_t(id.size())
        ) != 0) {
        const QString error = lastHostError();
        if (m_uiError != error) {
            m_uiError = error;
            emit dashboardChanged();
        }
        return false;
    }
    closeRecording();
    return refreshDashboard();
}

bool NativeBridge::createProject(const QString &name)
{
    const QByteArray value = name.trimmed().toUtf8();
    if (value.isEmpty()) {
        return false;
    }
    if (dicta_native_host_project_create(
            reinterpret_cast<const unsigned char *>(value.constData()),
            std::size_t(value.size())
        ) != 0) {
        const QString error = lastHostError();
        if (m_uiError != error) {
            m_uiError = error;
            emit dashboardChanged();
        }
        return false;
    }
    closeRecording();
    return refreshDashboard();
}

bool NativeBridge::selectRecording(const QString &recordingId)
{
    const QByteArray id = recordingId.trimmed().toUtf8();
    if (id.isEmpty()) {
        return false;
    }
    QByteArray bytes(RecordingDetailCapacity, '\0');
    const std::size_t length = dicta_native_host_recording_detail(
        reinterpret_cast<const unsigned char *>(id.constData()),
        std::size_t(id.size()),
        reinterpret_cast<unsigned char *>(bytes.data()),
        std::size_t(bytes.size())
    );
    if (length == 0 || length > std::size_t(bytes.size())) {
        const QString error = lastHostError();
        if (m_uiError != error) {
            m_uiError = error;
            emit dashboardChanged();
        }
        return false;
    }
    bytes.truncate(qsizetype(length));
    QJsonParseError parseError;
    const QJsonDocument document = QJsonDocument::fromJson(bytes, &parseError);
    const QJsonObject payload = document.object();
    const QJsonObject recording = payload.value(QStringLiteral("recording")).toObject();
    if (parseError.error != QJsonParseError::NoError
        || !document.isObject()
        || payload.value(QStringLiteral("version")).toInt() != 1
        || recording.value(QStringLiteral("id")).toString() != recordingId) {
        const QString error = tr("The native recording detail is invalid.");
        if (m_uiError != error) {
            m_uiError = error;
            emit dashboardChanged();
        }
        return false;
    }
    m_selectedRecording = recording.toVariantMap();
    const QString videoPath = recording.value(QStringLiteral("video_path")).toString();
    m_selectedRecording.insert(
        QStringLiteral("video_url"),
        QUrl::fromLocalFile(videoPath).toString()
    );
    const QString posterPath = recording.value(QStringLiteral("poster_path")).toString();
    m_selectedRecording.insert(
        QStringLiteral("preview_image_url"),
        posterPath.isEmpty() ? QString() : QUrl::fromLocalFile(posterPath).toString()
    );
    m_uiError.clear();
    emit dashboardChanged();
    emit selectedRecordingChanged();
    return true;
}

void NativeBridge::closeRecording()
{
    if (m_selectedRecording.isEmpty()) {
        return;
    }
    m_selectedRecording.clear();
    emit selectedRecordingChanged();
}

bool NativeBridge::deleteSelectedRecording()
{
    const QByteArray id = selectedRecordingId().toUtf8();
    if (id.isEmpty()) {
        return false;
    }
    const int result = dicta_native_host_recording_delete(
        reinterpret_cast<const unsigned char *>(id.constData()),
        std::size_t(id.size())
    );
    if (result != 0) {
        const QString error = lastHostError();
        if (m_uiError != error) {
            m_uiError = error;
            emit dashboardChanged();
        }
        return false;
    }
    closeRecording();
    return refreshDashboard();
}

bool NativeBridge::transcribeSelectedRecording()
{
    const QByteArray id = selectedRecordingId().toUtf8();
    if (id.isEmpty()) {
        return false;
    }
    const int result = dicta_native_host_recording_transcribe(
        reinterpret_cast<const unsigned char *>(id.constData()),
        std::size_t(id.size())
    );
    if (result != 0) {
        const QString error = lastHostError();
        if (m_uiError != error) {
            m_uiError = error;
            emit dashboardChanged();
        }
        return false;
    }
    m_uiError.clear();
    emit dashboardChanged();
    return refreshDashboard();
}

bool NativeBridge::addTimelineNote(const QString &text, const double timestampSeconds)
{
    const QString value = text.trimmed();
    if (selectedRecordingId().isEmpty() || value.isEmpty() || value.size() > 2000
        || !std::isfinite(timestampSeconds) || timestampSeconds < 0.0) {
        return false;
    }
    QVariantList notes = m_selectedRecording
        .value(QStringLiteral("timeline_notes"))
        .toList();
    QVariantMap note;
    note.insert(
        QStringLiteral("id"),
        QUuid::createUuid().toString(QUuid::WithoutBraces)
    );
    note.insert(QStringLiteral("timestamp_seconds"), timestampSeconds);
    note.insert(QStringLiteral("text"), value);
    note.insert(
        QStringLiteral("created_at"),
        QDateTime::currentDateTimeUtc().toString(Qt::ISODateWithMs)
    );
    note.insert(QStringLiteral("source"), QStringLiteral("typed"));
    notes.append(note);
    return saveTimelineNotes(notes);
}

bool NativeBridge::removeTimelineNote(const QString &noteId)
{
    const QString id = noteId.trimmed();
    if (selectedRecordingId().isEmpty() || id.isEmpty()) {
        return false;
    }
    const QVariantList current = m_selectedRecording
        .value(QStringLiteral("timeline_notes"))
        .toList();
    QVariantList notes;
    notes.reserve(current.size());
    for (const QVariant &entry : current) {
        if (entry.toMap().value(QStringLiteral("id")).toString() != id) {
            notes.append(entry);
        }
    }
    if (notes.size() == current.size()) {
        return false;
    }
    return saveTimelineNotes(notes);
}

bool NativeBridge::saveTimelineNotes(const QVariantList &notes)
{
    const QByteArray id = selectedRecordingId().toUtf8();
    if (id.isEmpty()) {
        return false;
    }
    const QByteArray json = QJsonDocument(QJsonArray::fromVariantList(notes))
        .toJson(QJsonDocument::Compact);
    const int result = dicta_native_host_timeline_notes_set(
        reinterpret_cast<const unsigned char *>(id.constData()),
        std::size_t(id.size()),
        reinterpret_cast<const unsigned char *>(json.constData()),
        std::size_t(json.size())
    );
    if (result != 0) {
        const QString error = lastHostError();
        if (m_uiError != error) {
            m_uiError = error;
            emit dashboardChanged();
        }
        return false;
    }
    return selectRecording(QString::fromUtf8(id));
}

bool NativeBridge::copySelectedContext()
{
    const QByteArray recordingId = selectedRecordingId().toUtf8();
    const QByteArray projectId = m_selectedRecording
        .value(QStringLiteral("project_id"))
        .toString()
        .toUtf8();
    if (recordingId.isEmpty()) {
        return false;
    }
    QByteArray bytes(RecordingContextCapacity, '\0');
    const std::size_t length = dicta_native_host_recording_context(
        reinterpret_cast<const unsigned char *>(recordingId.constData()),
        std::size_t(recordingId.size()),
        reinterpret_cast<const unsigned char *>(projectId.constData()),
        std::size_t(projectId.size()),
        reinterpret_cast<unsigned char *>(bytes.data()),
        std::size_t(bytes.size())
    );
    if (length == 0 || length > std::size_t(bytes.size())) {
        const QString error = lastHostError();
        if (m_uiError != error) {
            m_uiError = error;
            emit dashboardChanged();
        }
        return false;
    }
    bytes.truncate(qsizetype(length));
    QGuiApplication::clipboard()->setText(QString::fromUtf8(bytes));
    m_uiError.clear();
    emit dashboardChanged();
    return true;
}

bool NativeBridge::revealSelectedRecording()
{
    const QString videoPath = m_selectedRecording.value(QStringLiteral("video_path")).toString();
    const QString metadataPath = m_selectedRecording
        .value(QStringLiteral("metadata_path"))
        .toString();
    const QFileInfo artifact(videoPath.isEmpty() ? metadataPath : videoPath);
    if (artifact.filePath().isEmpty()) {
        return false;
    }
    if (QDesktopServices::openUrl(QUrl::fromLocalFile(artifact.absolutePath()))) {
        return true;
    }
    m_uiError = tr("Could not reveal the recording folder.");
    emit dashboardChanged();
    return false;
}

bool NativeBridge::openSelectedRecording()
{
    const QString videoPath = m_selectedRecording.value(QStringLiteral("video_path")).toString();
    if (videoPath.isEmpty()) {
        return false;
    }
    if (QDesktopServices::openUrl(QUrl::fromLocalFile(videoPath))) {
        return true;
    }
    m_uiError = tr("Could not open the recording.");
    emit dashboardChanged();
    return false;
}

bool NativeBridge::startHost(
    const QString &socketPath,
    const QString &storageRoot,
    const QString &outputName,
    const bool e2e
)
{
    if (m_started) {
        return true;
    }
    m_e2e = e2e;
    m_failedEmitted = false;
    const QByteArray socket = socketPath.toUtf8();
    const QByteArray storage = storageRoot.toUtf8();
    const QByteArray output = outputName.toUtf8();
    const DictaNativeHostConfig config {
        reinterpret_cast<const unsigned char *>(socket.constData()),
        std::size_t(socket.size()),
        reinterpret_cast<const unsigned char *>(storage.constData()),
        std::size_t(storage.size()),
        reinterpret_cast<const unsigned char *>(output.constData()),
        std::size_t(output.size()),
        e2e ? HostFlagE2e : 0,
    };
    const int result = dicta_native_host_start(
        &config,
        &NativeBridge::overlayCallback,
        this
    );
    if (result != 0) {
        refreshHostDiagnostics();
        return false;
    }
    m_started = true;
    if (m_socketPath != socketPath) {
        m_socketPath = socketPath;
        emit socketPathChanged();
    }
    m_statusTimer.start();
    refreshHostDiagnostics();
    return true;
}

void NativeBridge::stopHost()
{
    if (!m_started) {
        return;
    }
    m_statusTimer.stop();
    dicta_native_host_request_stop();
    const int result = dicta_native_host_join();
    m_started = false;
    refreshHostDiagnostics();
    if (result != 0 && m_hostError.isEmpty()) {
        m_hostError = tr("The native service thread could not be joined.");
        emit hostErrorChanged();
    }
}

void NativeBridge::overlayCallback(
    void *context,
    const DictaNativeOverlayCommand *command
)
{
    if (context == nullptr || command == nullptr) {
        return;
    }
    auto *bridge = static_cast<NativeBridge *>(context);
    const quint32 kind = command->kind;
    const quint32 tool = command->tool;
    const QString outputName = command->outputNameLength == 0
        ? QString()
        : QString::fromUtf8(
            reinterpret_cast<const char *>(command->outputName),
            qsizetype(command->outputNameLength)
        );
    QMetaObject::invokeMethod(
        bridge,
        [bridge, kind, tool, outputName] {
            bridge->dispatchOverlayCommand(kind, tool, outputName);
        },
        Qt::QueuedConnection
    );
}

void NativeBridge::dispatchOverlayCommand(
    const quint32 kind,
    const quint32 tool,
    const QString &outputName
)
{
    switch (kind) {
    case 1:
        if (!m_overlay.showOnOutput(m_e2e ? QString() : outputName)) {
            return;
        }
        break;
    case 2:
        if (!m_overlay.startRecordingClock()) {
            return;
        }
        break;
    case 3:
        m_overlay.setAnnotationMode(tool != 0);
        break;
    case 4:
        m_overlay.setTool(static_cast<OverlayController::Tool>(tool));
        break;
    case 5:
        (void)m_overlay.undo();
        break;
    case 6:
        m_overlay.clear();
        break;
    case 7:
        m_overlay.finishAndHide();
        break;
    case 8:
        emit uiShowRequested();
        break;
    case 9:
        if (selectRecording(outputName)) {
            emit uiShowRequested();
        }
        break;
    default:
        break;
    }
}

void NativeBridge::submitStroke(
    const QVariantList &normalizedPoints,
    const int tool,
    const double startedAtSeconds,
    const double endedAtSeconds
)
{
    std::vector<double> points;
    points.reserve(std::size_t(normalizedPoints.size()) * 2);
    for (const QVariant &value : normalizedPoints) {
        const QPointF point = value.toPointF();
        points.push_back(point.x());
        points.push_back(point.y());
    }
    if (points.size() < 4) {
        return;
    }
    dicta_native_host_overlay_stroke(
        std::uint32_t(tool),
        startedAtSeconds,
        endedAtSeconds,
        points.data(),
        points.size() / 2
    );
    refreshHostDiagnostics();
}

void NativeBridge::refreshHostDiagnostics()
{
    const QString state = hostStateName(dicta_native_host_state());
    const bool becameRunning = m_hostState != state && state == QStringLiteral("running");
    if (m_hostState != state) {
        m_hostState = state;
        emit hostStateChanged();
    }
    const QString error = lastHostError();
    if (m_hostError != error) {
        m_hostError = error;
        emit hostErrorChanged();
    }
    const qulonglong strokes = dicta_native_host_stroke_count();
    if (m_strokeCount != strokes) {
        m_strokeCount = strokes;
        emit strokeCountChanged();
    }
    if (state == QStringLiteral("failed") && !m_failedEmitted) {
        m_failedEmitted = true;
        emit hostFailed();
    }
    if (state == QStringLiteral("running")) {
        if (becameRunning || m_dashboardRefreshCountdown <= 0) {
            (void)refreshDashboard();
            m_dashboardRefreshCountdown = 4;
        } else {
            --m_dashboardRefreshCountdown;
        }
    }
}
