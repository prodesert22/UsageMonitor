import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
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
    property var settings: ({"providers": [], "pinnableProviders": [], "pinnedProvider": "", "refreshIntervalSeconds": 30, "showBarText": true, "showAccountEmail": true, "providerOrder": "[]", "plasmoidVersion": "", "cliVersion": ""})
    property string helperPath: localFilePath(Qt.resolvedUrl("../code/usage_monitor_kde.py"))
    property string monitorIcon: "utilities-system-monitor"

    // Reload display prefs (bar text, refresh interval) when the popup opens, so
    // changes made in the native config window take effect.
    onExpandedChanged: {
        if (expanded) {
            loadSettings()
            refresh()
        }
    }

    Plasmoid.icon: "utilities-system-monitor"
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

    function levelColor(percent) {
        if (percent >= 90) {
            return "#ff453a"
        }
        if (percent >= 70) {
            return "#ff9f0a"
        }
        return "#0a84ff"
    }

    function compactLabelPct() {
        var pinned = root.summary.barProvider || ""
        if (pinned) {
            var providers = root.summary.providers || []
            for (var i = 0; i < providers.length; i++) {
                if (providers[i].provider === pinned) {
                    return providers[i].maxPercent || 0
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
        implicitWidth: compactRow.implicitWidth + Kirigami.Units.smallSpacing * 2
        implicitHeight: Math.max(Kirigami.Units.iconSizes.smallMedium, compactLabel.implicitHeight) + Kirigami.Units.smallSpacing * 2

        // Make the panel slot track the content width at runtime. Without
        // these, Plasma reads implicitWidth once and won't shrink/grow when
        // the bar text is toggled, clipping the icon and cutting the text.
        Layout.minimumWidth: implicitWidth
        Layout.maximumWidth: implicitWidth

        clip: true

        Rectangle {
            anchors.fill: parent
            radius: Kirigami.Units.smallSpacing
            color: compactMouse.containsMouse ? Kirigami.Theme.hoverColor : "transparent"
            opacity: compactMouse.containsMouse ? 0.25 : 1
        }

        RowLayout {
            id: compactRow
            anchors.centerIn: parent
            spacing: Kirigami.Units.smallSpacing

            Kirigami.Icon {
                source: root.monitorIcon
                Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
                Layout.preferredHeight: Kirigami.Units.iconSizes.smallMedium
            }

            PlasmaComponents3.Label {
                id: compactLabel
                visible: root.settings.showBarText !== false
                text: root.summary.text || "--"
                wrapMode: Text.NoWrap
                elide: Text.ElideRight
                color: {
                    var pct = compactLabelPct()
                    return pct < 70 ? Kirigami.Theme.textColor : root.levelColor(pct)
                }
                font.bold: compactLabelPct() >= 90
            }
        }

        MouseArea {
            id: compactMouse
            anchors.fill: parent
            hoverEnabled: true
            onClicked: root.expanded = !root.expanded
        }
    }

    fullRepresentation: FullPopup { }
}
