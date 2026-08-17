import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami

// Icon-only toolbar button for the popup.
//
// A plain QQC2.ToolButton is styled by qqc2-desktop-style, which paints the icon
// and the hover/pressed background from the *desktop* color scheme — so on a
// light custom theme with a dark Plasma scheme the icons render white and
// disappear (and a dark scheme hides them again on hover). That style paints
// everything inside its background delegate, so overriding just the background
// collapses the button to nothing. Hence an AbstractButton with our own icon and
// background: every colour comes from the widget palette, which falls back to
// Kirigami.Theme when the user follows the desktop theme.
QQC2.AbstractButton {
    id: control

    // ThemePalette instance.
    property var ui: null
    property int iconSize: Kirigami.Units.iconSizes.smallMedium

    readonly property color themeTextColor: control.ui ? control.ui.textColor : Kirigami.Theme.textColor
    readonly property color themeAccentColor: control.ui ? control.ui.accentColor : Kirigami.Theme.highlightColor
    readonly property color themeHoverColor: (control.ui && control.ui.themed) ? control.ui.borderColor : Kirigami.Theme.hoverColor

    // Background tint for the current interaction state; a function so it can be
    // checked without a pointer device.
    function stateOpacity(isHovered, isActive) {
        if (isActive) {
            return 0.45
        }
        return isHovered ? 0.3 : 0.0
    }

    padding: Kirigami.Units.smallSpacing
    implicitWidth: iconSize + padding * 2
    implicitHeight: iconSize + padding * 2
    hoverEnabled: true

    Accessible.role: Accessible.Button
    Accessible.name: text

    background: Rectangle {
        readonly property bool active: control.down || control.checked
        radius: control.ui ? control.ui.cornerRadius : Kirigami.Units.smallSpacing
        color: active ? control.themeAccentColor : control.themeHoverColor
        opacity: control.stateOpacity(control.hovered, active)
    }

    contentItem: Kirigami.Icon {
        source: control.icon.name
        color: control.themeTextColor
        isMask: true
        implicitWidth: control.iconSize
        implicitHeight: control.iconSize
        opacity: control.enabled ? 1.0 : 0.5
    }
}
