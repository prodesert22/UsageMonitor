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

    // The desktop style paints a sunken frame here, filled with the color
    // scheme's base color. That is opaque, so it covered the popup background —
    // both the themed palette and anything the transparency slider let through.
    background: null

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
                        font.family: root.ui.fontFamily
                        font.pointSize: root.ui.cardTitleFontSize
                        color: root.ui.textColor
                        elide: Text.ElideRight
                    }

                    PlasmaComponents3.Label {
                        text: Math.round(providerCard.modelData.maxPercent || 0) + "%"
                        color: root.ui.levelColor(Number(providerCard.modelData.maxPercent || 0))
                        font.family: root.ui.fontFamily
                        font.pointSize: root.ui.fontSize
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
                    font.family: root.ui.fontFamily
                    font.pointSize: root.ui.smallFontSize
                    color: root.ui.subtextColor
                }

                PlasmaComponents3.Label {
                    visible: (providerCard.modelData.accountPlan || "").length > 0 && root.settings.showAccountEmail === false
                    Layout.fillWidth: true
                    text: providerCard.modelData.accountPlan || ""
                    elide: Text.ElideRight
                    opacity: 0.7
                    font.family: root.ui.fontFamily
                    font.pointSize: root.ui.smallFontSize
                    color: root.ui.subtextColor
                }

                PlasmaComponents3.Label {
                    visible: providerCard.modelData.stale === true
                    Layout.fillWidth: true
                    text: "Using last successful value"
                    opacity: 0.65
                    font.family: root.ui.fontFamily
                    font.pointSize: root.ui.smallFontSize
                    color: root.ui.subtextColor
                }

                PlasmaComponents3.Label {
                    visible: providerCard.modelData.error !== undefined
                    Layout.fillWidth: true
                    text: providerCard.modelData.error ? providerCard.modelData.error.message : ""
                    wrapMode: Text.WordWrap
                    font.family: root.ui.fontFamily
                    font.pointSize: root.ui.fontSize
                    color: root.ui.errorColor
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
                                font.family: root.ui.fontFamily
                                font.pointSize: root.ui.fontSize
                                color: root.ui.textColor
                            }

                            PlasmaComponents3.Label {
                                text: Math.round(modelData.percent) + "%"
                                color: root.ui.levelColor(Number(modelData.percent))
                                font.family: root.ui.fontFamily
                                font.pointSize: root.ui.fontSize
                                font.bold: true
                            }
                        }

                        UsageBar {
                            Layout.fillWidth: true
                            value: modelData.percent
                            ui: root.ui
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
                            font.family: root.ui.fontFamily
                            font.pointSize: root.ui.smallFontSize
                            color: root.ui.subtextColor
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
                        font.family: root.ui.fontFamily
                        font.pointSize: root.ui.smallFontSize
                        color: root.ui.subtextColor
                    }
                }

                Rectangle {
                    Layout.fillWidth: true
                    height: 1
                    color: root.ui.borderColor
                    opacity: root.ui.dividerOpacity
                }
            }
        }

        PlasmaComponents3.Label {
            visible: !(root.summary.providers && root.summary.providers.length) && !(root.cost.cost && root.cost.cost.length)
            Layout.fillWidth: true
            text: "No provider data yet. Enable a provider or configure credentials, then refresh."
            wrapMode: Text.WordWrap
            opacity: 0.75
            font.family: root.ui.fontFamily
            font.pointSize: root.ui.fontSize
            color: root.ui.subtextColor
        }
    }
}
