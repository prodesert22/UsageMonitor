import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.plasma.components as PlasmaComponents3

Item {
    id: popup
    implicitWidth: Kirigami.Units.gridUnit * 24
    implicitHeight: Kirigami.Units.gridUnit * 28

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
                    font.pointSize: Kirigami.Theme.defaultFont.pointSize + 3
                }

                PlasmaComponents3.Label {
                    text: root.summary.text + (root.summary.class === "stale" ? " · cached/stale" : "")
                    opacity: 0.72
                    font.pointSize: Kirigami.Theme.smallFont.pointSize
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

            QQC2.ToolButton {
                icon.name: "view-refresh"
                text: "Refresh"
                display: QQC2.AbstractButton.IconOnly
                Layout.alignment: Qt.AlignVCenter
                QQC2.ToolTip.visible: hovered
                QQC2.ToolTip.text: "Refresh usage data"
                onClicked: root.refresh()
            }

            QQC2.ToolButton {
                icon.name: "office-chart-line"
                text: "Cost"
                display: QQC2.AbstractButton.IconOnly
                Layout.alignment: Qt.AlignVCenter
                QQC2.ToolTip.visible: hovered
                QQC2.ToolTip.text: "Fetch cost / spend data"
                onClicked: root.fetchCost()
            }

            QQC2.ToolButton {
                icon.name: "window-pin"
                text: "Keep open"
                display: QQC2.AbstractButton.IconOnly
                Layout.alignment: Qt.AlignVCenter
                checkable: true
                checked: !root.hideOnWindowDeactivate
                QQC2.ToolTip.visible: hovered
                QQC2.ToolTip.text: checked ? "Unpin popup (auto-hide)" : "Pin popup (keep open)"
                onToggled: root.hideOnWindowDeactivate = !checked
            }

            QQC2.ToolButton {
                icon.name: "configure"
                text: "Settings"
                display: QQC2.AbstractButton.IconOnly
                Layout.alignment: Qt.AlignVCenter
                QQC2.ToolTip.visible: hovered
                QQC2.ToolTip.text: "Open settings"
                onClicked: root.openConfig()
            }
        }

        Rectangle {
            Layout.fillWidth: true
            height: 1
            color: Kirigami.Theme.disabledTextColor
            opacity: 0.25
        }

        PlasmaComponents3.Label {
            visible: root.errorText.length > 0
            Layout.fillWidth: true
            text: root.errorText
            wrapMode: Text.WordWrap
            color: Kirigami.Theme.negativeTextColor
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
