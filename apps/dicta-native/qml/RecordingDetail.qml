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
    property bool keyboardActive: false
    property int keyboardTarget: 0
    readonly property bool noteEditorFocused: noteField.activeFocus
    property var recording: bridge.selectedRecording || ({})
    property bool hasRecording: Boolean(recording.id)
    signal keyboardFocusRequested()

    function resetKeyboardTarget() {
        keyboardTarget = 0
    }

    function moveKeyboardHorizontal(delta) {
        if (keyboardTarget === 3) {
            if (delta < 0 && currentTab === 0) {
                keyboardTarget = 0
                return true
            }
            currentTab = Math.max(0, Math.min(2, currentTab + delta))
            return true
        }
        var next = keyboardTarget + delta
        if (next < 0)
            return false
        keyboardTarget = Math.min(2, next)
        return true
    }

    function moveKeyboardVertical(delta) {
        if (delta > 0 && keyboardTarget !== 3) {
            keyboardTarget = 3
            return true
        }
        if (delta < 0 && keyboardTarget === 3) {
            keyboardTarget = 0
            return true
        }
        return false
    }

    function activateKeyboardTarget() {
        if (keyboardTarget === 0)
            return togglePlayback()
        if (keyboardTarget === 1)
            return copyContext()
        if (keyboardTarget === 2) {
            actionPopup.open()
            return true
        }
        return keyboardTarget === 3
    }

    function playbackPosition() {
        return playerLoader.active && playerLoader.item
            ? Number(playerLoader.item.positionSeconds || 0) : 0
    }

    function seek(seconds) {
        if (playerLoader.active && playerLoader.item)
            playerLoader.item.seek(seconds)
    }

    function togglePlayback() {
        if (playerLoader.active && playerLoader.item) {
            playerLoader.item.togglePlayback()
            return true
        }
        return bridge.openSelectedRecording()
    }

    function copyContext() {
        if (!bridge.copySelectedContext())
            return false
        copied = true
        copiedTimer.restart()
        return true
    }

    function promptDelete() {
        if (!hasRecording || bridge.runtimePhase !== "idle")
            return false
        confirmDelete = true
        deleteConfirmTimer.restart()
        actionPopup.open()
        return true
    }

    function confirmDeleteNow() {
        if (!confirmDelete)
            return false
        var removed = bridge.deleteSelectedRecording()
        confirmDelete = false
        actionPopup.close()
        return removed
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
                spacing: 0
                Text {
                    Layout.fillWidth: true
                    text: root.recording.note || root.recording.id || "Recording"
                    color: root.dictaTheme.brightForeground
                    font.family: root.dictaTheme.fontFamily
                    font.pixelSize: root.dictaTheme.baseFontSize + 3
                    font.weight: Font.DemiBold
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
                objectName: "copyContext"
                dictaTheme: root.dictaTheme
                iconName: "copy"
                iconOnly: true
                quiet: true
                toolTip: root.copied ? "Context copied" : "Copy context"
                selected: root.keyboardActive && root.keyboardTarget === 1 || root.copied
                onClicked: root.copyContext()
            }
            FlatButton {
                objectName: "recordingMoreActions"
                dictaTheme: root.dictaTheme
                iconName: "more"
                iconOnly: true
                quiet: true
                toolTip: "Recording actions"
                selected: root.keyboardActive && root.keyboardTarget === 2
                onClicked: actionPopup.open()
            }
        }

        Rectangle {
            id: playerSurface
            objectName: "playerKeyboardSurface"
            Layout.fillWidth: true
            Layout.leftMargin: 22 * root.dictaTheme.spacingScale
            Layout.rightMargin: 22 * root.dictaTheme.spacingScale
            Layout.preferredHeight: Math.max(190 * root.dictaTheme.spacingScale,
                Math.min(370 * root.dictaTheme.spacingScale, root.height * 0.43))
            color: root.dictaTheme.darkerBackground
            border.color: root.keyboardActive && root.keyboardTarget === 0
                ? root.dictaTheme.accent : root.dictaTheme.muted
            border.width: root.keyboardActive && root.keyboardTarget === 0 ? 3 : 1
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
                    item.dictaTheme = root.dictaTheme
                }
            }

            Binding {
                target: playerLoader.item
                property: "source"
                value: root.recording.video_url || ""
                when: playerLoader.status === Loader.Ready
            }

            Binding {
                target: playerLoader.item
                property: "posterSource"
                value: root.recording.preview_image_url || ""
                when: playerLoader.status === Loader.Ready
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
                    objectName: "detailTab-" + index
                    Layout.preferredWidth: 92 * root.dictaTheme.spacingScale
                    Layout.preferredHeight: 42 * root.dictaTheme.spacingScale
                    hoverEnabled: true
                    onClicked: root.currentTab = index
                    contentItem: Text {
                        text: tabButton.text
                        color: root.currentTab === tabButton.index
                            ? root.dictaTheme.accent : root.dictaTheme.darkForeground
                        font.family: root.dictaTheme.fontFamily
                        font.pixelSize: root.dictaTheme.baseFontSize
                        font.weight: Font.Medium
                        horizontalAlignment: Text.AlignHCenter
                        verticalAlignment: Text.AlignVCenter
                    }
                    background: Rectangle {
                        implicitHeight: 42 * root.dictaTheme.spacingScale
                        radius: 3 * root.dictaTheme.spacingScale
                        color: root.keyboardActive && root.keyboardTarget === 3
                                && root.currentTab === tabButton.index
                            ? Qt.rgba(root.dictaTheme.accent.r,
                                root.dictaTheme.accent.g,
                                root.dictaTheme.accent.b, 0.1)
                            : "transparent"
                        border.width: root.keyboardActive && root.keyboardTarget === 3
                                && root.currentTab === tabButton.index ? 2 : 0
                        border.color: root.dictaTheme.accent
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
                                TextEdit {
                                    Layout.fillWidth: true
                                    text: segmentRow.modelData.text
                                    readOnly: true
                                    selectByMouse: true
                                    selectionColor: root.dictaTheme.selection
                                    selectedTextColor: root.dictaTheme.brightForeground
                                    color: root.dictaTheme.foreground
                                    font.family: root.dictaTheme.fontFamily
                                    font.pixelSize: root.dictaTheme.baseFontSize
                                    wrapMode: TextEdit.Wrap
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
                            id: chapterRow
                            required property var modelData
                            Layout.fillWidth: true
                            Layout.leftMargin: 24 * root.dictaTheme.spacingScale
                            Layout.rightMargin: 26 * root.dictaTheme.spacingScale
                            Layout.topMargin: 16 * root.dictaTheme.spacingScale
                            Layout.bottomMargin: 16 * root.dictaTheme.spacingScale
                            spacing: 20 * root.dictaTheme.spacingScale
                            Text {
                                Layout.preferredWidth: 52 * root.dictaTheme.spacingScale
                                text: root.timestamp(chapterRow.modelData.timestamp_seconds)
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
                    Rectangle {
                        Layout.fillWidth: true
                        Layout.leftMargin: 24 * root.dictaTheme.spacingScale
                        Layout.rightMargin: 26 * root.dictaTheme.spacingScale
                        Layout.bottomMargin: 10 * root.dictaTheme.spacingScale
                        Layout.preferredHeight: 54 * root.dictaTheme.spacingScale
                        color: root.dictaTheme.darkBackground
                        border.width: 1
                        border.color: root.bridge.voiceNoteStatus.state === "recording"
                            ? root.dictaTheme.red : root.dictaTheme.muted
                        radius: 3 * root.dictaTheme.spacingScale

                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 12 * root.dictaTheme.spacingScale
                            anchors.rightMargin: 8 * root.dictaTheme.spacingScale
                            spacing: 8 * root.dictaTheme.spacingScale
                            Text {
                                Layout.fillWidth: true
                                text: root.bridge.voiceNoteStatus.message
                                    || "Record a spoken note at the playback cursor"
                                color: root.bridge.voiceNoteStatus.state === "failed"
                                    ? root.dictaTheme.red : root.dictaTheme.darkForeground
                                font.family: root.dictaTheme.fontFamily
                                font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
                                elide: Text.ElideRight
                            }
                            FlatButton {
                                objectName: "voiceNoteRecord"
                                visible: root.bridge.voiceNoteStatus.state !== "recording"
                                    && root.bridge.voiceNoteStatus.state !== "processing"
                                    && root.bridge.voiceNoteStatus.state !== "cancelling"
                                dictaTheme: root.dictaTheme
                                text: "RECORD VOICE"
                                iconName: "microphone"
                                enabled: root.bridge.runtimePhase === "idle"
                                onClicked: {
                                    if (playerLoader.active && playerLoader.item)
                                        playerLoader.item.pause()
                                    root.bridge.startVoiceNote(root.playbackPosition())
                                }
                            }
                            FlatButton {
                                objectName: "voiceNoteStop"
                                visible: root.bridge.voiceNoteStatus.state === "recording"
                                dictaTheme: root.dictaTheme
                                text: "STOP & TRANSCRIBE"
                                selected: true
                                onClicked: root.bridge.stopVoiceNote()
                            }
                            FlatButton {
                                objectName: "voiceNoteCancel"
                                visible: root.bridge.voiceNoteStatus.state === "recording"
                                    || root.bridge.voiceNoteStatus.state === "processing"
                                    || root.bridge.voiceNoteStatus.state === "cancelling"
                                dictaTheme: root.dictaTheme
                                text: "CANCEL"
                                quiet: true
                                onClicked: root.bridge.cancelVoiceNote()
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

    }

    Rectangle {
        objectName: "detailKeyboardBorder"
        anchors.fill: parent
        z: 90
        color: "transparent"
        border.width: root.keyboardActive ? 3 : 0
        border.color: root.dictaTheme.accent
        visible: root.keyboardActive
    }

    Popup {
        id: actionPopup
        objectName: "recordingActionPopup"
        x: root.width - width - 18 * root.dictaTheme.spacingScale
        y: 56 * root.dictaTheme.spacingScale
        z: 100
        width: 230 * root.dictaTheme.spacingScale
        padding: 6 * root.dictaTheme.spacingScale
        modal: false
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        onClosed: {
            root.confirmDelete = false
            root.keyboardFocusRequested()
        }
        onOpened: actionContent.forceActiveFocus()

        background: Rectangle {
            color: root.dictaTheme.darkBackground
            border.width: 1
            border.color: root.dictaTheme.muted
            radius: 4 * root.dictaTheme.spacingScale
        }

        contentItem: ColumnLayout {
            id: actionContent
            focus: true
            spacing: 3 * root.dictaTheme.spacingScale
            Keys.onPressed: event => {
                if ((event.key === Qt.Key_Return || event.key === Qt.Key_Enter)
                        && root.confirmDelete) {
                    root.confirmDeleteNow()
                    event.accepted = true
                }
            }
            FlatButton {
                Layout.fillWidth: true
                dictaTheme: root.dictaTheme
                text: "Open externally"
                iconName: "play"
                quiet: true
                onClicked: {
                    root.bridge.openSelectedRecording()
                    actionPopup.close()
                }
            }
            FlatButton {
                objectName: "transcribeRecording"
                Layout.fillWidth: true
                dictaTheme: root.dictaTheme
                text: root.recording.transcription_status === "failed"
                    ? "Retry transcription" : "Transcribe again"
                iconName: "microphone"
                quiet: true
                enabled: root.bridge.runtimePhase === "idle"
                    && Boolean(root.recording.success)
                onClicked: {
                    root.bridge.transcribeSelectedRecording()
                    actionPopup.close()
                }
            }
            FlatButton {
                Layout.fillWidth: true
                dictaTheme: root.dictaTheme
                text: "Reveal files"
                iconName: "folder-open"
                quiet: true
                onClicked: {
                    root.bridge.revealSelectedRecording()
                    actionPopup.close()
                }
            }
            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 1
                color: root.dictaTheme.muted
            }
            FlatButton {
                objectName: "deleteRecording"
                Layout.fillWidth: true
                dictaTheme: root.dictaTheme
                text: root.confirmDelete ? "Confirm delete" : "Delete recording"
                iconName: "clear"
                quiet: true
                destructive: true
                enabled: root.bridge.runtimePhase === "idle"
                onClicked: {
                    if (root.confirmDelete)
                        root.confirmDeleteNow()
                    else
                        root.promptDelete()
                }
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
