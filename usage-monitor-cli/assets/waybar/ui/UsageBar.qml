import QtQuick

Item {
    id: bar

    property real value: 0
    // ThemePalette instance; when unset the bar keeps its original colours.
    property var ui: null
    property color fillColor: bar.ui ? bar.ui.levelColor(value)
                                     : (value >= 90 ? "#ff453a" : (value >= 70 ? "#ff9f0a" : "#0a84ff"))

    implicitHeight: bar.ui ? bar.ui.barHeight : 6
    implicitWidth: 180

    Rectangle {
        anchors.fill: parent
        radius: height / 2
        color: bar.ui ? bar.ui.trackColor : "#3a3a3c"
        opacity: bar.ui ? bar.ui.trackOpacity : 0.22
    }

    Rectangle {
        width: Math.max(0, Math.min(100, bar.value)) / 100 * parent.width
        height: parent.height
        radius: height / 2
        color: bar.fillColor

        Behavior on width {
            NumberAnimation { duration: 160; easing.type: Easing.OutCubic }
        }
    }
}
