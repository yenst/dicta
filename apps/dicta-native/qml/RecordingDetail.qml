pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: root

    required property QtObject bridge
    required property QtObject dictaTheme
    property int currentTab: 0
    property bool confirmDelete: false
    property bool copied: false
    property var recording: bridge.selectedRecording || ({})
    property bool hasRecording: Boolean(recording.id)

    function playbackPosition() {
        return playerLoader.active && playerLoader.item
            ? Number(playerLoader.item.positionSeconds || 0) : 0
    }

    function seek(seconds) {
        if (playerLoader.active && playerLoader.item)
            playerLoader.item.seek(seconds)
    }

    function duration(seconds) {
        var total = Math.max(0, Math.round(Number(seconds) || 0))
        return String(Math.floor(total / 60)).padStart(2, "0") + ":"
            + String(total % 60).padStart(2, "0")
    }

    function timestamp(seconds) {
        var total = Math.max(0, Math.round(Number(seconds) || 0))
        var minutes = Math.floor(total / 60)
        var remainder = total % 60
        return String(minutes).padStart(2, "0") + ":" + String(remainder).padStart(2, "0")
    }

    function startedLabel(value) {
        if (!value)
            return ""
        var date = new Date(value)
        return isNaN(date.getTime()) ? "" : Qt.formatDateTime(date, "MMM d, yyyy · HH:mm")
    }

    function transcriptSegments() {
        var segments = recording.transcript_segments || []
        if (segments.length)
            return segments
        if (recording.transcript)
            return [{start_seconds: 0, end_seconds: recording.duration_seconds || 0,
                text: recording.transcript}]
        return []
    }

    function chapterRows() {
        var segments = transcriptSegments()
        var chapters = []
        var seen = ({})
        for (var i = 0; i < segments.length; ++i) {
            var minute = Math.floor(Number(segments[i].start_seconds || 0) / 60)
            if (seen[minute])
                continue
            seen[minute] = true
            var words = String(segments[i].text || "Chapter " + (chapters.length + 1))
                .trim().split(/\s+/).slice(0, 8).join(" ")
            chapters.push({timestamp_seconds: minute * 60, title: words})
        }
        return chapters
    }

    function annotationLabel() {
        var count = Number(recording.annotation_count || 0)
            + Number((recording.timeline_notes || []).length)
        if (count > 0)
            return count + (count === 1 ? " mark" : " marks")
        return recording.annotation_path ? "annotations" : "no marks"
    }

    function hasTimelineNoteAt(seconds) {
        var notes = recording.timeline_notes || []
        for (var i = 0; i < notes.length; ++i) {
            if (Math.abs(Number(notes[i].timestamp_seconds || 0) - Number(seconds || 0)) < 0.5)
                return true
        }
        return false
    }

    Rectangle {
        anchors.fill: parent
        color: root.dictaTheme.background
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: 22 * root.dictaTheme.spacingScale
            Layout.rightMargin: 16 * root.dictaTheme.spacingScale
            Layout.topMargin: 16 * root.dictaTheme.spacingScale
            Layout.bottomMargin: 12 * root.dictaTheme.spacingScale
            spacing: 10 * root.dictaTheme.spacingScale

            FlatButton {
                id: backButton
                objectName: "backToCapture"
                visible: root.width < 700 * root.dictaTheme.spacingScale
                dictaTheme: root.dictaTheme
                iconName: "back"
                iconOnly: true
                quiet: true
                toolTip: "Back to recordings"
                onClicked: root.bridge.closeRecording()
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 4 * root.dictaTheme.spacingScale
                Text {
                    Layout.fillWidth: true
                    text: root.recording.note || root.recording.id || "Recording"
                    color: root.dictaTheme.brightForeground
                    font.family: root.dictaTheme.fontFamily
                    font.pixelSize: root.dictaTheme.baseFontSize + 3
                    font.weight: Font.DemiBold
                    elide: Text.ElideRight
                }
                Text {
                    Layout.fillWidth: true
                    text: root.recording.transcript
                        ? String(root.recording.transcript).split(/\s+/).slice(0, 18).join(" ")
                        : (root.recording.id || "")
                    color: root.dictaTheme.darkForeground
                    font.family: root.dictaTheme.fontFamily
                    font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
                    elide: Text.ElideRight
                }
            }

            Text {
                visible: root.width > 650 * root.dictaTheme.spacingScale
                text: root.startedLabel(root.recording.started_at)
                color: root.dictaTheme.darkForeground
                font.family: root.dictaTheme.fontFamily
                font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
            }
            FlatButton {
                dictaTheme: root.dictaTheme
                iconName: "more"
                iconOnly: true
                quiet: true
                toolTip: "Recording actions"
                onClicked: actionMenu.open()

                Menu {
                    id: actionMenu
                    y: parent.height
                    background: Rectangle {
                        color: root.dictaTheme.darkBackground
                        border.width: 1
                        border.color: root.dictaTheme.muted
                        radius: 3 * root.dictaTheme.spacingScale
                    }
                    MenuItem {
                        text: "Open recording"
                        onTriggered: root.bridge.openSelectedRecording()
                    }
                    MenuItem {
                        id: transcribeAction
                        objectName: "transcribeRecording"
                        text: root.recording.transcription_status === "failed"
                            ? "Retry transcription" : "Transcribe again"
                        enabled: root.bridge.runtimePhase === "idle" && Boolean(root.recording.success)
                        onTriggered: root.bridge.transcribeSelectedRecording()
                    }
                    MenuSeparator {}
                    MenuItem {
                        id: deleteAction
                        objectName: "deleteRecording"
                        text: root.confirmDelete ? "Confirm delete" : "Delete recording"
                        enabled: root.bridge.runtimePhase === "idle"
                        onTriggered: {
                            if (root.confirmDelete) {
                                root.bridge.deleteSelectedRecording()
                                root.confirmDelete = false
                            } else {
                                root.confirmDelete = true
                                deleteConfirmTimer.restart()
                            }
                        }
                    }
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: 22 * root.dictaTheme.spacingScale
            Layout.rightMargin: 18 * root.dictaTheme.spacingScale
            Layout.topMargin: 8 * root.dictaTheme.spacingScale
            Layout.bottomMargin: 32 * root.dictaTheme.spacingScale
            spacing: 10 * root.dictaTheme.spacingScale
            Text {
                text: root.duration(root.recording.duration_seconds)
                color: root.dictaTheme.darkForeground
                font.family: root.dictaTheme.fontFamily
                font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
            }
            Rectangle {
                Layout.preferredWidth: 1
                Layout.preferredHeight: 14 * root.dictaTheme.spacingScale
                color: root.dictaTheme.muted
            }
            Text {
                text: root.recording.transcription_status === "complete"
                    ? "Transcribed"
                    : root.recording.transcription_status === "failed"
                        ? "Retry needed"
                        : (root.recording.transcription_status || "Unavailable")
                color: root.recording.transcription_status === "failed"
                    ? root.dictaTheme.yellow
                    : root.recording.transcription_status === "complete"
                        ? root.dictaTheme.accent : root.dictaTheme.darkForeground
                font.family: root.dictaTheme.fontFamily
                font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
            }
            Rectangle {
                Layout.preferredWidth: 1
                Layout.preferredHeight: 14 * root.dictaTheme.spacingScale
                color: root.dictaTheme.muted
            }
            Text {
                text: root.annotationLabel()
                color: root.dictaTheme.darkForeground
                font.family: root.dictaTheme.fontFamily
                font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
            }
            Item { Layout.fillWidth: true }
            Text {
                visible: Boolean(root.recording.git_branch)
                text: root.recording.recording_scope === "repository"
                    ? "repository" : (root.recording.git_branch || "")
                color: root.dictaTheme.accent
                font.family: root.dictaTheme.fontFamily
                font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.leftMargin: 22 * root.dictaTheme.spacingScale
            Layout.rightMargin: 22 * root.dictaTheme.spacingScale
            Layout.preferredHeight: Math.max(190 * root.dictaTheme.spacingScale,
                Math.min(370 * root.dictaTheme.spacingScale, root.height * 0.43))
            color: root.dictaTheme.darkerBackground
            border.width: 1
            border.color: root.dictaTheme.muted
            radius: 4 * root.dictaTheme.spacingScale
            clip: true

            Loader {
                id: playerLoader
                objectName: "recordingPlayerLoader"
                anchors.fill: parent
                active: root.hasRecording && root.bridge.multimediaAvailable
                    && Boolean(root.recording.video_url)
                source: active ? Qt.resolvedUrl("RecordingPlayer.qml") : ""
                onLoaded: {
                    item.source = root.recording.video_url
                    item.posterSource = root.recording.preview_image_url || ""
                    item.dictaTheme = root.dictaTheme
                }
            }

            Image {
                id: previewImage
                anchors.fill: parent
                visible: !playerLoader.active && Boolean(root.recording.preview_image_url)
                source: root.recording.preview_image_url || ""
                fillMode: Image.PreserveAspectCrop
                sourceSize.width: width
                sourceSize.height: height
                asynchronous: true
                cache: true

                Rectangle {
                    anchors.centerIn: parent
                    width: 54 * root.dictaTheme.spacingScale
                    height: width
                    radius: width / 2
                    color: Qt.rgba(root.dictaTheme.darkerBackground.r,
                        root.dictaTheme.darkerBackground.g,
                        root.dictaTheme.darkerBackground.b, 0.84)
                    ThemeIcon {
                        anchors.centerIn: parent
                        width: 21 * root.dictaTheme.spacingScale
                        height: width
                        iconName: "play"
                        iconColor: root.dictaTheme.brightForeground
                        iconSize: Math.round(width)
                    }
                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.bridge.openSelectedRecording()
                    }
                }

                Rectangle {
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.bottom: parent.bottom
                    height: 42 * root.dictaTheme.spacingScale
                    color: Qt.rgba(root.dictaTheme.darkerBackground.r,
                        root.dictaTheme.darkerBackground.g,
                        root.dictaTheme.darkerBackground.b, 0.92)
                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 6 * root.dictaTheme.spacingScale
                        anchors.rightMargin: 6 * root.dictaTheme.spacingScale
                        spacing: 7 * root.dictaTheme.spacingScale
                        FlatButton {
                            dictaTheme: root.dictaTheme
                            iconName: "play"
                            iconOnly: true
                            quiet: true
                            toolTip: "Open recording"
                            onClicked: root.bridge.openSelectedRecording()
                        }
                        Text {
                            text: "00:30 / " + root.duration(root.recording.duration_seconds)
                            color: root.dictaTheme.foreground
                            font.family: root.dictaTheme.fontFamily
                            font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
                        }
                        Rectangle {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 2
                            color: root.dictaTheme.muted
                            Rectangle {
                                width: parent.width * Math.min(1,
                                    30 / Math.max(1, Number(root.recording.duration_seconds || 0)))
                                height: parent.height
                                color: root.dictaTheme.accent
                            }
                        }
                        FlatButton {
                            dictaTheme: root.dictaTheme
                            iconName: "volume"
                            iconOnly: true
                            quiet: true
                            toolTip: "Open recording for audio controls"
                            onClicked: root.bridge.openSelectedRecording()
                        }
                        FlatButton {
                            dictaTheme: root.dictaTheme
                            iconName: "fullscreen"
                            iconOnly: true
                            quiet: true
                            toolTip: "Open recording fullscreen"
                            onClicked: root.bridge.openSelectedRecording()
                        }
                    }
                }
            }

            Column {
                visible: !playerLoader.active && !previewImage.visible
                anchors.centerIn: parent
                width: parent.width - 48 * root.dictaTheme.spacingScale
                spacing: 8 * root.dictaTheme.spacingScale
                ThemeIcon {
                    anchors.horizontalCenter: parent.horizontalCenter
                    width: 28 * root.dictaTheme.spacingScale
                    height: width
                    iconName: "play"
                    iconColor: root.dictaTheme.darkForeground
                    iconSize: Math.round(width)
                }
                Text {
                    width: parent.width
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.Wrap
                    text: root.bridge.multimediaAvailable
                        ? "The recording file is unavailable."
                        : "Video playback is unavailable in this build."
                    color: root.dictaTheme.darkForeground
                    font.family: root.dictaTheme.fontFamily
                    font.pixelSize: root.dictaTheme.baseFontSize
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: 22 * root.dictaTheme.spacingScale
            Layout.rightMargin: 22 * root.dictaTheme.spacingScale
            Layout.topMargin: 8 * root.dictaTheme.spacingScale
            spacing: 2 * root.dictaTheme.spacingScale
            Repeater {
                model: ["Transcript", "Chapters", "Notes"]
                delegate: Button {
                    id: tabButton
                    required property string modelData
                    required property int index
                    text: modelData
                    hoverEnabled: true
                    onClicked: root.currentTab = index
                    contentItem: Text {
                        text: tabButton.text
                        color: root.currentTab === tabButton.index
                            ? root.dictaTheme.accent : root.dictaTheme.darkForeground
                        font.family: root.dictaTheme.fontFamily
                        font.pixelSize: root.dictaTheme.baseFontSize
                        font.weight: root.currentTab === tabButton.index
                            ? Font.DemiBold : Font.Normal
                        horizontalAlignment: Text.AlignHCenter
                        verticalAlignment: Text.AlignVCenter
                    }
                    background: Item {
                        implicitHeight: 42 * root.dictaTheme.spacingScale
                        Rectangle {
                            visible: root.currentTab === tabButton.index
                            anchors.left: parent.left
                            anchors.right: parent.right
                            anchors.bottom: parent.bottom
                            height: 2
                            color: root.dictaTheme.accent
                        }
                    }
                }
            }
            Item { Layout.fillWidth: true }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 1
            Layout.leftMargin: 22 * root.dictaTheme.spacingScale
            Layout.rightMargin: 22 * root.dictaTheme.spacingScale
            color: Qt.rgba(root.dictaTheme.muted.r, root.dictaTheme.muted.g,
                root.dictaTheme.muted.b, 0.7)
        }

        StackLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            currentIndex: root.currentTab

            ScrollView {
                clip: true
                contentWidth: availableWidth
                ColumnLayout {
                    width: parent.width
                    spacing: 0
                    Repeater {
                        model: root.transcriptSegments()
                        delegate: Item {
                            id: segmentRow
                            required property var modelData
                            Layout.fillWidth: true
                            Layout.preferredHeight: transcriptContent.implicitHeight
                                + 16 * root.dictaTheme.spacingScale + 1

                            RowLayout {
                                id: transcriptContent
                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.top: parent.top
                                anchors.leftMargin: 24 * root.dictaTheme.spacingScale
                                anchors.rightMargin: 26 * root.dictaTheme.spacingScale
                                anchors.topMargin: 8 * root.dictaTheme.spacingScale
                                spacing: 20 * root.dictaTheme.spacingScale
                                Item {
                                    Layout.preferredWidth: 52 * root.dictaTheme.spacingScale
                                    Layout.alignment: Qt.AlignTop
                                    Layout.preferredHeight: transcriptTime.implicitHeight
                                    Text {
                                        id: transcriptTime
                                        anchors.left: parent.left
                                        text: root.timestamp(segmentRow.modelData.start_seconds)
                                        color: root.dictaTheme.accent
                                        font.family: root.dictaTheme.fontFamily
                                        font.pixelSize: root.dictaTheme.baseFontSize
                                        font.weight: Font.DemiBold
                                    }
                                    Rectangle {
                                        visible: root.hasTimelineNoteAt(
                                            segmentRow.modelData.start_seconds)
                                        anchors.right: transcriptTime.left
                                        anchors.rightMargin: 6 * root.dictaTheme.spacingScale
                                        anchors.verticalCenter: transcriptTime.verticalCenter
                                        width: 7 * root.dictaTheme.spacingScale
                                        height: width
                                        radius: width / 2
                                        color: root.dictaTheme.red
                                    }
                                }
                                Text {
                                    Layout.fillWidth: true
                                    text: segmentRow.modelData.text
                                    color: root.dictaTheme.foreground
                                    font.family: root.dictaTheme.fontFamily
                                    font.pixelSize: root.dictaTheme.baseFontSize
                                    lineHeight: 1.45
                                    wrapMode: Text.Wrap
                                }
                            }
                            Rectangle {
                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.bottom: parent.bottom
                                anchors.leftMargin: 24 * root.dictaTheme.spacingScale
                                anchors.rightMargin: 26 * root.dictaTheme.spacingScale
                                height: 1
                                color: Qt.rgba(root.dictaTheme.muted.r,
                                    root.dictaTheme.muted.g,
                                    root.dictaTheme.muted.b, 0.45)
                            }
                        }
                    }
                    Text {
                        visible: root.transcriptSegments().length === 0
                        Layout.fillWidth: true
                        Layout.margins: 28 * root.dictaTheme.spacingScale
                        horizontalAlignment: Text.AlignHCenter
                        text: root.recording.transcription_error
                            || "No transcript is available yet."
                        color: root.dictaTheme.darkForeground
                        font.family: root.dictaTheme.fontFamily
                        font.pixelSize: root.dictaTheme.baseFontSize
                        wrapMode: Text.Wrap
                    }
                }
            }

            ScrollView {
                clip: true
                contentWidth: availableWidth
                ColumnLayout {
                    width: parent.width
                    spacing: 0
                    Repeater {
                        model: root.chapterRows()
                        delegate: RowLayout {
                            required property var modelData
                            Layout.fillWidth: true
                            Layout.leftMargin: 24 * root.dictaTheme.spacingScale
                            Layout.rightMargin: 26 * root.dictaTheme.spacingScale
                            Layout.topMargin: 16 * root.dictaTheme.spacingScale
                            Layout.bottomMargin: 16 * root.dictaTheme.spacingScale
                            spacing: 20 * root.dictaTheme.spacingScale
                            Text {
                                Layout.preferredWidth: 52 * root.dictaTheme.spacingScale
                                text: root.timestamp(noteRow.modelData.timestamp_seconds)
                                color: root.dictaTheme.accent
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: root.dictaTheme.baseFontSize
                            }
                            Text {
                                Layout.fillWidth: true
                                text: modelData.title
                                color: root.dictaTheme.foreground
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: root.dictaTheme.baseFontSize + 1
                                elide: Text.ElideRight
                            }
                        }
                    }
                    Text {
                        visible: root.chapterRows().length === 0
                        Layout.fillWidth: true
                        Layout.margins: 28 * root.dictaTheme.spacingScale
                        horizontalAlignment: Text.AlignHCenter
                        text: "Chapters appear when timed transcript segments are available."
                        color: root.dictaTheme.darkForeground
                        font.family: root.dictaTheme.fontFamily
                        font.pixelSize: root.dictaTheme.baseFontSize
                        wrapMode: Text.Wrap
                    }
                }
            }

            ScrollView {
                clip: true
                contentWidth: availableWidth
                ColumnLayout {
                    width: parent.width
                    spacing: 0
                    Rectangle {
                        Layout.fillWidth: true
                        Layout.leftMargin: 24 * root.dictaTheme.spacingScale
                        Layout.rightMargin: 26 * root.dictaTheme.spacingScale
                        Layout.topMargin: 14 * root.dictaTheme.spacingScale
                        Layout.bottomMargin: 10 * root.dictaTheme.spacingScale
                        Layout.preferredHeight: 46 * root.dictaTheme.spacingScale
                        color: root.dictaTheme.darkBackground
                        border.width: 1
                        border.color: noteField.activeFocus
                            ? root.dictaTheme.accent : root.dictaTheme.muted
                        radius: 3 * root.dictaTheme.spacingScale

                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 12 * root.dictaTheme.spacingScale
                            anchors.rightMargin: 6 * root.dictaTheme.spacingScale
                            spacing: 8 * root.dictaTheme.spacingScale
                            Text {
                                text: root.timestamp(root.playbackPosition())
                                color: root.dictaTheme.red
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: root.dictaTheme.baseFontSize
                                font.weight: Font.DemiBold
                            }
                            TextField {
                                id: noteField
                                objectName: "timelineNoteField"
                                Layout.fillWidth: true
                                placeholderText: "Add a note at the playback cursor"
                                color: root.dictaTheme.foreground
                                placeholderTextColor: root.dictaTheme.darkForeground
                                selectionColor: root.dictaTheme.selection
                                selectedTextColor: root.dictaTheme.brightForeground
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: root.dictaTheme.baseFontSize
                                maximumLength: 2000
                                background: Item {}
                                Keys.onReturnPressed: addNoteButton.clicked()
                            }
                            FlatButton {
                                id: addNoteButton
                                objectName: "addTimelineNote"
                                dictaTheme: root.dictaTheme
                                text: "ADD NOTE"
                                enabled: noteField.text.trim().length > 0
                                selected: enabled
                                onClicked: {
                                    if (root.bridge.addTimelineNote(
                                            noteField.text, root.playbackPosition())) {
                                        noteField.text = ""
                                    }
                                }
                            }
                        }
                    }
                    Repeater {
                        model: root.recording.timeline_notes || []
                        delegate: RowLayout {
                            id: noteRow
                            required property var modelData
                            Layout.fillWidth: true
                            Layout.leftMargin: 24 * root.dictaTheme.spacingScale
                            Layout.rightMargin: 26 * root.dictaTheme.spacingScale
                            Layout.topMargin: 14 * root.dictaTheme.spacingScale
                            Layout.bottomMargin: 14 * root.dictaTheme.spacingScale
                            spacing: 20 * root.dictaTheme.spacingScale
                            Text {
                                Layout.preferredWidth: 52 * root.dictaTheme.spacingScale
                                text: root.timestamp(modelData.timestamp_seconds)
                                color: root.dictaTheme.red
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: root.dictaTheme.baseFontSize
                                font.weight: Font.DemiBold
                                MouseArea {
                                    anchors.fill: parent
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: root.seek(noteRow.modelData.timestamp_seconds)
                                }
                            }
                            Text {
                                Layout.fillWidth: true
                                text: noteRow.modelData.text
                                color: root.dictaTheme.foreground
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: root.dictaTheme.baseFontSize
                                wrapMode: Text.Wrap
                            }
                            FlatButton {
                                dictaTheme: root.dictaTheme
                                iconName: "delete"
                                iconOnly: true
                                quiet: true
                                destructive: true
                                toolTip: "Remove timeline note"
                                onClicked: root.bridge.removeTimelineNote(noteRow.modelData.id)
                            }
                        }
                    }
                    Text {
                        visible: (root.recording.timeline_notes || []).length === 0
                        Layout.fillWidth: true
                        Layout.margins: 28 * root.dictaTheme.spacingScale
                        horizontalAlignment: Text.AlignHCenter
                        text: "No timeline notes yet. Play the recording and add one at the cursor."
                        color: root.dictaTheme.darkForeground
                        font.family: root.dictaTheme.fontFamily
                        font.pixelSize: root.dictaTheme.baseFontSize
                        wrapMode: Text.Wrap
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
            Layout.preferredHeight: 60 * root.dictaTheme.spacingScale
            Layout.leftMargin: 14 * root.dictaTheme.spacingScale
            Layout.rightMargin: 14 * root.dictaTheme.spacingScale
            Layout.topMargin: 6 * root.dictaTheme.spacingScale
            Layout.bottomMargin: 6 * root.dictaTheme.spacingScale
            spacing: 4 * root.dictaTheme.spacingScale

            FlatButton {
                dictaTheme: root.dictaTheme
                text: root.copied ? "Copied" : "Copy context"
                iconName: "copy"
                selected: root.copied
                quiet: true
                onClicked: {
                    if (root.bridge.copySelectedContext()) {
                        root.copied = true
                        copiedTimer.restart()
                    }
                }
            }
            FlatButton {
                dictaTheme: root.dictaTheme
                text: "Reveal files"
                iconName: "folder-open"
                quiet: true
                onClicked: root.bridge.revealSelectedRecording()
            }
            Item { Layout.fillWidth: true }
            FlatButton {
                dictaTheme: root.dictaTheme
                iconName: "more"
                iconOnly: true
                quiet: true
                toolTip: "More actions"
                onClicked: actionMenu.open()
            }
        }
    }

    Timer {
        id: copiedTimer
        interval: 1800
        onTriggered: root.copied = false
    }
    Timer {
        id: deleteConfirmTimer
        interval: 5000
        onTriggered: root.confirmDelete = false
    }
}
