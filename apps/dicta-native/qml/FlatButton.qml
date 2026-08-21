pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Button {
    id: root

    required property QtObject dictaTheme
    property string iconName: ""
    property bool selected: false
    property bool destructive: false
    property bool quiet: false
    property bool iconOnly: false
    property bool centerContent: false
    property bool leftAlignContent: false
    property string toolTip: ""

    hoverEnabled: true
    focusPolicy: Qt.StrongFocus
    implicitHeight: Math.round(34 * dictaTheme.spacingScale)
    implicitWidth: iconOnly
        ? implicitHeight
        : Math.max(implicitHeight, contentRow.implicitWidth + 20 * dictaTheme.spacingScale)
    leftPadding: 10 * dictaTheme.spacingScale
    rightPadding: 10 * dictaTheme.spacingScale
    topPadding: 6 * dictaTheme.spacingScale
    bottomPadding: 6 * dictaTheme.spacingScale

    contentItem: RowLayout {
        id: contentRow
        spacing: 7 * root.dictaTheme.spacingScale

        Item {
            visible: root.centerContent && !root.iconOnly
            Layout.fillWidth: true
        }

        ThemeIcon {
            visible: root.iconName.length > 0
            Layout.alignment: Qt.AlignVCenter
            Layout.preferredWidth: iconSize
            Layout.preferredHeight: iconSize
            Layout.maximumWidth: iconSize
            Layout.maximumHeight: iconSize
            iconName: root.iconName
            iconColor: !root.enabled ? root.dictaTheme.darkForeground
                : root.destructive ? root.dictaTheme.red
                : root.selected ? root.dictaTheme.accent
                : root.dictaTheme.foreground
            iconSize: Math.round(root.dictaTheme.baseFontSize + 4)
        }

        Text {
            visible: !root.iconOnly
            Layout.fillWidth: !root.centerContent && !root.leftAlignContent
            text: root.text
            color: !root.enabled ? root.dictaTheme.darkForeground
                : root.destructive ? root.dictaTheme.red
                : root.selected ? root.dictaTheme.accent
                : root.dictaTheme.foreground
            font.family: root.dictaTheme.fontFamily
            font.pixelSize: root.dictaTheme.baseFontSize
            font.weight: root.selected ? Font.DemiBold : Font.Normal
            horizontalAlignment: root.leftAlignContent
                ? Text.AlignLeft : Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
            elide: Text.ElideRight
        }

        Item {
            visible: root.centerContent && !root.iconOnly
            Layout.fillWidth: true
        }
    }

    background: Rectangle {
        radius: 3 * root.dictaTheme.spacingScale
        color: !root.enabled || root.quiet && !root.hovered && !root.activeFocus
            ? "transparent"
            : root.down
                ? Qt.rgba(root.dictaTheme.accent.r, root.dictaTheme.accent.g,
                    root.dictaTheme.accent.b, 0.22)
                : root.selected
                    ? Qt.rgba(root.dictaTheme.accent.r, root.dictaTheme.accent.g,
                        root.dictaTheme.accent.b, 0.14)
                    : root.hovered || root.activeFocus
                        ? Qt.rgba(root.dictaTheme.foreground.r, root.dictaTheme.foreground.g,
                            root.dictaTheme.foreground.b, 0.07)
                        : "transparent"
        border.width: root.quiet ? (root.activeFocus ? 1 : 0) : 1
        border.color: root.selected || root.activeFocus
            ? Qt.rgba(root.dictaTheme.accent.r, root.dictaTheme.accent.g,
                root.dictaTheme.accent.b, 0.7)
            : Qt.rgba(root.dictaTheme.muted.r, root.dictaTheme.muted.g,
                root.dictaTheme.muted.b, 0.75)
    }

    ToolTip.visible: hovered && toolTip.length > 0
    ToolTip.text: toolTip
    ToolTip.delay: 500
}
