import QtQuick
import Quickshell
import Quickshell.Io

Item {
  id: root
  visible: false

  property var settings: ({})
  property var projects: []
  property var contexts: []
  property string selectedProjectId: ""
  property string error: ""
  property string actionStatus: ""
  property bool loading: false
  property bool refreshQueued: false
  property string refreshPhase: ""
  property string requestedProjectId: ""
  property string stateOutput: ""
  property string stateError: ""
  property string copyError: ""
  property string projectError: ""
  property string recordError: ""
  property string statusOutput: ""
  property string pendingProjectId: ""
  property string pendingContextId: ""
  property string copiedContextId: ""
  property bool dictaRunning: false
  property bool recordingActive: false

  readonly property string dictaCommand: String(setting("dictaCommand", "dicta") || "dicta")
  readonly property int refreshInterval: Math.max(5, Number(setting("refreshIntervalSec", 30))) * 1000

  function setting(name, fallback) {
    var value = settings ? settings[name] : undefined
    return value === undefined || value === null ? fallback : value
  }

  function checkDictaRunning() {
    if (dictaStatusProcess.running) return
    statusOutput = ""
    dictaStatusProcess.command = [dictaCommand, "--no-start", "--json", "status"]
    dictaStatusProcess.running = true
  }

  function finishDictaStatus(exitCode) {
    var wasRunning = dictaRunning
    var wasRecording = recordingActive
    dictaRunning = Number(exitCode) === 0
    recordingActive = false
    if (dictaRunning) {
      try {
        var payload = JSON.parse(statusOutput || "{}")
        var data = payload && payload.data ? payload.data : ({})
        var phase = String(data.phase || "")
        recordingActive = phase === "recording" || phase === "annotating"
      } catch (parseError) {
        dictaRunning = false
      }
    }
    if (dictaRunning && (!wasRunning || wasRecording !== recordingActive))
      Qt.callLater(function() { root.refresh(root.selectedProjectId) })
    if (!dictaRunning) {
      loading = false
      projects = []
      contexts = []
    }
  }

  function refresh(projectId) {
    if (projectId !== undefined && String(projectId || "") !== "")
      requestedProjectId = String(projectId)
    if (!dictaRunning) {
      loading = false
      return
    }
    if (stateProcess.running) {
      refreshQueued = true
      return
    }
    stateOutput = ""
    stateError = ""
    error = ""
    loading = true
    refreshPhase = "projects"
    stateProcess.command = [dictaCommand, "--no-start", "--json", "project", "list"]
    stateProcess.running = true
  }

  function finishRefresh(exitCode) {
    if (Number(exitCode) !== 0) {
      loading = false
      error = stateError.trim() || "Dicta context is unavailable."
    } else {
      try {
        var payload = JSON.parse(stateOutput || "{}")
        var data = payload && payload.data instanceof Array ? payload.data : []
        if (refreshPhase === "projects") {
          projects = data
          var selected = requestedProjectId
          if (selected === "") {
            for (var i = 0; i < projects.length; i++) {
              if (projects[i] && projects[i].selected) {
                selected = String(projects[i].id || "")
                break
              }
            }
          }
          selectedProjectId = selected
          requestedProjectId = ""
          if (selectedProjectId === "") {
            contexts = []
            loading = false
          } else {
            stateOutput = ""
            stateError = ""
            refreshPhase = "recordings"
            stateProcess.command = [
              dictaCommand, "--no-start", "--json", "recording", "list",
              "--project", selectedProjectId, "--limit", "3"
            ]
            stateProcess.running = true
            return
          }
        } else {
          contexts = []
          for (var j = 0; j < data.length; j++) {
            var recording = data[j] || {}
            contexts.push({
              id: String(recording.id || ""),
              projectId: String(recording.project || selectedProjectId),
              branch: String(recording.branch || ""),
              title: String(recording.note || recording.transcript_preview || recording.id || "Untitled recording"),
              startedAt: String(recording.started_at || "")
            })
          }
          loading = false
        }
      } catch (parseError) {
        loading = false
        error = "Dicta returned invalid panel data."
      }
    }
    if (refreshQueued) {
      refreshQueued = false
      Qt.callLater(function() { root.refresh(root.selectedProjectId) })
    }
  }

  function openProject(projectId) {
    if (projectProcess.running) return
    var id = String(projectId || "")
    if (id === "") return
    pendingProjectId = id
    projectError = ""
    actionStatus = "SELECTING PROJECT"
    projectProcess.command = [dictaCommand, "--no-start", "project", "select", id]
    projectProcess.running = true
  }

  function finishProject(exitCode) {
    if (Number(exitCode) !== 0) {
      actionStatus = ""
      error = projectError.trim() || "Could not select the Dicta project."
      pendingProjectId = ""
      return
    }
    selectedProjectId = pendingProjectId
    pendingProjectId = ""
    actionStatus = "PROJECT OPENED"
    actionClear.restart()
    refresh(selectedProjectId)
    Quickshell.execDetached([dictaCommand, "ui"])
  }

  function openRecording(projectId, recordingId) {
    var project = String(projectId || "")
    var recording = String(recordingId || "")
    if (project === "" || recording === "") return
    actionStatus = "OPENING RECORDING"
    actionClear.restart()
    Quickshell.execDetached([dictaCommand, "recording", "open", recording])
  }

  function openDicta() {
    actionStatus = "OPENING DICTA"
    actionClear.restart()
    Quickshell.execDetached([dictaCommand, "ui"])
  }

  function toggleRecording() {
    if (recordProcess.running) return
    recordError = ""
    actionStatus = "RECORDING REQUESTED"
    recordProcess.command = [dictaCommand, "record", "toggle"]
    recordProcess.running = true
  }

  function finishRecord(exitCode) {
    if (Number(exitCode) !== 0) {
      actionStatus = ""
      error = recordError.trim() || "Could not toggle Dicta recording."
      return
    }
    actionStatus = "RECORDING UPDATED"
    actionClear.restart()
    refresh(selectedProjectId)
  }

  function copyContext(projectId, recordingId) {
    if (copyProcess.running) return
    var project = String(projectId || "")
    var recording = String(recordingId || "")
    if (project === "" || recording === "") return
    copyError = ""
    pendingContextId = recording
    copiedContextId = ""
    actionStatus = "COPYING CONTEXT"
    copyProcess.command = [
      dictaCommand, "--no-start", "context", recording,
      "--project", project, "--copy"
    ]
    copyProcess.running = true
  }

  function finishCopy(exitCode) {
    if (Number(exitCode) === 0) {
      copiedContextId = pendingContextId
      actionStatus = "CONTEXT COPIED"
      actionClear.restart()
    } else {
      copiedContextId = ""
      actionStatus = ""
      error = copyError.trim() || "Could not copy Dicta context."
    }
    pendingContextId = ""
  }

  function relativeTime(value) {
    var timestamp = new Date(String(value || "")).getTime()
    if (!isFinite(timestamp)) return ""
    var seconds = Math.max(0, Math.floor((Date.now() - timestamp) / 1000))
    if (seconds < 60) return "now"
    var minutes = Math.floor(seconds / 60)
    if (minutes < 60) return minutes + " min ago"
    var hours = Math.floor(minutes / 60)
    if (hours < 24) return hours + " hr ago"
    var days = Math.floor(hours / 24)
    if (days === 1) return "Yesterday"
    if (days < 7) return days + " days ago"
    var date = new Date(timestamp)
    return date.toLocaleDateString(Qt.locale(), "MMM d")
  }

  Process {
    id: stateProcess
    running: false
    onExited: function(exitCode) { Qt.callLater(function() { root.finishRefresh(exitCode) }) }
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.stateOutput = text
    }
    stderr: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.stateError = text
    }
  }

  Process {
    id: copyProcess
    running: false
    onExited: function(exitCode) { Qt.callLater(function() { root.finishCopy(exitCode) }) }
    stderr: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.copyError = text
    }
  }

  Process {
    id: projectProcess
    running: false
    onExited: function(exitCode) { Qt.callLater(function() { root.finishProject(exitCode) }) }
    stderr: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.projectError = text
    }
  }

  Process {
    id: recordProcess
    running: false
    onExited: function(exitCode) { Qt.callLater(function() { root.finishRecord(exitCode) }) }
    stderr: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.recordError = text
    }
  }

  Process {
    id: dictaStatusProcess
    running: false
    onExited: function(exitCode) {
      Qt.callLater(function() { root.finishDictaStatus(exitCode) })
    }
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.statusOutput = text
    }
  }

  Timer {
    interval: 1000
    running: true
    repeat: true
    triggeredOnStart: true
    onTriggered: root.checkDictaRunning()
  }

  Timer {
    interval: root.refreshInterval
    running: true
    repeat: true
    triggeredOnStart: true
    onTriggered: root.refresh(root.selectedProjectId)
  }

  Timer {
    id: actionClear
    interval: 2200
    repeat: false
    onTriggered: {
      root.actionStatus = ""
      root.copiedContextId = ""
    }
  }
}
