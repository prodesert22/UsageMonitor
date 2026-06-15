import QtQuick
import org.kde.kirigami as Kirigami

Item {
    id: root

    property real value: 0
    property color fillColor: value >= 90 ? "#ff453a" : (value >= 70 ? "#ff9f0a" : "#0a84ff")

    implicitHeight: 6
    implicitWidth: 180

    Rectangle {
        anchors.fill: parent
        radius: height / 2
        color: Kirigami.Theme.disabledTextColor
        opacity: 0.22
    }

    Rectangle {
        width: Math.max(0, Math.min(100, root.value)) / 100 * parent.width
        height: parent.height
        radius: height / 2
        color: root.fillColor
    }
}
