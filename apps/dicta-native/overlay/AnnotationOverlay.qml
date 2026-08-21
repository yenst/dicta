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
        showToast("Hold F8 to draw while recording");
    }

    function hideHelper() {
    }

    function showToast(message) {
        toastText.text = String(message || "");
        if (!toastText.text.length)
            return;
        toast.screen = overlay.screen;
        toast.show();
        toastHideTimer.restart();
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

    onAnnotationModeChanged: if (annotationMode)
        showToast("Drawing · release F8 to interact")

    Timer {
        id: toastHideTimer
        interval: 2200
        repeat: false
        onTriggered: toast.hide()
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
        id: toast

        objectName: "dictaStatusToast"
        title: "Dicta status"
        transientParent: null
        width: Math.max(188, toastText.implicitWidth + 54)
        height: 46
        color: "transparent"
        visible: false
        screen: overlay.screen
        x: screen ? screen.virtualX + screen.width - width - 24 : 0
        y: screen ? screen.virtualY + screen.height - height - 34 : 0
        flags: Qt.FramelessWindowHint
            | Qt.Tool
            | Qt.BypassWindowManagerHint
            | Qt.WindowStaysOnTopHint
            | Qt.WindowTransparentForInput
            | Qt.WindowDoesNotAcceptFocus

        Rectangle {
            anchors.fill: parent
            radius: 10
            color: "#ed1a1924"
            border.width: 1
            border.color: "#596078"

            Rectangle {
                anchors.left: parent.left
                anchors.leftMargin: 15
                anchors.verticalCenter: parent.verticalCenter
                width: 8
                height: width
                radius: width / 2
                color: "#8cafef"
            }

            Text {
                id: toastText
                objectName: "dictaStatusToastText"
                anchors.left: parent.left
                anchors.leftMargin: 35
                anchors.right: parent.right
                anchors.rightMargin: 15
                anchors.verticalCenter: parent.verticalCenter
                color: "#e8e9f2"
                font.family: "monospace"
                font.pixelSize: 13
                font.weight: Font.Medium
                elide: Text.ElideRight
            }
        }
    }
}
