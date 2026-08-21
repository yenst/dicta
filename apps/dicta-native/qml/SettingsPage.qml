pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts

Rectangle {
    id: root
    objectName: "settingsPage"

    required property QtObject bridge
    required property QtObject dictaTheme
    required property var host

    color: dictaTheme.background
    focus: visible
    onVisibleChanged: {
        if (visible)
            forceActiveFocus()
    }
    Keys.onPressed: event => {
        if (event.key === Qt.Key_Escape || event.key === Qt.Key_Left) {
            host.settingsOpen = false
            event.accepted = true
        }
    }

    component SettingsMcpCopyRow: RowLayout {
        required property string clientId
        required property string clientLabel
        required property string clientDetail

        Layout.fillWidth: true
        Layout.preferredHeight: 62 * root.dictaTheme.spacingScale
        spacing: 12 * root.dictaTheme.spacingScale
        Rectangle {
            Layout.preferredWidth: 42 * root.dictaTheme.spacingScale
            Layout.preferredHeight: width
            radius: 7 * root.dictaTheme.spacingScale
            color: Qt.rgba(root.dictaTheme.foreground.r,
                root.dictaTheme.foreground.g,
                root.dictaTheme.foreground.b, 0.07)
            Text {
                anchors.centerIn: parent
                text: clientLabel.substring(0, 1)
                color: root.dictaTheme.brightForeground
                font.family: root.dictaTheme.fontFamily
                font.pixelSize: root.dictaTheme.baseFontSize + 3
                font.weight: Font.DemiBold
            }
        }
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 2 * root.dictaTheme.spacingScale
            Text {
                text: clientLabel
                color: root.dictaTheme.brightForeground
                font.family: root.dictaTheme.fontFamily
                font.pixelSize: root.dictaTheme.baseFontSize + 1
                font.weight: Font.DemiBold
            }
            Text {
                Layout.fillWidth: true
                text: clientDetail
                color: root.dictaTheme.darkForeground
                font.family: root.dictaTheme.fontFamily
                font.pixelSize: Math.max(9, root.dictaTheme.baseFontSize - 1)
                elide: Text.ElideRight
            }
        }
        FlatButton {
            objectName: "copy-" + clientId + "-mcp-config"
            dictaTheme: root.dictaTheme
            text: "COPY CONFIG"
            iconName: "copy"
            enabled: Boolean(root.bridge.codexMcp.mcp_path)
            onClicked: root.host.copyMcpConfig(clientLabel)
        }
    }

    component SettingsNavItem: FlatButton {
        required property string sectionId
        required property string sectionLabel
        required property string sectionIcon

        width: settingsNavColumn.width
        height: 46 * root.dictaTheme.spacingScale
        dictaTheme: root.dictaTheme
        text: sectionLabel
        iconName: sectionIcon
        quiet: true
        leftAlignContent: true
        selected: root.host.settingsSection === sectionId
        onClicked: root.host.settingsSection = sectionId
    }

    Rectangle {
        id: settingsNavigation
        anchors.left: parent.left
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        width: Math.min(230 * dictaTheme.spacingScale,
            Math.max(184 * dictaTheme.spacingScale, parent.width * 0.24))
        color: dictaTheme.darkBackground
        clip: true

        Column {
            id: settingsNavColumn
            anchors.fill: parent
            anchors.margins: 14 * dictaTheme.spacingScale
            spacing: 6 * dictaTheme.spacingScale

            FlatButton {
                objectName: "settingsBack"
                width: parent.width
                height: 44 * dictaTheme.spacingScale
                dictaTheme: root.dictaTheme
                text: "Settings"
                iconName: "back"
                quiet: true
                leftAlignContent: true
                onClicked: host.settingsOpen = false
            }

            Rectangle {
                width: parent.width
                height: 1
                color: Qt.rgba(dictaTheme.muted.r, dictaTheme.muted.g,
                    dictaTheme.muted.b, 0.55)
            }

            SettingsNavItem {
                objectName: "settings-section-appearance"
                sectionId: "appearance"
                sectionLabel: "Appearance"
                sectionIcon: "appearance"
            }
            SettingsNavItem {
                objectName: "settings-section-connections"
                sectionId: "connections"
                sectionLabel: "Connections"
                sectionIcon: "connections"
            }
            SettingsNavItem {
                objectName: "settings-section-shortcuts"
                sectionId: "shortcuts"
                sectionLabel: "Shortcuts"
                sectionIcon: "shortcuts"
            }
            SettingsNavItem {
                objectName: "settings-section-transcription"
                sectionId: "transcription"
                sectionLabel: "Transcription"
                sectionIcon: "transcription"
            }
            SettingsNavItem {
                objectName: "settings-section-storage"
                sectionId: "storage"
                sectionLabel: "Storage"
                sectionIcon: "storage"
            }
        }

        Rectangle {
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.bottom: parent.bottom
            width: 1
            color: Qt.rgba(dictaTheme.muted.r, dictaTheme.muted.g,
                dictaTheme.muted.b, 0.55)
        }
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.leftMargin: settingsNavigation.width
            + 34 * dictaTheme.spacingScale
        anchors.rightMargin: 34 * dictaTheme.spacingScale
        anchors.topMargin: 30 * dictaTheme.spacingScale
        anchors.bottomMargin: 24 * dictaTheme.spacingScale
        spacing: 10 * dictaTheme.spacingScale

        RowLayout {
            Layout.fillWidth: true
            Text {
                Layout.fillWidth: true
                text: host.settingsTitle(host.settingsSection)
                color: dictaTheme.brightForeground
                font.family: dictaTheme.fontFamily
                font.pixelSize: dictaTheme.baseFontSize + 5
                font.weight: Font.DemiBold
            }
        }

        Text {
            Layout.fillWidth: true
            text: host.settingsDescription(host.settingsSection)
            color: dictaTheme.darkForeground
            font.family: dictaTheme.fontFamily
            font.pixelSize: dictaTheme.baseFontSize
            wrapMode: Text.Wrap
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 1
            color: dictaTheme.muted
        }

        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            contentWidth: availableWidth

            GridLayout {
                width: parent.width
                columns: 1
                columnSpacing: 18 * dictaTheme.spacingScale
                rowSpacing: 18 * dictaTheme.spacingScale

                Rectangle {
                    objectName: "settingsAppearanceCard"
                    visible: host.settingsSection === "appearance"
                    Layout.fillWidth: true
                    Layout.preferredHeight: 218 * dictaTheme.spacingScale
                    radius: 3 * dictaTheme.spacingScale
                    color: dictaTheme.darkBackground
                    border.width: 1
                    border.color: dictaTheme.muted
                    Column {
                        anchors.fill: parent
                        anchors.margins: 16 * dictaTheme.spacingScale
                        spacing: 7 * dictaTheme.spacingScale
                        Text {
                            width: parent.width
                            text: "APPEARANCE"
                            color: dictaTheme.darkForeground
                            font.family: dictaTheme.fontFamily
                            font.pixelSize: Math.max(9, dictaTheme.baseFontSize - 1)
                            font.weight: Font.DemiBold
                        }
                        Text {
                            width: parent.width
                            text: dictaTheme.appearance === "system"
                                ? "Follows desktop · " + dictaTheme.name
                                : dictaTheme.name
                            color: dictaTheme.brightForeground
                            font.family: dictaTheme.fontFamily
                            font.pixelSize: dictaTheme.baseFontSize + 1
                            elide: Text.ElideMiddle
                        }
                        Text {
                            width: parent.width
                            text: dictaTheme.fontFamily + " · "
                                + dictaTheme.baseFontSize + " px\n"
                                + (dictaTheme.appearance === "system"
                                    ? "Desktop theme changes reload live."
                                    : "Built-in themes work on any Linux desktop.")
                            color: dictaTheme.darkForeground
                            font.family: dictaTheme.fontFamily
                            font.pixelSize: Math.max(9, dictaTheme.baseFontSize - 1)
                            wrapMode: Text.Wrap
                        }
                        Flow {
                            width: parent.width
                            spacing: 6 * dictaTheme.spacingScale
                            FlatButton {
                                objectName: "appearance-system"
                                dictaTheme: root.dictaTheme
                                text: "DESKTOP"
                                selected: dictaTheme.appearance === "system"
                                onClicked: host.updateAppearance("system")
                            }
                            FlatButton {
                                objectName: "appearance-dark"
                                dictaTheme: root.dictaTheme
                                text: "DICTA DARK"
                                selected: dictaTheme.appearance === "dark"
                                onClicked: host.updateAppearance("dark")
                            }
                            FlatButton {
                                objectName: "appearance-light"
                                dictaTheme: root.dictaTheme
                                text: "DICTA LIGHT"
                                selected: dictaTheme.appearance === "light"
                                onClicked: host.updateAppearance("light")
                            }
                        }
                    }
                }

                Rectangle {
                    objectName: "settingsTranscriptionCard"
                    visible: host.settingsSection === "transcription"
                    Layout.fillWidth: true
                    Layout.preferredHeight: 345 * dictaTheme.spacingScale
                    radius: 3 * dictaTheme.spacingScale
                    color: dictaTheme.darkBackground
                    border.width: 1
                    border.color: dictaTheme.muted
                    ColumnLayout {
                        anchors.fill: parent
                        anchors.margins: 16 * dictaTheme.spacingScale
                        spacing: 8 * dictaTheme.spacingScale
                        Text {
                            text: "CURRENT ENGINE"
                            color: dictaTheme.darkForeground
                            font.family: dictaTheme.fontFamily
                            font.pixelSize: Math.max(9, dictaTheme.baseFontSize - 1)
                            font.weight: Font.DemiBold
                        }
                        Rectangle {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 66 * dictaTheme.spacingScale
                            radius: 3 * dictaTheme.spacingScale
                            color: dictaTheme.background
                            border.width: 1
                            border.color: dictaTheme.muted
                            RowLayout {
                                anchors.fill: parent
                                anchors.margins: 12 * dictaTheme.spacingScale
                                spacing: 10 * dictaTheme.spacingScale
                                Rectangle {
                                    Layout.preferredWidth: 9 * dictaTheme.spacingScale
                                    Layout.preferredHeight: width
                                    radius: width / 2
                                    color: bridge.modelStatus.quality_state === "ready"
                                        ? dictaTheme.green : dictaTheme.yellow
                                }
                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 1
                                    Text {
                                        text: bridge.modelStatus.active_model
                                            || "Compact fallback"
                                        color: dictaTheme.brightForeground
                                        font.family: dictaTheme.fontFamily
                                        font.pixelSize: dictaTheme.baseFontSize
                                        font.weight: Font.DemiBold
                                    }
                                    Text {
                                        Layout.fillWidth: true
                                        text: bridge.modelStatus.message || "Local Whisper"
                                        color: dictaTheme.darkForeground
                                        font.family: dictaTheme.fontFamily
                                        font.pixelSize: Math.max(9,
                                            dictaTheme.baseFontSize - 1)
                                        elide: Text.ElideRight
                                    }
                                }
                                FlatButton {
                                    objectName: "installQualityModel"
                                    dictaTheme: root.dictaTheme
                                    text: bridge.modelStatus.quality_state === "ready"
                                        ? "ACTIVE"
                                        : bridge.modelStatus.quality_state === "installing"
                                            ? "INSTALLING…" : "INSTALL QUALITY"
                                    selected: bridge.modelStatus.quality_state === "ready"
                                    enabled: bridge.hostState === "running"
                                        && bridge.runtimePhase === "idle"
                                        && bridge.modelStatus.quality_state !== "ready"
                                        && bridge.modelStatus.quality_state !== "installing"
                                    onClicked: host.installQualityModel()
                                }
                            }
                        }
                        Text {
                            text: "AVAILABLE MODELS"
                            color: dictaTheme.darkForeground
                            font.family: dictaTheme.fontFamily
                            font.pixelSize: Math.max(9, dictaTheme.baseFontSize - 1)
                            font.weight: Font.DemiBold
                        }
                        Rectangle {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 66 * dictaTheme.spacingScale
                            radius: 3 * dictaTheme.spacingScale
                            color: dictaTheme.background
                            border.width: 1
                            border.color: dictaTheme.muted
                            RowLayout {
                                anchors.fill: parent
                                anchors.margins: 12 * dictaTheme.spacingScale
                                spacing: 10 * dictaTheme.spacingScale
                                ThemeIcon {
                                    Layout.preferredWidth: 22 * dictaTheme.spacingScale
                                    Layout.preferredHeight: width
                                    iconName: "transcription"
                                    iconColor: dictaTheme.darkForeground
                                    iconSize: Math.round(width)
                                }
                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 1
                                    Text {
                                        Layout.fillWidth: true
                                        text: "Compact  ·  Included"
                                        color: dictaTheme.brightForeground
                                        font.family: dictaTheme.fontFamily
                                        font.pixelSize: dictaTheme.baseFontSize
                                        font.weight: Font.DemiBold
                                        elide: Text.ElideRight
                                    }
                                    Text {
                                        Layout.fillWidth: true
                                        text: "Fast offline fallback for rough transcripts."
                                        color: dictaTheme.darkForeground
                                        font.family: dictaTheme.fontFamily
                                        font.pixelSize: Math.max(9,
                                            dictaTheme.baseFontSize - 1)
                                        elide: Text.ElideMiddle
                                    }
                                }
                                Text {
                                    text: "57 MB"
                                    color: dictaTheme.darkForeground
                                    font.family: dictaTheme.fontFamily
                                    font.pixelSize: Math.max(9,
                                        dictaTheme.baseFontSize - 1)
                                }
                            }
                        }
                        Text {
                            text: "TRANSCRIPT LANGUAGE"
                            color: dictaTheme.darkForeground
                            font.family: dictaTheme.fontFamily
                            font.pixelSize: Math.max(9, dictaTheme.baseFontSize - 1)
                            font.weight: Font.DemiBold
                        }
                        Flow {
                            Layout.fillWidth: true
                            spacing: 5 * dictaTheme.spacingScale
                            Repeater {
                                model: ["auto", "nl", "en", "fr", "de", "es"]
                                Item {
                                    required property string modelData
                                    implicitWidth: languageButton.implicitWidth
                                    implicitHeight: languageButton.implicitHeight
                                    width: implicitWidth
                                    height: implicitHeight
                                    FlatButton {
                                        id: languageButton
                                        objectName: "language-" + parent.modelData
                                        anchors.fill: parent
                                        dictaTheme: root.dictaTheme
                                        text: parent.modelData.toUpperCase()
                                        selected: bridge.settings.transcription_language
                                            === parent.modelData
                                        onClicked: host.updateTranscriptionLanguage(
                                            parent.modelData)
                                    }
                                }
                            }
                        }
                        RowLayout {
                            Layout.fillWidth: true
                            Text {
                                text: bridge.modelStatus.install_stage
                                    ? bridge.modelStatus.install_stage + " · "
                                        + Math.round(Number(
                                            bridge.modelStatus.downloaded_bytes || 0)
                                            / 1048576) + " MiB"
                                    : "Models are stored locally and work offline."
                                color: dictaTheme.darkForeground
                                font.family: dictaTheme.fontFamily
                                font.pixelSize: Math.max(9,
                                    dictaTheme.baseFontSize - 1)
                                elide: Text.ElideRight
                            }
                        }
                    }
                }

                Rectangle {
                    objectName: "settingsStorageCard"
                    visible: host.settingsSection === "storage"
                    Layout.fillWidth: true
                    Layout.preferredHeight: 310 * dictaTheme.spacingScale
                    radius: 3 * dictaTheme.spacingScale
                    color: dictaTheme.darkBackground
                    border.width: 1
                    border.color: dictaTheme.muted
                    ColumnLayout {
                        anchors.fill: parent
                        anchors.margins: 16 * dictaTheme.spacingScale
                        spacing: 8 * dictaTheme.spacingScale
                        Text {
                            text: "STORAGE"
                            color: dictaTheme.darkForeground
                            font.family: dictaTheme.fontFamily
                            font.pixelSize: Math.max(9, dictaTheme.baseFontSize - 1)
                            font.weight: Font.DemiBold
                        }
                        RowLayout {
                            Layout.fillWidth: true
                            Text {
                                Layout.fillWidth: true
                                text: "Lock Git recordings to branches"
                                color: dictaTheme.foreground
                                font.family: dictaTheme.fontFamily
                                font.pixelSize: dictaTheme.baseFontSize
                            }
                            FlatButton {
                                objectName: "branchLockingToggle"
                                dictaTheme: root.dictaTheme
                                text: bridge.settings.branch_locking ? "ON" : "OFF"
                                selected: bridge.settings.branch_locking
                                onClicked: host.updateBranchLocking(
                                    !bridge.settings.branch_locking)
                            }
                        }
                        RowLayout {
                            Layout.fillWidth: true
                            Text {
                                Layout.fillWidth: true
                                text: "Clean videos from merged branches"
                                color: dictaTheme.foreground
                                font.family: dictaTheme.fontFamily
                                font.pixelSize: dictaTheme.baseFontSize
                            }
                            FlatButton {
                                objectName: "cleanupToggle"
                                dictaTheme: root.dictaTheme
                                text: bridge.settings.cleanup_merged_videos ? "ON" : "OFF"
                                selected: bridge.settings.cleanup_merged_videos
                                onClicked: host.updateCleanupPolicy(
                                    !bridge.settings.cleanup_merged_videos)
                            }
                        }
                        Text {
                            text: "General recordings path"
                            color: dictaTheme.darkForeground
                            font.family: dictaTheme.fontFamily
                            font.pixelSize: Math.max(9, dictaTheme.baseFontSize - 1)
                        }
                        RowLayout {
                            Layout.fillWidth: true
                            TextField {
                                id: generalPathField
                                objectName: "generalPathField"
                                Layout.fillWidth: true
                                text: bridge.settings.general_path || ""
                                placeholderText: "Default · ~/Documents/Dicta/General"
                                color: dictaTheme.foreground
                                placeholderTextColor: dictaTheme.darkForeground
                                font.family: dictaTheme.fontFamily
                                font.pixelSize: dictaTheme.baseFontSize
                                selectByMouse: true
                                background: Rectangle {
                                    color: dictaTheme.background
                                    border.width: 1
                                    border.color: generalPathField.activeFocus
                                        ? dictaTheme.accent : dictaTheme.muted
                                    radius: 3 * dictaTheme.spacingScale
                                }
                            }
                            FlatButton {
                                objectName: "generalPathBrowse"
                                dictaTheme: root.dictaTheme
                                text: "BROWSE"
                                onClicked: {
                                    var candidate = generalPathField.text.trim()
                                    if (!candidate.length)
                                        candidate = bridge.currentProject.path || "/"
                                    generalFolderDialog.currentFolder =
                                        "file://" + encodeURI(candidate)
                                    generalFolderDialog.open()
                                }
                            }
                            FlatButton {
                                objectName: "generalPathApply"
                                dictaTheme: root.dictaTheme
                                text: generalPathField.text.trim().length ? "APPLY" : "DEFAULT"
                                onClicked: host.updateGeneralPath(generalPathField.text)
                            }
                        }
                        RowLayout {
                            Layout.fillWidth: true
                            FlatButton {
                                objectName: "cleanupNow"
                                dictaTheme: root.dictaTheme
                                text: "CLEAN MERGED VIDEOS"
                                enabled: bridge.settings.cleanup_merged_videos
                                onClicked: host.cleanupMergedVideos()
                            }
                            Text {
                                Layout.fillWidth: true
                                text: bridge.settingsMessage
                                color: dictaTheme.accent
                                font.family: dictaTheme.fontFamily
                                font.pixelSize: Math.max(9, dictaTheme.baseFontSize - 1)
                                elide: Text.ElideRight
                            }
                        }
                    }
                }

                Rectangle {
                    objectName: "settingsConnectionsCard"
                    visible: host.settingsSection === "connections"
                    Layout.fillWidth: true
                    Layout.preferredHeight: 330 * dictaTheme.spacingScale
                    radius: 3 * dictaTheme.spacingScale
                    color: dictaTheme.darkBackground
                    border.width: 1
                    border.color: dictaTheme.muted
                    ColumnLayout {
                        anchors.fill: parent
                        anchors.margins: 20 * dictaTheme.spacingScale
                        spacing: 10 * dictaTheme.spacingScale
                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 12 * dictaTheme.spacingScale

                            Rectangle {
                                Layout.preferredWidth: 42 * dictaTheme.spacingScale
                                Layout.preferredHeight: width
                                radius: 7 * dictaTheme.spacingScale
                                color: Qt.rgba(dictaTheme.accent.r,
                                    dictaTheme.accent.g,
                                    dictaTheme.accent.b, 0.11)
                                ThemeIcon {
                                    anchors.centerIn: parent
                                    width: 21 * dictaTheme.spacingScale
                                    height: width
                                    iconName: "connections"
                                    iconColor: dictaTheme.accent
                                    iconSize: Math.round(width)
                                }
                            }

                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 2 * dictaTheme.spacingScale
                                Text {
                                    Layout.fillWidth: true
                                    text: "Codex"
                                    color: dictaTheme.brightForeground
                                    font.family: dictaTheme.fontFamily
                                    font.pixelSize: dictaTheme.baseFontSize + 2
                                    font.weight: Font.DemiBold
                                }
                                Text {
                                    Layout.fillWidth: true
                                    text: bridge.codexMcp.state === "connected"
                                        ? "Connected" : "Not connected"
                                    color: bridge.codexMcp.state === "connected"
                                        ? dictaTheme.green
                                        : dictaTheme.darkForeground
                                    font.family: dictaTheme.fontFamily
                                    font.pixelSize: Math.max(9,
                                        dictaTheme.baseFontSize - 1)
                                }
                            }

                            FlatButton {
                                objectName: "mcpActionButton"
                                dictaTheme: root.dictaTheme
                                text: bridge.codexMcp.state === "disconnected"
                                    ? "CONNECT"
                                    : bridge.codexMcp.state === "connected"
                                        ? "RECONNECT" : "REPLACE"
                                selected: bridge.codexMcp.state === "connected"
                                enabled: Boolean(bridge.codexMcp.codex_path)
                                    && Boolean(bridge.codexMcp.mcp_path)
                                onClicked: {
                                    if (bridge.codexMcp.state === "disconnected")
                                        bridge.connectCodexMcp()
                                    else
                                        bridge.restartCodexMcp()
                                }
                            }
                        }
                        Text {
                            Layout.fillWidth: true
                            visible: bridge.codexMcp.state !== "connected"
                            text: bridge.codexMcp.message || "Checking Codex…"
                            color: bridge.codexMcp.state === "connected"
                                ? dictaTheme.accent : dictaTheme.darkForeground
                            font.family: dictaTheme.fontFamily
                            font.pixelSize: Math.max(9, dictaTheme.baseFontSize - 1)
                            wrapMode: Text.Wrap
                        }
                        Rectangle {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 1
                            color: dictaTheme.muted
                        }
                        SettingsMcpCopyRow {
                            clientId: "claude"
                            clientLabel: "Claude"
                            clientDetail: "Claude-compatible MCP configuration"
                        }
                        SettingsMcpCopyRow {
                            clientId: "grok"
                            clientLabel: "Grok"
                            clientDetail: "Grok-compatible MCP configuration"
                        }
                        Item { Layout.fillHeight: true }
                    }
                }

                Rectangle {
                    objectName: "settingsShortcutsCard"
                    visible: host.settingsSection === "shortcuts"
                    Layout.fillWidth: true
                    Layout.preferredHeight: 215 * dictaTheme.spacingScale
                    radius: 3 * dictaTheme.spacingScale
                    color: dictaTheme.darkBackground
                    border.width: 1
                    border.color: dictaTheme.muted
                    Column {
                        anchors.fill: parent
                        anchors.margins: 16 * dictaTheme.spacingScale
                        spacing: 8 * dictaTheme.spacingScale
                        Text {
                            text: "RECORDING SHORTCUT"
                            color: dictaTheme.darkForeground
                            font.family: dictaTheme.fontFamily
                            font.pixelSize: Math.max(9, dictaTheme.baseFontSize - 1)
                            font.weight: Font.DemiBold
                        }
                        Flow {
                            width: parent.width
                            spacing: 6 * dictaTheme.spacingScale
                            FlatButton {
                                objectName: "shortcut-alt_shift_r"
                                dictaTheme: root.dictaTheme
                                text: "ALT SHIFT R"
                                selected: bridge.settings.shortcut_id === "alt_shift_r"
                                onClicked: host.updateShortcut("alt_shift_r")
                            }
                            FlatButton {
                                objectName: "shortcut-command_shift_d"
                                dictaTheme: root.dictaTheme
                                text: "SUPER SHIFT D"
                                selected: bridge.settings.shortcut_id === "command_shift_d"
                                onClicked: host.updateShortcut("command_shift_d")
                            }
                            FlatButton {
                                objectName: "shortcut-option_space"
                                dictaTheme: root.dictaTheme
                                text: "ALT SPACE"
                                selected: bridge.settings.shortcut_id === "option_space"
                                onClicked: host.updateShortcut("option_space")
                            }
                            FlatButton {
                                objectName: "shortcut-control_space"
                                dictaTheme: root.dictaTheme
                                text: "CTRL SPACE"
                                selected: bridge.settings.shortcut_id === "control_space"
                                onClicked: host.updateShortcut("control_space")
                            }
                        }
                        Text {
                            width: parent.width
                            text: "Current · " + (bridge.settings.shortcut_id || "alt_shift_r")
                            color: dictaTheme.accent
                            font.family: dictaTheme.fontFamily
                            font.pixelSize: dictaTheme.baseFontSize
                            wrapMode: Text.Wrap
                        }
                    }
                }
            }
        }
    }

    FolderDialog {
        id: generalFolderDialog
        title: "Choose the General recordings folder"
        onAccepted: {
            var path = host.pathFromFolderUrl(selectedFolder)
            generalPathField.text = path
            host.updateGeneralPath(path)
        }
    }
}
