import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.plasma.core as PlasmaCore
import org.kde.plasma.plasma5support as Plasma5Support
import org.kde.plasma.plasmoid
import org.kde.plasma.components as PlasmaComponents3

PlasmoidItem {
    id: root

    clip: true

    property var summary: ({"text": "--", "tooltip": "Usage Monitor is loading…", "class": "stale", "percentage": 0, "providers": []})
    property var cost: ({"cost": [], "updatedAt": ""})
    property string errorText: ""
    property string errorDetails: ""
    property bool busy: false
    property var settings: ({"providers": [], "pinnableProviders": [], "pinnedProvider": "", "refreshIntervalSeconds": 30, "showBarText": true, "showAccountEmail": true, "showDecimals": true, "providerOrder": "[]", "theme": ({"mode": "plasma", "colors": ({}), "font": ({}), "metrics": ({})}), "plasmoidVersion": "", "cliVersion": ""})
    property string helperPath: localFilePath(Qt.resolvedUrl("../code/usage_monitor_kde.py"))
    property string monitorIcon: Qt.resolvedUrl("../images/usage-monitor.png")

    // Shared palette for the panel bar and the popup. The summary payload
    // carries the theme too, so the bar is styled on the first refresh instead
    // of waiting for the (slower) settings payload.
    property alias ui: themePalette

    ThemePalette {
        id: themePalette
        spec: root.summary.theme || root.settings.theme || ({})
    }

    // Reload display prefs (bar text, refresh interval) when the popup opens, so
    // changes made in the native config window take effect.
    onExpandedChanged: {
        if (expanded) {
            loadSettings()
            refresh()
        }
    }

    // The config pages bump this KConfig key after writing state.json (see
    // SettingsBackend.notifyApplet), which is the only signal the applet gets
    // for settings that do not live in KConfig. Reload at once so Apply is
    // reflected immediately instead of on the next refresh tick.
    readonly property string stateRevision: Plasmoid.configuration.stateRevision || ""
    // Revisions are timestamps, so anything not newer than the last one handled
    // is ignored: Plasma's generic Apply loop can write back the value a config
    // page captured when it opened, which would otherwise reload the widget from
    // a state.json the helper has not written yet.
    property double lastStateRevision: 0
    onStateRevisionChanged: {
        var revision = Number(root.stateRevision) || 0
        if (revision <= root.lastStateRevision) {
            return
        }
        root.lastStateRevision = revision
        // `cache` re-renders the bar and popup from the last-good data with the
        // new theme/pin applied without hitting the network, so Apply lands in
        // milliseconds; live data still arrives on the next refresh tick.
        loadSettings()
        loadCache()
    }

    // The applet popup Plasma creates for us is a PlasmaWindow, whose background
    // can only be StandardBackground or SolidBackground — never "none". Painting
    // our layer at partial alpha therefore only blends with that opaque
    // background: the desktop never shows through. So when transparency is on we
    // open our own PlasmaCore.Dialog with NoBackground and paint the popup
    // ourselves; at 0 % transparency the native popup is used, unchanged.
    property Item compactItem: null
    // Set when the dialog auto-hides: the press that dismisses it deactivates
    // the window before the click reaches us, so without this a click on the
    // icon to close the popup would immediately reopen it.
    property double popupHiddenAt: 0

    function togglePopup() {
        if (root.ui.translucent) {
            root.expanded = false
            translucentPopup.active = true
            var dialog = translucentPopup.item
            if (!dialog) {
                return
            }
            if (dialog.visible) {
                dialog.visible = false
                return
            }
            if (Date.now() - root.popupHiddenAt < 300) {
                return
            }
            loadSettings()
            refresh()
            dialog.visible = true
        } else {
            root.expanded = !root.expanded
        }
    }

    Loader {
        id: translucentPopup
        active: false
        // Inline component on purpose: a separate file would not see the ids of
        // this one, and FullPopup reads `root`.
        sourceComponent: Component {
            PlasmaCore.Dialog {
                id: translucentDialog

                type: PlasmaCore.Dialog.AppletPopup
                backgroundHints: PlasmaCore.Dialog.NoBackground
                location: Plasmoid.location
                visualParent: root.compactItem
                hideOnWindowDeactivate: root.hideOnWindowDeactivate
                visible: false

                onVisibleChanged: if (!visible) root.popupHiddenAt = Date.now()

                mainItem: FullPopup {
                    width: implicitWidth
                    height: implicitHeight
                }
            }
        }
    }

    // Switching between the two popup paths closes whichever one is open, so the
    // widget never ends up with both (or a stale translucent window).
    readonly property bool translucent: root.ui.translucent
    onTranslucentChanged: {
        root.expanded = false
        if (translucentPopup.item) {
            translucentPopup.item.visible = false
        }
    }

    Plasmoid.icon: Qt.resolvedUrl("../images/usage-monitor.png")
    // With transparency on, drop the applet's own background so the widget layer
    // is what decides how see-through it looks (this covers a widget placed on
    // the desktop; the panel popup is handled by the dialog above).
    Plasmoid.backgroundHints: root.ui.translucent
        ? PlasmaCore.Types.NoBackground
        : (PlasmaCore.Types.DefaultBackground | PlasmaCore.Types.ConfigurableBackground)
    toolTipMainText: "Usage Monitor"
    toolTipSubText: summary.tooltip || errorText || "No provider data yet"
    preferredRepresentation: compactRepresentation

    function localFilePath(url) {
        var text = String(url)
        if (text.indexOf("file://") === 0) {
            return decodeURIComponent(text.substring(7))
        }
        return text
    }

    function shellQuote(path) {
        return "'" + String(path).replace(/'/g, "'\\''") + "'"
    }

    function runHelper(command) {
        busy = true
        errorText = ""
        errorDetails = ""
        executor.connectSource("python3 " + shellQuote(helperPath) + " " + command)
    }

    function refresh() {
        runHelper("summary")
    }

    function loadCache() {
        runHelper("cache")
    }

    function loadSettings() {
        runHelper("settings")
    }

    function fetchCost() {
        runHelper("cost")
    }

    function openConfig() {
        Plasmoid.internalAction("configure").trigger()
    }

    function windowList(entry) {
        if (!entry || !entry.usage) {
            return []
        }
        var result = []
        var keys = ["primary", "secondary", "tertiary"]
        var labels = {"primary": "Session", "secondary": "Weekly", "tertiary": "Monthly"}
        for (var i = 0; i < keys.length; i++) {
            var key = keys[i]
            var win = entry.usage[key]
            if (win && win.usedPercent !== undefined && win.usedPercent !== null) {
                result.push({
                    "key": key,
                    "label": labels[key],
                    "percent": Number(win.usedPercent),
                    "reset": win.resetDescription || ""
                })
            }
        }
        return result
    }

    function pinKey(entry) {
        // Mirrors pin_key_for_entry in usage_monitor_kde.py: `provider` for
        // the implicit default login, `provider/account` for named accounts.
        if (entry && entry.account) return entry.provider + "/" + entry.account
        return entry ? entry.provider : ""
    }

    function compactLabelPct() {
        var pinned = root.summary.barProvider || ""
        if (pinned) {
            var providers = root.summary.providers || []
            for (var i = 0; i < providers.length; i++) {
                if (pinKey(providers[i]) === pinned) {
                    return providers[i].maxPercent || 0
                }
            }
            // Legacy provider-level pin: first entry of that provider.
            if (pinned.indexOf("/") === -1) {
                for (var j = 0; j < providers.length; j++) {
                    if (providers[j].provider === pinned) {
                        return providers[j].maxPercent || 0
                    }
                }
            }
        }
        return root.summary.percentage || 0
    }

    function providerCostEntry(usageEntry) {
        if (!usageEntry || !usageEntry.provider) return null
        var items = root.cost.cost || []
        for (var i = 0; i < items.length; i++) {
            if (items[i].provider === usageEntry.provider) {
                var cost = items[i].last30DaysCostUSD || items[i].sessionCostUSD
                if (cost !== undefined && cost !== null) {
                    return Number(cost).toFixed(2) + " (30d)"
                }
                cost = items[i].sessionCostUSD
                if (cost !== undefined && cost !== null) {
                    return Number(cost).toFixed(2) + " (session)"
                }
                return null
            }
        }
        return null
    }

    Component.onCompleted: {
        loadSettings()
        loadCache()
        refresh()
    }

    Timer {
        interval: Math.max(10, root.settings.refreshIntervalSeconds || 30) * 1000
        repeat: true
        running: true
        triggeredOnStart: false
        onTriggered: root.refresh()
    }

    Plasma5Support.DataSource {
        id: executor
        engine: "executable"
        connectedSources: []

        onNewData: function(sourceName, data) {
            executor.disconnectSource(sourceName)
            root.busy = false

            if (data["exit code"] !== 0) {
                root.errorText = "An error occurred."
                root.errorDetails = data.stderr || data.stdout || "Usage Monitor helper failed"
                return
            }

            try {
                var payload = JSON.parse(data.stdout)
                if (sourceName.indexOf(" cost") !== -1) {
                    root.cost = payload
                } else if (sourceName.indexOf(" settings") !== -1) {
                    root.settings = payload
                } else {
                    root.summary = payload
                }
                root.errorText = ""
                root.errorDetails = ""
            } catch (e) {
                root.errorText = "Invalid JSON from Usage Monitor helper"
                root.errorDetails = String(e) + "\n\nOutput:\n" + (data.stdout || "")
            }
        }
    }

    compactRepresentation: Item {
        id: compactRoot

        // The icon scales to the panel thickness so it is never clipped on a
        // thin panel (which previously hid it entirely). Capped at smallMedium
        // so it does not balloon on a thick panel or in the desktop applet.
        readonly property int iconSize: Math.max(
            Kirigami.Units.iconSizes.small,
            Math.min(Kirigami.Units.iconSizes.smallMedium,
                     compactRoot.height - Kirigami.Units.smallSpacing * 2))

        implicitWidth: compactRow.implicitWidth + Kirigami.Units.smallSpacing * 2
        implicitHeight: Math.max(Kirigami.Units.iconSizes.smallMedium, compactLabel.implicitHeight) + Kirigami.Units.smallSpacing * 2

        // Make the panel slot track the content width at runtime. Without
        // these, Plasma reads implicitWidth once and won't shrink/grow when
        // the bar text is toggled, clipping the icon and cutting the text.
        Layout.minimumWidth: implicitWidth
        Layout.maximumWidth: implicitWidth

        clip: true

        // Themed pill behind the bar content. Without a theme (and at full
        // opacity) the panel paints its own background, so only the hover
        // highlight is drawn.
        Rectangle {
            anchors.fill: parent
            visible: root.ui.paintsBackground
            radius: root.ui.themed ? root.ui.cornerRadius : Kirigami.Units.smallSpacing
            color: root.ui.backgroundColor
            opacity: root.ui.backgroundOpacity
            border.width: root.ui.themed ? 1 : 0
            border.color: root.ui.borderColor
        }

        Rectangle {
            anchors.fill: parent
            radius: root.ui.themed ? root.ui.cornerRadius : Kirigami.Units.smallSpacing
            visible: compactMouse.containsMouse
            color: root.ui.themed ? root.ui.accentColor : Kirigami.Theme.hoverColor
            opacity: 0.25
        }

        RowLayout {
            id: compactRow
            anchors.centerIn: parent
            spacing: Kirigami.Units.smallSpacing

            Kirigami.Icon {
                source: root.monitorIcon
                Layout.alignment: Qt.AlignVCenter
                Layout.preferredWidth: compactRoot.iconSize
                Layout.preferredHeight: compactRoot.iconSize
            }

            PlasmaComponents3.Label {
                id: compactLabel
                Layout.alignment: Qt.AlignVCenter
                visible: root.settings.showBarText !== false
                text: root.summary.text || "--"
                wrapMode: Text.NoWrap
                elide: Text.ElideRight
                color: {
                    var pct = compactLabelPct()
                    return pct < 70 ? root.ui.textColor : root.ui.levelColor(pct)
                }
                font.family: root.ui.fontFamily
                font.pointSize: root.ui.fontSize
                font.bold: compactLabelPct() >= 90
            }
        }

        // The dialog anchors to the panel slot, so it has to know this item —
        // and must not keep pointing at it once Plasma recreates the compact
        // representation (form factor change, panel re-layout).
        Component.onCompleted: root.compactItem = compactRoot
        Component.onDestruction: if (root.compactItem === compactRoot) root.compactItem = null

        MouseArea {
            id: compactMouse
            anchors.fill: parent
            hoverEnabled: true
            onClicked: root.togglePopup()
        }
    }

    fullRepresentation: FullPopup { }
}
