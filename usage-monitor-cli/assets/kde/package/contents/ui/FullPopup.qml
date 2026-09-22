import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.plasma.components as PlasmaComponents3

Item {
    id: popup
    implicitWidth: Kirigami.Units.gridUnit * 24
    implicitHeight: Kirigami.Units.gridUnit * 28

    // Everything that colours itself from Kirigami (busy indicator, scrollbars,
    // text areas) would otherwise use the desktop color scheme and disappear on
    // a themed background — white on a light theme, dark on a dark one. With
    // "Follow the current desktop theme" these resolve to the Plasma values, so
    // the native look is unchanged.
    Kirigami.Theme.inherit: false
    Kirigami.Theme.backgroundColor: root.ui.backgroundColor
    Kirigami.Theme.textColor: root.ui.textColor
    Kirigami.Theme.disabledTextColor: root.ui.subtextColor
    Kirigami.Theme.highlightColor: root.ui.highlightColor
    Kirigami.Theme.negativeTextColor: root.ui.errorColor
    // Only these five roles are overridden, on purpose. Re-supplying the rest
    // (hover/focus/link/visitedLink/positive/neutral/alternate/highlightedText)
    // looked harmless but made Plasma hand the scrollbar handle the *visited
    // link* colour, painting it purple inside the shell — something no harness
    // outside plasmashell reproduces. Roles left alone keep inheriting.

    // Painted for a themed palette, and for the transparency slider — which also
    // works while following the desktop theme, since backgroundColor then is the
    // inherited Kirigami/Plasma colour.
    Rectangle {
        anchors.fill: parent
        visible: root.ui.paintsBackground
        color: root.ui.backgroundColor
        opacity: root.ui.backgroundOpacity
        radius: root.ui.cornerRadius
    }

    // Outline drawn at full opacity: at a high transparency the fill alone is
    // nearly invisible against a dark screen, and the popup reads as "nothing is
    // there". The border keeps its shape without tinting the content.
    Rectangle {
        anchors.fill: parent
        visible: root.ui.paintsBackground
        color: "transparent"
        radius: root.ui.cornerRadius
        border.width: 1
        border.color: root.ui.borderColor
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Kirigami.Units.largeSpacing
        spacing: Kirigami.Units.smallSpacing

        RowLayout {
            Layout.fillWidth: true
            Layout.alignment: Qt.AlignVCenter

            Kirigami.Icon {
                source: root.monitorIcon
                Layout.preferredWidth: Kirigami.Units.iconSizes.medium
                Layout.preferredHeight: Kirigami.Units.iconSizes.medium
                Layout.alignment: Qt.AlignVCenter
            }

            ColumnLayout {
                Layout.alignment: Qt.AlignVCenter
                spacing: 0

                PlasmaComponents3.Label {
                    text: "Usage Monitor"
                    font.bold: true
                    font.family: root.ui.fontFamily
                    font.pointSize: root.ui.headingFontSize
                    color: root.ui.textColor
                }

                PlasmaComponents3.Label {
                    text: root.summary.text + (root.summary.class === "stale" ? " · cached/stale" : "")
                    opacity: 0.72
                    font.family: root.ui.fontFamily
                    font.pointSize: root.ui.smallFontSize
                    color: root.ui.subtextColor
                }
            }

            Item {
                Layout.fillWidth: true
            }

            PlasmaComponents3.BusyIndicator {
                running: root.busy
                visible: true
                opacity: root.busy ? 1 : 0
                Layout.preferredWidth: Kirigami.Units.gridUnit
                Layout.preferredHeight: Kirigami.Units.gridUnit
                Layout.alignment: Qt.AlignVCenter
            }

            ThemedToolButton {
                ui: root.ui
                icon.name: "view-refresh"
                text: "Refresh"
                Layout.alignment: Qt.AlignVCenter
                QQC2.ToolTip.visible: hovered
                QQC2.ToolTip.text: "Refresh usage data"
                onClicked: root.refresh()
            }

            ThemedToolButton {
                ui: root.ui
                icon.name: "office-chart-line"
                text: "Cost"
                Layout.alignment: Qt.AlignVCenter
                QQC2.ToolTip.visible: hovered
                QQC2.ToolTip.text: "Fetch cost / spend data"
                onClicked: root.fetchCost()
            }

            ThemedToolButton {
                ui: root.ui
                icon.name: "window-pin"
                text: "Keep open"
                Layout.alignment: Qt.AlignVCenter
                checkable: true
                checked: !root.hideOnWindowDeactivate
                QQC2.ToolTip.visible: hovered
                QQC2.ToolTip.text: checked ? "Unpin popup (auto-hide)" : "Pin popup (keep open)"
                onToggled: root.hideOnWindowDeactivate = !checked
            }

            ThemedToolButton {
                ui: root.ui
                icon.name: "configure"
                text: "Settings"
                Layout.alignment: Qt.AlignVCenter
                QQC2.ToolTip.visible: hovered
                QQC2.ToolTip.text: "Open settings"
                onClicked: root.openConfig()
            }
        }

        Rectangle {
            Layout.fillWidth: true
            height: 1
            color: root.ui.borderColor
            opacity: root.ui.themed ? 0.6 : 0.25
        }

        UpdateBanner {
            Layout.fillWidth: true
        }

        PlasmaComponents3.Label {
            visible: root.errorText.length > 0
            Layout.fillWidth: true
            text: root.errorText
            wrapMode: Text.WordWrap
            font.family: root.ui.fontFamily
            font.pointSize: root.ui.fontSize
            color: root.ui.errorColor
        }

        ColumnLayout {
            visible: root.errorDetails.length > 0
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            RowLayout {
                Layout.fillWidth: true

                PlasmaComponents3.Label {
                    Layout.fillWidth: true
                    text: "Error details"
                    font.bold: true
                    font.family: root.ui.fontFamily
                    font.pointSize: root.ui.fontSize
                    color: root.ui.textColor
                }

                PlasmaComponents3.Button {
                    text: "Copy"
                    icon.name: "edit-copy"
                    onClicked: {
                        errorDetailsArea.forceActiveFocus()
                        errorDetailsArea.selectAll()
                        errorDetailsArea.copy()
                    }
                }
            }

            QQC2.ScrollView {
                Layout.fillWidth: true
                Layout.preferredHeight: Kirigami.Units.gridUnit * 8
                clip: true

                QQC2.TextArea {
                    id: errorDetailsArea
                    text: root.errorDetails
                    readOnly: true
                    selectByMouse: true
                    wrapMode: TextEdit.NoWrap
                    font.family: "monospace"
                }
            }
        }

        UsagePage {
            Layout.fillWidth: true
            Layout.fillHeight: true
        }
    }
}
