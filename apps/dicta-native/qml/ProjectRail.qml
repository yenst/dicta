pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: root

    required property QtObject bridge
    required property QtObject dictaTheme
    property bool settingsActive: false
    property bool keyboardFocused: false
    property int keyboardIndex: 0
    signal settingsRequested()
    signal addProjectRequested()
    signal keyboardFocusRequested()

    function isGeneral(project) {
        return String(project && project.id || "") === "__unprojected__"
            || String(project && project.id || "") === "general"
    }

    function sortedProjects() {
        var rows = (bridge.projects || []).slice()
        rows.sort(function(left, right) {
            var leftGeneral = root.isGeneral(left)
            var rightGeneral = root.isGeneral(right)
            if (leftGeneral !== rightGeneral)
                return leftGeneral ? -1 : 1
            return String(left.name || left.id || "").localeCompare(
                String(right.name || right.id || ""))
        })
        return rows
    }

    function syncKeyboardIndex() {
        var rows = sortedProjects()
        for (var i = 0; i < rows.length; ++i) {
            if (rows[i] && rows[i].selected) {
                keyboardIndex = i
                return
            }
        }
        keyboardIndex = Math.max(0, Math.min(keyboardIndex, rows.length - 1))
    }

    function moveKeyboardSelection(delta) {
        var rows = sortedProjects()
        if (!rows.length)
            return
        keyboardIndex = Math.max(0, Math.min(rows.length - 1, keyboardIndex + delta))
        projectList.positionViewAtIndex(keyboardIndex, ListView.Contain)
    }

    function activateKeyboardSelection() {
        var rows = sortedProjects()
        if (keyboardIndex >= 0 && keyboardIndex < rows.length)
            bridge.selectProject(rows[keyboardIndex].id)
    }

    function activateRecordingSelection() {
        var rows = sortedProjects()
        if (keyboardIndex < 0 || keyboardIndex >= rows.length)
            return false
        return bridge.selectRecordingProject(rows[keyboardIndex].id)
    }

    onKeyboardFocusedChanged: if (keyboardFocused) syncKeyboardIndex()

    Connections {
        target: root.bridge
        function onDashboardChanged() {
            if (!root.keyboardFocused)
                root.syncKeyboardIndex()
        }
    }

    Rectangle {
        anchors.fill: parent
        color: root.dictaTheme.darkBackground
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        RowLayout {
            Layout.fillWidth: true
            Layout.preferredHeight: 84 * root.dictaTheme.spacingScale
            Layout.leftMargin: 22 * root.dictaTheme.spacingScale
            Layout.rightMargin: 18 * root.dictaTheme.spacingScale
            spacing: 11 * root.dictaTheme.spacingScale

            Image {
                Layout.preferredWidth: 26 * root.dictaTheme.spacingScale
                Layout.preferredHeight: 26 * root.dictaTheme.spacingScale
                source: root.dictaTheme.mode === "light"
                    ? "qrc:/dicta/assets/dicta-mark.png"
                    : "qrc:/dicta/assets/dicta-mark-light.png"
                fillMode: Image.PreserveAspectFit
                smooth: true
                mipmap: true
                sourceSize.width: Math.round(width * Screen.devicePixelRatio)
                sourceSize.height: Math.round(height * Screen.devicePixelRatio)
            }

            Text {
                Layout.fillWidth: true
                text: "Dicta"
                color: root.dictaTheme.brightForeground
                font.family: root.dictaTheme.fontFamily
                font.pixelSize: root.dictaTheme.baseFontSize + 5
                font.weight: Font.DemiBold
            }
        }

        Text {
            Layout.fillWidth: true
            Layout.leftMargin: 22 * root.dictaTheme.spacingScale
            Layout.rightMargin: 18 * root.dictaTheme.spacingScale
            Layout.topMargin: 8 * root.dictaTheme.spacingScale
            Layout.bottomMargin: 10 * root.dictaTheme.spacingScale
            text: "PROJECTS"
            color: root.dictaTheme.darkForeground
            font.family: root.dictaTheme.fontFamily
            font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 2)
            font.weight: Font.DemiBold
            font.letterSpacing: 1.1
        }

        ListView {
            id: projectList
            objectName: "projectList"
            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(contentHeight,
                420 * root.dictaTheme.spacingScale)
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            model: root.sortedProjects()

            delegate: Item {
                id: projectRow
                required property var modelData
                required property int index
                width: projectList.width
                height: 68 * root.dictaTheme.spacingScale
                property bool selected: Boolean(modelData.selected)
                property bool recordingSelected: Boolean(modelData.recordingSelected)
                property bool keyboardSelected: root.keyboardFocused
                    && root.keyboardIndex === index

                Rectangle {
                    anchors.fill: parent
                    color: projectRow.selected
                        ? Qt.rgba(root.dictaTheme.accent.r, root.dictaTheme.accent.g,
                            root.dictaTheme.accent.b, 0.12)
                        : projectRow.keyboardSelected
                            ? Qt.rgba(root.dictaTheme.foreground.r,
                                root.dictaTheme.foreground.g,
                                root.dictaTheme.foreground.b, 0.075)
                        : projectMouse.containsMouse
                            ? Qt.rgba(root.dictaTheme.foreground.r, root.dictaTheme.foreground.g,
                                root.dictaTheme.foreground.b, 0.045)
                            : "transparent"
                }
                Rectangle {
                    visible: projectRow.selected || projectRow.keyboardSelected
                    anchors.left: parent.left
                    anchors.top: parent.top
                    anchors.bottom: parent.bottom
                    width: 2
                    color: projectRow.selected
                        ? root.dictaTheme.accent : root.dictaTheme.foreground
                }

                ThemeIcon {
                    anchors.left: parent.left
                    anchors.leftMargin: 22 * root.dictaTheme.spacingScale
                    anchors.verticalCenter: parent.verticalCenter
                    width: 18 * root.dictaTheme.spacingScale
                    height: width
                    iconName: projectRow.selected ? "folder-open" : "folder"
                    iconColor: projectRow.selected
                        ? root.dictaTheme.accent : root.dictaTheme.darkForeground
                    iconSize: Math.round(width)
                }

                Column {
                    anchors.left: parent.left
                    anchors.leftMargin: 56 * root.dictaTheme.spacingScale
                    anchors.right: parent.right
                    anchors.rightMargin: 76 * root.dictaTheme.spacingScale
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 3 * root.dictaTheme.spacingScale

                    Text {
                        width: parent.width
                        text: projectRow.modelData.name || projectRow.modelData.id
                        color: projectRow.selected
                            ? root.dictaTheme.accent : root.dictaTheme.foreground
                        font.family: root.dictaTheme.fontFamily
                        font.pixelSize: root.dictaTheme.baseFontSize + 1
                        font.weight: projectRow.selected ? Font.DemiBold : Font.Normal
                        elide: Text.ElideRight
                    }
                    Text {
                        visible: !root.isGeneral(projectRow.modelData)
                        width: parent.width
                        text: projectRow.modelData.branch || ""
                        color: root.dictaTheme.darkForeground
                        font.family: root.dictaTheme.fontFamily
                        font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
                        elide: Text.ElideMiddle
                    }
                }

                ThemeIcon {
                    objectName: "recordingDestinationCheck"
                    visible: projectRow.recordingSelected
                    anchors.right: parent.right
                    anchors.rightMargin: 43 * root.dictaTheme.spacingScale
                    anchors.verticalCenter: parent.verticalCenter
                    width: 17 * root.dictaTheme.spacingScale
                    height: width
                    iconName: "check"
                    iconColor: root.dictaTheme.accent
                    iconSize: Math.round(width)
                }

                FlatButton {
                    id: projectActionsButton
                    objectName: "projectActions-" + projectRow.modelData.id
                    visible: !root.isGeneral(projectRow.modelData)
                        && (projectMouse.containsMouse || hovered || projectActions.opened)
                    anchors.right: parent.right
                    anchors.rightMargin: 7 * root.dictaTheme.spacingScale
                    anchors.verticalCenter: parent.verticalCenter
                    width: 30 * root.dictaTheme.spacingScale
                    height: width
                    z: 2
                    dictaTheme: root.dictaTheme
                    iconName: "more"
                    iconOnly: true
                    quiet: true
                    toolTip: "Project actions"
                    onClicked: projectActions.open()
                }

                Popup {
                    id: projectActions
                    x: projectRow.width - width - 8 * root.dictaTheme.spacingScale
                    y: projectRow.height - 4 * root.dictaTheme.spacingScale
                    z: 10
                    width: 174 * root.dictaTheme.spacingScale
                    padding: 6 * root.dictaTheme.spacingScale
                    modal: false
                    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
                    background: Rectangle {
                        color: root.dictaTheme.darkBackground
                        border.width: 1
                        border.color: root.dictaTheme.muted
                        radius: 4 * root.dictaTheme.spacingScale
                    }
                    contentItem: FlatButton {
                        objectName: "removeProject-" + projectRow.modelData.id
                        dictaTheme: root.dictaTheme
                        text: "Remove project"
                        iconName: "clear"
                        quiet: true
                        destructive: true
                        enabled: root.bridge.runtimePhase === "idle"
                        onClicked: {
                            projectActions.close()
                            if (root.bridge.removeProject(projectRow.modelData.id))
                                root.bridge.showToast("Project removed")
                        }
                    }
                }

                MouseArea {
                    id: projectMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: {
                        root.bridge.selectProject(projectRow.modelData.id)
                        root.keyboardFocusRequested()
                    }
                    onDoubleClicked: {
                        if (root.bridge.selectRecordingProject(projectRow.modelData.id))
                            root.bridge.showToast("Recording destination · "
                                + (projectRow.modelData.name || projectRow.modelData.id))
                        root.keyboardFocusRequested()
                    }
                }
            }
        }

        FlatButton {
            Layout.fillWidth: true
            Layout.preferredHeight: 40 * root.dictaTheme.spacingScale
            Layout.leftMargin: 12 * root.dictaTheme.spacingScale
            Layout.rightMargin: 12 * root.dictaTheme.spacingScale
            Layout.topMargin: 8 * root.dictaTheme.spacingScale
            Layout.bottomMargin: 10 * root.dictaTheme.spacingScale
            dictaTheme: root.dictaTheme
            text: "New project"
            iconName: "add"
            quiet: true
            onClicked: root.addProjectRequested()
        }

        Item { Layout.fillHeight: true }

        FlatButton {
            Layout.fillWidth: true
            Layout.leftMargin: 12 * root.dictaTheme.spacingScale
            Layout.rightMargin: 12 * root.dictaTheme.spacingScale
            Layout.topMargin: 5 * root.dictaTheme.spacingScale
            Layout.bottomMargin: 12 * root.dictaTheme.spacingScale
            dictaTheme: root.dictaTheme
            text: "Settings"
            iconName: "settings"
            quiet: true
            selected: root.settingsActive
            onClicked: root.settingsRequested()
        }
    }

    Item {
        objectName: "projectKeyboardBorder"
        anchors.fill: parent
        z: 20
        visible: root.keyboardFocused

        Rectangle {
            anchors.top: parent.top
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            width: 2
            color: root.dictaTheme.accent
        }
    }
}
