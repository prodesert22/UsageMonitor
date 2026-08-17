import QtQuick
import org.kde.kirigami as Kirigami

// Resolved look-and-feel for the widget.
//
// `spec` is the theme object emitted by the Python helper
// (`{mode, id, name, dark, colors, font, metrics}`). Mode "plasma" means
// "follow the desktop theme": every token then falls back to Kirigami.Theme,
// so the widget keeps the native Plasma look unless the user picked otherwise.
// Deliberately a plain object, not an Item.
//
// Kirigami's attached theme only initialises for a *visible* item: as a hidden
// zero-sized Item (the obvious way to park a non-visual helper in the tree)
// every role read back #000000 and "follow the desktop theme" painted the whole
// widget black. On a QtObject the attached theme resolves against the live
// platform palette instead. widgets/kde/tests/test_theme_palette_qml.py guards
// this.
QtObject {
    id: themePalette

    property var spec: ({})

    readonly property string mode: (spec && spec.mode) ? String(spec.mode) : "plasma"
    readonly property string name: (spec && spec.name) ? String(spec.name) : "Current desktop theme"
    readonly property bool themed: mode !== "plasma"
    readonly property bool dark: (spec && spec.dark === true)

    readonly property var specColors: (spec && spec.colors) ? spec.colors : ({})
    readonly property var specFont: (spec && spec.font) ? spec.font : ({})
    readonly property var specMetrics: (spec && spec.metrics) ? spec.metrics : ({})

    function colorOr(key, fallback) {
        var value = themePalette.specColors[key]
        return (themePalette.themed && value) ? value : fallback
    }

    function sizeOr(key, fallback) {
        var value = Number(themePalette.specFont[key] || 0)
        return (themePalette.themed && value > 0) ? value : fallback
    }

    function metricOr(key, fallback) {
        var value = Number(themePalette.specMetrics[key])
        return (themePalette.themed && !isNaN(value) && value > 0) ? value : fallback
    }

    readonly property color backgroundColor: colorOr("background", Kirigami.Theme.backgroundColor)
    readonly property color textColor: colorOr("text", Kirigami.Theme.textColor)
    readonly property color subtextColor: colorOr("subtext", Kirigami.Theme.disabledTextColor)
    readonly property color accentColor: colorOr("accent", "#0a84ff")
    readonly property color warningColor: colorOr("warning", "#ff9f0a")
    readonly property color criticalColor: colorOr("critical", "#ff453a")
    readonly property color trackColor: colorOr("track", Kirigami.Theme.disabledTextColor)
    readonly property color borderColor: colorOr("border", Kirigami.Theme.disabledTextColor)
    readonly property color errorColor: themed ? criticalColor : Kirigami.Theme.negativeTextColor

    // Roles the popup re-supplies to its Kirigami.Theme (it turns inheritance
    // off): with a theme they follow the palette, otherwise they resolve to the
    // very values inheritance would have provided.
    // Selection/highlight for the popup subtree. accentColor keeps its fixed
    // fallback because it also colours the usage bars, but this one must fall
    // back to the desktop's own highlight, or "follow the current desktop theme"
    // would repaint selection with our blue instead of the system's.
    //
    // The other Kirigami roles are deliberately not exposed: overriding them in
    // the popup made Plasma paint the scrollbar handle with the visited-link
    // colour. Anything not listed here keeps inheriting.
    readonly property color highlightColor: themed ? accentColor : Kirigami.Theme.highlightColor

    // The track is drawn at full opacity for a themed palette (the colour was
    // chosen deliberately) but stays a faint tint of the Plasma text colour
    // otherwise, which is how the widget looked before theming existed.
    readonly property real trackOpacity: themed ? 1.0 : 0.22
    readonly property real dividerOpacity: themed ? 0.45 : 0.18

    readonly property string fontFamily: {
        var family = String(themePalette.specFont.family || "")
        return (themed && family) ? family : Kirigami.Theme.defaultFont.family
    }
    readonly property real fontSize: sizeOr("size", Kirigami.Theme.defaultFont.pointSize)
    readonly property real smallFontSize: sizeOr("smallSize", Kirigami.Theme.smallFont.pointSize)
    readonly property real headingFontSize: sizeOr("headingSize", fontSize + 3)
    // Provider names sit one step above the body text, always — deriving this
    // from headingSize made every card title jump to the popup-heading size as
    // soon as a custom theme set one.
    readonly property real cardTitleFontSize: fontSize + 1

    readonly property real barHeight: metricOr("barHeight", 6)
    readonly property real cornerRadius: metricOr("radius", 4)

    // Transparency is global (it also applies to "follow the desktop theme"),
    // so unlike the other metrics it is read even when themed is false.
    readonly property real backgroundOpacity: {
        var value = Number(themePalette.specMetrics.opacity)
        return (!isNaN(value) && value > 0) ? Math.min(1.0, value) : 1.0
    }
    readonly property bool translucent: backgroundOpacity < 1.0
    // Whether the widget paints a background of its own: for a theme palette, or
    // to tint the desktop palette when transparency is in play.
    readonly property bool paintsBackground: themed || translucent

    function levelColor(percent) {
        var pct = Number(percent) || 0
        if (pct >= 90) {
            return themePalette.criticalColor
        }
        if (pct >= 70) {
            return themePalette.warningColor
        }
        return themePalette.accentColor
    }
}
