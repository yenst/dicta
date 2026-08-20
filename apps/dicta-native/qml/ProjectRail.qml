pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: root

    required property QtObject bridge
    required property QtObject dictaTheme
    signal settingsRequested()
    signal addProjectRequested()

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
                Layout.preferredWidth: 22 * root.dictaTheme.spacingScale
                Layout.preferredHeight: 30 * root.dictaTheme.spacingScale
                source: root.dictaTheme.mode === "light"
                    ? "qrc:/dicta/assets/dicta-mark.png"
                    : "qrc:/dicta/assets/dicta-mark-light.png"
                fillMode: Image.PreserveAspectFit
                smooth: true
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
            model: root.bridge.projects || []

            delegate: Item {
                id: projectRow
                required property var modelData
                required property int index
                width: projectList.width
                height: 68 * root.dictaTheme.spacingScale
                property bool selected: Boolean(modelData.selected)

                Rectangle {
                    anchors.fill: parent
                    color: projectRow.selected
                        ? Qt.rgba(root.dictaTheme.accent.r, root.dictaTheme.accent.g,
                            root.dictaTheme.accent.b, 0.12)
                        : projectMouse.containsMouse
                            ? Qt.rgba(root.dictaTheme.foreground.r, root.dictaTheme.foreground.g,
                                root.dictaTheme.foreground.b, 0.045)
                            : "transparent"
                }
                Rectangle {
                    visible: projectRow.selected
                    anchors.left: parent.left
                    anchors.top: parent.top
                    anchors.bottom: parent.bottom
                    width: 2
                    color: root.dictaTheme.accent
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
                    anchors.rightMargin: 16 * root.dictaTheme.spacingScale
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
                        width: parent.width
                        text: projectRow.modelData.id === "general"
                            ? "all recordings"
                            : root.bridge.currentProject.id === projectRow.modelData.id
                                ? (root.bridge.recentRecordings.length > 0
                                    && root.bridge.recentRecordings[0].branch
                                    ? root.bridge.recentRecordings[0].branch : "repository")
                                : (projectRow.modelData.path || "standalone")
                        color: root.dictaTheme.darkForeground
                        font.family: root.dictaTheme.fontFamily
                        font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
                        elide: Text.ElideMiddle
                    }
                }

                MouseArea {
                    id: projectMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.bridge.selectProject(projectRow.modelData.id)
                }
            }
        }

        FlatButton {
            Layout.fillWidth: true
            Layout.leftMargin: 12 * root.dictaTheme.spacingScale
            Layout.rightMargin: 12 * root.dictaTheme.spacingScale
            Layout.bottomMargin: 10 * root.dictaTheme.spacingScale
            dictaTheme: root.dictaTheme
            text: "New project"
            iconName: "add"
            quiet: true
            onClicked: root.addProjectRequested()
        }

        Item { Layout.fillHeight: true }

        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: 22 * root.dictaTheme.spacingScale
            Layout.rightMargin: 18 * root.dictaTheme.spacingScale
            Layout.topMargin: 12 * root.dictaTheme.spacingScale
            spacing: 8 * root.dictaTheme.spacingScale
            Rectangle {
                Layout.preferredWidth: 8 * root.dictaTheme.spacingScale
                Layout.preferredHeight: width
                radius: width / 2
                color: root.bridge.modelStatus.quality_state === "ready"
                    || Boolean(root.bridge.modelStatus.active_model)
                    ? root.dictaTheme.green : root.dictaTheme.yellow
            }
            Text {
                Layout.fillWidth: true
                text: (root.bridge.modelStatus.active_model || "model unavailable")
                    + (root.bridge.modelStatus.active_model ? " · ready" : "")
                color: root.dictaTheme.darkForeground
                font.family: root.dictaTheme.fontFamily
                font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
                elide: Text.ElideRight
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 1
            Layout.leftMargin: 16 * root.dictaTheme.spacingScale
            Layout.rightMargin: 16 * root.dictaTheme.spacingScale
            color: Qt.rgba(root.dictaTheme.muted.r, root.dictaTheme.muted.g,
                root.dictaTheme.muted.b, 0.55)
        }

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
            onClicked: root.settingsRequested()
        }
    }
}
