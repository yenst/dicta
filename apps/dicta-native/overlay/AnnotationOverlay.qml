import QtQuick
import QtQuick.Window
import Dicta.Native

Window {
    id: overlay

    objectName: "dictaAnnotationOverlay"
    color: "transparent"
    visible: false
    flags: Qt.FramelessWindowHint

    property bool annotationMode: false
    property alias tool: surface.tool
    property alias strokeColor: surface.strokeColor
    property alias strokeWidth: surface.strokeWidth

    signal strokeCommitted(var normalizedPoints, int tool, double startedAtSeconds, double endedAtSeconds)
    signal passThroughRequested()

    function showOverlay() {
        surface.clear();
        passThroughRequested();
        showFullScreen();
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
}
