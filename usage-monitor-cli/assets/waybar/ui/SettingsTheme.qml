import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// Plasma's configTheme page: bundled themes, installed KDE color schemes, named
// custom palettes and the transparency slider, with a live preview.
//
// Ported differences: the first mode is "Follow the desktop colors" instead of
// "Follow the current desktop theme" — outside Plasma the helper resolves those
// colors itself (kdeglobals, else the GTK/GNOME dark preference) — and the
// transparency note mentions the compositor, since a bar session without one
// simply cannot show anything through the popup.
QQC2.ScrollView {
    id: page

    property var host: null
    readonly property var ui: page.host ? page.host.ui : null
    readonly property var settings: page.host ? page.host.settings : ({})
    readonly property var catalog: page.host ? page.host.catalog : ({ "builtin": [], "schemes": [], "custom": [] })

    readonly property var modeIds: ["system", "builtin", "scheme", "custom"]
    readonly property string mode: page.host ? page.host.stateValue("themeMode", "system") : "system"
    readonly property var builtinThemes: page.catalog.builtin || []
    readonly property var installedSchemes: page.catalog.schemes || []

    // Labels only; the key list comes from the helper (`colorKeys`), so a token
    // added there shows up here instead of being silently dropped.
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

    clip: true
    background: null

    // Slider works in "transparency", state.json stores opacity.
    function themeOpacity() {
        var value = Number(page.host ? page.host.stateValue("themeOpacity", "1") : "1")
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
    // save is a single atomic write; `themeCustomId` picks the active one.

    function customList() {
        var raw = page.host ? page.host.stateValue("customThemes", "") : ""
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
        page.host.setPending("customThemes", JSON.stringify(list))
    }

    function selectedCustomId() {
        var list = page.customList()
        var wanted = page.host ? page.host.stateValue("themeCustomId", "") : ""
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
        var baseId = theme ? theme.base : (page.host ? page.host.stateValue("themeBuiltin", "macos-dark") : "macos-dark")
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
            "base": page.host.stateValue("themeBuiltin", "macos-dark"),
            "colors": ({}),
            "font": { "family": "", "size": 0, "headingSize": 0, "smallSize": 0 },
            "metrics": { "barHeight": 6, "radius": 8, "opacity": 1 }
        })
        page.storeCustomList(list)
        page.host.setPending("themeCustomId", id)
    }

    function removeSelectedTheme() {
        var id = page.selectedCustomId()
        var list = page.customListCopy().filter(function(theme) { return theme.id !== id })
        page.storeCustomList(list)
        page.host.setPending("themeCustomId", list.length > 0 ? list[0].id : "")
    }

    // Seed the selected theme's colours from its base: "start from this theme
    // and tweak it" is one click instead of eight hex values.
    function copyBaseIntoSelected() {
        var base = page.selectedBaseColors()
        var colors = ({})
        for (var key in base) {
            colors[key] = base[key]
        }
        page.updateSelected("", "colors", colors)
    }

    // Mirror of the helper's resolve_theme(), so the preview updates while the
    // window is open (the saved palette only arrives after Apply).
    function modeSpec() {
        if (page.mode === "builtin") {
            var builtin = page.themeById(page.builtinThemes, page.host.stateValue("themeBuiltin", "macos-dark"))
            if (builtin) {
                return { "mode": "builtin", "name": builtin.name, "dark": builtin.dark,
                         "colors": builtin.colors, "font": builtin.font || ({}),
                         "metrics": builtin.metrics || ({}) }
            }
        } else if (page.mode === "scheme") {
            var scheme = page.themeById(page.installedSchemes, page.host.stateValue("themeScheme", ""))
            if (scheme && scheme.colors) {
                return { "mode": "scheme", "name": scheme.name, "dark": scheme.dark,
                         "colors": scheme.colors, "font": ({}), "metrics": ({}) }
            }
        } else if (page.mode === "custom") {
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
        var system = page.catalog.system || ({})
        return { "mode": "system", "name": system.name || "Desktop colors", "dark": system.dark === true,
                 "colors": system.colors || ({}), "font": system.font || ({}), "metrics": system.metrics || ({}) }
    }

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

    ThemePalette {
        id: previewPalette
        spec: page.previewSpec()
    }

    // Loaded only when the user asks for the font chooser; if QtQuick.Dialogs is
    // missing (a common trim in minimal Qt packages) the status turns to Error
    // and the button hides itself.
    Loader {
        id: fontDialogLoader
        active: false
        source: "FontPicker.qml"
        onLoaded: item.picked.connect(function(family) {
            page.updateSelected("font", "family", family)
        })
    }

    ColumnLayout {
        width: page.availableWidth
        spacing: page.ui ? page.ui.largeSpacing : 8

        ColumnLayout {
            Layout.fillWidth: true
            Layout.leftMargin: page.ui ? page.ui.largeSpacing : 8
            Layout.rightMargin: page.ui ? page.ui.largeSpacing : 8
            Layout.topMargin: page.ui ? page.ui.largeSpacing : 8
            spacing: 0

            QQC2.Label {
                text: "Theme"
                opacity: 0.8
                font.pointSize: page.ui ? page.ui.smallFontSize : 9
                color: page.ui ? page.ui.textColor : "#f5f5f7"
            }

            QQC2.ComboBox {
                Layout.fillWidth: true
                model: ["Follow the desktop colors",
                        "Built-in theme",
                        "Installed KDE theme / color scheme",
                        "Custom"]
                currentIndex: Math.max(0, page.modeIds.indexOf(page.mode))
                onActivated: (index) => page.host.setPending("themeMode", page.modeIds[index])
            }

            QQC2.Label {
                Layout.fillWidth: true
                visible: page.mode === "system"
                text: "Resolved from this session: " + ((page.catalog.system && page.catalog.system.name) || "desktop colors")
                      + ". KDE sessions use their color scheme; elsewhere the GTK/GNOME dark preference picks a light or dark palette."
                wrapMode: Text.WordWrap
                opacity: 0.65
                font.pointSize: page.ui ? page.ui.smallFontSize : 9
                color: page.ui ? page.ui.subtextColor : "#98989d"
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.leftMargin: page.ui ? page.ui.largeSpacing : 8
            Layout.rightMargin: page.ui ? page.ui.largeSpacing : 8
            spacing: 0

            QQC2.Label {
                text: "Transparency"
                opacity: 0.8
                font.pointSize: page.ui ? page.ui.smallFontSize : 9
                color: page.ui ? page.ui.textColor : "#f5f5f7"
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: page.ui ? page.ui.largeSpacing : 8

                QQC2.Slider {
                    id: transparencySlider
                    Layout.fillWidth: true
                    from: 0
                    to: 70
                    stepSize: 5
                    snapMode: QQC2.Slider.SnapAlways

                    // Dragging assigns to `value` and destroys a plain binding,
                    // so the stored value is tracked separately and pushed in.
                    readonly property real storedValue: page.transparencyPercent()
                    value: storedValue
                    onStoredValueChanged: if (value !== storedValue) value = storedValue
                    onMoved: page.host.setPending("themeOpacity", ((100 - value) / 100).toFixed(2))
                }

                QQC2.Label {
                    Layout.preferredWidth: page.ui ? page.ui.gridUnit * 3 : 40
                    text: Math.round(transparencySlider.value) + " %"
                    horizontalAlignment: Text.AlignRight
                    color: page.ui ? page.ui.textColor : "#f5f5f7"
                }

                ThemedToolButton {
                    ui: page.ui
                    iconName: "undo"
                    text: "Reset"
                    tooltipText: "Back to a solid background"
                    enabled: transparencySlider.value > 0
                    onClicked: page.host.setPending("themeOpacity", "1")
                }
            }

            QQC2.Label {
                Layout.fillWidth: true
                text: "Only the background fades — text, icons and bars stay solid. A compositor is required for the desktop to show through: "
                      + "Wayland always has one, on X11 it needs picom/compton (without it the transparent area is painted black)."
                wrapMode: Text.WordWrap
                opacity: 0.6
                font.pointSize: page.ui ? page.ui.smallFontSize : 9
                color: page.ui ? page.ui.subtextColor : "#98989d"
            }
        }

        ColumnLayout {
            visible: page.mode === "builtin"
            Layout.fillWidth: true
            Layout.leftMargin: page.ui ? page.ui.largeSpacing : 8
            Layout.rightMargin: page.ui ? page.ui.largeSpacing : 8
            spacing: 0

            QQC2.Label {
                text: "Built-in theme"
                opacity: 0.8
                font.pointSize: page.ui ? page.ui.smallFontSize : 9
                color: page.ui ? page.ui.textColor : "#f5f5f7"
            }

            QQC2.ComboBox {
                Layout.fillWidth: true
                model: page.builtinThemes.map(function(t) { return t.name })
                currentIndex: {
                    var id = page.host.stateValue("themeBuiltin", "macos-dark")
                    for (var i = 0; i < page.builtinThemes.length; i++) {
                        if (page.builtinThemes[i].id === id) return i
                    }
                    return 0
                }
                onActivated: (index) => page.host.setPending("themeBuiltin", page.builtinThemes[index].id)
            }
        }

        ColumnLayout {
            visible: page.mode === "scheme"
            Layout.fillWidth: true
            Layout.leftMargin: page.ui ? page.ui.largeSpacing : 8
            Layout.rightMargin: page.ui ? page.ui.largeSpacing : 8
            spacing: 0

            QQC2.Label {
                text: "Installed color scheme"
                opacity: 0.8
                font.pointSize: page.ui ? page.ui.smallFontSize : 9
                color: page.ui ? page.ui.textColor : "#f5f5f7"
            }

            QQC2.ComboBox {
                Layout.fillWidth: true
                enabled: page.installedSchemes.length > 0
                model: page.installedSchemes.map(function(s) { return s.name + (s.dark ? " (dark)" : " (light)") })
                currentIndex: {
                    var id = page.host.stateValue("themeScheme", "")
                    for (var i = 0; i < page.installedSchemes.length; i++) {
                        if (page.installedSchemes[i].id === id) return i
                    }
                    return 0
                }
                onActivated: (index) => page.host.setPending("themeScheme", page.installedSchemes[index].id)
            }

            QQC2.Label {
                Layout.fillWidth: true
                text: page.installedSchemes.length > 0
                      ? "KDE color schemes and Plasma desktop themes found in the XDG data directories — no Plasma session needed, just the files."
                      : "No color schemes found. They are plain .colors files under ~/.local/share/color-schemes (installing any KDE color-scheme package adds some)."
                wrapMode: Text.WordWrap
                opacity: 0.65
                font.pointSize: page.ui ? page.ui.smallFontSize : 9
                color: page.ui ? page.ui.subtextColor : "#98989d"
            }
        }

        // ---- Custom editor -------------------------------------------------

        ColumnLayout {
            id: customEditor
            visible: page.mode === "custom"
            Layout.fillWidth: true
            Layout.leftMargin: page.ui ? page.ui.largeSpacing : 8
            Layout.rightMargin: page.ui ? page.ui.largeSpacing : 8
            spacing: page.ui ? page.ui.smallSpacing : 4

            readonly property bool hasTheme: page.selectedCustom() !== null

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 0

                QQC2.Label {
                    text: "Your themes"
                    opacity: 0.8
                    font.pointSize: page.ui ? page.ui.smallFontSize : 9
                    color: page.ui ? page.ui.textColor : "#f5f5f7"
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: page.ui ? page.ui.smallSpacing : 4

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
                        onActivated: (index) => page.host.setPending("themeCustomId", page.customList()[index].id)
                    }

                    QQC2.Button {
                        text: "New"
                        onClicked: page.addCustomTheme()

                        QQC2.ToolTip.visible: hovered
                        QQC2.ToolTip.text: "Create a theme from the current built-in base"
                    }

                    QQC2.Button {
                        text: "Delete"
                        enabled: customEditor.hasTheme
                        onClicked: page.removeSelectedTheme()

                        QQC2.ToolTip.visible: hovered
                        QQC2.ToolTip.text: "Delete the selected theme (applied on Apply)"
                    }
                }
            }

            QQC2.Label {
                Layout.fillWidth: true
                visible: !customEditor.hasTheme
                text: "No custom theme yet — click New to create one, then edit its name, colors and fonts."
                wrapMode: Text.WordWrap
                opacity: 0.7
                font.pointSize: page.ui ? page.ui.smallFontSize : 9
                color: page.ui ? page.ui.subtextColor : "#98989d"
            }

            RowLayout {
                Layout.fillWidth: true
                visible: customEditor.hasTheme
                spacing: page.ui ? page.ui.largeSpacing : 8

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0

                    QQC2.Label {
                        text: "Name"
                        opacity: 0.8
                        font.pointSize: page.ui ? page.ui.smallFontSize : 9
                        color: page.ui ? page.ui.textColor : "#f5f5f7"
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

                    QQC2.Label {
                        text: "Base theme (colours you do not override)"
                        opacity: 0.8
                        font.pointSize: page.ui ? page.ui.smallFontSize : 9
                        color: page.ui ? page.ui.textColor : "#f5f5f7"
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

                QQC2.Label {
                    Layout.fillWidth: true
                    text: "Colors — hex values such as #1c1c1e; leave empty to use the base theme."
                    wrapMode: Text.WordWrap
                    opacity: 0.7
                    font.pointSize: page.ui ? page.ui.smallFontSize : 9
                    color: page.ui ? page.ui.subtextColor : "#98989d"
                }

                QQC2.Button {
                    text: "Copy from base theme"
                    onClicked: page.copyBaseIntoSelected()
                }
            }

            GridLayout {
                Layout.fillWidth: true
                visible: customEditor.hasTheme
                columns: 2
                columnSpacing: page.ui ? page.ui.largeSpacing : 8
                rowSpacing: page.ui ? page.ui.smallSpacing : 4

                Repeater {
                    model: page.colorFields

                    delegate: RowLayout {
                        id: colorRow
                        required property var modelData
                        Layout.fillWidth: true
                        spacing: page.ui ? page.ui.smallSpacing : 4

                        // See themeNameField: typing kills the binding, so the
                        // stored value is pushed back in explicitly.
                        readonly property string currentValue: page.selectedColor(colorRow.modelData.key)
                        onCurrentValueChanged: if (colorField.text !== currentValue) colorField.text = currentValue

                        Rectangle {
                            Layout.preferredWidth: page.ui ? page.ui.gridUnit : 16
                            Layout.preferredHeight: page.ui ? page.ui.gridUnit : 16
                            radius: 3
                            border.width: 1
                            border.color: page.ui ? page.ui.borderColor : "#48484a"
                            color: {
                                var preview = previewPalette.specColors[colorRow.modelData.key]
                                return preview ? preview : "transparent"
                            }
                        }

                        QQC2.Label {
                            Layout.preferredWidth: page.ui ? page.ui.gridUnit * 7 : 110
                            text: colorRow.modelData.label
                            elide: Text.ElideRight
                            font.pointSize: page.ui ? page.ui.smallFontSize : 9
                            color: page.ui ? page.ui.textColor : "#f5f5f7"
                        }

                        QQC2.TextField {
                            id: colorField
                            Layout.fillWidth: true
                            Layout.minimumWidth: page.ui ? page.ui.gridUnit * 5 : 80
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
                Layout.topMargin: page.ui ? page.ui.smallSpacing : 4
                Layout.preferredHeight: 1
                visible: customEditor.hasTheme
                color: page.ui ? page.ui.borderColor : "#48484a"
                opacity: 0.35
            }

            ColumnLayout {
                Layout.fillWidth: true
                visible: customEditor.hasTheme
                spacing: 0

                QQC2.Label {
                    text: "Font"
                    opacity: 0.8
                    font.pointSize: page.ui ? page.ui.smallFontSize : 9
                    color: page.ui ? page.ui.textColor : "#f5f5f7"
                }

                // Deliberately a text field plus an on-demand dialog: a ComboBox
                // listing every installed family measures each entry eagerly and
                // stalls the UI on a machine with many fonts.
                RowLayout {
                    Layout.fillWidth: true
                    spacing: page.ui ? page.ui.smallSpacing : 4

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

                    QQC2.Button {
                        text: "Choose…"
                        visible: fontDialogLoader.status !== Loader.Error
                        onClicked: {
                            fontDialogLoader.active = true
                            if (fontDialogLoader.item) {
                                fontDialogLoader.item.openWith(fontFamilyField.text)
                            }
                        }
                    }

                    ThemedToolButton {
                        ui: page.ui
                        iconName: "clear"
                        text: "Clear"
                        tooltipText: "Use the system font"
                        enabled: fontFamilyField.text.length > 0
                        onClicked: page.updateSelected("font", "family", "")
                    }
                }
            }

            Flow {
                Layout.fillWidth: true
                visible: customEditor.hasTheme
                spacing: page.ui ? page.ui.largeSpacing : 8

                Repeater {
                    model: [
                        { section: "font", key: "size", label: "Text size (0 = default)", from: 0, to: 30, def: 0 },
                        { section: "font", key: "headingSize", label: "Title size (0 = default)", from: 0, to: 40, def: 0 },
                        { section: "font", key: "smallSize", label: "Small text size (0 = default)", from: 0, to: 30, def: 0 },
                        { section: "metrics", key: "barHeight", label: "Bar height (px)", from: 2, to: 24, def: 6 },
                        { section: "metrics", key: "radius", label: "Corner radius (px)", from: 0, to: 24, def: 8 }
                    ]

                    delegate: ColumnLayout {
                        id: numberField
                        required property var modelData
                        spacing: 0

                        QQC2.Label {
                            text: numberField.modelData.label
                            opacity: 0.8
                            font.pointSize: page.ui ? page.ui.smallFontSize : 9
                            color: page.ui ? page.ui.textColor : "#f5f5f7"
                        }

                        QQC2.SpinBox {
                            from: numberField.modelData.from
                            to: numberField.modelData.to
                            editable: true

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
            Layout.leftMargin: page.ui ? page.ui.largeSpacing : 8
            Layout.rightMargin: page.ui ? page.ui.largeSpacing : 8
            Layout.bottomMargin: page.ui ? page.ui.largeSpacing : 8
            spacing: page.ui ? page.ui.smallSpacing : 4

            QQC2.Label {
                text: "Preview — " + previewPalette.name
                opacity: 0.8
                font.pointSize: page.ui ? page.ui.smallFontSize : 9
                color: page.ui ? page.ui.textColor : "#f5f5f7"
            }

            Item {
                Layout.fillWidth: true
                Layout.preferredHeight: previewColumn.implicitHeight + (page.ui ? page.ui.largeSpacing : 8) * 2
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
                    anchors.margins: page.ui ? page.ui.largeSpacing : 8
                    spacing: page.ui ? page.ui.smallSpacing : 4

                    QQC2.Label {
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

                                QQC2.Label {
                                    Layout.fillWidth: true
                                    text: previewRow.modelData.label
                                    font.family: previewPalette.fontFamily
                                    font.pointSize: previewPalette.fontSize
                                    color: previewPalette.textColor
                                }

                                QQC2.Label {
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

                    QQC2.Label {
                        text: "account@example.com · Pro plan"
                        font.family: previewPalette.fontFamily
                        font.pointSize: previewPalette.smallFontSize
                        color: previewPalette.subtextColor
                    }
                }
            }

            QQC2.Label {
                Layout.fillWidth: true
                text: "The popup uses this palette. The Waybar module itself is styled by your Waybar CSS (see the class names in docs/widgets/waybar.md)."
                wrapMode: Text.WordWrap
                opacity: 0.6
                font.pointSize: page.ui ? page.ui.smallFontSize : 9
                color: page.ui ? page.ui.subtextColor : "#98989d"
            }
        }
    }
}
