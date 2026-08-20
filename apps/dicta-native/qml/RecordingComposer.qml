pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: root

    required property QtObject bridge
    required property QtObject dictaTheme
    property alias noteText: sessionNote.text
    property bool recording: bridge.runtimePhase === "recording"
        || bridge.runtimePhase === "annotating"
    property bool transitionBusy: bridge.runtimePhase === "preparing"
        || bridge.runtimePhase === "stopping"
    implicitHeight: annotationControls.visible
        ? 112 * dictaTheme.spacingScale : 74 * dictaTheme.spacingScale

    Rectangle {
        anchors.fill: parent
        anchors.margins: 1
        radius: 4 * root.dictaTheme.spacingScale
        color: Qt.rgba(root.dictaTheme.background.r, root.dictaTheme.background.g,
            root.dictaTheme.background.b, 0.72)
        border.width: 1
        border.color: sessionNote.activeFocus
            ? Qt.rgba(root.dictaTheme.accent.r, root.dictaTheme.accent.g,
                root.dictaTheme.accent.b, 0.75)
            : Qt.rgba(root.dictaTheme.muted.r, root.dictaTheme.muted.g,
                root.dictaTheme.muted.b, 0.75)
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.leftMargin: 18 * root.dictaTheme.spacingScale
        anchors.rightMargin: 10 * root.dictaTheme.spacingScale
        anchors.topMargin: 9 * root.dictaTheme.spacingScale
        anchors.bottomMargin: 9 * root.dictaTheme.spacingScale
        spacing: 4 * root.dictaTheme.spacingScale

        RowLayout {
            Layout.fillWidth: true
            Layout.fillHeight: !annotationControls.visible
            spacing: 12 * root.dictaTheme.spacingScale

            Image {
                Layout.preferredWidth: 19 * root.dictaTheme.spacingScale
                Layout.preferredHeight: 26 * root.dictaTheme.spacingScale
                source: root.dictaTheme.mode === "light"
                    ? "qrc:/dicta/assets/dicta-mark.png"
                    : "qrc:/dicta/assets/dicta-mark-light.png"
                fillMode: Image.PreserveAspectFit
                smooth: true
            }

            ColumnLayout {
                Layout.fillWidth: true
                Layout.alignment: Qt.AlignVCenter
                spacing: 1 * root.dictaTheme.spacingScale

                TextField {
                    id: sessionNote
                    objectName: "sessionNote"
                    Layout.fillWidth: true
                    enabled: root.bridge.hostState === "running"
                        && !root.recording && !root.transitionBusy
                    placeholderText: "What should this recording explain?"
                    color: root.dictaTheme.foreground
                    placeholderTextColor: root.dictaTheme.darkForeground
                    selectionColor: root.dictaTheme.selection
                    selectedTextColor: root.dictaTheme.brightForeground
                    font.family: root.dictaTheme.fontFamily
                    font.pixelSize: root.dictaTheme.baseFontSize + 1
                    leftPadding: 0
                    rightPadding: 0
                    topPadding: 0
                    bottomPadding: 0
                    background: Item {}
                    Keys.onReturnPressed: {
                        if (!root.recording && !root.transitionBusy)
                            root.bridge.startRecording(text)
                    }
                }

                Text {
                    Layout.fillWidth: true
                    text: root.recording
                        ? (root.bridge.activeRecordingId || "recording")
                            + " · screen + desktop audio + microphone"
                        : "screen + desktop audio + microphone"
                    color: root.dictaTheme.darkForeground
                    font.family: root.dictaTheme.fontFamily
                    font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
                    elide: Text.ElideRight
                }
            }

            Rectangle {
                Layout.preferredWidth: 1
                Layout.preferredHeight: 38 * root.dictaTheme.spacingScale
                color: Qt.rgba(root.dictaTheme.muted.r, root.dictaTheme.muted.g,
                    root.dictaTheme.muted.b, 0.65)
            }

            Rectangle {
                visible: root.width >= 620 * root.dictaTheme.spacingScale
                Layout.preferredWidth: 106 * root.dictaTheme.spacingScale
                Layout.preferredHeight: 40 * root.dictaTheme.spacingScale
                radius: 3 * root.dictaTheme.spacingScale
                color: "transparent"
                border.width: 1
                border.color: Qt.rgba(root.dictaTheme.muted.r,
                    root.dictaTheme.muted.g, root.dictaTheme.muted.b, 0.75)
                Text {
                    anchors.centerIn: parent
                    text: "Ctrl Space"
                    color: root.dictaTheme.darkForeground
                    font.family: root.dictaTheme.fontFamily
                    font.pixelSize: Math.max(10, root.dictaTheme.baseFontSize - 1)
                }
            }

            Button {
                id: recordButton
                objectName: "recordToggle"
                Layout.preferredWidth: 118 * root.dictaTheme.spacingScale
                Layout.fillHeight: true
                enabled: root.bridge.hostState === "running" && !root.transitionBusy
                    && (root.recording || root.bridge.runtimePhase === "idle")
                hoverEnabled: true
                text: root.recording ? "Stop" : "Record"
                onClicked: root.recording
                    ? root.bridge.stopRecording()
                    : root.bridge.startRecording(sessionNote.text)

                contentItem: RowLayout {
                    spacing: 8 * root.dictaTheme.spacingScale
                    ThemeIcon {
                        Layout.preferredWidth: 17 * root.dictaTheme.spacingScale
                        Layout.preferredHeight: width
                        iconName: "record"
                        iconColor: recordButton.enabled
                            ? root.dictaTheme.red : root.dictaTheme.darkForeground
                        iconSize: Math.round(width)
                    }
                    Text {
                        Layout.fillWidth: true
                        text: recordButton.text
                        color: recordButton.enabled
                            ? root.dictaTheme.red : root.dictaTheme.darkForeground
                        font.family: root.dictaTheme.fontFamily
                        font.pixelSize: root.dictaTheme.baseFontSize + 1
                        font.weight: Font.DemiBold
                        horizontalAlignment: Text.AlignHCenter
                        verticalAlignment: Text.AlignVCenter
                    }
                }
                background: Rectangle {
                    radius: 3 * root.dictaTheme.spacingScale
                    color: recordButton.down
                        ? Qt.rgba(root.dictaTheme.red.r, root.dictaTheme.red.g,
                            root.dictaTheme.red.b, 0.16)
                        : recordButton.hovered
                            ? Qt.rgba(root.dictaTheme.red.r, root.dictaTheme.red.g,
                                root.dictaTheme.red.b, 0.08)
                            : "transparent"
                    border.width: recordButton.activeFocus ? 1 : 0
                    border.color: root.dictaTheme.red
                }
            }
        }

        RowLayout {
            id: annotationControls
            objectName: "annotationControls"
            visible: root.recording
            Layout.fillWidth: true
            Layout.preferredHeight: 31 * root.dictaTheme.spacingScale
            spacing: 4 * root.dictaTheme.spacingScale

            FlatButton {
                id: annotationToggle
                objectName: "annotationToggle"
                dictaTheme: root.dictaTheme
                text: root.bridge.annotationsEnabled ? "Annotations on" : "Annotations off"
                selected: root.bridge.annotationsEnabled
                quiet: true
                onClicked: root.bridge.setAnnotationsEnabled(!root.bridge.annotationsEnabled)
            }
            FlatButton {
                objectName: "annotationToolPen"
                dictaTheme: root.dictaTheme
                text: "Pen"
                enabled: root.bridge.annotationsEnabled
                selected: root.bridge.annotationTool === "pen"
                quiet: true
                onClicked: root.bridge.chooseAnnotationTool("pen")
            }
            FlatButton {
                objectName: "annotationToolArrow"
                dictaTheme: root.dictaTheme
                text: "Arrow"
                enabled: root.bridge.annotationsEnabled
                selected: root.bridge.annotationTool === "arrow"
                quiet: true
                onClicked: root.bridge.chooseAnnotationTool("arrow")
            }
            FlatButton {
                objectName: "annotationToolRectangle"
                dictaTheme: root.dictaTheme
                text: "Box"
                enabled: root.bridge.annotationsEnabled
                selected: root.bridge.annotationTool === "rectangle"
                quiet: true
                onClicked: root.bridge.chooseAnnotationTool("rectangle")
            }
            FlatButton {
                objectName: "annotationToolSpotlight"
                dictaTheme: root.dictaTheme
                text: "Spot"
                enabled: root.bridge.annotationsEnabled
                selected: root.bridge.annotationTool === "spotlight"
                quiet: true
                onClicked: root.bridge.chooseAnnotationTool("spotlight")
            }
            Item { Layout.fillWidth: true }
            FlatButton {
                objectName: "annotationUndo"
                dictaTheme: root.dictaTheme
                iconName: "undo"
                iconOnly: true
                toolTip: "Undo annotation"
                enabled: root.bridge.annotationsEnabled
                quiet: true
                onClicked: root.bridge.undoAnnotation()
            }
            FlatButton {
                objectName: "annotationClear"
                dictaTheme: root.dictaTheme
                iconName: "clear"
                iconOnly: true
                toolTip: "Clear annotations"
                enabled: root.bridge.annotationsEnabled
                quiet: true
                onClicked: root.bridge.clearAnnotations()
            }
        }
    }
}
