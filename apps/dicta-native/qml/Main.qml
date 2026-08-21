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
        if (section === "storage") return "Control branch scope, paths, and merged-video cleanup."
        return "Dicta follows your active Omarchy theme."
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

    function moveVertical(delta) {
        if (navigationColumn === 0)
            projectRail.moveKeyboardSelection(delta)
        else if (navigationColumn === 1)
            recordingPanel.moveKeyboardSelection(delta)
        else if (navigationColumn === 2)
            detailPage.moveKeyboardVertical(delta)
    }

    function handleNavigationKey(event) {
        if (!keyboardNavigationEnabled() || event.modifiers !== Qt.NoModifier)
            return
        if (event.key === Qt.Key_Left) {
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
        else if (event.key === Qt.Key_Space && navigationColumn === 2 && showingDetail
                && detailPage.keyboardTarget === 0)
            detailPage.togglePlayback()
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
        enabled: root.settingsOpen
        onActivated: root.settingsOpen = false
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
                Layout.preferredHeight: 0
                visible: false
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
                            spacing: 8 * root.dictaTheme.spacingScale
                            ThemeIcon {
                                Layout.preferredWidth: 15 * root.dictaTheme.spacingScale
                                Layout.preferredHeight: width
                                iconName: "branch"
                                iconColor: root.dictaTheme.accent
                                iconSize: Math.round(width)
                            }
                            Text {
                                Layout.fillWidth: true
                                text: root.bridge.currentProject.branch
                                    || (root.bridge.currentProject.id === "general"
                                        ? "General" : "Not a Git working tree")
                                color: root.dictaTheme.accent
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: root.dictaTheme.baseFontSize
                                elide: Text.ElideRight
                            }
                        }
                    }

                    Rectangle {
                        Layout.preferredWidth: Math.max(180 * root.dictaTheme.spacingScale,
                            Math.min(300 * root.dictaTheme.spacingScale, root.width * 0.28))
                        Layout.preferredHeight: 40 * root.dictaTheme.spacingScale
                        radius: 3 * root.dictaTheme.spacingScale
                        color: root.dictaTheme.darkBackground
                        border.width: 1
                        border.color: globalSearch.activeFocus
                            ? root.dictaTheme.accent : root.dictaTheme.muted

                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 11 * root.dictaTheme.spacingScale
                            anchors.rightMargin: 10 * root.dictaTheme.spacingScale
                            spacing: 8 * root.dictaTheme.spacingScale
                            ThemeIcon {
                                Layout.preferredWidth: 16 * root.dictaTheme.spacingScale
                                Layout.preferredHeight: width
                                iconName: "search"
                                iconColor: root.dictaTheme.darkForeground
                                iconSize: Math.round(width)
                            }
                            TextField {
                                id: legacyGlobalSearch
                                objectName: "legacyGlobalSearchField"
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
                                Keys.onEscapePressed: {
                                    text = ""
                                    focus = false
                                }
                            }
                            Text {
                                visible: !legacyGlobalSearch.activeFocus && !legacyGlobalSearch.text.length
                                text: "Ctrl K"
                                color: root.dictaTheme.darkForeground
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 2)
                            }
                        }
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
                Layout.preferredHeight: 0
                visible: false
                color: Qt.rgba(root.dictaTheme.muted.r, root.dictaTheme.muted.g,
                    root.dictaTheme.muted.b, 0.55)
            }

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

                    ColumnLayout {
                        visible: !root.searchExpanded
                        Layout.fillWidth: true
                        spacing: 4 * root.dictaTheme.spacingScale
                        Text {
                            Layout.fillWidth: true
                            text: root.bridge.currentProject.name || "Dicta"
                            color: root.dictaTheme.brightForeground
                            font.family: root.dictaTheme.fontFamily
                            font.pixelSize: root.dictaTheme.baseFontSize + 4
                            font.weight: Font.DemiBold
                            elide: Text.ElideRight
                        }
                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 7 * root.dictaTheme.spacingScale
                            FlatButton {
                                objectName: "projectBranchBadge"
                                visible: Boolean(root.bridge.currentProject.branch)
                                dictaTheme: root.dictaTheme
                                text: root.bridge.currentProject.branch || ""
                                iconName: "branch"
                                selected: true
                                toolTip: "Copy Git branch"
                                onClicked: root.bridge.copyText(root.bridge.currentProject.branch || "")
                            }
                            Item { Layout.fillWidth: true }
                        }
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
                    onContextCopied: root.bridge.showToast("Recording ID copied")
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

    Rectangle {
        id: settingsPage
        visible: root.settingsOpen
        anchors.fill: parent
        anchors.leftMargin: root.railLayout
            ? 255 * root.dictaTheme.spacingScale : 0
        z: 30
        color: root.dictaTheme.background
        focus: visible
        onVisibleChanged: {
            if (visible)
                forceActiveFocus()
        }
        Keys.onPressed: event => {
            if (event.key === Qt.Key_Escape || event.key === Qt.Key_Left) {
                root.settingsOpen = false
                event.accepted = true
            }
        }

        Rectangle {
            id: settingsNavigation
            anchors.left: parent.left
            anchors.top: parent.top
            anchors.bottom: parent.bottom
            width: Math.min(230 * root.dictaTheme.spacingScale,
                Math.max(184 * root.dictaTheme.spacingScale, parent.width * 0.24))
            color: root.dictaTheme.darkBackground

            ColumnLayout {
                anchors.fill: parent
                anchors.margins: 14 * root.dictaTheme.spacingScale
                spacing: 7 * root.dictaTheme.spacingScale

                FlatButton {
                    objectName: "settingsBack"
                    Layout.fillWidth: true
                    Layout.preferredHeight: 44 * root.dictaTheme.spacingScale
                    dictaTheme: root.dictaTheme
                    text: "Settings"
                    iconName: "back"
                    quiet: true
                    onClicked: root.settingsOpen = false
                }

                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 1
                    Layout.topMargin: 4 * root.dictaTheme.spacingScale
                    Layout.bottomMargin: 8 * root.dictaTheme.spacingScale
                    color: Qt.rgba(root.dictaTheme.muted.r, root.dictaTheme.muted.g,
                        root.dictaTheme.muted.b, 0.55)
                }

                Repeater {
                    model: [
                        {id: "appearance", label: "Appearance", icon: "appearance"},
                        {id: "connections", label: "Connections", icon: "connections"},
                        {id: "shortcuts", label: "Shortcuts", icon: "shortcuts"},
                        {id: "transcription", label: "Transcription", icon: "transcription"},
                        {id: "storage", label: "Storage", icon: "storage"}
                    ]
                    FlatButton {
                        required property var modelData
                        objectName: "settings-section-" + modelData.id
                        Layout.fillWidth: true
                        Layout.preferredHeight: 46 * root.dictaTheme.spacingScale
                        dictaTheme: root.dictaTheme
                        text: modelData.label
                        iconName: modelData.icon
                        quiet: true
                        selected: root.settingsSection === modelData.id
                        onClicked: root.settingsSection = modelData.id
                    }
                }

                Item { Layout.fillHeight: true }
            }

            Rectangle {
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.bottom: parent.bottom
                width: 1
                color: Qt.rgba(root.dictaTheme.muted.r, root.dictaTheme.muted.g,
                    root.dictaTheme.muted.b, 0.55)
            }
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.leftMargin: settingsNavigation.width
                + 34 * root.dictaTheme.spacingScale
            anchors.rightMargin: 34 * root.dictaTheme.spacingScale
            anchors.topMargin: 30 * root.dictaTheme.spacingScale
            anchors.bottomMargin: 24 * root.dictaTheme.spacingScale
            spacing: 10 * root.dictaTheme.spacingScale

            RowLayout {
                Layout.fillWidth: true
                FlatButton {
                    visible: false
                    dictaTheme: root.dictaTheme
                    iconName: "back"
                    iconOnly: true
                    quiet: true
                    toolTip: "Back to Dicta"
                    onClicked: root.settingsOpen = false
                }
                Text {
                    Layout.fillWidth: true
                    text: root.settingsTitle(root.settingsSection)
                    color: root.dictaTheme.brightForeground
                    font.family: root.dictaTheme.fontFamily
                    font.pixelSize: root.dictaTheme.baseFontSize + 5
                    font.weight: Font.DemiBold
                }
            }

            Text {
                Layout.fillWidth: true
                text: root.settingsDescription(root.settingsSection)
                color: root.dictaTheme.darkForeground
                font.family: root.dictaTheme.fontFamily
                font.pixelSize: root.dictaTheme.baseFontSize
                wrapMode: Text.Wrap
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
                    columns: 1
                    columnSpacing: 18 * root.dictaTheme.spacingScale
                    rowSpacing: 18 * root.dictaTheme.spacingScale

                    Rectangle {
                        objectName: "settingsAppearanceCard"
                        visible: root.settingsSection === "appearance"
                        Layout.fillWidth: true
                        Layout.preferredHeight: 168 * root.dictaTheme.spacingScale
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
                        objectName: "settingsTranscriptionCard"
                        visible: root.settingsSection === "transcription"
                        Layout.fillWidth: true
                        Layout.preferredHeight: 345 * root.dictaTheme.spacingScale
                        radius: 3 * root.dictaTheme.spacingScale
                        color: root.dictaTheme.darkBackground
                        border.width: 1
                        border.color: root.dictaTheme.muted
                        ColumnLayout {
                            anchors.fill: parent
                            anchors.margins: 16 * root.dictaTheme.spacingScale
                            spacing: 8 * root.dictaTheme.spacingScale
                            Text {
                                text: "MODELS"
                                color: root.dictaTheme.darkForeground
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
                                font.weight: Font.DemiBold
                            }
                            Rectangle {
                                Layout.fillWidth: true
                                Layout.preferredHeight: 66 * root.dictaTheme.spacingScale
                                radius: 3 * root.dictaTheme.spacingScale
                                color: root.dictaTheme.background
                                border.width: 1
                                border.color: root.dictaTheme.muted
                                RowLayout {
                                    anchors.fill: parent
                                    anchors.margins: 12 * root.dictaTheme.spacingScale
                                    spacing: 10 * root.dictaTheme.spacingScale
                                    ThemeIcon {
                                        Layout.preferredWidth: 22 * root.dictaTheme.spacingScale
                                        Layout.preferredHeight: width
                                        iconName: "transcription"
                                        iconColor: root.dictaTheme.darkForeground
                                        iconSize: Math.round(width)
                                    }
                                    ColumnLayout {
                                        Layout.fillWidth: true
                                        spacing: 1
                                        Text {
                                            text: "Compact  ·  Included"
                                            color: root.dictaTheme.brightForeground
                                            font.family: root.dictaTheme.fontFamily
                                            font.pixelSize: root.dictaTheme.baseFontSize
                                            font.weight: Font.DemiBold
                                        }
                                        Text {
                                            Layout.fillWidth: true
                                            text: "Fast offline fallback for rough transcripts."
                                            color: root.dictaTheme.darkForeground
                                            font.family: root.dictaTheme.fontFamily
                                            font.pixelSize: Math.max(9,
                                                root.dictaTheme.baseFontSize - 1)
                                            elide: Text.ElideRight
                                        }
                                    }
                                    Text {
                                        text: "57 MB"
                                        color: root.dictaTheme.darkForeground
                                        font.family: root.dictaTheme.fontFamily
                                        font.pixelSize: Math.max(9,
                                            root.dictaTheme.baseFontSize - 1)
                                    }
                                }
                            }
                            Text {
                                text: "CURRENT ENGINE"
                                color: root.dictaTheme.darkForeground
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
                                font.weight: Font.DemiBold
                            }
                            Rectangle {
                                Layout.fillWidth: true
                                Layout.preferredHeight: 66 * root.dictaTheme.spacingScale
                                radius: 3 * root.dictaTheme.spacingScale
                                color: root.dictaTheme.background
                                border.width: 1
                                border.color: root.dictaTheme.muted
                                RowLayout {
                                    anchors.fill: parent
                                    anchors.margins: 12 * root.dictaTheme.spacingScale
                                    spacing: 10 * root.dictaTheme.spacingScale
                                    Rectangle {
                                        Layout.preferredWidth: 9 * root.dictaTheme.spacingScale
                                        Layout.preferredHeight: width
                                        radius: width / 2
                                        color: root.bridge.modelStatus.quality_state === "ready"
                                            ? root.dictaTheme.green : root.dictaTheme.yellow
                                    }
                                    ColumnLayout {
                                        Layout.fillWidth: true
                                        spacing: 1
                                        Text {
                                            Layout.fillWidth: true
                                            text: root.bridge.modelStatus.active_model
                                                || "Compact fallback"
                                            color: root.dictaTheme.brightForeground
                                            font.family: root.dictaTheme.fontFamily
                                            font.pixelSize: root.dictaTheme.baseFontSize
                                            font.weight: Font.DemiBold
                                            elide: Text.ElideRight
                                        }
                                        Text {
                                            Layout.fillWidth: true
                                            text: root.bridge.modelStatus.message || "Local Whisper"
                                            color: root.dictaTheme.darkForeground
                                            font.family: root.dictaTheme.fontFamily
                                            font.pixelSize: Math.max(9,
                                                root.dictaTheme.baseFontSize - 1)
                                            elide: Text.ElideMiddle
                                        }
                                    }
                                    FlatButton {
                                        objectName: "installQualityModel"
                                        dictaTheme: root.dictaTheme
                                        text: root.bridge.modelStatus.quality_state === "ready"
                                            ? "ACTIVE"
                                            : root.bridge.modelStatus.quality_state === "installing"
                                                ? "INSTALLING…" : "INSTALL QUALITY"
                                        selected: root.bridge.modelStatus.quality_state === "ready"
                                        enabled: root.bridge.hostState === "running"
                                            && root.bridge.runtimePhase === "idle"
                                            && root.bridge.modelStatus.quality_state !== "ready"
                                            && root.bridge.modelStatus.quality_state !== "installing"
                                        onClicked: root.installQualityModel()
                                    }
                                }
                            }
                            Text {
                                text: "TRANSCRIPT LANGUAGE"
                                color: root.dictaTheme.darkForeground
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
                                font.weight: Font.DemiBold
                            }
                            Flow {
                                Layout.fillWidth: true
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
                            RowLayout {
                                Layout.fillWidth: true
                                Text {
                                    text: root.bridge.modelStatus.install_stage
                                        ? root.bridge.modelStatus.install_stage + " · "
                                            + Math.round(Number(
                                                root.bridge.modelStatus.downloaded_bytes || 0)
                                                / 1048576) + " MiB"
                                        : "Models are stored locally and work offline."
                                    color: root.dictaTheme.darkForeground
                                    font.family: root.dictaTheme.fontFamily
                                    font.pixelSize: Math.max(9,
                                        root.dictaTheme.baseFontSize - 1)
                                    elide: Text.ElideRight
                                }
                            }
                        }
                    }

                    Rectangle {
                        objectName: "settingsStorageCard"
                        visible: root.settingsSection === "storage"
                        Layout.fillWidth: true
                        Layout.preferredHeight: 310 * root.dictaTheme.spacingScale
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
                                    objectName: "generalPathBrowse"
                                    dictaTheme: root.dictaTheme
                                    text: "BROWSE"
                                    onClicked: {
                                        var candidate = generalPathField.text.trim()
                                        if (!candidate.length)
                                            candidate = root.bridge.currentProject.path || "/"
                                        generalFolderDialog.currentFolder =
                                            "file://" + encodeURI(candidate)
                                        generalFolderDialog.open()
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
                        objectName: "settingsConnectionsCard"
                        visible: root.settingsSection === "connections"
                        Layout.fillWidth: true
                        Layout.preferredHeight: 150 * root.dictaTheme.spacingScale
                        radius: 3 * root.dictaTheme.spacingScale
                        color: root.dictaTheme.darkBackground
                        border.width: 1
                        border.color: root.dictaTheme.muted
                        ColumnLayout {
                            anchors.fill: parent
                            anchors.margins: 20 * root.dictaTheme.spacingScale
                            spacing: 10 * root.dictaTheme.spacingScale
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 12 * root.dictaTheme.spacingScale

                                Rectangle {
                                    Layout.preferredWidth: 42 * root.dictaTheme.spacingScale
                                    Layout.preferredHeight: width
                                    radius: 7 * root.dictaTheme.spacingScale
                                    color: Qt.rgba(root.dictaTheme.accent.r,
                                        root.dictaTheme.accent.g,
                                        root.dictaTheme.accent.b, 0.11)
                                    ThemeIcon {
                                        anchors.centerIn: parent
                                        width: 21 * root.dictaTheme.spacingScale
                                        height: width
                                        iconName: "connections"
                                        iconColor: root.dictaTheme.accent
                                        iconSize: Math.round(width)
                                    }
                                }

                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 2 * root.dictaTheme.spacingScale
                                    Text {
                                        Layout.fillWidth: true
                                        text: "Codex"
                                        color: root.dictaTheme.brightForeground
                                        font.family: root.dictaTheme.fontFamily
                                        font.pixelSize: root.dictaTheme.baseFontSize + 2
                                        font.weight: Font.DemiBold
                                    }
                                    Text {
                                        Layout.fillWidth: true
                                        text: root.bridge.codexMcp.state === "connected"
                                            ? "Connected" : "Not connected"
                                        color: root.bridge.codexMcp.state === "connected"
                                            ? root.dictaTheme.green
                                            : root.dictaTheme.darkForeground
                                        font.family: root.dictaTheme.fontFamily
                                        font.pixelSize: Math.max(9,
                                            root.dictaTheme.baseFontSize - 1)
                                    }
                                }

                                FlatButton {
                                    objectName: "mcpActionButton"
                                    dictaTheme: root.dictaTheme
                                    text: root.bridge.codexMcp.state === "disconnected"
                                        ? "CONNECT"
                                        : root.bridge.codexMcp.state === "connected"
                                            ? "RECONNECT" : "REPLACE"
                                    selected: root.bridge.codexMcp.state === "connected"
                                    enabled: Boolean(root.bridge.codexMcp.codex_path)
                                        && Boolean(root.bridge.codexMcp.mcp_path)
                                    onClicked: {
                                        if (root.bridge.codexMcp.state === "disconnected")
                                            root.bridge.connectCodexMcp()
                                        else
                                            root.bridge.restartCodexMcp()
                                    }
                                }
                            }
                            Text {
                                Layout.fillWidth: true
                                visible: root.bridge.codexMcp.state !== "connected"
                                text: root.bridge.codexMcp.message || "Checking Codex…"
                                color: root.bridge.codexMcp.state === "connected"
                                    ? root.dictaTheme.accent : root.dictaTheme.darkForeground
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
                                wrapMode: Text.Wrap
                            }
                            Item { Layout.fillHeight: true }
                        }
                    }

                    Rectangle {
                        objectName: "settingsShortcutsCard"
                        visible: root.settingsSection === "shortcuts"
                        Layout.fillWidth: true
                        Layout.preferredHeight: 215 * root.dictaTheme.spacingScale
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

    FolderDialog {
        id: generalFolderDialog
        title: "Choose the General recordings folder"
        onAccepted: {
            var path = root.pathFromFolderUrl(selectedFolder)
            generalPathField.text = path
            root.updateGeneralPath(path)
        }
    }
}
