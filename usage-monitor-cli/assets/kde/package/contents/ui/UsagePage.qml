import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.plasma.components as PlasmaComponents3

QQC2.ScrollView {
    id: usageScroll
    Layout.fillWidth: true
    Layout.fillHeight: true
    clip: true

    ColumnLayout {
        width: usageScroll.availableWidth
        spacing: Kirigami.Units.largeSpacing

        Repeater {
            model: root.summary.providers || []

            delegate: ColumnLayout {
                id: providerCard
                required property var modelData
                readonly property string costText: {
                    var entry = root.providerCostEntry(providerCard.modelData)
                    return entry ? "$ " + entry : ""
                }
                readonly property string statusIndicator: {
                    var s = providerCard.modelData.status
                    return (s && s.indicator) ? s.indicator : ""
                }
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing
                clip: true

                RowLayout {
                    Layout.fillWidth: true

                    PlasmaComponents3.Label {
                        Layout.fillWidth: true
                        text: providerCard.modelData.displayName || providerCard.modelData.provider || "Provider"
                        font.bold: true
                        font.pointSize: Kirigami.Theme.defaultFont.pointSize + 1
                        elide: Text.ElideRight
                    }

                    PlasmaComponents3.Label {
                        text: Math.round(providerCard.modelData.maxPercent || 0) + "%"
                        color: root.levelColor(Number(providerCard.modelData.maxPercent || 0))
                        font.bold: true
                    }

                    PlasmaComponents3.Label {
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

                PlasmaComponents3.Label {
                    visible: (providerCard.modelData.accountText || "").length > 0 && root.settings.showAccountEmail !== false
                    Layout.fillWidth: true
                    text: providerCard.modelData.accountText || ""
                    elide: Text.ElideRight
                    opacity: 0.7
                    font.pointSize: Kirigami.Theme.smallFont.pointSize
                }

                PlasmaComponents3.Label {
                    visible: (providerCard.modelData.accountPlan || "").length > 0 && root.settings.showAccountEmail === false
                    Layout.fillWidth: true
                    text: providerCard.modelData.accountPlan || ""
                    elide: Text.ElideRight
                    opacity: 0.7
                    font.pointSize: Kirigami.Theme.smallFont.pointSize
                }

                PlasmaComponents3.Label {
                    visible: providerCard.modelData.stale === true
                    Layout.fillWidth: true
                    text: "Using last successful value"
                    opacity: 0.65
                    font.pointSize: Kirigami.Theme.smallFont.pointSize
                }

                PlasmaComponents3.Label {
                    visible: providerCard.modelData.error !== undefined
                    Layout.fillWidth: true
                    text: providerCard.modelData.error ? providerCard.modelData.error.message : ""
                    wrapMode: Text.WordWrap
                    color: Kirigami.Theme.negativeTextColor
                }

                Repeater {
                    model: root.windowList(providerCard.modelData)

                    delegate: ColumnLayout {
                        required property var modelData
                        Layout.fillWidth: true
                        spacing: Kirigami.Units.smallSpacing / 2

                        RowLayout {
                            Layout.fillWidth: true

                            PlasmaComponents3.Label {
                                Layout.fillWidth: true
                                text: modelData.label
                            }

                            PlasmaComponents3.Label {
                                text: Math.round(modelData.percent) + "%"
                                color: root.levelColor(Number(modelData.percent))
                                font.bold: true
                            }
                        }

                        UsageBar {
                            Layout.fillWidth: true
                            value: modelData.percent
                        }

                        PlasmaComponents3.Label {
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
                            font.pointSize: Kirigami.Theme.smallFont.pointSize
                            elide: Text.ElideRight
                        }
                    }
                }

                RowLayout {
                    visible: providerCard.costText.length > 0
                    Layout.fillWidth: true
                    spacing: Kirigami.Units.smallSpacing

                    PlasmaComponents3.Label {
                        text: providerCard.costText
                        opacity: 0.75
                        font.pointSize: Kirigami.Theme.smallFont.pointSize
                    }
                }

                Rectangle {
                    Layout.fillWidth: true
                    height: 1
                    color: Kirigami.Theme.disabledTextColor
                    opacity: 0.18
                }
            }
        }

        PlasmaComponents3.Label {
            visible: !(root.summary.providers && root.summary.providers.length) && !(root.cost.cost && root.cost.cost.length)
            Layout.fillWidth: true
            text: "No provider data yet. Enable a provider or configure credentials, then refresh."
            wrapMode: Text.WordWrap
            opacity: 0.75
        }
    }
}
