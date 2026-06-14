import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kcmutils as KCM
import org.kde.kirigami as Kirigami
import org.kde.plasma.components as PlasmaComponents3

KCM.SimpleKCM {
    id: page

    signal configurationChanged

    function saveConfig() {
        backend.saveProviderOrder()
    }

    leftPadding: Kirigami.Units.gridUnit * 2
    rightPadding: Kirigami.Units.gridUnit * 2
    topPadding: Kirigami.Units.gridUnit
    bottomPadding: Kirigami.Units.gridUnit

    SettingsBackend {
        id: backend
    }

    ColumnLayout {
        width: parent.width
        spacing: Kirigami.Units.smallSpacing

        PlasmaComponents3.Label {
            text: "Provider order"
            font.bold: true
            opacity: 0.8
            font.pointSize: Kirigami.Theme.smallFont.pointSize
        }

        PlasmaComponents3.Label {
            Layout.fillWidth: true
            text: "Drag to reorder how providers appear in the popup and which one drives the panel. Changes take effect on next refresh."
            wrapMode: Text.WordWrap
            opacity: 0.65
            font.pointSize: Kirigami.Theme.smallFont.pointSize
        }

        ListView {
            id: providerOrderList
            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(providerOrderList.contentHeight > 0 ? providerOrderList.contentHeight : (backend.orderModel.count * Kirigami.Units.gridUnit * 2), Kirigami.Units.gridUnit * 18)
            clip: true
            interactive: backend.orderModel.count > 0
            model: backend.orderModel

            moveDisplaced: Transition {
                YAnimator {
                    duration: Kirigami.Units.longDuration
                    easing.type: Easing.InOutQuad
                }
            }

            delegate: QQC2.ItemDelegate {
                id: dragItem
                width: providerOrderList.width

                contentItem: RowLayout {
                    spacing: 0

                    Kirigami.ListItemDragHandle {
                        listItem: dragItem
                        listView: providerOrderList
                        onMoveRequested: function(oldIndex, newIndex) {
                            backend.orderModel.move(oldIndex, newIndex, 1)
                            page.configurationChanged()
                        }
                    }

                    PlasmaComponents3.Label {
                        Layout.fillWidth: true
                        text: model.displayName || model.providerId || ""
                        elide: Text.ElideRight
                    }
                }
            }
        }

        PlasmaComponents3.Label {
            visible: backend.orderModel.count === 0
            Layout.fillWidth: true
            text: "No providers available. Enable at least one provider on the Providers page."
            wrapMode: Text.WordWrap
            opacity: 0.65
        }

        Item { Layout.fillHeight: true }
    }
}
