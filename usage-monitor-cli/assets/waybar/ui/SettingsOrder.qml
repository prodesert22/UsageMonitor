import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts

// Plasma's configOrder page.
//
// Ported difference: reordering uses Move up / Move down buttons instead of
// Kirigami.ListItemDragHandle, which does not exist outside Kirigami. The stored
// value is the same `providerOrder` JSON array the Plasma widget writes, so a
// machine running both widgets sees the same ordering semantics.
Item {
    id: page

    property var host: null
    readonly property var ui: page.host ? page.host.ui : null
    readonly property var settings: page.host ? page.host.settings : ({})

    function orderedProviders() {
        var stored = []
        try {
            var pendingOrder = (page.host && page.host.pending.hasOwnProperty("providerOrder"))
                ? page.host.pending["providerOrder"]
                : (page.settings.providerOrder || "[]")
            var parsed = JSON.parse(pendingOrder || "[]")
            if (Array.isArray(parsed)) stored = parsed
        } catch (e) {
            stored = []
        }
        var enabled = (page.settings.providers || []).filter(function(p) { return p.enabled !== false })
        var seen = ({})
        var ordered = []
        for (var i = 0; i < stored.length; i++) {
            for (var j = 0; j < enabled.length; j++) {
                if (enabled[j].id === stored[i] && !seen[stored[i]]) {
                    ordered.push({ "id": stored[i], "displayName": enabled[j].displayName || stored[i] })
                    seen[stored[i]] = true
                }
            }
        }
        for (var k = 0; k < enabled.length; k++) {
            var pid = enabled[k].id || ""
            if (pid && !seen[pid]) {
                ordered.push({ "id": pid, "displayName": enabled[k].displayName || pid })
            }
        }
        return ordered
    }

    function move(from, to) {
        var list = page.orderedProviders()
        if (to < 0 || to >= list.length) {
            return
        }
        var moved = list.splice(from, 1)[0]
        list.splice(to, 0, moved)
        page.host.setPending("providerOrder", JSON.stringify(list.map(function(item) { return item.id })))
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: page.ui ? page.ui.largeSpacing : 8
        spacing: page.ui ? page.ui.smallSpacing : 4

        QQC2.Label {
            text: "Provider order"
            font.bold: true
            opacity: 0.8
            font.pointSize: page.ui ? page.ui.smallFontSize : 9
            color: page.ui ? page.ui.textColor : "#f5f5f7"
        }

        QQC2.Label {
            Layout.fillWidth: true
            text: "Reorder how providers appear in the popup. Applied with the Apply button; the popup picks it up on the next refresh."
            wrapMode: Text.WordWrap
            opacity: 0.65
            font.pointSize: page.ui ? page.ui.smallFontSize : 9
            color: page.ui ? page.ui.subtextColor : "#98989d"
        }

        ListView {
            id: orderList
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            spacing: page.ui ? Math.round(page.ui.smallSpacing / 2) : 2
            model: page.orderedProviders()

            delegate: Rectangle {
                id: orderRow
                required property var modelData
                required property int index
                width: ListView.view.width
                height: orderContent.implicitHeight + (page.ui ? page.ui.smallSpacing : 4) * 2
                radius: page.ui ? page.ui.cornerRadius : 4
                color: page.ui ? page.ui.borderColor : "#48484a"
                opacity: 0.18

                RowLayout {
                    id: orderContent
                    anchors.fill: parent
                    anchors.margins: page.ui ? page.ui.smallSpacing : 4
                    spacing: page.ui ? page.ui.smallSpacing : 4

                    GlyphIcon {
                        name: "drag"
                        color: page.ui ? page.ui.subtextColor : "#98989d"
                        implicitWidth: page.ui ? page.ui.iconSize : 16
                        implicitHeight: page.ui ? page.ui.iconSize : 16
                    }

                    QQC2.Label {
                        Layout.fillWidth: true
                        text: orderRow.modelData.displayName || orderRow.modelData.id || ""
                        elide: Text.ElideRight
                        color: page.ui ? page.ui.textColor : "#f5f5f7"
                    }

                    ThemedToolButton {
                        ui: page.ui
                        iconName: "undo"
                        text: "Move up"
                        tooltipText: "Move up"
                        enabled: orderRow.index > 0
                        rotation: 90
                        onClicked: page.move(orderRow.index, orderRow.index - 1)
                    }

                    ThemedToolButton {
                        ui: page.ui
                        iconName: "undo"
                        text: "Move down"
                        tooltipText: "Move down"
                        enabled: orderRow.index < orderList.count - 1
                        rotation: -90
                        onClicked: page.move(orderRow.index, orderRow.index + 1)
                    }
                }
            }
        }

        QQC2.Label {
            visible: orderList.count === 0
            Layout.fillWidth: true
            text: "No providers available. Enable at least one provider on the Providers page."
            wrapMode: Text.WordWrap
            opacity: 0.65
            color: page.ui ? page.ui.subtextColor : "#98989d"
        }
    }
}
