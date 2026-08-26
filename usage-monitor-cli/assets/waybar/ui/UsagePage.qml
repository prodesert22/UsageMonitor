import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// Provider cards with their usage windows — the Plasma popup's UsagePage.
//
// Ported difference: everything it renders arrives through properties instead of
// reaching for the plasmoid's `root` id, because the same component is also
// instantiated from the settings preview and from tests, which have no plasmoid
// around them.
QQC2.ScrollView {
    id: usageScroll

    property var ui: null
    property var summary: ({})
    property var settings: ({})
    property var cost: ({})

    readonly property var providers: usageScroll.summary.providers || []

    clip: true
    // The default style paints an opaque frame here, which would cover the
    // popup background (and anything the transparency slider lets through).
    background: null

    // Mirrors the Python `pct_label` helper: whole numbers render without
    // decimals, everything else keeps 1 decimal place.
    function pctLabel(v) {
        v = Number(v) || 0
        return Number.isInteger(v) ? v + "%" : v.toFixed(1) + "%"
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

    function providerCostEntry(usageEntry) {
        if (!usageEntry || !usageEntry.provider) return null
        var items = usageScroll.cost.cost || []
        for (var i = 0; i < items.length; i++) {
            if (items[i].provider === usageEntry.provider) {
                var cost = items[i].last30DaysCostUSD
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

    ColumnLayout {
        width: usageScroll.availableWidth
        spacing: usageScroll.ui ? usageScroll.ui.largeSpacing : 8

        Repeater {
            model: usageScroll.providers

            delegate: ColumnLayout {
                id: providerCard
                required property var modelData
                readonly property string costText: {
                    var entry = usageScroll.providerCostEntry(providerCard.modelData)
                    return entry ? "$ " + entry : ""
                }
                readonly property string statusIndicator: {
                    var s = providerCard.modelData.status
                    return (s && s.indicator) ? s.indicator : ""
                }
                Layout.fillWidth: true
                spacing: usageScroll.ui ? usageScroll.ui.smallSpacing : 4
                clip: true

                RowLayout {
                    Layout.fillWidth: true

                    QQC2.Label {
                        Layout.fillWidth: true
                        text: providerCard.modelData.displayName || providerCard.modelData.provider || "Provider"
                        font.bold: true
                        font.family: usageScroll.ui.fontFamily
                        font.pointSize: usageScroll.ui.cardTitleFontSize
                        color: usageScroll.ui.textColor
                        elide: Text.ElideRight
                    }

                    QQC2.Label {
                        text: usageScroll.pctLabel(providerCard.modelData.maxPercent || 0)
                        color: usageScroll.ui.levelColor(Number(providerCard.modelData.maxPercent || 0))
                        font.family: usageScroll.ui.fontFamily
                        font.pointSize: usageScroll.ui.fontSize
                        font.bold: true
                    }

                    QQC2.Label {
                        visible: providerCard.statusIndicator !== ""
                        text: "●"
                        color: {
                            if (providerCard.statusIndicator === "none") return "#30d158"
                            if (providerCard.statusIndicator === "minor") return "#ff9f0a"
                            if (providerCard.statusIndicator === "major") return "#ff453a"
                            return "transparent"
                        }
                    }
                }

                QQC2.Label {
                    visible: (providerCard.modelData.accountText || "").length > 0
                             && usageScroll.settings.showAccountEmail !== false
                    Layout.fillWidth: true
                    text: providerCard.modelData.accountText || ""
                    elide: Text.ElideRight
                    opacity: 0.7
                    font.family: usageScroll.ui.fontFamily
                    font.pointSize: usageScroll.ui.smallFontSize
                    color: usageScroll.ui.subtextColor
                }

                QQC2.Label {
                    visible: (providerCard.modelData.accountPlan || "").length > 0
                             && usageScroll.settings.showAccountEmail === false
                    Layout.fillWidth: true
                    text: providerCard.modelData.accountPlan || ""
                    elide: Text.ElideRight
                    opacity: 0.7
                    font.family: usageScroll.ui.fontFamily
                    font.pointSize: usageScroll.ui.smallFontSize
                    color: usageScroll.ui.subtextColor
                }

                QQC2.Label {
                    visible: providerCard.modelData.stale === true
                    Layout.fillWidth: true
                    text: "Using last successful value"
                    opacity: 0.65
                    font.family: usageScroll.ui.fontFamily
                    font.pointSize: usageScroll.ui.smallFontSize
                    color: usageScroll.ui.subtextColor
                }

                QQC2.Label {
                    visible: providerCard.modelData.error !== undefined
                    Layout.fillWidth: true
                    text: providerCard.modelData.error ? providerCard.modelData.error.message : ""
                    wrapMode: Text.WordWrap
                    font.family: usageScroll.ui.fontFamily
                    font.pointSize: usageScroll.ui.fontSize
                    color: usageScroll.ui.errorColor
                }

                Repeater {
                    model: usageScroll.windowList(providerCard.modelData)

                    delegate: ColumnLayout {
                        required property var modelData
                        Layout.fillWidth: true
                        spacing: usageScroll.ui ? usageScroll.ui.smallSpacing / 2 : 2

                        RowLayout {
                            Layout.fillWidth: true

                            QQC2.Label {
                                Layout.fillWidth: true
                                text: modelData.label
                                font.family: usageScroll.ui.fontFamily
                                font.pointSize: usageScroll.ui.fontSize
                                color: usageScroll.ui.textColor
                            }

                            QQC2.Label {
                                text: usageScroll.pctLabel(modelData.percent)
                                color: usageScroll.ui.levelColor(Number(modelData.percent))
                                font.family: usageScroll.ui.fontFamily
                                font.pointSize: usageScroll.ui.fontSize
                                font.bold: true
                            }
                        }

                        UsageBar {
                            Layout.fillWidth: true
                            value: modelData.percent
                            ui: usageScroll.ui
                        }

                        QQC2.Label {
                            visible: modelData.reset.length > 0
                            Layout.fillWidth: true
                            text: {
                                var r = modelData.reset
                                if (r && r.indexOf("Reset") !== 0) {
                                    return "Resets: " + r
                                }
                                return r
                            }
                            opacity: 0.65
                            font.family: usageScroll.ui.fontFamily
                            font.pointSize: usageScroll.ui.smallFontSize
                            color: usageScroll.ui.subtextColor
                            elide: Text.ElideRight
                        }
                    }
                }

                RowLayout {
                    visible: providerCard.costText.length > 0
                    Layout.fillWidth: true
                    spacing: usageScroll.ui ? usageScroll.ui.smallSpacing : 4

                    QQC2.Label {
                        text: providerCard.costText
                        opacity: 0.75
                        font.family: usageScroll.ui.fontFamily
                        font.pointSize: usageScroll.ui.smallFontSize
                        color: usageScroll.ui.subtextColor
                    }
                }

                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 1
                    color: usageScroll.ui.borderColor
                    opacity: usageScroll.ui.dividerOpacity
                }
            }
        }

        QQC2.Label {
            visible: !(usageScroll.providers && usageScroll.providers.length)
                     && !(usageScroll.cost.cost && usageScroll.cost.cost.length)
            Layout.fillWidth: true
            text: "No provider data yet. Enable a provider or configure credentials, then refresh."
            wrapMode: Text.WordWrap
            opacity: 0.75
            font.family: usageScroll.ui.fontFamily
            font.pointSize: usageScroll.ui.fontSize
            color: usageScroll.ui.subtextColor
        }
    }
}
