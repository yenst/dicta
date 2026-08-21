pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Window
import QtMultimedia

Item {
    id: root
    objectName: "recordingPlayer"

    property url source
    property url posterSource
    property QtObject dictaTheme
    readonly property real positionSeconds: player.position / 1000

    function seek(seconds) {
        player.setPosition(Math.max(0, Number(seconds) || 0) * 1000)
    }

    function pause() {
        player.pause()
    }

    function togglePlayback() {
        if (player.playbackState === MediaPlayer.PlayingState)
            player.pause()
        else
            player.play()
    }

    function duration(value) {
        var total = Math.max(0, Math.round(Number(value) / 1000 || 0))
        return String(Math.floor(total / 60)).padStart(2, "0") + ":"
            + String(total % 60).padStart(2, "0")
    }

    Rectangle {
        anchors.fill: parent
        color: root.dictaTheme ? root.dictaTheme.darkerBackground : "#101010"
    }

    MediaPlayer {
        id: player
        source: root.source
        videoOutput: videoOutput
        audioOutput: AudioOutput { id: audioOutput }
        onSourceChanged: {
            stop()
            setPosition(0)
        }
    }

    VideoOutput {
        id: videoOutput
        anchors.fill: parent
        anchors.bottomMargin: 42 * (root.dictaTheme ? root.dictaTheme.spacingScale : 1)
        fillMode: VideoOutput.PreserveAspectFit
    }

    Image {
        anchors.fill: videoOutput
        visible: source.toString().length > 0 && player.position <= 0
        source: root.posterSource
        fillMode: Image.PreserveAspectFit
        asynchronous: true
        cache: true
    }

    RowLayout {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.margins: 8
        spacing: 10 * (root.dictaTheme ? root.dictaTheme.spacingScale : 1)

        FlatButton {
            dictaTheme: root.dictaTheme
            iconName: player.playbackState === MediaPlayer.PlayingState ? "pause" : "play"
            iconOnly: true
            quiet: true
            toolTip: player.playbackState === MediaPlayer.PlayingState ? "Pause" : "Play"
            onClicked: root.togglePlayback()
        }
        Text {
            text: root.duration(player.position) + " / " + root.duration(player.duration)
            color: root.dictaTheme ? root.dictaTheme.foreground : "#dddddd"
            font.family: root.dictaTheme ? root.dictaTheme.fontFamily : "monospace"
            font.pixelSize: root.dictaTheme ? root.dictaTheme.baseFontSize - 1 : 11
        }
        Slider {
            id: scrubber
            Layout.fillWidth: true
            from: 0
            to: Math.max(1, player.duration)
            value: player.position
            onMoved: player.setPosition(value)
            background: Rectangle {
                x: scrubber.leftPadding
                y: scrubber.topPadding + scrubber.availableHeight / 2 - height / 2
                width: scrubber.availableWidth
                height: 2
                color: root.dictaTheme ? root.dictaTheme.muted : "#555555"
                Rectangle {
                    width: parent.width * player.position / Math.max(1, player.duration)
                    height: parent.height
                    color: root.dictaTheme ? root.dictaTheme.accent : "#7aa2f7"
                }
            }
        }
        FlatButton {
            dictaTheme: root.dictaTheme
            iconName: audioOutput.muted ? "muted" : "volume"
            iconOnly: true
            quiet: true
            toolTip: audioOutput.muted ? "Unmute" : "Mute"
            onClicked: audioOutput.muted = !audioOutput.muted
        }
        FlatButton {
            dictaTheme: root.dictaTheme
            iconName: root.Window.window
                && root.Window.window.visibility === Window.FullScreen
                ? "restore" : "fullscreen"
            iconOnly: true
            quiet: true
            toolTip: "Toggle fullscreen"
            onClicked: {
                var window = root.Window.window
                if (!window)
                    return
                window.visibility = window.visibility === Window.FullScreen
                    ? Window.Windowed : Window.FullScreen
            }
        }
    }

    Text {
        anchors.centerIn: parent
        visible: player.error !== MediaPlayer.NoError
        width: parent.width - 32
        horizontalAlignment: Text.AlignHCenter
        wrapMode: Text.Wrap
        text: player.errorString
        color: root.dictaTheme ? root.dictaTheme.red : "#e9b4b4"
        font.family: root.dictaTheme ? root.dictaTheme.fontFamily : "monospace"
        font.pixelSize: root.dictaTheme ? root.dictaTheme.baseFontSize : 12
    }

    Component.onDestruction: player.stop()
}
