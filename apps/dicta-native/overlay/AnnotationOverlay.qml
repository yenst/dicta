import QtQuick
import QtQuick.Window
import Dicta.Native

Window {
    id: overlay

    objectName: "dictaAnnotationOverlay"
    title: "Dicta Annotation Overlay"
    color: "transparent"
    visible: false
    flags: Qt.FramelessWindowHint
        | Qt.Tool
        | Qt.BypassWindowManagerHint
        | Qt.WindowStaysOnTopHint

    property bool annotationMode: false
    property alias tool: surface.tool
    property alias strokeColor: surface.strokeColor
    property alias strokeWidth: surface.strokeWidth

    signal strokeCommitted(var normalizedPoints, int tool, double startedAtSeconds, double endedAtSeconds)
    signal passThroughRequested()

    function showHelper() {
        helper.show();
        helperHideTimer.restart();
    }

    function hideHelper() {
        helperHideTimer.stop();
        helper.hide();
    }

    function showOverlay() {
        surface.clear();
        passThroughRequested();
        show();
    }

    function enterPassThroughMode() {
        passThroughRequested();
    }

    function startRecordingClock() {
        surface.startRecordingClock();
    }

    function undo() {
        return surface.undo();
    }

    function clear() {
        surface.clear();
    }

    function clearAndHide() {
        passThroughRequested();
        surface.clear();
        hide();
    }

    onAnnotationModeChanged: {
        if (annotationMode) {
            helperHideTimer.stop();
            helper.show();
        } else {
            helper.hide();
        }
    }

    Timer {
        id: helperHideTimer
        interval: 3200
        repeat: false
        onTriggered: helper.hide()
    }

    AnnotationItem {
        id: surface
        objectName: "dictaAnnotationSurface"
        anchors.fill: parent
        focus: overlay.annotationMode
        annotationMode: overlay.annotationMode

        Keys.onEscapePressed: (event) => {
            overlay.passThroughRequested();
            event.accepted = true;
        }

        onPassThroughRequested: overlay.passThroughRequested()

        onStrokeCommitted: (points, selectedTool, started, ended) => {
            overlay.strokeCommitted(points, selectedTool, started, ended);
        }
    }

    Shortcut {
        sequences: ["Escape"]
        enabled: overlay.visible && overlay.annotationMode
        onActivated: overlay.passThroughRequested()
    }

    Window {
        id: helper

        objectName: "dictaAnnotationHelper"
        title: "Dicta Annotation Helper"
        transientParent: null
        width: 326
        height: 42
        color: "transparent"
        visible: false
        screen: overlay.screen
        x: screen ? screen.virtualX + Math.round((screen.width - width) / 2) : 0
        y: screen ? screen.virtualY + 42 : 42
        flags: Qt.FramelessWindowHint
            | Qt.Tool
            | Qt.BypassWindowManagerHint
            | Qt.WindowStaysOnTopHint
            | Qt.WindowTransparentForInput
            | Qt.WindowDoesNotAcceptFocus

        Rectangle {
            anchors.fill: parent
            radius: 11
            color: overlay.annotationMode ? "#dd22212c" : "#d91a1924"
            border.width: 1
            border.color: overlay.annotationMode ? "#ff7597" : "#596078"

            Text {
                objectName: "dictaAnnotationHelperText"
                anchors.centerIn: parent
                color: overlay.annotationMode ? "#ff91aa" : "#e8e9f2"
                font.family: "monospace"
                font.pixelSize: 13
                font.weight: Font.Medium
                text: overlay.annotationMode
                    ? "Drawing · release F8 to interact"
                    : "Hold F8 to draw while recording"
            }
        }
    }
}
