pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: root

    required property QtObject bridge
    required property QtObject dictaTheme
    property string filterText: ""
    property var recordings: bridge.recentRecordings || []
    property bool keyboardFocused: false
    property string pendingDeleteId: ""
    property int activityFrame: 0
    readonly property bool firstRowDividerVisible: dividerVisibleAt(0)
    readonly property string activityIndicatorText:
        ["[   ]", "[.  ]", "[.. ]", "[...]"][activityFrame]
    readonly property bool deleteActionVisible: pendingDeleteId.length > 0
    signal keyboardFocusRequested()

    function requestDelete() {
        var recordingId = String(bridge.selectedRecordingId || "")
        if (!recordingId.length || bridge.runtimePhase !== "idle")
            return false
        if (pendingDeleteId !== recordingId) {
            pendingDeleteId = recordingId
            deleteArmTimer.restart()
            return true
        }
        var removed = bridge.deleteSelectedRecording()
        if (removed) {
            pendingDeleteId = ""
            bridge.showToast("Recording deleted")
        }
        return removed
    }

    function confirmDelete(recordingId) {
        if (pendingDeleteId !== recordingId)
            return false
        if (String(bridge.selectedRecordingId || "") !== recordingId)
            bridge.selectRecording(recordingId)
        return requestDelete()
    }

    function filteredRecordings() {
        var query = filterText.trim().toLowerCase()
        if (!query.length)
            return recordings
        return recordings.filter(function(recording) {
            return String(recording.id || "").toLowerCase().indexOf(query) >= 0
                || String(recording.note || "").toLowerCase().indexOf(query) >= 0
                || String(recording.transcript_preview || "").toLowerCase().indexOf(query) >= 0
        })
    }

    function selectedIndex() {
        var rows = filteredRecordings()
        for (var i = 0; i < rows.length; ++i) {
            if (String(rows[i].id || "") === String(bridge.selectedRecordingId || ""))
                return i
        }
        return rows.length ? 0 : -1
    }

    function moveKeyboardSelection(delta) {
        var rows = filteredRecordings()
        if (!rows.length)
            return
        var next = selectedIndex()
        next = Math.max(0, Math.min(rows.length - 1, next + delta))
        pendingDeleteId = ""
        bridge.selectRecording(rows[next].id)
        recordingList.positionViewAtIndex(next, ListView.Contain)
    }

    function activateKeyboardSelection() {
        var rows = filteredRecordings()
        var index = selectedIndex()
        if (index >= 0 && index < rows.length)
            bridge.selectRecording(rows[index].id)
    }

    function dateValue(value) {
        var date = value ? new Date(value) : null
        return date && !isNaN(date.getTime()) ? date : null
    }

    function dayKey(value) {
        var date = dateValue(value)
        return date ? date.getFullYear() + "-" + (date.getMonth() + 1) + "-" + date.getDate() : "unknown"
    }

    function dayLabel(value) {
        var date = dateValue(value)
        if (!date)
            return "Earlier"
        var today = new Date()
        if (date.toDateString() === today.toDateString())
            return "Today · " + Qt.formatDate(date, "MMM d")
        return Qt.formatDate(date, "MMM d, yyyy")
    }

    function timeLabel(value) {
        var date = dateValue(value)
        return date ? Qt.formatTime(date, "HH:mm") : "--:--"
    }

    function duration(seconds) {
        var total = Math.max(0, Math.round(Number(seconds) || 0))
        return String(Math.floor(total / 60)).padStart(2, "0") + ":"
            + String(total % 60).padStart(2, "0")
    }

    function statusLabel(value) {
        if (value === "complete") return "Transcribed"
        if (value === "failed") return "Retry"
        if (value === "processing" || value === "pending")
            return root.activityIndicatorText
        return "Unavailable"
    }

    function hasActiveTranscription() {
        for (var i = 0; i < recordings.length; ++i) {
            var state = String(recordings[i].transcription || "")
            if (state === "pending" || state === "processing")
                return true
        }
        return false
    }

    function dividerVisibleAt(index) {
        var rows = filteredRecordings()
        if (index < 0 || index >= rows.length)
            return false
        if (String(bridge.selectedRecordingId || "") === String(rows[index].id || ""))
            return false
        return index + 1 >= rows.length
            || root.dayKey(rows[index].started_at) !== root.dayKey(rows[index + 1].started_at)
            || String(bridge.selectedRecordingId || "") !== String(rows[index + 1].id || "")
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
            Layout.preferredHeight: 58 * root.dictaTheme.spacingScale
            Layout.leftMargin: 24 * root.dictaTheme.spacingScale
            Layout.rightMargin: 14 * root.dictaTheme.spacingScale
            spacing: 8 * root.dictaTheme.spacingScale

            Text {
                Layout.fillWidth: true
                text: "Context log"
                color: root.dictaTheme.brightForeground
                font.family: root.dictaTheme.fontFamily
                font.pixelSize: root.dictaTheme.baseFontSize + 3
                font.weight: Font.DemiBold
            }

        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 1
            color: Qt.rgba(root.dictaTheme.muted.r, root.dictaTheme.muted.g,
                root.dictaTheme.muted.b, 0.55)
        }

        ListView {
            id: recordingList
            objectName: "recentRecordingsList"
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            model: root.filteredRecordings()
            ScrollBar.vertical: ScrollBar {}

            delegate: Item {
                id: recordingRow
                required property var modelData
                required property int index
                width: recordingList.width
                property bool firstOfDay: index === 0
                    || root.dayKey(recordingList.model[index - 1].started_at)
                        !== root.dayKey(modelData.started_at)
                property bool selected: root.bridge.selectedRecordingId === modelData.id
                property bool keyboardSelected: root.keyboardFocused && selected
                property bool deleteArmed: root.pendingDeleteId === modelData.id
                height: (firstOfDay ? 46 : 0) * root.dictaTheme.spacingScale
                    + 88 * root.dictaTheme.spacingScale

                Text {
                    visible: recordingRow.firstOfDay
                    anchors.left: parent.left
                    anchors.leftMargin: 24 * root.dictaTheme.spacingScale
                    anchors.top: parent.top
                    anchors.topMargin: 18 * root.dictaTheme.spacingScale
                    text: root.dayLabel(recordingRow.modelData.started_at)
                    color: root.dictaTheme.darkForeground
                    font.family: root.dictaTheme.fontFamily
                    font.pixelSize: root.dictaTheme.baseFontSize
                }

                Rectangle {
                    visible: root.deleteActionVisible && recordingRow.deleteArmed
                    anchors.right: parent.right
                    anchors.bottom: parent.bottom
                    width: 58 * root.dictaTheme.spacingScale
                    height: 88 * root.dictaTheme.spacingScale
                    color: Qt.rgba(root.dictaTheme.red.r, root.dictaTheme.red.g,
                        root.dictaTheme.red.b, 0.11)

                    FlatButton {
                        objectName: "confirmDelete-" + recordingRow.modelData.id
                        anchors.centerIn: parent
                        dictaTheme: root.dictaTheme
                        iconName: "delete"
                        iconOnly: true
                        quiet: true
                        destructive: true
                        toolTip: "Delete recording"
                        onClicked: root.confirmDelete(recordingRow.modelData.id)
                    }
                }

                Rectangle {
                    id: rowSurface
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.rightMargin: recordingRow.deleteArmed
                        ? 58 * root.dictaTheme.spacingScale : 0
                    anchors.bottom: parent.bottom
                    height: 88 * root.dictaTheme.spacingScale
                    color: recordingRow.selected
                        ? Qt.rgba(root.dictaTheme.accent.r, root.dictaTheme.accent.g,
                            root.dictaTheme.accent.b,
                            recordingRow.keyboardSelected ? 0.17 : 0.11)
                        : rowMouse.containsMouse
                            ? Qt.rgba(root.dictaTheme.foreground.r, root.dictaTheme.foreground.g,
                                root.dictaTheme.foreground.b, 0.04)
                            : "transparent"

                    Behavior on anchors.rightMargin {
                        NumberAnimation { duration: 140; easing.type: Easing.OutCubic }
                    }

                    Rectangle {
                        visible: recordingRow.selected
                        anchors.left: parent.left
                        anchors.top: parent.top
                        anchors.bottom: parent.bottom
                        width: recordingRow.keyboardSelected ? 3 : 2
                        color: root.dictaTheme.accent
                    }
                    Rectangle {
                        visible: root.dividerVisibleAt(recordingRow.index)
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.bottom: parent.bottom
                        anchors.leftMargin: 16 * root.dictaTheme.spacingScale
                        anchors.rightMargin: 16 * root.dictaTheme.spacingScale
                        height: 1
                        color: Qt.rgba(root.dictaTheme.muted.r, root.dictaTheme.muted.g,
                            root.dictaTheme.muted.b, 0.5)
                    }
                    Rectangle {
                        visible: recordingRow.selected
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.top: parent.top
                        height: 1
                        color: Qt.rgba(root.dictaTheme.accent.r,
                            root.dictaTheme.accent.g,
                            root.dictaTheme.accent.b, 0.55)
                    }
                    Rectangle {
                        visible: recordingRow.selected
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.bottom: parent.bottom
                        height: 1
                        color: Qt.rgba(root.dictaTheme.accent.r,
                            root.dictaTheme.accent.g,
                            root.dictaTheme.accent.b, 0.55)
                    }

                    Text {
                        anchors.left: parent.left
                        anchors.leftMargin: 24 * root.dictaTheme.spacingScale
                        anchors.top: parent.top
                        anchors.topMargin: 18 * root.dictaTheme.spacingScale
                        width: 44 * root.dictaTheme.spacingScale
                        horizontalAlignment: Text.AlignHCenter
                        text: root.timeLabel(recordingRow.modelData.started_at)
                        color: root.dictaTheme.foreground
                        font.family: root.dictaTheme.fontFamily
                        font.pixelSize: root.dictaTheme.baseFontSize
                    }

                    Column {
                        anchors.left: parent.left
                        anchors.leftMargin: 86 * root.dictaTheme.spacingScale
                        anchors.right: statusColumn.left
                        anchors.rightMargin: 12 * root.dictaTheme.spacingScale
                        anchors.top: parent.top
                        anchors.topMargin: 15 * root.dictaTheme.spacingScale
                        spacing: 6 * root.dictaTheme.spacingScale
                        Text {
                            width: parent.width
                            text: recordingRow.modelData.note || recordingRow.modelData.id
                            color: recordingRow.selected
                                ? root.dictaTheme.accent : root.dictaTheme.brightForeground
                            font.family: root.dictaTheme.fontFamily
                            font.pixelSize: root.dictaTheme.baseFontSize + 1
                            font.weight: Font.Medium
                            elide: Text.ElideRight
                        }
                        Text {
                            width: parent.width
                            text: recordingRow.modelData.transcript_preview
                                || ((recordingRow.modelData.project || "General")
                                    + (recordingRow.modelData.branch
                                        ? " · " + recordingRow.modelData.branch : ""))
                            color: root.dictaTheme.darkForeground
                            font.family: root.dictaTheme.fontFamily
                            font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 2)
                            elide: Text.ElideRight
                        }
                    }

                    Item {
                        id: statusColumn
                        anchors.right: parent.right
                        anchors.rightMargin: 22 * root.dictaTheme.spacingScale
                        anchors.top: parent.top
                        anchors.bottom: parent.bottom
                        width: 82 * root.dictaTheme.spacingScale
                        Text {
                            anchors.right: parent.right
                            anchors.top: parent.top
                            anchors.topMargin: 17 * root.dictaTheme.spacingScale
                            text: root.duration(recordingRow.modelData.duration_seconds)
                            color: root.dictaTheme.darkForeground
                            font.family: root.dictaTheme.fontFamily
                            font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
                        }
                        ThemeIcon {
                            visible: recordingRow.modelData.transcription === "complete"
                            anchors.right: parent.right
                            anchors.bottom: parent.bottom
                            anchors.bottomMargin: 17 * root.dictaTheme.spacingScale
                            width: 16 * root.dictaTheme.spacingScale
                            height: width
                            iconName: "check"
                            iconColor: root.dictaTheme.foreground
                            iconSize: Math.round(width)
                        }
                        Text {
                            visible: recordingRow.modelData.transcription !== "complete"
                            anchors.right: parent.right
                            anchors.bottom: parent.bottom
                            anchors.bottomMargin: 17 * root.dictaTheme.spacingScale
                            text: root.statusLabel(recordingRow.modelData.transcription)
                            color: recordingRow.modelData.transcription === "failed"
                                ? root.dictaTheme.yellow
                                : root.dictaTheme.darkForeground
                            font.family: root.dictaTheme.fontFamily
                            font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
                        }
                    }

                    MouseArea {
                        id: rowMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: {
                            root.pendingDeleteId = ""
                            root.bridge.selectRecording(recordingRow.modelData.id)
                            root.keyboardFocusRequested()
                        }
                    }
                }
            }

            Text {
                anchors.centerIn: parent
                visible: recordingList.count === 0
                width: parent.width - 48 * root.dictaTheme.spacingScale
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.Wrap
                text: root.filterText.length
                    ? "No recording matches this filter."
                    : "No recordings yet. Start with a short explanation."
                color: root.dictaTheme.darkForeground
                font.family: root.dictaTheme.fontFamily
                font.pixelSize: root.dictaTheme.baseFontSize
            }
        }

    }

    Item {
        objectName: "recordingKeyboardBorder"
        anchors.fill: parent
        z: 20
        visible: root.keyboardFocused


        Rectangle {
            anchors.top: parent.top
            anchors.left: parent.left
            anchors.right: parent.right
            height: 2
            color: root.dictaTheme.accent
        }
        Rectangle {
            anchors.top: parent.top
            anchors.left: parent.left
            anchors.bottom: parent.bottom
            width: 2
            color: root.dictaTheme.accent
        }
        Rectangle {
            anchors.top: parent.top
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            width: 2
            color: root.dictaTheme.accent
        }
    }

    Timer {
        interval: 280
        repeat: true
        running: root.hasActiveTranscription()
        onTriggered: root.activityFrame = (root.activityFrame + 1) % 4
    }

    Timer {
        id: deleteArmTimer
        interval: 5000
        onTriggered: root.pendingDeleteId = ""
    }
}
