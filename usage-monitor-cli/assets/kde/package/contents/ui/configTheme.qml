import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kcmutils as KCM
import org.kde.kirigami as Kirigami
import org.kde.plasma.components as PlasmaComponents3

KCM.SimpleKCM {
    id: page

    // Same contract as the other config pages: pending edits are applied by the
    // native Apply/OK buttons, which call saveConfig().
    signal configurationChanged

    property var pending: ({})

    // Labels only; the key list itself comes from the helper (`colorKeys`), so a
    // token added there shows up here instead of being silently dropped.
    readonly property var colorLabels: ({
        "background": "Background",
        "text": "Text",
        "subtext": "Secondary text",
        "accent": "Accent (<70%)",
        "warning": "Warning (70–89%)",
        "critical": "Critical (90%+)",
        "track": "Bar track",
        "border": "Borders"
    })
    readonly property var colorFields: {
        var keys = page.catalog.colorKeys || Object.keys(page.colorLabels)
        return keys.map(function(key) {
            return { "key": key, "label": page.colorLabels[key] || key }
        })
    }
    readonly property var modeIds: ["plasma", "builtin", "scheme", "custom"]

    readonly property var catalog: backend.settings.themeCatalog || ({ "builtin": [], "schemes": [] })
    readonly property var builtinThemes: catalog.builtin || []
    readonly property var installedSchemes: catalog.schemes || []
    readonly property string mode: page.stateValue("themeMode", "plasma")

    leftPadding: Kirigami.Units.gridUnit * 2
    rightPadding: Kirigami.Units.gridUnit * 2
    topPadding: Kirigami.Units.gridUnit
    bottomPadding: Kirigami.Units.gridUnit

    function setPending(key, value) {
        // A fresh object on every edit: reassigning the same reference does not
        // invalidate the bindings that read page.pending, so the mode sections
        // and the preview would only catch up after the dialog was reopened.
        var p = {}
        for (var k in page.pending) {
            p[k] = page.pending[k]
        }
        p[key] = String(value)
        page.pending = p
        page.configurationChanged()
    }

    // Pending edit first, then the saved state key, then the built-in default.
    function stateValue(key, fallback) {
        if (key in page.pending) {
            return page.pending[key]
        }
        var saved = backend.settings.themeState || ({})
        var value = saved[key]
        return (value !== undefined && String(value) !== "") ? String(value) : fallback
    }

    // Slider works in "transparency", state.json stores opacity.
    function themeOpacity() {
        var value = Number(page.stateValue("themeOpacity", "1"))
        if (isNaN(value) || value <= 0) {
            return 1
        }
        return Math.min(1, value)
    }

    function transparencyPercent() {
        return Math.round((1 - page.themeOpacity()) * 100)
    }

    function themeById(list, id) {
        for (var i = 0; i < list.length; i++) {
            if (list[i].id === id) return list[i]
        }
        return null
    }

    // ---- named custom themes ------------------------------------------------
    //
    // The whole list lives in one state key (`customThemes`, a JSON array) so a
    // save is a single atomic write; `themeCustomId` picks the active one. Edits
    // go through the same pending map as everything else, so Cancel discards
    // them and Apply writes once.

    function customList() {
        var raw = page.stateValue("customThemes", "")
        if (raw) {
            try {
                var parsed = JSON.parse(raw)
                if (Array.isArray(parsed)) return parsed
            } catch (e) {
                // fall through to the saved catalog
            }
        }
        return page.catalog.custom || []
    }

    function customListCopy() {
        return JSON.parse(JSON.stringify(page.customList()))
    }

    function storeCustomList(list) {
        page.setPending("customThemes", JSON.stringify(list))
    }

    function selectedCustomId() {
        var list = page.customList()
        var wanted = page.stateValue("themeCustomId", "")
        for (var i = 0; i < list.length; i++) {
            if (list[i].id === wanted) return wanted
        }
        return list.length > 0 ? list[0].id : ""
    }

    function selectedCustom() {
        return page.themeById(page.customList(), page.selectedCustomId())
    }

    function selectedBaseColors() {
        var theme = page.selectedCustom()
        var baseId = theme ? theme.base : page.stateValue("themeBuiltin", "macos-dark")
        var builtin = page.themeById(page.builtinThemes, baseId)
        return builtin ? builtin.colors : ({})
    }

    function selectedColor(key) {
        var theme = page.selectedCustom()
        return (theme && theme.colors && theme.colors[key]) ? String(theme.colors[key]) : ""
    }

    // section "" reads a top-level field (name/base); otherwise font/metrics.
    function selectedText(section, key, fallback) {
        var theme = page.selectedCustom()
        if (!theme) {
            return fallback
        }
        var holder = (section === "") ? theme : theme[section]
        var value = holder ? holder[key] : undefined
        return (value === undefined || value === null) ? fallback : String(value)
    }

    function selectedNumber(section, key, fallback) {
        var value = Number(page.selectedText(section, key, fallback))
        return isNaN(value) ? Number(fallback) : value
    }

    function updateSelected(section, key, value) {
        var list = page.customListCopy()
        var id = page.selectedCustomId()
        for (var i = 0; i < list.length; i++) {
            if (list[i].id !== id) {
                continue
            }
            if (section === "") {
                list[i][key] = value
            } else {
                if (!list[i][section]) {
                    list[i][section] = ({})
                }
                list[i][section][key] = value
            }
            page.storeCustomList(list)
            return
        }
    }

    function addCustomTheme() {
        var list = page.customListCopy()
        var id = "custom-" + Date.now()
        list.push({
            "id": id,
            "name": "My theme " + (list.length + 1),
            "base": page.stateValue("themeBuiltin", "macos-dark"),
            "colors": ({}),
            "font": { "family": "", "size": 0, "headingSize": 0, "smallSize": 0 },
            "metrics": { "barHeight": 6, "radius": 4, "opacity": 1 }
        })
        page.storeCustomList(list)
        page.setPending("themeCustomId", id)
    }

    function removeSelectedTheme() {
        var id = page.selectedCustomId()
        var list = page.customListCopy().filter(function(theme) { return theme.id !== id })
        page.storeCustomList(list)
        page.setPending("themeCustomId", list.length > 0 ? list[0].id : "")
    }

    // Mirror of the helper's resolve_theme(), so the preview updates while the
    // dialog is still open (the saved palette only arrives after Apply).
    function previewSpec() {
        var spec = page.modeSpec()
        // Copy the metrics: they may point straight at the saved catalog, and
        // transparency is global, overriding whatever the theme carries.
        var metrics = ({})
        var source = spec.metrics || ({})
        for (var key in source) {
            metrics[key] = source[key]
        }
        metrics.opacity = page.themeOpacity()
        spec.metrics = metrics
        return spec
    }

    function modeSpec() {
        var mode = page.stateValue("themeMode", "plasma")
        if (mode === "builtin") {
            var builtin = themeById(page.builtinThemes, page.stateValue("themeBuiltin", "macos-dark"))
            if (builtin) {
                // font/metrics ship with the catalog entry, so the preview draws
                // the same corner radius and sizes the widget will apply.
                return { "mode": "builtin", "name": builtin.name, "dark": builtin.dark,
                         "colors": builtin.colors, "font": builtin.font || ({}),
                         "metrics": builtin.metrics || ({}) }
            }
        } else if (mode === "scheme") {
            var scheme = themeById(page.installedSchemes, page.stateValue("themeScheme", ""))
            if (scheme && scheme.colors) {
                return { "mode": "scheme", "name": scheme.name, "dark": scheme.dark,
                         "colors": scheme.colors, "font": ({}), "metrics": ({}) }
            }
        } else if (mode === "custom") {
            var theme = page.selectedCustom()
            var colors = ({})
            var base = page.selectedBaseColors()
            for (var k in base) {
                colors[k] = base[k]
            }
            var overrides = (theme && theme.colors) ? theme.colors : ({})
            for (var ck in overrides) {
                if (overrides[ck]) colors[ck] = overrides[ck]
            }
            return {
                "mode": "custom",
                "name": theme ? theme.name : "Custom",
                "colors": colors,
                "font": (theme && theme.font) ? theme.font : ({}),
                "metrics": (theme && theme.metrics) ? theme.metrics : ({})
            }
        }
        return { "mode": "plasma", "name": "Current desktop theme", "colors": ({}), "font": ({}), "metrics": ({}) }
    }

    // Seed the selected theme's colours from its base, so "start from this theme
    // and tweak it" is one click instead of eight hex values.
    function copyBaseIntoSelected() {
        var base = page.selectedBaseColors()
        var colors = ({})
        for (var key in base) {
            colors[key] = base[key]
        }
        page.updateSelected("", "colors", colors)
    }

    function saveConfig() {
        backend.batchSetState(page.pending)
        // Fold the pending edits into the cached state before clearing them:
        // otherwise the page (and its preview) would snap back to the previous
        // theme until the slow `settings` reload lands.
        var settings = backend.settings
        var state = ({})
        var saved = settings.themeState || ({})
        for (var savedKey in saved) {
            state[savedKey] = saved[savedKey]
        }
        for (var key in page.pending) {
            state[key] = page.pending[key]
        }
        settings.themeState = state
        backend.settings = settings
        page.pending = ({})
    }

    SettingsBackend {
        id: backend
    }

    ThemePalette {
        id: previewPalette
        spec: page.previewSpec()
    }

    // Loaded only when the user asks for the font chooser; if QtQuick.Dialogs
    // is missing the status turns to Error and the button hides itself.
    Loader {
        id: fontDialogLoader
        active: false
        source: Qt.resolvedUrl("FontPicker.qml")
        onLoaded: item.picked.connect(function(family) {
            page.updateSelected("font", "family", family)
        })
    }

    ColumnLayout {
        width: parent.width
        spacing: Kirigami.Units.largeSpacing

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 0

            PlasmaComponents3.Label {
                text: "Theme"
                opacity: 0.8
                font.pointSize: Kirigami.Theme.smallFont.pointSize
            }

            QQC2.ComboBox {
                id: modeBox
                Layout.fillWidth: true
                model: ["Follow the current desktop theme",
                        "Built-in theme",
                        "Installed KDE theme / color scheme",
                        "Custom"]
                currentIndex: Math.max(0, page.modeIds.indexOf(page.mode))
                onActivated: (index) => page.setPending("themeMode", page.modeIds[index])
            }
        }

        // Transparency applies to every mode, including "follow the current
        // desktop theme", where the widget tints with the inherited colours.
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 0

            PlasmaComponents3.Label {
                text: "Transparency"
                opacity: 0.8
                font.pointSize: Kirigami.Theme.smallFont.pointSize
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.largeSpacing

                QQC2.Slider {
                    id: transparencySlider
                    Layout.fillWidth: true
                    from: 0
                    to: 70
                    stepSize: 5
                    snapMode: QQC2.Slider.SnapAlways

                    // Dragging assigns to `value` and destroys a plain binding,
                    // after which Reset (or a settings reload) could no longer
                    // move the handle. Track the stored value separately and
                    // push it in whenever it changes.
                    readonly property real storedValue: page.transparencyPercent()
                    value: storedValue
                    onStoredValueChanged: if (value !== storedValue) value = storedValue
                    onMoved: page.setPending("themeOpacity", ((100 - value) / 100).toFixed(2))
                }

                PlasmaComponents3.Label {
                    Layout.preferredWidth: Kirigami.Units.gridUnit * 3
                    text: Math.round(transparencySlider.value) + " %"
                    horizontalAlignment: Text.AlignRight
                }

                QQC2.ToolButton {
                    text: "Reset"
                    icon.name: "edit-undo"
                    display: QQC2.AbstractButton.IconOnly
                    enabled: transparencySlider.value > 0
                    onClicked: page.setPending("themeOpacity", "1")

                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: "Back to a solid background"
                }
            }

            PlasmaComponents3.Label {
                Layout.fillWidth: true
                text: "Only the background fades — text, icons and bars stay solid. At 0 % the popup uses the standard Plasma dialog; above it the widget draws its own borderless window so the desktop really shows through (no Plasma shadow or blur behind it)."
                wrapMode: Text.WordWrap
                opacity: 0.6
                font.pointSize: Kirigami.Theme.smallFont.pointSize
            }
        }

        ColumnLayout {
            visible: page.mode === "builtin"
            Layout.fillWidth: true
            spacing: 0

            PlasmaComponents3.Label {
                text: "Built-in theme"
                opacity: 0.8
                font.pointSize: Kirigami.Theme.smallFont.pointSize
            }

            QQC2.ComboBox {
                Layout.fillWidth: true
                model: page.builtinThemes.map(function(t) { return t.name })
                currentIndex: {
                    var id = page.stateValue("themeBuiltin", "macos-dark")
                    for (var i = 0; i < page.builtinThemes.length; i++) {
                        if (page.builtinThemes[i].id === id) return i
                    }
                    return 0
                }
                onActivated: (index) => page.setPending("themeBuiltin", page.builtinThemes[index].id)
            }
        }

        ColumnLayout {
            visible: page.mode === "scheme"
            Layout.fillWidth: true
            spacing: 0

            PlasmaComponents3.Label {
                text: "Installed KDE theme"
                opacity: 0.8
                font.pointSize: Kirigami.Theme.smallFont.pointSize
            }

            QQC2.ComboBox {
                Layout.fillWidth: true
                enabled: page.installedSchemes.length > 0
                model: page.installedSchemes.map(function(s) { return s.name + (s.dark ? " (dark)" : " (light)") })
                currentIndex: {
                    var id = page.stateValue("themeScheme", "")
                    for (var i = 0; i < page.installedSchemes.length; i++) {
                        if (page.installedSchemes[i].id === id) return i
                    }
                    return 0
                }
                onActivated: (index) => page.setPending("themeScheme", page.installedSchemes[index].id)
            }

            PlasmaComponents3.Label {
                Layout.fillWidth: true
                text: page.installedSchemes.length > 0
                      ? "Color schemes from System Settings and the KDE Store (for example the macOS look-alikes) are listed here."
                      : "No color schemes found. Install one from System Settings → Colors → Get New Color Schemes."
                wrapMode: Text.WordWrap
                opacity: 0.65
                font.pointSize: Kirigami.Theme.smallFont.pointSize
            }
        }

        // ---- Custom editor -------------------------------------------------

        ColumnLayout {
            id: customEditor
            visible: page.mode === "custom"
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            readonly property bool hasTheme: page.selectedCustom() !== null

            // ---- theme picker + add/remove ---------------------------------

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 0

                PlasmaComponents3.Label {
                    text: "Your themes"
                    opacity: 0.8
                    font.pointSize: Kirigami.Theme.smallFont.pointSize
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: Kirigami.Units.smallSpacing

                    QQC2.ComboBox {
                        Layout.fillWidth: true
                        enabled: page.customList().length > 0
                        model: page.customList().map(function(theme) { return theme.name })
                        currentIndex: {
                            var list = page.customList()
                            var id = page.selectedCustomId()
                            for (var i = 0; i < list.length; i++) {
                                if (list[i].id === id) return i
                            }
                            return -1
                        }
                        displayText: page.customList().length > 0 ? currentText : "No custom theme yet"
                        onActivated: (index) => page.setPending("themeCustomId", page.customList()[index].id)
                    }

                    QQC2.ToolButton {
                        text: "New"
                        icon.name: "list-add"
                        display: QQC2.AbstractButton.TextBesideIcon
                        onClicked: page.addCustomTheme()

                        QQC2.ToolTip.visible: hovered
                        QQC2.ToolTip.text: "Create a theme from the current built-in base"
                    }

                    QQC2.ToolButton {
                        text: "Delete"
                        icon.name: "list-remove"
                        display: QQC2.AbstractButton.TextBesideIcon
                        enabled: customEditor.hasTheme
                        onClicked: page.removeSelectedTheme()

                        QQC2.ToolTip.visible: hovered
                        QQC2.ToolTip.text: "Delete the selected theme (applied on Apply)"
                    }
                }
            }

            PlasmaComponents3.Label {
                Layout.fillWidth: true
                visible: !customEditor.hasTheme
                text: "No custom theme yet — click New to create one, then edit its name, colors and fonts."
                wrapMode: Text.WordWrap
                opacity: 0.7
                font.pointSize: Kirigami.Theme.smallFont.pointSize
            }

            RowLayout {
                Layout.fillWidth: true
                visible: customEditor.hasTheme
                spacing: Kirigami.Units.largeSpacing

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0

                    PlasmaComponents3.Label {
                        text: "Name"
                        opacity: 0.8
                        font.pointSize: Kirigami.Theme.smallFont.pointSize
                    }

                    QQC2.TextField {
                        id: themeNameField
                        Layout.fillWidth: true
                        placeholderText: "Theme name"

                        // Typing breaks a plain `text:` binding, so switching
                        // themes would leave the old name in the field — and
                        // write it into the newly selected theme on the next
                        // edit. Re-assign whenever the stored value changes.
                        readonly property string storedValue: page.selectedText("", "name", "")
                        text: storedValue
                        onStoredValueChanged: if (text !== storedValue) text = storedValue
                        onEditingFinished: {
                            var name = text.trim()
                            if (name && name !== storedValue) {
                                page.updateSelected("", "name", name)
                            }
                        }
                    }
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0

                    PlasmaComponents3.Label {
                        text: "Base theme (colours you do not override)"
                        opacity: 0.8
                        font.pointSize: Kirigami.Theme.smallFont.pointSize
                    }

                    QQC2.ComboBox {
                        Layout.fillWidth: true
                        model: page.builtinThemes.map(function(theme) { return theme.name })
                        currentIndex: {
                            var id = page.selectedText("", "base", "macos-dark")
                            for (var i = 0; i < page.builtinThemes.length; i++) {
                                if (page.builtinThemes[i].id === id) return i
                            }
                            return 0
                        }
                        onActivated: (index) => page.updateSelected("", "base", page.builtinThemes[index].id)
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true
                visible: customEditor.hasTheme

                PlasmaComponents3.Label {
                    Layout.fillWidth: true
                    text: "Colors — hex values such as #1c1c1e; leave empty to use the base theme."
                    wrapMode: Text.WordWrap
                    opacity: 0.7
                    font.pointSize: Kirigami.Theme.smallFont.pointSize
                }

                QQC2.ToolButton {
                    text: "Copy from base theme"
                    icon.name: "edit-copy"
                    display: QQC2.AbstractButton.TextBesideIcon
                    onClicked: page.copyBaseIntoSelected()
                }
            }

            GridLayout {
                Layout.fillWidth: true
                visible: customEditor.hasTheme
                columns: 2
                columnSpacing: Kirigami.Units.largeSpacing
                rowSpacing: Kirigami.Units.smallSpacing

                Repeater {
                    model: page.colorFields

                    delegate: RowLayout {
                        id: colorRow
                        required property var modelData
                        Layout.fillWidth: true
                        spacing: Kirigami.Units.smallSpacing

                        // See themeNameField: typing kills the binding, so the
                        // stored value is pushed back in explicitly.
                        readonly property string currentValue: page.selectedColor(colorRow.modelData.key)
                        onCurrentValueChanged: if (colorField.text !== currentValue) colorField.text = currentValue

                        Rectangle {
                            Layout.preferredWidth: Kirigami.Units.gridUnit
                            Layout.preferredHeight: Kirigami.Units.gridUnit
                            radius: 3
                            border.width: 1
                            border.color: Kirigami.Theme.disabledTextColor
                            color: {
                                var preview = previewPalette.specColors[colorRow.modelData.key]
                                return preview ? preview : "transparent"
                            }
                        }

                        PlasmaComponents3.Label {
                            Layout.preferredWidth: Kirigami.Units.gridUnit * 8
                            text: colorRow.modelData.label
                            elide: Text.ElideRight
                            font.pointSize: Kirigami.Theme.smallFont.pointSize
                        }

                        QQC2.TextField {
                            id: colorField
                            Layout.fillWidth: true
                            Layout.minimumWidth: Kirigami.Units.gridUnit * 5
                            text: colorRow.currentValue
                            placeholderText: page.selectedBaseColors()[colorRow.modelData.key] || "#rrggbb"
                            inputMethodHints: Qt.ImhNoAutoUppercase
                            onEditingFinished: {
                                if (text !== colorRow.currentValue) {
                                    page.updateSelected("colors", colorRow.modelData.key, text.trim())
                                }
                            }
                        }
                    }
                }
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.topMargin: Kirigami.Units.smallSpacing
                Layout.preferredHeight: 1
                visible: customEditor.hasTheme
                color: Kirigami.Theme.disabledTextColor
                opacity: 0.18
            }

            RowLayout {
                Layout.fillWidth: true
                visible: customEditor.hasTheme
                spacing: Kirigami.Units.largeSpacing

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0

                    PlasmaComponents3.Label {
                        text: "Font"
                        opacity: 0.8
                        font.pointSize: Kirigami.Theme.smallFont.pointSize
                    }

                    // Deliberately a text field plus an on-demand dialog: a
                    // ComboBox listing every installed family locks up Plasma
                    // while the style measures the ~2000 entries.
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Kirigami.Units.smallSpacing

                        QQC2.TextField {
                            id: fontFamilyField
                            Layout.fillWidth: true
                            placeholderText: "Default (system font)"

                            readonly property string storedValue: page.selectedText("font", "family", "")
                            text: storedValue
                            onStoredValueChanged: if (text !== storedValue) text = storedValue
                            onEditingFinished: {
                                var family = text.trim()
                                if (family !== storedValue) {
                                    page.updateSelected("font", "family", family)
                                }
                            }
                        }

                        QQC2.ToolButton {
                            text: "Choose…"
                            icon.name: "font"
                            display: QQC2.AbstractButton.TextBesideIcon
                            visible: fontDialogLoader.status !== Loader.Error
                            onClicked: {
                                fontDialogLoader.active = true
                                if (fontDialogLoader.item) {
                                    fontDialogLoader.item.openWith(fontFamilyField.text)
                                }
                            }
                        }

                        QQC2.ToolButton {
                            text: "Clear"
                            icon.name: "edit-clear"
                            display: QQC2.AbstractButton.IconOnly
                            enabled: fontFamilyField.text.length > 0
                            onClicked: page.updateSelected("font", "family", "")

                            QQC2.ToolTip.visible: hovered
                            QQC2.ToolTip.text: "Use the system font"
                        }
                    }
                }
            }

            Flow {
                Layout.fillWidth: true
                visible: customEditor.hasTheme
                spacing: Kirigami.Units.largeSpacing

                Repeater {
                    model: [
                        { section: "font", key: "size", label: "Text size (0 = default)", from: 0, to: 30, def: 0 },
                        { section: "font", key: "headingSize", label: "Title size (0 = default)", from: 0, to: 40, def: 0 },
                        { section: "font", key: "smallSize", label: "Small text size (0 = default)", from: 0, to: 30, def: 0 },
                        { section: "metrics", key: "barHeight", label: "Bar height (px)", from: 2, to: 24, def: 6 },
                        { section: "metrics", key: "radius", label: "Corner radius (px)", from: 0, to: 24, def: 4 }
                    ]

                    delegate: ColumnLayout {
                        id: numberField
                        required property var modelData
                        spacing: 0

                        PlasmaComponents3.Label {
                            text: numberField.modelData.label
                            opacity: 0.8
                            font.pointSize: Kirigami.Theme.smallFont.pointSize
                        }

                        QQC2.SpinBox {
                            from: numberField.modelData.from
                            to: numberField.modelData.to
                            editable: true

                            // Same binding hazard as the text fields: editing
                            // the box replaces the binding, so the stored value
                            // is re-applied whenever it changes.
                            readonly property int storedValue: Math.round(
                                page.selectedNumber(numberField.modelData.section,
                                                    numberField.modelData.key,
                                                    numberField.modelData.def))
                            value: storedValue
                            onStoredValueChanged: if (value !== storedValue) value = storedValue
                            onValueModified: page.updateSelected(numberField.modelData.section,
                                                                 numberField.modelData.key, value)
                        }
                    }
                }

            }
        }

        // ---- Preview -------------------------------------------------------

        ColumnLayout {
            Layout.fillWidth: true
            Layout.topMargin: Kirigami.Units.largeSpacing
            spacing: Kirigami.Units.smallSpacing

            PlasmaComponents3.Label {
                text: "Preview — " + previewPalette.name
                opacity: 0.8
                font.pointSize: Kirigami.Theme.smallFont.pointSize
            }

            // The preview mirrors the widget: only the fill carries the
            // transparency, the content stays fully opaque on top of it. The
            // checkerboard behind it is what makes the level readable.
            Item {
                Layout.fillWidth: true
                Layout.preferredHeight: previewColumn.implicitHeight + Kirigami.Units.largeSpacing * 2
                clip: true

                Rectangle {
                    anchors.fill: parent
                    radius: previewPalette.cornerRadius
                    color: previewPalette.backgroundColor
                    opacity: previewPalette.backgroundOpacity
                }

                Rectangle {
                    anchors.fill: parent
                    radius: previewPalette.cornerRadius
                    color: "transparent"
                    border.width: 1
                    border.color: previewPalette.borderColor
                }

                ColumnLayout {
                    id: previewColumn
                    anchors.fill: parent
                    anchors.margins: Kirigami.Units.largeSpacing
                    spacing: Kirigami.Units.smallSpacing

                    PlasmaComponents3.Label {
                        text: "Usage Monitor"
                        font.bold: true
                        font.family: previewPalette.fontFamily
                        font.pointSize: previewPalette.headingFontSize
                        color: previewPalette.textColor
                    }

                    Repeater {
                        model: [
                            { "label": "Session", "percent": 42 },
                            { "label": "Weekly", "percent": 74 },
                            { "label": "Monthly", "percent": 93 }
                        ]

                        delegate: ColumnLayout {
                            id: previewRow
                            required property var modelData
                            Layout.fillWidth: true
                            spacing: 2

                            RowLayout {
                                Layout.fillWidth: true

                                PlasmaComponents3.Label {
                                    Layout.fillWidth: true
                                    text: previewRow.modelData.label
                                    font.family: previewPalette.fontFamily
                                    font.pointSize: previewPalette.fontSize
                                    color: previewPalette.textColor
                                }

                                PlasmaComponents3.Label {
                                    text: previewRow.modelData.percent + "%"
                                    font.bold: true
                                    font.family: previewPalette.fontFamily
                                    font.pointSize: previewPalette.fontSize
                                    color: previewPalette.levelColor(previewRow.modelData.percent)
                                }
                            }

                            UsageBar {
                                Layout.fillWidth: true
                                value: previewRow.modelData.percent
                                ui: previewPalette
                            }
                        }
                    }

                    PlasmaComponents3.Label {
                        text: "account@example.com · Pro plan"
                        font.family: previewPalette.fontFamily
                        font.pointSize: previewPalette.smallFontSize
                        color: previewPalette.subtextColor
                    }
                }
            }

            PlasmaComponents3.Label {
                Layout.fillWidth: true
                text: "The panel bar and the popup use this palette. \"Follow the current desktop theme\" keeps the native Plasma colors and fonts."
                wrapMode: Text.WordWrap
                opacity: 0.6
                font.pointSize: Kirigami.Theme.smallFont.pointSize
            }
        }

        Item { Layout.fillHeight: true }
    }
}
