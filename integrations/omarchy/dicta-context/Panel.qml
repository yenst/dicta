import QtQuick
import QtQuick.Controls
import QtQuick.Effects
import QtQuick.Layouts
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui

Panel {
  id: root
  moduleName: "dicta.context"
  ipcTarget: "dicta.context"

  property string projectQuery: ""
  property string focusSection: "projects"
  property int projectIndex: 0
  property int contextIndex: 0
  property bool cursorActive: false

  readonly property color foreground: bar ? bar.foreground : Color.foreground
  readonly property color dim: Qt.darker(foreground, 1.55)
  readonly property color hoverFill: bar ? Style.hoverFillFor(bar.foreground, Color.accent) : "transparent"
  readonly property color selectedFill: bar ? Style.selectedFillFor(bar.foreground, Color.accent) : "transparent"
  readonly property string fontFamily: bar ? bar.fontFamily : Style.font.family
  readonly property var visibleProjects: filteredProjects()

  function filteredProjects() {
    var query = projectQuery.trim().toLowerCase()
    var result = []
    for (var i = 0; i < service.projects.length; i++) {
      var project = service.projects[i] || {}
      var haystack = (String(project.name || "") + " " + String(project.branch || "") + " " + String(project.path || "")).toLowerCase()
      if (query === "" || haystack.indexOf(query) !== -1) result.push(project)
    }
    result.sort(function(left, right) {
      var leftGeneral = String(left && left.id || "") === "__unprojected__"
      var rightGeneral = String(right && right.id || "") === "__unprojected__"
      if (leftGeneral !== rightGeneral) return leftGeneral ? -1 : 1
      return String(left && left.name || "").localeCompare(String(right && right.name || ""))
    })
    return result.slice(0, 3)
  }

  function setProjectCursor(index) {
    cursorActive = true
    focusSection = "projects"
    projectIndex = Math.max(0, Math.min(visibleProjects.length - 1, index))
  }

  function setContextCursor(index) {
    cursorActive = true
    focusSection = "contexts"
    contextIndex = Math.max(0, Math.min(service.contexts.length - 1, index))
  }

  function moveCursor(dy) {
    cursorActive = true
    if (dy === 0) return
    if (focusSection === "projects") {
      if (dy > 0 && projectIndex >= visibleProjects.length - 1 && service.contexts.length > 0) {
        focusSection = "contexts"
        contextIndex = 0
      } else {
        projectIndex = Math.max(0, Math.min(visibleProjects.length - 1, projectIndex + dy))
      }
    } else if (focusSection === "contexts") {
      if (dy < 0 && contextIndex === 0 && visibleProjects.length > 0) {
        focusSection = "projects"
        projectIndex = visibleProjects.length - 1
      } else {
        contextIndex = Math.max(0, Math.min(service.contexts.length - 1, contextIndex + dy))
      }
    }
  }

  function activateProject(project) {
    if (!project) return
    service.openProject(project.id)
  }

  function activateContext(context) {
    if (!context) return
    service.copyContext(service.selectedProjectId, context.id)
  }

  function activateCursor() {
    if (focusSection === "projects") activateProject(visibleProjects[projectIndex])
    else activateContext(service.contexts[contextIndex])
  }

  function openCursor() {
    if (focusSection === "projects") {
      var project = visibleProjects[projectIndex]
      if (project) service.openProject(project.id)
    } else {
      var context = service.contexts[contextIndex]
      if (context) service.openRecording(service.selectedProjectId, context.id)
    }
  }

  implicitWidth: barControls.implicitWidth
  implicitHeight: barControls.implicitHeight

  onOpenedChanged: if (opened) {
    projectQuery = ""
    projectIndex = 0
    contextIndex = 0
    cursorActive = false
    service.refresh(service.selectedProjectId)
    Qt.callLater(function() { projectSearch.forceActiveFocus() })
  }

  Service {
    id: service
    settings: root.settings
  }

  Row {
    id: barControls

    BarIconButton {
      id: panelButton
      visible: true
      interactive: false
      bar: root.bar
      tooltipText: service.dictaRunning
        ? "Dicta projects"
        : "Open Dicta"
      iconComponent: Component {
        Item {
          Image {
            anchors.fill: parent
            source: Qt.resolvedUrl("assets/dicta-mark-light.png")
            fillMode: Image.PreserveAspectFit
            sourceSize.width: Math.round(width * Screen.devicePixelRatio)
            sourceSize.height: Math.round(height * Screen.devicePixelRatio)
            smooth: true
            mipmap: true
          }
        }
      }

      MouseArea {
        id: barActionArea
        anchors.fill: parent
        z: 10
        acceptedButtons: Qt.LeftButton | Qt.RightButton | Qt.MiddleButton
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onEntered: if (root.bar) root.bar.showTooltip(panelButton, panelButton.tooltipText)
        onExited: if (root.bar) root.bar.hideTooltip(panelButton)
        onClicked: function(mouse) {
          if (root.bar) root.bar.hideTooltip(panelButton)
          if (mouse.button === Qt.RightButton) {
            service.openDicta()
          } else if (mouse.button === Qt.MiddleButton) {
            service.refresh(service.selectedProjectId)
          } else {
            root.toggle()
          }
        }
      }
    }
  }

  KeyboardPanel {
    id: panel
    anchorItem: panelButton
    owner: root
    bar: root.bar
    open: root.opened
    focusTarget: projectSearch
    contentWidth: panel.fittedContentWidth(Style.space(380))
    contentHeight: panel.fittedContentHeight(
      Math.max(contentColumn.implicitHeight, Style.space(500)),
      Style.space(600)
    )

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      blocked: projectSearch.activeFocus
      onMoveRequested: function(dx, dy) {
        if (dx > 0 && root.cursorActive) root.openCursor()
        else if (dy !== 0) root.moveCursor(dy)
      }
      onActivateRequested: if (root.cursorActive) root.activateCursor()
      onCloseRequested: root.close()
      onTabRequested: function(direction) { root.switchPanel(direction) }

      Flickable {
        id: panelFlick
        anchors.fill: parent
        contentWidth: width
        contentHeight: contentColumn.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        flickableDirection: Flickable.VerticalFlick
        interactive: contentHeight > height
        ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }

        Column {
          id: contentColumn
          width: panelFlick.width
          spacing: Style.space(12)

          PanelHero {
            width: parent.width
            title: "Dicta"
            meta: service.actionStatus !== "" ? service.actionStatus : "PROJECT CONTEXT"
            foreground: root.foreground
            fontFamily: root.fontFamily
            iconComponent: Component {
              Image {
                width: Style.space(38)
                height: Style.space(38)
                source: Qt.resolvedUrl("assets/dicta-mark-light.png")
                fillMode: Image.PreserveAspectFit
                sourceSize.width: Math.round(width * Screen.devicePixelRatio)
                sourceSize.height: Math.round(height * Screen.devicePixelRatio)
                smooth: true
                mipmap: true
              }
            }
            trailingControl: Component {
              HeaderActionButton {
                iconName: "window-new-symbolic"
                tooltipText: "Open Dicta"
                onClicked: service.openDicta()
              }
            }
          }

          PanelSeparator {
            foreground: root.foreground
          }

          Column {
            width: parent.width
            spacing: Style.space(8)

            PanelSectionHeader {
              text: "PROJECT"
              foreground: root.foreground
              fontFamily: root.fontFamily
            }

            Item {
              width: parent.width
              implicitHeight: projectSearch.implicitHeight

              TextField {
                id: projectSearch
                width: parent.width
                foreground: root.foreground
                placeholderText: "Search projects…"
                text: root.projectQuery
                leftPadding: Style.space(34)
                onTextChanged: {
                  root.projectQuery = text
                  root.projectIndex = 0
                  root.focusSection = "projects"
                }
                Keys.onPressed: function(event) {
                  if (event.key === Qt.Key_Down) {
                    root.setProjectCursor(0)
                    keyCatcher.forceActiveFocus()
                    event.accepted = true
                  } else if (event.key === Qt.Key_Escape) {
                    root.close()
                    event.accepted = true
                  } else if ((event.key === Qt.Key_Return || event.key === Qt.Key_Enter) && root.visibleProjects.length > 0) {
                    root.activateProject(root.visibleProjects[0])
                    event.accepted = true
                  }
                }
              }

              ThemeIcon {
                anchors.left: projectSearch.left
                anchors.leftMargin: Style.space(10)
                anchors.verticalCenter: projectSearch.verticalCenter
                width: Style.space(16)
                height: width
                iconName: "system-search-symbolic"
                iconColor: root.dim
              }
            }

            Column {
              width: parent.width
              spacing: 0

              Repeater {
                model: root.visibleProjects

                ProjectRow {
                  required property var modelData
                  required property int index
                  width: parent.width
                  project: modelData
                  rowIndex: index
                }
              }
            }

            Text {
              visible: !service.loading && root.visibleProjects.length === 0
              width: parent.width
              text: root.projectQuery === "" ? "No Dicta projects found." : "No matching projects."
              color: root.dim
              font.family: root.fontFamily
              font.pixelSize: Style.font.bodySmall
              horizontalAlignment: Text.AlignHCenter
              topPadding: Style.space(8)
              bottomPadding: Style.space(8)
            }
          }

          PanelSeparator {
            foreground: root.foreground
          }

          Column {
            width: parent.width
            spacing: Style.space(8)

            PanelSectionHeader {
              text: "RECENT CONTEXT"
              foreground: root.foreground
              fontFamily: root.fontFamily
            }

            Column {
              width: parent.width
              spacing: 0

              Repeater {
                model: service.contexts

                ContextRow {
                  required property var modelData
                  required property int index
                  width: parent.width
                  context: modelData
                  rowIndex: index
                }
              }
            }

            Text {
              visible: !service.loading && service.contexts.length === 0
              width: parent.width
              text: service.error !== "" ? service.error : "No context recorded for this project yet."
              color: service.error !== "" ? Color.urgent : root.dim
              font.family: root.fontFamily
              font.pixelSize: Style.font.bodySmall
              wrapMode: Text.WordWrap
              horizontalAlignment: Text.AlignHCenter
              topPadding: Style.space(8)
              bottomPadding: Style.space(8)
            }
          }
        }
      }
    }
  }

  component ProjectRow: CursorSurface {
    id: projectRow
    property var project: null
    property int rowIndex: 0
    readonly property bool selected: project && String(project.id || "") === service.selectedProjectId
    readonly property bool general: project && String(project.id || "") === "__unprojected__"
    readonly property bool recordingTarget: project && Boolean(project.selected)

    hasCursor: root.cursorActive && root.focusSection === "projects" && root.projectIndex === rowIndex
    current: selected
    foreground: root.foreground
    fill: root.hoverFill
    currentFill: root.selectedFill
    radius: 0
    bordered: true
    implicitHeight: Style.space(54)

    MouseArea {
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onEntered: root.setProjectCursor(projectRow.rowIndex)
      onClicked: root.activateProject(projectRow.project)
    }

    RowLayout {
      anchors.fill: parent
      anchors.leftMargin: Style.space(10)
      anchors.rightMargin: Style.space(10)
      spacing: Style.space(9)

      ThemeIcon {
        Layout.preferredWidth: Style.space(20)
        Layout.preferredHeight: Style.space(20)
        iconName: "folder-symbolic"
        iconColor: root.foreground
      }

      Text {
        Layout.preferredWidth: Style.space(88)
        text: projectRow.project ? String(projectRow.project.name || "") : ""
        color: root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
        font.bold: projectRow.selected
        elide: Text.ElideRight
      }

      Text {
        visible: !projectRow.general
        text: ""
        color: root.dim
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
        Layout.alignment: Qt.AlignVCenter
      }

      Text {
        Layout.fillWidth: true
        text: projectRow.project ? String(projectRow.project.branch || "") : ""
        color: root.dim
        font.family: root.fontFamily
        font.pixelSize: Style.font.bodySmall
        elide: Text.ElideMiddle
      }

      ThemeIcon {
        visible: projectRow.recordingTarget
        Layout.preferredWidth: visible ? Style.space(18) : 0
        Layout.preferredHeight: Style.space(18)
        iconName: "object-select-symbolic"
        iconColor: root.foreground
      }

      ThemeIcon {
        visible: projectRow.hasCursor
        Layout.preferredWidth: visible ? Style.space(18) : 0
        Layout.preferredHeight: Style.space(18)
        iconName: "document-open-symbolic"
        iconColor: root.foreground
      }
    }
  }

  component ContextRow: CursorSurface {
    id: contextRow
    property var context: null
    property int rowIndex: 0
    readonly property bool copied: context
      && String(context.id || "") === service.copiedContextId

    hasCursor: root.cursorActive && root.focusSection === "contexts" && root.contextIndex === rowIndex
    foreground: root.foreground
    fill: root.hoverFill
    radius: 0
    bordered: true
    implicitHeight: Style.space(50)

    MouseArea {
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onEntered: root.setContextCursor(contextRow.rowIndex)
      onClicked: root.activateContext(contextRow.context)
    }

    RowLayout {
      anchors.fill: parent
      anchors.leftMargin: Style.space(10)
      anchors.rightMargin: Style.space(10)
      spacing: Style.space(9)

      ThemeIcon {
        Layout.preferredWidth: Style.space(18)
        Layout.preferredHeight: Style.space(18)
        iconName: "x-office-document-symbolic"
        iconColor: root.foreground
      }

      Text {
        Layout.fillWidth: true
        text: contextRow.context ? String(contextRow.context.title || "Untitled recording") : ""
        color: root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.font.bodySmall
        font.bold: contextRow.rowIndex === 0
        elide: Text.ElideRight
      }

      Text {
        text: contextRow.context ? service.relativeTime(contextRow.context.startedAt) : ""
        color: root.dim
        font.family: root.fontFamily
        font.pixelSize: Style.font.caption
      }

      Text {
        visible: contextRow.copied
        text: "COPIED"
        color: root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.font.caption
        font.bold: true
      }

      ThemeIcon {
        visible: contextRow.hasCursor || contextRow.copied
        Layout.preferredWidth: visible ? Style.space(18) : 0
        Layout.preferredHeight: Style.space(18)
        iconName: contextRow.copied ? "object-select-symbolic" : "edit-copy-symbolic"
        iconColor: root.foreground
      }
    }
  }

  component HeaderActionButton: BorderSurface {
    id: headerButton
    property string iconName: ""
    property string tooltipText: ""
    signal clicked()

    implicitWidth: Style.space(30)
    implicitHeight: Style.space(30)
    color: headerMouse.containsMouse
      ? Style.hoverFillFor(root.foreground, Color.accent)
      : "transparent"
    borderSpec: Border.controlSpec(
      headerMouse.containsMouse ? "hover-cursor" : "normal",
      root.foreground,
      Color.accent
    )
    radius: Style.cornerRadius

    ThemeIcon {
      anchors.centerIn: parent
      width: Style.space(17)
      height: width
      iconName: headerButton.iconName
      iconColor: root.foreground
    }

    MouseArea {
      id: headerMouse
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onClicked: headerButton.clicked()
    }

    PanelToolTip {
      visible: headerMouse.containsMouse
      text: headerButton.tooltipText
      fontFamily: root.fontFamily
    }
  }

  component ThemeIcon: Item {
    id: themeIcon
    property string iconName: ""
    property color iconColor: root.foreground

    Image {
      id: iconSource
      anchors.fill: parent
      source: Quickshell.iconPath(themeIcon.iconName, true)
      fillMode: Image.PreserveAspectFit
      sourceSize.width: Math.round(width * Screen.devicePixelRatio)
      sourceSize.height: Math.round(height * Screen.devicePixelRatio)
      visible: false
      layer.enabled: true
    }

    MultiEffect {
      anchors.fill: iconSource
      source: iconSource
      colorization: 1.0
      colorizationColor: themeIcon.iconColor
    }
  }
}
