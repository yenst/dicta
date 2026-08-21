pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts

ApplicationWindow {
    id: root
    objectName: "dictaMainWindow"

    required property QtObject bridge
    required property QtObject dictaTheme
    property bool settingsOpen: false
    property string settingsSection: "appearance"
    property bool compactProjectsOpen: false
    property bool wideLayout: width >= 1180 * dictaTheme.spacingScale
    property bool railLayout: width >= 900 * dictaTheme.spacingScale
    property bool showingDetail: bridge.selectedRecordingId.length > 0
    property bool recording: bridge.runtimePhase === "recording"
        || bridge.runtimePhase === "annotating"
    property bool autoSelecting: false
    property bool autoOpenLatest: false
    property bool searchExpanded: false
    property int navigationColumn: 1
    property int transcriptionElapsedSeconds: 0
    property string previousRuntimePhase: ""
    property bool branchCopied: false

    width: 1440
    height: 900
    minimumWidth: 720
    minimumHeight: 560
    visible: true
    title: "Dicta"
    color: dictaTheme.background

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

    function pathFromFolderUrl(folderUrl) {
        var value = decodeURIComponent(String(folderUrl || ""))
        return value.indexOf("file://") === 0 ? value.substring(7) : value
    }

    function linkProjectFolder(folderUrl) {
        return bridge.addProject(pathFromFolderUrl(folderUrl))
    }

    function cleanupMergedVideos() {
        return bridge.cleanupMergedVideos()
    }

    function installQualityModel() {
        return bridge.installQualityModel()
    }

    function updateAppearance(appearance) {
        var changed = dictaTheme.setAppearance(appearance)
        if (changed)
            bridge.showToast("Appearance changed")
        return changed
    }

    function copyMcpConfig(provider) {
        var command = String(bridge.codexMcp.mcp_path || "dicta-mcp")
        var config = JSON.stringify({mcpServers: {dicta: {command: command}}}, null, 2)
        if (!bridge.copyText(config))
            return false
        bridge.showToast(provider + " MCP config copied")
        return true
    }

    function settingsTitle(section) {
        if (section === "connections") return "MCP connections"
        if (section === "shortcuts") return "Shortcuts"
        if (section === "transcription") return "Transcription"
        if (section === "storage") return "Storage"
        return "Appearance"
    }

    function settingsDescription(section) {
        if (section === "connections") return "Choose which local tools can use Dicta context."
        if (section === "shortcuts") return "Choose the global shortcut that starts or stops recording."
        if (section === "transcription") return "Manage the local speech model and transcript language."
        if (section === "storage") return "Control branch scope, paths, and merged-video cleanup across linked projects."
        return "Follow the desktop theme or choose a portable Dicta appearance."
    }

    onRailLayoutChanged: {
        if (railLayout)
            compactProjectsOpen = false
    }

    onSettingsOpenChanged: {
        if (settingsOpen)
            bridge.refreshCodexMcp()
        else
            Qt.callLater(function() { keyboardFocus.forceActiveFocus() })
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

    function keyboardNavigationEnabled() {
        return !settingsOpen && !compactProjectsOpen
            && !globalSearch.activeFocus && !detailPage.noteEditorFocused
    }

    function restoreKeyboardFocus(column) {
        if (column !== undefined)
            navigationColumn = Math.max(0, Math.min(2, Number(column)))
        if (keyboardNavigationEnabled())
            keyboardFocus.forceActiveFocus()
    }

    function moveNavigationColumn(delta) {
        navigationColumn = Math.max(0, Math.min(showingDetail ? 2 : 1,
            navigationColumn + delta))
        if (navigationColumn === 2)
            detailPage.resetKeyboardTarget()
    }

    function cycleNavigationColumn(delta) {
        var count = showingDetail ? 3 : 2
        navigationColumn = (navigationColumn + delta + count) % count
        if (navigationColumn === 2)
            detailPage.resetKeyboardTarget()
        restoreKeyboardFocus(navigationColumn)
    }

    function moveVertical(delta) {
        if (navigationColumn === 0)
            projectRail.moveKeyboardSelection(delta)
        else if (navigationColumn === 1)
            recordingPanel.moveKeyboardSelection(delta)
        else if (navigationColumn === 2)
            detailPage.moveKeyboardVertical(delta)
    }

    function handleNavigationKey(event) {
        if (!keyboardNavigationEnabled())
            return
        if (event.key === Qt.Key_Tab || event.key === Qt.Key_Backtab) {
            var backwards = event.key === Qt.Key_Backtab
                    || Boolean(event.modifiers & Qt.ShiftModifier)
            cycleNavigationColumn(backwards ? -1 : 1)
        } else if (event.modifiers !== Qt.NoModifier) {
            return
        } else if (event.key === Qt.Key_Left) {
            if (navigationColumn !== 2 || !detailPage.moveKeyboardHorizontal(-1))
                moveNavigationColumn(-1)
        } else if (event.key === Qt.Key_Right) {
            if (navigationColumn === 2)
                detailPage.moveKeyboardHorizontal(1)
            else
                moveNavigationColumn(1)
        }
        else if (event.key === Qt.Key_Up)
            moveVertical(-1)
        else if (event.key === Qt.Key_Down)
            moveVertical(1)
        else if (event.key === Qt.Key_Escape && navigationColumn > 0)
            moveNavigationColumn(-1)
        else if (event.key === Qt.Key_Space) {
            if (navigationColumn === 0)
                projectRail.activateRecordingSelection()
            else if (navigationColumn === 2 && showingDetail
                    && detailPage.keyboardTarget === 0)
                detailPage.togglePlayback()
            else
                return
        }
        else if (event.key === Qt.Key_C && showingDetail)
            detailPage.copyContext()
        else if (event.key === Qt.Key_Delete) {
            var deleteAccepted = navigationColumn === 0
                ? projectRail.requestDelete()
                : showingDetail && recordingPanel.requestDelete()
            if (!deleteAccepted && root.recording)
                bridge.showToast("Stop recording before deleting")
        }
        else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
            if (navigationColumn === 0)
                projectRail.activateKeyboardSelection()
            else if (navigationColumn === 1)
                recordingPanel.activateKeyboardSelection()
            else if (navigationColumn === 2 && showingDetail)
                detailPage.activateKeyboardTarget()
        } else {
            return
        }
        event.accepted = true
    }

    Shortcut {
        sequence: "Ctrl+Space"
        enabled: bridge.hostState === "running"
        onActivated: root.recording
            ? bridge.stopRecording()
            : bridge.startRecording("")
    }

    Shortcut {
        sequence: "Ctrl+K"
        enabled: !root.settingsOpen
        onActivated: {
            root.searchExpanded = true
            globalSearch.forceActiveFocus()
            globalSearch.selectAll()
        }
    }
    Shortcut {
        sequence: "Escape"
        enabled: root.settingsOpen || root.compactProjectsOpen
        onActivated: {
            if (root.settingsOpen)
                root.settingsOpen = false
            else
                root.compactProjectsOpen = false
        }
    }

    Shortcut {
        sequence: "Left"
        enabled: root.settingsOpen
        onActivated: root.settingsOpen = false
    }

    Connections {
        target: bridge
        function onDashboardChanged() {
            if (root.bridge.runtimePhase === "transcribing"
                    && root.previousRuntimePhase !== "transcribing")
                root.transcriptionElapsedSeconds = 0
            root.previousRuntimePhase = root.bridge.runtimePhase
            Qt.callLater(root.ensureWideSelection)
        }
        function onSelectedRecordingChanged() { root.autoSelecting = false }
    }

    onActiveChanged: if (active)
        Qt.callLater(function() { root.restoreKeyboardFocus() })

    Timer {
        interval: 1000
        repeat: true
        running: root.bridge.runtimePhase === "transcribing"
        onTriggered: root.transcriptionElapsedSeconds += 1
    }

    Timer {
        id: branchCopiedTimer
        interval: 1600
        repeat: false
        onTriggered: root.branchCopied = false
    }

    Component.onCompleted: {
        bridge.refreshDashboard()
        keyboardFocus.forceActiveFocus()
        Qt.callLater(ensureWideSelection)
    }

    Item {
        id: keyboardFocus
        objectName: "keyboardNavigationFocus"
        width: 0
        height: 0
        focus: true
        Keys.onPressed: event => root.handleNavigationKey(event)
    }

    RowLayout {
        anchors.fill: parent
        spacing: 0

        ProjectRail {
            id: projectRail
            objectName: "projectRail"
            visible: root.railLayout
            Layout.preferredWidth: 254 * root.dictaTheme.spacingScale
            Layout.fillHeight: true
            bridge: root.bridge
            dictaTheme: root.dictaTheme
            settingsActive: root.settingsOpen
            keyboardFocused: root.navigationColumn === 0 && !root.settingsOpen
            onSettingsRequested: root.settingsOpen = true
            onAddProjectRequested: linkProjectFolderDialog.open()
            onKeyboardFocusRequested: root.restoreKeyboardFocus(0)
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
                Layout.leftMargin: 24 * root.dictaTheme.spacingScale
                Layout.rightMargin: 24 * root.dictaTheme.spacingScale
                Layout.topMargin: 10 * root.dictaTheme.spacingScale
                Layout.bottomMargin: 10 * root.dictaTheme.spacingScale
                Layout.preferredHeight: 58 * root.dictaTheme.spacingScale
                color: "transparent"

                RowLayout {
                    anchors.fill: parent
                    spacing: 16 * root.dictaTheme.spacingScale

                    FlatButton {
                        objectName: "compactProjectsButton"
                        visible: !root.railLayout
                        dictaTheme: root.dictaTheme
                        iconName: "folder"
                        iconOnly: true
                        quiet: true
                        selected: root.compactProjectsOpen
                        toolTip: "Projects"
                        onClicked: root.compactProjectsOpen = !root.compactProjectsOpen
                    }

                    RowLayout {
                        visible: !root.searchExpanded
                        Layout.fillWidth: true
                        spacing: 9 * root.dictaTheme.spacingScale
                        Text {
                            Layout.maximumWidth: 260 * root.dictaTheme.spacingScale
                            text: root.bridge.currentProject.name || "Dicta"
                            color: root.dictaTheme.brightForeground
                            font.family: root.dictaTheme.fontFamily
                            font.pixelSize: root.dictaTheme.baseFontSize + 4
                            font.weight: Font.DemiBold
                            elide: Text.ElideRight
                        }
                        FlatButton {
                            objectName: "projectBranchBadge"
                            visible: Boolean(root.bridge.currentProject.branch)
                            dictaTheme: root.dictaTheme
                            text: root.bridge.currentProject.branch || ""
                            iconName: root.branchCopied ? "check" : "branch"
                            selected: true
                            toolTip: "Copy Git branch"
                            onClicked: {
                                var branch = root.bridge.currentProject.branch || ""
                                if (!root.bridge.copyText(branch))
                                    return
                                root.branchCopied = true
                                branchCopiedTimer.restart()
                                root.bridge.showToast("Git branch copied · " + branch)
                            }
                        }
                        Item { Layout.fillWidth: true }
                    }

                    Rectangle {
                        Layout.fillWidth: root.searchExpanded
                        Layout.preferredWidth: root.searchExpanded
                            ? -1 : 350 * root.dictaTheme.spacingScale
                        Layout.preferredHeight: 44 * root.dictaTheme.spacingScale
                        radius: 4 * root.dictaTheme.spacingScale
                        color: root.dictaTheme.darkBackground
                        border.width: 1
                        border.color: globalSearch.activeFocus
                            ? root.dictaTheme.accent : root.dictaTheme.muted

                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 13 * root.dictaTheme.spacingScale
                            anchors.rightMargin: 12 * root.dictaTheme.spacingScale
                            spacing: 9 * root.dictaTheme.spacingScale
                            ThemeIcon {
                                Layout.preferredWidth: 17 * root.dictaTheme.spacingScale
                                Layout.preferredHeight: width
                                iconName: "search"
                                iconColor: globalSearch.activeFocus
                                    ? root.dictaTheme.accent : root.dictaTheme.darkForeground
                                iconSize: Math.round(width)
                            }
                            TextField {
                                id: globalSearch
                                objectName: "globalSearchField"
                                Layout.fillWidth: true
                                placeholderText: "Search recordings"
                                color: root.dictaTheme.foreground
                                placeholderTextColor: root.dictaTheme.darkForeground
                                selectionColor: root.dictaTheme.selection
                                selectedTextColor: root.dictaTheme.brightForeground
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: root.dictaTheme.baseFontSize
                                leftPadding: 0
                                rightPadding: 0
                                background: Item {}
                                onActiveFocusChanged: {
                                    if (activeFocus)
                                        root.searchExpanded = true
                                    else if (!text.length)
                                        root.searchExpanded = false
                                }
                                Keys.onEscapePressed: {
                                    text = ""
                                    root.searchExpanded = false
                                    keyboardFocus.forceActiveFocus()
                                }
                            }
                            RowLayout {
                                visible: root.bridge.runtimePhase === "transcribing"
                                spacing: 8 * root.dictaTheme.spacingScale
                                ProgressBar {
                                    Layout.preferredWidth: 72 * root.dictaTheme.spacingScale
                                    indeterminate: true
                                }
                                Text {
                                    text: "Transcribing · "
                                        + String(Math.floor(root.transcriptionElapsedSeconds / 60)).padStart(2, "0")
                                        + ":" + String(root.transcriptionElapsedSeconds % 60).padStart(2, "0")
                                    color: root.dictaTheme.accent
                                    font.family: root.dictaTheme.fontFamily
                                    font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 2)
                                }
                            }
                            Text {
                                visible: root.bridge.runtimePhase !== "transcribing"
                                    && !globalSearch.activeFocus && !globalSearch.text.length
                                text: "Ctrl K"
                                color: root.dictaTheme.darkForeground
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 2)
                            }
                        }
                    }

                    FlatButton {
                        objectName: "compactSettingsButton"
                        visible: !root.railLayout
                        dictaTheme: root.dictaTheme
                        iconName: "settings"
                        iconOnly: true
                        quiet: true
                        toolTip: "Settings"
                        onClicked: {
                            root.compactProjectsOpen = false
                            root.settingsOpen = true
                        }
                    }
                }
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
                    filterText: globalSearch.text
                    keyboardFocused: root.navigationColumn === 1 && !root.settingsOpen
                    onKeyboardFocusRequested: root.restoreKeyboardFocus(1)
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
                    keyboardActive: root.navigationColumn === 2 && !root.settingsOpen
                    onKeyboardFocusRequested: root.restoreKeyboardFocus(2)
                    onContextCopied: root.bridge.showToast(
                        root.bridge.selectedRecordingId + " copied")
                    onDeleteRequested: recordingPanel.requestDelete()
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
        anchors.fill: parent
        z: 19
        color: Qt.rgba(0, 0, 0, 0.35)
        MouseArea {
            anchors.fill: parent
            onClicked: root.compactProjectsOpen = false
        }
    }

    Rectangle {
        objectName: "compactProjectDrawer"
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
            settingsActive: root.settingsOpen
            onSettingsRequested: {
                root.compactProjectsOpen = false
                root.settingsOpen = true
            }
            onAddProjectRequested: linkProjectFolderDialog.open()
        }
    }

    SettingsPage {
        id: settingsPage
        visible: root.settingsOpen
        anchors.fill: parent
        anchors.leftMargin: root.railLayout
            ? 255 * root.dictaTheme.spacingScale : 0
        z: 30
        bridge: root.bridge
        dictaTheme: root.dictaTheme
        host: root
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

    FolderDialog {
        id: linkProjectFolderDialog
        objectName: "linkProjectFolderDialog"
        title: "Choose a Git repository to link"
        onAccepted: {
            root.linkProjectFolder(selectedFolder)
            Qt.callLater(function() { root.restoreKeyboardFocus(0) })
        }
        onRejected: Qt.callLater(function() { root.restoreKeyboardFocus(0) })
    }

}
