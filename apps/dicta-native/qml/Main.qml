pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ApplicationWindow {
    id: root
    objectName: "dictaMainWindow"

    required property QtObject bridge
    required property QtObject dictaTheme
    property bool settingsOpen: false
    property bool compactProjectsOpen: false
    property bool wideLayout: width >= 1180 * dictaTheme.spacingScale
    property bool railLayout: width >= 900 * dictaTheme.spacingScale
    property bool showingDetail: bridge.selectedRecordingId.length > 0
    property bool recording: bridge.runtimePhase === "recording"
        || bridge.runtimePhase === "annotating"
    property bool autoSelecting: false
    property bool autoOpenLatest: true

    width: 1440
    height: 900
    minimumWidth: 720
    minimumHeight: 560
    visible: true
    title: "Dicta"
    color: dictaTheme.background

    function currentBranch() {
        var recordings = bridge.recentRecordings || []
        for (var i = 0; i < recordings.length; ++i) {
            if (recordings[i].branch)
                return recordings[i].branch
        }
        return "repository"
    }

    function showRecording(recordingId) {
        return bridge.selectRecording(recordingId)
    }

    function updateTranscriptionLanguage(language) {
        return bridge.setTranscriptionLanguage(language)
    }

    function updateShortcut(shortcutId) {
        return bridge.setShortcut(shortcutId)
    }

    function updateBranchLocking(enabled) {
        return bridge.setBranchLocking(enabled)
    }

    function updateCleanupPolicy(enabled) {
        return bridge.setCleanupMergedVideos(enabled)
    }

    function updateGeneralPath(path) {
        return bridge.setGeneralPath(path)
    }

    function cleanupMergedVideos() {
        return bridge.cleanupMergedVideos()
    }

    function installQualityModel() {
        return bridge.installQualityModel()
    }

    onSettingsOpenChanged: {
        if (settingsOpen)
            bridge.refreshCodexMcp()
    }

    function ensureWideSelection() {
        if (!autoOpenLatest || !wideLayout || settingsOpen || showingDetail || autoSelecting)
            return
        var recordings = bridge.recentRecordings || []
        if (!recordings.length)
            return
        autoSelecting = true
        bridge.selectRecording(recordings[0].id)
        autoSelecting = false
    }

    Shortcut {
        sequence: "Ctrl+Space"
        enabled: bridge.hostState === "running"
        onActivated: root.recording
            ? bridge.stopRecording()
            : bridge.startRecording(composer.noteText)
    }

    Shortcut {
        sequence: "Ctrl+K"
        enabled: !root.settingsOpen
        onActivated: recordingPanel.toggleFilter()
    }

    Connections {
        target: bridge
        function onDashboardChanged() { Qt.callLater(root.ensureWideSelection) }
        function onSelectedRecordingChanged() { root.autoSelecting = false }
    }

    Component.onCompleted: {
        bridge.refreshDashboard()
        Qt.callLater(ensureWideSelection)
    }

    RowLayout {
        anchors.fill: parent
        spacing: 0

        ProjectRail {
            id: projectRail
            visible: root.railLayout
            Layout.preferredWidth: 254 * root.dictaTheme.spacingScale
            Layout.fillHeight: true
            bridge: root.bridge
            dictaTheme: root.dictaTheme
            onSettingsRequested: root.settingsOpen = true
            onAddProjectRequested: createProjectDialog.open()
        }

        Rectangle {
            visible: root.railLayout
            Layout.preferredWidth: 1
            Layout.fillHeight: true
            color: Qt.rgba(root.dictaTheme.muted.r, root.dictaTheme.muted.g,
                root.dictaTheme.muted.b, 0.6)
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 0

            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 86 * root.dictaTheme.spacingScale
                color: root.dictaTheme.background

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 28 * root.dictaTheme.spacingScale
                    anchors.rightMargin: 22 * root.dictaTheme.spacingScale
                    spacing: 12 * root.dictaTheme.spacingScale

                    FlatButton {
                        visible: !root.railLayout
                        dictaTheme: root.dictaTheme
                        iconName: "folder"
                        iconOnly: true
                        quiet: true
                        toolTip: "Projects"
                        onClicked: root.compactProjectsOpen = !root.compactProjectsOpen
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 5 * root.dictaTheme.spacingScale
                        Text {
                            Layout.fillWidth: true
                            text: root.bridge.currentProject.name || "Dicta"
                            color: root.dictaTheme.brightForeground
                            font.family: root.dictaTheme.fontFamily
                            font.pixelSize: root.dictaTheme.baseFontSize + 5
                            font.weight: Font.DemiBold
                            elide: Text.ElideRight
                        }
                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 10 * root.dictaTheme.spacingScale
                            ThemeIcon {
                                Layout.preferredWidth: 15 * root.dictaTheme.spacingScale
                                Layout.preferredHeight: width
                                iconName: "folder"
                                iconColor: root.dictaTheme.darkForeground
                                iconSize: Math.round(width)
                            }
                            Text {
                                text: root.bridge.currentProject.path || "General recordings"
                                color: root.dictaTheme.darkForeground
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: root.dictaTheme.baseFontSize
                                elide: Text.ElideMiddle
                            }
                            Rectangle {
                                Layout.preferredWidth: 1
                                Layout.preferredHeight: 16 * root.dictaTheme.spacingScale
                                color: root.dictaTheme.muted
                            }
                            Text {
                                text: root.currentBranch()
                                color: root.dictaTheme.accent
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: root.dictaTheme.baseFontSize
                            }
                        }
                    }

                    FlatButton {
                        dictaTheme: root.dictaTheme
                        iconName: "search"
                        text: "Ctrl K"
                        Layout.preferredWidth: 102 * root.dictaTheme.spacingScale
                        Layout.preferredHeight: 40 * root.dictaTheme.spacingScale
                        selected: recordingPanel.filterVisible
                        toolTip: "Filter recordings"
                        onClicked: recordingPanel.toggleFilter()
                    }
                    FlatButton {
                        visible: !root.railLayout
                        dictaTheme: root.dictaTheme
                        iconName: "settings"
                        iconOnly: true
                        quiet: true
                        toolTip: "Settings"
                        onClicked: root.settingsOpen = true
                    }
                }
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 1
                color: Qt.rgba(root.dictaTheme.muted.r, root.dictaTheme.muted.g,
                    root.dictaTheme.muted.b, 0.55)
            }

            RecordingComposer {
                id: composer
                Layout.fillWidth: true
                Layout.leftMargin: 24 * root.dictaTheme.spacingScale
                Layout.rightMargin: 24 * root.dictaTheme.spacingScale
                Layout.topMargin: 10 * root.dictaTheme.spacingScale
                Layout.bottomMargin: 10 * root.dictaTheme.spacingScale
                bridge: root.bridge
                dictaTheme: root.dictaTheme
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 1
                color: Qt.rgba(root.dictaTheme.muted.r, root.dictaTheme.muted.g,
                    root.dictaTheme.muted.b, 0.55)
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: 0

                    RecordingList {
                        id: recordingPanel
                        objectName: "recordingPanel"
                    visible: root.wideLayout || !root.showingDetail
                    Layout.preferredWidth: root.wideLayout
                        ? 464 * root.dictaTheme.spacingScale : -1
                    Layout.fillWidth: !root.wideLayout
                    Layout.fillHeight: true
                    bridge: root.bridge
                    dictaTheme: root.dictaTheme
                }

                Rectangle {
                    visible: root.wideLayout
                    Layout.preferredWidth: 1
                    Layout.fillHeight: true
                    color: Qt.rgba(root.dictaTheme.muted.r, root.dictaTheme.muted.g,
                        root.dictaTheme.muted.b, 0.55)
                }

                RecordingDetail {
                    id: detailPage
                    objectName: "recordingDetailPage"
                    visible: root.showingDetail
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    bridge: root.bridge
                    dictaTheme: root.dictaTheme
                }

                Item {
                    visible: root.wideLayout && !root.showingDetail
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    Column {
                        anchors.centerIn: parent
                        width: Math.min(parent.width - 64, 420 * root.dictaTheme.spacingScale)
                        spacing: 10 * root.dictaTheme.spacingScale
                        Text {
                            width: parent.width
                            horizontalAlignment: Text.AlignHCenter
                            text: "Select a recording"
                            color: root.dictaTheme.brightForeground
                            font.family: root.dictaTheme.fontFamily
                            font.pixelSize: root.dictaTheme.baseFontSize + 3
                        }
                        Text {
                            width: parent.width
                            horizontalAlignment: Text.AlignHCenter
                            wrapMode: Text.Wrap
                            text: "Video, transcript, chapters, notes, and agent context stay together here."
                            color: root.dictaTheme.darkForeground
                            font.family: root.dictaTheme.fontFamily
                            font.pixelSize: root.dictaTheme.baseFontSize
                        }
                    }
                }
            }
        }
    }

    Rectangle {
        visible: root.compactProjectsOpen && !root.railLayout
        anchors.left: parent.left
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        width: Math.min(parent.width * 0.78, 280 * root.dictaTheme.spacingScale)
        z: 20
        ProjectRail {
            anchors.fill: parent
            bridge: root.bridge
            dictaTheme: root.dictaTheme
            onSettingsRequested: {
                root.compactProjectsOpen = false
                root.settingsOpen = true
            }
            onAddProjectRequested: createProjectDialog.open()
        }
    }

    Rectangle {
        visible: root.settingsOpen
        anchors.fill: parent
        z: 30
        color: root.dictaTheme.background

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 28 * root.dictaTheme.spacingScale
            spacing: 18 * root.dictaTheme.spacingScale

            RowLayout {
                Layout.fillWidth: true
                FlatButton {
                    dictaTheme: root.dictaTheme
                    iconName: "back"
                    iconOnly: true
                    quiet: true
                    toolTip: "Back to Dicta"
                    onClicked: root.settingsOpen = false
                }
                Text {
                    Layout.fillWidth: true
                    text: "Settings"
                    color: root.dictaTheme.brightForeground
                    font.family: root.dictaTheme.fontFamily
                    font.pixelSize: root.dictaTheme.baseFontSize + 5
                    font.weight: Font.DemiBold
                }
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 1
                color: root.dictaTheme.muted
            }

            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                contentWidth: availableWidth

                GridLayout {
                    width: parent.width
                    columns: root.width > 900 * root.dictaTheme.spacingScale ? 2 : 1
                    columnSpacing: 18 * root.dictaTheme.spacingScale
                    rowSpacing: 18 * root.dictaTheme.spacingScale

                    Rectangle {
                        Layout.fillWidth: true
                        Layout.preferredHeight: 208 * root.dictaTheme.spacingScale
                        radius: 3 * root.dictaTheme.spacingScale
                        color: root.dictaTheme.darkBackground
                        border.width: 1
                        border.color: root.dictaTheme.muted
                        Column {
                            anchors.fill: parent
                            anchors.margins: 16 * root.dictaTheme.spacingScale
                            spacing: 7 * root.dictaTheme.spacingScale
                            Text {
                                width: parent.width
                                text: "APPEARANCE"
                                color: root.dictaTheme.darkForeground
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
                                font.weight: Font.DemiBold
                            }
                            Text {
                                width: parent.width
                                text: "Follows Omarchy · " + root.dictaTheme.name
                                color: root.dictaTheme.brightForeground
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: root.dictaTheme.baseFontSize + 1
                                elide: Text.ElideMiddle
                            }
                            Text {
                                width: parent.width
                                text: root.dictaTheme.fontFamily + " · "
                                    + root.dictaTheme.baseFontSize + " px\n"
                                    + "colors.toml and shell.toml reload live."
                                color: root.dictaTheme.darkForeground
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
                                wrapMode: Text.Wrap
                            }
                        }
                    }

                    Rectangle {
                        Layout.fillWidth: true
                        Layout.preferredHeight: 152 * root.dictaTheme.spacingScale
                        radius: 3 * root.dictaTheme.spacingScale
                        color: root.dictaTheme.darkBackground
                        border.width: 1
                        border.color: root.dictaTheme.muted
                        Column {
                            anchors.fill: parent
                            anchors.margins: 16 * root.dictaTheme.spacingScale
                            spacing: 8 * root.dictaTheme.spacingScale
                            Text {
                                text: "TRANSCRIPTION"
                                color: root.dictaTheme.darkForeground
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
                                font.weight: Font.DemiBold
                            }
                            Text {
                                width: parent.width
                                text: root.bridge.modelStatus.active_model || "No active model"
                                color: root.dictaTheme.brightForeground
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: root.dictaTheme.baseFontSize + 1
                                elide: Text.ElideRight
                            }
                            Row {
                                width: parent.width
                                spacing: 8 * root.dictaTheme.spacingScale
                                FlatButton {
                                    objectName: "installQualityModel"
                                    dictaTheme: root.dictaTheme
                                    text: root.bridge.modelStatus.quality_state === "ready"
                                        ? "QUALITY READY"
                                        : root.bridge.modelStatus.quality_state === "installing"
                                            ? "INSTALLING…" : "INSTALL QUALITY"
                                    selected: root.bridge.modelStatus.quality_state === "ready"
                                    enabled: root.bridge.hostState === "running"
                                        && root.bridge.runtimePhase === "idle"
                                        && root.bridge.modelStatus.quality_state !== "ready"
                                        && root.bridge.modelStatus.quality_state !== "installing"
                                    onClicked: root.installQualityModel()
                                }
                                Text {
                                    width: parent.width - 160 * root.dictaTheme.spacingScale
                                    anchors.verticalCenter: parent.verticalCenter
                                    text: root.bridge.modelStatus.install_stage
                                        ? root.bridge.modelStatus.install_stage + " · "
                                            + Math.round(Number(
                                                root.bridge.modelStatus.downloaded_bytes || 0)
                                                / 1048576) + " MiB"
                                        : root.bridge.modelStatus.message || "Local Whisper"
                                    color: root.dictaTheme.darkForeground
                                    font.family: root.dictaTheme.fontFamily
                                    font.pixelSize: Math.max(9,
                                        root.dictaTheme.baseFontSize - 1)
                                    elide: Text.ElideRight
                                }
                            }
                            Flow {
                                width: parent.width
                                spacing: 5 * root.dictaTheme.spacingScale
                                Repeater {
                                    model: ["auto", "nl", "en", "fr", "de", "es"]
                                    FlatButton {
                                        required property string modelData
                                        objectName: "language-" + modelData
                                        dictaTheme: root.dictaTheme
                                        text: modelData.toUpperCase()
                                        selected: root.bridge.settings.transcription_language === modelData
                                        onClicked: root.updateTranscriptionLanguage(modelData)
                                    }
                                }
                            }
                        }
                    }

                    Rectangle {
                        Layout.fillWidth: true
                        Layout.preferredHeight: 274 * root.dictaTheme.spacingScale
                        radius: 3 * root.dictaTheme.spacingScale
                        color: root.dictaTheme.darkBackground
                        border.width: 1
                        border.color: root.dictaTheme.muted
                        ColumnLayout {
                            anchors.fill: parent
                            anchors.margins: 16 * root.dictaTheme.spacingScale
                            spacing: 8 * root.dictaTheme.spacingScale
                            Text {
                                text: "STORAGE"
                                color: root.dictaTheme.darkForeground
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
                                font.weight: Font.DemiBold
                            }
                            RowLayout {
                                Layout.fillWidth: true
                                Text {
                                    Layout.fillWidth: true
                                    text: "Lock Git recordings to branches"
                                    color: root.dictaTheme.foreground
                                    font.family: root.dictaTheme.fontFamily
                                    font.pixelSize: root.dictaTheme.baseFontSize
                                }
                                FlatButton {
                                    objectName: "branchLockingToggle"
                                    dictaTheme: root.dictaTheme
                                    text: root.bridge.settings.branch_locking ? "ON" : "OFF"
                                    selected: root.bridge.settings.branch_locking
                                    onClicked: root.updateBranchLocking(
                                        !root.bridge.settings.branch_locking)
                                }
                            }
                            RowLayout {
                                Layout.fillWidth: true
                                Text {
                                    Layout.fillWidth: true
                                    text: "Clean videos from merged branches"
                                    color: root.dictaTheme.foreground
                                    font.family: root.dictaTheme.fontFamily
                                    font.pixelSize: root.dictaTheme.baseFontSize
                                }
                                FlatButton {
                                    objectName: "cleanupToggle"
                                    dictaTheme: root.dictaTheme
                                    text: root.bridge.settings.cleanup_merged_videos ? "ON" : "OFF"
                                    selected: root.bridge.settings.cleanup_merged_videos
                                    onClicked: root.updateCleanupPolicy(
                                        !root.bridge.settings.cleanup_merged_videos)
                                }
                            }
                            Text {
                                text: "General recordings path"
                                color: root.dictaTheme.darkForeground
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
                            }
                            RowLayout {
                                Layout.fillWidth: true
                                TextField {
                                    id: generalPathField
                                    objectName: "generalPathField"
                                    Layout.fillWidth: true
                                    text: root.bridge.settings.general_path || ""
                                    placeholderText: "Default · ~/Documents/Dicta/General"
                                    color: root.dictaTheme.foreground
                                    placeholderTextColor: root.dictaTheme.darkForeground
                                    font.family: root.dictaTheme.fontFamily
                                    font.pixelSize: root.dictaTheme.baseFontSize
                                    selectByMouse: true
                                    background: Rectangle {
                                        color: root.dictaTheme.background
                                        border.width: 1
                                        border.color: generalPathField.activeFocus
                                            ? root.dictaTheme.accent : root.dictaTheme.muted
                                        radius: 3 * root.dictaTheme.spacingScale
                                    }
                                }
                                FlatButton {
                                    objectName: "generalPathApply"
                                    dictaTheme: root.dictaTheme
                                    text: generalPathField.text.trim().length ? "APPLY" : "DEFAULT"
                                    onClicked: root.updateGeneralPath(generalPathField.text)
                                }
                            }
                            RowLayout {
                                Layout.fillWidth: true
                                FlatButton {
                                    objectName: "cleanupNow"
                                    dictaTheme: root.dictaTheme
                                    text: "CLEAN MERGED VIDEOS"
                                    enabled: root.bridge.settings.cleanup_merged_videos
                                        && Boolean(root.bridge.currentProject.id)
                                    onClicked: root.cleanupMergedVideos()
                                }
                                Text {
                                    Layout.fillWidth: true
                                    text: root.bridge.settingsMessage
                                    color: root.dictaTheme.accent
                                    font.family: root.dictaTheme.fontFamily
                                    font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
                                    elide: Text.ElideRight
                                }
                            }
                        }
                    }

                    Rectangle {
                        Layout.fillWidth: true
                        Layout.preferredHeight: 208 * root.dictaTheme.spacingScale
                        radius: 3 * root.dictaTheme.spacingScale
                        color: root.dictaTheme.darkBackground
                        border.width: 1
                        border.color: root.dictaTheme.muted
                        ColumnLayout {
                            anchors.fill: parent
                            anchors.margins: 16 * root.dictaTheme.spacingScale
                            spacing: 8 * root.dictaTheme.spacingScale
                            Text {
                                text: "CODEX MCP"
                                color: root.dictaTheme.darkForeground
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
                                font.weight: Font.DemiBold
                            }
                            Text {
                                Layout.fillWidth: true
                                text: root.bridge.codexMcp.mcp_path
                                    || "Packaged dicta-mcp was not found"
                                color: root.dictaTheme.brightForeground
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: root.dictaTheme.baseFontSize
                                elide: Text.ElideMiddle
                            }
                            Text {
                                Layout.fillWidth: true
                                text: root.bridge.codexMcp.message || "Checking Codex…"
                                color: root.bridge.codexMcp.state === "connected"
                                    ? root.dictaTheme.accent : root.dictaTheme.darkForeground
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
                                wrapMode: Text.Wrap
                            }
                            RowLayout {
                                Layout.fillWidth: true
                                FlatButton {
                                    objectName: "connectCodexMcp"
                                    dictaTheme: root.dictaTheme
                                    text: root.bridge.codexMcp.state === "connected"
                                        ? "CONNECTED" : "CONNECT CODEX"
                                    selected: root.bridge.codexMcp.state === "connected"
                                    enabled: root.bridge.codexMcp.state === "disconnected"
                                    onClicked: root.bridge.connectCodexMcp()
                                }
                                FlatButton {
                                    objectName: "restartCodexMcp"
                                    dictaTheme: root.dictaTheme
                                    text: "RESTART DICTA MCP"
                                    enabled: Boolean(root.bridge.codexMcp.codex_path)
                                        && Boolean(root.bridge.codexMcp.mcp_path)
                                    onClicked: root.bridge.restartCodexMcp()
                                }
                                Item { Layout.fillWidth: true }
                            }
                        }
                    }

                    Rectangle {
                        Layout.fillWidth: true
                        Layout.preferredHeight: 274 * root.dictaTheme.spacingScale
                        radius: 3 * root.dictaTheme.spacingScale
                        color: root.dictaTheme.darkBackground
                        border.width: 1
                        border.color: root.dictaTheme.muted
                        Column {
                            anchors.fill: parent
                            anchors.margins: 16 * root.dictaTheme.spacingScale
                            spacing: 8 * root.dictaTheme.spacingScale
                            Text {
                                text: "RECORDING SHORTCUT"
                                color: root.dictaTheme.darkForeground
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
                                font.weight: Font.DemiBold
                            }
                            Text {
                                width: parent.width
                                text: "Choose the native preset. Omarchy binding activation is shown separately."
                                color: root.dictaTheme.darkForeground
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
                                wrapMode: Text.Wrap
                            }
                            Flow {
                                width: parent.width
                                spacing: 6 * root.dictaTheme.spacingScale
                                Repeater {
                                    model: [
                                        {id: "alt_shift_r", label: "ALT SHIFT R"},
                                        {id: "command_shift_d", label: "SUPER SHIFT D"},
                                        {id: "option_space", label: "ALT SPACE"},
                                        {id: "control_space", label: "CTRL SPACE"}
                                    ]
                                    FlatButton {
                                        required property var modelData
                                        objectName: "shortcut-" + modelData.id
                                        dictaTheme: root.dictaTheme
                                        text: modelData.label
                                        selected: root.bridge.settings.shortcut_id === modelData.id
                                        onClicked: root.updateShortcut(modelData.id)
                                    }
                                }
                            }
                            Text {
                                width: parent.width
                                text: "Current · " + (root.bridge.settings.shortcut_id || "alt_shift_r")
                                color: root.dictaTheme.accent
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: root.dictaTheme.baseFontSize
                                wrapMode: Text.Wrap
                            }
                        }
                    }
                }
            }
        }
    }

    Rectangle {
        visible: root.bridge.uiError.length > 0 || root.bridge.hostError.length > 0
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        z: 50
        implicitHeight: errorText.implicitHeight + 18 * root.dictaTheme.spacingScale
        color: root.dictaTheme.darkBackground
        border.width: 1
        border.color: root.dictaTheme.red
        Text {
            id: errorText
            anchors.fill: parent
            anchors.margins: 9 * root.dictaTheme.spacingScale
            text: root.bridge.uiError || root.bridge.hostError
            color: root.dictaTheme.red
            font.family: root.dictaTheme.fontFamily
            font.pixelSize: root.dictaTheme.baseFontSize
            wrapMode: Text.Wrap
        }
    }

    Dialog {
        id: createProjectDialog
        title: "Create project"
        modal: true
        anchors.centerIn: parent
        standardButtons: Dialog.Ok | Dialog.Cancel
        onAccepted: {
            if (projectName.text.trim().length)
                root.bridge.createProject(projectName.text.trim())
            projectName.text = ""
        }
        contentItem: ColumnLayout {
            spacing: 8 * root.dictaTheme.spacingScale
            Text {
                text: "Create a standalone Dicta project"
                color: root.dictaTheme.foreground
                font.family: root.dictaTheme.fontFamily
                font.pixelSize: root.dictaTheme.baseFontSize
            }
            TextField {
                id: projectName
                Layout.preferredWidth: 360 * root.dictaTheme.spacingScale
                placeholderText: "Project name"
                color: root.dictaTheme.foreground
                placeholderTextColor: root.dictaTheme.darkForeground
                font.family: root.dictaTheme.fontFamily
                font.pixelSize: root.dictaTheme.baseFontSize
            }
        }
        background: Rectangle {
            color: root.dictaTheme.darkBackground
            border.width: 1
            border.color: root.dictaTheme.accent
            radius: 4 * root.dictaTheme.spacingScale
        }
    }
}
