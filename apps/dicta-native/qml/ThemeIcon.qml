import QtQuick

Image {
    id: root

    required property string iconName
    property color iconColor: "white"
    property int iconSize: 16

    width: iconSize
    height: iconSize
    source: iconName.length > 0
        ? "image://dicta-icons/" + iconName + "?color="
            + encodeURIComponent(String(iconColor))
        : ""
    sourceSize.width: iconSize
    sourceSize.height: iconSize
    fillMode: Image.PreserveAspectFit
    smooth: true
    mipmap: true
}
