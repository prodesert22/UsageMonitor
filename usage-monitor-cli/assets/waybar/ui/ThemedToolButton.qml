import QtQuick
import QtQuick.Controls as QQC2

// Icon-only toolbar button, ported from the Plasma widget.
//
// Same reasoning as there — every colour comes from the widget palette rather
// than the platform style — but the icon is a GlyphIcon instead of
// Kirigami.Icon, because a Waybar host may have no icon theme at all.
QQC2.AbstractButton {
    id: control

    // ThemePalette instance.
    property var ui: null
    property string iconName: "refresh"
    property int iconSize: control.ui ? Math.round(control.ui.iconSize) : 18
    property string tooltipText: ""

    readonly property color themeTextColor: control.ui ? control.ui.textColor : "#f5f5f7"
    readonly property color themeAccentColor: control.ui ? control.ui.accentColor : "#0a84ff"
    readonly property color themeHoverColor: control.ui ? control.ui.borderColor : "#48484a"

    // Background tint for the current interaction state; a function so it can be
    // checked without a pointer device.
    function stateOpacity(isHovered, isActive) {
        if (isActive) {
            return 0.45
        }
        return isHovered ? 0.3 : 0.0
    }

    padding: control.ui ? control.ui.smallSpacing : 4
    implicitWidth: iconSize + padding * 2
    implicitHeight: iconSize + padding * 2
    hoverEnabled: true

    Accessible.role: Accessible.Button
    Accessible.name: text

    QQC2.ToolTip.visible: hovered && control.tooltipText.length > 0
    QQC2.ToolTip.text: control.tooltipText
    QQC2.ToolTip.delay: 400

    background: Rectangle {
        readonly property bool active: control.down || control.checked
        radius: control.ui ? control.ui.cornerRadius : 4
        color: active ? control.themeAccentColor : control.themeHoverColor
        opacity: control.stateOpacity(control.hovered, active)
    }

    contentItem: GlyphIcon {
        name: control.iconName
        color: control.themeTextColor
        implicitWidth: control.iconSize
        implicitHeight: control.iconSize
        opacity: control.enabled ? 1.0 : 0.5
    }
}
