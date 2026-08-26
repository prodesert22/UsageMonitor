import QtQuick

// Resolved look-and-feel for the popup — the Plasma widget's ThemePalette,
// ported to plain Qt Quick.
//
// `spec` is the theme object emitted by the Python helper
// (`{mode, id, name, dark, colors, font, metrics}`). The difference from the
// Plasma version: there is no Kirigami.Theme to fall back to here, so the helper
// always resolves concrete colors (mode "system" reads the desktop's own
// palette) and this object only supplies constants for a malformed/partial spec.
//
// Deliberately a QtObject: it is non-visual, and parking it as a hidden Item in
// the tree only costs a scene-graph node.
QtObject {
    id: themePalette

    property var spec: ({})

    readonly property string mode: (spec && spec.mode) ? String(spec.mode) : "system"
    readonly property string name: (spec && spec.name) ? String(spec.name) : "Desktop colors"
    readonly property bool dark: (spec && spec.dark === true)

    readonly property var specColors: (spec && spec.colors) ? spec.colors : ({})
    readonly property var specFont: (spec && spec.font) ? spec.font : ({})
    readonly property var specMetrics: (spec && spec.metrics) ? spec.metrics : ({})

    function colorOr(key, fallback) {
        var value = themePalette.specColors[key]
        return value ? value : fallback
    }

    function sizeOr(key, fallback) {
        var value = Number(themePalette.specFont[key] || 0)
        return value > 0 ? value : fallback
    }

    function metricOr(key, fallback) {
        var value = Number(themePalette.specMetrics[key])
        return (!isNaN(value) && value > 0) ? value : fallback
    }

    readonly property color backgroundColor: colorOr("background", "#1c1c1e")
    readonly property color textColor: colorOr("text", "#f5f5f7")
    readonly property color subtextColor: colorOr("subtext", "#98989d")
    readonly property color accentColor: colorOr("accent", "#0a84ff")
    readonly property color warningColor: colorOr("warning", "#ff9f0a")
    readonly property color criticalColor: colorOr("critical", "#ff453a")
    readonly property color trackColor: colorOr("track", "#3a3a3c")
    readonly property color borderColor: colorOr("border", "#48484a")
    readonly property color errorColor: criticalColor
    readonly property color highlightColor: accentColor

    readonly property real trackOpacity: 1.0
    readonly property real dividerOpacity: 0.45

    readonly property string fontFamily: {
        var family = String(themePalette.specFont.family || "")
        return family ? family : Qt.application.font.family
    }
    readonly property real fontSize: sizeOr("size", Qt.application.font.pointSize > 0
                                                    ? Qt.application.font.pointSize : 10)
    readonly property real smallFontSize: sizeOr("smallSize", Math.max(6, fontSize - 1))
    readonly property real headingFontSize: sizeOr("headingSize", fontSize + 3)
    // Provider names sit one step above the body text, always.
    readonly property real cardTitleFontSize: fontSize + 1

    readonly property real barHeight: metricOr("barHeight", 6)
    readonly property real cornerRadius: metricOr("radius", 8)

    // Kirigami.Units replacement: everything scales off the text size, so a
    // theme with a bigger font gets proportional spacing (Plasma got this from
    // Kirigami; a bare Qt Quick app has to derive it).
    readonly property real gridUnit: Math.max(12, Math.round(fontSize * 2))
    readonly property real smallSpacing: Math.max(2, Math.round(gridUnit / 4))
    readonly property real largeSpacing: Math.max(4, Math.round(gridUnit / 2))
    readonly property real iconSize: Math.round(gridUnit * 1.1)

    // The popup is a frameless window painting its own background, so unlike the
    // plasmoid it always draws the layer; only how transparent it is varies.
    readonly property real backgroundOpacity: {
        var value = Number(themePalette.specMetrics.opacity)
        return (!isNaN(value) && value > 0) ? Math.min(1.0, value) : 1.0
    }
    readonly property bool translucent: backgroundOpacity < 1.0

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
